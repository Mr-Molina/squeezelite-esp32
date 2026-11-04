//! Mercury request and response framing for Spotify Connect.
//!
//! Encodes and decodes Mercury headers, multipart packets (`0xB2`..`0xB5`),
//! and audio key packets (`0x0C`..`0x0E`) matching `MercurySession.cpp`.

extern crate alloc;

use crate::spirc::decode_varint;
use crate::spirc::encode_varint;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

/// Mercury message type opcodes matching `MercurySession.h`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MercuryType {
    /// Subscribe to Mercury URI topic (`0xB3`).
    Sub = 0xB3,
    /// Unsubscribe from Mercury URI topic (`0xB4`).
    Unsub = 0xB4,
    /// Subscription event response (`0xB5`).
    Subres = 0xB5,
    /// Send/Get request payload (`0xB2`).
    Send = 0xB2,
    /// Ping keepalive packet (`0x04`).
    Ping = 0x04,
    /// Pong acknowledgment packet (`0x4A`).
    PongAck = 0x4A,
    /// Audio chunk request (`0x08`).
    AudioChunkRequest = 0x08,
    /// Audio chunk success response (`0x09`).
    AudioChunkSuccess = 0x09,
    /// Audio chunk failure response (`0x0A`).
    AudioChunkFailure = 0x0A,
    /// Audio AES key request command (`0x0C`).
    AudioKeyRequest = 0x0C,
    /// Audio AES key success response (`0x0D`).
    AudioKeySuccess = 0x0D,
    /// Audio AES key failure response (`0x0E`).
    AudioKeyFailure = 0x0E,
    /// User country code response (`0x1B`).
    CountryCodeResponse = 0x1B,
    /// Unknown Mercury opcode (`0xFF`).
    Unknown = 0xFF,
}

#[allow(non_upper_case_globals)]
impl MercuryType {
    /// Alias for `Send` (`0xB2`, `MercuryReq`).
    pub const MercuryReq: Self = Self::Send;
    /// Alias for `Sub` (`0xB3`, `MercurySub`).
    pub const MercurySub: Self = Self::Sub;
    /// Alias for `Unsub` (`0xB4`, `MercuryUnsub`).
    pub const MercuryUnsub: Self = Self::Unsub;
    /// Alias for `Subres` (`0xB5`, `MercuryEvent`).
    pub const MercuryEvent: Self = Self::Subres;
    /// Alias for `AudioKeyRequest` (`0x0C`, `RequestKey`).
    pub const RequestKey: Self = Self::AudioKeyRequest;
    /// Alias for `AudioKeySuccess` (`0x0D`, `AesKey`).
    pub const AesKey: Self = Self::AudioKeySuccess;
    /// Alias for `AudioKeyFailure` (`0x0E`, `AesKeyError`).
    pub const AesKeyError: Self = Self::AudioKeyFailure;

    /// Maps an HTTP-like Mercury method string to its wire command opcode.
    #[must_use]
    pub fn from_method(method: &str) -> Self {
        match method {
            "SUB" => Self::Sub,
            "UNSUB" => Self::Unsub,
            "SEND" | "GET" => Self::Send,
            _ => Self::Send,
        }
    }
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
    /// Packet size or parts count exceeded bounds.
    PacketTooLarge,
}

impl fmt::Display for MercuryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedPacket => write!(f, "mercury packet truncated"),
            Self::InvalidFraming => write!(f, "invalid mercury packet framing"),
            Self::AudioKeyFailed(code) => {
                write!(f, "audio key request failed: code {code}")
            }
            Self::InvalidUtf8 => write!(f, "invalid utf-8 string in header"),
            Self::PacketTooLarge => write!(f, "mercury packet too large"),
        }
    }
}

/// Mercury Header protobuf payload (`mercury.proto`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MercuryHeader {
    /// Target Mercury URI (field 1).
    pub uri: String,
    /// Optional MIME content type (field 2).
    pub content_type: Option<String>,
    /// HTTP-like method, e.g. `"GET"`, `"SUB"`, `"UNSUB"`, `"SEND"` (field 3).
    pub method: String,
    /// Optional status code, e.g. `200` (field 4).
    pub status_code: Option<i32>,
}

