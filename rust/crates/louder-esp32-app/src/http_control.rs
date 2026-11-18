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

extern crate alloc;

use core::cell::RefCell;
use core::fmt::Write;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use heapless::String;

use squeezelite_cspot::crypto::hmac_sha1;
use squeezelite_cspot::crypto::OakleyGroup1Dh;
use squeezelite_cspot::zeroconf::ZeroConfServer;

use crate::ota_safety::{REQUIRED_OTA_PROJECT_NAME, RUST_OTA_FIRMWARE_VERSION};
use crate::pipeline::{PSRAM_HEAP_BYTES, PSRAM_OUTPUT_RING_BYTES, PSRAM_STREAM_RING_BYTES};

/// `psram_used_bytes` reported when no PSRAM heap statistics exist: the
/// firmware's two PSRAM rings (512 KiB stream + 2000 KiB output = 2,572,288 B).
pub const DEFAULT_PSRAM_USED_BYTES: u32 =
    (PSRAM_STREAM_RING_BYTES + PSRAM_OUTPUT_RING_BYTES) as u32;
/// `psram_free_bytes` to match [`DEFAULT_PSRAM_USED_BYTES`] in the 4 MiB
/// region (1,622,016 B).
pub const DEFAULT_PSRAM_FREE_BYTES: u32 = PSRAM_HEAP_BYTES as u32 - DEFAULT_PSRAM_USED_BYTES;

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
    /// Disconnects from active Wi-Fi AP (`DELETE /connect.json`).
    DisconnectWifi,
    /// Applies a key-value configuration mutation (`/config.json`).
    UpdateConfig,
    /// Dispatches a console CLI command (`/commands.json`).
    ExecuteCliCommand,
    /// Initiates streaming OTA firmware update (`/flash.json`).
    StartOtaFlash,
}

/// Standard HTTP request methods recognized by the Port 80 HTTP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET method
    Get,
    /// POST method
    Post,
    /// DELETE method
    Delete,
    /// Other / unsupported HTTP method
    Other,
}

/// Parses an HTTP method string into [`HttpMethod`].
#[must_use]
pub fn parse_http_method(method: &str) -> HttpMethod {
    let verb = method.split_whitespace().next().unwrap_or(method);
    match verb {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "DELETE" => HttpMethod::Delete,
        _ => HttpMethod::Other,
    }
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
    /// Whether headphone jack is inserted (`"Jack"` in C `/status.json`).
    pub jack_inserted: bool,
    /// Supply/battery voltage in millivolts (`"Voltage"` in C `/status.json`).
    pub voltage_mv: u16,
    /// Wi-Fi station disconnect count (`"disconnect_count"` in C `/status.json`).
    pub disconnect_count: u32,
    /// Average Wi-Fi association duration in milliseconds (`"avg_conn_time"`).
    pub avg_conn_time_ms: u32,
    /// Whether the I2C bus is locked by a peripheral (`"is_i2c_locked"`).
    pub is_i2c_locked: bool,
    /// Wi-Fi connection reason/result code (`"urc"`, `0` when connected).
    pub urc: u8,
    /// C Bluetooth status (`0` = idle, `1` = connected, `2` = streaming).
    pub bt_status: u8,
    /// C-compatible Bluetooth sub-status code (`"bt_sub_status"`).
    pub bt_sub_status: u8,
    /// Active 10-band equalizer band gains in dB (-12..=+12).
    pub eq_bands_db: [i8; 10],
    /// Squeezelite player name / hostname override.
    pub player_name: &'a str,
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
            jack_inserted: false,
            voltage_mv: 0,
            disconnect_count: 0,
            avg_conn_time_ms: 0,
            is_i2c_locked: false,
            urc: 0,
            bt_status: 0,
            bt_sub_status: 0,
            eq_bands_db: [0; 10],
            player_name: crate::board::first_boot_defaults::PLAYER_NAME,
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

    fn remaining_len(&self) -> usize {
        self.buf.len().saturating_sub(self.offset)
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
    let jack_str = if snapshot.jack_inserted { "1" } else { "0" };
    let bt_status = if snapshot.bt_status != 0 {
        snapshot.bt_status
    } else if snapshot.bt_streaming {
        2
    } else if snapshot.bt_connected {
        1
    } else {
        0
    };
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
        \"bt_frames\":{},\"decode_us_max\":{},\"decode_us_first\":{},\
        \"Jack\":\"{}\",\"Voltage\":{},\"disconnect_count\":{},\
        \"avg_conn_time\":{},\"is_i2c_locked\":{},\"urc\":{},\
        \"bt_status\":{},\"bt_sub_status\":{}}}",
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
        jack_str,
        snapshot.voltage_mv,
        snapshot.disconnect_count,
        snapshot.avg_conn_time_ms,
        snapshot.is_i2c_locked,
        snapshot.urc,
        bt_status,
        snapshot.bt_sub_status,
    )
    .ok()?;
    Some(writer.written_len())
}

/// Formats the `/status.json` payload into `out_buf` (up to 2048 bytes).
#[inline]
pub fn format_status_json(snapshot: &HttpStatusSnapshot<'_>, out_buf: &mut [u8]) -> Option<usize> {
    write_status_json(snapshot, out_buf)
}

/// Fixed hardware GPIO pin descriptor exposed in `GET /config.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioPinDescriptor {
    /// Physical ESP32 GPIO number.
    pub gpio: u8,
    /// Signal name (`"SDA"`, `"SCL"`, `"BCK"`, `"WS"`, `"DO"`, `"PDN"`).
    pub name: &'static str,
    /// Subsystem group (`"I2C"`, `"I2S"`, `"TAS"`).
    pub group: &'static str,
    /// Whether the pin assignment is hardware-fixed on the PCB.
    pub fixed: bool,
}

/// Louder-ESP32 hardware GPIO map serialized in `GET /config.json`.
pub const BOARD_GPIO_MAP: &[GpioPinDescriptor] = &[
    GpioPinDescriptor {
        gpio: 21,
        name: "SDA",
        group: "I2C",
        fixed: true,
    },
    GpioPinDescriptor {
        gpio: 27,
        name: "SCL",
        group: "I2C",
        fixed: true,
    },
    GpioPinDescriptor {
        gpio: 26,
        name: "BCK",
        group: "I2S",
        fixed: true,
    },
    GpioPinDescriptor {
        gpio: 25,
        name: "WS",
        group: "I2S",
        fixed: true,
    },
    GpioPinDescriptor {
        gpio: 22,
        name: "DO",
        group: "I2S",
        fixed: true,
    },
    GpioPinDescriptor {
        gpio: 17,
        name: "PDN",
        group: "TAS",
        fixed: true,
    },
];

/// Default C-compatible NVS entries exposed in `GET /config.json`.
pub const DEFAULT_NVS_CONFIG_ENTRIES: &[(&str, &str, u8)] = &[
    (
        "host_name",
        crate::board::first_boot_defaults::PLAYER_NAME,
        crate::config_handle::NVS_TYPE_STR,
    ),
    ("autoexec", "1", crate::config_handle::NVS_TYPE_U8),
    (
        "autoexec1",
        "squeezelite -o I2S -b 500:2000 -d all=info -C 30 -W",
        crate::config_handle::NVS_TYPE_STR,
    ),
    ("enable_airplay", "1", crate::config_handle::NVS_TYPE_U8),
    ("enable_cspot", "1", crate::config_handle::NVS_TYPE_U8),
    ("enable_bt_sink", "1", crate::config_handle::NVS_TYPE_U8),
    ("enable_telnet", "0", crate::config_handle::NVS_TYPE_U8),
    (
        "dac_config",
        "model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=17:0",
        crate::config_handle::NVS_TYPE_STR,
    ),
    (
        "i2c_config",
        "sda=21,scl=27,speed=400000,port=0",
        crate::config_handle::NVS_TYPE_STR,
    ),
    ("rotary_config", "", crate::config_handle::NVS_TYPE_STR),
];

/// Type alias for `/config.json` snapshot input.
pub type HttpConfigSnapshot<'a> = HttpStatusSnapshot<'a>;

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

