//! Pure-Rust Port 80 HTTP Control/Recovery Server & Dead-Man Network Lease
//! Watchdog (Gates 3 & 4).
//!
//! Provides:
//! - Non-blocking HTTP request parser and response formatter for TCP Port 80
//!   on `172.22.1.15`:
//!   - `GET /status.json` (and `GET /`): Squeezelite-ESP32 compatible status
//!     JSON + Rust telemetry
//!   - `POST /recovery.json`: HTTP 200 JSON + immediate `RebootToRecovery`
//!     (`0x10000` factory)
//!   - `POST /reboot.json`: HTTP 200 JSON + `PersistOta0AndReboot`
//!     (`0x160000` `ota_0`)
//!   - `POST /reboot_ota.json`: HTTP 200 JSON + `PersistOta0AndReboot`
//!     (`0x160000` `ota_0`)
//! - `DeadManLeaseWatchdog`: 45-second boot connect/DHCP timeout and 60-second
//!   link-drop timeout.

use core::cell::RefCell;
use core::fmt::Write;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use heapless::String;

use squeezelite_cspot::crypto::hmac_sha1;
use squeezelite_cspot::crypto::OakleyGroup1Dh;
use squeezelite_cspot::zeroconf::ZeroConfServer;

use crate::ota_safety::{REQUIRED_OTA_PROJECT_NAME, RUST_OTA_FIRMWARE_VERSION};
use crate::pipeline::{
    PSRAM_HEAP_BYTES, PSRAM_OUTPUT_RING_BYTES, PSRAM_STREAM_RING_BYTES,
};

/// `psram_used_bytes` reported when no PSRAM heap statistics exist: the
/// firmware's two PSRAM rings (512 KiB stream + 2000 KiB output = 2,572,288 B).
pub const DEFAULT_PSRAM_USED_BYTES: u32 =
    (PSRAM_STREAM_RING_BYTES + PSRAM_OUTPUT_RING_BYTES) as u32;
/// `psram_free_bytes` to match [`DEFAULT_PSRAM_USED_BYTES`] in the 4 MiB
/// region (1,622,016 B).
pub const DEFAULT_PSRAM_FREE_BYTES: u32 =
    PSRAM_HEAP_BYTES as u32 - DEFAULT_PSRAM_USED_BYTES;

/// Maximum time allowed from boot to establish Wi-Fi STA link + DHCPv4 lease
/// before rolling back.
pub const BOOT_LEASE_TIMEOUT_MS: u64 = 45_000;

/// Maximum continuous time Wi-Fi link or DHCPv4 lease may remain down at
/// runtime before rolling back.
pub const DROP_LEASE_TIMEOUT_MS: u64 = 60_000;

/// Post-response action required after flushing the Port 80 HTTP response
/// socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpControlAction {
    /// Keep running normally.
    None,
    /// Trigger `esp_hal::system::software_reset()` to boot into `recovery`
    /// (`0x10000`), relying on Gate 1 (`otadata` pre-erased to `0xFF`).
    RebootToRecovery,
    /// Write `OtaSelectEntry::new_ota_0()` to `0xD000` and trigger
    /// `software_reset()` so the next boot stays in `ota_0` (`0x160000`).
    PersistOta0AndReboot,
    /// Transmit SlimProto IR packet to LMS and/or adjust amplifier volume/mute.
    PlayerIrCommand {
        ir_code: u32,
        vol_pct: Option<u8>,
        mute: Option<bool>,
    },
    /// Sets the active 10-band equalizer preset (`0 = Flat`, `1 = Bass`,
    /// `2 = Loudness`, `3 = Vocal`, `4 = Rock`).
    SetEqPreset(u8),
    /// Sets TAS5805M analog gain attenuation (`0x00 = 0 dB`, `0x08 = -4 dB`,
    /// `0x0C = -6 dB`, `0x10 = -8 dB`).
    SetAnalogGain(u8),
    /// Starts Spotify playback arbitration and streaming.
    SpotifyPlay,
    /// Stops Spotify playback arbitration and returns to suspended source.
    SpotifyStop,
    /// Triggers an asynchronous 802.11 Wi-Fi AP scan (`/scan.json`).
    TriggerWifiScan,
    /// Initiates Wi-Fi station association with new credentials (`/connect.json`).
    ConnectWifi,
    /// Applies a key-value configuration mutation (`/config.json`).
    UpdateConfig,
    /// Dispatches a console CLI command (`/commands.json`).
    ExecuteCliCommand,
    /// Initiates streaming OTA firmware update (`/flash.json`).
    StartOtaFlash,
}

/// Amp I2C NACK count per operation class (spec §4.8): the bus wrapper still
/// swallows I2C errors (shadow-register fallback), so these counters keep
/// faults from being fully silent. A NACKed `write_read` retries as a `write`
/// + `read`, which count in their own classes too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct I2cNackByOp {
    /// NACKed plain reads.
    pub read: u32,
    /// NACKed plain writes.
    pub write: u32,
    /// NACKed combined write-then-read transfers.
    pub write_read: u32,
}

/// Live telemetry snapshot serialized into `GET /status.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpStatusSnapshot<'a> {
    /// Assigned IPv4 address (`[172, 22, 1, 15]`).
    pub ip: [u8; 4],
    /// Assigned IPv4 netmask (`[255, 255, 255, 0]`).
    pub netmask: [u8; 4],
    /// Assigned IPv4 gateway (`[172, 22, 1, 1]`).
    pub gw: [u8; 4],
    /// Connected Wi-Fi SSID (`"iot"`).
    pub ssid: &'a str,
    /// Current Wi-Fi RSSI in dBm.
    pub rssi: i32,
    /// Target Lyrion Music Server IPv4 (`[10, 0, 0, 10]`).
    pub lms_ip: [u8; 4],
    /// LMS HTTP port (`9000`).
    pub lms_port: u16,
    /// LMS CLI port (`9090`).
    pub lms_cport: u16,
    /// Whether `otadata` (`0xD000..0xF000`) is pre-armed to `0xFF` (`Factory`
    /// recovery on reset).
    pub otadata_prearmed_factory: bool,
    /// Total audio frames rendered via `I2S0` DMA.
    pub frames_played: u64,
    /// Current TAS5805M amplifier mode label (`"Play"`, `"HiZ"`, `"DeepSleep"`,
    /// `"Fault"`).
    pub amp_mode: &'a str,
    /// Current TAS5805M amplifier volume percentage (`0..=100%`).
    pub vol_pct: u8,
    /// Total hardware `I2C0` ACK transactions to `0x2D`.
    pub i2c_acks: u32,
    /// Total hardware `I2C0` NACK errors.
    pub i2c_nacks: u32,
    /// `I2C0` NACK errors per operation class
    /// (`"i2c_nack_by_op":{"read":..,"write":..,"write_read":..}`).
    pub i2c_nack_by_op: I2cNackByOp,
    /// Monotonic uptime in milliseconds since boot.
    pub uptime_ms: u64,
    /// Free PSRAM in external Quad-SPI RAM in bytes.
    pub psram_free_bytes: u32,
    /// Used PSRAM in external Quad-SPI RAM in bytes.
    pub psram_used_bytes: u32,
    /// Whether Core 1 (`APP_CPU`) audio/watchdog task is active.
    pub core1_active: bool,
    /// Core 1 monotonic loop execution counter.
    pub core1_loops: u32,
    /// Active decoder codec name (`"PCM"`, `"FLAC"`, `"MP3"`, etc.).
    pub codec: &'a str,
    /// Active decoder audio sample rate in Hz.
    pub codec_sample_rate: u32,
    /// Active decoder channel count (1 = mono, 2 = stereo).
    pub codec_channels: u8,
    /// Active decoder sample bit depth (16, 24, 32).
    pub codec_bit_depth: u8,
    /// Active 10-band equalizer preset name (`"Flat"`, `"Bass"`,
    /// `"Loudness"`, `"Vocal"`, `"Rock"`).
    pub eq_preset: &'a str,
    /// Whether 10-band graphic equalizer is actively filtering samples.
    pub eq_enabled: bool,
    /// Detected hardware I2S sampling frequency in Hz from TAS5805M register 0x37.
    pub tas_fs_hz: u32,
    /// Detected hardware BCK-to-FS ratio from TAS5805M register 0x38.
    pub tas_bck_ratio: u16,
    /// Whether TAS5805M internal PLL clock synthesizer is locked
    /// (register 0x39 bit 3).
    pub tas_pll_locked: bool,
    /// Whether TAS5805M clock fault auto-mute is active (register 0x39 bit 0).
    pub tas_automuted: bool,
    /// Hardware analog gain code (`0x54`, bits 4:0; `0x00` = 0 dB,
    /// `0x08` = -4 dB, `0x0C` = -6 dB, `0x10` = -8 dB).
    pub again_code: u8,
    /// NVS FlashJournal monotonic sequence number.
    pub settings_seq: u32,
    /// Whether Bluetooth Classic A2DP sink subsystem is enabled and initialized.
    pub bt_enabled: bool,
    /// Whether Bluetooth Classic connection is active with peer device.
    pub bt_connected: bool,
    /// Whether Bluetooth A2DP audio stream is currently streaming SBC frames.
    pub bt_streaming: bool,
    /// Total received HCI packets from Bluetooth controller.
    pub bt_rx_pkts: u32,
    /// Total decoded audio frames from Bluetooth A2DP stream.
    pub bt_frames: u64,
    /// Maximum execution duration of a single `decode()` step in microseconds.
    pub decode_us_max: u32,
    /// Execution duration of the very first `decode()` step in microseconds.
    pub decode_us_first: u32,
}

/// Type alias for runtime status snapshot.
pub type RuntimeStatusSnapshot<'a> = HttpStatusSnapshot<'a>;

impl<'a> Default for HttpStatusSnapshot<'a> {
    fn default() -> Self {
        Self {
            ip: [172, 22, 1, 15],
            netmask: [255, 255, 255, 0],
            gw: [172, 22, 1, 1],
            ssid: "iot",
            rssi: -50,
            lms_ip: [10, 0, 0, 10],
            lms_port: 9000,
            lms_cport: 9090,
            otadata_prearmed_factory: true,
            frames_played: 0,
            amp_mode: "HiZ",
            vol_pct: 55,
            i2c_acks: 0,
            i2c_nacks: 0,
            i2c_nack_by_op: I2cNackByOp::default(),
            uptime_ms: 0,
            psram_free_bytes: DEFAULT_PSRAM_FREE_BYTES,
            psram_used_bytes: DEFAULT_PSRAM_USED_BYTES,
            core1_active: true,
            core1_loops: 0,
            codec: "PCM",
            codec_sample_rate: 44_100,
            codec_channels: 2,
            codec_bit_depth: 16,
            eq_preset: "Flat",
            eq_enabled: false,
            tas_fs_hz: 44_100,
            tas_bck_ratio: 64,
            tas_pll_locked: true,
            tas_automuted: false,
            again_code: 0x08,
            settings_seq: 1,
            bt_enabled: false,
            bt_connected: false,
            bt_streaming: false,
            bt_rx_pkts: 0,
            bt_frames: 0,
            decode_us_max: 0,
            decode_us_first: 0,
        }
    }
}

struct BufferWriter<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> BufferWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    fn written_len(&self) -> usize {
        self.offset
    }
}

impl<'a> core::fmt::Write for BufferWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.offset);
        if bytes.len() > remaining {
            return Err(core::fmt::Error);
        }
        self.buf[self.offset..self.offset + bytes.len()].copy_from_slice(bytes);
        self.offset += bytes.len();
        Ok(())
    }
}

/// Writes a string slice escaped for JSON output (escaping `"`, `\`, and
/// ASCII control chars < 0x20).
pub fn write_json_escaped_str<W: core::fmt::Write>(mut writer: W, s: &str) -> core::fmt::Result {
    for b in s.bytes() {
        match b {
            b'"' => writer.write_str("\\\"")?,
            b'\\' => writer.write_str("\\\\")?,
            b'\n' => writer.write_str("\\n")?,
            b'\r' => writer.write_str("\\r")?,
            b'\t' => writer.write_str("\\t")?,
            0x00..=0x1F => {
                let hex_digits = b"0123456789abcdef";
                let hi = hex_digits[(b >> 4) as usize] as char;
                let lo = hex_digits[(b & 0x0F) as usize] as char;
                writer.write_str("\\u00")?;
                writer.write_char(hi)?;
                writer.write_char(lo)?;
            }
            _ => writer.write_char(b as char)?,
        }
    }
    Ok(())
}

