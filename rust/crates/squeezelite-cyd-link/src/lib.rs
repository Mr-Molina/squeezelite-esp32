//! `squeezelite-cyd-link`
//!
//! A zero-allocation, bounded `no_std` protocol parser and serializer for the
//! Cheap Yellow Display (CYD) UART link.

#![no_std]

use core::fmt;
use heapless::{String, Vec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

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

/// Incoming interactive commands sent from the CYD display touch controller.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    BufferTooShort,
    BufferOverflow,
    InvalidFrame,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Frame incomplete"),
            Self::BufferOverflow => write!(f, "Frame exceeded maximum bounded capacity"),
            Self::InvalidFrame => write!(f, "Invalid frame delimiters or payload"),
        }
    }
}

pub struct CydLinkProtocol;

impl CydLinkProtocol {
    /// Encodes a CYD message into a delimited packet (`\n` terminated).
    pub fn encode_message(
        msg: &CydMessage,
        out: &mut Vec<u8, 256>,
    ) -> Result<(), LinkError> {
        out.clear();
        match msg {
            CydMessage::PlaybackState(state) => {
                let code = match state {
                    PlayState::Stopped => b'S',
                    PlayState::Playing => b'P',
                    PlayState::Paused => b'A',
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
                out.extend_from_slice(artist.as_bytes()).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b';').map_err(|_| LinkError::BufferOverflow)?;
                out.extend_from_slice(title.as_bytes()).map_err(|_| LinkError::BufferOverflow)?;
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

    /// Parses a single line from a buffer.
    pub fn parse_line(line: &[u8]) -> Result<CydMessage, LinkError> {
        let trimmed = if line.ends_with(b"\n") {
            &line[..line.len() - 1]
        } else {
            line
        };

        if let Some(rest) = trimmed.strip_prefix(b"STATE=") {
            match rest.first() {
                Some(b'P') => Ok(CydMessage::PlaybackState(PlayState::Playing)),
                Some(b'A') => Ok(CydMessage::PlaybackState(PlayState::Paused)),
                Some(b'S') => Ok(CydMessage::PlaybackState(PlayState::Stopped)),
                _ => Err(LinkError::InvalidFrame),
            }
        } else if let Some(rest) = trimmed.strip_prefix(b"VOL=") {
            let s = core::str::from_utf8(rest).map_err(|_| LinkError::InvalidFrame)?;
            let vol: u8 = parse_u8(s).ok_or(LinkError::InvalidFrame)?;
            Ok(CydMessage::Volume(vol))
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
                out.extend_from_slice(b"CMD=VOL:").map_err(|_| LinkError::BufferOverflow)?;
                let mut num_buf = [0u8; 3];
                let s = format_u8(*vol, &mut num_buf);
                out.extend_from_slice(s).map_err(|_| LinkError::BufferOverflow)?;
                out.push(b'\n').map_err(|_| LinkError::BufferOverflow)?;
            }
        }
        Ok(())
    }

    /// Parses an incoming interactive command from the CYD display.
    pub fn parse_command(line: &[u8]) -> Result<CydCommand, LinkError> {
        let trimmed = if line.ends_with(b"\n") {
            &line[..line.len() - 1]
        } else {
            line
        };

        if let Some(rest) = trimmed.strip_prefix(b"CMD=") {
            match rest {
                b"PLAY" => Ok(CydCommand::Play),
                b"PAUSE" => Ok(CydCommand::Pause),
                b"TOGGLE" => Ok(CydCommand::Toggle),
                b"NEXT" => Ok(CydCommand::Next),
                b"PREV" => Ok(CydCommand::Previous),
                b"STATUS" => Ok(CydCommand::QueryStatus),
                other if other.starts_with(b"VOL:") => {
                    let s = core::str::from_utf8(&other[4..]).map_err(|_| LinkError::InvalidFrame)?;
                    let vol: u8 = parse_u8(s).ok_or(LinkError::InvalidFrame)?;
                    Ok(CydCommand::SetVolume(vol))
                }
                _ => Err(LinkError::InvalidFrame),
            }
        } else {
            Err(LinkError::InvalidFrame)
        }
    }
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
        temp[i] = b'0' + (val % 10) as u8;
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
    fn test_encode_and_parse_commands() {
        let mut buf = Vec::new();

        CydLinkProtocol::encode_command(&CydCommand::Play, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=PLAY\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::Play);

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

        CydLinkProtocol::encode_command(&CydCommand::QueryStatus, &mut buf).unwrap();
        assert_eq!(&buf[..], b"CMD=STATUS\n");
        assert_eq!(CydLinkProtocol::parse_command(&buf).unwrap(), CydCommand::QueryStatus);
    }
}
