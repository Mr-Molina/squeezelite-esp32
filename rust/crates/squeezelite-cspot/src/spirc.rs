//! Spirc command parsing and state machine for Spotify Connect.
//!
//! Decodes and encodes Spirc frames (matching `spirc.proto`) and manages
//! playback state transitions (kPlay, kPause, kNext, kPrev, kSetVolume).

#![allow(clippy::identity_op)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

/// Maximum Spotify Connect volume level (`0..=65535`).
pub const SPOTIFY_MAX_VOLUME: u32 = 65535;

/// Default volume step for `VolumeUp` / `VolumeDown` Spirc commands.
pub const SPOTIFY_VOLUME_STEP: u32 = 4096;

/// Base URL for Spotify CDN album/track artwork images.
pub const SPOTIFY_IMAGE_CDN_BASE: &str = "https://i.scdn.co/image/";

/// Decoded Spotify track metadata (`metadata.proto`) with CDN artwork URL
/// builder.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrackMetadata {
    /// Track title (`Track.name`, field 2).
    pub title: String,
    /// Primary artist name (`Track.artist[0].name`, field 4 -> field 2).
    pub artist: String,
    /// Album name (`Track.album.name`, field 3 -> field 2).
    pub album: String,
    /// Track duration in milliseconds (`Track.duration`, field 7).
    pub duration_ms: u32,
    /// Hex-encoded 20-byte Spotify artwork image ID
    /// (`Album.cover_group.image[0].file_id`).
    pub artwork_image_id: Option<String>,
}

