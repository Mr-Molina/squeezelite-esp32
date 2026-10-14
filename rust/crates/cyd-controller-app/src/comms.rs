//! CYD Display-Side UART Telemetry Link & Heartbeat Monitor
//!
//! Handles asynchronous UART communication between the CYD display and the
//! Squeezelite-ESP32 / Louder-ESP32-Plus audio host.

use core::fmt::Write;
use heapless::{Deque, String, Vec};
use squeezelite_cyd_link::{truncate_utf8, CydCommand, CydLinkProtocol, CydMessage, LinkError, PlayState};

pub const CYD_UART_TX_PIN: u8 = 27;
pub const CYD_UART_RX_PIN: u8 = 22;
pub const CYD_UART_BAUD: u32 = 460_800;
pub const CYD_HEARTBEAT_TIMEOUT_US: i64 = 5_000_000;
pub const CYD_RESYNC_INTERVAL_US: i64 = 3_000_000;

/// Current playback, network, and system state displayed on the CYD screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CydTelemetryState {
    pub title: String<64>,
    pub artist: String<64>,
    pub album: String<64>,
    pub state: PlayState,
    pub mode: String<16>,
    pub ip: String<32>,
    pub elapsed_secs: u32,
    pub duration_secs: u32,
    pub vol: u8,
    pub wifi_connected: bool,
    pub wifi_rssi: i8,
    pub link_active: bool,
}

impl Default for CydTelemetryState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            state: PlayState::Stopped,
            mode: String::new(),
            ip: String::new(),
            elapsed_secs: 0,
            duration_secs: 0,
            vol: 0,
            wifi_connected: false,
            wifi_rssi: 0,
            link_active: false,
        }
    }
}

/// CYD Display-Side UART Protocol Manager and Heartbeat Monitor.
pub struct CydDisplayLink {
    pub state: CydTelemetryState,
    pub last_rx_time_us: i64,
    pub last_sync_time_us: i64,
    rx_line: Vec<u8, 256>,
    discarding_overflow: bool,
    pub tx_queue: Deque<Vec<u8, 64>, 16>,
}

impl Default for CydDisplayLink {
    fn default() -> Self {
        Self::new()
    }
}

impl CydDisplayLink {
    /// Create a new link monitor in uninitialized / inactive state.
    pub fn new() -> Self {
        Self {
            state: CydTelemetryState::default(),
            last_rx_time_us: 0,
            last_sync_time_us: -1,
            rx_line: Vec::new(),
            discarding_overflow: false,
            tx_queue: Deque::new(),
        }
    }

    /// Reset internal state, buffer, and timers.
    pub fn reset_state(&mut self) {
        self.state = CydTelemetryState::default();
        self.last_rx_time_us = 0;
        self.last_sync_time_us = -1;
        self.rx_line.clear();
        self.discarding_overflow = false;
        self.tx_queue.clear();
    }

    /// Enqueue an interactive touch command for transmission to host.
    pub fn send_command(&mut self, cmd: CydCommand) -> Result<Vec<u8, 64>, LinkError> {
        let mut buf = Vec::new();
        CydLinkProtocol::encode_command(&cmd, &mut buf)?;
        if self.tx_queue.is_full() {
            let _ = self.tx_queue.pop_front();
        }
        let _ = self.tx_queue.push_back(buf.clone());
        Ok(buf)
    }

    /// Feed raw bytes received from UART into the line accumulator.
    ///
    /// Strips `\r`, accumulates up to 256 bytes, handles overflow discard recovery,
    /// and parses full frames terminated by `\n`.
    /// Returns the number of events parsed and applied.
    pub fn feed_rx_bytes(&mut self, bytes: &[u8], now_us: i64) -> usize {
        let mut events_parsed = 0;

        for &b in bytes {
            if b == b'\r' {
                continue;
            }
            if b == b'\n' {
                if self.discarding_overflow {
                    self.discarding_overflow = false;
                    self.rx_line.clear();
                } else if !self.rx_line.is_empty() {
                    if self.process_line(now_us) {
                        events_parsed += 1;
                    }
                    self.rx_line.clear();
                }
            } else if self.discarding_overflow {
                continue;
            } else if self.rx_line.push(b).is_err() {
                self.discarding_overflow = true;
                self.rx_line.clear();
            }
        }

        events_parsed
    }

