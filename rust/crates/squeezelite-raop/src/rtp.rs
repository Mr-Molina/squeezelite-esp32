//! RTP audio receiver, AES-128-CBC decryption, and ALAC frame decoding.
//!
//! Ingests UDP RTP audio packets, parses 12-byte headers, decrypts payloads,
//! decodes ALAC audio frames, and outputs 16-bit 44.1 kHz stereo PCM.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::crypto::Aes128Cbc;

/// Errors encountered while processing RTP audio packets.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RtpReceiverError {
    /// Packet size is under the 12-byte RTP header minimum.
    PacketTooShort,
    /// RTP version in header is not 2.
    InvalidRtpVersion,
    /// ALAC codec initialization failed from SDP format parameters.
    AlacInitError,
    /// ALAC frame decoding failed.
    AlacDecodeError,
    /// Output buffer is smaller than maximum decoded frame samples.
    BufferTooSmall,
}

impl fmt::Display for RtpReceiverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketTooShort => write!(f, "RTP packet too short (< 12 bytes)"),
            Self::InvalidRtpVersion => write!(f, "Invalid RTP version (expected v2)"),
            Self::AlacInitError => write!(f, "Failed to initialize ALAC decoder from SDP fmtp"),
            Self::AlacDecodeError => write!(f, "ALAC frame decode error"),
            Self::BufferTooSmall => write!(f, "PCM output buffer too small"),
        }
    }
}

/// Parsed 12-byte RTP audio packet header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RtpHeader {
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
}

impl RtpHeader {
    /// Parses an RTP header from a byte buffer.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8]), RtpReceiverError> {
        if buf.len() < 12 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        let version = buf[0] >> 6;
        if version != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let payload_type = buf[1] & 0x7F;
        let sequence_number = u16::from_be_bytes([buf[2], buf[3]]);
        let timestamp = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);

        let header = Self {
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
        };
        Ok((header, &buf[12..]))
    }
}

/// RTP Audio Receiver and Decryption Engine.
pub struct RtpAudioReceiver {
    aes_cipher: Option<Aes128Cbc>,
    aes_iv: [u8; 16],
    alac_decoder: Option<alac::Decoder>,
    pcm_out: Vec<i16>,
    decrypt_scratch: Vec<u8>,
    last_seq: Option<u16>,
    last_timestamp: Option<u32>,
    packets_received: u64,
}

impl Default for RtpAudioReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl RtpAudioReceiver {
    /// Creates a new RTP audio receiver.
    pub fn new() -> Self {
        Self {
            aes_cipher: None,
            aes_iv: [0u8; 16],
            alac_decoder: None,
            // Typical max AirPlay ALAC frame is 352 or 4096 samples stereo (8192 i16)
            pcm_out: vec![0i16; 8192],
            decrypt_scratch: Vec::with_capacity(2048),
            last_seq: None,
            last_timestamp: None,
            packets_received: 0,
        }
    }

    /// Configures the AES-128-CBC decryption session key and base IV.
    pub fn configure_crypto(&mut self, key: &[u8; 16], iv: &[u8; 16]) {
        self.aes_cipher = Some(Aes128Cbc::new(key));
        self.aes_iv = *iv;
    }

    /// Disables encryption (cleartext RTP streaming).
    pub fn disable_crypto(&mut self) {
        self.aes_cipher = None;
    }

    /// Configures the ALAC decoder using the SDP `fmtp` attribute.
    ///
    /// e.g. `96 352 0 16 40 10 14 2 255 0 0 44100` or
    /// `352 0 16 40 10 14 2 255 0 0 44100`
    pub fn configure_alac(&mut self, fmtp: &str) -> Result<(), RtpReceiverError> {
        let trimmed = fmtp.trim();
        // If fmtp begins with payload type token ("96 "), strip it
        let params = if let Some(stripped) = trimmed.strip_prefix("96 ") {
            stripped.trim()
        } else {
            trimmed
        };

        let stream_info = alac::StreamInfo::from_sdp_format_parameters(params)
            .map_err(|_| RtpReceiverError::AlacInitError)?;

        let max_samples = stream_info.max_samples_per_packet() as usize;
        if self.pcm_out.len() < max_samples {
            self.pcm_out.resize(max_samples, 0);
        }

        self.alac_decoder = Some(alac::Decoder::new(stream_info));
        Ok(())
    }

    /// Total packets successfully received.
    pub fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Last received sequence number.
    pub fn last_sequence_number(&self) -> Option<u16> {
        self.last_seq
    }

    /// Last received RTP timestamp.
    pub fn last_timestamp(&self) -> Option<u32> {
        self.last_timestamp
    }

    /// Ingests, decrypts, and decodes an RTP audio packet into stereo PCM.
    ///
    /// Returns a slice of decoded `i16` samples `[L, R, L, R, ...]`.
    pub fn process_packet<'a>(
        &'a mut self,
        packet: &[u8],
    ) -> Result<(&'a [i16], RtpHeader), RtpReceiverError> {
        let (header, payload) = RtpHeader::parse(packet)?;

        self.last_seq = Some(header.sequence_number);
        self.last_timestamp = Some(header.timestamp);
        self.packets_received += 1;

        // In-place decryption if encrypted
        let payload_to_decode = if let Some(ref cipher) = self.aes_cipher {
            self.decrypt_scratch.clear();
            self.decrypt_scratch.extend_from_slice(payload);
            cipher.decrypt_in_place(&self.aes_iv, &mut self.decrypt_scratch);
            &self.decrypt_scratch[..]
        } else {
            payload
        };

        let decoder = self.alac_decoder.as_mut().ok_or(RtpReceiverError::AlacInitError)?;
        let max_samples = decoder.stream_info().max_samples_per_packet() as usize;

        if self.pcm_out.len() < max_samples {
            self.pcm_out.resize(max_samples, 0);
        }

        let decoded_len = {
            let decoded_slice = decoder
                .decode_packet(payload_to_decode, &mut self.pcm_out)
                .map_err(|_| RtpReceiverError::AlacDecodeError)?;
            decoded_slice.len()
        };

        Ok((&self.pcm_out[..decoded_len], header))
    }
}