impl TrackMetadata {
    /// Creates a new `TrackMetadata` descriptor.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        duration_ms: u32,
        artwork_image_id: Option<String>,
    ) -> Self {
        Self {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration_ms,
            artwork_image_id,
        }
    }

    /// Formats a 20-byte raw Spotify image `file_id` into a lowercase hex string.
    #[must_use]
    pub fn bytes_to_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    /// Builds a Spotify CDN artwork URL (`https://i.scdn.co/image/<hex_id>`) from hex ID.
    #[must_use]
    pub fn build_artwork_url(hex_id: &str) -> String {
        let trimmed = hex_id.trim_start_matches(SPOTIFY_IMAGE_CDN_BASE);
        format!("{SPOTIFY_IMAGE_CDN_BASE}{trimmed}")
    }

    /// Builds a Spotify CDN artwork URL from raw `file_id` bytes.
    #[must_use]
    pub fn build_artwork_url_from_bytes(file_id: &[u8]) -> String {
        let hex_id = Self::bytes_to_hex(file_id);
        Self::build_artwork_url(&hex_id)
    }

    /// Returns the full CDN artwork URL (`https://i.scdn.co/image/<hex_id>`) if present.
    #[must_use]
    pub fn artwork_url(&self) -> Option<String> {
        self.artwork_image_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(Self::build_artwork_url)
    }

    fn decode_hex(hex: &str) -> Option<Vec<u8>> {
        if hex.is_empty() || (hex.len() & 1) != 0 {
            return None;
        }
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() / 2);
        let mut idx = 0usize;
        while idx < bytes.len() {
            let pair = core::str::from_utf8(&bytes[idx..idx + 2]).ok()?;
            let val = u8::from_str_radix(pair, 16).ok()?;
            out.push(val);
            idx += 2;
        }
        Some(out)
    }

    /// Encodes `TrackMetadata` into `metadata.proto` `Track` protobuf bytes.
    #[must_use]
    pub fn encode_protobuf(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Field 2: name (title)
        if !self.title.is_empty() {
            encode_varint(&mut buf, (2 << 3) | 2);
            let title_bytes = self.title.as_bytes();
            encode_varint(&mut buf, title_bytes.len() as u64);
            buf.extend_from_slice(title_bytes);
        }

        // Field 3: album (Album submessage)
        if !self.album.is_empty() || self.artwork_image_id.is_some() {
            let mut album_buf = Vec::new();
            if !self.album.is_empty() {
                encode_varint(&mut album_buf, (2 << 3) | 2);
                let album_bytes = self.album.as_bytes();
                encode_varint(&mut album_buf, album_bytes.len() as u64);
                album_buf.extend_from_slice(album_bytes);
            }
            if let Some(hex_id) = &self.artwork_image_id {
                if !hex_id.is_empty() {
                    let file_id_bytes =
                        Self::decode_hex(hex_id).unwrap_or_else(|| hex_id.as_bytes().to_vec());
                    let mut image_buf = Vec::new();
                    encode_varint(&mut image_buf, (1 << 3) | 2);
                    encode_varint(&mut image_buf, file_id_bytes.len() as u64);
                    image_buf.extend_from_slice(&file_id_bytes);

                    let mut cover_group_buf = Vec::new();
                    encode_varint(&mut cover_group_buf, (1 << 3) | 2);
                    encode_varint(&mut cover_group_buf, image_buf.len() as u64);
                    cover_group_buf.extend_from_slice(&image_buf);

                    // Album field 17 (0x11): cover_group
                    encode_varint(&mut album_buf, (0x11 << 3) | 2);
                    encode_varint(&mut album_buf, cover_group_buf.len() as u64);
                    album_buf.extend_from_slice(&cover_group_buf);
                }
            }
            encode_varint(&mut buf, (3 << 3) | 2);
            encode_varint(&mut buf, album_buf.len() as u64);
            buf.extend_from_slice(&album_buf);
        }

        // Field 4: artist (Artist submessage)
        if !self.artist.is_empty() {
            let mut artist_buf = Vec::new();
            encode_varint(&mut artist_buf, (2 << 3) | 2);
            let artist_bytes = self.artist.as_bytes();
            encode_varint(&mut artist_buf, artist_bytes.len() as u64);
            artist_buf.extend_from_slice(artist_bytes);

            encode_varint(&mut buf, (4 << 3) | 2);
            encode_varint(&mut buf, artist_buf.len() as u64);
            buf.extend_from_slice(&artist_buf);
        }

        // Field 7: duration (milliseconds)
        if self.duration_ms > 0 {
            encode_varint(&mut buf, 7 << 3);
            encode_varint(&mut buf, self.duration_ms as u64);
        }

        buf
    }

    /// Decodes `TrackMetadata` from `metadata.proto` `Track` protobuf bytes.
    pub fn decode_protobuf(data: &[u8]) -> Result<Self, SpircError> {
        let mut pos = 0usize;
        let mut title = String::new();
        let mut artist = String::new();
        let mut album = String::new();
        let mut duration_ms = 0u32;
        let mut artwork_image_id = None;

        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;

            match wire_type {
                0 => {
                    let val = decode_varint(data, &mut pos)?;
                    if field_num == 7 {
                        duration_ms = val as u32;
                    }
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    let sub = &data[pos..pos + len];
                    pos += len;

                    match field_num {
                        2 => {
                            let s =
                                core::str::from_utf8(sub).map_err(|_| SpircError::InvalidUtf8)?;
                            title = s.to_string();
                        }
                        3 => {
                            Self::parse_album_submessage(sub, &mut album, &mut artwork_image_id)?;
                        }
                        4 if artist.is_empty() => {
                            Self::parse_artist_submessage(sub, &mut artist)?;
                        }
                        _ => {}
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }

        Ok(Self {
            title,
            artist,
            album,
            duration_ms,
            artwork_image_id,
        })
    }

    fn parse_artist_submessage(data: &[u8], artist_out: &mut String) -> Result<(), SpircError> {
        let mut pos = 0usize;
        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;
            match wire_type {
                0 => {
                    let _ = decode_varint(data, &mut pos)?;
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    if field_num == 2 {
                        let s = core::str::from_utf8(&data[pos..pos + len])
                            .map_err(|_| SpircError::InvalidUtf8)?;
                        *artist_out = s.to_string();
                    }
                    pos += len;
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }
        Ok(())
    }

    fn parse_album_submessage(
        data: &[u8],
        album_out: &mut String,
        artwork_out: &mut Option<String>,
    ) -> Result<(), SpircError> {
        let mut pos = 0usize;
        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;
            match wire_type {
                0 => {
                    let _ = decode_varint(data, &mut pos)?;
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    let sub = &data[pos..pos + len];
                    pos += len;

                    if field_num == 2 {
                        let s = core::str::from_utf8(sub).map_err(|_| SpircError::InvalidUtf8)?;
                        *album_out = s.to_string();
                    } else if field_num == 0x11 {
                        Self::parse_cover_group(sub, artwork_out)?;
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }
        Ok(())
    }

    fn parse_cover_group(data: &[u8], artwork_out: &mut Option<String>) -> Result<(), SpircError> {
        let mut pos = 0usize;
        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;
            match wire_type {
                0 => {
                    let _ = decode_varint(data, &mut pos)?;
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    let img_sub = &data[pos..pos + len];
                    pos += len;
                    if field_num == 1 && artwork_out.is_none() {
                        Self::parse_image_submessage(img_sub, artwork_out)?;
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }
        Ok(())
    }

    fn parse_image_submessage(
        data: &[u8],
        artwork_out: &mut Option<String>,
    ) -> Result<(), SpircError> {
        let mut pos = 0usize;
        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;
            match wire_type {
                0 => {
                    let _ = decode_varint(data, &mut pos)?;
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    let raw = &data[pos..pos + len];
                    pos += len;
                    if field_num == 1 && !raw.is_empty() {
                        let is_ascii_hex =
                            raw.len() >= 32 && raw.iter().all(|b| b.is_ascii_hexdigit());
                        if is_ascii_hex {
                            if let Ok(s) = core::str::from_utf8(raw) {
                                *artwork_out = Some(s.to_ascii_lowercase());
                            }
                        } else {
                            *artwork_out = Some(Self::bytes_to_hex(raw));
                        }
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }
        Ok(())
    }
}

/// Spirc message types matching `spirc.proto`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SpircMessageType {
    /// Initial client handshake hello.
    Hello = 0x01,
    /// Client disconnect goodbye.
    Goodbye = 0x02,
    /// Heartbeat probe.
    Probe = 0x03,
    /// Player status notification broadcast.
    Notify = 0x0A,
    /// Load track context.
    Load = 0x14,
    /// Resume audio playback.
    Play = 0x15,
    /// Pause audio playback.
    Pause = 0x16,
    /// Toggle play / pause state.
    PlayPause = 0x17,
    /// Seek to millisecond position.
    Seek = 0x18,
    /// Skip to previous track in queue.
    Prev = 0x19,
    /// Skip to next track in queue.
    Next = 0x1A,
    /// Set absolute volume level (0..65536).
    Volume = 0x1B,
    /// Toggle shuffle order.
    Shuffle = 0x1C,
    /// Toggle repeat mode.
    Repeat = 0x1D,
    /// Decrement volume step.
    VolumeDown = 0x1F,
    /// Increment volume step.
    VolumeUp = 0x20,
    /// Replace playing queue with new context.
    Replace = 0x21,
    /// Log out active user.
    Logout = 0x22,
    /// Custom action.
    Action = 0x23,
    /// Unknown message type opcode.
    Unknown = 0xFF,
}

impl From<u32> for SpircMessageType {
    fn from(val: u32) -> Self {
        match val {
            0x01 => Self::Hello,
            0x02 => Self::Goodbye,
            0x03 => Self::Probe,
            0x0A => Self::Notify,
            0x14 => Self::Load,
            0x15 => Self::Play,
            0x16 => Self::Pause,
            0x17 => Self::PlayPause,
            0x18 => Self::Seek,
            0x19 => Self::Prev,
            0x1A => Self::Next,
            0x1B => Self::Volume,
            0x1C => Self::Shuffle,
            0x1D => Self::Repeat,
            0x1F => Self::VolumeDown,
            0x20 => Self::VolumeUp,
            0x21 => Self::Replace,
            0x22 => Self::Logout,
            0x23 => Self::Action,
            _ => Self::Unknown,
        }
    }
}

/// Playback status matching `spirc.proto`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PlayStatus {
    /// Playback stopped.
    Stop = 0,
    /// Actively streaming and playing audio.
    Play = 1,
    /// Audio paused.
    Pause = 2,
    /// Buffering or resolving audio track.
    Loading = 3,
}

impl From<u32> for PlayStatus {
    fn from(val: u32) -> Self {
        match val {
            0 => Self::Stop,
            1 => Self::Play,
            2 => Self::Pause,
            _ => Self::Loading,
        }
    }
}

/// High-level event emitted by the Spirc state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpircEvent {
    /// Request playback start.
    Play,
    /// Request playback pause.
    Pause,
    /// Request skip to next song.
    Next,
    /// Request skip to previous song.
    Prev,
    /// Seek to millisecond position.
    Seek(u32),
    /// Set volume level (`0..=65535`).
    SetVolume(u32),
    /// Load track from URI.
    LoadTrack {
        /// Spotify track URI (e.g. `spotify:track:...`).
        uri: String,
        /// Initial playback offset in milliseconds.
        position_ms: u32,
    },
    /// Active track playback finished (`TrackEnded`).
    TrackEnded,
    /// Track metadata updated.
    MetadataUpdated(TrackMetadata),
    /// Status update notification sent to Spotify network.
    Notify,
}

/// Spirc protocol parsing or decoding error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpircError {
    /// Message truncated or incomplete.
    TruncatedMessage,
    /// Varint exceeds 64 bits.
    VarintOverflow,
    /// Unknown wire format.
    InvalidWireType,
    /// UTF-8 decode error.
    InvalidUtf8,
}

impl fmt::Display for SpircError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedMessage => write!(f, "spirc message is truncated"),
            Self::VarintOverflow => write!(f, "protobuf varint overflow"),
            Self::InvalidWireType => write!(f, "invalid protobuf wire type"),
            Self::InvalidUtf8 => write!(f, "invalid utf-8 string in frame"),
        }
    }
}

/// Decodes a protobuf varint from slice.
pub fn decode_varint(data: &[u8], pos: &mut usize) -> Result<u64, SpircError> {
    let mut result = 0u64;
    let mut shift = 0;
    while *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        result |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift > 64 {
            return Err(SpircError::VarintOverflow);
        }
    }
    Err(SpircError::TruncatedMessage)
}