/// Helper wrapper struct implementing Display to format JSON-escaped strings
/// inline.
pub struct JsonEscaped<'a>(pub &'a str);

impl<'a> core::fmt::Display for JsonEscaped<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_json_escaped_str(f, self.0)
    }
}

/// Formats the `/status.json` payload into `out_buf` (up to 2048 bytes).
/// Returns the number of bytes written, or `None` if `out_buf` is too small.
pub fn write_status_json(snapshot: &HttpStatusSnapshot<'_>, out_buf: &mut [u8]) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    write!(
        writer,
        "{{\"project_name\":\"{}\",\"platform_name\":\"ESP32\",\"version\":\"{}\",\
        \"recovery\":0,\"rust_engine\":true,\"otadata_prearmed_factory\":{},\
        \"if\":\"wifi\",\"ip\":\"{}.{}.{}.{}\",\"netmask\":\"{}.{}.{}.{}\",\
        \"gw\":\"{}.{}.{}.{}\",\"ssid\":\"{}\",\"rssi\":{},\"lms_cport\":{},\
        \"lms_port\":{},\"lms_ip\":\"{}.{}.{}.{}\",\"frames_played\":{},\
        \"amp_mode\":\"{}\",\"vol_pct\":{},\"i2c_acks\":{},\"i2c_nacks\":{},\
        \"i2c_nack_by_op\":{{\"read\":{},\"write\":{},\"write_read\":{}}},\
        \"uptime_ms\":{},\"psram_free_bytes\":{},\"psram_used_bytes\":{},\
        \"core1_active\":{},\"core1_loops\":{},\"codec\":\"{}\",\
        \"codec_sample_rate\":{},\"codec_channels\":{},\"codec_bit_depth\":{},\
        \"eq_preset\":\"{}\",\"eq_enabled\":{},\"tas_fs_hz\":{},\
        \"tas_bck_ratio\":{},\"tas_pll_locked\":{},\"tas_automuted\":{},\
        \"again_code\":{},\"settings_seq\":{},\"bt_enabled\":{},\
        \"bt_connected\":{},\"bt_streaming\":{},\"bt_rx_pkts\":{},\
        \"bt_frames\":{},\"decode_us_max\":{},\"decode_us_first\":{}}}",
        JsonEscaped(REQUIRED_OTA_PROJECT_NAME),
        JsonEscaped(RUST_OTA_FIRMWARE_VERSION),
        snapshot.otadata_prearmed_factory,
        snapshot.ip[0],
        snapshot.ip[1],
        snapshot.ip[2],
        snapshot.ip[3],
        snapshot.netmask[0],
        snapshot.netmask[1],
        snapshot.netmask[2],
        snapshot.netmask[3],
        snapshot.gw[0],
        snapshot.gw[1],
        snapshot.gw[2],
        snapshot.gw[3],
        JsonEscaped(snapshot.ssid),
        snapshot.rssi,
        snapshot.lms_cport,
        snapshot.lms_port,
        snapshot.lms_ip[0],
        snapshot.lms_ip[1],
        snapshot.lms_ip[2],
        snapshot.lms_ip[3],
        snapshot.frames_played,
        JsonEscaped(snapshot.amp_mode),
        snapshot.vol_pct,
        snapshot.i2c_acks,
        snapshot.i2c_nacks,
        snapshot.i2c_nack_by_op.read,
        snapshot.i2c_nack_by_op.write,
        snapshot.i2c_nack_by_op.write_read,
        snapshot.uptime_ms,
        snapshot.psram_free_bytes,
        snapshot.psram_used_bytes,
        snapshot.core1_active,
        snapshot.core1_loops,
        JsonEscaped(snapshot.codec),
        snapshot.codec_sample_rate,
        snapshot.codec_channels,
        snapshot.codec_bit_depth,
        JsonEscaped(snapshot.eq_preset),
        snapshot.eq_enabled,
        snapshot.tas_fs_hz,
        snapshot.tas_bck_ratio,
        snapshot.tas_pll_locked,
        snapshot.tas_automuted,
        snapshot.again_code,
        snapshot.settings_seq,
        snapshot.bt_enabled,
        snapshot.bt_connected,
        snapshot.bt_streaming,
        snapshot.bt_rx_pkts,
        snapshot.bt_frames,
        snapshot.decode_us_max,
        snapshot.decode_us_first,
    ).ok()?;
    Some(writer.written_len())
}

/// Formats the `/status.json` payload into `out_buf` (up to 2048 bytes).
#[inline]
pub fn format_status_json(snapshot: &HttpStatusSnapshot<'_>, out_buf: &mut [u8]) -> Option<usize> {
    write_status_json(snapshot, out_buf)
}

/// View of a single NVS / FlashJournal configuration entry serialized in
/// `GET /config.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigEntryView<'a> {
    /// Configuration key name (e.g. `"autoexec"`, `"dac_config"`).
    pub key: &'a str,
    /// Current serialized string value.
    pub value: &'a str,
    /// Value type label (`"str"`, `"u8"`, `"u32"`).
    pub entry_type: &'a str,
}

impl<'a> ConfigEntryView<'a> {
    /// Creates a new [`ConfigEntryView`].
    #[must_use]
    pub const fn new(key: &'a str, value: &'a str, entry_type: &'a str) -> Self {
        Self {
            key,
            value,
            entry_type,
        }
    }
}

/// Parsed key-value mutation from `POST /config.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedConfigUpdate<'a> {
    /// Target configuration key.
    pub key: &'a str,
    /// New configuration value.
    pub value: &'a str,
}

/// Formats the `/config.json` payload combining active snapshot settings and
/// optional `extra_entries` into `out_buf`.
pub fn write_config_json(
    snapshot: &HttpStatusSnapshot<'_>,
    extra_entries: &[ConfigEntryView<'_>],
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    write!(
        writer,
        "{{\"settings_seq\":{},\"config\":{{\"ssid\":\"{}\",\
         \"lms_ip\":\"{}.{}.{}.{}\",\"lms_port\":{},\"lms_cport\":{},\
         \"vol_pct\":{},\"eq_preset\":\"{}\",\"again_code\":{}",
        snapshot.settings_seq,
        JsonEscaped(snapshot.ssid),
        snapshot.lms_ip[0],
        snapshot.lms_ip[1],
        snapshot.lms_ip[2],
        snapshot.lms_ip[3],
        snapshot.lms_port,
        snapshot.lms_cport,
        snapshot.vol_pct,
        JsonEscaped(snapshot.eq_preset),
        snapshot.again_code,
    )
    .ok()?;
    for entry in extra_entries {
        write!(
            writer,
            ",\"{}\":{{\"value\":\"{}\",\"type\":\"{}\"}}",
            JsonEscaped(entry.key),
            JsonEscaped(entry.value),
            JsonEscaped(entry.entry_type),
        )
        .ok()?;
    }
    write!(writer, "}}}}").ok()?;
    Some(writer.written_len())
}

/// Decodes a percent-encoded (`%XX` and `+`) URL string into `dst`, returning
/// the number of bytes written.
pub fn percent_decode_into(src: &str, dst: &mut [u8]) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut w = 0;
    let mut i = 0;
    while i < bytes.len() {
        if w >= dst.len() {
            return None;
        }
        match bytes[i] {
            b'+' => {
                dst[w] = b' ';
                w += 1;
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h1 = match bytes[i + 1] {
                    b'0'..=b'9' => bytes[i + 1] - b'0',
                    b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                    _ => return None,
                };
                let h2 = match bytes[i + 2] {
                    b'0'..=b'9' => bytes[i + 2] - b'0',
                    b'a'..=b'f' => bytes[i + 2] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 2] - b'A' + 10,
                    _ => return None,
                };
                dst[w] = (h1 << 4) | h2;
                w += 1;
                i += 3;
            }
            b'%' => return None,
            b => {
                dst[w] = b;
                w += 1;
                i += 1;
            }
        }
    }
    Some(w)
}

/// Decodes a percent-encoded URL string into a fixed-capacity heapless
/// `String<N>`.
pub fn percent_decode_str<const N: usize>(src: &str) -> Option<String<N>> {
    let mut buf = [0u8; N];
    let len = percent_decode_into(src, &mut buf)?;
    let s = core::str::from_utf8(&buf[..len]).ok()?;
    let mut out = String::new();
    out.push_str(s).ok()?;
    Some(out)
}

fn extract_param_or_json_field<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    for part in input.split('&') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(name) {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                return Some(val.trim());
            }
        }
    }
    // Minimal JSON string field fallback with optional whitespace around `:`
    // e.g. `"name":"value"`, `"name": "value"`, `"name" : "value"`
    let mut search_from = 0;
    while let Some(idx) = input[search_from..].find('"') {
        let key_start = search_from + idx + 1;
        if input[key_start..].starts_with(name) {
            let after_name = key_start + name.len();
            if input[after_name..].starts_with('"') {
                let rest = &input[after_name + 1..];
                let rest_trimmed = rest.trim_start();
                if let Some(after_colon) = rest_trimmed.strip_prefix(':') {
                    let after_colon_trimmed = after_colon.trim_start();
                    if let Some(after_quote) = after_colon_trimmed.strip_prefix('"') {
                        let end = after_quote.find('"')?;
                        return Some(&after_quote[..end]);
                    }
                }
            }
        }
        search_from = key_start;
    }
    None
}

/// Parses a `key=...&value=...` query/form or `{"key":"...","value":"..."}`
/// JSON body for `/config.json`.
#[must_use]
pub fn parse_config_update_query(input: &str) -> Option<ParsedConfigUpdate<'_>> {
    let key = extract_param_or_json_field(input, "key")?;
    if key.is_empty() {
        return None;
    }
    let value = extract_param_or_json_field(input, "value")
        .or_else(|| extract_param_or_json_field(input, "val"))
        .unwrap_or("");
    Some(ParsedConfigUpdate { key, value })
}

/// View of a single scanned 802.11 access point serialized in `GET /scan.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanApView<'a> {
    /// Broadcast SSID.
    pub ssid: &'a str,
    /// 6-byte BSSID MAC address.
    pub bssid: [u8; 6],
    /// Signal strength in dBm.
    pub rssi: i8,
    /// 2.4 GHz channel (`1..=14`).
    pub channel: u8,
    /// Authentication mode label (`"OPEN"`, `"WPA2_PSK"`, etc.).
    pub auth: &'a str,
}

impl<'a> ScanApView<'a> {
    /// Creates a new [`ScanApView`].
    #[must_use]
    pub const fn new(
        ssid: &'a str,
        bssid: [u8; 6],
        rssi: i8,
        channel: u8,
        auth: &'a str,
    ) -> Self {
        Self {
            ssid,
            bssid,
            rssi,
            channel,
            auth,
        }
    }
}

/// Formats a slice of [`ScanApView`] entries as the `/scan.json` JSON array.
pub fn write_scan_json(aps: &[ScanApView<'_>], out_buf: &mut [u8]) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"aps\":[").ok()?;
    for (idx, ap) in aps.iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"ssid\":\"{}\",\"bssid\":\"{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\",\
             \"rssi\":{},\"chan\":{},\"auth\":\"{}\"}}",
            JsonEscaped(ap.ssid),
            ap.bssid[0],
            ap.bssid[1],
            ap.bssid[2],
            ap.bssid[3],
            ap.bssid[4],
            ap.bssid[5],
            ap.rssi,
            ap.channel,
            JsonEscaped(ap.auth),
        )
        .ok()?;
    }
    writer.write_str("]}").ok()?;
    Some(writer.written_len())
}

/// Formats a [`crate::wifi::WifiScanTable`] as a `/scan.json` JSON array into
/// `out_buf`.
pub fn write_scan_table_json(
    table: &crate::wifi::WifiScanTable,
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"aps\":[").ok()?;
    for (idx, ap) in table.entries().iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"ssid\":\"{}\",\"bssid\":\"{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\",\
             \"rssi\":{},\"chan\":{},\"auth\":\"{}\"}}",
            JsonEscaped(ap.ssid.as_str()),
            ap.bssid[0],
            ap.bssid[1],
            ap.bssid[2],
            ap.bssid[3],
            ap.bssid[4],
            ap.bssid[5],
            ap.rssi,
            ap.channel,
            JsonEscaped(ap.auth.as_str()),
        )
        .ok()?;
    }
    writer.write_str("]}").ok()?;
    Some(writer.written_len())
}

/// Parsed Wi-Fi connection request from `POST /connect.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectRequest<'a> {
    /// Target Wi-Fi SSID.
    pub ssid: &'a str,
    /// WPA/WPA2 passphrase (or empty for open AP).
    pub password: &'a str,
    /// Optional static IPv4 string.
    pub static_ip: Option<&'a str>,
}

