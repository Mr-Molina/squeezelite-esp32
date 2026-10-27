//! `squeezelite-cyd-link`
//!
//! A zero-allocation, `no_std` UART framing and telemetry link protocol
//! between Squeezelite-ESP32 / Louder-ESP32-Plus audio engine and the
//! CYD (Cheap Yellow Display) front-panel touch controller.
//!
//! Text-based, newline-delimited (`\n` or `\r\n`) protocol for easy debugging,
//! human inspection, and serial terminal compatibility.

#![no_std]
#![forbid(unsafe_code)]

use core::fmt;
use heapless::{String, Vec};

/// High-level playback status sent to the CYD display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

/// Outbound telemetry and state messages sent from player to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CydMessage {
    PlaybackState(PlayState),
    Volume(u8),
    TrackMetadata {
        artist: String<64>,
        title: String<64>,
    },
    Progress {
        elapsed_secs: u32,
        duration_secs: u32,
    },
    WifiStatus {
        connected: bool,
        rssi: i8,
    },
}

/// Inbound interactive commands from display touch screen to player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CydCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    SetVolume(u8),
    QueryStatus,
}

/// Framing and serialization errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    BufferOverflow,
    InvalidFrame,
    ValueOutOfRange,
}

/// Convenience alias matching work order and protocol specifications.
pub type CydLinkError = LinkError;

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferOverflow => write!(f, "Buffer capacity exceeded"),
            Self::InvalidFrame => write!(f, "Invalid frame format or delimiter"),
            Self::ValueOutOfRange => write!(f, "Value out of valid range (0..=100)"),
        }
    }
}

pub struct CydLinkProtocol;

