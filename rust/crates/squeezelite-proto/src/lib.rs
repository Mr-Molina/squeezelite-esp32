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

/// Converts LMS SlimProto sample rate index into Hz.
pub const fn sample_rate_from_index(index: u8) -> u32 {
    match index {
        0 => 11025,
        1 => 22050,
        2 => 32000,
        3 => 44100,
        4 => 48000,
        5 => 8000,
        6 => 12000,
        7 => 16000,
        8 => 24000,
        9 => 96000,
        _ => 44100,
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
        let flags = payload[11];
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
        let digital_atten = payload[8];
        let preamp = payload[9];
        let gain_left = u32::from_be_bytes([payload[10], payload[11], payload[12], payload[13]]);
        let gain_right = u32::from_be_bytes([payload[14], payload[15], payload[16], payload[17]]);

        Ok(ServerCommand::AudioGain {
            gain_left,
            gain_right,
            digital_atten,
            preamp,
        })
    }

    fn parse_setd(payload: &[u8]) -> Result<ServerCommand<'_>, ParseError> {
        if payload.is_empty() {
            return Err(ParseError::InvalidFormat);
        }
        let sample_rate = if payload.len() >= 4 {
            u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
        } else {
            payload[0] as u32
        };
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
        // Fill UUID (16), WLAN channels (2), bytes received (8), lang (2) with zero
        out[16..44].fill(0);

        out[44..total_len].copy_from_slice(cap_bytes);

        Ok(total_len)
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
        const PAYLOAD_LEN: usize = 53;
        const TOTAL_LEN: usize = 8 + PAYLOAD_LEN;

        if out.len() < TOTAL_LEN {
            return Err(ParseError::PayloadTooLarge);
        }

        // Header
        out[0..4].copy_from_slice(b"STAT");
        out[4..8].copy_from_slice(&(PAYLOAD_LEN as u32).to_be_bytes());

        // Payload (53 bytes)
        out[8..12].copy_from_slice(&event.opcode_bytes());
        out[12] = 0; // num_crlf
        out[13] = 0; // mas_initialized
        out[14] = 0; // mas_mode
        out[15..19].copy_from_slice(&65536u32.to_be_bytes()); // stream_buffer_size
        out[19..23].copy_from_slice(&stream_buffer_fullness.to_be_bytes());
        out[23..27].copy_from_slice(&0u32.to_be_bytes()); // bytes_received_H
        out[27..31].copy_from_slice(&frames_played.to_be_bytes()); // bytes_received_L (frames counter)
        out[31..33].copy_from_slice(&0xFFFFu16.to_be_bytes()); // signal_strength
        out[33..37].copy_from_slice(&timestamp_ms.to_be_bytes()); // jiffies
        out[37..41].copy_from_slice(&65536u32.to_be_bytes()); // output_buffer_size
        out[41..45].copy_from_slice(&output_buffer_fullness.to_be_bytes());
        out[45..49].copy_from_slice(&(timestamp_ms / 1000).to_be_bytes()); // elapsed_seconds
        out[49..51].copy_from_slice(&0u16.to_be_bytes()); // voltage
        out[51..55].copy_from_slice(&timestamp_ms.to_be_bytes()); // elapsed_milliseconds
        out[55..59].copy_from_slice(&0u32.to_be_bytes()); // server_timestamp
        out[59..61].copy_from_slice(&0u16.to_be_bytes()); // error_code

        Ok(TOTAL_LEN)
    }
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
    /// Parses an LMS discovery response (must start with uppercase `b'E'`).
    /// Rejects client reflection queries starting with `b'e'`.
    ///
    /// Supports both standard LMS 1-byte TLV length-prefixed tags
    /// (`JSON`/`CLIP` + 1-byte len + `port_str`) and null-terminated fallback
    /// (`JSON`/`CLIP` + `port_str\0`).
    pub fn parse_response(buf: &[u8]) -> Option<DiscoveredServer> {
        if buf.is_empty() || buf[0] != b'E' {
            return None;
        }

        let mut json_port = 9000;
        let mut cli_port = 9090;
        let mut tag_found = false;

        let payload = &buf[1..];
        let mut i = 0;
        while i < payload.len() {
            let is_json = payload[i..].starts_with(b"JSON");
            let is_clip = payload[i..].starts_with(b"CLIP");

            if is_json || is_clip {
                let tag_end = i + 4;
                // Try standard LMS 1-byte TLV length-prefixed tag first
                let tlv_parsed = if tag_end < payload.len() {
                    let tlv_len = payload[tag_end] as usize;
                    if (1..=5).contains(&tlv_len) && tag_end + 1 + tlv_len <= payload.len() {
                        let val_bytes = &payload[tag_end + 1..tag_end + 1 + tlv_len];
                        if let Ok(s) = core::str::from_utf8(val_bytes) {
                            parse_u16(s.trim_end_matches('\0')).ok().map(|port| (port, tag_end + 1 + tlv_len))
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
                    if is_json {
                        json_port = port;
                    } else {
                        cli_port = port;
                    }
                    tag_found = true;
                    i = next_i;
                    continue;
                } else if let Some(end) = payload[tag_end..].iter().position(|&b| b == 0) {
                    // Fallback to null-terminated: Tag + port_str\0
                    if let Ok(s) = core::str::from_utf8(&payload[tag_end..tag_end + end]) {
                        if let Ok(port) = parse_u16(s) {
                            if is_json {
                                json_port = port;
                            } else {
                                cli_port = port;
                            }
                            tag_found = true;
                            i = tag_end + end + 1;
                            continue;
                        }
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
    }
}
