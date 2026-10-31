//! `squeezelite-proto`
//!
//! A zero-allocation, `no_std` SlimProto network protocol engine for connecting
//! to Logitech Media Server (LMS) / Lyrion Music Server.
//!
//! Handles decoding of server packets (`strm`, `audg`, `setd`, `flsh`, `body`)
//! and encoding of client telemetry packets (`HELO`, `STAT`, `RESP`).

#![no_std]
#![forbid(unsafe_code)]

use core::fmt;

/// Errors encountered during SlimProto packet parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Buffer does not contain enough bytes to read the frame header or payload.
    BufferTooShort,
    /// Packet length exceeds allowable bounded buffer size.
    PayloadTooLarge,
    /// Packet format does not match the SlimProto specification.
    InvalidFormat,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer contains insufficient bytes"),
            Self::PayloadTooLarge => write!(f, "Payload exceeds bounded buffer limit"),
            Self::InvalidFormat => write!(f, "Malformed SlimProto frame format"),
        }
    }
}

/// Type alias for packet serialization errors.
pub type BuildError = ParseError;

/// Supported audio streaming formats transmitted by LMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Pcm,
    Flac,
    Mp3,
    Aac,
    Ogg,
    Opus,
    Alac,
    Unknown(u8),
}

impl AudioFormat {
    pub const fn from_byte(b: u8) -> Self {
        match b {
            b'p' => Self::Pcm,
            b'f' => Self::Flac,
            b'm' => Self::Mp3,
            b'a' => Self::Aac,
            b'o' => Self::Ogg,
            b'u' => Self::Opus,
            b'l' => Self::Alac,
            other => Self::Unknown(other),
        }
    }

    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Pcm => b'p',
            Self::Flac => b'f',
            Self::Mp3 => b'm',
            Self::Aac => b'a',
            Self::Ogg => b'o',
            Self::Opus => b'u',
            Self::Alac => b'l',
            Self::Unknown(b) => b,
        }
    }
}

/// Converts LMS SlimProto sample rate index into Hz.
pub const fn sample_rate_from_index(index: u8) -> u32 {
    match index {
        0 | b'0' => 11025,
        1 | b'1' => 22050,
        2 | b'2' => 32000,
        3 | b'3' => 44100,
        4 | b'4' => 48000,
        5 | b'5' => 8000,
        6 | b'6' => 12000,
        7 | b'7' => 16000,
        8 | b'8' => 24000,
        9 | b'9' => 96000,
        10 | b':' => 88200,
        11 | b';' => 176400,
        12 | b'<' => 192000,
        _ => 44100,
    }
}

/// Autostart mode for audio streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartMode {
    Off,
    Auto,
    DirectOff,
    DirectAuto,
}

pub const STRM_FLAG_CONT_WAIT: u8 = 0x20;
pub const STRM_FLAG_SERVER_REBOOT: u8 = 0x40;

impl AutostartMode {
    pub const fn from_byte(b: u8) -> Self {
        match b {
            b'1' | 1 => Self::Auto,
            b'2' | 2 => Self::DirectOff,
            b'3' | 3 => Self::DirectAuto,
            _ => Self::Off,
        }
    }

    pub const fn is_auto(self) -> bool {
        matches!(self, Self::Auto | Self::DirectAuto)
    }

    pub const fn requires_cont(self) -> bool {
        matches!(self, Self::DirectOff | Self::DirectAuto)
    }

    pub const fn from_strm(autostart: bool, flags: u8) -> Self {
        if flags & STRM_FLAG_CONT_WAIT != 0 {
            if autostart {
                Self::DirectAuto
            } else {
                Self::DirectOff
            }
        } else if autostart {
            Self::Auto
        } else {
            Self::Off
        }
    }
}

/// Disconnect reason codes for client-initiated disconnection (`DSCO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DisconnectReason {
    ConnectionClosed = 0,
    ResetByLocal = 1,
    ResetByRemote = 2,
    Unreachable = 3,
    TimedOut = 4,
}

impl DisconnectReason {
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    pub const fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::ConnectionClosed,
            1 => Self::ResetByLocal,
            2 => Self::ResetByRemote,
            3 => Self::Unreachable,
            4 => Self::TimedOut,
            _ => Self::ConnectionClosed,
        }
    }
}

/// PCM sample endianness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    BigEndian,
    LittleEndian,
}

impl Endianness {
    pub const fn from_byte(b: u8) -> Self {
        match b {
            b'0' | 0 => Self::BigEndian,
            _ => Self::LittleEndian,
        }
    }
}

/// Stream playback transition type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionType {
    None,
    Crossfade,
    FadeIn,
    FadeOut,
    FadeInOut,
    Unknown(u8),
}

impl TransitionType {
    pub const fn from_byte(b: u8) -> Self {
        match b {
            b'0' | 0 => Self::None,
            b'1' | 1 => Self::Crossfade,
            b'2' | 2 => Self::FadeIn,
            b'3' | 3 => Self::FadeOut,
            b'4' | 4 => Self::FadeInOut,
            other => Self::Unknown(other),
        }
    }
}

/// Stream command sub-types within a `strm` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCommandType {
    /// Start streaming audio immediately.
    Start,
    /// Pause audio playback.
    Pause,
    /// Resume paused playback.
    Resume,
    /// Stop streaming audio and close stream buffer.
    Stop,
    /// Flush audio buffer immediately.
    Flush,
    /// Undefined or extended stream command (including `'t'` timestamp and `'a'` skip-ahead).
    Unknown(u8),
}

impl StreamCommandType {
    /// Query current playback timestamp (`'t'`).
    #[allow(non_upper_case_globals)]
    pub const Timestamp: Self = Self::Unknown(b't');

    /// Skip ahead in output buffer (`'a'`).
    #[allow(non_upper_case_globals)]
    pub const SkipAhead: Self = Self::Unknown(b'a');

    pub const fn from_byte(b: u8) -> Self {
        match b {
            b's' => Self::Start,
            b'p' => Self::Pause,
            b'u' => Self::Resume,
            b'q' => Self::Stop,
            b'f' => Self::Flush,
            b't' => Self::Timestamp,
            b'a' => Self::SkipAhead,
            other => Self::Unknown(other),
        }
    }

    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Start => b's',
            Self::Pause => b'p',
            Self::Resume => b'u',
            Self::Stop => b'q',
            Self::Flush => b'f',
            Self::Unknown(b) => b,
        }
    }

    /// Returns `true` if this stream command is a timestamp query (`'t'`).
    pub const fn is_timestamp(self) -> bool {
        matches!(self, Self::Unknown(b't'))
    }

    /// Returns `true` if this stream command is a skip-ahead request (`'a'`).
    pub const fn is_skip_ahead(self) -> bool {
        matches!(self, Self::Unknown(b'a'))
    }
}

/// Parsed LMS `setd` device setting payload (`[id: u8, data...]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceSetting<'a> {
    pub id: u8,
    pub data: &'a [u8],
}

/// Parsed commands received from the Logitech Media Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCommand<'a> {
    /// Audio stream control command (`strm`).
    Stream {
        command: StreamCommandType,
        autostart: bool,
        format: AudioFormat,
        pcm_sample_size: u8,
        pcm_sample_rate: u8,
        pcm_channels: u8,
        pcm_endianness: Endianness,
        threshold_kb: u16,
        spdif_enable: bool,
        trans_duration: u8,
        trans_type: TransitionType,
        flags: u8,
        output_threshold: u8,
        replay_gain: u32,
        server_port: u16,
        server_ip: [u8; 4],
        http_headers: &'a [u8],
    },
    /// Audio gain / volume attenuation control (`audg`).
    AudioGain {
        gain_left: u32,
        gain_right: u32,
        digital_atten: u8,
        preamp: u8,
    },
    /// Set audio output device setting or sample rate (`setd`).
    SetDevice {
        sample_rate: u32,
    },
    /// Flush playback buffer (`flsh`).
    Flush,
    /// Metadata body packet (`body`).
    Body {
        data: &'a [u8],
    },
    /// Extended or unrecognized packet opcode (`setd` variable data, `aude`, `cont`, `serv`, `vers`, `dsco`, etc.).
    Unknown {
        opcode: [u8; 4],
        payload: &'a [u8],
    },
}