/// Parses a `/connect.json` form or JSON request body/query.
#[must_use]
pub fn parse_connect_request(input: &str) -> Option<ConnectRequest<'_>> {
    let ssid = extract_param_or_json_field(input, "ssid")?;
    if ssid.is_empty() {
        return None;
    }
    let password = extract_param_or_json_field(input, "password")
        .or_else(|| extract_param_or_json_field(input, "pass"))
        .or_else(|| extract_param_or_json_field(input, "pwd"))
        .unwrap_or("");
    let static_ip = extract_param_or_json_field(input, "ip");
    Some(ConnectRequest {
        ssid,
        password,
        static_ip,
    })
}

/// Formats the `/connect.json` Wi-Fi association status payload into `out_buf`.
pub fn write_connect_status_json(
    connected: bool,
    ssid: &str,
    ip: [u8; 4],
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    let status_label = if connected { "connected" } else { "disconnected" };
    write!(
        writer,
        "{{\"connected\":{},\"status\":\"{}\",\"ssid\":\"{}\",\
         \"ip\":\"{}.{}.{}.{}\"}}",
        connected,
        status_label,
        JsonEscaped(ssid),
        ip[0],
        ip[1],
        ip[2],
        ip[3],
    )
    .ok()?;
    Some(writer.written_len())
}

/// View of a single syslog / console ringbuffer message for
/// `GET /messages.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyslogMessageView<'a> {
    /// Monotonic message sequence number.
    pub seq: u32,
    /// Uptime timestamp in milliseconds.
    pub uptime_ms: u64,
    /// Severity label (`"INFO"`, `"WARN"`, `"ERROR"`, `"DEBUG"`).
    pub level: &'a str,
    /// Log message text.
    pub message: &'a str,
}

impl<'a> SyslogMessageView<'a> {
    /// Creates a new [`SyslogMessageView`].
    #[must_use]
    pub const fn new(seq: u32, uptime_ms: u64, level: &'a str, message: &'a str) -> Self {
        Self {
            seq,
            uptime_ms,
            level,
            message,
        }
    }
}

/// Formats a slice of [`SyslogMessageView`] entries into `out_buf` for
/// `GET /messages.json`.
pub fn write_messages_json(
    messages: &[SyslogMessageView<'_>],
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"messages\":[").ok()?;
    for (idx, msg) in messages.iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"seq\":{},\"uptime_ms\":{},\"level\":\"{}\",\"msg\":\"{}\"}}",
            msg.seq,
            msg.uptime_ms,
            JsonEscaped(msg.level),
            JsonEscaped(msg.message),
        )
        .ok()?;
    }
    writer.write_str("]}").ok()?;
    Some(writer.written_len())
}

/// Formats a [`crate::console::SyslogRingBuffer`] into `out_buf` for
/// `GET /messages.json`.
pub fn write_syslog_ring_json<const N: usize>(
    ring: &crate::console::SyslogRingBuffer<N>,
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"messages\":[").ok()?;
    for idx in 0..ring.len() {
        let Some(msg) = ring.entry(idx) else {
            continue;
        };
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"seq\":{},\"uptime_ms\":{},\"level\":\"{}\",\"msg\":\"{}\"}}",
            msg.seq,
            msg.uptime_ms,
            JsonEscaped(msg.level.as_str()),
            JsonEscaped(msg.text.as_str()),
        )
        .ok()?;
    }
    writer.write_str("]}").ok()?;
    Some(writer.written_len())
}

/// Descriptor for a registered console CLI command exposed via
/// `/commands.json`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandCatalogEntry<'a> {
    /// Command verb (e.g. `"system"`, `"i2c"`).
    pub name: &'a str,
    /// Argument synopsis.
    pub usage: &'a str,
    /// Short human-readable summary.
    pub description: &'a str,
}

impl<'a> CommandCatalogEntry<'a> {
    /// Creates a new [`CommandCatalogEntry`].
    #[must_use]
    pub const fn new(name: &'a str, usage: &'a str, description: &'a str) -> Self {
        Self {
            name,
            usage,
            description,
        }
    }
}

/// Default built-in CLI command catalog serialized by `GET /commands.json`.
pub const DEFAULT_COMMAND_CATALOG: &[CommandCatalogEntry<'static>] = &[
    CommandCatalogEntry::new(
        "system",
        "system <info|heap|reboot|recovery>",
        "System diagnostics and reboot",
    ),
    CommandCatalogEntry::new("i2c", "i2c <scan|get|set>", "Hardware I2C bus tools"),
    CommandCatalogEntry::new(
        "config",
        "config <get|set|erase|dump>",
        "NVS / FlashJournal settings",
    ),
    CommandCatalogEntry::new("join", "join <ssid> [password]", "Associate with Wi-Fi AP"),
    CommandCatalogEntry::new("update", "update <url>", "Trigger streaming OTA update"),
    CommandCatalogEntry::new(
        "player",
        "player <play|pause|stop|vol>",
        "Audio transport and volume",
    ),
    CommandCatalogEntry::new("sleep", "sleep [light|deep] <seconds>", "Enter low-power sleep mode"),
    CommandCatalogEntry::new("help", "help", "List available console commands"),
];

/// Formats the `/commands.json` CLI catalog and optional execution output into
/// `out_buf`.
pub fn write_commands_json(
    catalog: &[CommandCatalogEntry<'_>],
    last_output: Option<&str>,
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"commands\":[").ok()?;
    for (idx, cmd) in catalog.iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"name\":\"{}\",\"usage\":\"{}\",\"desc\":\"{}\"}}",
            JsonEscaped(cmd.name),
            JsonEscaped(cmd.usage),
            JsonEscaped(cmd.description),
        )
        .ok()?;
    }
    writer.write_str("]").ok()?;
    if let Some(out) = last_output {
        write!(writer, ",\"output\":\"{}\"", JsonEscaped(out)).ok()?;
    }
    writer.write_str("}").ok()?;
    Some(writer.written_len())
}

/// Formats the `/flash.json` OTA status payload into `out_buf`.
pub fn write_flash_status_json(
    state_label: &str,
    progress_pct: u8,
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    write!(
        writer,
        "{{\"ok\":true,\"ota_state\":\"{}\",\"progress_pct\":{},\
         \"target_partition\":\"ota_0_0x160000\"}}",
        JsonEscaped(state_label),
        progress_pct,
    )
    .ok()?;
    Some(writer.written_len())
}

fn extract_query_or_body<'a>(raw_path: &'a str, req_bytes: &'a [u8]) -> &'a str {
    let query = raw_path.split_once('?').map(|(_, q)| q).unwrap_or("");
    if !query.is_empty() {
        return query;
    }
    let body_slice = req_bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| &req_bytes[p + 4..])
        .or_else(|| {
            req_bytes
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|p| &req_bytes[p + 2..])
        })
        .unwrap_or(&[]);
    core::str::from_utf8(body_slice).unwrap_or("").trim()
}

/// Parses a query parameter and decodes `+` as space (`b' '`).
pub fn parse_query_string<const N: usize>(query: &str, param_name: &str) -> Option<String<N>> {
    for part in query.split('&') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(param_name) {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                return percent_decode_str::<N>(val.trim());
            }
        }
    }
    None
}

/// Formats a complete HTTP response including standard security headers:
/// `X-Content-Type-Options: nosniff` and `X-Frame-Options: DENY`.
pub fn write_http_response(
    status_code: u16,
    content_type: &str,
    body: &[u8],
    out_buf: &mut [u8],
) -> Option<usize> {
    const HEADER_ROOM: usize = 256;
    if HEADER_ROOM.saturating_add(body.len()) > out_buf.len() {
        return None;
    }
    let mut header: String<HEADER_ROOM> = String::new();
    write!(
        header,
        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\n\
        X-Content-Type-Options: nosniff\r\n\
        X-Frame-Options: DENY\r\n\
        Access-Control-Allow-Origin: *\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\
        Cache-Control: no-store\r\n\r\n",
        status_code,
        content_type,
        body.len()
    )
    .ok()?;
    let header_len = header.len();
    out_buf[..header_len].copy_from_slice(header.as_bytes());
    out_buf[header_len..header_len + body.len()].copy_from_slice(body);
    Some(header_len + body.len())
}

fn write_json_http_response(body_len: usize, out_buf: &mut [u8]) -> Option<usize> {
    const HEADER_ROOM: usize = 160;
    if HEADER_ROOM.saturating_add(body_len) > out_buf.len() {
        return None;
    }
    let mut header: String<HEADER_ROOM> = String::new();
    write!(
        header,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
        Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
        Connection: close\r\nCache-Control: no-store\r\n\r\n",
        body_len
    )
    .ok()?;
    let header_len = header.len();
    out_buf.copy_within(HEADER_ROOM..HEADER_ROOM + body_len, header_len);
    out_buf[..header_len].copy_from_slice(header.as_bytes());
    Some(header_len + body_len)
}

type ZeroconfDhCell = RefCell<Option<([u8; 4], OakleyGroup1Dh)>>;
static ZEROCONF_DH_CACHE: Mutex<CriticalSectionRawMutex, ZeroconfDhCell> =
    Mutex::new(RefCell::new(None));

/// Derives a deterministic 768-bit Diffie-Hellman private key from the assigned
/// IP address without volatile metrics (uptime, volume, i2c ACKs) so the
/// keypair remains invariant across `getInfo` and `addUser` requests
/// (`NET-403`).
#[must_use]
pub fn derive_zeroconf_dh_private_key(status: &HttpStatusSnapshot<'_>) -> [u8; 96] {
    const SEED_PREFIX: &[u8] = b"louder-esp32-zeroconf-dh-v1";
    let mut key = [0u8; 96];
    let mut material = [0u8; 4];
    material.copy_from_slice(&status.ip);

    for (counter, chunk) in key.chunks_mut(20).enumerate() {
        let mut msg = [0u8; 5];
        msg[0..4].copy_from_slice(&material);
        msg[4] = counter as u8;
        let digest = hmac_sha1(SEED_PREFIX, &msg);
        let len = chunk.len().min(digest.len());
        chunk[..len].copy_from_slice(&digest[..len]);
    }

    // SplitMix64 / FNV-1a nonlinear permutation across all 96 bytes
    let mut state: u64 = 0xcbf29ce484222325;
    for &b in &status.ip {
        state ^= b as u64;
        state = state.wrapping_mul(0x100000001b3);
    }
    for chunk in key.chunks_mut(8) {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        let val = z ^ (z >> 31);
        let b = val.to_le_bytes();
        for (i, byte) in chunk.iter_mut().enumerate() {
            *byte ^= b[i % 8];
        }
    }

    key[0] |= 0x01;
    key[95] &= 0x7F;
    key[95] |= 0x40;
    key
}

/// Derives a 768-bit Diffie-Hellman private key from the assigned IP address
/// and an explicit entropy source (such as hardware RNG or device MAC address).
#[must_use]
pub fn derive_zeroconf_dh_private_key_with_entropy(ip: [u8; 4], entropy: [u8; 8]) -> [u8; 96] {
    const SEED_PREFIX: &[u8] = b"louder-esp32-zeroconf-dh-v1";
    let mut key = [0u8; 96];
    let mut material = [0u8; 12];
    material[0..4].copy_from_slice(&ip);
    material[4..12].copy_from_slice(&entropy);

    for (counter, chunk) in key.chunks_mut(20).enumerate() {
        let mut msg = [0u8; 13];
        msg[0..12].copy_from_slice(&material);
        msg[12] = counter as u8;
        let digest = hmac_sha1(SEED_PREFIX, &msg);
        let len = chunk.len().min(digest.len());
        chunk[..len].copy_from_slice(&digest[..len]);
    }

    key[95] |= 0x01;
    key
}

/// Formats the `/eq.json` payload containing equalizer status into `out_buf`.
pub fn write_eq_json(
    snapshot: &HttpStatusSnapshot<'_>,
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    write!(
        writer,
        "{{\"ok\":true,\"eq_preset\":\"{}\",\"eq_enabled\":{},\
         \"again_code\":{},\"vol_pct\":{}}}",
        JsonEscaped(snapshot.eq_preset),
        snapshot.eq_enabled,
        snapshot.again_code,
        snapshot.vol_pct,
    )
    .ok()?;
    Some(writer.written_len())
}