/// Encodes a protobuf varint to buffer.
pub fn encode_varint(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
            buf.push(byte);
        } else {
            buf.push(byte);
            break;
        }
    }
}

/// Skips an unknown protobuf field according to its wire type.
pub fn skip_unknown_field(wire_type: u8, data: &[u8], pos: &mut usize) -> Result<(), SpircError> {
    match wire_type {
        0 => {
            let _ = decode_varint(data, pos)?;
            Ok(())
        }
        1 => {
            if *pos + 8 > data.len() {
                return Err(SpircError::TruncatedMessage);
            }
            *pos += 8;
            Ok(())
        }
        2 => {
            let len = decode_varint(data, pos)? as usize;
            if *pos + len > data.len() {
                return Err(SpircError::TruncatedMessage);
            }
            *pos += len;
            Ok(())
        }
        5 => {
            if *pos + 4 > data.len() {
                return Err(SpircError::TruncatedMessage);
            }
            *pos += 4;
            Ok(())
        }
        _ => Err(SpircError::InvalidWireType),
    }
}

/// Parsed Spirc wire frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpircFrame {
    /// Frame sequence number.
    pub seq_nr: u32,
    /// Frame message type.
    pub message_type: SpircMessageType,
    /// Optional volume override (`0..=65535`).
    pub volume: Option<u32>,
    /// Optional seek / offset position in milliseconds.
    pub position_ms: Option<u32>,
    /// Optional track URI.
    pub track_uri: Option<String>,
}

