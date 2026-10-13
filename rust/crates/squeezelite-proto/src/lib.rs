//! `squeezelite-proto`
//!
//! A zero-allocation, `no_std` SlimProto network protocol engine for connecting
//! to Logitech Media Server (LMS) / Lyrion Music Server.
//!
//! Handles decoding of server packets (`strm`, `audg`, `setd`, `flsh`, `body`)
//! and encoding of client telemetry packets (`HELO`, `STAT`, `RESP`).

#![no_std]

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
    /// Undefined or proprietary stream command.
    Unknown(u8),
}

impl StreamCommandType {
    pub const fn from_byte(b: u8) -> Self {
        match b {
            b's' => Self::Start,
            b'p' => Self::Pause,
            b'u' => Self::Resume,
            b'q' => Self::Stop,
            b'f' => Self::Flush,
            other => Self::Unknown(other),
        }
    }
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
    /// Set audio output device sample rate (`setd`).
    SetDevice {
        sample_rate: u32,
    },
    /// Flush playback buffer (`flsh`).
    Flush,
    /// Metadata body packet (`body`).
    Body {
        data: &'a [u8],
    },
    /// Unrecognized packet opcode.
    Unknown {
        opcode: [u8; 4],
        payload: &'a [u8],
    },
}

/// Zero-allocation parser for SlimProto frame sequences.
pub struct SlimprotoParser;

impl SlimprotoParser {
    /// Parses a single SlimProto frame from a byte slice.
    ///
    /// Returns `Ok((ServerCommand, bytes_consumed))` on success.
    /// Returns `Err(ParseError::BufferTooShort)` if more data is needed.
    pub fn parse_frame(buf: &[u8]) -> Result<(ServerCommand<'_>, usize), ParseError> {
        // SlimProto frame header is 8 bytes: 4-byte opcode + 4-byte big-endian length
        if buf.len() < 8 {
            return Err(ParseError::BufferTooShort);
        }

        let opcode = [buf[0], buf[1], buf[2], buf[3]];
        let length = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
        let total_frame_len = 8 + length;

        if buf.len() < total_frame_len {
            return Err(ParseError::BufferTooShort);
        }

        let payload = &buf[8..total_frame_len];

        let cmd = match &opcode {
            b"strm" => Self::parse_strm(payload)?,
            b"audg" => Self::parse_audg(payload)?,
            b"setd" => Self::parse_setd(payload)?,
            b"flsh" => ServerCommand::Flush,
            b"body" => ServerCommand::Body { data: payload },
            _ => ServerCommand::Unknown { opcode, payload },
        };

        Ok((cmd, total_frame_len))
    }

    fn parse_strm(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 24 {
            return Err(ParseError::InvalidFormat);
        }

        let command = StreamCommandType::from_byte(payload[0]);
        let autostart = payload[1] == b'1';
        let format = AudioFormat::from_byte(payload[2]);
        let pcm_sample_size = payload[3];
        let pcm_sample_rate = payload[4];
        let pcm_channels = payload[5];
        let pcm_endianness = Endianness::from_byte(payload[6]);
        let threshold_kb = u16::from_be_bytes([payload[7], payload[8]]);
        let spdif_enable = payload[9] != 0;
        let trans_duration = payload[10];
        let trans_type = TransitionType::from_byte(payload[11]);
        let flags = payload[12];
        let output_threshold = payload[13];
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
        if payload.len() < 10 {
            return Err(ParseError::InvalidFormat);
        }
        let gain_left = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        let gain_right = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
        let digital_atten = payload[8];
        let preamp = payload[9];

        Ok(ServerCommand::AudioGain {
            gain_left,
            gain_right,
            digital_atten,
            preamp,
        })
    }

