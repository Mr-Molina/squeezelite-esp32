//! Pure-Rust Port 80 HTTP Control/Recovery Server & Dead-Man Network Lease Watchdog (Gates 3 & 4).
//!
//! Provides:
//! - Non-blocking HTTP request parser and response formatter for TCP Port 80 on `172.22.1.15`:
//!   - `GET /status.json` (and `GET /`): Squeezelite-ESP32 compatible status JSON + Rust telemetry
//!   - `POST /recovery.json`: HTTP 200 JSON + immediate `RebootToRecovery` (`0x10000` factory)
//!   - `POST /reboot.json`: HTTP 200 JSON + immediate `RebootToRecovery` (`0x10000` factory)
//!   - `POST /reboot_ota.json`: HTTP 200 JSON + `PersistOta0AndReboot` (`0x160000` `ota_0`)
//! - `DeadManLeaseWatchdog`: 45-second boot connect/DHCP timeout and 60-second link-drop timeout.

use core::fmt::Write;
use heapless::String;

use crate::ota_safety::{REQUIRED_OTA_PROJECT_NAME, RUST_OTA_FIRMWARE_VERSION};

/// Maximum time allowed from boot to establish Wi-Fi STA link + DHCPv4 lease before rolling back.
pub const BOOT_LEASE_TIMEOUT_MS: u64 = 45_000;

/// Maximum continuous time Wi-Fi link or DHCPv4 lease may remain down at runtime before rolling back.
pub const DROP_LEASE_TIMEOUT_MS: u64 = 60_000;

/// Post-response action required after flushing the Port 80 HTTP response socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpControlAction {
    /// Keep running normally.
    None,
    /// Trigger `esp_hal::system::software_reset()` to boot into `recovery` (`0x10000`),
    /// relying on Gate 1 (`otadata` pre-erased to `0xFF`).
    RebootToRecovery,
    /// Write `OtaSelectEntry::new_ota_0()` to `0xD000` and trigger `software_reset()`
    /// so the next boot stays in `ota_0` (`0x160000`).
    PersistOta0AndReboot,
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
    /// Whether `otadata` (`0xD000..0xF000`) is pre-armed to `0xFF` (`Factory` recovery on reset).
    pub otadata_prearmed_factory: bool,
    /// Total audio frames rendered via `I2S0` DMA.
    pub frames_played: u64,
    /// Current TAS5805M amplifier mode label (`"Play"`, `"HiZ"`, `"DeepSleep"`, `"Fault"`).
    pub amp_mode: &'a str,
    /// Total hardware `I2C0` ACK transactions to `0x2D`.
    pub i2c_acks: u32,
    /// Total hardware `I2C0` NACK errors.
    pub i2c_nacks: u32,
    /// Monotonic uptime in milliseconds since boot.
    pub uptime_ms: u64,
}

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
            i2c_acks: 0,
            i2c_nacks: 0,
            uptime_ms: 0,
        }
    }
}