impl<'a> ServerCommand<'a> {
    /// Extracts the `setd` device setting `(id, data)` if this command is a `setd` frame.
    pub fn device_setting(&self) -> Option<DeviceSetting<'a>> {
        match *self {
            Self::Unknown {
                opcode: [b's', b'e', b't', b'd'],
                payload,
            } if !payload.is_empty() => Some(DeviceSetting {
                id: payload[0],
                data: &payload[1..],
            }),
            Self::SetDevice { sample_rate } if sample_rate <= u8::MAX as u32 => Some(DeviceSetting {
                id: sample_rate as u8,
                data: &[],
            }),
            _ => None,
        }
    }

    /// Extracts `(spdif_enable, dac_enable)` if this command is an `aude` frame.
    pub const fn audio_enable(&self) -> Option<(bool, bool)> {
        match *self {
            Self::Unknown {
                opcode: [b'a', b'u', b'd', b'e'],
                payload,
            } if payload.len() >= 2 => Some((payload[0] != 0, payload[1] != 0)),
            _ => None,
        }
    }

    /// Extracts the `metaint` metadata interval if this command is a `cont` frame.
    pub fn stream_continue(&self) -> Option<u32> {
        match *self {
            Self::Unknown {
                opcode: [b'c', b'o', b'n', b't'],
                payload,
            } => {
                if payload.len() >= 4 {
                    Some(u32::from_be_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                    ]))
                } else {
                    Some(0)
                }
            }
            _ => None,
        }
    }

    /// Extracts `(server_ip, syncgroup)` if this command is a `serv` frame.
    ///
    /// C parity (`slimproto.c:641-656`): the syncgroup is the tail after the IPv4 address only
    /// when it is exactly [`SYNCGROUP_ID_LEN`] bytes; any other tail means "no syncgroup" (C
    /// frees `new_server_cap`). `0.0.0.1` is C's SqueezeNetwork marker (`slimproto.c:633-634`); it
    /// is returned as is for the caller to decide.
    pub fn switch_server(&self) -> Option<([u8; 4], Option<&'a [u8]>)> {
        match *self {
            Self::Unknown {
                opcode: [b's', b'e', b'r', b'v'],
                payload,
            } if payload.len() >= 4 => {
                let ip = [payload[0], payload[1], payload[2], payload[3]];
                let tail = &payload[4..];
                let syncgroup = (tail.len() == SYNCGROUP_ID_LEN).then_some(tail);
                Some((ip, syncgroup))
            }
            _ => None,
        }
    }

    /// Extracts the server version string if this command is a `vers` frame.
    pub fn server_version(&self) -> Option<&'a str> {
        match *self {
            Self::Unknown {
                opcode: [b'v', b'e', b'r', b's'],
                payload,
            } if !payload.is_empty() => core::str::from_utf8(payload).ok(),
            _ => None,
        }
    }

    /// Extracts the disconnect reason code if this command is a `dsco` frame.
    pub const fn stream_disconnected(&self) -> Option<u8> {
        match *self {
            Self::Unknown {
                opcode: [b'd', b's', b'c', b'o'],
                payload,
            } if !payload.is_empty() => Some(payload[0]),
            _ => None,
        }
    }

    /// Extracts equalizer gains if this command is an `eqlz` frame.
    pub fn equalizer_gains(&self) -> Option<&'a [u8]> {
        match *self {
            Self::Unknown {
                opcode: [b'e', b'q', b'l', b'z'],
                payload,
            } => Some(payload),
            _ => None,
        }
    }

    /// Extracts loudness gain if this command is a `loud` frame.
    pub const fn loudness_gain(&self) -> Option<u8> {
        match *self {
            Self::Unknown {
                opcode: [b'l', b'o', b'u', b'd'],
                payload,
            } if !payload.is_empty() => Some(payload[0]),
            _ => None,
        }
    }
}

/// Zero-allocation parser for SlimProto frame sequences.
pub struct SlimprotoParser;

impl SlimprotoParser {
    pub const MAX_FRAME_LENGTH: usize = 4096;

    /// Parses a single SlimProto frame from a byte slice.
    ///
    /// Supports the official SlimProto server-to-client 6-byte frame header
    /// (`[2-byte BE length][4-byte ASCII opcode][payload]`) and the 8-byte frame header
    /// (`[4-byte ASCII opcode][4-byte BE length][payload]`).
    ///
    /// Returns `Ok((ServerCommand, bytes_consumed))` on success.
    /// Returns `Err(ParseError::BufferTooShort)` if more data is needed.
    pub fn parse_frame(buf: &[u8]) -> Result<(ServerCommand<'_>, usize), ParseError> {
        if buf.len() < 6 {
            return Err(ParseError::BufferTooShort);
        }

        let (opcode, payload, total_frame_len) = if !buf[0].is_ascii_alphabetic()
            && buf[2..6].iter().all(|b| b.is_ascii_alphabetic())
        {
            let length = u16::from_be_bytes([buf[0], buf[1]]) as usize;
            if length < 4 {
                return Err(ParseError::InvalidFormat);
            }
            let payload_len = length - 4;
            if payload_len > Self::MAX_FRAME_LENGTH {
                return Err(ParseError::PayloadTooLarge);
            }
            let total_frame_len = match 2usize.checked_add(length) {
                Some(l) => l,
                None => return Err(ParseError::PayloadTooLarge),
            };
            if buf.len() < total_frame_len {
                return Err(ParseError::BufferTooShort);
            }
            let opcode = [buf[2], buf[3], buf[4], buf[5]];
            let payload = &buf[6..total_frame_len];
            (opcode, payload, total_frame_len)
        } else {
            if buf.len() < 8 {
                return Err(ParseError::BufferTooShort);
            }
            let opcode = [buf[0], buf[1], buf[2], buf[3]];
            let length = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
            if length > Self::MAX_FRAME_LENGTH {
                return Err(ParseError::PayloadTooLarge);
            }
            let total_frame_len = match 8usize.checked_add(length) {
                Some(l) => l,
                None => return Err(ParseError::PayloadTooLarge),
            };
            if buf.len() < total_frame_len {
                return Err(ParseError::BufferTooShort);
            }
            let payload = &buf[8..total_frame_len];
            (opcode, payload, total_frame_len)
        };

        let cmd = match &opcode {
            b"strm" => Self::parse_strm(payload)?,
            b"audg" => Self::parse_audg(payload)?,
            b"setd" => Self::parse_setd(payload)?,
            b"flsh" => ServerCommand::Flush,
            b"body" => ServerCommand::Body { data: payload },
            b"aude" => Self::parse_aude(payload)?,
            b"cont" => Self::parse_cont(payload)?,
            b"serv" => Self::parse_serv(payload)?,
            b"vers" => Self::parse_vers(payload)?,
            b"dsco" => Self::parse_dsco(payload)?,
            _ => ServerCommand::Unknown { opcode, payload },
        };

        Ok((cmd, total_frame_len))
    }

    fn parse_strm(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 24 {
            return Err(ParseError::InvalidFormat);
        }

        let command = StreamCommandType::from_byte(payload[0]);
        let autostart = payload[1] == b'1' || payload[1] == b'3';
        let format = AudioFormat::from_byte(payload[2]);
        let pcm_sample_size = payload[3];
        let pcm_sample_rate = payload[4];
        let pcm_channels = payload[5];
        let pcm_endianness = Endianness::from_byte(payload[6]);
        let threshold_kb = payload[7] as u16;
        let spdif_enable = payload[8] != 0;
        let trans_duration = payload[9];
        let trans_type = TransitionType::from_byte(payload[10]);
        let flags = if payload[1] == b'2' || payload[1] == b'3' || payload[1] == 2 || payload[1] == 3 {
            payload[11] | STRM_FLAG_CONT_WAIT
        } else {
            payload[11]
        };
        let output_threshold = payload[12];
        let _slaves = payload[13];
        let replay_gain = u32::from_be_bytes([payload[14], payload[15], payload[16], payload[17]]);
        let server_port = u16::from_be_bytes([payload[18], payload[19]]);
        let server_ip = [payload[20], payload[21], payload[22], payload[23]];
        let http_headers = if payload.len() > 24 { &payload[24..] } else { &[] };

        Ok(ServerCommand::Stream {
            command,
            autostart,
            format,
            pcm_sample_size,
            pcm_sample_rate,
            pcm_channels,
            pcm_endianness,
            threshold_kb,
            spdif_enable,
            trans_duration,
            trans_type,
            flags,
            output_threshold,
            replay_gain,
            server_port,
            server_ip,
            http_headers,
        })
    }

    fn parse_audg(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 18 {
            return Err(ParseError::InvalidFormat);
        }
        let old_left = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let old_right = i32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let digital_atten = payload[8];
        let preamp = payload[9];
        let raw_left = u32::from_be_bytes([payload[10], payload[11], payload[12], payload[13]]);
        let raw_right = u32::from_be_bytes([payload[14], payload[15], payload[16], payload[17]]);

        // Legacy negative dB value in old_gain_left/right indicates mute when 16.16 fixed-point gain is zero (CODE-PROTO-001)
        let _ = (old_left, old_right);
        let gain_left = raw_left;
        let gain_right = raw_right;

        Ok(ServerCommand::AudioGain {
            gain_left,
            gain_right,
            digital_atten,
            preamp,
        })
    }

    /// Parses a raw `setd` payload into `(setting_id, data)` according to the LMS `setd` specification.
    pub fn parse_setd_setting(payload: &[u8]) -> Result<DeviceSetting<'_>, ParseError> {
        if payload.is_empty() {
            return Err(ParseError::InvalidFormat);
        }
        Ok(DeviceSetting {
            id: payload[0],
            data: &payload[1..],
        })
    }

    fn parse_setd(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.is_empty() {
            return Err(ParseError::InvalidFormat);
        }
        // Only interpret 4-byte payloads (or 1-byte setting ID queries) via SetDevice;
        // variable-length device setting frames (e.g. id=0 player name strings) must not
        // corrupt sample_rate with ASCII name bytes.
        match payload.len() {
            4 => {
                let sample_rate =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                Ok(ServerCommand::SetDevice { sample_rate })
            }
            1 => Ok(ServerCommand::SetDevice {
                sample_rate: payload[0] as u32,
            }),
            _ => Ok(ServerCommand::Unknown {
                opcode: *b"setd",
                payload,
            }),
        }
    }

    fn parse_aude(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 2 {
            return Err(ParseError::InvalidFormat);
        }
        Ok(ServerCommand::Unknown {
            opcode: *b"aude",
            payload,
        })
    }

    fn parse_cont(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        Ok(ServerCommand::Unknown {
            opcode: *b"cont",
            payload,
        })
    }

    fn parse_serv(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 4 {
            return Err(ParseError::InvalidFormat);
        }
        Ok(ServerCommand::Unknown {
            opcode: *b"serv",
            payload,
        })
    }

    fn parse_vers(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.is_empty() {
            return Err(ParseError::InvalidFormat);
        }
        Ok(ServerCommand::Unknown {
            opcode: *b"vers",
            payload,
        })
    }

    fn parse_dsco(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.is_empty() {
            return Err(ParseError::InvalidFormat);
        }
        Ok(ServerCommand::Unknown {
            opcode: *b"dsco",
            payload,
        })
    }
}