/// Formats the `/config.json` payload combining active snapshot settings,
/// `BOARD_GPIO_MAP`, C-typed NVS config entries, and optional `extra_entries`
/// (with sensitive password keys stripped) into `out_buf`.
pub fn write_config_json(
    snapshot: &HttpStatusSnapshot<'_>,
    extra_entries: &[ConfigEntryView<'_>],
    out_buf: &mut [u8],
) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    let find_override =
        |k: &str| -> Option<&str> { extra_entries.iter().find(|e| e.key == k).map(|e| e.value) };
    write!(
        writer,
        "{{\"settings_seq\":{},\"gpio\":[",
        snapshot.settings_seq
    )
    .ok()?;
    for (idx, pin) in BOARD_GPIO_MAP.iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"gpio\":{},\"name\":\"{}\",\"group\":\"{}\",\"fixed\":{}}}",
            pin.gpio,
            JsonEscaped(pin.name),
            JsonEscaped(pin.group),
            pin.fixed,
        )
        .ok()?;
    }
    write!(
        writer,
        "],\"config\":{{\"ssid\":\"{}\",\
         \"lms_ip\":\"{}.{}.{}.{}\",\"lms_port\":{},\"lms_cport\":{},\
         \"vol_pct\":{},\"eq_preset\":\"{}\",\"again_code\":{}",
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
    for &(key, val, nvs_type) in DEFAULT_NVS_CONFIG_ENTRIES {
        if crate::config_handle::is_secret_config_key(key) {
            continue;
        }
        let effective_val = find_override(key).unwrap_or(val);
        write!(
            writer,
            ",\"{}\":{{\"value\":\"{}\",\"type\":{}}}",
            JsonEscaped(key),
            JsonEscaped(effective_val),
            nvs_type,
        )
        .ok()?;
    }
    for entry in extra_entries {
        if crate::config_handle::is_secret_config_key(entry.key) {
            continue;
        }
        if DEFAULT_NVS_CONFIG_ENTRIES
            .iter()
            .any(|&(k, _, _)| k == entry.key)
            || matches!(entry.key, "lms_ip" | "volume" | "again_code")
        {
            continue;
        }
        write!(
            writer,
            ",\"{}\":{{\"value\":\"{}\",\"type\":\"{}\"}}",
            JsonEscaped(entry.key),
            JsonEscaped(entry.value),
            JsonEscaped(entry.entry_type),
        )
        .ok()?;
    }
    let host_name_val =
        find_override("host_name").unwrap_or(crate::board::first_boot_defaults::PLAYER_NAME);
    let autoexec_val = find_override("autoexec").unwrap_or("1");
    let autoexec1_val =
        find_override("autoexec1").unwrap_or("squeezelite -o I2S -b 500:2000 -d all=info -C 30 -W");
    write!(
        writer,
        "}},\"nvs\":{{\"host_name\":{{\"value\":\"{}\",\"type\":33}},\
         \"autoexec\":{{\"value\":\"{}\",\"type\":1}},\
         \"autoexec1\":{{\"value\":\"{}\",\
         \"type\":33}}}}}}",
        JsonEscaped(host_name_val),
        JsonEscaped(autoexec_val),
        JsonEscaped(autoexec1_val),
    )
    .ok()?;
    Some(writer.written_len())
}

/// Formats the `/config.json` payload into `out_buf`.
#[inline]
pub fn format_config_json(snapshot: &HttpConfigSnapshot<'_>, out_buf: &mut [u8]) -> Option<usize> {
    write_config_json(snapshot, &[], out_buf)
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

fn parse_nested_c_config_update(input: &str) -> Option<ParsedConfigUpdate<'_>> {
    let cfg_idx = input.find("\"config\"")?;
    let after_cfg = input[cfg_idx + "\"config\"".len()..].trim_start();
    let after_colon = after_cfg.strip_prefix(':')?.trim_start();
    let inner = after_colon.strip_prefix('{')?.trim_start();
    let after_quote = inner.strip_prefix('"')?;
    let key_end = after_quote.find('"')?;
    let key = &after_quote[..key_end];
    if key.is_empty() {
        return None;
    }
    let entry_rest = &after_quote[key_end + 1..];
    let value = extract_param_or_json_field(entry_rest, "value").unwrap_or("");
    Some(ParsedConfigUpdate { key, value })
}

/// Parses a `key=...&value=...` query/form, flat `{"key":"...","value":"..."}`,
/// nested C `{"config":{"<key>":{"value":"...","type":33}}}`, or `{"fwurl":..}`
/// JSON body for `/config.json`.
#[must_use]
pub fn parse_config_update_query(input: &str) -> Option<ParsedConfigUpdate<'_>> {
    if let Some(key) = extract_param_or_json_field(input, "key") {
        if !key.is_empty() {
            let value = extract_param_or_json_field(input, "value")
                .or_else(|| extract_param_or_json_field(input, "val"))
                .unwrap_or("");
            return Some(ParsedConfigUpdate { key, value });
        }
    }
    if let Some(nested) = parse_nested_c_config_update(input) {
        return Some(nested);
    }
    if let Some(fwurl) = extract_param_or_json_field(input, "fwurl") {
        if !fwurl.is_empty() {
            return Some(ParsedConfigUpdate {
                key: "fwurl",
                value: fwurl,
            });
        }
    }
    if let Some(eq) = extract_param_or_json_field(input, "equalizer") {
        if !eq.is_empty() {
            return Some(ParsedConfigUpdate {
                key: "equalizer",
                value: eq,
            });
        }
    }
    None
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
    pub const fn new(ssid: &'a str, bssid: [u8; 6], rssi: i8, channel: u8, auth: &'a str) -> Self {
        Self {
            ssid,
            bssid,
            rssi,
            channel,
            auth,
        }
    }
}

/// Scanned 802.11 access point entry for C-compatible `/ap.json` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScannedApEntry {
    /// Broadcast SSID raw bytes.
    pub ssid: [u8; 32],
    /// Valid SSID byte length (`0..=32`).
    pub ssid_len: u8,
    /// 2.4 GHz channel (`1..=14`).
    pub chan: u8,
    /// Signal strength in dBm.
    pub rssi: i8,
    /// Authentication mode code (e.g. `0` = Open, `3` = WPA2_PSK).
    pub auth: u8,
    /// Whether this access point is a pre-configured known network.
    pub known: bool,
}

/// Formats the C-compatible AP list JSON array into `out`.
pub fn format_ap_list_json(
    aps: &[ScannedApEntry],
    current_ssid: &str,
    current_rssi: i8,
    out: &mut [u8],
) -> usize {
    let mut writer = BufferWriter::new(out);
    let _ = writer.write_str("[");
    let mut first = true;
    let mut current_found = false;

    for ap in aps {
        let ssid_len = (ap.ssid_len as usize).min(ap.ssid.len());
        let ssid_str = core::str::from_utf8(&ap.ssid[..ssid_len]).unwrap_or_default();
        if ssid_str == current_ssid {
            current_found = true;
        }
        if !first {
            let _ = writer.write_str(",");
        }
        first = false;
        let _ = write!(
            writer,
            "{{\"ssid\":\"{}\",\"chan\":{},\"rssi\":{},\"auth\":{},\"known\":{}}}",
            JsonEscaped(ssid_str),
            ap.chan,
            ap.rssi,
            ap.auth,
            ap.known,
        );
    }

    if !current_found && !current_ssid.is_empty() {
        if !first {
            let _ = writer.write_str(",");
        }
        let _ = write!(
            writer,
            "{{\"ssid\":\"{}\",\"chan\":6,\"rssi\":{},\"auth\":3,\"known\":true}}",
            JsonEscaped(current_ssid),
            current_rssi,
        );
    }

    let _ = writer.write_str("]");
    writer.written_len()
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
    /// Optional device hostname override.
    pub host_name: Option<&'a str>,
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
    let host_name = extract_param_or_json_field(input, "host_name")
        .or_else(|| extract_param_or_json_field(input, "hostname"));
    Some(ConnectRequest {
        ssid,
        password,
        static_ip,
        host_name,
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
    let status_label = if connected {
        "connected"
    } else {
        "disconnected"
    };
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

/// Fixed-capacity entry stored in [`SyslogRingBuffer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyslogRingEntry {
    /// Monotonic message sequence number.
    pub seq: u32,
    /// Uptime timestamp in milliseconds.
    pub uptime_ms: u64,
    /// Severity label (`"INFO"`, `"WARN"`, `"ERROR"`).
    pub level: String<16>,
    /// C messaging class (`"MESSAGING_CLASS_SYSTEM"`, `"MESSAGING_CLASS_STATS"`).
    pub msg_class: String<32>,
    /// Log message text.
    pub msg: String<128>,
}

/// Fixed-capacity 16-entry ring buffer storing `seq`, `uptime_ms`, `level`,
/// `msg_class`, and `msg` for `/messages.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyslogRingBuffer {
    entries: [Option<SyslogRingEntry>; 16],
    head: usize,
    len: usize,
    next_seq: u32,
}

impl Default for SyslogRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl SyslogRingBuffer {
    /// Fixed ring capacity (16 entries).
    pub const CAPACITY: usize = 16;

    /// Creates an empty [`SyslogRingBuffer`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; 16],
            head: 0,
            len: 0,
            next_seq: 1,
        }
    }

    /// Appends a message to the ring buffer, overwriting the oldest when full.
    pub fn push(&mut self, uptime_ms: u64, level: &str, msg_class: &str, msg: &str) {
        let mut lvl = String::new();
        let _ = lvl.push_str(level);
        let mut cls = String::new();
        let _ = cls.push_str(msg_class);
        let mut text = String::new();
        let _ = text.push_str(msg);
        let entry = SyslogRingEntry {
            seq: self.next_seq,
            uptime_ms,
            level: lvl,
            msg_class: cls,
            msg: text,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        if self.len < Self::CAPACITY {
            let write_idx = (self.head + self.len) % Self::CAPACITY;
            self.entries[write_idx] = Some(entry);
            self.len += 1;
        } else {
            self.entries[self.head] = Some(entry);
            self.head = (self.head + 1) % Self::CAPACITY;
        }
    }

    /// Returns the number of stored entries (`0..=16`).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the ring buffer contains no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Retrieves an entry in chronological order (`0..self.len()`).
    #[must_use]
    pub fn entry(&self, idx: usize) -> Option<&SyslogRingEntry> {
        if idx >= self.len {
            return None;
        }
        let pos = (self.head + idx) % Self::CAPACITY;
        self.entries[pos].as_ref()
    }

    /// Returns the most recently pushed log message string slice, or `""` if empty.
    #[must_use]
    pub fn latest_message(&self) -> &str {
        if self.len == 0 {
            return "";
        }
        self.entry(self.len - 1).map_or("", |e| e.msg.as_str())
    }
}