impl SpircFrame {
    /// Creates a new `SpircFrame` with the given sequence number and message type.
    #[must_use]
    pub const fn new(seq_nr: u32, message_type: SpircMessageType) -> Self {
        Self {
            seq_nr,
            message_type,
            volume: None,
            position_ms: None,
            track_uri: None,
        }
    }

    /// Creates a `Seek` (`0x18`) frame with `position_ms`.
    #[must_use]
    pub const fn seek(seq_nr: u32, position_ms: u32) -> Self {
        Self {
            seq_nr,
            message_type: SpircMessageType::Seek,
            volume: None,
            position_ms: Some(position_ms),
            track_uri: None,
        }
    }

    /// Creates a `Volume` (`0x1B`) frame with `volume` (`0..=65535`).
    #[must_use]
    pub const fn volume(seq_nr: u32, volume: u32) -> Self {
        Self {
            seq_nr,
            message_type: SpircMessageType::Volume,
            volume: Some(volume),
            position_ms: None,
            track_uri: None,
        }
    }

    /// Creates a `Next` (`0x1A`) frame.
    #[must_use]
    pub const fn next(seq_nr: u32) -> Self {
        Self::new(seq_nr, SpircMessageType::Next)
    }

    /// Creates a `Prev` (`0x19`) frame.
    #[must_use]
    pub const fn prev(seq_nr: u32) -> Self {
        Self::new(seq_nr, SpircMessageType::Prev)
    }