/// Telemetry status events sent from the player to LMS (`STAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatEvent {
    /// Heartbeat status report (`STMt`).
    Heartbeat,
    /// Track playback started (`STMs`).
    TrackStarted,
    /// Track playback paused (`STMp`).
    TrackPaused,
    /// Track playback resumed (`STMr`).
    TrackResumed,
    /// Playback stopped (`STMq`).
    TrackStopped,
    /// Stream buffer underrun / threshold reached (`STMl`).
    StreamUnderrun,
    /// Track underrun (`STMu`).
    TrackUnderrun,
    /// Output buffer underrun (`STMo`).
    OutputUnderrun,
    /// Stream flushed (`STMf`).
    StreamFlushed,
    /// Track connection established (`STMc`).
    Connected,
    /// Track connection established alias (`STMc`).
    ConnectEstablished,
    /// Decoder ready for next track (`STMd`).
    DecodeReady,
    /// Format or stream not supported (`STMn`).
    NotSupported,
}

impl StatEvent {
    pub const BUFFER_THRESHOLD: Self = Self::StreamUnderrun;

    pub const fn opcode_bytes(self) -> [u8; 4] {
        match self {
            Self::Heartbeat => *b"STMt",
            Self::TrackStarted => *b"STMs",
            Self::TrackPaused => *b"STMp",
            Self::TrackResumed => *b"STMr",
            Self::TrackStopped => *b"STMq",
            Self::StreamUnderrun => *b"STMl",
            Self::TrackUnderrun => *b"STMu",
            Self::OutputUnderrun => *b"STMo",
            Self::StreamFlushed => *b"STMf",
            Self::Connected | Self::ConnectEstablished => *b"STMc",
            Self::DecodeReady => *b"STMd",
            Self::NotSupported => *b"STMn",
        }
    }
}

/// Standard 53-byte payload size of a SlimProto `STAT` frame.
pub const STAT_PAYLOAD_SIZE: usize = 53;

/// Total wire size (8-byte header + 53-byte payload) of a SlimProto `STAT` frame.
pub const STAT_FRAME_SIZE: usize = 8 + STAT_PAYLOAD_SIZE;

/// Full structured telemetry report for SlimProto `STAT` packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatReport {
    pub event: StatEvent,
    pub stream_buffer_size: u32,
    pub stream_buffer_fullness: u32,
    pub bytes_received: u64,
    pub signal_strength: u16,
    pub jiffies: u32,
    pub output_buffer_size: u32,
    pub output_buffer_fullness: u32,
    pub elapsed_seconds: u32,
    pub voltage: u16,
    pub elapsed_ms: u32,
    pub server_timestamp: u32,
}

/// Dedicated `STAT` packet builder supporting both full `StatReport` structs and compact telemetry counters.
pub struct StatBuilder;

impl StatBuilder {
    /// Serializes a complete `StatReport` into a 61-byte `STAT` wire frame.
    pub fn build_stat_report(report: &StatReport, out: &mut [u8]) -> Result<usize, ParseError> {
        SlimprotoBuilder::build_stat_report(report, out)
    }

    /// Formats a `STAT` telemetry packet estimating `elapsed_seconds` and `elapsed_ms` from `frames_played` at 44.1 kHz.
    pub fn build_stat(
        event: StatEvent,
        stream_buffer_fullness: u32,
        output_buffer_fullness: u32,
        frames_played: u32,
        timestamp_ms: u32,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        let elapsed_ms = ((frames_played as u64) * 1000 / 44_100) as u32;
        let elapsed_seconds = frames_played / 44_100;
        let report = StatReport {
            event,
            stream_buffer_size: 65536,
            stream_buffer_fullness,
            bytes_received: frames_played as u64,
            signal_strength: 0xFFFF,
            jiffies: timestamp_ms,
            output_buffer_size: 65536,
            output_buffer_fullness,
            elapsed_seconds,
            voltage: 0,
            elapsed_ms,
            server_timestamp: 0,
        };
        Self::build_stat_report(&report, out)
    }

    /// Formats a `STAT` telemetry packet with explicit metrics including custom sample rate,
    /// 64-bit frames played, server timestamp, and 64-bit byte counter.
    #[allow(clippy::too_many_arguments)]
    pub fn build_stat_with_metrics(
        event: StatEvent,
        stream_buffer_fullness: u32,
        output_buffer_fullness: u32,
        frames_played: u64,
        sample_rate: u32,
        bytes_received: u64,
        jiffies_ms: u32,
        server_timestamp: u32,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        let rate = if sample_rate == 0 {
            44_100
        } else {
            sample_rate as u64
        };
        let elapsed_ms = (frames_played.saturating_mul(1000) / rate) as u32;
        let elapsed_seconds = (frames_played / rate) as u32;
        let report = StatReport {
            event,
            stream_buffer_size: 65536,
            stream_buffer_fullness,
            bytes_received,
            signal_strength: 0xFFFF,
            jiffies: jiffies_ms,
            output_buffer_size: 65536,
            output_buffer_fullness,
            elapsed_seconds,
            voltage: 0,
            elapsed_ms,
            server_timestamp,
        };
        Self::build_stat_report(&report, out)
    }
}

/// Packet serializer for outbound telemetry to LMS.
pub struct SlimprotoBuilder;

impl SlimprotoBuilder {
    /// Formats a client HELO packet into a provided byte buffer.
    /// Returns the total bytes written.
    pub fn build_helo(
        mac: &[u8; 6],
        device_id: u8,
        capabilities: &str,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        Self::build_helo_with_options(mac, device_id, capabilities, false, 0, out)
    }

    /// Formats a client HELO packet with extended options (reconnect flag and bytes received).
    pub fn build_helo_with_options(
        mac: &[u8; 6],
        device_id: u8,
        capabilities: &str,
        reconnect: bool,
        bytes_received: u64,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        let cap_bytes = capabilities.as_bytes();
        let payload_len = 36 + cap_bytes.len();
        let total_len = 8 + payload_len;

        if out.len() < total_len {
            return Err(ParseError::PayloadTooLarge);
        }

        // Header
        out[0..4].copy_from_slice(b"HELO");
        out[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());

        // Payload (Standard 36-byte prefix before capabilities)
        out[8] = device_id; // Device ID (e.g. 12 = Squeezelite)
        out[9] = 0;         // Firmware revision
        out[10..16].copy_from_slice(mac);
        // Fill UUID (16 bytes) with zero
        out[16..32].fill(0);
        // WLAN channels / reconnect flag (2 bytes at 32..34)
        let wlan_flags: u16 = if reconnect { 0x4000 } else { 0 };
        out[32..34].copy_from_slice(&wlan_flags.to_be_bytes());
        // Bytes received (8 bytes at 34..42)
        out[34..42].copy_from_slice(&bytes_received.to_be_bytes());
        // Lang padding (2 bytes at 42..44)
        out[42..44].fill(0);

        out[44..total_len].copy_from_slice(cap_bytes);

        Ok(total_len)
    }

    /// Serializes a complete `StatReport` into a 61-byte `STAT` wire frame, populating all wire offsets.
    pub fn build_stat_report(report: &StatReport, out: &mut [u8]) -> Result<usize, ParseError> {
        if out.len() < STAT_FRAME_SIZE {
            return Err(ParseError::PayloadTooLarge);
        }

        // Header (8 bytes)
        out[0..4].copy_from_slice(b"STAT");
        out[4..8].copy_from_slice(&(STAT_PAYLOAD_SIZE as u32).to_be_bytes());

        // Payload (53 bytes, offsets relative to frame start = 8 + payload_offset)
        out[8..12].copy_from_slice(&report.event.opcode_bytes());
        out[12] = 0; // num_crlf
        out[13] = 0; // mas_initialized
        out[14] = 0; // mas_mode
        out[15..19].copy_from_slice(&report.stream_buffer_size.to_be_bytes());
        out[19..23].copy_from_slice(&report.stream_buffer_fullness.to_be_bytes());
        out[23..31].copy_from_slice(&report.bytes_received.to_be_bytes());
        out[31..33].copy_from_slice(&report.signal_strength.to_be_bytes());
        out[33..37].copy_from_slice(&report.jiffies.to_be_bytes());
        out[37..41].copy_from_slice(&report.output_buffer_size.to_be_bytes());
        out[41..45].copy_from_slice(&report.output_buffer_fullness.to_be_bytes());
        out[45..49].copy_from_slice(&report.elapsed_seconds.to_be_bytes());
        out[49..51].copy_from_slice(&report.voltage.to_be_bytes());
        out[51..55].copy_from_slice(&report.elapsed_ms.to_be_bytes());
        out[55..59].copy_from_slice(&report.server_timestamp.to_be_bytes());
        out[59..61].copy_from_slice(&0u16.to_be_bytes()); // error_code

        Ok(STAT_FRAME_SIZE)
    }

    /// Formats a STAT telemetry heartbeat packet into a provided byte buffer.
    /// Returns the total bytes written (61 bytes).
    pub fn build_stat(
        event: StatEvent,
        stream_buffer_fullness: u32,
        output_buffer_fullness: u32,
        frames_played: u32,
        timestamp_ms: u32,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        Self::build_stat_full(
            event,
            stream_buffer_fullness,
            output_buffer_fullness,
            frames_played,
            timestamp_ms,
            out,
        )
    }