impl MercuryHeader {
    /// Creates a new `MercuryHeader` with `uri` and `method`.
    #[must_use]
    pub fn new(uri: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            content_type: None,
            method: method.into(),
            status_code: None,
        }
    }

    /// Sets the optional `status_code` field.
    #[must_use]
    pub fn with_status_code(mut self, status_code: i32) -> Self {
        self.status_code = Some(status_code);
        self
    }

    /// Sets the optional `content_type` field.
    #[must_use]
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Encodes header into protobuf bytes (`mercury.proto`).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Field 1: uri (string, wire type 2)
        if !self.uri.is_empty() {
            buf.push((1 << 3) | 2);
            let bytes = self.uri.as_bytes();
            encode_varint(&mut buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }

        // Field 2: content_type (string, wire type 2)
        if let Some(ct) = &self.content_type {
            if !ct.is_empty() {
                buf.push((2 << 3) | 2);
                let bytes = ct.as_bytes();
                encode_varint(&mut buf, bytes.len() as u64);
                buf.extend_from_slice(bytes);
            }
        }

        // Field 3: method (string, wire type 2)
        if !self.method.is_empty() {
            buf.push((3 << 3) | 2);
            let bytes = self.method.as_bytes();
            encode_varint(&mut buf, bytes.len() as u64);
            buf.extend_from_slice(bytes);
        }

        // Field 4: status_code (int32, wire type 0)
        if let Some(code) = self.status_code {
            buf.push(4 << 3);
            encode_varint(&mut buf, code as u32 as u64);
        }

        buf
    }

    /// Decodes a `MercuryHeader` from protobuf bytes (`mercury.proto`).
    pub fn decode(data: &[u8]) -> Result<Self, MercuryError> {
        let mut pos = 0usize;
        let mut uri = String::new();
        let mut content_type = None;
        let mut method = String::new();
        let mut status_code = None;

        while pos < data.len() {
            let key = decode_varint(data, &mut pos)
                .map_err(|_| MercuryError::TruncatedPacket)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;

            match wire_type {
                0 => {
                    let val = decode_varint(data, &mut pos)
                        .map_err(|_| MercuryError::TruncatedPacket)?;
                    if field_num == 4 {
                        status_code = Some(val as i32);
                    }
                }
                1 => {
                    if pos + 8 > data.len() {
                        return Err(MercuryError::TruncatedPacket);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(data, &mut pos)
                        .map_err(|_| MercuryError::TruncatedPacket)?
                        as usize;
                    let end = pos
                        .checked_add(len)
                        .ok_or(MercuryError::TruncatedPacket)?;
                    if end > data.len() {
                        return Err(MercuryError::TruncatedPacket);
                    }
                    let slice = &data[pos..end];
                    pos = end;

                    match field_num {
                        1 => {
                            let s = core::str::from_utf8(slice)
                                .map_err(|_| MercuryError::InvalidUtf8)?;
                            uri = s.to_string();
                        }
                        2 => {
                            let s = core::str::from_utf8(slice)
                                .map_err(|_| MercuryError::InvalidUtf8)?;
                            content_type = Some(s.to_string());
                        }
                        3 => {
                            let s = core::str::from_utf8(slice)
                                .map_err(|_| MercuryError::InvalidUtf8)?;
                            method = s.to_string();
                        }
                        _ => {}
                    }
                }
                5 => {
                    if pos + 4 > data.len() {
                        return Err(MercuryError::TruncatedPacket);
                    }
                    pos += 4;
                }
                _ => return Err(MercuryError::InvalidFraming),
            }
        }

        Ok(Self {
            uri,
            content_type,
            method,
            status_code,
        })
    }
}