    /// Encodes the `SpircFrame` into protobuf bytes matching `spirc.proto`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);

        // Field 1: version = 1
        encode_varint(&mut buf, (1 << 3) | 0);
        encode_varint(&mut buf, 1);

        // Field 4: seq_nr
        encode_varint(&mut buf, (4 << 3) | 0);
        encode_varint(&mut buf, self.seq_nr as u64);

        // Field 5: typ
        encode_varint(&mut buf, (5 << 3) | 0);
        encode_varint(&mut buf, self.message_type as u64);

        // Field 13: position
        if let Some(pos) = self.position_ms {
            encode_varint(&mut buf, (13 << 3) | 0);
            encode_varint(&mut buf, pos as u64);
        }

        // Field 14: volume
        if let Some(vol) = self.volume {
            encode_varint(&mut buf, (14 << 3) | 0);
            encode_varint(&mut buf, vol as u64);
        }

        // Field 12: state with TrackRef (field 27 -> field 2)
        if let Some(uri) = &self.track_uri {
            let mut track_ref = Vec::new();
            encode_varint(&mut track_ref, (2 << 3) | 2);
            encode_varint(&mut track_ref, uri.len() as u64);
            track_ref.extend_from_slice(uri.as_bytes());

            let mut state = Vec::new();
            if let Some(pos) = self.position_ms {
                encode_varint(&mut state, (4 << 3) | 0);
                encode_varint(&mut state, pos as u64);
            }
            encode_varint(&mut state, (27 << 3) | 2);
            encode_varint(&mut state, track_ref.len() as u64);
            state.extend_from_slice(&track_ref);

            encode_varint(&mut buf, (12 << 3) | 2);
            encode_varint(&mut buf, state.len() as u64);
            buf.extend_from_slice(&state);
        }

