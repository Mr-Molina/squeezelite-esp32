//! Spirc command parsing and state machine for Spotify Connect.
//!
//! Decodes and encodes Spirc frames (matching `spirc.proto`) and manages
//! playback state transitions (kPlay, kPause, kNext, kPrev, kSetVolume).

#![allow(clippy::identity_op)]

extern crate alloc;

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

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
    /// Set volume level (0..65536).
    SetVolume(u32),
    /// Load track from URI.
    LoadTrack {
        /// Spotify track URI (e.g. `spotify:track:...`).
        uri: String,
        /// Initial playback offset in milliseconds.
        position_ms: u32,
    },
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

/// Parsed Spirc wire frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpircFrame {
    /// Frame sequence number.
    pub seq_nr: u32,
    /// Frame message type.
    pub message_type: SpircMessageType,
    /// Optional volume override.
    pub volume: Option<u32>,
    /// Optional seek / offset position.
    pub position_ms: Option<u32>,
    /// Optional track URI.
    pub track_uri: Option<String>,
}

impl SpircFrame {
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
                            } else if wt_sub == 2 {
                                let l = decode_varint(sub_data, &mut sub_pos)? as usize;
                                sub_pos += l;
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
                            } else if wt_sub == 2 {
                                let l = decode_varint(sub_data, &mut sub_pos)? as usize;
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
                                            if t_fn == 2 {
                                                let str_bytes =
                                                    &track_slice[t_pos..t_pos + tl];
                                                if let Ok(uri) =
                                                    core::str::from_utf8(str_bytes)
                                                {
                                                    track_uri = Some(uri.to_string());
                                                }
                                            }
                                            t_pos += tl;
                                        } else if t_wt == 0 {
                                            let _ = decode_varint(track_slice, &mut t_pos)?;
                                        }
                                    }
                                }
                                sub_pos += l;
                            }
                        }
                    }
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

/// Spirc state machine tracking playback status, position, and volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpircStateMachine {
    status: PlayStatus,
    position_ms: u32,
    volume: u32,
    current_track: Option<String>,
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

    /// Current volume level (0..65536).
    pub fn volume(&self) -> u32 {
        self.volume
    }

    /// Returns active track URI, if any.
    pub fn current_track(&self) -> Option<&str> {
        self.current_track.as_deref()
    }

    /// Sets playback status.
    pub fn set_status(&mut self, status: PlayStatus) {
        self.status = status;
    }

    /// Updates playback position in milliseconds.
    pub fn update_position_ms(&mut self, position_ms: u32) {
        self.position_ms = position_ms;
    }

    /// Sets volume (clamped to 65536).
    pub fn set_volume(&mut self, volume: u32) {
        self.volume = core::cmp::min(volume, 65536);
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
            SpircMessageType::Next => Some(SpircEvent::Next),
            SpircMessageType::Prev => Some(SpircEvent::Prev),
            SpircMessageType::Seek => {
                if let Some(pos) = frame.position_ms {
                    self.position_ms = pos;
                    Some(SpircEvent::Seek(pos))
                } else {
                    None
                }
            }
            SpircMessageType::Volume => {
                if let Some(vol) = frame.volume {
                    self.set_volume(vol);
                    Some(SpircEvent::SetVolume(self.volume))
                } else {
                    None
                }
            }
            SpircMessageType::Load => {
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