    /// Parse a single accumulated line and update telemetry state.
    fn process_line(&mut self, now_us: i64) -> bool {
        // 1. Try standard pure-Rust Squeezelite CYD Link protocol
        if let Ok(msg) = CydLinkProtocol::parse_line(&self.rx_line) {
            match msg {
                CydMessage::PlaybackState(st) => self.state.state = st,
                CydMessage::Volume(v) => self.state.vol = v,
                CydMessage::TrackMetadata { artist, title } => {
                    self.state.artist = artist;
                    self.state.title = title;
                }
                CydMessage::Progress {
                    elapsed_secs,
                    duration_secs,
                } => {
                    self.state.elapsed_secs = elapsed_secs;
                    self.state.duration_secs = duration_secs;
                }
                CydMessage::WifiStatus { connected, rssi } => {
                    self.state.wifi_connected = connected;
                    self.state.wifi_rssi = rssi;
                }
            }
            self.state.link_active = true;
            self.last_rx_time_us = now_us;
            return true;
        }

        // 2. Try JSON line event parsing for legacy test & host compatibility
        if self.rx_line.starts_with(b"{") {
            if let Ok(line_str) = core::str::from_utf8(&self.rx_line) {
                if parse_json_event(line_str, &mut self.state).is_ok() {
                    self.state.link_active = true;
                    self.last_rx_time_us = now_us;
                    return true;
                }
            }
        }

        false
    }

    /// Periodic heartbeat and connection liveness monitor.
    ///
    /// - If link is active: checks for timeout (> 5s since last RX). If timed out,
    ///   marks link inactive and transmits `CMD=STATUS\n`.
    /// - If link is inactive: periodically transmits `CMD=STATUS\n` every 3s.
    pub fn poll_heartbeat(&mut self, now_us: i64) -> Option<Vec<u8, 64>> {
        if self.state.link_active {
            if (now_us - self.last_rx_time_us) > CYD_HEARTBEAT_TIMEOUT_US {
                self.state.link_active = false;
                self.last_sync_time_us = now_us;
                self.send_command(CydCommand::QueryStatus).ok()
            } else {
                None
            }
        } else if self.last_sync_time_us < 0
            || (now_us - self.last_sync_time_us) >= CYD_RESYNC_INTERVAL_US
        {
            self.last_sync_time_us = now_us;
            self.send_command(CydCommand::QueryStatus).ok()
        } else {
            None
        }
    }
}

/// Parse legacy JSON event strings into telemetry state.
pub fn parse_json_event(json_str: &str, state: &mut CydTelemetryState) -> Result<(), LinkError> {
    let trimmed = json_str.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(LinkError::InvalidFrame);
    }

    let event = extract_json_str(trimmed, "event").ok_or(LinkError::InvalidFrame)?;

    match event {
        "meta" => {
            if let Some(t) = extract_json_str(trimmed, "title") {
                state.title.clear();
                let _ = state.title.push_str(truncate_utf8(t, 63));
            } else {
                state.title.clear();
            }
            if let Some(a) = extract_json_str(trimmed, "artist") {
                state.artist.clear();
                let _ = state.artist.push_str(truncate_utf8(a, 63));
            } else {
                state.artist.clear();
            }
            if let Some(alb) = extract_json_str(trimmed, "album") {
                state.album.clear();
                let _ = state.album.push_str(truncate_utf8(alb, 63));
            } else {
                state.album.clear();
            }
            state.link_active = true;
            Ok(())
        }
        "status" => {
            if let Some(st) = extract_json_str(trimmed, "state") {
                state.state = match st {
                    "play" => PlayState::Playing,
                    "pause" => PlayState::Paused,
                    _ => PlayState::Stopped,
                };
            }
            if let Some(el) = extract_json_i64(trimmed, "elapsed") {
                state.elapsed_secs = el.max(0) as u32;
            }
            if let Some(dur) = extract_json_i64(trimmed, "duration") {
                state.duration_secs = dur.max(0) as u32;
            }
            if let Some(v) = extract_json_i64(trimmed, "vol") {
                state.vol = v.clamp(0, 100) as u8;
            }
            state.link_active = true;
            Ok(())
        }
        "sys" => {
            if let Some(m) = extract_json_str(trimmed, "mode") {
                state.mode.clear();
                let _ = state.mode.push_str(truncate_utf8(m, 15));
            }
            if let Some(ip) = extract_json_str(trimmed, "ip") {
                state.ip.clear();
                let _ = state.ip.push_str(truncate_utf8(ip, 31));
            }
            state.link_active = true;
            Ok(())
        }
        _ => Err(LinkError::InvalidFrame),
    }
}