        buf
    }

    /// Parses a Spirc wire frame from raw bytes.
    pub fn parse(data: &[u8]) -> Result<Self, SpircError> {
        let mut pos = 0;
        let mut seq_nr = 0u32;
        let mut message_type = SpircMessageType::Unknown;
        let mut volume = None;
        let mut position_ms = None;
        let mut track_uri = None;

        while pos < data.len() {
            let key = decode_varint(data, &mut pos)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;

            match wire_type {
                0 => {
                    let val = decode_varint(data, &mut pos)?;
                    match field_num {
                        4 => seq_nr = val as u32,
                        5 => message_type = SpircMessageType::from(val as u32),
                        13 => position_ms = Some(val as u32),
                        14 => volume = Some(val as u32),
                        _ => {}
                    }
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)? as usize;
                    if pos + len > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    let sub_data = &data[pos..pos + len];
                    pos += len;

                    // Inspect submessage device_state (field 7) or state (field 12)
                    if field_num == 7 {
                        let mut sub_pos = 0;
                        while sub_pos < sub_data.len() {
                            let k = decode_varint(sub_data, &mut sub_pos)?;
                            let fn_sub = (k >> 3) as u32;
                            let wt_sub = (k & 0x07) as u8;
                            if wt_sub == 0 {
                                let v = decode_varint(sub_data, &mut sub_pos)?;
                                if fn_sub == 12 {
                                    volume = Some(v as u32);
                                }
                            } else if wt_sub == 1 {
                                if sub_pos + 8 > sub_data.len() {
                                    break;
                                }
                                sub_pos += 8;
                            } else if wt_sub == 2 {
                                let l = decode_varint(sub_data, &mut sub_pos)? as usize;
                                if sub_pos + l > sub_data.len() {
                                    break;
                                }
                                sub_pos += l;
                            } else if wt_sub == 5 {
                                if sub_pos + 4 > sub_data.len() {
                                    break;
                                }
                                sub_pos += 4;
                            }
                        }
                    } else if field_num == 12 {
                        let mut sub_pos = 0;
                        while sub_pos < sub_data.len() {
                            let k = decode_varint(sub_data, &mut sub_pos)?;
                            let fn_sub = (k >> 3) as u32;
                            let wt_sub = (k & 0x07) as u8;
                            if wt_sub == 0 {
                                let v = decode_varint(sub_data, &mut sub_pos)?;
                                if fn_sub == 4 {
                                    position_ms = Some(v as u32);
                                }
                            } else if wt_sub == 1 {
                                if sub_pos + 8 > sub_data.len() {
                                    break;
                                }
                                sub_pos += 8;
                            } else if wt_sub == 2 {
                                let l = decode_varint(sub_data, &mut sub_pos)? as usize;
                                if sub_pos + l > sub_data.len() {
                                    break;
                                }
                                if fn_sub == 27 {
                                    // TrackRef submessage
                                    let track_slice = &sub_data[sub_pos..sub_pos + l];
                                    let mut t_pos = 0;
                                    while t_pos < track_slice.len() {
                                        let tk = decode_varint(track_slice, &mut t_pos)?;
                                        let t_fn = (tk >> 3) as u32;
                                        let t_wt = (tk & 0x07) as u8;
                                        if t_wt == 2 {
                                            let tl =
                                                decode_varint(track_slice, &mut t_pos)? as usize;
                                            if t_pos + tl > track_slice.len() {
                                                break;
                                            }
                                            if t_fn == 2 {
                                                let str_bytes = &track_slice[t_pos..t_pos + tl];
                                                if let Ok(uri) = core::str::from_utf8(str_bytes) {
                                                    track_uri = Some(uri.to_string());
                                                }
                                            }
                                            t_pos += tl;
                                        } else if t_wt == 0 {
                                            let _ = decode_varint(track_slice, &mut t_pos)?;
                                        } else if t_wt == 1 {
                                            if t_pos + 8 > track_slice.len() {
                                                break;
                                            }
                                            t_pos += 8;
                                        } else if t_wt == 5 {
                                            if t_pos + 4 > track_slice.len() {
                                                break;
                                            }
                                            t_pos += 4;
                                        }
                                    }
                                }
                                sub_pos += l;
                            } else if wt_sub == 5 {
                                if sub_pos + 4 > sub_data.len() {
                                    break;
                                }
                                sub_pos += 4;
                            }
                        }
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(SpircError::TruncatedMessage);
                    }
                    pos += 4;
                }
                _ => return Err(SpircError::InvalidWireType),
            }
        }

        Ok(Self {
            seq_nr,
            message_type,
            volume,
            position_ms,
            track_uri,
        })
    }
}

/// Spirc state machine tracking playback status, position, volume, queue,
/// and metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpircStateMachine {
    status: PlayStatus,
    position_ms: u32,
    volume: u32,
    current_track: Option<String>,
    track_queue: Vec<String>,
    playing_track_index: usize,
    metadata: Option<TrackMetadata>,
    is_active: bool,
}

impl Default for SpircStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl SpircStateMachine {
    /// Creates a new Spirc state machine.
    pub fn new() -> Self {
        Self {
            status: PlayStatus::Stop,
            position_ms: 0,
            volume: 32768, // 50% volume default
            current_track: None,
            track_queue: Vec::new(),
            playing_track_index: 0,
            metadata: None,
            is_active: false,
        }
    }

    /// Current playback status.
    pub fn status(&self) -> PlayStatus {
        self.status
    }

    /// Current playback position in milliseconds.
    pub fn position_ms(&self) -> u32 {
        self.position_ms
    }

    /// Current volume level (`0..=65535`).
    pub fn volume(&self) -> u32 {
        self.volume
    }

    /// Returns active track URI, if any.
    pub fn current_track(&self) -> Option<&str> {
        self.current_track.as_deref()
    }

    /// Returns the queued track URIs.
    #[must_use]
    pub fn track_queue(&self) -> &[String] {
        &self.track_queue
    }

    /// Returns the index of the currently playing track within `track_queue`.
    #[must_use]
    pub const fn playing_track_index(&self) -> usize {
        self.playing_track_index
    }