    /// Formats the full official 53-byte payload STAT telemetry packet into a provided byte buffer.
    /// Total frame length is 61 bytes (8-byte header + 53-byte payload).
    pub fn build_stat_full(
        event: StatEvent,
        stream_buffer_fullness: u32,
        output_buffer_fullness: u32,
        frames_played: u32,
        timestamp_ms: u32,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        let report = StatReport {
            event,
            stream_buffer_size: 65536,
            stream_buffer_fullness,
            bytes_received: frames_played as u64,
            signal_strength: 0xFFFF,
            jiffies: timestamp_ms,
            output_buffer_size: 65536,
            output_buffer_fullness,
            elapsed_seconds: timestamp_ms / 1000,
            voltage: 0,
            elapsed_ms: timestamp_ms,
            server_timestamp: 0,
        };
        Self::build_stat_report(&report, out)
    }

    /// Formats an HTTP response packet (`RESP`) sending received HTTP headers to LMS.
    pub fn build_resp(headers: &[u8], out: &mut [u8]) -> Result<usize, ParseError> {
        let total_len = 8 + headers.len();
        if out.len() < total_len {
            return Err(ParseError::PayloadTooLarge);
        }

        out[0..4].copy_from_slice(b"RESP");
        out[4..8].copy_from_slice(&(headers.len() as u32).to_be_bytes());
        out[8..total_len].copy_from_slice(headers);

        Ok(total_len)
    }

    /// Formats a disconnect notification packet (`DSCO`) sending disconnect reason to LMS.
    pub fn build_dsco(reason: DisconnectReason, out: &mut [u8]) -> Result<usize, ParseError> {
        if out.len() < 9 {
            return Err(ParseError::PayloadTooLarge);
        }

        out[0..4].copy_from_slice(b"DSCO");
        out[4..8].copy_from_slice(&1u32.to_be_bytes());
        out[8] = reason.to_byte();

        Ok(9)
    }

    /// Formats a stream metadata packet (`META`) forwarding stream metadata (e.g. ICY) to LMS.
    pub fn build_meta(metadata: &[u8], out: &mut [u8]) -> Result<usize, ParseError> {
        let total_len = 8 + metadata.len();
        if out.len() < total_len {
            return Err(ParseError::PayloadTooLarge);
        }

        out[0..4].copy_from_slice(b"META");
        out[4..8].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
        out[8..total_len].copy_from_slice(metadata);

        Ok(total_len)
    }

    /// Formats a player name device setting packet (`SETD` with id 0x00) for LMS.
    pub fn build_setd_name(name: &str, out: &mut [u8]) -> Result<usize, ParseError> {
        let name_bytes = name.as_bytes();
        let payload_len = 1 + name_bytes.len();
        let total_len = 8 + payload_len;

        if out.len() < total_len {
            return Err(ParseError::PayloadTooLarge);
        }

        out[0..4].copy_from_slice(b"SETD");
        out[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());
        out[8] = 0x00;
        out[9..total_len].copy_from_slice(name_bytes);

        Ok(total_len)
    }

    /// Formats an infrared remote control packet (`IR  `) forwarding NEC IR button presses to LMS.
    /// Wire format: 4-byte opcode (`b"IR  "`), 4-byte length (10), 4-byte timestamp, 1-byte code type (0xFF), 1-byte bits (16), 4-byte IR code.
    pub fn build_ir(time_ms: u32, ir_code: u32, out: &mut [u8]) -> Result<usize, BuildError> {
        if out.len() < 18 {
            return Err(ParseError::PayloadTooLarge);
        }

        out[0..4].copy_from_slice(b"IR  ");
        out[4..8].copy_from_slice(&10u32.to_be_bytes());
        out[8..12].copy_from_slice(&time_ms.to_be_bytes());
        out[12] = 0xFF;
        out[13] = 16;
        out[14..18].copy_from_slice(&ir_code.to_be_bytes());

        Ok(18)
    }
}

/// Length of a `serv` SyncgroupID (C `slimproto.c:641`: `len - sizeof(serv_packet) == 10`).
pub const SYNCGROUP_ID_LEN: usize = 10;

/// `serv` address that C maps to SqueezeNetwork (`slimproto.c:633-634`, `pkt[4..8] == 0.0.0.1`).
pub const SQUEEZENETWORK_MARKER_IP: [u8; 4] = [0, 0, 0, 1];

/// Writes the `HELO` capability string.
///
/// C builds it from three parts (`sendHELO`, `slimproto.c:128-185`):
/// * `base_cap` (`embedded.h:47-49`): `Model=squeezeesp32,AccuratePlayPoints=1,HasDigitalOut=1,
///   HasPolarityInversion=1,Balance=1,Depth=<16|32>,Firmware=<VERSION>`;
/// * `fixed_cap` (`slimproto.c:1068-1082`): `,ModelName=<model>,MaxSampleRate=<rate>` and one
///   `,<types>` per registered codec;
/// * `var_cap` (`slimproto.c:1157-1174`): `,loc` for a local player and the `serv`
///   `,SyncgroupID=<10 chars>`, used for one `HELO` only.
///
/// This writes the same fields in the same order, minus the ones the Rust firmware cannot back
/// (deliberate divergences): no `HasDigitalOut` (no SPDIF yet), no `HasPolarityInversion` (the
/// gain path has no inversion until Task 14), no `Balance` (the device collapses `audg` L/R onto
/// one amplifier gain), no `,loc` (never a local player). `Depth=32` matches C's
/// `BYTES_PER_FRAME == 8` branch: the Rust output ring holds 8-byte stereo `i32` frames.
/// A target that cannot hold the whole string is an error, never a truncation.
pub fn build_capabilities<W: fmt::Write>(
    model_name: &str,
    firmware: &str,
    max_rate: u32,
    syncgroup: Option<&str>,
    codecs: &str,
    out: &mut W,
) -> Result<(), ParseError> {
    let write = |out: &mut W| -> fmt::Result {
        write!(out, "Model=squeezeesp32,AccuratePlayPoints=1,Depth=32,Firmware={firmware}")?;
        write!(out, ",ModelName={model_name},MaxSampleRate={max_rate},{codecs}")?;
        if let Some(id) = syncgroup {
            write!(out, ",SyncgroupID={id}")?;
        }
        Ok(())
    };
    write(out).map_err(|_| ParseError::PayloadTooLarge)
}

/// Delay before reconnecting after the server connection dropped, for the `attempt`-th
/// consecutive drop (0 = first), with `seed` as the random source.
///
/// C parity (MK5, `slimproto.c:1196-1203`): `reconnect_delay` starts at 1 s and doubles after
/// every drop, capped at 30 s; the delay is `reconnect_delay * 1000 + rand() % 1000` ms. So the
/// sequence is 1, 2, 4, 8, 16, 30, 30, ... s plus 0..999 ms of jitter. (The caller resets
/// `attempt` to 0 after a session that lasted over 30 s, `slimproto.c:1191-1193`, and after a
/// `serv` switch, `slimproto.c:1103`.)
pub fn next_backoff_ms(attempt: u32, seed: u32) -> u32 {
    const CAP_S: u32 = 30;
    let base_s = if attempt >= 5 { CAP_S } else { (1u32 << attempt).min(CAP_S) };
    base_s * 1000 + seed % 1000
}

/// Discovered Logitech Media Server details from UDP broadcast query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredServer {
    pub json_port: u16,
    pub cli_port: u16,
}

pub struct DiscoveryBuilder;

impl DiscoveryBuilder {
    /// Formats the standard LMS UDP discovery query payload: `b"eJSON\0CLIP\0"`.
    pub fn build_query(out: &mut [u8]) -> Result<usize, ParseError> {
        const QUERY: &[u8] = b"eJSON\0CLIP\0";
        if out.len() < QUERY.len() {
            return Err(ParseError::PayloadTooLarge);
        }
        out[..QUERY.len()].copy_from_slice(QUERY);
        Ok(QUERY.len())
    }
}

pub struct DiscoveryParser;

impl DiscoveryParser {
    /// Alias for [`Self::parse_response`] that parses an LMS UDP discovery response.
    pub fn parse_discovery_response(buf: &[u8]) -> Option<DiscoveredServer> {
        Self::parse_response(buf)
    }