/// Multipart Mercury protocol packet (`0xB2`..`0xB5`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MercuryPacket {
    /// Mercury AP opcode (`0xB2` Req, `0xB3` Sub, `0xB4` Unsub, `0xB5` Event).
    pub cmd: MercuryType,
    /// Sequence identifier.
    pub seq_id: u64,
    /// Packet flags (`0x01` = final complete packet).
    pub flags: u8,
    /// Decoded Mercury protobuf header (part 0).
    pub header: MercuryHeader,
    /// Multipart payload frames following the header.
    pub payload_parts: Vec<Vec<u8>>,
}

/// Maximum allowed Mercury payload parts per packet.
pub const MAX_MERCURY_PARTS: usize = 64;

/// Maximum allowed total multipart payload size in bytes.
pub const MAX_MULTIPART_BYTES: usize = 65_536;

impl MercuryPacket {
    /// Creates a new complete `MercuryPacket` (`flags = 0x01`).
    #[must_use]
    pub fn new(
        cmd: MercuryType,
        seq_id: u64,
        header: MercuryHeader,
        payload_parts: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            cmd,
            seq_id,
            flags: 0x01,
            header,
            payload_parts,
        }
    }

    /// Constructs a request `MercuryPacket` from method
    /// (`"SUB"`, `"UNSUB"`, `"SEND"`, `"GET"`).
    #[must_use]
    pub fn request(
        seq_id: u64,
        method: &str,
        uri: &str,
        payload_parts: Vec<Vec<u8>>,
    ) -> Self {
        let cmd = MercuryType::from_method(method);
        let header = MercuryHeader::new(uri, method);
        Self::new(cmd, seq_id, header, payload_parts)
    }

    /// Serializes the Mercury packet body into wire bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let header_bytes = self.header.encode();
        let mut out = Vec::with_capacity(32 + header_bytes.len());

        let seq_bytes = self.seq_id.to_be_bytes();
        out.extend_from_slice(&(seq_bytes.len() as u16).to_be_bytes());
        out.extend_from_slice(&seq_bytes);
        out.push(self.flags);

        let part_count = (1 + self.payload_parts.len()) as u16;
        out.extend_from_slice(&part_count.to_be_bytes());

        out.extend_from_slice(&(header_bytes.len() as u16).to_be_bytes());
        out.extend_from_slice(&header_bytes);

        for part in &self.payload_parts {
            out.extend_from_slice(&(part.len() as u16).to_be_bytes());
            out.extend_from_slice(part);
        }

        out
    }

    /// Serializes as `(cmd_byte, body_bytes)` for AP Shannon transmission.
    #[must_use]
    pub fn encode_ap_frame(&self) -> (u8, Vec<u8>) {
        (self.cmd as u8, self.encode())
    }

    fn parse_raw_parts(
        data: &[u8],
    ) -> Result<(u64, u8, Vec<Vec<u8>>), MercuryError> {
        if data.len() < 2 {
            return Err(MercuryError::TruncatedPacket);
        }
        if data.len() > MAX_MULTIPART_BYTES {
            return Err(MercuryError::PacketTooLarge);
        }
        let seq_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        if seq_len == 0 || seq_len > 8 || 2 + seq_len + 3 > data.len() {
            return Err(MercuryError::InvalidFraming);
        }

        let mut seq_id = 0u64;
        for &b in &data[2..2 + seq_len] {
            seq_id = (seq_id << 8) | (b as u64);
        }

        let mut pos = 2 + seq_len;
        let flags = data[pos];
        pos += 1;

        let part_count = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if part_count == 0 {
            return Err(MercuryError::InvalidFraming);
        }
        if part_count > MAX_MERCURY_PARTS {
            return Err(MercuryError::PacketTooLarge);
        }
        if pos + part_count.saturating_mul(2) > data.len() {
            return Err(MercuryError::TruncatedPacket);
        }

        let mut parts = Vec::with_capacity(part_count.min(64));
        for _ in 0..part_count {
            if pos + 2 > data.len() {
                return Err(MercuryError::TruncatedPacket);
            }
            let part_len =
                u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;
            if pos + part_len > data.len() {
                return Err(MercuryError::TruncatedPacket);
            }
            parts.push(data[pos..pos + part_len].to_vec());
            pos += part_len;
        }

        Ok((seq_id, flags, parts))
    }

    /// Decodes a single Mercury wire packet (`0xB2`..`0xB5`).
    pub fn decode(cmd: u8, data: &[u8]) -> Result<Self, MercuryError> {
        let typ = MercuryType::from(cmd);
        if !matches!(
            typ,
            MercuryType::Send
                | MercuryType::Sub
                | MercuryType::Unsub
                | MercuryType::Subres
        ) {
            return Err(MercuryError::InvalidFraming);
        }

        let (seq_id, flags, mut parts) = Self::parse_raw_parts(data)?;
        let header_bytes = parts.remove(0);
        let header = MercuryHeader::decode(&header_bytes)?;

        Ok(Self {
            cmd: typ,
            seq_id,
            flags,
            header,
            payload_parts: parts,
        })
    }

    /// Reassembles a multipart Mercury packet across one or more AP fragments.
    pub fn assemble_multipart(
        cmd: u8,
        fragments: &[&[u8]],
    ) -> Result<Self, MercuryError> {
        if fragments.is_empty() {
            return Err(MercuryError::TruncatedPacket);
        }
        let typ = MercuryType::from(cmd);
        if !matches!(
            typ,
            MercuryType::Send
                | MercuryType::Sub
                | MercuryType::Unsub
                | MercuryType::Subres
        ) {
            return Err(MercuryError::InvalidFraming);
        }

        let mut expected_seq = None;
        let mut final_flags = 0u8;
        let mut all_parts: Vec<Vec<u8>> = Vec::new();
        let mut total_bytes = 0usize;

        for &frag in fragments {
            total_bytes = total_bytes.saturating_add(frag.len());
            if total_bytes > MAX_MULTIPART_BYTES {
                return Err(MercuryError::PacketTooLarge);
            }
            let (seq_id, flags, parts) = Self::parse_raw_parts(frag)?;
            if let Some(prev_seq) = expected_seq {
                if prev_seq != seq_id {
                    return Err(MercuryError::InvalidFraming);
                }
            } else {
                expected_seq = Some(seq_id);
            }
            final_flags = flags;
            all_parts.extend(parts);
            if all_parts.len() > MAX_MERCURY_PARTS {
                return Err(MercuryError::PacketTooLarge);
            }
        }

        if all_parts.is_empty() {
            return Err(MercuryError::TruncatedPacket);
        }

        let header_bytes = all_parts.remove(0);
        let header = MercuryHeader::decode(&header_bytes)?;
        let seq_id = expected_seq.unwrap_or(0);

        Ok(Self {
            cmd: typ,
            seq_id,
            flags: final_flags,
            header,
            payload_parts: all_parts,
        })
    }
}