impl CydLinkProtocol {
    /// Strips trailing `\n` and `\r` line delimiters.
    pub fn trim_crlf(line: &[u8]) -> &[u8] {
        let mut trimmed = line;
        if trimmed.ends_with(b"\n") {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        if trimmed.ends_with(b"\r") {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        trimmed
    }

    /// Encodes a telemetry message into a delimited frame (`\n` terminated).
    /// Sanitizes metadata strings to prevent delimiter injection.
    pub fn encode_message(
        msg: &CydMessage,
        out: &mut Vec<u8, 256>,
    ) -> Result<(), LinkError> {
        out.clear();
        match msg {
            CydMessage::PlaybackState(state) => {
                let code = match state {
                    PlayState::Playing => b'P',
                    PlayState::Paused => b'A',
                    PlayState::Stopped => b'S',
                };
                out.extend_from_slice(b"STATE=").map_err(|_| LinkError::BufferOverflow)?;
                out.push(code).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
            CydMessage::Volume(vol) => {
                out.extend_from_slice(b"VOL=").map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf = [0u8; 3];
                let s = format_u8(*vol, &mut num_buf);
                out.extend_from_slice(s).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
            CydMessage::TrackMetadata { artist, title } => {
                out.extend_from_slice(b"META=").map_err(|_| LinkError::BufferOverflow)?;
                // Sanitize delimiters (\r, \n, ;) to prevent protocol delimiter injection (CYD-001)
                for &b in artist.as_bytes() {
                    let c = if b == b'\n' || b == b'\r' || b == b';' { b' ' } else { b };
                    out.push(c).map_err(|_| LinkError::BufferOverflow)?;
                }
                out.push(b';').map_err(|_| LinkError::BufferOverflow)?;
                // Sanitize newlines only (\r, \n), preserving semicolons in title (LOGIC-404)
                for &b in title.as_bytes() {
                    let c = if b == b'\n' || b == b'\r' { b' ' } else { b };
                    out.push(c).map_err(|_| LinkError::BufferOverflow)?;
                }
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
            CydMessage::Progress { elapsed_secs, duration_secs } => {
                out.extend_from_slice(b"TIME=").map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf = [0u8; 10];
                let s1 = format_u32(*elapsed_secs, &mut num_buf);
                out.extend_from_slice(s1).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'/').map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf2 = [0u8; 10];
                let s2 = format_u32(*duration_secs, &mut num_buf2);
                out.extend_from_slice(s2).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
            CydMessage::WifiStatus { connected, rssi } => {
                out.extend_from_slice(b"WIFI=").map_err(|_| LinkError::BufferOverflow)?;
                out.push(if *connected { b'1' } else { b'0' }).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b';').map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf = [0u8; 5];
                let s = format_i8(*rssi, &mut num_buf);
                out.extend_from_slice(s).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
        }
        Ok(())
    }

    /// Parses a single line from a buffer (supporting `\n` or `\r\n`).
    /// Implements symmetric deserialization for all message types (CYD-005, CYD-006).
    pub fn parse_line(line: &[u8]) -> Result<CydMessage, LinkError> {
        let trimmed = Self::trim_crlf(line);

        if let Some(rest) = trimmed.strip_prefix(b"STATE=") {
            // Exact single-byte match to prevent semantic inversion (CYD-005)
            match rest {
                b"P" => Ok(CydMessage::PlaybackState(PlayState::Playing)),
                b"A" => Ok(CydMessage::PlaybackState(PlayState::Paused)),
                b"S" => Ok(CydMessage::PlaybackState(PlayState::Stopped)),
                _ => Err(LinkError::InvalidFrame),
            }
        } else if let Some(rest) = trimmed.strip_prefix(b"VOL=") {
            let s = core::str::from_utf8(rest).map_err(|_| LinkError::InvalidFrame)?;
            let vol: u8 = parse_u8(s).ok_or(LinkError::InvalidFrame)?;
            Ok(CydMessage::Volume(vol))
        } else if let Some(rest) = trimmed.strip_prefix(b"META=") {
            let mut parts = rest.splitn(2, |&b| b == b';');
            let artist_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let title_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let artist_str = core::str::from_utf8(artist_bytes).map_err(|_| LinkError::InvalidFrame)?;
            let title_str = core::str::from_utf8(title_bytes).map_err(|_| LinkError::InvalidFrame)?;
            let artist_trunc = truncate_utf8(artist_str, 64);
            let title_trunc = truncate_utf8(title_str, 64);
            let mut artist = String::new();
            artist.push_str(artist_trunc).map_err(|_| LinkError::BufferOverflow)?;
            let mut title = String::new();
            title.push_str(title_trunc).map_err(|_| LinkError::BufferOverflow)?;
            Ok(CydMessage::TrackMetadata { artist, title })
        } else if let Some(rest) = trimmed.strip_prefix(b"TIME=") {
            let mut parts = rest.splitn(2, |&b| b == b'/');
            let elapsed_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let duration_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let elapsed_str = core::str::from_utf8(elapsed_bytes).map_err(|_| LinkError::InvalidFrame)?;
            let duration_str = core::str::from_utf8(duration_bytes).map_err(|_| LinkError::InvalidFrame)?;
            let elapsed: u32 = parse_u32(elapsed_str).ok_or(LinkError::InvalidFrame)?;
            let duration: u32 = parse_u32(duration_str).ok_or(LinkError::InvalidFrame)?;
            Ok(CydMessage::Progress { elapsed_secs: elapsed, duration_secs: duration })
        } else if let Some(rest) = trimmed.strip_prefix(b"WIFI=") {
            let mut parts = rest.splitn(2, |&b| b == b';');
            let conn_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let rssi_bytes = parts.next().ok_or(LinkError::InvalidFrame)?;
            let connected = match conn_bytes {
                b"1" => true,
                b"0" => false,
                _ => return Err(LinkError::InvalidFrame),
            };
            let rssi_str = core::str::from_utf8(rssi_bytes).map_err(|_| LinkError::InvalidFrame)?;
            let rssi: i8 = parse_i8(rssi_str).ok_or(LinkError::InvalidFrame)?;
            Ok(CydMessage::WifiStatus { connected, rssi })
        } else {
            Err(LinkError::InvalidFrame)
        }
    }

    /// Encodes an interactive touch command into a delimited frame (`\n` terminated).
    pub fn encode_command(
        cmd: &CydCommand,
        out: &mut Vec<u8, 64>,
    ) -> Result<(), LinkError> {
        out.clear();
        match cmd {
            CydCommand::Play => out.extend_from_slice(b"CMD=PLAY\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::Pause => out.extend_from_slice(b"CMD=PAUSE\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::Toggle => out.extend_from_slice(b"CMD=TOGGLE\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::Next => out.extend_from_slice(b"CMD=NEXT\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::Previous => out.extend_from_slice(b"CMD=PREV\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::QueryStatus => out.extend_from_slice(b"CMD=STATUS\n").map_err(|_| LinkError::BufferOverflow)?,
            CydCommand::SetVolume(vol) => {
                if *vol > 100 {
                    return Err(LinkError::ValueOutOfRange);
                }
                out.extend_from_slice(b"CMD=VOL:").map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf = [0u8; 3];
                let s = format_u8(*vol, &mut num_buf);
                out.extend_from_slice(s).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
        }
        Ok(())
    }

    /// Encodes an interactive touch command into C-firmware compatible NDJSON (`\n` terminated).
    pub fn encode_json_command(
        cmd: &CydCommand,
        buf: &mut [u8],
    ) -> Result<usize, LinkError> {
        match cmd {
            CydCommand::Toggle => copy_slice(b"{\"cmd\":\"toggle\"}\n", buf),
            CydCommand::Play => copy_slice(b"{\"cmd\":\"play\"}\n", buf),
            CydCommand::Pause => copy_slice(b"{\"cmd\":\"pause\"}\n", buf),
            CydCommand::Next => copy_slice(b"{\"cmd\":\"next\"}\n", buf),
            CydCommand::Previous => copy_slice(b"{\"cmd\":\"prev\"}\n", buf),
            CydCommand::QueryStatus => copy_slice(b"{\"cmd\":\"sync\"}\n", buf),
            CydCommand::SetVolume(vol) => {
                if *vol > 100 {
                    return Err(LinkError::ValueOutOfRange);
                }
                let prefix = b"{\"cmd\":\"vol\",\"val\":";
                let suffix = b"}\n";
                let mut num_buf = [0u8; 3];
                let digits = format_u8(*vol, &mut num_buf);
                let total = prefix.len() + digits.len() + suffix.len();
                if buf.len() < total {
                    return Err(LinkError::BufferOverflow);
                }
                buf[..prefix.len()].copy_from_slice(prefix);
                buf[prefix.len()..prefix.len() + digits.len()].copy_from_slice(digits);
                buf[prefix.len() + digits.len()..total].copy_from_slice(suffix);
                Ok(total)
            }
        }
    }

    /// Parses an incoming interactive command from the CYD display (supports `\n` and `\r\n`,
    /// both `CMD=...` wire format and C-firmware NDJSON `{"cmd":...}` format).
    pub fn parse_command<B: AsRef<[u8]> + ?Sized>(line: &B) -> Result<CydCommand, LinkError> {
        let trimmed = Self::trim_crlf(line.as_ref());

        if let Some(rest) = trimmed.strip_prefix(b"CMD=") {
            match rest {
                b"PLAY" => Ok(CydCommand::Play),
                b"PAUSE" => Ok(CydCommand::Pause),
                b"TOGGLE" => Ok(CydCommand::Toggle),
                b"NEXT" => Ok(CydCommand::Next),
                b"PREV" => Ok(CydCommand::Previous),
                b"STATUS" => Ok(CydCommand::QueryStatus),
                other if other.starts_with(b"VOL:") || other.starts_with(b"VOL=") => {
                    let s = core::str::from_utf8(&other[4..]).map_err(|_| LinkError::InvalidFrame)?;
                    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(LinkError::InvalidFrame);
                    }
                    let vol: u32 = parse_u32(s).ok_or(LinkError::ValueOutOfRange)?;
                    if vol > 100 {
                        return Err(LinkError::ValueOutOfRange);
                    }
                    Ok(CydCommand::SetVolume(vol as u8))
                }
                _ => Err(LinkError::InvalidFrame),
            }
        } else if let Ok(s) = core::str::from_utf8(trimmed) {
            let s = s.trim();
            if s.starts_with('{') && s.ends_with('}') {
                parse_json_command(s)
            } else {
                Err(LinkError::InvalidFrame)
            }
        } else {
            Err(LinkError::InvalidFrame)
        }
    }
}

fn copy_slice(src: &[u8], dst: &mut [u8]) -> Result<usize, LinkError> {
    if dst.len() < src.len() {
        return Err(LinkError::BufferOverflow);
    }
    dst[..src.len()].copy_from_slice(src);
    Ok(src.len())
}

fn find_json_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let bytes = json.as_bytes();
    let k_bytes = key.as_bytes();
    let pat_len = k_bytes.len() + 2;

    let mut in_quotes = false;
    let mut escaped = false;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if in_quotes {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_quotes = false;
            }
            i += 1;
        } else {
            if b == b'"' {
                // Outside string quotes: check if this quote starts `"key":`
                if i + pat_len <= bytes.len()
                    && &bytes[i + 1..i + 1 + k_bytes.len()] == k_bytes
                    && bytes[i + 1 + k_bytes.len()] == b'"'
                {
                    // Check if followed by optional whitespace and ':'
                    let mut colon_idx = i + pat_len;
                    while colon_idx < bytes.len() && bytes[colon_idx].is_ascii_whitespace() {
                        colon_idx += 1;
                    }
                    if colon_idx < bytes.len() && bytes[colon_idx] == b':' {
                        let after_colon = &json[colon_idx + 1..];
                        return Some(after_colon.trim_start());
                    }
                }
                // Entered a string property or string value
                in_quotes = true;
                escaped = false;
                i += 1;
            } else {
                i += 1;
            }
        }
    }
    None
}

fn parse_json_command(json: &str) -> Result<CydCommand, LinkError> {
    let cmd_rest = find_json_field(json, "cmd").ok_or(LinkError::InvalidFrame)?;
    let cmd_inner = cmd_rest.strip_prefix('"').ok_or(LinkError::InvalidFrame)?;
    let end_quote = cmd_inner.find('"').ok_or(LinkError::InvalidFrame)?;
    let cmd_name = &cmd_inner[..end_quote];

    match cmd_name {
        "toggle" => Ok(CydCommand::Toggle),
        "play" => Ok(CydCommand::Play),
        "pause" => Ok(CydCommand::Pause),
        "next" => Ok(CydCommand::Next),
        "prev" => Ok(CydCommand::Previous),
        "sync" | "status" => Ok(CydCommand::QueryStatus),
        "vol" => {
            let val_rest = find_json_field(json, "val").ok_or(LinkError::InvalidFrame)?;
            let end = val_rest
                .find(|c: char| c == ',' || c == '}' || c.is_ascii_whitespace())
                .unwrap_or(val_rest.len());
            let num_str = &val_rest[..end];
            if num_str.is_empty() {
                return Err(LinkError::InvalidFrame);
            }
            if let Some(neg) = num_str.strip_prefix('-') {
                if !neg.is_empty() && neg.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(LinkError::ValueOutOfRange);
                }
                return Err(LinkError::InvalidFrame);
            }
            if !num_str.bytes().all(|b| b.is_ascii_digit()) {
                return Err(LinkError::InvalidFrame);
            }
            let vol = parse_u32(num_str).ok_or(LinkError::ValueOutOfRange)?;
            if vol > 100 {
                return Err(LinkError::ValueOutOfRange);
            }
            Ok(CydCommand::SetVolume(vol as u8))
        }
        _ => Err(LinkError::InvalidFrame),
    }
}

/// Truncates a UTF-8 string slice to at most `max_bytes` without splitting code points.
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

fn format_u8(mut val: u8, buf: &mut [u8; 3]) -> &[u8] {
    if val == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = 0;
    let mut temp = [0u8; 3];
    while val > 0 {
        temp[i] = b'0' + (val % 10);
        val /= 10;
        i += 1;
    }
    for j in 0..i {
        buf[j] = temp[i - 1 - j];
    }
    &buf[..i]
}

fn format_u32(mut val: u32, buf: &mut [u8; 10]) -> &[u8] {
    if val == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = 0;
    let mut temp = [0u8; 10];
    while val > 0 {
        temp[i] = b'0' + ((val % 10) as u8);
        val /= 10;
        i += 1;
    }
    for j in 0..i {
        buf[j] = temp[i - 1 - j];
    }
    &buf[..i]
}

fn format_i8(val: i8, buf: &mut [u8; 5]) -> &[u8] {
    if val < 0 {
        buf[0] = b'-';
        let mut sub = [0u8; 3];
        let s = format_u8(val.unsigned_abs(), &mut sub);
        buf[1..1 + s.len()].copy_from_slice(s);
        &buf[..1 + s.len()]
    } else {
        let mut sub = [0u8; 3];
        let s = format_u8(val as u8, &mut sub);
        buf[..s.len()].copy_from_slice(s);
        &buf[..s.len()]
    }
}

fn parse_u8(s: &str) -> Option<u8> {
    if s.is_empty() {
        return None;
    }
    let mut res: u8 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        res = res.checked_mul(10)?.checked_add(b - b'0')?;
    }
    Some(res)
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut res: u32 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        res = res.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(res)
}

fn parse_i8(s: &str) -> Option<i8> {
    if s.is_empty() {
        return None;
    }
    let (is_neg, digits) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, s)
    };
    let u = parse_u8(digits)?;
    if is_neg {
        if u > 128 {
            return None;
        }
        Some((u as i8).wrapping_neg())
    } else {
        if u > 127 {
            return None;
        }
        Some(u as i8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_and_parse_state() {
        let mut buf = Vec::new();
        CydLinkProtocol::encode_message(&CydMessage::PlaybackState(PlayState::Playing), &mut buf).unwrap();
        assert_eq!(&buf[..], b"STATE=P\n");

        let parsed = CydLinkProtocol::parse_line(&buf).unwrap();
        assert_eq!(parsed, CydMessage::PlaybackState(PlayState::Playing));

        // Exact match test: STATE=PAUSE must NOT parse as Playing
        assert_eq!(CydLinkProtocol::parse_line(b"STATE=PAUSE\n"), Err(LinkError::InvalidFrame));
    }

    #[test]
    fn test_encode_and_parse_volume() {
        let mut buf = Vec::new();
        CydLinkProtocol::encode_message(&CydMessage::Volume(85), &mut buf).unwrap();
        assert_eq!(&buf[..], b"VOL=85\n");

        let parsed = CydLinkProtocol::parse_line(&buf).unwrap();
        assert_eq!(parsed, CydMessage::Volume(85));
    }

    #[test]
    fn test_encode_and_parse_commands_crlf() {
        let mut buf = Vec::new();

        CydLinkProtocol::encode_command(&CydCommand::Play, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=PLAY\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Play);
        // CRLF tolerance test
        assert_eq!(CydLinkProtocol::parse_command(b"CMD=PLAY\r\n").unwrap(), CydCommand::Play);
        assert_eq!(CydLinkProtocol::parse_command(b"CMD=VOL:80\r\n").unwrap(), CydCommand::SetVolume(80));
        assert_eq!(CydLinkProtocol::parse_command(b"CMD=VOL=80\r\n").unwrap(), CydCommand::SetVolume(80));
        assert_eq!(CydLinkProtocol::parse_command(b"CMD=VOL=50\n").unwrap(), CydCommand::SetVolume(50));

        CydLinkProtocol::encode_command(&CydCommand::Pause, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=PAUSE\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Pause);

        CydLinkProtocol::encode_command(&CydCommand::Toggle, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=TOGGLE\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Toggle);

        CydLinkProtocol::encode_command(&CydCommand::Next, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=NEXT\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Next);

        CydLinkProtocol::encode_command(&CydCommand::Previous, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=PREV\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Previous);

        CydLinkProtocol::encode_command(&CydCommand::SetVolume(75), &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=VOL:75\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::SetVolume(75));

        // Volume out of range (>100) validations
        assert_eq!(
            CydLinkProtocol::encode_command(&CydCommand::SetVolume(101), &mut buf),
            Err(LinkError::ValueOutOfRange)
        );
        assert_eq!(
            CydLinkProtocol::parse_command(b"CMD=VOL:101\n"),
            Err(LinkError::ValueOutOfRange)
        );
        assert_eq!(
            CydLinkProtocol::parse_command(b"CMD=VOL=255\r\n"),
            Err(LinkError::ValueOutOfRange)
        );
        assert_eq!(
            CydLinkProtocol::parse_command(b"CMD=VOL:999\n"),
            Err(LinkError::ValueOutOfRange)
        );

        CydLinkProtocol::encode_command(&CydCommand::QueryStatus, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=STATUS\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::QueryStatus);
    }

    #[test]
    fn test_metadata_sanitization_and_roundtrip() {
        let mut artist = String::new();
        artist.push_str("AC;DC\nInjected").unwrap();
        let mut title = String::new();
        title.push_str("Track; Part 2\rCMD=VOL:100").unwrap();

        let msg = CydMessage::TrackMetadata { artist, title };
        let mut buf = Vec::new();
        CydLinkProtocol::encode_message(&msg, &mut buf).unwrap();

        // Delimiters should be replaced with spaces, but ';' preserved in title (LOGIC-404)
        let s = core::str::from_utf8(&buf).unwrap();
        assert!(!s.contains("AC;DC"));
        assert!(s.contains("AC DC"));
        assert!(!s.contains("\nInjected"));
        assert!(s.contains("Track; Part 2 CMD=VOL:100"));

        let parsed = CydLinkProtocol::parse_line(&buf).unwrap();
        match parsed {
            CydMessage::TrackMetadata { artist: a, title: t } => {
                assert_eq!(a.as_str(), "AC DC Injected");
                assert_eq!(t.as_str(), "Track; Part 2 CMD=VOL:100");
            }
            _ => panic!("Expected TrackMetadata"),
        }
    }

    #[test]
    fn test_metadata_truncation_utf8() {
        // Test UTF-8 safe truncation helper directly
        let s = "Hello, world! 12345";
        assert_eq!(truncate_utf8(s, 5), "Hello");

        // Multibyte: Euro symbol is 3 bytes (E2 82 AC)
        let euro = "€€€€€";
        assert_eq!(truncate_utf8(euro, 7), "€€");
        assert_eq!(truncate_utf8(euro, 6), "€€");
        assert_eq!(truncate_utf8(euro, 5), "€");

        // Overlong metadata in parse_line (>64 bytes)
        let long_line = b"META=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA;BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\n";
        let parsed = CydLinkProtocol::parse_line(long_line).unwrap();
        match parsed {
            CydMessage::TrackMetadata { artist, title } => {
                assert_eq!(artist.len(), 64);
                assert_eq!(title.len(), 64);
            }
            _ => panic!("Expected TrackMetadata"),
        }
    }

    #[test]
    fn test_progress_and_wifi_roundtrip() {
        let mut buf = Vec::new();
        let p_msg = CydMessage::Progress { elapsed_secs: 125, duration_secs: 300 };
        CydLinkProtocol::encode_message(&p_msg, &mut buf).unwrap();
        assert_eq!(CydLinkProtocol::parse_line(&buf).unwrap(), p_msg);

        let w_msg = CydMessage::WifiStatus { connected: true, rssi: -65 };
        CydLinkProtocol::encode_message(&w_msg, &mut buf).unwrap();
        assert_eq!(CydLinkProtocol::parse_line(&buf).unwrap(), w_msg);
    }

    #[test]
    fn test_parse_and_encode_json_commands() {
        // Parse C-firmware NDJSON commands (&str and &[u8])
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"toggle\"}\n"), Ok(CydCommand::Toggle));
        assert_eq!(CydLinkProtocol::parse_command(b"{\"cmd\":\"play\"}\r\n"), Ok(CydCommand::Play));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"pause\"}"), Ok(CydCommand::Pause));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"next\"}"), Ok(CydCommand::Next));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"prev\"}"), Ok(CydCommand::Previous));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"sync\"}"), Ok(CydCommand::QueryStatus));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"status\"}"), Ok(CydCommand::QueryStatus));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":0}"), Ok(CydCommand::SetVolume(0)));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":80}\n"), Ok(CydCommand::SetVolume(80)));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":100}"), Ok(CydCommand::SetVolume(100)));

        // Out-of-range volume returns ValueOutOfRange
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":101}"), Err(LinkError::ValueOutOfRange));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":255}"), Err(LinkError::ValueOutOfRange));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":-1}"), Err(LinkError::ValueOutOfRange));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\",\"val\":99999}"), Err(LinkError::ValueOutOfRange));

        // Malformed JSON commands return InvalidFrame
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"vol\"}"), Err(LinkError::InvalidFrame));
        assert_eq!(CydLinkProtocol::parse_command("{\"cmd\":\"unknown\"}"), Err(LinkError::InvalidFrame));
        assert_eq!(CydLinkProtocol::parse_command("{\"val\":50}"), Err(LinkError::InvalidFrame));

        // JSON commands with key pattern inside string values must not falsely match
        assert_eq!(
            CydLinkProtocol::parse_command("{\"title\":\"Song with \\\"cmd\\\": \\\"pause\\\" in title\",\"cmd\":\"play\"}\n"),
            Ok(CydCommand::Play)
        );

        // Encode NDJSON commands and roundtrip
        let mut raw = [0u8; 64];
        let cases = [
            (CydCommand::Toggle, &b"{\"cmd\":\"toggle\"}\n"[..]),
            (CydCommand::Play, &b"{\"cmd\":\"play\"}\n"[..]),
            (CydCommand::Pause, &b"{\"cmd\":\"pause\"}\n"[..]),
            (CydCommand::Next, &b"{\"cmd\":\"next\"}\n"[..]),
            (CydCommand::Previous, &b"{\"cmd\":\"prev\"}\n"[..]),
            (CydCommand::QueryStatus, &b"{\"cmd\":\"sync\"}\n"[..]),
            (CydCommand::SetVolume(80), &b"{\"cmd\":\"vol\",\"val\":80}\n"[..]),
        ];
        for (cmd, expected) in cases {
            let n = CydLinkProtocol::encode_json_command(&cmd, &mut raw).unwrap();
            assert_eq!(&raw[..n], expected);
            assert_eq!(CydLinkProtocol::parse_command(&raw[..n]), Ok(cmd));
        }

        // Encode out-of-range volume and small buffer
        assert_eq!(
            CydLinkProtocol::encode_json_command(&CydCommand::SetVolume(101), &mut raw),
            Err(LinkError::ValueOutOfRange)
        );
        let mut tiny = [0u8; 4];
        assert_eq!(
            CydLinkProtocol::encode_json_command(&CydCommand::Toggle, &mut tiny),
            Err(LinkError::BufferOverflow)
        );
    }
}