    /// Parses an LMS discovery response (must start with uppercase `b'E'`).
    /// Rejects client reflection queries starting with `b'e'`.
    ///
    /// Supports standard LMS 1-byte TLV length-prefixed tags (`JSON`, `CLIP`, `PORT`,
    /// skipping informational tags like `NAME`, `VERS`, `UUID`), null-terminated fallback
    /// (`JSON`/`CLIP` + `port_str\0`), and 2-byte big-endian binary port responses.
    pub fn parse_response(buf: &[u8]) -> Option<DiscoveredServer> {
        if buf.is_empty() || buf[0] != b'E' {
            return None;
        }

        let mut json_port = 9000;
        let mut cli_port = 9090;
        let mut tag_found = false;

        let payload = &buf[1..];

        // Legacy 2-byte big-endian port right after 'E'
        if payload.len() == 2 {
            let port = u16::from_be_bytes([payload[0], payload[1]]);
            if port > 0 {
                return Some(DiscoveredServer {
                    json_port: port,
                    cli_port,
                });
            }
        }

        let mut i = 0;
        while i < payload.len() {
            let rem = &payload[i..];
            let is_json = rem.starts_with(b"JSON");
            let is_clip = rem.starts_with(b"CLIP");
            let is_port = rem.starts_with(b"PORT");

            if is_json || is_clip || is_port {
                let tag_end = i + 4;
                let tlv_parsed = if tag_end < payload.len() {
                    let tlv_len = payload[tag_end] as usize;
                    if (1..=5).contains(&tlv_len) && tag_end + 1 + tlv_len <= payload.len() {
                        let val_bytes = &payload[tag_end + 1..tag_end + 1 + tlv_len];
                        if let Ok(s) = core::str::from_utf8(val_bytes) {
                            if let Ok(port) = parse_u16(s.trim_end_matches('\0')) {
                                Some((port, tag_end + 1 + tlv_len))
                            } else if tlv_len == 2 {
                                let port = u16::from_be_bytes([val_bytes[0], val_bytes[1]]);
                                (port > 0).then_some((port, tag_end + 1 + tlv_len))
                            } else {
                                None
                            }
                        } else if tlv_len == 2 {
                            let port = u16::from_be_bytes([val_bytes[0], val_bytes[1]]);
                            (port > 0).then_some((port, tag_end + 1 + tlv_len))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some((port, next_i)) = tlv_parsed {
                    if is_clip {
                        cli_port = port;
                    } else {
                        json_port = port;
                    }
                    tag_found = true;
                    i = next_i;
                    continue;
                } else if let Some(end) = payload[tag_end..].iter().position(|&b| b == 0) {
                    // Fallback to null-terminated: Tag + port_str\0
                    if let Ok(s) = core::str::from_utf8(&payload[tag_end..tag_end + end]) {
                        if let Ok(port) = parse_u16(s) {
                            if is_clip {
                                cli_port = port;
                            } else {
                                json_port = port;
                            }
                            tag_found = true;
                            i = tag_end + end + 1;
                            continue;
                        }
                    }
                }
            } else if rem.starts_with(b"NAME")
                || rem.starts_with(b"VERS")
                || rem.starts_with(b"UUID")
                || rem.starts_with(b"IPAD")
            {
                let tag_end = i + 4;
                if tag_end < payload.len() {
                    let tlv_len = payload[tag_end] as usize;
                    if tag_end + 1 + tlv_len <= payload.len() {
                        i = tag_end + 1 + tlv_len;
                        continue;
                    }
                }
            }
            i += 1;
        }

        if tag_found {
            Some(DiscoveredServer { json_port, cli_port })
        } else {
            None
        }
    }
}

fn parse_u16(s: &str) -> Result<u16, ()> {
    if s.is_empty() {
        return Err(());
    }
    let mut res: u16 = 0;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return Err(());
        }
        res = res.checked_mul(10).ok_or(())?.checked_add((b - b'0') as u16).ok_or(())?;
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_incomplete_header() {
        let buf = *b"str";
        assert_eq!(SlimprotoParser::parse_frame(&buf), Err(ParseError::BufferTooShort));
    }

    #[test]
    fn test_parse_frame_payload_too_large() {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(b"body");
        buf[4..8].copy_from_slice(&(5000u32).to_be_bytes()); // Exceeds MAX_FRAME_LENGTH 4096
        assert_eq!(SlimprotoParser::parse_frame(&buf), Err(ParseError::PayloadTooLarge));
    }

    #[test]
    fn test_parse_audg_frame() {
        let mut frame = [0u8; 26]; // 8 header + 18 payload
        frame[0..4].copy_from_slice(b"audg");
        frame[4..8].copy_from_slice(&18u32.to_be_bytes()); // length = 18
        // 8..16 legacy old_gain
        frame[16] = 255; // atten
        frame[17] = 0;   // preamp
        frame[18..22].copy_from_slice(&0x00010000u32.to_be_bytes()); // gain L (active)
        frame[22..26].copy_from_slice(&0x00010000u32.to_be_bytes()); // gain R (active)

        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(consumed, 26);
        assert_eq!(
            cmd,
            ServerCommand::AudioGain {
                gain_left: 0x00010000,
                gain_right: 0x00010000,
                digital_atten: 255,
                preamp: 0,
            }
        );
    }

    #[test]
    fn test_parse_strm_start_frame() {
        let mut frame = [0u8; 32];
        frame[0..4].copy_from_slice(b"strm");
        frame[4..8].copy_from_slice(&24u32.to_be_bytes()); // length = 24
        frame[8] = b's'; // start
        frame[9] = b'1'; // autostart
        frame[10] = b'f'; // flac
        frame[11] = 16; // sample size
        frame[12] = 44; // 44.1k
        frame[13] = 2;  // stereo
        frame[14] = b'1'; // little endian
        frame[15] = 200; // 200KB threshold (1 byte)
        frame[16] = 0; // spdif
        frame[17] = 0; // trans_dur
        frame[18] = 0; // trans_type
        frame[19] = 0; // flags
        frame[20] = 0; // output_thresh
        frame[21] = 0; // slaves
        frame[22..26].copy_from_slice(&0u32.to_be_bytes()); // replay gain
        frame[26..28].copy_from_slice(&9000u16.to_be_bytes()); // server port
        frame[28..32].copy_from_slice(&[192, 168, 1, 50]); // server ip

        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(consumed, 32);
        match cmd {
            ServerCommand::Stream {
                command,
                autostart,
                format,
                threshold_kb,
                server_ip,
                ..
            } => {
                assert_eq!(command, StreamCommandType::Start);
                assert!(autostart);
                assert_eq!(format, AudioFormat::Flac);
                assert_eq!(threshold_kb, 200);
                assert_eq!(server_ip, [192, 168, 1, 50]);
            }
            _ => panic!("Expected Stream command"),
        }
    }

    #[test]
    fn test_build_helo_packet() {
        let mac = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut buf = [0u8; 128];
        let len = SlimprotoBuilder::build_helo(&mac, 12, "Model=LouderESP32Plus", &mut buf).unwrap();

        assert_eq!(&buf[0..4], b"HELO");
        assert_eq!(buf[8], 12);
        assert_eq!(&buf[10..16], &mac);
        assert_eq!(&buf[16..44], &[0u8; 28]); // UUID, WLAN, bytes received, lang padding
        assert_eq!(len, 8 + 36 + "Model=LouderESP32Plus".len());
    }

    #[test]
    fn test_build_stat_packet() {
        let mut buf = [0u8; 64];
        let len = SlimprotoBuilder::build_stat(
            StatEvent::TrackStarted,
            128000,
            64000,
            44100,
            1000,
            &mut buf,
        )
        .unwrap();

        assert_eq!(len, 61);
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[4..8], &53u32.to_be_bytes());
        assert_eq!(&buf[8..12], b"STMs");
        assert_eq!(buf[12], 0); // num_crlf
        assert_eq!(buf[13], 0); // mas_initialized
        assert_eq!(buf[14], 0); // mas_mode
        assert_eq!(u32::from_be_bytes([buf[15], buf[16], buf[17], buf[18]]), 65536);
        assert_eq!(u32::from_be_bytes([buf[19], buf[20], buf[21], buf[22]]), 128000);
        assert_eq!(u32::from_be_bytes([buf[23], buf[24], buf[25], buf[26]]), 0);
        assert_eq!(u32::from_be_bytes([buf[27], buf[28], buf[29], buf[30]]), 44100);
        assert_eq!(u16::from_be_bytes([buf[31], buf[32]]), 0xFFFF);
        assert_eq!(u32::from_be_bytes([buf[33], buf[34], buf[35], buf[36]]), 1000);
        assert_eq!(u32::from_be_bytes([buf[37], buf[38], buf[39], buf[40]]), 65536);
        assert_eq!(u32::from_be_bytes([buf[41], buf[42], buf[43], buf[44]]), 64000);
        assert_eq!(u32::from_be_bytes([buf[45], buf[46], buf[47], buf[48]]), 1); // 1000 / 1000
        assert_eq!(u16::from_be_bytes([buf[49], buf[50]]), 0);
        assert_eq!(u32::from_be_bytes([buf[51], buf[52], buf[53], buf[54]]), 1000);
        assert_eq!(u32::from_be_bytes([buf[55], buf[56], buf[57], buf[58]]), 0);
        assert_eq!(u16::from_be_bytes([buf[59], buf[60]]), 0);

        // Verify buffer too short
        let mut small = [0u8; 60];
        assert_eq!(
            SlimprotoBuilder::build_stat(StatEvent::Heartbeat, 0, 0, 0, 0, &mut small),
            Err(ParseError::PayloadTooLarge)
        );
    }

    #[test]
    fn test_parse_server_to_client_6byte_frame() {
        // 6-byte header: length = 4 (opcode only, payload = 0)
        let frame = [0x00, 0x04, b'f', b'l', b's', b'h'];
        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(consumed, 6);
        assert_eq!(cmd, ServerCommand::Flush);

        // 6-byte header with strm and autostart b'3'
        let mut strm_frame = [0u8; 30]; // 2 len + 4 opcode + 24 payload
        strm_frame[0..2].copy_from_slice(&28u16.to_be_bytes()); // length = 28
        strm_frame[2..6].copy_from_slice(b"strm");
        strm_frame[6] = b's'; // start
        strm_frame[7] = b'3'; // direct autostart
        strm_frame[8] = b'f'; // flac
        strm_frame[9] = 16;
        strm_frame[10] = 44;
        strm_frame[11] = 2;
        strm_frame[12] = b'1';
        strm_frame[13] = 200;
        strm_frame[24..26].copy_from_slice(&9000u16.to_be_bytes());
        strm_frame[26..30].copy_from_slice(&[10, 0, 0, 1]);

        let (cmd, consumed) = SlimprotoParser::parse_frame(&strm_frame).unwrap();
        assert_eq!(consumed, 30);
        match cmd {
            ServerCommand::Stream { autostart, command, .. } => {
                assert!(autostart);
                assert_eq!(command, StreamCommandType::Start);
            }
            _ => panic!("Expected Stream command"),
        }
    }

    #[test]
    fn test_parse_setd_variants() {
        // Standard 4-byte payload (6-byte frame header)
        let mut frame_4b = [0u8; 10]; // 2 + 4 + 4
        frame_4b[0..2].copy_from_slice(&8u16.to_be_bytes()); // length = 8 (4 opcode + 4 payload)
        frame_4b[2..6].copy_from_slice(b"setd");
        frame_4b[6..10].copy_from_slice(&44100u32.to_be_bytes());
        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame_4b).unwrap();
        assert_eq!(consumed, 10);
        assert_eq!(cmd, ServerCommand::SetDevice { sample_rate: 44100 });

        // 1-byte payload (6-byte frame header)
        let mut frame_1b = [0u8; 7]; // 2 + 4 + 1
        frame_1b[0..2].copy_from_slice(&5u16.to_be_bytes()); // length = 5 (4 opcode + 1 payload)
        frame_1b[2..6].copy_from_slice(b"setd");
        frame_1b[6] = 48; // setting ID / sample rate byte
        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame_1b).unwrap();
        assert_eq!(consumed, 7);
        assert_eq!(cmd, ServerCommand::SetDevice { sample_rate: 48 });