/// AudioKey AP protocol packets (`0x0C` `RequestKey`, `0x0D` `AesKey`,
/// `0x0E` `AesKeyError`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioKeyPacket {
    /// `0x0C` `RequestKey`: 20-byte `file_id` + 16-byte `track_gid` + 4-byte `seq`.
    RequestKey {
        /// 20-byte Spotify audio file ID.
        file_id: [u8; 20],
        /// 16-byte Spotify track GID.
        track_gid: [u8; 16],
        /// 4-byte sequence number.
        seq: u32,
    },
    /// `0x0D` `AesKey`: 4-byte `seq` + 16-byte AES-128 audio key response.
    AesKey {
        /// 4-byte sequence number.
        seq: u32,
        /// 16-byte AES-128-CTR audio key.
        key: [u8; 16],
    },
    /// `0x0E` `AesKeyError`: 4-byte `seq` + 2-byte error code response.
    AesKeyError {
        /// 4-byte sequence number.
        seq: u32,
        /// 2-byte error code.
        error_code: u16,
    },
}

impl AudioKeyPacket {
    /// Returns the AP command byte (`0x0C`, `0x0D`, or `0x0E`).
    #[must_use]
    pub const fn cmd(&self) -> u8 {
        match self {
            Self::RequestKey { .. } => MercuryType::AudioKeyRequest as u8,
            Self::AesKey { .. } => MercuryType::AudioKeySuccess as u8,
            Self::AesKeyError { .. } => MercuryType::AudioKeyFailure as u8,
        }
    }