fn redact_command_password(cmd: &str) -> String<160> {
    let mut out: String<160> = String::new();
    let is_urlencoded = cmd.contains("%20");
    let sep = if is_urlencoded { "%20" } else { " " };
    let parts: heapless::Vec<&str, 8> = cmd.split(sep).collect();
    let ws_parts: heapless::Vec<&str, 8> = cmd.split_whitespace().collect();
    if (parts.first() == Some(&"join") || parts.first() == Some(&"connect")) && parts.len() >= 3 {
        let mask = if parts.first() == Some(&"connect") { "<redacted>" } else { "***" };
        let _ = write!(out, "{}{}{}{}{}", parts[0], sep, parts[1], sep, mask);
        for extra in &parts[3..] {
            let _ = write!(out, "{}{}", sep, extra);
        }
    } else if (ws_parts.first() == Some(&"join") || ws_parts.first() == Some(&"connect"))
        && ws_parts.len() >= 3
    {
        let mask = if ws_parts.first() == Some(&"connect") { "<redacted>" } else { "***" };
        let _ = write!(out, "{}{}{}{}{}", ws_parts[0], sep, ws_parts[1], sep, mask);
        for extra in &ws_parts[3..] {
            let _ = write!(out, "{}{}", sep, extra);
        }
    } else if parts.first() == Some(&"config")
        && parts.get(1) == Some(&"set")
        && (parts.get(2) == Some(&"wifi.pass") || parts.get(2) == Some(&"wifi_pass"))
        && parts.len() >= 4
    {
        let _ = write!(
            out,
            "config{}set{}{}{}***",
            sep,
            sep,
            parts[2],
            sep
        );
    } else if let Some(idx) = cmd.find("wifi.pass=") {
        let prefix = &cmd[..idx + "wifi.pass=".len()];
        let rest = &cmd[idx + "wifi.pass=".len()..];
        let end_idx = rest.find('&').or_else(|| rest.find(' ')).unwrap_or(rest.len());
        let _ = write!(out, "{}***{}", prefix, &rest[end_idx..]);
    } else if let Some(idx) = cmd.find("password=") {
        let prefix = &cmd[..idx + "password=".len()];
        let rest = &cmd[idx + "password=".len()..];
        let end_idx = rest.find('&').or_else(|| rest.find(' ')).unwrap_or(rest.len());
        let _ = write!(out, "{}***{}", prefix, &rest[end_idx..]);
    } else {
        let _ = out.push_str(cmd);
    }
    out
}

fn is_authorized(req_bytes: &[u8], expected_token: &str) -> bool {
    let Ok(req_str) = core::str::from_utf8(req_bytes) else {
        return false;
    };
    if let Some(first_line) = req_str.lines().next() {
        if let Some(path_and_query) = first_line.split_whitespace().nth(1) {
            if let Some((_, query)) = path_and_query.split_once('?') {
                for param in query.split('&') {
                    if let Some(val) = param.strip_prefix("auth=") {
                        if val == expected_token {
                            return true;
                        }
                    }
                    if let Some(val) = param.strip_prefix("token=") {
                        if val == expected_token {
                            return true;
                        }
                    }
                }
            }
        }
    }
    for line in req_str.lines() {
        let trimmed = line.trim();
        if let Some(auth_val) = trimmed.strip_prefix("Authorization:")
            .or_else(|| trimmed.strip_prefix("authorization:"))
        {
            let auth_val = auth_val.trim();
            if let Some(bearer_val) = auth_val.strip_prefix("Bearer ")
                .or_else(|| auth_val.strip_prefix("bearer "))
            {
                if bearer_val.trim() == expected_token {
                    return true;
                }
            } else if auth_val == expected_token {
                return true;
            }
        } else if let Some(token_val) = trimmed.strip_prefix("X-Auth-Token:")
            .or_else(|| trimmed.strip_prefix("x-auth-token:"))
        {
            if token_val.trim() == expected_token {
                return true;
            }
        }
    }
    false
}

/// Port 80 HTTP Control router with optional authentication token enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpControlRouter {
    /// Optional authentication token required for HTTP requests.
    pub auth_token: Option<String<32>>,
}

impl HttpControlRouter {
    /// Creates a new unauthenticated router (`auth_token: None`).
    #[must_use]
    pub const fn new() -> Self {
        Self { auth_token: None }
    }

    /// Creates a router with an authentication token.
    #[must_use]
    pub fn with_auth_token(token: &str) -> Self {
        let mut s = String::new();
        let _ = s.push_str(token);
        Self {
            auth_token: Some(s),
        }
    }

    /// Handles an HTTP request with streaming support.
    pub fn handle_request_streaming(
        &self,
        req_bytes: &[u8],
        snapshot: &HttpStatusSnapshot<'_>,
        out_buf: &mut [u8],
    ) -> Option<(usize, &'static [u8], HttpControlAction)> {
        if let Some(ref token) = self.auth_token {
            if !is_authorized(req_bytes, token.as_str()) {
                const BODY: &str = "{\"error\":\"unauthorized\"}";
                let mut response: String<256> = String::new();
                let _ = write!(
                    response,
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\
                    Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                    Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                    BODY.len(),
                    BODY
                );
                let resp_bytes = response.as_bytes();
                if resp_bytes.len() > out_buf.len() {
                    return None;
                }
                out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
                return Some((resp_bytes.len(), &[], HttpControlAction::None));
            }
        }
        handle_http_request_streaming_impl(req_bytes, snapshot, out_buf)
    }

    /// Handles an HTTP request into `out_buf`.
    pub fn handle_request(
        &self,
        req_bytes: &[u8],
        snapshot: &HttpStatusSnapshot<'_>,
        out_buf: &mut [u8],
    ) -> Option<(usize, HttpControlAction)> {
        let (hdr_len, static_body, action) =
            self.handle_request_streaming(req_bytes, snapshot, out_buf)?;
        if !static_body.is_empty() {
            let total_len = hdr_len + static_body.len();
            if total_len > out_buf.len() {
                return None;
            }
            out_buf[hdr_len..total_len].copy_from_slice(static_body);
            Some((total_len, action))
        } else {
            Some((hdr_len, action))
        }
    }
}

fn is_permitted_origin(req_bytes: &[u8]) -> bool {
    let Ok(req_str) = core::str::from_utf8(req_bytes) else {
        return false;
    };
    for line in req_str.lines() {
        let trimmed = line.trim();
        if let Some(origin_val) = trimmed.strip_prefix("Origin:")
            .or_else(|| trimmed.strip_prefix("origin:"))
        {
            let val = origin_val.trim();
            if val.is_empty() || val == "null" {
                return false;
            }
            let host_part = val.strip_prefix("http://")
                .or_else(|| val.strip_prefix("https://"))
                .unwrap_or(val);
            let host_end = host_part.find(&['/', ':'][..]).unwrap_or(host_part.len());
            let host = &host_part[..host_end];
            return crate::ota_safety::is_local_lan_host(host);
        }
    }
    true
}