    /// Returns current track metadata, if available.
    #[must_use]
    pub fn metadata(&self) -> Option<&TrackMetadata> {
        self.metadata.as_ref()
    }

    /// Returns the active track's Spotify CDN artwork URL (`https://i.scdn.co/image/<hex_id>`).
    #[must_use]
    pub fn artwork_url(&self) -> Option<String> {
        self.metadata.as_ref().and_then(TrackMetadata::artwork_url)
    }

    /// Replaces the track queue and sets the active track index.
    pub fn set_track_queue(&mut self, queue: Vec<String>, start_index: usize) {
        self.track_queue = queue;
        if self.track_queue.is_empty() {
            self.playing_track_index = 0;
            self.current_track = None;
        } else {
            self.playing_track_index = core::cmp::min(start_index, self.track_queue.len() - 1);
            self.current_track = Some(self.track_queue[self.playing_track_index].clone());
        }
    }

    /// Sets current track metadata and returns a `MetadataUpdated` event.
    pub fn set_metadata(&mut self, metadata: TrackMetadata) -> SpircEvent {
        self.metadata = Some(metadata.clone());
        SpircEvent::MetadataUpdated(metadata)
    }

    /// Sets playback status.
    pub fn set_status(&mut self, status: PlayStatus) {
        self.status = status;
    }

    /// Updates playback position in milliseconds.
    pub fn update_position_ms(&mut self, position_ms: u32) {
        self.position_ms = position_ms;
    }

    /// Seeks to `position_ms` (clamped to `metadata.duration_ms` if known)
    /// and emits `Seek`.
    pub fn seek(&mut self, position_ms: u32) -> SpircEvent {
        let clamped = match &self.metadata {
            Some(meta) if meta.duration_ms > 0 => core::cmp::min(position_ms, meta.duration_ms),
            _ => position_ms,
        };
        self.position_ms = clamped;
        SpircEvent::Seek(clamped)
    }

    /// Sets volume (clamped to `0..=65535`).
    pub fn set_volume(&mut self, volume: u32) {
        self.volume = core::cmp::min(volume, SPOTIFY_MAX_VOLUME);
    }

    /// Advances playback position by `delta_ms` and emits `TrackEnded` if
    /// `duration_ms` is reached.
    pub fn advance_position_ms(&mut self, delta_ms: u32) -> Option<SpircEvent> {
        if self.status != PlayStatus::Play {
            return None;
        }
        self.position_ms = self.position_ms.saturating_add(delta_ms);
        if let Some(meta) = &self.metadata {
            if meta.duration_ms > 0 && self.position_ms >= meta.duration_ms {
                return Some(self.notify_track_ended());
            }
        }
        None
    }

    /// Marks the active track as finished, advancing queue or stopping, and
    /// returns `TrackEnded`.
    pub fn notify_track_ended(&mut self) -> SpircEvent {
        self.position_ms = 0;
        if !self.track_queue.is_empty() && self.playing_track_index + 1 < self.track_queue.len() {
            self.playing_track_index += 1;
            self.current_track = Some(self.track_queue[self.playing_track_index].clone());
            self.status = PlayStatus::Play;
        } else {
            self.status = PlayStatus::Stop;
        }
        SpircEvent::TrackEnded
    }

