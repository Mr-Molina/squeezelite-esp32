//! Mercury request and response framing for Spotify Connect.
//!
//! Encodes Mercury subscriptions, metadata requests, and audio key requests
//! matching `MercurySession.cpp`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Mercury message type opcodes matching `MercurySession.h`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MercuryType {
    /// Subscribe to Mercury URI topic.
    Sub = 0xB3,
    /// Unsubscribe from Mercury URI topic.
    Unsub = 0xB4,
    /// Subscription response.
    Subres = 0xB5,
    /// Send request payload.
    Send = 0xB2,
    /// Ping keepalive packet.
    Ping = 0x04,
    /// Pong acknowledgment packet.
    PongAck = 0x4A,
    /// Audio chunk request.
    AudioChunkRequest = 0x08,
    /// Audio chunk success response.
    AudioChunkSuccess = 0x09,
    /// Audio chunk failure response.
    AudioChunkFailure = 0x0A,
    /// Audio AES key request command.
    AudioKeyRequest = 0x0C,
    /// Audio AES key success response.
    AudioKeySuccess = 0x0D,
    /// Audio AES key failure response.
    AudioKeyFailure = 0x0E,
    /// User country code response.
    CountryCodeResponse = 0x1B,
    /// Unknown Mercury opcode.
    Unknown = 0xFF,
}

impl From<u8> for MercuryType {
    fn from(val: u8) -> Self {
        match val {
            0xB3 => Self::Sub,
            0xB4 => Self::Unsub,
            0xB5 => Self::Subres,
            0xB2 => Self::Send,
            0x04 => Self::Ping,
            0x4A => Self::PongAck,
            0x08 => Self::AudioChunkRequest,
            0x09 => Self::AudioChunkSuccess,
            0x0A => Self::AudioChunkFailure,
            0x0C => Self::AudioKeyRequest,
            0x0D => Self::AudioKeySuccess,
            0x0E => Self::AudioKeyFailure,
            0x1B => Self::CountryCodeResponse,
            _ => Self::Unknown,
        }
    }
}

/// Mercury framing or parsing error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MercuryError {
    /// Packet payload shorter than header fields.
    TruncatedPacket,
    /// Invalid sequence length or payload part count.
    InvalidFraming,
    /// Server returned audio key fetch failure.
    AudioKeyFailed(u16),
    /// UTF-8 decode error.
    InvalidUtf8,
}

impl fmt::Display for MercuryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedPacket => write!(f, "mercury packet truncated"),
            Self::InvalidFraming => write!(f, "invalid mercury packet framing"),
            Self::AudioKeyFailed(code) => write!(f, "audio key request failed: code {code}"),
            Self::InvalidUtf8 => write!(f, "invalid utf-8 string in header"),
        }
    }
}

/// Mercury Header protobuf payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MercuryHeader {
    /// Target Mercury URI.
    pub uri: String,
    /// HTTP-like method (e.g. "GET", "SUB", "SEND").
    pub method: String,
}

impl MercuryHeader {
    /// Encodes header into minimal protobuf bytes (`mercury.proto`).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Field 1: uri (string, wire type 2)
        if !self.uri.is_empty() {
            buf.push((1 << 3) | 2);
            let bytes = self.uri.as_bytes();
            crate::spirc::encode_varint(&mut buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }

        // Field 3: method (string, wire type 2)
        if !self.method.is_empty() {
            buf.push((3 << 3) | 2);
            let bytes = self.method.as_bytes();
            crate::spirc::encode_varint(&mut buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }

        buf
    }
}

/// Builder for Mercury wire packets.
pub struct MercuryPacketBuilder;

impl MercuryPacketBuilder {
    /// Frames a Mercury request packet.
    /// Matches `MercurySession::executeSubscription`.
    pub fn build_request(
        seq_id: u64,
        method: &str,
        uri: &str,
        payloads: &[&[u8]],
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);

        // Sequence ID: 8 bytes big-endian, preceded by 2-byte size (8)
        let seq_bytes = seq_id.to_be_bytes();
        let seq_size = (seq_bytes.len() as u16).to_be_bytes();
        out.extend_from_slice(&seq_size);
        out.extend_from_slice(&seq_bytes);

        // Flags: 0x01
        out.push(0x01);

        // Payloads count: header counts as 1 + number of payloads
        let payload_num = ((payloads.len() + 1) as u16).to_be_bytes();
        out.extend_from_slice(&payload_num);

        // Header part
        let header = MercuryHeader {
            uri: String::from(uri),
            method: String::from(method),
        };
        let header_bytes = header.encode();
        let header_len = (header_bytes.len() as u16).to_be_bytes();
        out.extend_from_slice(&header_len);
        out.extend_from_slice(&header_bytes);

        // Subsequent payload parts
        for part in payloads {
            let part_len = (part.len() as u16).to_be_bytes();
            out.extend_from_slice(&part_len);
            out.extend_from_slice(part);
        }

        out
    }

    /// Formats an Audio Key Request payload (command 0x0C).
    pub fn build_audio_key_request(
        file_id: &[u8; 20],
        track_id: &[u8; 16],
        seq_id: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(42);
        buf.extend_from_slice(file_id);
        buf.extend_from_slice(track_id);
        buf.extend_from_slice(&seq_id.to_be_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        buf
    }

    /// Parses an Audio Key Response (command 0x0D or 0x0E).
    pub fn parse_audio_key_response(
        cmd: u8,
        payload: &[u8],
    ) -> Result<(u32, [u8; 16]), MercuryError> {
        let typ = MercuryType::from(cmd);
        match typ {
            MercuryType::AudioKeySuccess => {
                if payload.len() < 20 {
                    return Err(MercuryError::TruncatedPacket);
                }
                let seq_id = u32::from_be_bytes([
                    payload[0], payload[1], payload[2], payload[3],
                ]);
                let mut aes_key = [0u8; 16];
                aes_key.copy_from_slice(&payload[4..20]);
                Ok((seq_id, aes_key))
            }
            MercuryType::AudioKeyFailure => {
                if payload.len() < 6 {
                    return Err(MercuryError::TruncatedPacket);
                }
                let error_code = u16::from_be_bytes([payload[4], payload[5]]);
                Err(MercuryError::AudioKeyFailed(error_code))
            }
            _ => Err(MercuryError::InvalidFraming),
        }
    }
}