    fn parse_setd(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.len() < 4 {
            return Err(ParseError::InvalidFormat);
        }
        let sample_rate = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
        Ok(ServerCommand::SetDevice { sample_rate })
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
    /// Track playback resumed (`STMu`).
    TrackResumed,
    /// Playback stopped (`STMq`).
    TrackStopped,
    /// Stream buffer underrun / starving (`STMu`).
    StreamUnderrun,
    /// Output buffer underrun (`STMo`).
    OutputUnderrun,
    /// Stream flushed (`STMf`).
    StreamFlushed,
    /// Track connection established (`STMc`).
    Connected,
}

impl StatEvent {
    pub const fn opcode_bytes(self) -> [u8; 4] {
        match self {
            Self::Heartbeat => *b"STMt",
            Self::TrackStarted => *b"STMs",
            Self::TrackPaused => *b"STMp",
            Self::TrackResumed => *b"STMu",
            Self::TrackStopped => *b"STMq",
            Self::StreamUnderrun => *b"STMl",
            Self::OutputUnderrun => *b"STMo",
            Self::StreamFlushed => *b"STMf",
            Self::Connected => *b"STMc",
        }
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
        let cap_bytes = capabilities.as_bytes();
        let payload_len = 10 + cap_bytes.len();
        let total_len = 8 + payload_len;

        if out.len() < total_len {
            return Err(ParseError::PayloadTooLarge);
        }

        // Header
        out[0..4].copy_from_slice(b"HELO");
        out[4..8].copy_from_slice(&(payload_len as u32).to_be_bytes());

        // Payload
        out[8] = device_id; // Device ID (e.g. 12 = Squeezebox Receiver / Squeezelite)
        out[9] = 0;         // Firmware revision
        out[10..16].copy_from_slice(mac);
        out[16] = 0;        // UUID / padding
        out[17] = 0;

        out[18..total_len].copy_from_slice(cap_bytes);

        Ok(total_len)
    }

    /// Formats a STAT telemetry heartbeat packet into a provided byte buffer.
    /// Returns the total bytes written.
    pub fn build_stat(
        event: StatEvent,
        stream_buffer_fullness: u32,
        output_buffer_fullness: u32,
        frames_played: u32,
        timestamp_ms: u32,
        out: &mut [u8],
    ) -> Result<usize, ParseError> {
        const PAYLOAD_LEN: usize = 20;
        const TOTAL_LEN: usize = 8 + PAYLOAD_LEN;

        if out.len() < TOTAL_LEN {
            return Err(ParseError::PayloadTooLarge);
        }

        // Header
        out[0..4].copy_from_slice(b"STAT");
        out[4..8].copy_from_slice(&(PAYLOAD_LEN as u32).to_be_bytes());

        // Payload
        out[8..12].copy_from_slice(&event.opcode_bytes());
        out[12..16].copy_from_slice(&stream_buffer_fullness.to_be_bytes());
        out[16..20].copy_from_slice(&output_buffer_fullness.to_be_bytes());
        out[20..24].copy_from_slice(&frames_played.to_be_bytes());
        out[24..28].copy_from_slice(&timestamp_ms.to_be_bytes());

        Ok(TOTAL_LEN)
    }
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
    fn test_parse_audg_frame() {
        let mut frame = [0u8; 18];
        frame[0..4].copy_from_slice(b"audg");
        frame[4..8].copy_from_slice(&10u32.to_be_bytes()); // length = 10
        frame[8..12].copy_from_slice(&0x00010000u32.to_be_bytes()); // gain L
        frame[12..16].copy_from_slice(&0x00010000u32.to_be_bytes()); // gain R
        frame[16] = 255; // atten
        frame[17] = 0;   // preamp

        let (cmd, consumed) = SlimprotoParser::parse_frame(&frame).unwrap();
        assert_eq!(consumed, 18);
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
        frame[15..17].copy_from_slice(&400u16.to_be_bytes()); // 400KB threshold
        frame[17] = 0; // spdif
        frame[18] = 0; // trans_dur
        frame[19] = 0; // trans_type
        frame[20] = 0; // flags
        frame[21] = 0; // output_thresh
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
                assert_eq!(threshold_kb, 400);
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
        assert_eq!(len, 8 + 10 + "Model=LouderESP32Plus".len());
    }

    #[test]
    fn test_build_stat_packet() {
        let mut buf = [0u8; 32];
        let len = SlimprotoBuilder::build_stat(
            StatEvent::TrackStarted,
            128000,
            64000,
            44100,
            1000,
            &mut buf,
        )
        .unwrap();

        assert_eq!(len, 28);
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], b"STMs");
    }
}