/// Parses an HTTP request from `req_bytes` and writes the HTTP header (or full
/// response for JSON) into `out_buf`.
///
/// For static HTML responses (`GET /`, `GET /index.html`), writes ONLY the HTTP
/// headers into `out_buf` and returns `Some((header_len,
/// DASHBOARD_HTML.as_bytes(), action))`, enabling multi-chunk streaming
/// directly from `.rodata` without buffer overflow or truncation.
///
/// Returns `None` if `req_bytes` does not yet contain a complete request line
/// (`\r\n` or `\n`).
pub fn handle_http_request_streaming_impl(
    req_bytes: &[u8],
    snapshot: &HttpStatusSnapshot<'_>,
    out_buf: &mut [u8],
) -> Option<(usize, &'static [u8], HttpControlAction)> {
    let line_end = req_bytes.iter().position(|&b| b == b'\n')?;
    let first_line = core::str::from_utf8(&req_bytes[..line_end])
        .unwrap_or("")
        .trim_end_matches('\r');

    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("");
    let path = raw_path.split('?').next().unwrap_or(raw_path);

    if method == "POST" && !is_permitted_origin(req_bytes) {
        const BODY: &str = "{\"error\":\"forbidden_cross_origin\"}";
        let mut response: String<256> = String::new();
        let _ = write!(
            response,
            "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\n\
            X-Content-Type-Options: nosniff\r\n\
            X-Frame-Options: DENY\r\n\
            Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
            Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
            BODY.len(),
            BODY
        );
        let resp_bytes = response.as_bytes();
        if resp_bytes.len() > out_buf.len() {
            return None;
        }
        out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
        return Some((resp_bytes.len(), &[], HttpControlAction::None));
    }

    // Spotify Connect ZeroConf HTTP handling
    // (GET /?action=getInfo, POST /?action=addUser, /spotify_info)
    if raw_path.contains("action=getInfo")
        || raw_path.contains("action=addUser")
        || raw_path.contains("spotify_info")
    {
        let body_slice = req_bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| &req_bytes[p + 4..])
            .or_else(|| {
                req_bytes
                    .windows(2)
                    .position(|w| w == b"\n\n")
                    .map(|p| &req_bytes[p + 2..])
            })
            .unwrap_or(&[]);
        let body_str = core::str::from_utf8(body_slice).unwrap_or("");
        let priv_key = derive_zeroconf_dh_private_key(snapshot);
        let dh = ZEROCONF_DH_CACHE.lock(|cell| {
            let mut opt = cell.borrow_mut();
            if let Some((cached_ip, ref dh)) = *opt {
                if cached_ip == snapshot.ip {
                    return Some(dh.clone());
                }
            }
            let new_dh = OakleyGroup1Dh::from_private_key(priv_key).ok()?;
            *opt = Some((snapshot.ip, new_dh.clone()));
            Some(new_dh)
        })?;
        let device_id = "142137fd329622137a149016c82e18beb834";
        let remote_name = "BASEMENT";

        let mut zc = ZeroConfServer::new();
        if let Ok(resp) = zc.handle_request(
            raw_path,
            body_str,
            &dh,
            device_id,
            remote_name,
        ) {
            let mut header: String<160> = String::new();
            let _ = write!(
                header,
                "HTTP/1.1 {} OK\r\nContent-Type: {}\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n",
                resp.status_code,
                resp.content_type,
                resp.body.len()
            );
            let header_bytes = header.as_bytes();
            let total_len = header_bytes.len() + resp.body.len();
            if total_len > out_buf.len() {
                return None;
            }
            out_buf[..header_bytes.len()].copy_from_slice(header_bytes);
            out_buf[header_bytes.len()..total_len].copy_from_slice(&resp.body);
            return Some((total_len, &[], HttpControlAction::None));
        }
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            let body_bytes = DASHBOARD_HTML.as_bytes();
            let mut header: String<160> = String::new();
            let _ = write!(
                header,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                Content-Length: {}\r\nConnection: close\r\n\
                Cache-Control: no-store\r\n\r\n",
                body_bytes.len()
            );
            let header_bytes = header.as_bytes();
            if header_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..header_bytes.len()].copy_from_slice(header_bytes);
            Some((header_bytes.len(), body_bytes, HttpControlAction::None))
        }
        ("GET", "/favicon.ico") => {
            let resp = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            if resp.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp.len()].copy_from_slice(resp);
            Some((resp.len(), &[], HttpControlAction::None))
        }
        ("GET", "/status.json") => {
            // The body is formatted straight into `out_buf` behind room for the
            // header, then moved down once the header (which carries the body
            // length) is known: no body scratch buffer on the stack.
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len = write_status_json(snapshot, &mut out_buf[HEADER_ROOM..])?;

            let mut header: String<HEADER_ROOM> = String::new();
            write!(
                header,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n",
                body_len
            )
            .ok()?;
            let header_len = header.len();
            out_buf.copy_within(HEADER_ROOM..HEADER_ROOM + body_len, header_len);
            out_buf[..header_len].copy_from_slice(header.as_bytes());
            Some((header_len + body_len, &[], HttpControlAction::None))
        }
        ("GET", "/cmd.json") | ("POST", "/cmd.json") => {
            let query = raw_path.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut act = query.split('&').find_map(|p| p.strip_prefix("act=")).unwrap_or("");
            if act.is_empty() {
                let body_slice = req_bytes
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| &req_bytes[p + 4..])
                    .or_else(|| {
                        req_bytes
                            .windows(2)
                            .position(|w| w == b"\n\n")
                            .map(|p| &req_bytes[p + 2..])
                    })
                    .unwrap_or(&[]);
                if let Ok(body_str) = core::str::from_utf8(body_slice) {
                    act = body_str.split('&').find_map(|p| p.strip_prefix("act=")).unwrap_or("");
                    if act.is_empty() {
                        let trimmed = body_str.trim();
                        if let Some(rest) = trimmed.strip_prefix("act=") {
                            act = rest.split('&').next().unwrap_or(rest);
                        }
                    }
                }
            }
            let (action, ok) = match act {
                "play" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_10EF,
                        vol_pct: None,
                        mute: Some(false),
                    },
                    true,
                ),
                "pause" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_20DF,
                        vol_pct: None,
                        mute: None,
                    },
                    true,
                ),
                "stop" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_20DF,
                        vol_pct: None,
                        mute: Some(true),
                    },
                    true,
                ),
                "prev" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_C03F,
                        vol_pct: None,
                        mute: None,
                    },
                    true,
                ),
                "next" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_A05F,
                        vol_pct: None,
                        mute: None,
                    },
                    true,
                ),
                "volup" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_807F,
                        vol_pct: Some(snapshot.vol_pct.saturating_add(5).min(100)),
                        mute: Some(false),
                    },
                    true,
                ),
                "voldown" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_00FF,
                        vol_pct: Some(snapshot.vol_pct.saturating_sub(5)),
                        mute: None,
                    },
                    true,
                ),
                "preset1" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_F00F,
                        vol_pct: None,
                        mute: Some(false),
                    },
                    true,
                ),
                "preset2" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_08F7,
                        vol_pct: None,
                        mute: Some(false),
                    },
                    true,
                ),
                "preset3" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_8877,
                        vol_pct: None,
                        mute: Some(false),
                    },
                    true,
                ),
                "preset4" => (
                    HttpControlAction::PlayerIrCommand {
                        ir_code: 0x7689_48B7,
                        vol_pct: None,
                        mute: Some(false),
                    },
                    true,
                ),
                "eq_flat" => (HttpControlAction::SetEqPreset(0), true),
                "eq_bass" => (HttpControlAction::SetEqPreset(1), true),
                "eq_loudness" => (HttpControlAction::SetEqPreset(2), true),
                "eq_vocal" | "eq_voice" => (HttpControlAction::SetEqPreset(3), true),
                "eq_rock" | "eq_treble" => (HttpControlAction::SetEqPreset(4), true),
                "again0" => (HttpControlAction::SetAnalogGain(0x00), true),
                "again4" => (HttpControlAction::SetAnalogGain(0x08), true),
                "again6" => (HttpControlAction::SetAnalogGain(0x0C), true),
                "again8" => (HttpControlAction::SetAnalogGain(0x10), true),
                "again10" => (HttpControlAction::SetAnalogGain(0x14), true),
                "spotify_play" => (HttpControlAction::SpotifyPlay, true),
                "spotify_stop" | "spotify_pause" => (HttpControlAction::SpotifyStop, true),
                other
                    if other.starts_with("vol")
                        && !other[3..].is_empty()
                        && other[3..].chars().all(|c| c.is_ascii_digit()) =>
                {
                    let pct: u8 = other[3..]
                        .parse::<u32>()
                        .map(|v| v.min(100) as u8)
                        .unwrap_or(snapshot.vol_pct);
                    (
                        HttpControlAction::PlayerIrCommand {
                            ir_code: 0,
                            vol_pct: Some(pct),
                            mute: Some(false),
                        },
                        true,
                    )
                }
                _ => (HttpControlAction::None, false),
            };

            let mut body: String<128> = String::new();
            if ok {
                let _ = write!(body, "{{\"ok\":true,\"act\":\"{}\"}}", act);
            } else {
                let _ = write!(body, "{{\"ok\":false,\"error\":\"unknown_action\"}}");
            }

            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body.as_str()
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], action))
        }
        ("GET", "/recovery.json") | ("GET", "/reboot.json") | ("GET", "/reboot_ota.json") => {
            let body = "{\"error\":\"method_not_allowed\"}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 405 Method Not Allowed\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], HttpControlAction::None))
        }
        ("POST", "/recovery.json") => {
            let body = "{\"status\":\"rebooting_to_recovery\",\
                \"target_partition\":\"factory_0x10000\"}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], HttpControlAction::RebootToRecovery))
        }
        ("POST", "/reboot.json") => {
            let body = "{\"status\":\"rebooting_to_ota_0\",\
                \"target_partition\":\"ota_0_0x160000\"}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], HttpControlAction::PersistOta0AndReboot))
        }
        ("POST", "/reboot_ota.json") => {
            let body = "{\"status\":\"rebooting_to_ota_0\",\
                \"target_partition\":\"ota_0_0x160000\"}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], HttpControlAction::PersistOta0AndReboot))
        }
        ("GET", "/eq.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len = write_eq_json(snapshot, &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("GET", "/config.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len = write_config_json(snapshot, &[], &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("POST", "/config.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let payload = extract_query_or_body(raw_path, req_bytes);
            let (body_len, action) = if let Some(upd) = parse_config_update_query(payload) {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                let mut decoded_key_buf = [0u8; 64];
                let decoded_key = percent_decode_into(upd.key, &mut decoded_key_buf)
                    .and_then(|len| core::str::from_utf8(&decoded_key_buf[..len]).ok())
                    .unwrap_or(upd.key);
                let mut decoded_val_buf = [0u8; 128];
                let decoded_val = percent_decode_into(upd.value, &mut decoded_val_buf)
                    .and_then(|len| core::str::from_utf8(&decoded_val_buf[..len]).ok())
                    .unwrap_or(upd.value);
                write!(
                    writer,
                    "{{\"ok\":true,\"key\":\"{}\",\"value\":\"{}\"}}",
                    JsonEscaped(decoded_key),
                    JsonEscaped(decoded_val),
                )
                .ok()?;
                (writer.written_len(), HttpControlAction::UpdateConfig)
            } else {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                writer
                    .write_str("{\"ok\":false,\"error\":\"invalid_config_param\"}")
                    .ok()?;
                (writer.written_len(), HttpControlAction::None)
            };
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], action))
        }
        ("GET", "/scan.json") | ("POST", "/scan.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let rssi_i8 = snapshot.rssi.clamp(-127, 0) as i8;
            let active_ap = [ScanApView::new(
                snapshot.ssid,
                [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
                rssi_i8,
                6,
                "WPA2_PSK",
            )];
            let body_len = write_scan_json(&active_ap, &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            let action = if method == "POST" {
                HttpControlAction::TriggerWifiScan
            } else {
                HttpControlAction::None
            };
            Some((total_len, &[], action))
        }
        ("GET", "/connect.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let connected = !snapshot.ssid.is_empty() && snapshot.ip != [0, 0, 0, 0];
            let body_len = write_connect_status_json(
                connected,
                snapshot.ssid,
                snapshot.ip,
                &mut out_buf[HEADER_ROOM..],
            )?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("POST", "/connect.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let payload = extract_query_or_body(raw_path, req_bytes);
            let (body_len, action) = if let Some(req) = parse_connect_request(payload) {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                let mut decoded_buf = [0u8; 64];
                let decoded_ssid = percent_decode_into(req.ssid, &mut decoded_buf)
                    .and_then(|len| core::str::from_utf8(&decoded_buf[..len]).ok())
                    .unwrap_or(req.ssid);
                let mut decoded_pass_buf = [0u8; 64];
                let _decoded_password = percent_decode_into(req.password, &mut decoded_pass_buf)
                    .and_then(|len| core::str::from_utf8(&decoded_pass_buf[..len]).ok())
                    .unwrap_or(req.password);
                write!(
                    writer,
                    "{{\"ok\":true,\"status\":\"connecting\",\"ssid\":\"{}\"}}",
                    JsonEscaped(decoded_ssid),
                )
                .ok()?;
                (writer.written_len(), HttpControlAction::ConnectWifi)
            } else {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                writer
                    .write_str("{\"ok\":false,\"error\":\"missing_ssid\"}")
                    .ok()?;
                (writer.written_len(), HttpControlAction::None)
            };
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], action))
        }
        ("GET", "/messages.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let entries = [SyslogMessageView::new(
                snapshot.settings_seq,
                snapshot.uptime_ms,
                "INFO",
                snapshot.amp_mode,
            )];
            let body_len = write_messages_json(&entries, &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("GET", "/commands.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len =
                write_commands_json(DEFAULT_COMMAND_CATALOG, None, &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("POST", "/commands.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let payload = extract_query_or_body(raw_path, req_bytes);
            let cmd_str = extract_param_or_json_field(payload, "cmd").unwrap_or("");
            let (body_len, action) = if !cmd_str.is_empty() {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                let redacted = redact_command_password(cmd_str);
                write!(
                    writer,
                    "{{\"ok\":true,\"cmd\":\"{}\"}}",
                    JsonEscaped(redacted.as_str()),
                )
                .ok()?;
                (writer.written_len(), HttpControlAction::ExecuteCliCommand)
            } else {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                writer
                    .write_str("{\"ok\":false,\"error\":\"missing_cmd\"}")
                    .ok()?;
                (writer.written_len(), HttpControlAction::None)
            };
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], action))
        }
        ("GET", "/flash.json") | ("POST", "/flash.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let (state_label, action) = if method == "POST" {
                ("starting_ota", HttpControlAction::StartOtaFlash)
            } else {
                ("idle", HttpControlAction::None)
            };
            let body_len = write_flash_status_json(state_label, 0, &mut out_buf[HEADER_ROOM..])?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], action))
        }
        _ => {
            let body = "{\"error\":\"not_found\"}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n{}",
                body.len(),
                body
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            Some((resp_bytes.len(), &[], HttpControlAction::None))
        }
    }
}

/// Parses an HTTP request from `req_bytes` and writes the HTTP header (or full
/// response for JSON) into `out_buf`.
pub fn handle_http_request_streaming(
    req_bytes: &[u8],
    snapshot: &HttpStatusSnapshot<'_>,
    out_buf: &mut [u8],
) -> Option<(usize, &'static [u8], HttpControlAction)> {
    HttpControlRouter::new().handle_request_streaming(req_bytes, snapshot, out_buf)
}

/// Parses an HTTP request from `req_bytes` and writes a complete HTTP/1.1
/// response into `out_buf`.
///
/// Returns `None` if `req_bytes` does not yet contain a complete request line
/// (`\r\n` or `\n`), or if `out_buf` is too small to contain the full response.
pub fn handle_http_control_request(
    req_bytes: &[u8],
    snapshot: &HttpStatusSnapshot<'_>,
    out_buf: &mut [u8],
) -> Option<(usize, HttpControlAction)> {
    HttpControlRouter::new().handle_request(req_bytes, snapshot, out_buf)
}