/// The global web syslog ring. Only the pointer is static: the ring itself
/// (about 3.3 KiB) is allocated on first use, which on the device is the PSRAM
/// heap region, so it costs no internal RAM.
static WEB_SYSLOG_RING: Mutex<
    CriticalSectionRawMutex,
    RefCell<Option<alloc::boxed::Box<SyslogRingBuffer>>>,
> = Mutex::new(RefCell::new(None));

/// Runs `f` on the global web syslog ring, allocating it on first use.
fn with_syslog_ring<R>(f: impl FnOnce(&mut SyslogRingBuffer) -> R) -> R {
    WEB_SYSLOG_RING.lock(|cell| {
        let mut slot = cell.borrow_mut();
        f(slot.get_or_insert_with(|| alloc::boxed::Box::new(SyslogRingBuffer::new())))
    })
}

/// Appends a runtime event to the global web syslog ring buffer. Only the
/// first call allocates (the ring itself); it must come after the heap is up.
pub fn push_runtime_syslog(level: &str, msg_class: &str, msg: &str, timestamp_ms: u64) {
    with_syslog_ring(|ring| ring.push(timestamp_ms, level, msg_class, msg));
}

/// Appends a runtime event to the web syslog ring buffer (alias for
/// [`push_runtime_syslog`]).
pub fn push_syslog_entry(level: &str, msg_class: &str, msg: &str, timestamp_ms: u64) {
    push_runtime_syslog(level, msg_class, msg, timestamp_ms);
}

/// Applies a configuration key-value mutation from `/config.json` to
/// [`crate::config_handle::ConfigHandle`] and logs the update to `syslog`.
pub fn apply_web_config_update(
    key: &str,
    value: &str,
    uptime_ms: u64,
    config: &mut crate::config_handle::ConfigHandle,
    syslog: &mut SyslogRingBuffer,
) -> bool {
    let mut k_buf = [0u8; 64];
    let decoded_key = percent_decode_into(key, &mut k_buf)
        .and_then(|n| core::str::from_utf8(&k_buf[..n]).ok())
        .unwrap_or(key)
        .trim();
    if decoded_key.is_empty() {
        return false;
    }
    let mut v_buf = [0u8; 128];
    let decoded_val = percent_decode_into(value, &mut v_buf)
        .and_then(|n| core::str::from_utf8(&v_buf[..n]).ok())
        .unwrap_or(value)
        .trim();

    let applied = config.apply_key_value(decoded_key, decoded_val);
    let shown = if crate::config_handle::is_secret_config_key(decoded_key) {
        "***"
    } else {
        decoded_val
    };
    let mut msg: String<128> = String::new();
    let _ = write!(msg, "config: {}={}", decoded_key, shown);
    syslog.push(uptime_ms, "INFO", "MESSAGING_CLASS_SYSTEM", msg.as_str());
    applied
}