    /// Handles an incoming SpircFrame and triggers state transitions.
    pub fn handle_frame(&mut self, frame: &SpircFrame) -> Option<SpircEvent> {
        match frame.message_type {
            SpircMessageType::Play => {
                self.status = PlayStatus::Play;
                self.is_active = true;
                Some(SpircEvent::Play)
            }
            SpircMessageType::Pause => {
                self.status = PlayStatus::Pause;
                Some(SpircEvent::Pause)
            }
            SpircMessageType::PlayPause => {
                if self.status == PlayStatus::Play {
                    self.status = PlayStatus::Pause;
                    Some(SpircEvent::Pause)
                } else {
                    self.status = PlayStatus::Play;
                    self.is_active = true;
                    Some(SpircEvent::Play)
                }
            }
            SpircMessageType::Next => {
                self.position_ms = 0;
                self.metadata = None;
                if !self.track_queue.is_empty()
                    && self.playing_track_index + 1 < self.track_queue.len()
                {
                    self.playing_track_index += 1;
                    self.current_track = Some(self.track_queue[self.playing_track_index].clone());
                } else {
                    self.status = PlayStatus::Stop;
                    self.position_ms = 0;
                    self.metadata = None;
                }
                Some(SpircEvent::Next)
            }
            SpircMessageType::Prev => {
                self.position_ms = 0;
                self.metadata = None;
                if !self.track_queue.is_empty() && self.playing_track_index > 0 {
                    self.playing_track_index -= 1;
                    self.current_track = Some(self.track_queue[self.playing_track_index].clone());
                }
                Some(SpircEvent::Prev)
            }
            SpircMessageType::Seek => frame.position_ms.map(|pos| self.seek(pos)),
            SpircMessageType::Volume => {
                if let Some(vol) = frame.volume {
                    self.set_volume(vol);
                    Some(SpircEvent::SetVolume(self.volume))
                } else {
                    None
                }
            }
            SpircMessageType::VolumeUp => {
                self.set_volume(self.volume.saturating_add(SPOTIFY_VOLUME_STEP));
                Some(SpircEvent::SetVolume(self.volume))
            }
            SpircMessageType::VolumeDown => {
                self.set_volume(self.volume.saturating_sub(SPOTIFY_VOLUME_STEP));
                Some(SpircEvent::SetVolume(self.volume))
            }
            SpircMessageType::Load | SpircMessageType::Replace => {
                self.metadata = None;
                if let Some(uri) = &frame.track_uri {
                    self.current_track = Some(uri.clone());
                    let pos = frame.position_ms.unwrap_or(0);
                    self.position_ms = pos;
                    self.status = PlayStatus::Play;
                    self.is_active = true;
                    Some(SpircEvent::LoadTrack {
                        uri: uri.clone(),
                        position_ms: pos,
                    })
                } else {
                    None
                }
            }
            SpircMessageType::Logout | SpircMessageType::Goodbye => {
                self.status = PlayStatus::Stop;
                self.is_active = false;
                None
            }
            SpircMessageType::Shuffle | SpircMessageType::Repeat => Some(SpircEvent::Notify),
            SpircMessageType::Notify => Some(SpircEvent::Notify),
            _ => None,
        }
    }

    /// Builds a binary Spirc `Notify` response frame.
    pub fn build_notify_frame(&self, seq_nr: u32, ident: &str) -> Vec<u8> {
        let mut frame = Vec::with_capacity(128);

        // Field 1: version = 1
        encode_varint(&mut frame, (1 << 3) | 0);
        encode_varint(&mut frame, 1);

        // Field 2: ident
        encode_varint(&mut frame, (2 << 3) | 2);
        encode_varint(&mut frame, ident.len() as u64);
        frame.extend_from_slice(ident.as_bytes());

        // Field 4: seq_nr
        encode_varint(&mut frame, (4 << 3) | 0);
        encode_varint(&mut frame, seq_nr as u64);

        // Field 5: typ = Notify (0x0A)
        encode_varint(&mut frame, (5 << 3) | 0);
        encode_varint(&mut frame, SpircMessageType::Notify as u64);

        // Field 7: device_state
        let mut dev_state = Vec::new();
        // Field 12: volume
        encode_varint(&mut dev_state, (12 << 3) | 0);
        encode_varint(&mut dev_state, self.volume as u64);
        // Field 10: is_active
        encode_varint(&mut dev_state, (10 << 3) | 0);
        encode_varint(&mut dev_state, if self.is_active { 1 } else { 0 });

        encode_varint(&mut frame, (7 << 3) | 2);
        encode_varint(&mut frame, dev_state.len() as u64);
        frame.extend_from_slice(&dev_state);

        // Field 12: state
        let mut state = Vec::new();
        // Field 4: position_ms
        encode_varint(&mut state, (4 << 3) | 0);
        encode_varint(&mut state, self.position_ms as u64);
        // Field 5: status
        encode_varint(&mut state, (5 << 3) | 0);
        encode_varint(&mut state, self.status as u64);

        encode_varint(&mut frame, (12 << 3) | 2);
        encode_varint(&mut frame, state.len() as u64);
        frame.extend_from_slice(&state);

        frame
    }
}