/// Static HTML dashboard embedded for `GET /` and `GET /index.html` requests.
pub const DASHBOARD_HTML: &str = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Louder-ESP32 Control</title><style>body{font-family:sans-serif;margin:6px;background:#111;color:#eee}h3{margin:4px 0 2px;color:#0ef}table{width:100%;border-collapse:collapse}td{padding:2px 4px;border:1px solid #333;font-size:11px}button{margin:1px;padding:3px 6px;background:#222;color:#fff;border:1px solid #444;border-radius:3px;cursor:pointer}a{color:#0ef;text-decoration:none;font-size:11px}</style></head><body><h3>Louder-ESP32 Control</h3><table><tr><td>Net/RSSI</td><td id=\"n\">-</td></tr><tr><td>LMS</td><td id=\"l\">-</td></tr><tr><td>Amp/Up</td><td id=\"s\">-</td></tr><tr><td>Audio</td><td id=\"f\">-</td></tr><tr><td>PSRAM</td><td id=\"p\">-</td></tr><tr><td>Core 1</td><td id=\"c\">-</td></tr><tr><td>TAS5805M</td><td id=\"t\">-</td></tr><tr><td>EQ / NVS</td><td id=\"e\">-</td></tr><tr><td>I2C</td><td id=\"i\">-</td></tr><tr><td>Firmware</td><td id=\"w\">-</td></tr></table><h3>Transport &amp; Volume</h3><button style=\"background:#085\" onclick=\"cmd('play')\">&#9654;Play</button><button onclick=\"cmd('pause')\">&#10074;&#10074;Pause</button><button onclick=\"cmd('stop')\">&#9632;Stop</button><button onclick=\"cmd('prev')\">&#9198;Prev</button><button onclick=\"cmd('next')\">Next&#9197;</button><br><button onclick=\"cmd('voldown')\">-5%</button><button onclick=\"cmd('vol25')\">25%</button><button onclick=\"cmd('vol40')\">40%</button><button onclick=\"cmd('vol55')\">55%</button><button onclick=\"cmd('vol70')\">70%</button><button onclick=\"cmd('vol85')\">85%</button><button onclick=\"cmd('volup')\">+5%</button><h3>Presets</h3><button style=\"background:#158\" onclick=\"cmd('preset1')\">Ibiza</button><button style=\"background:#158\" onclick=\"cmd('preset2')\">WHYY</button><button style=\"background:#158\" onclick=\"cmd('preset3')\">WDIY</button><button style=\"background:#158\" onclick=\"cmd('preset4')\">Groove</button><h3>Equalizer (10-Band)</h3><button onclick=\"cmd('eq_flat')\">Flat</button><button onclick=\"cmd('eq_bass')\">Bass Boost</button><button onclick=\"cmd('eq_loudness')\">Loudness</button><button onclick=\"cmd('eq_vocal')\">Vocal</button><button onclick=\"cmd('eq_rock')\">Rock</button><h3>Analog Gain (AGAIN)</h3><button onclick=\"cmd('again0')\">0 dB (29.5V)</button><button onclick=\"cmd('again4')\">-4 dB (20V PD)</button><button onclick=\"cmd('again6')\">-6 dB (15V)</button><button onclick=\"cmd('again8')\">-8 dB (12V)</button><br><a id=\"ll\" href=\"http://10.0.0.10:9000/\" target=\"_blank\">&rarr; Open Full Lyrion UI (10.0.0.10:9000)</a><h3>System</h3><button style=\"background:#922\" onclick=\"if(confirm('Recovery?'))fetch('/recovery.json',{method:'POST'})\">Reboot to Recovery (0x10000)</button><button style=\"background:#444\" onclick=\"if(confirm('Reboot OTA?'))fetch('/reboot_ota.json',{method:'POST'})\">Commit OTA &amp; Reboot</button><script>var pF=0,pT=0,D=document;function cmd(a){fetch('/cmd.json?act='+a).then(u).catch(()=>{});}function g(k,v){D.getElementById(k).innerText=v;}function u(){var n=Date.now();fetch('/status.json').then(r=>r.json()).then(d=>{var h=d.lms_ip+':'+d.lms_port;D.getElementById('ll').href='http://'+h+'/';g('n',d.ip+' ('+d.ssid+' '+d.rssi+'dBm)');g('l',h+' (CLI '+d.lms_cport+')');var up=d.uptime_ms/1000|0;g('s',d.amp_mode+' ('+d.vol_pct+'%) | Up '+(up/60|0)+':'+((up%60<10)?'0':'')+(up%60));var fps=(pT>0&&n>pT)?((d.frames_played-pF)*1000/(n-pT)|0):0;pF=d.frames_played;pT=n;g('f',fps+' fps ('+d.frames_played+') | '+d.codec);g('p',(d.psram_used_bytes/1024|0)+' / '+(d.psram_free_bytes/1024|0)+' KB');g('c',(d.core1_active?'Active':'Halted')+' ('+d.core1_loops+' loops)');g('t',d.tas_fs_hz+'Hz|'+d.tas_bck_ratio+'x|'+(d.tas_pll_locked?'PLL-Lock':'Unlocked')+'|-'+(d.again_code*0.5).toFixed(1)+'dB'+(d.tas_automuted?'(Muted)':''));g('e',d.eq_preset+' | Seq '+d.settings_seq);g('i','ACK:'+d.i2c_acks+'/NACK:'+d.i2c_nacks);g('w',d.version+' ('+(d.otadata_prearmed_factory?'Pre-Armed':'Normal')+')');}).catch(()=>{});}u();setInterval(u,1500);</script></body></html>";

/// Alias for static HTML dashboard.
pub const INDEX_HTML: &str = DASHBOARD_HTML;

/// Health verdict emitted by [`DeadManLeaseWatchdog::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseHealth {
    /// Network lease is healthy or still within the allowed grace window.
    Healthy,
    /// Lease timeout exceeded; caller MUST immediately invoke
    /// `esp_hal::system::software_reset()`.
    ExpiredResetToRecovery,
}

/// Gate 3 Dead-Man Network Lease Watchdog.
///
/// Guarantees that if Wi-Fi + DHCPv4 is not established within
/// `BOOT_LEASE_TIMEOUT_MS` (45s) of boot, or if connectivity drops for more
/// than `DROP_LEASE_TIMEOUT_MS` (60s) at runtime, the device resets into the
/// untouched `recovery` partition (`0x10000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadManLeaseWatchdog {
    boot_ms: u64,
    ever_leased: bool,
    last_healthy_ms: u64,
    boot_timeout_ms: u64,
    drop_timeout_ms: u64,
}

impl DeadManLeaseWatchdog {
    /// Creates a new watchdog anchored at `boot_ms`.
    #[must_use]
    pub const fn new(boot_ms: u64) -> Self {
        Self {
            boot_ms,
            ever_leased: false,
            last_healthy_ms: boot_ms,
            boot_timeout_ms: BOOT_LEASE_TIMEOUT_MS,
            drop_timeout_ms: DROP_LEASE_TIMEOUT_MS,
        }
    }

    /// Evaluates current Wi-Fi link + IPv4 lease state at `now_ms`.
    pub fn evaluate(
        &mut self,
        now_ms: u64,
        wifi_connected: bool,
        has_ipv4_lease: bool,
    ) -> LeaseHealth {
        if wifi_connected && has_ipv4_lease {
            self.ever_leased = true;
            self.last_healthy_ms = now_ms;
            return LeaseHealth::Healthy;
        }

        if !self.ever_leased {
            if now_ms.saturating_sub(self.boot_ms) >= self.boot_timeout_ms {
                return LeaseHealth::ExpiredResetToRecovery;
            }
        } else if now_ms.saturating_sub(self.last_healthy_ms) >= self.drop_timeout_ms {
            return LeaseHealth::ExpiredResetToRecovery;
        }

        LeaseHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_status_json_matching_squeezelite_schema() {
        let snap = HttpStatusSnapshot {
            frames_played: 1024,
            amp_mode: "Play",
            i2c_acks: 18,
            i2c_nacks: 0,
            uptime_ms: 12_345,
            ..HttpStatusSnapshot::default()
        };
        let mut buf = [0u8; 2048];
        let req = b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (len, action) =
            handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(action, HttpControlAction::None);
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("\"project_name\":\"Squeezelite-ESP32\""));
        assert!(resp.contains("\"version\":\"I2S-4MFlash-Rust\""));
        assert!(resp.contains("\"recovery\":0"));
        assert!(resp.contains("\"rust_engine\":true"));
        assert!(resp.contains("\"ip\":\"172.22.1.15\""));
        assert!(resp.contains("\"frames_played\":1024"));
        assert!(resp.contains("\"amp_mode\":\"Play\""));
        assert!(resp.contains("\"vol_pct\":55"));
        assert!(resp.contains("\"psram_free_bytes\":1622016"));
        assert!(resp.contains("\"psram_used_bytes\":2572288"));
        assert!(resp.contains("\"core1_active\":true"));
        assert!(resp.contains("\"core1_loops\":0"));
        assert!(resp.contains("\"codec\":\"PCM\""));
        assert!(resp.contains("\"codec_sample_rate\":44100"));
        assert!(resp.contains("\"codec_channels\":2"));
        assert!(resp.contains("\"codec_bit_depth\":16"));
        assert!(resp.contains("\"eq_preset\":\"Flat\""));
        assert!(resp.contains("\"eq_enabled\":false"));
        assert!(resp.contains("\"tas_fs_hz\":44100"));
        assert!(resp.contains("\"tas_bck_ratio\":64"));
        assert!(resp.contains("\"tas_pll_locked\":true"));
        assert!(resp.contains("\"tas_automuted\":false"));
        assert!(resp.contains("\"again_code\":8"));
        assert!(resp.contains("\"settings_seq\":1"));
    }