/// Helper to format a command string into JSON or protocol buffer.
pub fn format_command(cmd: &str, val: i32) -> Option<Vec<u8, 64>> {
    let mut out = Vec::new();
    match cmd {
        "toggle" | "play" | "pause" | "next" | "prev" | "sync" => {
            let _ = writeln!(out, "{{\"cmd\":\"{}\"}}", cmd);
            Some(out)
        }
        "vol" => {
            if !(0..=100).contains(&val) {
                return None;
            }
            let _ = writeln!(out, "{{\"cmd\":\"vol\",\"val\":{}}}", val);
            Some(out)
        }
        "vol_step" => {
            let _ = writeln!(out, "{{\"cmd\":\"vol_step\",\"dir\":{}}}", val);
            Some(out)
        }
        _ => None,
    }
}

fn extract_json_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let mut pattern = [0u8; 32];
    let k_bytes = key.as_bytes();
    if k_bytes.len() + 3 > pattern.len() {
        return None;
    }
    pattern[0] = b'"';
    pattern[1..1 + k_bytes.len()].copy_from_slice(k_bytes);
    pattern[1 + k_bytes.len()] = b'"';
    pattern[2 + k_bytes.len()] = b':';
    let p_str = core::str::from_utf8(&pattern[..3 + k_bytes.len()]).ok()?;

    let idx = json.find(p_str)?;
    let rest = json[idx + p_str.len()..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else {
        None
    }
}