        // 1-byte payload (8-byte frame header)
        let mut frame_8b_1p = [0u8; 9];
        frame_8b_1p[0..4].copy_from_slice(b"setd");
        frame_8b_1p[4..8].copy_from_slice(&1u32.to_be_bytes());
        frame_8b_1p[8] = 96;
        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame_8b_1p).unwrap();
        assert_eq!(consumed, 9);
        assert_eq!(cmd, ServerCommand::SetDevice { sample_rate: 96 });
    }

    #[test]
    fn test_negative_truncation_bounds() {
        // Truncated strm payload (< 24 bytes)
        let mut frame = [0u8; 31]; // 8 header + 23 payload
        frame[0..4].copy_from_slice(b"strm");
        frame[4..8].copy_from_slice(&23u32.to_be_bytes());
        assert_eq!(SlimprotoParser::parse_frame(&frame), Err(ParseError::InvalidFormat));

        // Truncated audg payload (< 18 bytes)
        let mut frame = [0u8; 25]; // 8 header + 17 payload
        frame[0..4].copy_from_slice(b"audg");
        frame[4..8].copy_from_slice(&17u32.to_be_bytes());
        assert_eq!(SlimprotoParser::parse_frame(&frame), Err(ParseError::InvalidFormat));

        // Empty setd payload
        let mut frame = [0u8; 8];
        frame[0..4].copy_from_slice(b"setd");
        frame[4..8].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(SlimprotoParser::parse_frame(&frame), Err(ParseError::InvalidFormat));

        // 6-byte frame header with length < 4
        let frame = [0x00, 0x03, b's', b't', b'r', b'm'];
        assert_eq!(SlimprotoParser::parse_frame(&frame), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_discovery_query_and_response() {
        let mut query = [0u8; 32];
        let qlen = DiscoveryBuilder::build_query(&mut query).unwrap();
        assert_eq!(&query[..qlen], b"eJSON\0CLIP\0");

        // Client query reflection must be REJECTED
        assert_eq!(DiscoveryParser::parse_response(b"eJSON\0CLIP\0"), None);

        // Server response packet with null-termination
        let response_null = b"EJSON9000\0CLIP9090\0";
        let server = DiscoveryParser::parse_response(response_null).unwrap();
        assert_eq!(server.json_port, 9000);
        assert_eq!(server.cli_port, 9090);

        // Server response packet with standard LMS 1-byte TLV
        let response_tlv = b"EJSON\x049000CLIP\x049090";
        let server_tlv = DiscoveryParser::parse_response(response_tlv).unwrap();
        assert_eq!(server_tlv.json_port, 9000);
        assert_eq!(server_tlv.cli_port, 9090);

        // Discovery response with leading NAME/VERS TLV tags and PORT tag (PROTOCFG-806)
        let response_multi_tlv = b"ENAME\x06MyLMS1VERS\x058.5.0PORT\x043483CLIP\x049091";
        let server_multi = DiscoveryParser::parse_discovery_response(response_multi_tlv).unwrap();
        assert_eq!(server_multi.json_port, 3483);
        assert_eq!(server_multi.cli_port, 9091);

        // Legacy 2-byte big-endian port response after 'E'
        let response_2byte = [b'E', 0x23, 0x28]; // 0x2328 = 9000
        let server_2b = DiscoveryParser::parse_discovery_response(&response_2byte).unwrap();
        assert_eq!(server_2b.json_port, 9000);
        assert_eq!(server_2b.cli_port, 9090);
    }

    #[test]
    fn test_setd_device_setting_player_name_and_dac(
    ) {
        // Player name update (id = 0, data = "StudioPlayer\0") must NOT be parsed as sample_rate (PROTOCFG-801)
        let payload = b"\x00StudioPlayer\0";
        let len = (4 + payload.len()) as u16;
        let mut frame = [0u8; 32];
        frame[0..2].copy_from_slice(&len.to_be_bytes());
        frame[2..6].copy_from_slice(b"setd");
        frame[6..6 + payload.len()].copy_from_slice(payload);

        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame[..6 + payload.len()]).unwrap();
        assert_eq!(consumed, 6 + payload.len());
        let setting = cmd.device_setting().expect("Expected device setting");
        assert_eq!(setting.id, 0);
        assert_eq!(setting.data, b"StudioPlayer\0");

        let direct = SlimprotoParser::parse_setd_setting(payload).unwrap();
        assert_eq!(direct.id, 0);
        assert_eq!(direct.data, b"StudioPlayer\0");

        // Disable DAC query (1-byte id = 4)
        let frame_dac = [0x00, 0x05, b's', b'e', b't', b'd', 0x04];
        let (cmd_dac, _) = SlimprotoParser::parse_frame(&frame_dac).unwrap();
        let dac_setting = cmd_dac.device_setting().expect("Expected 1-byte device setting");
        assert_eq!(dac_setting.id, 4);
        assert!(dac_setting.data.is_empty());
    }

    #[test]
    fn test_stat_event_opcodes_and_stat_report_wire_offsets() {
        // PROTOCFG-802: TrackResumed is STMr; DecodeReady, NotSupported, ConnectEstablished supported
        assert_eq!(&StatEvent::TrackResumed.opcode_bytes(), b"STMr");
        assert_eq!(&StatEvent::DecodeReady.opcode_bytes(), b"STMd");
        assert_eq!(&StatEvent::NotSupported.opcode_bytes(), b"STMn");
        assert_eq!(&StatEvent::ConnectEstablished.opcode_bytes(), b"STMc");
        assert_eq!(&StatEvent::Connected.opcode_bytes(), b"STMc");

        let report = StatReport {
            event: StatEvent::TrackResumed,
            stream_buffer_size: 262_144,
            stream_buffer_fullness: 131_072,
            bytes_received: 0x0000_0001_8000_0000,
            signal_strength: 0x1234,
            jiffies: 55_000,
            output_buffer_size: 131_072,
            output_buffer_fullness: 65_536,
            elapsed_seconds: 42,
            voltage: 3300,
            elapsed_ms: 42_500,
            server_timestamp: 99_887_766,
        };

        let mut frame = [0u8; STAT_FRAME_SIZE];
        let written = StatBuilder::build_stat_report(&report, &mut frame).unwrap();
        assert_eq!(written, STAT_FRAME_SIZE);
        assert_eq!(&frame[0..4], b"STAT");
        assert_eq!(u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]), 53);
        assert_eq!(&frame[8..12], b"STMr");

        // Verify payload offsets relative to payload start (&frame[8..])
        let payload = &frame[8..];
        assert_eq!(u64::from_be_bytes(payload[15..23].try_into().unwrap()), 0x0000_0001_8000_0000);
        assert_eq!(u32::from_be_bytes(payload[37..41].try_into().unwrap()), 42);
        assert_eq!(u16::from_be_bytes(payload[41..43].try_into().unwrap()), 3300);
        assert_eq!(u32::from_be_bytes(payload[43..47].try_into().unwrap()), 42_500);
        assert_eq!(u32::from_be_bytes(payload[47..51].try_into().unwrap()), 99_887_766);

        // StatBuilder::build_stat estimates elapsed_seconds and elapsed_ms from frames_played / 44_100
        let mut frame2 = [0u8; STAT_FRAME_SIZE];
        StatBuilder::build_stat(StatEvent::DecodeReady, 4096, 2048, 88_200, 5000, &mut frame2).unwrap();
        let payload2 = &frame2[8..];
        assert_eq!(&payload2[0..4], b"STMd");
        assert_eq!(u32::from_be_bytes(payload2[37..41].try_into().unwrap()), 2); // 88200 / 44100 = 2s
        assert_eq!(u32::from_be_bytes(payload2[43..47].try_into().unwrap()), 2000); // 2000ms
    }

    #[test]
    fn test_strm_timestamp_and_skip_ahead_and_extended_opcodes() {
        // PROTOCFG-803: 't' and 'a' stream commands
        assert_eq!(StreamCommandType::from_byte(b't'), StreamCommandType::Timestamp);
        assert!(StreamCommandType::from_byte(b't').is_timestamp());
        assert_eq!(StreamCommandType::Timestamp.to_byte(), b't');

        assert_eq!(StreamCommandType::from_byte(b'a'), StreamCommandType::SkipAhead);
        assert!(StreamCommandType::from_byte(b'a').is_skip_ahead());
        assert_eq!(StreamCommandType::SkipAhead.to_byte(), b'a');

        // PROTOCFG-804: aude, cont, serv, vers, dsco frames
        let aude_frame = [0x00, 0x06, b'a', b'u', b'd', b'e', 1, 0];
        let (cmd_aude, _) = SlimprotoParser::parse_frame(&aude_frame).unwrap();
        assert_eq!(cmd_aude.audio_enable(), Some((true, false)));

        let cont_frame = [0x00, 0x08, b'c', b'o', b'n', b't', 0, 0, 0x40, 0x00];
        let (cmd_cont, _) = SlimprotoParser::parse_frame(&cont_frame).unwrap();
        assert_eq!(cmd_cont.stream_continue(), Some(16384));

        let serv_frame = [0x00, 0x08, b's', b'e', b'r', b'v', 192, 168, 1, 99];
        let (cmd_serv, _) = SlimprotoParser::parse_frame(&serv_frame).unwrap();
        assert_eq!(cmd_serv.switch_server(), Some(([192, 168, 1, 99], None)));

        let vers_frame = b"\x00\x09vers8.5.1";
        let (cmd_vers, _) = SlimprotoParser::parse_frame(vers_frame).unwrap();
        assert_eq!(cmd_vers.server_version(), Some("8.5.1"));

        let dsco_frame = [0x00, 0x05, b'd', b's', b'c', b'o', 0];
        let (cmd_dsco, _) = SlimprotoParser::parse_frame(&dsco_frame).unwrap();
        assert_eq!(cmd_dsco.stream_disconnected(), Some(0));
    }

    #[test]
    fn test_audg_negative_db_mute_indicator() {
        // PROTOCFG-805: negative old_gain_left/right mutes channel when raw_gain is zero
        let mut frame = [0u8; 26];
        frame[0..4].copy_from_slice(b"audg");
        frame[4..8].copy_from_slice(&18u32.to_be_bytes());
        frame[8..12].copy_from_slice(&(-30i32).to_be_bytes()); // old_left < 0
        frame[12..16].copy_from_slice(&(-30i32).to_be_bytes()); // old_right < 0
        frame[16] = 1;
        frame[17] = 255;
        frame[18..22].copy_from_slice(&0u32.to_be_bytes());
        frame[22..26].copy_from_slice(&0u32.to_be_bytes());

        let (cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(
            cmd,
            ServerCommand::AudioGain {
                gain_left: 0,
                gain_right: 0,
                digital_atten: 1,
                preamp: 255,
            }
        );
    }

    #[test]
    fn test_audg_negative_db_preserves_nonzero_raw() {
        // CODE-PROTO-001: non-zero raw_gain is preserved even if old_left/right < 0
        let mut frame = [0u8; 26];
        frame[0..4].copy_from_slice(b"audg");
        frame[4..8].copy_from_slice(&18u32.to_be_bytes());
        frame[8..12].copy_from_slice(&(-30i32).to_be_bytes());
        frame[12..16].copy_from_slice(&(-30i32).to_be_bytes());
        frame[16] = 1;
        frame[17] = 255;
        frame[18..22].copy_from_slice(&0x00010000u32.to_be_bytes());
        frame[22..26].copy_from_slice(&0x00010000u32.to_be_bytes());

        let (cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(
            cmd,
            ServerCommand::AudioGain {
                gain_left: 0x00010000,
                gain_right: 0x00010000,
                digital_atten: 1,
                preamp: 255,
            }
        );
    }

    #[test]
    fn test_build_resp_dsco_meta_and_setd_name() {
        // RESP
        let headers = b"HTTP/1.0 200 OK\r\nContent-Type: audio/flac\r\n\r\n";
        let mut buf_resp = [0u8; 128];
        let len_resp = SlimprotoBuilder::build_resp(headers, &mut buf_resp).unwrap();
        assert_eq!(len_resp, 8 + headers.len());
        assert_eq!(&buf_resp[0..4], b"RESP");
        assert_eq!(
            u32::from_be_bytes([buf_resp[4], buf_resp[5], buf_resp[6], buf_resp[7]]),
            headers.len() as u32
        );
        assert_eq!(&buf_resp[8..len_resp], headers);

        // DSCO
        let mut buf_dsco = [0u8; 16];
        let len_dsco =
            SlimprotoBuilder::build_dsco(DisconnectReason::ResetByRemote, &mut buf_dsco).unwrap();
        assert_eq!(len_dsco, 9);
        assert_eq!(&buf_dsco[0..4], b"DSCO");
        assert_eq!(
            u32::from_be_bytes([buf_dsco[4], buf_dsco[5], buf_dsco[6], buf_dsco[7]]),
            1
        );
        assert_eq!(buf_dsco[8], DisconnectReason::ResetByRemote as u8);

        // META
        let meta = b"TITLE=Song Name\0ARTIST=Artist Name\0";
        let mut buf_meta = [0u8; 64];
        let len_meta = SlimprotoBuilder::build_meta(meta, &mut buf_meta).unwrap();
        assert_eq!(len_meta, 8 + meta.len());
        assert_eq!(&buf_meta[0..4], b"META");
        assert_eq!(
            u32::from_be_bytes([buf_meta[4], buf_meta[5], buf_meta[6], buf_meta[7]]),
            meta.len() as u32
        );
        assert_eq!(&buf_meta[8..len_meta], meta);

        // SETD
        let name = "Kitchen Speaker";
        let mut buf_setd = [0u8; 32];
        let len_setd = SlimprotoBuilder::build_setd_name(name, &mut buf_setd).unwrap();
        assert_eq!(len_setd, 8 + 1 + name.len());
        assert_eq!(&buf_setd[0..4], b"SETD");
        assert_eq!(
            u32::from_be_bytes([buf_setd[4], buf_setd[5], buf_setd[6], buf_setd[7]]),
            (1 + name.len()) as u32
        );
        assert_eq!(buf_setd[8], 0x00);
        assert_eq!(&buf_setd[9..len_setd], name.as_bytes());
    }

    #[test]
    fn test_stat_event_stmu_stmc_stmf_and_build_stat_with_metrics() {
        assert_eq!(&StatEvent::TrackUnderrun.opcode_bytes(), b"STMu");
        assert_eq!(&StatEvent::Connected.opcode_bytes(), b"STMc");
        assert_eq!(&StatEvent::StreamFlushed.opcode_bytes(), b"STMf");
        assert_eq!(&StatEvent::StreamUnderrun.opcode_bytes(), b"STMl");
        assert_eq!(StatEvent::BUFFER_THRESHOLD, StatEvent::StreamUnderrun);

        let mut frame = [0u8; STAT_FRAME_SIZE];
        let sample_rate = 96_000;
        let frames_played = 192_000u64;
        let bytes_received = 0x0000_0002_0000_1234u64;
        let jiffies_ms = 45_000;
        let server_timestamp = 0x1234_5678;

        let len = StatBuilder::build_stat_with_metrics(
            StatEvent::TrackUnderrun,
            128_000,
            64_000,
            frames_played,
            sample_rate,
            bytes_received,
            jiffies_ms,
            server_timestamp,
            &mut frame,
        )
        .unwrap();

        assert_eq!(len, STAT_FRAME_SIZE);
        assert_eq!(&frame[0..4], b"STAT");
        assert_eq!(&frame[8..12], b"STMu");

        // bytes_received at frame[23..31] (payload[15..23])
        assert_eq!(
            u64::from_be_bytes(frame[23..31].try_into().unwrap()),
            bytes_received
        );
        // elapsed_seconds at frame[45..49] (payload[37..41]) = 192000 / 96000 = 2
        assert_eq!(
            u32::from_be_bytes(frame[45..49].try_into().unwrap()),
            2
        );
        // elapsed_ms at frame[51..55] (payload[43..47]) = 192000 * 1000 / 96000 = 2000
        assert_eq!(
            u32::from_be_bytes(frame[51..55].try_into().unwrap()),
            2000
        );
        // server_timestamp at payload offset 47..51 (frame offset 55..59)
        assert_eq!(
            u32::from_be_bytes(frame[55..59].try_into().unwrap()),
            server_timestamp
        );
    }

    #[test]
    fn test_autostart_mode_and_88200_sample_rate() {
        // AutostartMode methods
        assert_eq!(AutostartMode::from_byte(b'0'), AutostartMode::Off);
        assert_eq!(AutostartMode::from_byte(b'1'), AutostartMode::Auto);
        assert_eq!(AutostartMode::from_byte(b'2'), AutostartMode::DirectOff);
        assert_eq!(AutostartMode::from_byte(b'3'), AutostartMode::DirectAuto);

        assert!(!AutostartMode::Off.is_auto());
        assert!(AutostartMode::Auto.is_auto());
        assert!(!AutostartMode::DirectOff.is_auto());
        assert!(AutostartMode::DirectAuto.is_auto());

        assert!(!AutostartMode::Off.requires_cont());
        assert!(!AutostartMode::Auto.requires_cont());
        assert!(AutostartMode::DirectOff.requires_cont());
        assert!(AutostartMode::DirectAuto.requires_cont());

        // Test roundtrip via parse_frame with '0'..'3'
        for (byte, expected_mode, expected_cont_flag) in [
            (b'0', AutostartMode::Off, 0),
            (b'1', AutostartMode::Auto, 0),
            (b'2', AutostartMode::DirectOff, STRM_FLAG_CONT_WAIT),
            (b'3', AutostartMode::DirectAuto, STRM_FLAG_CONT_WAIT),
        ] {
            let mut strm_frame = [0u8; 32];
            strm_frame[0..4].copy_from_slice(b"strm");
            strm_frame[4..8].copy_from_slice(&24u32.to_be_bytes());
            strm_frame[8] = b's';
            strm_frame[9] = byte;
            strm_frame[10] = b'f';
            strm_frame[19] = 0; // flags

            let (cmd, _) = SlimprotoParser::parse_frame(&strm_frame).unwrap();
            match cmd {
                ServerCommand::Stream { autostart, flags, .. } => {
                    assert_eq!(flags & STRM_FLAG_CONT_WAIT, expected_cont_flag);
                    let recovered = AutostartMode::from_strm(autostart, flags);
                    assert_eq!(recovered, expected_mode);
                }
                _ => panic!("Expected Stream command"),
            }
        }

        // sample_rate_from_index: 10 | b':' => 88200, and ASCII '0'..='9'
        assert_eq!(sample_rate_from_index(10), 88200);
        assert_eq!(sample_rate_from_index(b':'), 88200);
        assert_eq!(sample_rate_from_index(11), 176400);
        assert_eq!(sample_rate_from_index(b';'), 176400);
        assert_eq!(sample_rate_from_index(12), 192000);
        assert_eq!(sample_rate_from_index(b'<'), 192000);
        assert_eq!(sample_rate_from_index(b'0'), 11025);
        assert_eq!(sample_rate_from_index(b'1'), 22050);
        assert_eq!(sample_rate_from_index(b'2'), 32000);
        assert_eq!(sample_rate_from_index(b'3'), 44100);
        assert_eq!(sample_rate_from_index(b'4'), 48000);
        assert_eq!(sample_rate_from_index(b'5'), 8000);
        assert_eq!(sample_rate_from_index(b'6'), 12000);
        assert_eq!(sample_rate_from_index(b'7'), 16000);
        assert_eq!(sample_rate_from_index(b'8'), 24000);
        assert_eq!(sample_rate_from_index(b'9'), 96000);

        // eqlz / loud helper accessors
        let mut eqlz_frame = [0u8; 14];
        eqlz_frame[0..4].copy_from_slice(b"eqlz");
        eqlz_frame[4..8].copy_from_slice(&6u32.to_be_bytes());
        eqlz_frame[8..14].copy_from_slice(&[10, 20, 30, 40, 50, 60]);
        let (cmd_eqlz, _) = SlimprotoParser::parse_frame(&eqlz_frame).unwrap();
        assert_eq!(cmd_eqlz.equalizer_gains(), Some(&[10, 20, 30, 40, 50, 60][..]));

        let mut loud_frame = [0u8; 9];
        loud_frame[0..4].copy_from_slice(b"loud");
        loud_frame[4..8].copy_from_slice(&1u32.to_be_bytes());
        loud_frame[8] = 42;
        let (cmd_loud, _) = SlimprotoParser::parse_frame(&loud_frame).unwrap();
        assert_eq!(cmd_loud.loudness_gain(), Some(42));
    }

    #[test]
    fn test_build_helo_with_options_sets_reconnect_bit_and_bytes() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let mut buf = [0u8; 128];
        let bytes_received = 0x0102_0304_0506_0708u64;
        let len = SlimprotoBuilder::build_helo_with_options(
            &mac,
            12,
            "Model=Test",
            true,
            bytes_received,
            &mut buf,
        )
        .unwrap();

        assert_eq!(&buf[0..4], b"HELO");
        assert_eq!(buf[8], 12);
        assert_eq!(&buf[10..16], &mac);
        assert_eq!(&buf[32..34], &0x4000u16.to_be_bytes());
        assert_eq!(&buf[34..42], &bytes_received.to_be_bytes());
        assert_eq!(len, 8 + 36 + "Model=Test".len());

        // Without reconnect
        let mut buf_noreconn = [0u8; 128];
        SlimprotoBuilder::build_helo_with_options(
            &mac,
            12,
            "Model=Test",
            false,
            0,
            &mut buf_noreconn,
        )
        .unwrap();
        assert_eq!(&buf_noreconn[32..34], &0u16.to_be_bytes());
        assert_eq!(&buf_noreconn[34..42], &0u64.to_be_bytes());
    }

    #[test]
    fn test_build_ir_encodes_nec_slimproto_frame() {
        let mut buf = [0u8; 32];
        let len = SlimprotoBuilder::build_ir(123456, 0x7689_10EF, &mut buf).unwrap();
        assert_eq!(len, 18);
        assert_eq!(&buf[0..4], b"IR  ");
        assert_eq!(&buf[4..8], &10u32.to_be_bytes());
        assert_eq!(&buf[8..12], &123456u32.to_be_bytes());
        assert_eq!(buf[12], 0xFF);
        assert_eq!(buf[13], 16);
        assert_eq!(&buf[14..18], &0x7689_10EFu32.to_be_bytes());

        let mut short_buf = [0u8; 17];
        assert_eq!(
            SlimprotoBuilder::build_ir(100, 0x1234, &mut short_buf),
            Err(ParseError::PayloadTooLarge)
        );
    }

    /// Fixed (non-SSL) string type for the capability tests.
    struct CapBuf {
        buf: [u8; 256],
        len: usize,
    }

    impl CapBuf {
        fn new() -> Self {
            Self { buf: [0; 256], len: 0 }
        }
        fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap()
        }
    }

    impl fmt::Write for CapBuf {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let end = self.len + s.len();
            if end > self.buf.len() {
                return Err(fmt::Error);
            }
            self.buf[self.len..end].copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    #[test]
    fn test_capabilities_string_exact() {
        // C parity: base_cap (embedded.h:47-49) + fixed_cap (slimproto.c:1068-1082) + var_cap
        // (slimproto.c:1171-1174), minus what the Rust firmware cannot do yet.
        let mut caps = CapBuf::new();
        build_capabilities("SqueezeESP32", "v0.1.0", 96_000, None, "flc,pcm,wav,aif,mp3", &mut caps)
            .unwrap();
        assert_eq!(
            caps.as_str(),
            "Model=squeezeesp32,AccuratePlayPoints=1,Depth=32,Firmware=v0.1.0,\
             ModelName=SqueezeESP32,MaxSampleRate=96000,flc,pcm,wav,aif,mp3"
        );
        assert!(!caps.as_str().contains("HasDigitalOut"));
        assert!(!caps.as_str().contains("HasPolarityInversion"));
        assert!(!caps.as_str().contains("Balance"));

        // A `serv` syncgroup is appended last, exactly as C's var_cap.
        let mut caps = CapBuf::new();
        build_capabilities("SqueezeESP32", "v0.1.0", 192_000, Some("0123456789"), "flc", &mut caps)
            .unwrap();
        assert_eq!(
            caps.as_str(),
            "Model=squeezeesp32,AccuratePlayPoints=1,Depth=32,Firmware=v0.1.0,\
             ModelName=SqueezeESP32,MaxSampleRate=192000,flc,SyncgroupID=0123456789"
        );

        // A target too small for the string is an error, never a silent truncation.
        struct Tiny(usize);
        impl fmt::Write for Tiny {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0 += s.len();
                if self.0 > 16 { Err(fmt::Error) } else { Ok(()) }
            }
        }
        assert_eq!(
            build_capabilities("M", "v", 1, None, "pcm", &mut Tiny(0)),
            Err(ParseError::PayloadTooLarge)
        );
    }

    #[test]
    fn test_backoff_sequence_capped_with_jitter() {
        // C parity (slimproto.c:1196-1203): delay = reconnect_delay s + rand() % 1000 ms,
        // reconnect_delay 1, 2, 4, 8, 16, then capped at 30 s.
        let bases = [1_000u32, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000];
        for (attempt, &base) in bases.iter().enumerate() {
            assert_eq!(next_backoff_ms(attempt as u32, 0), base, "attempt {attempt}");
            assert_eq!(next_backoff_ms(attempt as u32, 999), base + 999, "attempt {attempt}");
            assert_eq!(next_backoff_ms(attempt as u32, 1_000), base, "jitter is seed % 1000");
            assert_eq!(next_backoff_ms(attempt as u32, 12_345), base + 345);
        }
        // Never overflows, however many attempts.
        assert_eq!(next_backoff_ms(u32::MAX, u32::MAX), 30_000 + u32::MAX % 1000);
    }

    #[test]
    fn test_serv_syncgroup_only_when_exactly_ten_bytes() {
        // C parity (slimproto.c:641-656): a 10-byte tail is the SyncgroupID, anything else
        // clears it.
        let mut frame = b"serv\0\0\0\x0e".to_vec();
        frame.extend_from_slice(&[10, 0, 0, 5]);
        frame.extend_from_slice(b"0123456789");
        let (cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(cmd.switch_server(), Some(([10, 0, 0, 5], Some(&b"0123456789"[..]))));

        let mut frame = b"serv\0\0\0\x07".to_vec();
        frame.extend_from_slice(&[10, 0, 0, 5, b'a', b'b', b'c']);
        let (cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(cmd.switch_server(), Some(([10, 0, 0, 5], None)));
    }
}