    #[test]
    fn handles_cmd_json_player_ir_and_volume_commands() {
        let snap = HttpStatusSnapshot {
            vol_pct: 60,
            ..HttpStatusSnapshot::default()
        };
        let mut buf = [0u8; 512];

        // 1. Play
        let req1 = b"GET /cmd.json?act=play HTTP/1.1\r\n\r\n";
        let (len1, act1) =
            handle_http_control_request(req1, &snap, &mut buf).unwrap();
        assert_eq!(
            act1,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_10EF,
                vol_pct: None,
                mute: Some(false)
            }
        );
        let resp1 = core::str::from_utf8(&buf[..len1]).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp1.contains("Access-Control-Allow-Origin: *"));
        assert!(resp1.contains("{\"ok\":true,\"act\":\"play\"}"));

        // 2. Pause
        let req2 = b"GET /cmd.json?act=pause HTTP/1.1\r\n\r\n";
        let (_len2, act2) =
            handle_http_control_request(req2, &snap, &mut buf).unwrap();
        assert_eq!(
            act2,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_20DF,
                vol_pct: None,
                mute: None
            }
        );

        // 3. Stop
        let req3 = b"GET /cmd.json?act=stop HTTP/1.1\r\n\r\n";
        let (_len3, act3) =
            handle_http_control_request(req3, &snap, &mut buf).unwrap();
        assert_eq!(
            act3,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_20DF,
                vol_pct: None,
                mute: Some(true)
            }
        );

        // 4. VolUp (+5% from snapshot.vol_pct=60 -> 65)
        let req4 = b"GET /cmd.json?act=volup HTTP/1.1\r\n\r\n";
        let (_len4, act4) =
            handle_http_control_request(req4, &snap, &mut buf).unwrap();
        assert_eq!(
            act4,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_807F,
                vol_pct: Some(65),
                mute: Some(false)
            }
        );

        // 5. VolDown (-5% from snapshot.vol_pct=60 -> 55)
        let req5 = b"GET /cmd.json?act=voldown HTTP/1.1\r\n\r\n";
        let (_len5, act5) =
            handle_http_control_request(req5, &snap, &mut buf).unwrap();
        assert_eq!(
            act5,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_00FF,
                vol_pct: Some(55),
                mute: None
            }
        );

        // 6. Direct volume setting: vol70
        let req6 = b"GET /cmd.json?act=vol70 HTTP/1.1\r\n\r\n";
        let (_len6, act6) =
            handle_http_control_request(req6, &snap, &mut buf).unwrap();
        assert_eq!(
            act6,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0,
                vol_pct: Some(70),
                mute: Some(false)
            }
        );

        // 7. Preset1
        let req7 = b"GET /cmd.json?act=preset1 HTTP/1.1\r\n\r\n";
        let (_len7, act7) =
            handle_http_control_request(req7, &snap, &mut buf).unwrap();
        assert_eq!(
            act7,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_F00F,
                vol_pct: None,
                mute: Some(false)
            }
        );

        // 8. Unknown action -> ok: false
        let req8 = b"GET /cmd.json?act=unknown HTTP/1.1\r\n\r\n";
        let (len8, act8) =
            handle_http_control_request(req8, &snap, &mut buf).unwrap();
        assert_eq!(act8, HttpControlAction::None);
        let resp8 = core::str::from_utf8(&buf[..len8]).unwrap();
        assert!(resp8.contains("{\"ok\":false,\"error\":\"unknown_action\"}"));
    }

    #[test]
    fn triggers_reboot_to_recovery_on_recovery_and_reboot_endpoints() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 768];

        let (len1, act1) =
            handle_http_control_request(b"POST /recovery.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act1, HttpControlAction::RebootToRecovery);
        assert!(core::str::from_utf8(&buf[..len1])
            .unwrap()
            .contains("\"rebooting_to_recovery\""));

        let (_len2, act2) =
            handle_http_control_request(b"POST /reboot.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act2, HttpControlAction::PersistOta0AndReboot);

        let (_len3, act3) =
            handle_http_control_request(b"POST /reboot_ota.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act3, HttpControlAction::PersistOta0AndReboot);
    }

    #[test]
    fn test_cmd_json_post_body_and_json_string_escaping() {
        let snap = HttpStatusSnapshot {
            ssid: "My\"Wi-Fi\\Net\tName\x01",
            amp_mode: "Stereo\"BTL\"",
            ..Default::default()
        };
        let mut buf = [0u8; 1024];

        // JSON escaping in write_status_json
        let len = write_status_json(&snap, &mut buf).unwrap();
        let json_str = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(json_str.contains("\"ssid\":\"My\\\"Wi-Fi\\\\Net\\tName\\u0001\""));
        assert!(json_str.contains("\"amp_mode\":\"Stereo\\\"BTL\\\"\""));

        // POST /cmd.json with act= in body
        let req = b"POST /cmd.json HTTP/1.1\r\nHost: 192.168.1.50\r\n\
            Content-Type: application/x-www-form-urlencoded\r\n\r\nact=pause";
        let (resp_len, act) = handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(act, HttpControlAction::PlayerIrCommand {
            ir_code: 0x7689_20DF,
            vol_pct: None,
            mute: None,
        });
        assert!(core::str::from_utf8(&buf[..resp_len]).unwrap().contains("\"ok\":true"));
    }

    #[test]
    fn dead_man_lease_watchdog_enforces_45s_boot_and_60s_drop_timeouts() {
        let mut wd = DeadManLeaseWatchdog::new(1_000);

        assert_eq!(wd.evaluate(45_999, false, false), LeaseHealth::Healthy);
        assert_eq!(
            wd.evaluate(46_000, false, false),
            LeaseHealth::ExpiredResetToRecovery
        );

        let mut wd2 = DeadManLeaseWatchdog::new(0);
        assert_eq!(wd2.evaluate(5_000, true, true), LeaseHealth::Healthy);
        assert_eq!(wd2.evaluate(64_999, false, false), LeaseHealth::Healthy);
        assert_eq!(
            wd2.evaluate(65_000, false, false),
            LeaseHealth::ExpiredResetToRecovery
        );
    }

    #[test]
    fn serves_html_dashboard_on_root_and_index_html() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 4096];

        let req1 = b"GET / HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (len1, act1) =
            handle_http_control_request(req1, &snap, &mut buf).unwrap();
        assert_eq!(act1, HttpControlAction::None);
        let resp1 = core::str::from_utf8(&buf[..len1]).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp1.contains("Content-Type: text/html; charset=utf-8"));
        assert!(resp1.contains("<title>Louder-ESP32 Control</title>"));
        assert!(resp1.contains("/status.json"));

        let req2 = b"GET /index.html HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (len2, act2) =
            handle_http_control_request(req2, &snap, &mut buf).unwrap();
        assert_eq!(act2, HttpControlAction::None);
        let resp2 = core::str::from_utf8(&buf[..len2]).unwrap();
        assert!(resp2.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp2.contains("Content-Type: text/html; charset=utf-8"));
    }

    #[test]
    fn serves_favicon_204_and_handles_streaming() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 512];

        let (len, body, act) =
            handle_http_request_streaming(b"GET /favicon.ico HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act, HttpControlAction::None);
        assert!(body.is_empty());
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 204 No Content\r\n"));

        let (hdr_len, body_bytes, act2) =
            handle_http_request_streaming(b"GET / HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act2, HttpControlAction::None);
        assert!(!body_bytes.is_empty());
        let hdr_resp = core::str::from_utf8(&buf[..hdr_len]).unwrap();
        assert!(hdr_resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(hdr_resp.contains("Content-Type: text/html; charset=utf-8"));
    }

    fn sample_status() -> HttpStatusSnapshot<'static> {
        HttpStatusSnapshot::default()
    }

    #[test]
    fn test_cmd_json_eq_and_again_actions() {
        extern crate alloc;
        use alloc::format;

        let mut scratch = [0u8; 1024];
        for (act, expected_action) in [
            ("eq_flat", HttpControlAction::SetEqPreset(0)),
            ("eq_bass", HttpControlAction::SetEqPreset(1)),
            ("eq_loudness", HttpControlAction::SetEqPreset(2)),
            ("eq_vocal", HttpControlAction::SetEqPreset(3)),
            ("eq_rock", HttpControlAction::SetEqPreset(4)),
            ("again0", HttpControlAction::SetAnalogGain(0x00)),
            ("again4", HttpControlAction::SetAnalogGain(0x08)),
            ("again6", HttpControlAction::SetAnalogGain(0x0C)),
            ("again8", HttpControlAction::SetAnalogGain(0x10)),
        ] {
            let req = format!("GET /cmd.json?act={act} HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n");
            let (len, action) =
                handle_http_control_request(req.as_bytes(), &sample_status(), &mut scratch)
                    .expect("valid response");
            assert_eq!(action, expected_action);
            let text = core::str::from_utf8(&scratch[..len]).expect("utf8");
            assert!(text.contains("HTTP/1.1 200 OK"));
        }
    }

    #[test]
    fn test_cmd_json_eq_and_again_aliases() {
        let mut scratch = [0u8; 1024];

        // eq_voice alias
        let (len1, act1) = handle_http_control_request(
            b"GET /cmd.json?act=eq_voice HTTP/1.1\r\n\r\n",
            &sample_status(),
            &mut scratch,
        )
        .unwrap();
        assert_eq!(act1, HttpControlAction::SetEqPreset(3));
        assert!(core::str::from_utf8(&scratch[..len1])
            .unwrap()
            .contains("{\"ok\":true,\"act\":\"eq_voice\"}"));

        // eq_treble alias
        let (len2, act2) = handle_http_control_request(
            b"GET /cmd.json?act=eq_treble HTTP/1.1\r\n\r\n",
            &sample_status(),
            &mut scratch,
        )
        .unwrap();
        assert_eq!(act2, HttpControlAction::SetEqPreset(4));
        assert!(core::str::from_utf8(&scratch[..len2])
            .unwrap()
            .contains("{\"ok\":true,\"act\":\"eq_treble\"}"));

        // again10 alias
        let (len3, act3) = handle_http_control_request(
            b"GET /cmd.json?act=again10 HTTP/1.1\r\n\r\n",
            &sample_status(),
            &mut scratch,
        )
        .unwrap();
        assert_eq!(act3, HttpControlAction::SetAnalogGain(0x14));
        assert!(core::str::from_utf8(&scratch[..len3])
            .unwrap()
            .contains("{\"ok\":true,\"act\":\"again10\"}"));
    }

    #[test]
    fn test_format_status_json_buffer() {
        let snap = RuntimeStatusSnapshot::default();
        let mut buf = [0u8; 2048];
        let written = format_status_json(&snap, &mut buf).expect("format ok");
        assert!(written > 0);
        let s = core::str::from_utf8(&buf[..written]).expect("valid utf8");
        assert!(s.starts_with('{'));
        assert!(s.ends_with('}'));
        assert!(s.contains("\"psram_free_bytes\":1622016"));
        assert!(s.contains("\"core1_active\":true"));
        assert!(s.contains("\"tas_fs_hz\":44100"));
        assert!(s.contains("\"eq_preset\":\"Flat\""));
    }

    #[test]
    fn test_index_html_alias() {
        assert_eq!(INDEX_HTML, DASHBOARD_HTML);
        assert!(INDEX_HTML.contains("Equalizer (10-Band)"));
        assert!(INDEX_HTML.contains("Analog Gain (AGAIN)"));
        assert!(INDEX_HTML.contains("eq_flat"));
        assert!(INDEX_HTML.contains("again0"));
    }

    /// Worst case of every field: the response must still fit the device's
    /// 1,536-byte response buffer (`STATIC_HTTP_RESP_BUF`) and HTTP socket TX
    /// buffer.
    #[test]
    fn test_status_json_worst_case_fits_the_device_response_buffer() {
        extern crate alloc;
        use alloc::format;

        let worst = HttpStatusSnapshot {
            ip: [255; 4],
            netmask: [255; 4],
            gw: [255; 4],
            // 32 escaped quotes
            ssid: "\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"",
            rssi: -100,
            lms_ip: [255; 4],
            lms_port: u16::MAX,
            lms_cport: u16::MAX,
            frames_played: u64::MAX,
            amp_mode: "PowerDown",
            vol_pct: 100,
            i2c_acks: u32::MAX,
            i2c_nacks: u32::MAX,
            i2c_nack_by_op: I2cNackByOp {
                read: u32::MAX,
                write: u32::MAX,
                write_read: u32::MAX,
            },
            uptime_ms: u64::MAX,
            psram_free_bytes: u32::MAX,
            psram_used_bytes: u32::MAX,
            core1_loops: u32::MAX,
            codec: "Vorbis",
            codec_sample_rate: u32::MAX,
            eq_preset: "Loudness",
            tas_fs_hz: u32::MAX,
            tas_bck_ratio: u16::MAX,
            again_code: u8::MAX,
            settings_seq: u32::MAX,
            bt_rx_pkts: u32::MAX,
            bt_frames: u64::MAX,
            decode_us_max: u32::MAX,
            decode_us_first: u32::MAX,
            ..Default::default()
        };
        let mut out_buf = [0u8; 1536];
        let (len, _, _) = handle_http_request_streaming(
            b"GET /status.json HTTP/1.1\r\n\r\n",
            &worst,
            &mut out_buf,
        )
        .expect("worst-case status.json fits");
        let resp = core::str::from_utf8(&out_buf[..len]).unwrap();
        let (head, body) = resp.split_once("\r\n\r\n").unwrap();
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        assert!(body.starts_with('{') && body.ends_with('}'));
        // A buffer too small for the body is refused, never truncated.
        let mut small = [0u8; 600];
        assert!(
            handle_http_request_streaming(
                b"GET /status.json HTTP/1.1\r\n\r\n",
                &worst,
                &mut small,
            )
            .is_none()
        );
    }

    #[test]
    fn test_status_json_fits_in_the_device_response_buffer() {
        let snap = HttpStatusSnapshot {
            ip: [172, 22, 1, 15],
            netmask: [255, 255, 255, 0],
            gw: [172, 22, 1, 1],
            ssid: "IoT-LAN",
            rssi: -48,
            lms_ip: [10, 0, 0, 10],
            lms_port: 9000,
            lms_cport: 9090,
            otadata_prearmed_factory: true,
            frames_played: 123_456,
            amp_mode: "Play",
            vol_pct: 55,
            i2c_acks: 42,
            i2c_nacks: 5,
            i2c_nack_by_op: I2cNackByOp {
                read: 1,
                write: 3,
                write_read: 1,
            },
            uptime_ms: 98_765,
            psram_free_bytes: DEFAULT_PSRAM_FREE_BYTES,
            psram_used_bytes: DEFAULT_PSRAM_USED_BYTES,
            core1_active: true,
            core1_loops: 54_321,
            codec: "MP3",
            codec_sample_rate: 44_100,
            codec_channels: 2,
            codec_bit_depth: 16,
            eq_preset: "Loudness",
            eq_enabled: true,
            tas_fs_hz: 44_100,
            tas_bck_ratio: 64,
            tas_pll_locked: true,
            tas_automuted: false,
            again_code: 0x08,
            settings_seq: 7,
            bt_enabled: false,
            bt_connected: false,
            bt_streaming: false,
            bt_rx_pkts: 0,
            bt_frames: 0,
            decode_us_max: 0,
            decode_us_first: 0,
        };
        // Task 10: `i2c_nack_by_op` pushed a typical response past 1,024 bytes;
        // the device response buffer and HTTP TX buffer are 1,536 bytes.
        let mut out_buf = [0u8; 1536];
        let (len, static_body, action) = handle_http_request_streaming(
            b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n",
            &snap,
            &mut out_buf,
        )
        .expect("status.json should fit in the 1,536-byte device buffer");
        assert_eq!(action, HttpControlAction::None);
        assert!(static_body.is_empty());
        assert!(
            len > 600 && len <= 1536,
            "expected 600 < len <= 1536, got {len}"
        );
        let resp = core::str::from_utf8(&out_buf[..len]).expect("valid utf8");
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("\"eq_preset\":\"Loudness\""));
        assert!(resp.contains("\"settings_seq\":7"));
        assert!(resp.contains(
            "\"i2c_nacks\":5,\"i2c_nack_by_op\":{\"read\":1,\"write\":3,\"write_read\":1},"
        ));
        assert!(resp.contains("\"bt_enabled\":false"));
        assert!(resp.contains("\"bt_connected\":false"));
        assert!(resp.contains("\"bt_streaming\":false"));
        assert!(resp.contains("\"bt_rx_pkts\":0"));
        assert!(resp.contains("\"bt_frames\":0"));
    }

    #[test]
    fn handles_spotify_zeroconf_get_info() {
        let snap = HttpStatusSnapshot::default();
        let mut out_buf = [0u8; 1024];
        let (len, static_body, action) = handle_http_request_streaming(
            b"GET /?action=getInfo HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n",
            &snap,
            &mut out_buf,
        )
        .expect("getInfo should fit in 1024-byte buffer");
        assert_eq!(action, HttpControlAction::None);
        assert!(static_body.is_empty());
        let resp = core::str::from_utf8(&out_buf[..len]).expect("valid utf8");
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("\"status\":101"));
        assert!(resp.contains("\"statusString\":\"OK\""));
        assert!(resp.contains("\"deviceID\":\"142137fd329622137a149016c82e18beb834\""));
        assert!(resp.contains("\"remoteName\":\"BASEMENT\""));
        assert!(resp.contains("\"publicKey\":"));
    }

    #[test]
    fn handles_spotify_cmd_actions() {
        let snap = HttpStatusSnapshot::default();
        let mut out_buf = [0u8; 512];
        let (_len, _body, act_play) = handle_http_request_streaming(
            b"GET /cmd.json?act=spotify_play HTTP/1.1\r\n\r\n",
            &snap,
            &mut out_buf,
        )
        .expect("spotify_play");
        assert_eq!(act_play, HttpControlAction::SpotifyPlay);

        let (_len, _body, act_stop) = handle_http_request_streaming(
            b"GET /cmd.json?act=spotify_stop HTTP/1.1\r\n\r\n",
            &snap,
            &mut out_buf,
        )
        .expect("spotify_stop");
        assert_eq!(act_stop, HttpControlAction::SpotifyStop);
    }

    #[test]
    fn test_phase3_rest_endpoints_config_scan_connect_messages_commands_flash() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 2048];

        // 1. GET /config.json & POST /config.json
        let (clen, cact) = handle_http_control_request(
            b"GET /config.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("GET /config.json");
        assert_eq!(cact, HttpControlAction::None);
        let cstr = core::str::from_utf8(&buf[..clen]).expect("utf8");
        assert!(cstr.contains("\"settings_seq\":1"));
        assert!(cstr.contains("\"ssid\":\"iot\""));

        let (ulen, uact) = handle_http_control_request(
            b"POST /config.json HTTP/1.1\r\n\r\nkey=player_name&value=Kitchen",
            &snap,
            &mut buf,
        )
        .expect("POST /config.json");
        assert_eq!(uact, HttpControlAction::UpdateConfig);
        let ustr = core::str::from_utf8(&buf[..ulen]).expect("utf8");
        assert!(ustr.contains("\"ok\":true,\"key\":\"player_name\",\"value\":\"Kitchen\""));

        // 2. GET /scan.json & POST /scan.json
        let (slen, sact) = handle_http_control_request(
            b"POST /scan.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("POST /scan.json");
        assert_eq!(sact, HttpControlAction::TriggerWifiScan);
        let sstr = core::str::from_utf8(&buf[..slen]).expect("utf8");
        assert!(sstr.contains("\"aps\":[{\"ssid\":\"iot\""));

        // 3. GET /connect.json & POST /connect.json
        let (klen, kact) = handle_http_control_request(
            b"POST /connect.json HTTP/1.1\r\n\r\nssid=HomeWiFi&password=SecretPass123",
            &snap,
            &mut buf,
        )
        .expect("POST /connect.json");
        assert_eq!(kact, HttpControlAction::ConnectWifi);
        let kstr = core::str::from_utf8(&buf[..klen]).expect("utf8");
        assert!(kstr.contains("\"status\":\"connecting\",\"ssid\":\"HomeWiFi\""));

        // 4. GET /messages.json
        let (mlen, mact) = handle_http_control_request(
            b"GET /messages.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("GET /messages.json");
        assert_eq!(mact, HttpControlAction::None);
        let mstr = core::str::from_utf8(&buf[..mlen]).expect("utf8");
        assert!(mstr.contains("\"messages\":[{\"seq\":1"));

        // 5. GET /commands.json & POST /commands.json
        let (xlen, xact) = handle_http_control_request(
            b"GET /commands.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("GET /commands.json");
        assert_eq!(xact, HttpControlAction::None);
        let xstr = core::str::from_utf8(&buf[..xlen]).expect("utf8");
        assert!(xstr.contains("\"commands\":[{\"name\":\"system\""));

        let (elen, eact) = handle_http_control_request(
            b"POST /commands.json HTTP/1.1\r\n\r\ncmd=system%20info",
            &snap,
            &mut buf,
        )
        .expect("POST /commands.json");
        assert_eq!(eact, HttpControlAction::ExecuteCliCommand);
        let estr = core::str::from_utf8(&buf[..elen]).expect("utf8");
        assert!(estr.contains("\"ok\":true,\"cmd\":\"system%20info\""));

        // 6. POST /flash.json
        let (flen, fact) = handle_http_control_request(
            b"POST /flash.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("POST /flash.json");
        assert_eq!(fact, HttpControlAction::StartOtaFlash);
        let fstr = core::str::from_utf8(&buf[..flen]).expect("utf8");
        assert!(fstr.contains("\"ota_state\":\"starting_ota\""));
    }

    #[test]
    fn test_sec_502_percent_decode_and_json_whitespace() {
        let mut dst = [0u8; 64];
        let len = percent_decode_into("My%20Home+Network%21%2B", &mut dst).expect("decode");
        let decoded = core::str::from_utf8(&dst[..len]).expect("utf8");
        assert_eq!(decoded, "My Home Network!+");

        let p1 = extract_param_or_json_field("{\"key\" : \"val1\"}", "key");
        assert_eq!(p1, Some("val1"));
        let p2 = extract_param_or_json_field("{\"key\": \"val2\"}", "key");
        assert_eq!(p2, Some("val2"));
        let p3 = extract_param_or_json_field("{\"key\":\"val3\"}", "key");
        assert_eq!(p3, Some("val3"));
        let p4 = extract_param_or_json_field("key=val4&other=1", "key");
        assert_eq!(p4, Some("val4"));
    }

    #[test]
    fn test_sec_503_deterministic_dh_private_key_derivation() {
        let snap1 = HttpStatusSnapshot {
            ip: [192, 168, 1, 100],
            uptime_ms: 42000,
            ..HttpStatusSnapshot::default()
        };
        let key1 = derive_zeroconf_dh_private_key(&snap1);
        assert_ne!(key1, [0x42u8; 96]);
        assert_ne!(key1, [0u8; 96]);
        assert!(OakleyGroup1Dh::from_private_key(key1).is_ok());

        // Invariance across uptime changes (NET-403)
        let mut snap_uptime = snap1.clone();
        snap_uptime.uptime_ms = 999999;
        let key_uptime = derive_zeroconf_dh_private_key(&snap_uptime);
        assert_eq!(key1, key_uptime);

        // Different IP derives different DH key
        let mut snap2 = snap1.clone();
        snap2.ip = [192, 168, 1, 101];
        let key2 = derive_zeroconf_dh_private_key(&snap2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_logic_502_scan_table_and_syslog_ring_json_helpers() {
        use crate::console::{LogLevel, SyslogRingBuffer};
        use crate::wifi::{ScannedAp, WifiAuthMode, WifiScanTable};

        let mut table = WifiScanTable::new();
        let ap = ScannedAp::new(
            "TestNet",
            [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC],
            -60,
            11,
            WifiAuthMode::Wpa2Psk,
        )
        .expect("scanned ap");
        table.upsert(ap);

        let mut out = [0u8; 512];
        let len = write_scan_table_json(&table, &mut out).expect("write scan table");
        let s = core::str::from_utf8(&out[..len]).expect("utf8");
        assert!(s.contains("\"aps\":[{\"ssid\":\"TestNet\""));
        assert!(s.contains("\"bssid\":\"12:34:56:78:9A:BC\""));
        assert!(s.contains("\"auth\":\"WPA2_PSK\""));

        let mut ring = SyslogRingBuffer::<4>::new();
        ring.push(1234, LogLevel::Warn, "TAS5805M clip warning");
        let mlen = write_syslog_ring_json(&ring, &mut out).expect("write syslog ring");
        let ms = core::str::from_utf8(&out[..mlen]).expect("utf8");
        assert!(ms.contains("\"messages\":[{\"seq\":1"));
        assert!(ms.contains("\"level\":\"WARN\""));
        assert!(ms.contains("\"msg\":\"TAS5805M clip warning\""));
    }

    #[test]
    fn test_sec_504_write_json_http_response_bounds_check() {
        let mut small_buf = [0u8; 100];
        assert_eq!(write_json_http_response(10, &mut small_buf), None);

        let mut ok_buf = [0u8; 256];
        assert!(write_json_http_response(20, &mut ok_buf).is_some());
    }

    #[test]
    fn test_get_eq_json_endpoint() {
        let snap = HttpStatusSnapshot {
            eq_preset: "Bass",
            eq_enabled: true,
            again_code: 0x0C,
            vol_pct: 75,
            ..HttpStatusSnapshot::default()
        };
        let mut buf = [0u8; 512];
        let req = b"GET /eq.json HTTP/1.1\r\n\r\n";
        let (len, act) = handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(act, HttpControlAction::None);
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("\"ok\":true"));
        assert!(resp.contains("\"eq_preset\":\"Bass\""));
        assert!(resp.contains("\"eq_enabled\":true"));
        assert!(resp.contains("\"again_code\":12"));
        assert!(resp.contains("\"vol_pct\":75"));
    }

    #[test]
    fn test_percent_decoding_key_and_password() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 1024];

        // 1. POST /config.json with percent-encoded key
        let req = b"POST /config.json HTTP/1.1\r\n\r\nkey=player%20name&value=Kitchen%20HiFi";
        let (len, act) = handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(act, HttpControlAction::UpdateConfig);
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"key\":\"player name\""));
        assert!(resp.contains("\"value\":\"Kitchen HiFi\""));

        // 2. POST /connect.json with percent-encoded password
        let req_conn =
            b"POST /connect.json HTTP/1.1\r\n\r\nssid=Home%20Net&password=My%20Secret%21";
        let (clen, cact) = handle_http_control_request(req_conn, &snap, &mut buf).unwrap();
        assert_eq!(cact, HttpControlAction::ConnectWifi);
        let cresp = core::str::from_utf8(&buf[..clen]).unwrap();
        assert!(cresp.contains("\"ssid\":\"Home Net\""));
    }

    #[test]
    fn test_derive_zeroconf_dh_private_key_with_entropy() {
        let ip = [192, 168, 1, 50];
        let entropy = [1, 2, 3, 4, 5, 6, 7, 8];
        let key1 = derive_zeroconf_dh_private_key_with_entropy(ip, entropy);
        let key2 = derive_zeroconf_dh_private_key_with_entropy(ip, entropy);
        assert_eq!(key1, key2);
        assert_eq!(key1[95] & 0x01, 1);
        assert!(OakleyGroup1Dh::from_private_key(key1).is_ok());

        let key3 = derive_zeroconf_dh_private_key_with_entropy(ip, [8, 7, 6, 5, 4, 3, 2, 1]);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_rejects_get_on_recovery_and_reboot_and_supports_auth() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 2048];

        // GET /recovery.json rejected with 405
        let (len1, act1) = handle_http_control_request(
            b"GET /recovery.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .unwrap();
        assert_eq!(act1, HttpControlAction::None);
        let resp1 = core::str::from_utf8(&buf[..len1]).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

        // GET /reboot.json rejected with 405
        let (len2, act2) = handle_http_control_request(
            b"GET /reboot.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .unwrap();
        assert_eq!(act2, HttpControlAction::None);
        let resp2 = core::str::from_utf8(&buf[..len2]).unwrap();
        assert!(resp2.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

        // Authenticated router rejects missing token
        let router = HttpControlRouter::with_auth_token("secret_token_123");
        let (len3, _) = router.handle_request(
            b"GET /status.json HTTP/1.1\r\n\r\n",
            &snap,
            &mut buf,
        )
        .unwrap();
        let resp3 = core::str::from_utf8(&buf[..len3]).unwrap();
        assert!(resp3.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

        // Authenticated router accepts valid header
        let (len4, _) = router.handle_request(
            b"GET /status.json HTTP/1.1\r\nAuthorization: Bearer secret_token_123\r\n\r\n",
            &snap,
            &mut buf,
        )
        .unwrap();
        let resp4 = core::str::from_utf8(&buf[..len4]).unwrap();
        assert!(resp4.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn test_commands_json_redacts_passwords() {
        let snap = HttpStatusSnapshot::default();
        let mut buf = [0u8; 1024];

        let req = b"POST /commands.json HTTP/1.1\r\n\r\ncmd=join%20MyWiFi%20SuperSecretPass";
        let (len, act) = handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(act, HttpControlAction::ExecuteCliCommand);
        let resp = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(resp.contains("\"cmd\":\"join%20MyWiFi%20***\""));

        let req2 = b"POST /commands.json HTTP/1.1\r\n\r\ncmd=config set wifi.pass SuperSecretPass";
        let (len2, act2) = handle_http_control_request(req2, &snap, &mut buf).unwrap();
        assert_eq!(act2, HttpControlAction::ExecuteCliCommand);
        let resp2 = core::str::from_utf8(&buf[..len2]).unwrap();
        assert!(resp2.contains("\"cmd\":\"config set wifi.pass ***\""));
    }
}