/// Executes a web console CLI command (`/commands.json`), updating `config`
/// when `cfg-*` or `config set` mutations are invoked and appending diagnostic
/// output to `syslog`.
pub fn execute_web_cli_command(
    cmd: &str,
    snap: &HttpStatusSnapshot<'_>,
    config: &mut crate::config_handle::ConfigHandle,
    syslog: &mut SyslogRingBuffer,
) -> bool {
    let mut decoded_buf = [0u8; 128];
    let clean = percent_decode_into(cmd, &mut decoded_buf)
        .and_then(|n| core::str::from_utf8(&decoded_buf[..n]).ok())
        .unwrap_or(cmd)
        .trim();
    if clean.is_empty() {
        return false;
    }

    let mut parts = clean.split_whitespace();
    let verb = parts.next().unwrap_or("");
    let mut msg: String<128> = String::new();

    match verb {
        "free" | "heap" => {
            let _ = write!(
                msg,
                "PSRAM free: {} B, used: {} B",
                snap.psram_free_bytes, snap.psram_used_bytes
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "version" => {
            let _ = write!(
                msg,
                "{} {} (seq {})",
                REQUIRED_OTA_PROJECT_NAME, RUST_OTA_FIRMWARE_VERSION, snap.settings_seq
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "i2cdetect" => {
            let _ = write!(
                msg,
                "I2C0 scan: 0x2D (TAS5805M) ACK={} NACK={}",
                snap.i2c_acks, snap.i2c_nacks
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "i2cget" | "i2cset" | "i2cdump" | "i2c" => {
            let _ = write!(
                msg,
                "I2C0 0x2D: mode={} again=0x{:02X} ACK={} NACK={}",
                snap.amp_mode, snap.again_code, snap.i2c_acks, snap.i2c_nacks
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "system" | "restart" | "recovery" | "restart_ota" => {
            let _ = write!(
                msg,
                "system: up={}ms ip={}.{}.{}.{} PSRAM free: {} B",
                snap.uptime_ms,
                snap.ip[0],
                snap.ip[1],
                snap.ip[2],
                snap.ip[3],
                snap.psram_free_bytes
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "config" => {
            let sub = parts.next().unwrap_or("dump");
            if sub == "set" {
                let k = parts.next().unwrap_or("");
                let v = parts.next().unwrap_or("");
                if !k.is_empty() {
                    let _ = config.apply_key_value(k, v);
                }
            }
            let _ = write!(
                msg,
                "config: host={} seq={} vol={}% eq={}",
                config.host_name_str(),
                snap.settings_seq,
                snap.vol_pct,
                snap.eq_preset
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "join" => {
            let target_ssid = parts.next().unwrap_or(snap.ssid);
            let _ = write!(msg, "join: ssid={} (credentials staged)", target_ssid);
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "update" => {
            let _ = write!(msg, "update: OTA target ota_0_0x160000 ready");
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "player" => {
            let _ = write!(
                msg,
                "player: mode={} vol={}% frames={} codec={}",
                snap.amp_mode, snap.vol_pct, snap.frames_played, snap.codec
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "sleep" => {
            let _ = write!(msg, "sleep: standby armed (amp={})", snap.amp_mode);
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        "help" => {
            let _ = write!(
                msg,
                "commands: system i2c config join update player sleep free heap version i2cdetect"
            );
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        v if v.starts_with("cfg-") => {
            let mut prev_flag: Option<&str> = None;
            for tok in parts {
                if let Some(flag) = tok.strip_prefix("--") {
                    if let Some((k, val)) = flag.split_once('=') {
                        let _ = config.apply_key_value(k, val);
                        prev_flag = None;
                    } else {
                        prev_flag = Some(flag);
                    }
                } else if let Some(flag) = prev_flag.take() {
                    let _ = config.apply_key_value(flag, tok);
                } else if let Some((k, val)) = tok.split_once('=') {
                    let _ = config.apply_key_value(k, val);
                } else if v == "cfg-syst-name" {
                    let _ = config.set_host_name(tok);
                }
            }
            let _ = write!(msg, "{}: OK (host_name={})", v, config.host_name_str());
            syslog.push(
                snap.uptime_ms,
                "INFO",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            true
        }
        _ => {
            let _ = write!(msg, "unknown command: {}", verb);
            syslog.push(
                snap.uptime_ms,
                "WARN",
                "MESSAGING_CLASS_SYSTEM",
                msg.as_str(),
            );
            false
        }
    }
}

fn extract_raw_path_from_req(req_bytes: &[u8]) -> &str {
    let line_end = req_bytes
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(req_bytes.len());
    let first_line = core::str::from_utf8(&req_bytes[..line_end]).unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let _ = parts.next();
    parts.next().unwrap_or("")
}

/// Dispatches `UpdateConfig`, `ExecuteCliCommand`, `TriggerWifiScan`,
/// `ConnectWifi`, `DisconnectWifi`, and `StartOtaFlash` side effects from an
/// HTTP request, persisting changes to `config` and `journal` and recording
/// diagnostic entries in the web syslog ring buffer.
pub fn dispatch_web_post_action(
    action: HttpControlAction,
    req_bytes: &[u8],
    snap: &HttpStatusSnapshot<'_>,
    config: &crate::config_handle::ConfigHandle,
    journal: &mut crate::config_handle::RamJournal<'_>,
) -> Option<crate::ota_safety::WifiStaCredentials> {
    let raw_path = extract_raw_path_from_req(req_bytes);
    let payload = extract_query_or_body(raw_path, req_bytes);
    let mut new_wifi: Option<crate::ota_safety::WifiStaCredentials> = None;

    with_syslog_ring(|syslog| {
        let mut cfg_tmp = config.clone_handle();
        match action {
            HttpControlAction::UpdateConfig => {
                if let Some(upd) = parse_config_update_query(payload) {
                    let _ = apply_web_config_update(
                        upd.key,
                        upd.value,
                        snap.uptime_ms,
                        &mut cfg_tmp,
                        &mut *syslog,
                    );
                    let _ = config.sync_from(&cfg_tmp);
                }
                let _ = config.flush_now(journal);
            }
            HttpControlAction::ExecuteCliCommand => {
                let cmd_str = extract_param_or_json_field(payload, "cmd")
                    .or_else(|| extract_param_or_json_field(payload, "command"))
                    .unwrap_or("");
                if execute_web_cli_command(cmd_str, snap, &mut cfg_tmp, &mut *syslog) {
                    let _ = config.sync_from(&cfg_tmp);
                    let _ = config.flush_now(journal);
                }
            }
            HttpControlAction::TriggerWifiScan => {
                let mut msg: String<128> = String::new();
                let _ = write!(
                    msg,
                    "wifi scan: active ssid={} rssi={} dBm",
                    snap.ssid, snap.rssi
                );
                syslog.push(
                    snap.uptime_ms,
                    "INFO",
                    "MESSAGING_CLASS_SYSTEM",
                    msg.as_str(),
                );
            }
            HttpControlAction::ConnectWifi => {
                if let Some(req) = parse_connect_request(payload) {
                    let mut s_buf = [0u8; 64];
                    let dec_ssid = percent_decode_into(req.ssid, &mut s_buf)
                        .and_then(|n| core::str::from_utf8(&s_buf[..n]).ok())
                        .unwrap_or(req.ssid);
                    let mut p_buf = [0u8; 64];
                    let dec_pass = percent_decode_into(req.password, &mut p_buf)
                        .and_then(|n| core::str::from_utf8(&p_buf[..n]).ok())
                        .unwrap_or(req.password);
                    if let Some(hn) = req.host_name {
                        let _ = cfg_tmp.set_host_name(hn);
                        let _ = config.sync_from(&cfg_tmp);
                    }
                    let mut creds = crate::ota_safety::WifiStaCredentials {
                        ssid: String::new(),
                        password: String::new(),
                    };
                    let s_len = dec_ssid.floor_char_boundary(dec_ssid.len().min(32));
                    let p_len = dec_pass.floor_char_boundary(dec_pass.len().min(64));
                    let _ = creds.ssid.push_str(&dec_ssid[..s_len]);
                    let _ = creds.password.push_str(&dec_pass[..p_len]);
                    journal.set_pending_wifi(Some(creds.clone()));
                    let _ = config.flush_now(journal);
                    new_wifi = Some(creds);
                    let mut msg: String<128> = String::new();
                    let _ = write!(msg, "wifi connect: ssid={}", &dec_ssid[..s_len]);
                    syslog.push(
                        snap.uptime_ms,
                        "INFO",
                        "MESSAGING_CLASS_SYSTEM",
                        msg.as_str(),
                    );
                }
            }
            HttpControlAction::DisconnectWifi => {
                journal.set_pending_wifi(None);
                syslog.push(
                    snap.uptime_ms,
                    "INFO",
                    "MESSAGING_CLASS_SYSTEM",
                    "wifi disconnect requested",
                );
            }
            HttpControlAction::StartOtaFlash => {
                let _ = config.flush_now(journal);
                syslog.push(
                    snap.uptime_ms,
                    "INFO",
                    "MESSAGING_CLASS_SYSTEM",
                    "OTA flash started: target ota_0_0x160000",
                );
            }
            _ => {}
        }
    });

    new_wifi
}

/// Maps a severity label to the C web UI `MESSAGING_*` constant.
#[must_use]
pub fn messaging_type_for_level(level: &str) -> &'static str {
    if level.eq_ignore_ascii_case("ERROR") || level.eq_ignore_ascii_case("ERR") {
        "MESSAGING_ERROR"
    } else if level.eq_ignore_ascii_case("WARN") || level.eq_ignore_ascii_case("WARNING") {
        "MESSAGING_WARNING"
    } else {
        "MESSAGING_INFO"
    }
}

/// Formats `/messages.json` with both `"messages": [...]` and C-compatible
/// `type` (`MESSAGING_INFO`/`MESSAGING_WARNING`/`MESSAGING_ERROR`), `class`
/// (`MESSAGING_CLASS_SYSTEM`/`MESSAGING_CLASS_STATS`), `sent_time`,
/// `current_time`, and embedded task/heap telemetry.
pub fn format_messages_json(
    snapshot: &HttpStatusSnapshot<'_>,
    ring: Option<&SyslogRingBuffer>,
    out_buf: &mut [u8],
) -> Option<usize> {
    const STATS_RESERVE_BYTES: usize = 280;
    const ENTRY_RESERVE_THRESHOLD: usize = 420;
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"messages\":[").ok()?;
    let mut written_count = 0usize;
    if let Some(r) = ring {
        let start_idx = r.len().saturating_sub(6);
        for idx in start_idx..r.len() {
            let Some(entry) = r.entry(idx) else {
                continue;
            };
            let needed = ENTRY_RESERVE_THRESHOLD.max(STATS_RESERVE_BYTES + 160 + entry.msg.len());
            if writer.remaining_len() < needed {
                break;
            }
            if written_count > 0 {
                writer.write_str(",").ok()?;
            }
            let msg_type = messaging_type_for_level(entry.level.as_str());
            write!(
                writer,
                "{{\"seq\":{},\"uptime_ms\":{},\"level\":\"{}\",\"msg\":\"{}\",\
                 \"type\":\"{}\",\"class\":\"{}\",\"sent_time\":{},\"current_time\":{}}}",
                entry.seq,
                entry.uptime_ms,
                JsonEscaped(entry.level.as_str()),
                JsonEscaped(entry.msg.as_str()),
                msg_type,
                JsonEscaped(entry.msg_class.as_str()),
                entry.uptime_ms,
                snapshot.uptime_ms,
            )
            .ok()?;
            written_count += 1;
        }
    }
    if written_count == 0 {
        write!(
            writer,
            "{{\"seq\":{},\"uptime_ms\":{},\"level\":\"INFO\",\"msg\":\"{}\",\
             \"type\":\"MESSAGING_INFO\",\"class\":\"MESSAGING_CLASS_SYSTEM\",\
             \"sent_time\":{},\"current_time\":{}}}",
            snapshot.settings_seq,
            snapshot.uptime_ms,
            JsonEscaped(snapshot.amp_mode),
            snapshot.uptime_ms,
            snapshot.uptime_ms,
        )
        .ok()?;
    }
    let stats_seq = snapshot.settings_seq.saturating_add(1);
    write!(
        writer,
        ",{{\"seq\":{},\"uptime_ms\":{},\"level\":\"INFO\",\
         \"type\":\"MESSAGING_INFO\",\"class\":\"MESSAGING_CLASS_STATS\",\
         \"sent_time\":{},\"current_time\":{},\
         \"msg\":\"psram_free={} psram_used={} core1_active={} core1_loops={} \
         decode_us_max={}\"}}],\"current_time\":{}}}",
        stats_seq,
        snapshot.uptime_ms,
        snapshot.uptime_ms,
        snapshot.uptime_ms,
        snapshot.psram_free_bytes,
        snapshot.psram_used_bytes,
        snapshot.core1_active,
        snapshot.core1_loops,
        snapshot.decode_us_max,
        snapshot.uptime_ms,
    )
    .ok()?;
    Some(writer.written_len())
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
        let msg_type = messaging_type_for_level(msg.level);
        write!(
            writer,
            "{{\"seq\":{},\"uptime_ms\":{},\"level\":\"{}\",\"msg\":\"{}\",\
             \"type\":\"{}\",\"class\":\"MESSAGING_CLASS_SYSTEM\",\
             \"sent_time\":{},\"current_time\":{}}}",
            msg.seq,
            msg.uptime_ms,
            JsonEscaped(msg.level),
            JsonEscaped(msg.message),
            msg_type,
            msg.uptime_ms,
            msg.uptime_ms,
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
    CommandCatalogEntry::new(
        "sleep",
        "sleep [light|deep] <seconds>",
        "Enter low-power sleep mode",
    ),
    CommandCatalogEntry::new("help", "help", "List available console commands"),
];

/// Static `.rodata` full `/commands.json` catalog including `argtable` and
/// `values` for C web UI configuration forms and diagnostic CLI commands.
pub const COMMANDS_JSON_CATALOG: &str = concat!(
    "{\"commands\":[",
    "{\"name\":\"system\",\"usage\":\"system <info|heap|reboot|recovery>\",",
    "\"desc\":\"System diagnostics\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"i2c\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"config\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"join\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"update\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"player\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"sleep\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"help\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"free\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"heap\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"version\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"restart\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"recovery\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"restart_ota\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"i2cdetect\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"i2cget\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"i2cset\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"i2cdump\",\"argtable\":[],\"values\":{}},",
    "{\"name\":\"cfg-audio-general\",\"argtable\":[{\"name\":\"output\"}],",
    "\"values\":{\"output\":\"I2S\"}},",
    "{\"name\":\"cfg-audio-tmpl\",\"argtable\":[{\"name\":\"tmpl\"}],",
    "\"values\":{\"tmpl\":\"squeezelite -o I2S -b 500:2000\"}},",
    "{\"name\":\"cfg-syst-services\",\"argtable\":[{\"name\":\"cspot\"},",
    "{\"name\":\"airplay\"},{\"name\":\"bt\"}],",
    "\"values\":{\"cspot\":\"1\",\"airplay\":\"1\",\"bt\":\"1\"}},",
    "{\"name\":\"cfg-syst-name\",\"argtable\":[{\"name\":\"name\"}],",
    "\"values\":{\"name\":\"BASEMENT\"}},",
    "{\"name\":\"cfg-hw-preset\",\"argtable\":[{\"name\":\"model\"}],",
    "\"values\":{\"model\":\"Louder-ESP32\"}},",
    "{\"name\":\"cfg-hw-dac\",\"argtable\":[{\"name\":\"model\"}],",
    "\"values\":{\"model\":\"TAS5805M\"}},",
    "{\"name\":\"cfg-hw-spdif\",\"argtable\":[{\"name\":\"do\"}],",
    "\"values\":{\"do\":\"-1\"}},",
    "{\"name\":\"cfg-hw-rotary\",\"argtable\":[{\"name\":\"A\"}],",
    "\"values\":{\"A\":\"-1\"}},",
    "{\"name\":\"cfg-hw-i2c\",\"argtable\":[{\"name\":\"sda\"},{\"name\":\"scl\"}],",
    "\"values\":{\"sda\":\"21\",\"scl\":\"22\"}},",
    "{\"name\":\"cfg-hw-spi\",\"argtable\":[{\"name\":\"mosi\"}],",
    "\"values\":{\"mosi\":\"-1\"}},",
    "{\"name\":\"cfg-hw-display\",\"argtable\":[{\"name\":\"type\"}],",
    "\"values\":{\"type\":\"none\"}}",
    "]}"
);

/// Formats the full C-compatible `/commands.json` schema (`argtable` + `values`
/// + CLI commands) into `out_buf`.
pub fn format_commands_json(out_buf: &mut [u8]) -> Option<usize> {
    let bytes = COMMANDS_JSON_CATALOG.as_bytes();
    if bytes.len() > out_buf.len() {
        return None;
    }
    out_buf[..bytes.len()].copy_from_slice(bytes);
    Some(bytes.len())
}

/// Formats the `/commands.json` CLI catalog and optional execution output into
/// `out_buf`.
pub fn write_commands_json(
    catalog: &[CommandCatalogEntry<'_>],
    last_output: Option<&str>,
    out_buf: &mut [u8],
) -> Option<usize> {
    if last_output.is_none() && catalog.len() == DEFAULT_COMMAND_CATALOG.len() {
        return format_commands_json(out_buf);
    }
    let mut writer = BufferWriter::new(out_buf);
    writer.write_str("{\"commands\":[").ok()?;
    for (idx, cmd) in catalog.iter().enumerate() {
        if idx > 0 {
            writer.write_str(",").ok()?;
        }
        write!(
            writer,
            "{{\"name\":\"{}\",\"usage\":\"{}\",\"desc\":\"{}\",\
             \"argtable\":[],\"values\":{{}}}}",
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
pub fn write_eq_json(snapshot: &HttpStatusSnapshot<'_>, out_buf: &mut [u8]) -> Option<usize> {
    let mut writer = BufferWriter::new(out_buf);
    let preset = if snapshot.eq_enabled
        && snapshot.eq_preset == "Flat"
        && snapshot.eq_bands_db.iter().any(|&b| b != 0)
    {
        "Custom"
    } else {
        snapshot.eq_preset
    };
    let b = snapshot.eq_bands_db;
    write!(
        writer,
        "{{\"ok\":true,\"eq_preset\":\"{}\",\"eq_enabled\":{},\
         \"again_code\":{},\"vol_pct\":{},\
         \"bands\":[{},{},{},{},{},{},{},{},{},{}]}}",
        JsonEscaped(preset),
        snapshot.eq_enabled,
        snapshot.again_code,
        snapshot.vol_pct,
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
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
        let mask = if parts.first() == Some(&"connect") {
            "<redacted>"
        } else {
            "***"
        };
        let _ = write!(out, "{}{}{}{}{}", parts[0], sep, parts[1], sep, mask);
        for extra in &parts[3..] {
            let _ = write!(out, "{}{}", sep, extra);
        }
    } else if (ws_parts.first() == Some(&"join") || ws_parts.first() == Some(&"connect"))
        && ws_parts.len() >= 3
    {
        let mask = if ws_parts.first() == Some(&"connect") {
            "<redacted>"
        } else {
            "***"
        };
        let _ = write!(out, "{}{}{}{}{}", ws_parts[0], sep, ws_parts[1], sep, mask);
        for extra in &ws_parts[3..] {
            let _ = write!(out, "{}{}", sep, extra);
        }
    } else if parts.first() == Some(&"config")
        && parts.get(1) == Some(&"set")
        && parts
            .get(2)
            .is_some_and(|k| crate::config_handle::is_secret_config_key(k))
        && parts.len() >= 4
    {
        let _ = write!(out, "config{}set{}{}{}***", sep, sep, parts[2], sep);
    } else if ws_parts.first() == Some(&"config")
        && ws_parts.get(1) == Some(&"set")
        && ws_parts
            .get(2)
            .is_some_and(|k| crate::config_handle::is_secret_config_key(k))
        && ws_parts.len() >= 4
    {
        let _ = write!(out, "config set {} ***", ws_parts[2]);
    } else if let Some(idx) = cmd.find("wifi.pass=") {
        let prefix = &cmd[..idx + "wifi.pass=".len()];
        let rest = &cmd[idx + "wifi.pass=".len()..];
        let end_idx = rest
            .find('&')
            .or_else(|| rest.find(' '))
            .unwrap_or(rest.len());
        let _ = write!(out, "{}***{}", prefix, &rest[end_idx..]);
    } else if let Some(idx) = cmd.find("password=") {
        let prefix = &cmd[..idx + "password=".len()];
        let rest = &cmd[idx + "password=".len()..];
        let end_idx = rest
            .find('&')
            .or_else(|| rest.find(' '))
            .unwrap_or(rest.len());
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
        if let Some(auth_val) = trimmed
            .strip_prefix("Authorization:")
            .or_else(|| trimmed.strip_prefix("authorization:"))
        {
            let auth_val = auth_val.trim();
            if let Some(bearer_val) = auth_val
                .strip_prefix("Bearer ")
                .or_else(|| auth_val.strip_prefix("bearer "))
            {
                if bearer_val.trim() == expected_token {
                    return true;
                }
            } else if auth_val == expected_token {
                return true;
            }
        } else if let Some(token_val) = trimmed
            .strip_prefix("X-Auth-Token:")
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
        handle_http_request_streaming_impl(req_bytes, snapshot, &[], out_buf)
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
        if let Some(origin_val) = trimmed
            .strip_prefix("Origin:")
            .or_else(|| trimmed.strip_prefix("origin:"))
        {
            let val = origin_val.trim();
            if val.is_empty() || val == "null" {
                return false;
            }
            let host_part = val
                .strip_prefix("http://")
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
    extra_entries: &[ConfigEntryView<'_>],
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

    let parsed_method = parse_http_method(method);
    if (parsed_method == HttpMethod::Post || parsed_method == HttpMethod::Delete)
        && !is_permitted_origin(req_bytes)
    {
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

    // Captive portal probes:
    // Android / Samsung status probes expect 200 OK
    // Others expect 302 Found redirect to device IP
    match (method, path) {
        ("GET", "/generate_204") | ("GET", "/mobile/status.php") => {
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
                Content-Length: 0\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            return Some((resp_bytes.len(), &[], HttpControlAction::None));
        }
        ("GET", "/gen_204")
        | ("GET", "/hotspot-detect.html")
        | ("GET", "/hotspotdetect.html")
        | ("GET", "/library/test/success.html")
        | ("GET", "/success.txt") => {
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 302 Found\r\nLocation: http://{}.{}.{}.{}/\r\n\
                Content-Length: 0\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
                snapshot.ip[0], snapshot.ip[1], snapshot.ip[2], snapshot.ip[3]
            );
            let resp_bytes = response.as_bytes();
            if resp_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
            return Some((resp_bytes.len(), &[], HttpControlAction::None));
        }
        _ => {}
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
        let remote_name = if !snapshot.player_name.is_empty() {
            snapshot.player_name
        } else {
            crate::board::first_boot_defaults::PLAYER_NAME
        };

        let mut zc = ZeroConfServer::new();
        if let Ok(resp) = zc.handle_request(raw_path, body_str, &dh, device_id, remote_name) {
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
            let mut act = query
                .split('&')
                .find_map(|p| p.strip_prefix("act="))
                .unwrap_or("");
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
                    act = body_str
                        .split('&')
                        .find_map(|p| p.strip_prefix("act="))
                        .unwrap_or("");
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
            Some((
                resp_bytes.len(),
                &[],
                HttpControlAction::PersistOta0AndReboot,
            ))
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
            Some((
                resp_bytes.len(),
                &[],
                HttpControlAction::PersistOta0AndReboot,
            ))
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
        ("POST", "/eq.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
            writer.write_str("{\"ok\":true,\"result\":\"OK\"}").ok()?;
            let body_len = writer.written_len();
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::UpdateConfig))
        }
        ("GET", "/config.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len = write_config_json(snapshot, extra_entries, &mut out_buf[HEADER_ROOM..])?;
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
                let display_val = if crate::config_handle::is_secret_config_key(decoded_key.trim())
                {
                    "********"
                } else {
                    decoded_val
                };
                write!(
                    writer,
                    "{{\"ok\":true,\"key\":\"{}\",\"value\":\"{}\",\"result\":\"OK\"}}",
                    JsonEscaped(decoded_key),
                    JsonEscaped(display_val),
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
        ("GET", "/ap.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let rssi_i8 = snapshot.rssi.clamp(-127, 0) as i8;
            let body_len =
                format_ap_list_json(&[], snapshot.ssid, rssi_i8, &mut out_buf[HEADER_ROOM..]);
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("DELETE", "/connect.json") => {
            const BODY: &str = "{}";
            let mut response: String<256> = String::new();
            let _ = write!(
                response,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
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
            Some((resp_bytes.len(), &[], HttpControlAction::DisconnectWifi))
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
        ("GET", "/messages.json") | ("GET", "/syslog.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let body_len = with_syslog_ring(|ring| {
                let ring_opt = if ring.is_empty() { None } else { Some(&*ring) };
                format_messages_json(snapshot, ring_opt, &mut out_buf[HEADER_ROOM..])
            })?;
            let total_len = write_json_http_response(body_len, out_buf)?;
            Some((total_len, &[], HttpControlAction::None))
        }
        ("GET", "/commands.json") => {
            let body_bytes = COMMANDS_JSON_CATALOG.as_bytes();
            let mut header: String<160> = String::new();
            write!(
                header,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\
                Connection: close\r\nCache-Control: no-store\r\n\r\n",
                body_bytes.len()
            )
            .ok()?;
            let header_bytes = header.as_bytes();
            if header_bytes.len() > out_buf.len() {
                return None;
            }
            out_buf[..header_bytes.len()].copy_from_slice(header_bytes);
            Some((header_bytes.len(), body_bytes, HttpControlAction::None))
        }
        ("POST", "/commands.json") => {
            const HEADER_ROOM: usize = 160;
            if out_buf.len() <= HEADER_ROOM {
                return None;
            }
            let payload = extract_query_or_body(raw_path, req_bytes);
            let cmd_str = extract_param_or_json_field(payload, "cmd")
                .or_else(|| extract_param_or_json_field(payload, "command"))
                .unwrap_or("");
            let (body_len, action) = if !cmd_str.is_empty() {
                let mut writer = BufferWriter::new(&mut out_buf[HEADER_ROOM..]);
                let redacted = redact_command_password(cmd_str);
                write!(
                    writer,
                    "{{\"ok\":true,\"cmd\":\"{}\",\"Result\":\"Success\"}}",
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
    handle_http_request_streaming_impl(req_bytes, snapshot, &[], out_buf)
}

/// Parses an HTTP request from `req_bytes` and writes the HTTP header (or full
/// response for JSON) into `out_buf`, including caller-supplied configuration
/// entries in `GET /config.json`.
pub fn handle_http_request_streaming_with_entries(
    req_bytes: &[u8],
    snapshot: &HttpStatusSnapshot<'_>,
    extra_entries: &[ConfigEntryView<'_>],
    out_buf: &mut [u8],
) -> Option<(usize, &'static [u8], HttpControlAction)> {
    handle_http_request_streaming_impl(req_bytes, snapshot, extra_entries, out_buf)
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
pub const DASHBOARD_HTML: &str = crate::web_ui::WEB_UI_HTML;

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
        let (len, action) = handle_http_control_request(req, &snap, &mut buf).unwrap();
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
        let (len1, act1) = handle_http_control_request(req1, &snap, &mut buf).unwrap();
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
        let (_len2, act2) = handle_http_control_request(req2, &snap, &mut buf).unwrap();
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
        let (_len3, act3) = handle_http_control_request(req3, &snap, &mut buf).unwrap();
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
        let (_len4, act4) = handle_http_control_request(req4, &snap, &mut buf).unwrap();
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
        let (_len5, act5) = handle_http_control_request(req5, &snap, &mut buf).unwrap();
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
        let (_len6, act6) = handle_http_control_request(req6, &snap, &mut buf).unwrap();
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
        let (_len7, act7) = handle_http_control_request(req7, &snap, &mut buf).unwrap();
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
        let (len8, act8) = handle_http_control_request(req8, &snap, &mut buf).unwrap();
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
        let mut buf = [0u8; 1536];

        // JSON escaping in write_status_json
        let len = write_status_json(&snap, &mut buf).unwrap();
        let json_str = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(json_str.contains("\"ssid\":\"My\\\"Wi-Fi\\\\Net\\tName\\u0001\""));
        assert!(json_str.contains("\"amp_mode\":\"Stereo\\\"BTL\\\"\""));

        // POST /cmd.json with act= in body
        let req = b"POST /cmd.json HTTP/1.1\r\nHost: 192.168.1.50\r\n\
            Content-Type: application/x-www-form-urlencoded\r\n\r\nact=pause";
        let (resp_len, act) = handle_http_control_request(req, &snap, &mut buf).unwrap();
        assert_eq!(
            act,
            HttpControlAction::PlayerIrCommand {
                ir_code: 0x7689_20DF,
                vol_pct: None,
                mute: None,
            }
        );
        assert!(core::str::from_utf8(&buf[..resp_len])
            .unwrap()
            .contains("\"ok\":true"));
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
        let mut buf = [0u8; 32768];

        let req1 = b"GET / HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (len1, act1) = handle_http_control_request(req1, &snap, &mut buf).unwrap();
        assert_eq!(act1, HttpControlAction::None);
        let resp1 = core::str::from_utf8(&buf[..len1]).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp1.contains("Content-Type: text/html; charset=utf-8"));
        assert!(resp1.contains("<title>Louder-ESP32 Control</title>"));
        assert!(resp1.contains("/status.json"));

        let req2 = b"GET /index.html HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (len2, act2) = handle_http_control_request(req2, &snap, &mut buf).unwrap();
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
            handle_http_request_streaming(b"GET / HTTP/1.1\r\n\r\n", &snap, &mut buf).unwrap();
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
        assert!(handle_http_request_streaming(
            b"GET /status.json HTTP/1.1\r\n\r\n",
            &worst,
            &mut small,
        )
        .is_none());
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
            jack_inserted: false,
            voltage_mv: 0,
            disconnect_count: 0,
            avg_conn_time_ms: 0,
            is_i2c_locked: false,
            urc: 0,
            bt_status: 0,
            bt_sub_status: 0,
            eq_bands_db: [0; 10],
            player_name: "BASEMENT",
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
        let (clen, cact) =
            handle_http_control_request(b"GET /config.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
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
        let (slen, sact) =
            handle_http_control_request(b"POST /scan.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
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
        let (mlen, mact) =
            handle_http_control_request(b"GET /messages.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .expect("GET /messages.json");
        assert_eq!(mact, HttpControlAction::None);
        let mstr = core::str::from_utf8(&buf[..mlen]).expect("utf8");
        assert!(mstr.contains("\"messages\":[{\"seq\":1"));

        // 5. GET /commands.json & POST /commands.json
        let (xlen, xact) =
            handle_http_control_request(b"GET /commands.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
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
        let (flen, fact) =
            handle_http_control_request(b"POST /flash.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
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
        assert!(resp.contains("\"bands\":[0,0,0,0,0,0,0,0,0,0]"));

        // Custom preset when Flat has non-zero bands
        let snap_custom = HttpStatusSnapshot {
            eq_preset: "Flat",
            eq_enabled: true,
            eq_bands_db: [0, 2, 0, 0, 0, 0, 0, 0, 0, -3],
            ..HttpStatusSnapshot::default()
        };
        let (len2, _) = handle_http_control_request(req, &snap_custom, &mut buf).unwrap();
        let resp2 = core::str::from_utf8(&buf[..len2]).unwrap();
        assert!(resp2.contains("\"eq_preset\":\"Custom\""));
        assert!(resp2.contains("\"bands\":[0,2,0,0,0,0,0,0,0,-3]"));
    }

    #[test]
    fn test_board_gpio_map_scl_pin_27_no_conflict() {
        let scl = BOARD_GPIO_MAP
            .iter()
            .find(|p| p.name == "SCL")
            .expect("SCL pin");
        assert_eq!(scl.gpio, 27);
        let do_pin = BOARD_GPIO_MAP
            .iter()
            .find(|p| p.name == "DO")
            .expect("DO pin");
        assert_eq!(do_pin.gpio, 22);
        assert_ne!(scl.gpio, do_pin.gpio);
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
        let (len1, act1) =
            handle_http_control_request(b"GET /recovery.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act1, HttpControlAction::None);
        let resp1 = core::str::from_utf8(&buf[..len1]).unwrap();
        assert!(resp1.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

        // GET /reboot.json rejected with 405
        let (len2, act2) =
            handle_http_control_request(b"GET /reboot.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act2, HttpControlAction::None);
        let resp2 = core::str::from_utf8(&buf[..len2]).unwrap();
        assert!(resp2.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));

        // Authenticated router rejects missing token
        let router = HttpControlRouter::with_auth_token("secret_token_123");
        let (len3, _) = router
            .handle_request(b"GET /status.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
            .unwrap();
        let resp3 = core::str::from_utf8(&buf[..len3]).unwrap();
        assert!(resp3.starts_with("HTTP/1.1 401 Unauthorized\r\n"));

        // Authenticated router accepts valid header
        let (len4, _) = router
            .handle_request(
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

    fn sample_snapshot() -> HttpStatusSnapshot<'static> {
        HttpStatusSnapshot::default()
    }

    fn handle_request_bytes(
        req: &[u8],
        snap: &HttpStatusSnapshot<'_>,
        router_opt: Option<&HttpControlRouter>,
    ) -> (std::string::String, HttpControlAction) {
        let mut buf = [0u8; 32768];
        let res = if let Some(r) = router_opt {
            r.handle_request(req, snap, &mut buf)
        } else {
            handle_http_control_request(req, snap, &mut buf)
        };
        if let Some((len, action)) = res {
            let resp = core::str::from_utf8(&buf[..len]).unwrap_or("").into();
            (resp, action)
        } else {
            ("".into(), HttpControlAction::None)
        }
    }

    #[test]
    fn test_web_ui_contains_all_c_tabs_and_rust_dashboard_controls() {
        let snap = sample_snapshot();
        let req = b"GET / HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        for marker in [
            "id=\"tab-player\"",
            "id=\"tab-wifi\"",
            "id=\"tab-syslog\"",
            "id=\"tab-cfg-audio\"",
            "id=\"tab-cfg-syst\"",
            "id=\"tab-cfg-hw\"",
            "id=\"tab-cfg-fw\"",
            "id=\"tab-nvs\"",
            "id=\"tab-commands\"",
            "id=\"tab-credits\"",
            "id=\"navtitle\"",
            "id=\"wifiTable\"",
            "id=\"syslogTable\"",
            "id=\"gpiotable\"",
            "id=\"tasks\"",
            "id=\"save-autoexec1\"",
            "id=\"save-nvs\"",
            "id=\"save-audio\"",
            "id=\"save-hw\"",
            "id=\"sq-name\"",
            "id=\"sq-server\"",
            "id=\"led-vu\"",
            "id=\"led-brightness\"",
            "id=\"eq-b0\"",
            "id=\"eq-v0\"",
            "id=\"fwUpload\"",
            "eq_loudness",
            "again4",
            "/recovery.json",
            "/reboot_ota.json",
        ] {
            assert!(body.contains(marker), "missing UI element {}", marker);
        }
    }

    fn extract_http_body(resp: &str) -> &str {
        if let Some((_, body)) = resp.split_once("\r\n\r\n") {
            body
        } else if let Some((_, body)) = resp.split_once("\n\n") {
            body
        } else {
            ""
        }
    }

    #[test]
    fn test_get_ap_json_returns_c_compatible_array() {
        let snap = sample_snapshot();
        let req = b"GET /ap.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, action) = handle_request_bytes(req, &snap, None);
        assert_eq!(action, HttpControlAction::None);
        let body = extract_http_body(&resp);
        assert!(body.starts_with('[') && body.ends_with(']'));
        assert!(body.contains("\"ssid\":\"iot\""));
        assert!(body.contains("\"chan\":"));
        assert!(body.contains("\"rssi\":-50"));
        assert!(body.contains("\"auth\":"));
        assert!(body.contains("\"known\":true"));
    }

    #[test]
    fn test_delete_connect_json_emits_disconnect_wifi_and_enforces_origin() {
        let snap = sample_snapshot();
        let req = b"DELETE /connect.json HTTP/1.1\r\n\
            Host: 172.22.1.15\r\nOrigin: http://172.22.1.15\r\n\r\n";
        let (resp, action) = handle_request_bytes(req, &snap, None);
        assert_eq!(action, HttpControlAction::DisconnectWifi);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));

        let bad_req = b"DELETE /connect.json HTTP/1.1\r\n\
            Host: 172.22.1.15\r\nOrigin: http://evil.example.com\r\n\r\n";
        let (bad_resp, bad_action) = handle_request_bytes(bad_req, &snap, None);
        assert_eq!(bad_action, HttpControlAction::None);
        assert!(bad_resp.starts_with("HTTP/1.1 403 Forbidden"));
    }

    #[test]
    fn test_captive_portal_probes_return_200_or_302() {
        let snap = sample_snapshot();
        for path in ["/generate_204", "/mobile/status.php"] {
            let req = format!("GET {} HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n", path);
            let (resp, _) = handle_request_bytes(req.as_bytes(), &snap, None);
            assert!(resp.starts_with("HTTP/1.1 200 OK"));
        }
        let redirect_paths = [
            "/gen_204",
            "/hotspot-detect.html",
            "/hotspotdetect.html",
            "/library/test/success.html",
            "/success.txt",
        ];
        for path in redirect_paths {
            let req = format!("GET {} HTTP/1.1\r\nHost: 192.168.4.1\r\n\r\n", path);
            let (resp, _) = handle_request_bytes(req.as_bytes(), &snap, None);
            assert!(resp.starts_with("HTTP/1.1 302 Found"));
        }
    }

    fn body_contains_ok_or_result(resp: &str) -> bool {
        let body = extract_http_body(resp);
        body.contains("\"ok\":true") || body.contains("\"result\":\"OK\"")
    }

    #[test]
    fn test_status_json_includes_all_c_and_rust_fields() {
        let snap = sample_snapshot();
        let req = b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        for key in [
            "\"project_name\":",
            "\"version\":",
            "\"recovery\":",
            "\"Jack\":",
            "\"Voltage\":",
            "\"disconnect_count\":",
            "\"avg_conn_time\":",
            "\"is_i2c_locked\":",
            "\"urc\":",
            "\"bt_status\":",
            "\"bt_sub_status\":",
            "\"frames_played\":",
            "\"tas_pll_locked\":",
            "\"psram_free_bytes\":",
        ] {
            assert!(body.contains(key), "missing key {}", key);
        }
    }

    #[test]
    fn test_config_json_get_includes_gpio_and_strips_secrets() {
        let snap = sample_snapshot();
        let req = b"GET /config.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        assert!(body.contains("\"gpio\":["));
        assert!(body.contains("\"name\":\"SDA\""));
        assert!(body.contains("\"autoexec1\""));
        assert!(!body.contains("\"password\""));
        assert!(!body.contains("\"web_pwd\""));
    }

    #[test]
    fn test_config_json_post_accepts_nested_c_schema_and_fwurl() {
        let snap = sample_snapshot();
        let req = b"POST /config.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\
            Origin: http://172.22.1.15\r\nContent-Type: application/json\r\n\r\n\
            {\"config\":{\"host_name\":{\"value\":\"BASEMENT\",\"type\":33}}}";
        let (resp, action) = handle_request_bytes(req, &snap, None);
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(body_contains_ok_or_result(&resp));
        assert!(matches!(action, HttpControlAction::UpdateConfig));
    }

    #[test]
    fn test_commands_json_includes_argtable_and_values_and_accepts_c_post_body() {
        let snap = sample_snapshot();
        let req = b"GET /commands.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        assert!(body.contains("\"commands\":["));
        assert!(body.contains("\"argtable\":["));
        assert!(body.contains("\"values\":{"));
        assert!(body.contains("\"cfg-syst-services\""));
        assert!(body.contains("\"i2cdetect\""));

        let post_req = b"POST /commands.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\
            Origin: http://172.22.1.15\r\nContent-Type: application/json\r\n\r\n\
            {\"command\":\"i2cdetect\"}";
        let (post_resp, action) = handle_request_bytes(post_req, &snap, None);
        let post_body = extract_http_body(&post_resp);
        assert!(post_body.contains("\"Result\":\"Success\""));
        assert!(post_body.contains("\"ok\":true"));
        assert!(matches!(action, HttpControlAction::ExecuteCliCommand));
    }

    #[test]
    fn test_messages_json_includes_system_and_stats_classes() {
        let snap = sample_snapshot();
        let req = b"GET /messages.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        assert!(body.contains("\"messages\":["));
        assert!(body.contains("MESSAGING_CLASS_STATS"));
        assert!(body.contains("MESSAGING_INFO"));
    }

    #[test]
    fn test_syslog_json_route_and_push_runtime_syslog() {
        let snap = sample_snapshot();
        push_runtime_syslog(
            "WARN",
            "MESSAGING_CLASS_SYSTEM",
            "I2C NACK on read op",
            snap.uptime_ms,
        );
        let req = b"GET /syslog.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n";
        let (resp, _) = handle_request_bytes(req, &snap, None);
        let body = extract_http_body(&resp);
        assert!(body.contains("\"messages\":["));
        assert!(body.contains("I2C NACK on read op"));
        assert!(body.contains("MESSAGING_WARNING"));
        assert!(body.contains("MESSAGING_CLASS_SYSTEM"));
        assert!(body.contains("MESSAGING_CLASS_STATS"));

        push_syslog_entry(
            "ERROR",
            "MESSAGING_CLASS_SYSTEM",
            "TAS5805M fault recovery cooldown",
            snap.uptime_ms,
        );
        let (resp2, _) = handle_request_bytes(req, &snap, None);
        let body2 = extract_http_body(&resp2);
        assert!(body2.contains("TAS5805M fault recovery cooldown"));
        assert!(body2.contains("MESSAGING_ERROR"));
    }

    #[test]
    fn test_execute_web_cli_command_populates_syslog_for_diagnostics_and_cfg() {
        let snap = sample_snapshot();
        let mut cfg = crate::config_handle::ConfigHandle::default();
        let mut syslog = SyslogRingBuffer::new();

        assert!(execute_web_cli_command(
            "free",
            &snap,
            &mut cfg,
            &mut syslog
        ));
        assert!(syslog.latest_message().contains("PSRAM free:"));

        assert!(execute_web_cli_command(
            "i2cdetect",
            &snap,
            &mut cfg,
            &mut syslog
        ));
        assert!(syslog.latest_message().contains("0x2D"));

        assert!(execute_web_cli_command(
            "cfg-syst-name --host_name STUDIO",
            &snap,
            &mut cfg,
            &mut syslog
        ));
        assert_eq!(cfg.host_name_str(), "STUDIO");

        assert!(apply_web_config_update(
            "vol_pct",
            "80",
            snap.uptime_ms,
            &mut cfg,
            &mut syslog
        ));
        assert_eq!(cfg.read(|c| c.volume), 80);
        assert!(syslog.latest_message().contains("config: vol_pct=80"));
    }

    #[test]
    fn test_final_review_secret_redaction_and_messages_reserve_guard() {
        let snap = sample_snapshot();
        let mut buf = [0u8; 1536];

        // 1. Secret key matching for pwd, telnet_pwd, a2dp_spin, and bt_sink_pin
        for secret_key in ["pwd", "telnet_pwd", "a2dp_spin", "bt_sink_pin"] {
            assert!(
                crate::config_handle::is_secret_config_key(secret_key),
                "expected {secret_key} to be classified as secret"
            );
            let req = format!(
                "POST /config.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\
                 Origin: http://172.22.1.15\r\n\r\nkey={secret_key}&value=TopSecret99"
            );
            let (len, act) = handle_http_control_request(req.as_bytes(), &snap, &mut buf).unwrap();
            assert_eq!(act, HttpControlAction::UpdateConfig);
            let resp = core::str::from_utf8(&buf[..len]).unwrap();
            assert!(resp.contains("\"value\":\"********\""));
            assert!(!resp.contains("TopSecret99"));

            let cmd_req = format!(
                "POST /commands.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\
                 Origin: http://172.22.1.15\r\n\r\ncmd=config set {secret_key} TopSecret99"
            );
            let (clen, cact) =
                handle_http_control_request(cmd_req.as_bytes(), &snap, &mut buf).unwrap();
            assert_eq!(cact, HttpControlAction::ExecuteCliCommand);
            let cresp = core::str::from_utf8(&buf[..clen]).unwrap();
            assert!(cresp.contains(&format!("config set {secret_key} ***")));
            assert!(!cresp.contains("TopSecret99"));
        }

        // 2. Full 16-entry SyslogRingBuffer fits inside 1376-byte body buffer
        let mut ring = SyslogRingBuffer::new();
        for i in 0..16 {
            let msg = format!(
                "diagnostic syslog ring entry #{i} with long payload padding \
                 to verify reserve guard prevents buffer overflow on 1536B tx"
            );
            ring.push(1000 + i as u64, "INFO", "MESSAGING_CLASS_SYSTEM", &msg);
        }
        assert_eq!(ring.len(), 16);
        let mut body_buf = [0u8; 1376];
        let written = format_messages_json(&snap, Some(&ring), &mut body_buf)
            .expect("full 16-entry ring must fit inside 1376-byte buffer");
        let json_str = core::str::from_utf8(&body_buf[..written]).unwrap();
        assert!(json_str.contains("\"MESSAGING_CLASS_STATS\""));
        assert!(json_str.ends_with('}'));

        // 3. Live WEB_SYSLOG_RING wired into GET /messages.json & real value staged
        let mut cfg = crate::config_handle::ConfigHandle::default();
        let mut local_syslog = SyslogRingBuffer::new();
        assert!(apply_web_config_update(
            "player_name",
            "VaultRoom",
            snap.uptime_ms,
            &mut cfg,
            &mut local_syslog,
        ));
        assert_eq!(cfg.host_name_str(), "VaultRoom");

        with_syslog_ring(|ring| {
            let _ = execute_web_cli_command("version", &snap, &mut cfg, ring);
        });
        let (mlen, _) = handle_http_control_request(
            b"GET /messages.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n",
            &snap,
            &mut buf,
        )
        .expect("GET /messages.json fits in 1536B buffer");
        let mresp = core::str::from_utf8(&buf[..mlen]).unwrap();
        assert!(mresp.contains(RUST_OTA_FIRMWARE_VERSION));
        assert!(mresp.contains("\"MESSAGING_CLASS_STATS\""));
    }

    /// F3: the ring is on the heap; the static holds only the mutex and the
    /// pointer, so it costs (next to) no internal RAM on the device.
    #[test]
    fn web_syslog_ring_static_is_pointer_sized() {
        assert!(core::mem::size_of_val(&WEB_SYSLOG_RING) <= 32);
        push_runtime_syslog("INFO", "MESSAGING_CLASS_SYSTEM", "f3 ring probe", 1);
        assert!(with_syslog_ring(|ring| !ring.is_empty()));
    }
}