    /// Encodes the packet payload bytes.
    #[must_use]
    pub fn encode_payload(&self) -> Vec<u8> {
        match self {
            Self::RequestKey {
                file_id,
                track_gid,
                seq,
            } => MercuryPacketBuilder::build_audio_key_request(
                file_id, track_gid, *seq,
            ),
            Self::AesKey { seq, key } => {
                let mut buf = Vec::with_capacity(20);
                buf.extend_from_slice(&seq.to_be_bytes());
                buf.extend_from_slice(key);
                buf
            }
            Self::AesKeyError { seq, error_code } => {
                let mut buf = Vec::with_capacity(6);
                buf.extend_from_slice(&seq.to_be_bytes());
                buf.extend_from_slice(&error_code.to_be_bytes());
                buf
            }
        }
    }

    /// Encodes as `(cmd, payload)` tuple for AP transport.
    #[must_use]
    pub fn encode(&self) -> (u8, Vec<u8>) {
        (self.cmd(), self.encode_payload())
    }

    /// Decodes an `AudioKeyPacket` from AP command (`0x0C`, `0x0D`, `0x0E`)
    /// and payload.
    pub fn decode(cmd: u8, payload: &[u8]) -> Result<Self, MercuryError> {
        match MercuryType::from(cmd) {
            MercuryType::AudioKeyRequest => {
                if payload.len() < 40 {
                    return Err(MercuryError::TruncatedPacket);
                }
                let mut file_id = [0u8; 20];
                file_id.copy_from_slice(&payload[0..20]);
                let mut track_gid = [0u8; 16];
                track_gid.copy_from_slice(&payload[20..36]);
                let seq = u32::from_be_bytes([
                    payload[36],
                    payload[37],
                    payload[38],
                    payload[39],
                ]);
                Ok(Self::RequestKey {
                    file_id,
                    track_gid,
                    seq,
                })
            }
            MercuryType::AudioKeySuccess => {
                if payload.len() < 20 {
                    return Err(MercuryError::TruncatedPacket);
                }
                let seq = u32::from_be_bytes([
                    payload[0], payload[1], payload[2], payload[3],
                ]);
                let mut key = [0u8; 16];
                key.copy_from_slice(&payload[4..20]);
                Ok(Self::AesKey { seq, key })
            }
            MercuryType::AudioKeyFailure => {
                if payload.len() < 6 {
                    return Err(MercuryError::TruncatedPacket);
                }
                let seq = u32::from_be_bytes([
                    payload[0], payload[1], payload[2], payload[3],
                ]);
                let error_code = u16::from_be_bytes([payload[4], payload[5]]);
                Ok(Self::AesKeyError { seq, error_code })
            }
            _ => Err(MercuryError::InvalidFraming),
        }
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
        let header = MercuryHeader::new(uri, method);
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
        match AudioKeyPacket::decode(cmd, payload)? {
            AudioKeyPacket::AesKey { seq, key } => Ok((seq, key)),
            AudioKeyPacket::AesKeyError { error_code, .. } => {
                Err(MercuryError::AudioKeyFailed(error_code))
            }
            AudioKeyPacket::RequestKey { .. } => {
                Err(MercuryError::InvalidFraming)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mercury_packet_too_large_parts() {
        // Construct raw data with part_count = 65 (> MAX_MERCURY_PARTS = 64)
        let mut data = Vec::new();
        // seq_len = 2
        data.extend_from_slice(&[0x00, 0x02]);
        // seq_id = 0x0102
        data.extend_from_slice(&[0x01, 0x02]);
        // flags = 1
        data.push(0x01);
        // part_count = 65
        data.extend_from_slice(&65u16.to_be_bytes());

        let res = MercuryPacket::parse_raw_parts(&data);
        assert_eq!(res.err(), Some(MercuryError::PacketTooLarge));
    }
}