/// Parses an HTTP request from `req_bytes` and writes a complete HTTP/1.1 response into `out_buf`.
///
/// Returns `None` if `req_bytes` does not yet contain a complete request line (`\r\n` or `\n`).
pub fn handle_http_control_request(
    req_bytes: &[u8],
    snapshot: &HttpStatusSnapshot<'_>,
    out_buf: &mut [u8],
) -> Option<(usize, HttpControlAction)> {
    let line_end = req_bytes.iter().position(|&b| b == b'\n')?;
    let first_line = core::str::from_utf8(&req_bytes[..line_end])
        .unwrap_or("")
        .trim_end_matches('\r');

    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("");
    let path = raw_path.split('?').next().unwrap_or(raw_path);

    let mut body: String<512> = String::new();
    let (status_line, action) = match (method, path) {
        ("GET", "/status.json") | ("GET", "/") => {
            let _ = write!(
                body,
                "{{\"project_name\":\"{}\",\"platform_name\":\"ESP32\",\"version\":\"{}\",\"recovery\":0,\"rust_engine\":true,\"otadata_prearmed_factory\":{},\"if\":\"wifi\",\"ip\":\"{}.{}.{}.{}\",\"netmask\":\"{}.{}.{}.{}\",\"gw\":\"{}.{}.{}.{}\",\"ssid\":\"{}\",\"rssi\":{},\"lms_cport\":{},\"lms_port\":{},\"lms_ip\":\"{}.{}.{}.{}\",\"frames_played\":{},\"amp_mode\":\"{}\",\"i2c_acks\":{},\"i2c_nacks\":{},\"uptime_ms\":{}}}",
                REQUIRED_OTA_PROJECT_NAME,
                RUST_OTA_FIRMWARE_VERSION,
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
                snapshot.ssid,
                snapshot.rssi,
                snapshot.lms_cport,
                snapshot.lms_port,
                snapshot.lms_ip[0],
                snapshot.lms_ip[1],
                snapshot.lms_ip[2],
                snapshot.lms_ip[3],
                snapshot.frames_played,
                snapshot.amp_mode,
                snapshot.i2c_acks,
                snapshot.i2c_nacks,
                snapshot.uptime_ms,
            );
            ("HTTP/1.1 200 OK", HttpControlAction::None)
        }
        ("POST", "/recovery.json") | ("GET", "/recovery.json") => {
            let _ = body.push_str("{\"status\":\"rebooting_to_recovery\",\"target_partition\":\"factory_0x10000\"}");
            ("HTTP/1.1 200 OK", HttpControlAction::RebootToRecovery)
        }
        ("POST", "/reboot.json") | ("GET", "/reboot.json") => {
            let _ = body.push_str("{\"status\":\"rebooting_to_recovery\",\"target_partition\":\"factory_0x10000\"}");
            ("HTTP/1.1 200 OK", HttpControlAction::RebootToRecovery)
        }
        ("POST", "/reboot_ota.json") => {
            let _ = body.push_str("{\"status\":\"rebooting_to_ota_0\",\"target_partition\":\"ota_0_0x160000\"}");
            ("HTTP/1.1 200 OK", HttpControlAction::PersistOta0AndReboot)
        }
        _ => {
            let _ = body.push_str("{\"error\":\"not_found\"}");
            ("HTTP/1.1 404 Not Found", HttpControlAction::None)
        }
    };

    let mut response: String<768> = String::new();
    let _ = write!(
        response,
        "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{}",
        status_line,
        body.len(),
        body.as_str()
    );

    let resp_bytes = response.as_bytes();
    if resp_bytes.len() > out_buf.len() {
        return None;
    }
    out_buf[..resp_bytes.len()].copy_from_slice(resp_bytes);
    Some((resp_bytes.len(), action))
}

/// Health verdict emitted by [`DeadManLeaseWatchdog::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseHealth {
    /// Network lease is healthy or still within the allowed grace window.
    Healthy,
    /// Lease timeout exceeded; caller MUST immediately invoke `esp_hal::system::software_reset()`.
    ExpiredResetToRecovery,
}

/// Gate 3 Dead-Man Network Lease Watchdog.
///
/// Guarantees that if Wi-Fi + DHCPv4 is not established within `BOOT_LEASE_TIMEOUT_MS` (45s) of boot,
/// or if connectivity drops for more than `DROP_LEASE_TIMEOUT_MS` (60s) at runtime, the device
/// resets into the untouched `recovery` partition (`0x10000`).
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
    pub fn evaluate(&mut self, now_ms: u64, wifi_connected: bool, has_ipv4_lease: bool) -> LeaseHealth {
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
        let mut buf = [0u8; 768];
        let (len, action) =
            handle_http_control_request(b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n", &snap, &mut buf)
                .unwrap();
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
        assert_eq!(act2, HttpControlAction::RebootToRecovery);

        let (_len3, act3) =
            handle_http_control_request(b"POST /reboot_ota.json HTTP/1.1\r\n\r\n", &snap, &mut buf)
                .unwrap();
        assert_eq!(act3, HttpControlAction::PersistOta0AndReboot);
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
}