fn extract_json_i64(json: &str, key: &str) -> Option<i64> {
    let mut pattern = [0u8; 32];
    let k_bytes = key.as_bytes();
    if k_bytes.len() + 3 > pattern.len() {
        return None;
    }
    pattern[0] = b'"';
    pattern[1..1 + k_bytes.len()].copy_from_slice(k_bytes);
    pattern[1 + k_bytes.len()] = b'"';
    pattern[2 + k_bytes.len()] = b':';
    let p_str = core::str::from_utf8(&pattern[..3 + k_bytes.len()]).ok()?;

    let idx = json.find(p_str)?;
    let rest = json[idx + p_str.len()..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_parse_meta_event() {
        let mut state = CydTelemetryState::default();
        let line = "{\"event\":\"meta\",\"title\":\"Comfortably Numb\",\"artist\":\"Pink Floyd\",\"album\":\"The Wall\"}\n";
        let res = parse_json_event(line, &mut state);

        assert!(res.is_ok());
        assert_eq!(state.title.as_str(), "Comfortably Numb");
        assert_eq!(state.artist.as_str(), "Pink Floyd");
        assert_eq!(state.album.as_str(), "The Wall");
        assert!(state.link_active);
    }

    #[test]
    fn test_protocol_parse_meta_partial_and_long_strings() {
        let mut state = CydTelemetryState::default();

        // Only title provided
        let line1 = "{\"event\":\"meta\",\"title\":\"Solo Song\"}";
        assert!(parse_json_event(line1, &mut state).is_ok());
        assert_eq!(state.title.as_str(), "Solo Song");
        assert_eq!(state.artist.as_str(), "");
        assert_eq!(state.album.as_str(), "");

        // Excessively long string should be safely truncated
        let long_title = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let mut json_buf = heapless::String::<160>::new();
        let _ = write!(json_buf, "{{\"event\":\"meta\",\"title\":\"{}\"}}", long_title);

        assert!(parse_json_event(&json_buf, &mut state).is_ok());
        assert_eq!(state.title.len(), 63);
        assert_eq!(&state.title[..1], "A");
    }

    #[test]
    fn test_protocol_parse_status_event() {
        let mut state = CydTelemetryState::default();
        let line = "{\"event\":\"status\",\"state\":\"play\",\"elapsed\":125,\"duration\":360,\"vol\":85}\n";
        assert!(parse_json_event(line, &mut state).is_ok());

        assert_eq!(state.state, PlayState::Playing);
        assert_eq!(state.elapsed_secs, 125);
        assert_eq!(state.duration_secs, 360);
        assert_eq!(state.vol, 85);
        assert!(state.link_active);

        // Negative elapsed and duration clamp to 0
        let line_neg = "{\"event\":\"status\",\"state\":\"play\",\"elapsed\":-1,\"duration\":-10,\"vol\":-5}\n";
        assert!(parse_json_event(line_neg, &mut state).is_ok());
        assert_eq!(state.elapsed_secs, 0);
        assert_eq!(state.duration_secs, 0);
        assert_eq!(state.vol, 0);
    }

    #[test]
    fn test_protocol_parse_sys_event() {
        let mut state = CydTelemetryState::default();
        let line = "{\"event\":\"sys\",\"mode\":\"Spotify\",\"name\":\"Living Room\",\"ip\":\"192.168.1.150\"}\n";
        assert!(parse_json_event(line, &mut state).is_ok());

        assert_eq!(state.mode.as_str(), "Spotify");
        assert_eq!(state.ip.as_str(), "192.168.1.150");
        assert!(state.link_active);
    }

    #[test]
    fn test_protocol_parse_invalid_or_malformed() {
        let mut state = CydTelemetryState::default();

        assert!(parse_json_event("", &mut state).is_err());
        assert!(parse_json_event("hello world", &mut state).is_err());
        assert!(parse_json_event("{corrupted json", &mut state).is_err());
        assert!(parse_json_event("{\"title\":\"No Event\"}", &mut state).is_err());
        assert!(parse_json_event("{\"event\":\"unknown_xyz\"}", &mut state).is_err());
    }

    #[test]
    fn test_protocol_format_commands() {
        let basic_cmds = ["toggle", "play", "pause", "next", "prev", "sync"];
        for cmd in basic_cmds {
            let formatted = format_command(cmd, 0).expect("Should format");
            assert!(formatted.ends_with(b"\n"));
            let mut expected = Vec::<u8, 64>::new();
            let _ = writeln!(expected, "{{\"cmd\":\"{}\"}}", cmd);
            assert_eq!(formatted, expected);
        }

        // Volume boundaries
        let vol_cmd = format_command("vol", 42).expect("valid vol");
        assert_eq!(&vol_cmd[..], b"{\"cmd\":\"vol\",\"val\":42}\n");

        assert_eq!(&format_command("vol", 0).unwrap()[..], b"{\"cmd\":\"vol\",\"val\":0}\n");
        assert_eq!(&format_command("vol", 100).unwrap()[..], b"{\"cmd\":\"vol\",\"val\":100}\n");
        assert!(format_command("vol", -1).is_none());
        assert!(format_command("vol", 101).is_none());

        // Volume step commands
        assert_eq!(&format_command("vol_step", 5).unwrap()[..], b"{\"cmd\":\"vol_step\",\"dir\":5}\n");
        assert_eq!(&format_command("vol_step", -5).unwrap()[..], b"{\"cmd\":\"vol_step\",\"dir\":-5}\n");

        assert!(format_command("unsupported_cmd", 0).is_none());
    }

    #[test]
    fn test_uart_feed_rx_bytes_and_callbacks() {
        let mut link = CydDisplayLink::new();
        let line = b"{\"event\":\"meta\",\"title\":\"Time\",\"artist\":\"Pink Floyd\",\"album\":\"Dark Side\"}\n";
        let parsed = link.feed_rx_bytes(line, 1000);

        assert_eq!(parsed, 1);
        assert_eq!(link.state.title.as_str(), "Time");
        assert_eq!(link.state.artist.as_str(), "Pink Floyd");
        assert!(link.state.link_active);
    }

    #[test]
    fn test_uart_feed_chunked_bytes() {
        let mut link = CydDisplayLink::new();
        let chunk1 = b"{\"event\":\"st";
        let chunk2 = b"atus\",\"state\":\"pause\",\"el";
        let chunk3 = b"apsed\":50,\"duration\":200,\"vol\":60}\r\n";

        assert_eq!(link.feed_rx_bytes(chunk1, 100), 0);
        assert_eq!(link.feed_rx_bytes(chunk2, 200), 0);
        assert_eq!(link.feed_rx_bytes(chunk3, 300), 1);

        assert_eq!(link.state.state, PlayState::Paused);
        assert_eq!(link.state.elapsed_secs, 50);
        assert_eq!(link.state.duration_secs, 200);
        assert_eq!(link.state.vol, 60);
    }

    #[test]
    fn test_uart_feed_multiple_events_in_single_buffer() {
        let mut link = CydDisplayLink::new();
        let multi = b"{\"event\":\"sys\",\"mode\":\"LMS\",\"ip\":\"10.0.0.5\"}\n{\"event\":\"status\",\"state\":\"play\",\"elapsed\":10,\"duration\":100,\"vol\":50}\n";

        let parsed = link.feed_rx_bytes(multi, 500);
        assert_eq!(parsed, 2);
        assert_eq!(link.state.mode.as_str(), "LMS");
        assert_eq!(link.state.ip.as_str(), "10.0.0.5");
        assert_eq!(link.state.state, PlayState::Playing);
        assert_eq!(link.state.elapsed_secs, 10);
    }

    #[test]
    fn test_uart_feed_buffer_overflow_protection() {
        let mut link = CydDisplayLink::new();

        // Feed 600 bytes without newline
        let spam = [b'X'; 600];
        assert_eq!(link.feed_rx_bytes(&spam, 100), 0);
        assert_eq!(link.feed_rx_bytes(b"\n", 101), 0);

        // Subsequent valid packet should be received and parsed cleanly
        let valid = b"{\"event\":\"meta\",\"title\":\"Recovered\"}\n";
        assert_eq!(link.feed_rx_bytes(valid, 200), 1);
        assert_eq!(link.state.title.as_str(), "Recovered");
    }

    #[test]
    fn test_uart_heartbeat_and_timeout_resync() {
        let mut link = CydDisplayLink::new();

        // 1. Initial state: link inactive. Polling at t=0 transmits sync command
        let cmd = link.poll_heartbeat(0);
        assert_eq!(cmd.as_deref(), Some(&b"CMD=STATUS\n"[..]));
        assert!(!link.state.link_active);

        // 2. Polling before 3 seconds (t=1.5s): should NOT send duplicate sync
        assert!(link.poll_heartbeat(1_500_000).is_none());

        // 3. Polling after 3 seconds (t=3.1s): should retransmit sync command
        let cmd = link.poll_heartbeat(3_100_000);
        assert_eq!(cmd.as_deref(), Some(&b"CMD=STATUS\n"[..]));

        // 4. Valid packet arrives at t=3.5s: link becomes active
        let meta = b"{\"event\":\"meta\",\"title\":\"Sync OK\"}\n";
        assert_eq!(link.feed_rx_bytes(meta, 3_500_000), 1);
        assert!(link.state.link_active);

        // 5. Polling at t=7.0s (3.5s elapsed < 5s timeout): stays active, NO sync command sent
        assert!(link.poll_heartbeat(7_000_000).is_none());
        assert!(link.state.link_active);

        // 6. Polling at t=8.6s (5.1s elapsed > 5s timeout): link becomes inactive!
        let cmd = link.poll_heartbeat(8_600_000);
        assert!(!link.state.link_active);
        assert_eq!(cmd.as_deref(), Some(&b"CMD=STATUS\n"[..]));

        // 7. Polling at t=10.0s (1.4s since sync): no new sync sent
        assert!(link.poll_heartbeat(10_000_000).is_none());

        // 8. Polling at t=11.7s (3.1s since sync): retransmits sync
        let cmd = link.poll_heartbeat(11_700_000);
        assert_eq!(cmd.as_deref(), Some(&b"CMD=STATUS\n"[..]));
    }
}
