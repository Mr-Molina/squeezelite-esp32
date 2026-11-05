//! `squeezelite-decoders`
//!
//! Pure-Rust audio decoding pipeline for squeezelite-esp32:
//! - Defines unified `AudioDecoder` trait
//! - Zero-copy `PcmDecoder` with mono channel duplication and multi-depth PCM
//! - Heap-isolated `Mp3Decoder` using nanomp3-core (zero task stack explosion)
//! - Stateful streaming `FlacDecoder` using claxon
//! - ADTS/AAC decoder using syom with exact byte consumption
//! - Ogg/Vorbis decoder using lewton with header/audio packet differentiation

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

#[cfg(all(feature = "alloc", not(feature = "std")))]
extern crate alloc;

#[cfg(feature = "std")]
use std::boxed::Box;
#[cfg(feature = "std")]
use std::collections::VecDeque;
#[cfg(feature = "std")]
use std::vec;
#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::boxed::Box;
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::collections::VecDeque;
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::vec;
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use core::fmt;

/// Maximum stream buffer size for compressed audio decoders (FLAC, Vorbis)
/// (PERF-201).
pub const MAX_STREAM_BUF: usize = 65536;

/// Maximum queued audio packets in Vorbis and Opus decoders (AUD12-PERF-001).
pub const MAX_PACKET_QUEUE: usize = 64;

/// Maximum block samples supported by FLAC decoder (RFC 9639 standard CD-DA).
pub const MAX_FLAC_BLOCK_SAMPLES: usize = 4608;

/// Maximum MP4/M4A sample table entries permitted in memory.
pub const MAX_MP4_SAMPLES: usize = 65536;

/// Maximum stream buffer size for MP4/M4A metadata atoms prior to
/// entering mdat (PERF-301).
pub const MAX_MP4_META_BUF: usize = 270_336;

/// Minimum supported audio sample rate in Hz.
pub const MIN_SAMPLE_RATE: u32 = 8000;

/// Maximum supported audio sample rate in Hz.
pub const MAX_SAMPLE_RATE: u32 = 192_000;

/// Unified Audio Decoder Error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum DecoderError {
    #[cfg_attr(feature = "std", error("Input buffer needs more encoded bytes"))]
    NeedMoreData,
    #[cfg_attr(feature = "std", error("Audio stream header is invalid"))]
    InvalidHeader,
    #[cfg_attr(feature = "std", error("Corrupted audio frame detected"))]
    CorruptedFrame,
    #[cfg_attr(feature = "std", error("Unsupported sample depth or channels"))]
    UnsupportedSampleFormat,
    #[cfg_attr(
        feature = "std",
        error("Target PCM buffer too small for decoded frame")
    )]
    BufferOverflow,
}

/// Alias for [`DecoderError`] used by container and codec consumers.
pub type DecodeError = DecoderError;

impl DecoderError {
    /// Stream uses an unsupported codec profile, channel count, sample rate,
    /// or exceeds the decoder memory ceiling (triggers `STMn`).
    #[allow(non_upper_case_globals)]
    pub const UnsupportedFeature: Self = Self::UnsupportedSampleFormat;
}

#[cfg(not(feature = "std"))]
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedMoreData => write!(f, "Input buffer needs more encoded bytes"),
            Self::InvalidHeader => write!(f, "Audio stream header is invalid"),
            Self::CorruptedFrame => write!(f, "Corrupted audio frame detected"),
            Self::UnsupportedSampleFormat => write!(f, "Unsupported sample depth or channels"),
            Self::BufferOverflow => write!(f, "Target PCM buffer too small for decoded frame"),
        }
    }
}

/// Core trait implemented by all audio format decoders.
pub trait AudioDecoder {
    /// Decodes encoded audio bytes from `encoded_in` and outputs
    /// interleaved 32-bit PCM samples to `pcm_out`.
    ///
    /// Returns `Ok((bytes_consumed, frames_produced))`.
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError>;

    /// Flushes any internal state or history buffers.
    fn flush(&mut self);
}

/// Parse AIFF 80-bit IEEE 754 extended precision floating point sample rate.
const fn parse_aiff_extended_float(bytes: &[u8; 10]) -> u32 {
    let exponent = ((u16::from_be_bytes([bytes[0], bytes[1]]) & 0x7FFF) as i32) - 16383;
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    if exponent >= 0 && exponent <= 31 {
        (mantissa >> (63 - exponent)) as u32
    } else {
        0
    }
}

/// PCM Passthrough Decoder (16-bit / 24-bit / 32-bit, Big or Little Endian,
/// Mono or Stereo).
pub struct PcmDecoder {
    pub bits_per_sample: u8,
    pub channels: u8,
    pub big_endian: bool,
    pub sample_rate: Option<u32>,
    header_stripped: bool,
}

impl PcmDecoder {
    pub const fn new(bits_per_sample: u8, channels: u8, big_endian: bool) -> Self {
        Self {
            bits_per_sample,
            channels,
            big_endian,
            sample_rate: None,
            header_stripped: false,
        }
    }

    /// Automatically detects and strips WAV (RIFF...WAVE) and AIFF
    /// (FORM...AIFF) container headers.
    pub fn try_strip_container_header(&mut self, data: &[u8]) -> Result<usize, DecoderError> {
        if data.len() < 4 {
            if b"RIFF".starts_with(data) || b"FORM".starts_with(data) {
                return Err(DecoderError::NeedMoreData);
            }
            return Ok(0);
        }

        if data.starts_with(b"RIFF") {
            if data.len() < 12 {
                return Err(DecoderError::NeedMoreData);
            }
            if &data[8..12] != b"WAVE" {
                return Err(DecoderError::InvalidHeader);
            }

            let mut offset = 12;
            let mut found_fmt = false;

            while offset + 8 <= data.len() {
                let chunk_id = &data[offset..offset + 4];
                let chunk_size = u32::from_le_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]) as usize;
                let padded_size = chunk_size
                    .checked_add(chunk_size & 1)
                    .ok_or(DecoderError::InvalidHeader)?;

                if chunk_id == b"fmt " {
                    if chunk_size < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if offset.checked_add(24).is_none_or(|end| end > data.len()) {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let fmt_payload = &data[offset + 8..];
                    let format_tag = u16::from_le_bytes([fmt_payload[0], fmt_payload[1]]);
                    if format_tag != 1 {
                        return Err(DecoderError::UnsupportedSampleFormat);
                    }
                    let num_channels = u16::from_le_bytes([fmt_payload[2], fmt_payload[3]]);
                    if num_channels != 1 && num_channels != 2 {
                        return Err(DecoderError::UnsupportedSampleFormat);
                    }
                    let sample_rate = u32::from_le_bytes([
                        fmt_payload[4],
                        fmt_payload[5],
                        fmt_payload[6],
                        fmt_payload[7],
                    ]);
                    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    let bits_per_sample = u16::from_le_bytes([fmt_payload[14], fmt_payload[15]]);

                    self.channels = num_channels as u8;
                    self.sample_rate = Some(sample_rate);
                    self.bits_per_sample = bits_per_sample as u8;
                    self.big_endian = false;
                    found_fmt = true;

                    offset = offset
                        .checked_add(8)
                        .and_then(|o| o.checked_add(padded_size))
                        .ok_or(DecoderError::InvalidHeader)?;
                } else if chunk_id == b"data" {
                    if !found_fmt {
                        return Err(DecoderError::InvalidHeader);
                    }
                    let data_start = offset.checked_add(8).ok_or(DecoderError::InvalidHeader)?;
                    return Ok(data_start);
                } else {
                    let next_offset = offset
                        .checked_add(8)
                        .and_then(|o| o.checked_add(padded_size))
                        .ok_or(DecoderError::InvalidHeader)?;
                    if next_offset > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    offset = next_offset;
                }
            }
            return Err(DecoderError::NeedMoreData);
        } else if data.starts_with(b"FORM") {
            if data.len() < 12 {
                return Err(DecoderError::NeedMoreData);
            }
            if &data[8..12] != b"AIFF" && &data[8..12] != b"AIFC" {
                return Err(DecoderError::InvalidHeader);
            }

            let mut offset = 12;
            let mut found_comm = false;

            while offset + 8 <= data.len() {
                let chunk_id = &data[offset..offset + 4];
                let chunk_size = u32::from_be_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]) as usize;
                let padded_size = chunk_size
                    .checked_add(chunk_size & 1)
                    .ok_or(DecoderError::InvalidHeader)?;

                if chunk_id == b"COMM" {
                    if chunk_size < 18 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if offset.checked_add(26).is_none_or(|end| end > data.len()) {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let comm_payload = &data[offset + 8..];
                    let num_channels = i16::from_be_bytes([comm_payload[0], comm_payload[1]]);
                    if num_channels != 1 && num_channels != 2 {
                        return Err(DecoderError::UnsupportedSampleFormat);
                    }
                    let sample_size = i16::from_be_bytes([comm_payload[6], comm_payload[7]]);

                    let mut ext = [0u8; 10];
                    ext.copy_from_slice(&comm_payload[8..18]);
                    let sample_rate = parse_aiff_extended_float(&ext);
                    if sample_rate > 0
                        && !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate)
                    {
                        return Err(DecoderError::UnsupportedFeature);
                    }

                    self.channels = num_channels as u8;
                    if sample_size > 0 {
                        self.bits_per_sample = sample_size as u8;
                    }
                    if sample_rate > 0 {
                        self.sample_rate = Some(sample_rate);
                    }
                    self.big_endian = true;
                    found_comm = true;

                    offset = offset
                        .checked_add(8)
                        .and_then(|o| o.checked_add(padded_size))
                        .ok_or(DecoderError::InvalidHeader)?;
                } else if chunk_id == b"SSND" {
                    if !found_comm {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if offset.checked_add(16).is_none_or(|end| end > data.len()) {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let ssnd_payload = &data[offset + 8..];
                    let ssnd_offset = u32::from_be_bytes([
                        ssnd_payload[0],
                        ssnd_payload[1],
                        ssnd_payload[2],
                        ssnd_payload[3],
                    ]) as usize;

                    let audio_start = offset
                        .checked_add(16)
                        .and_then(|o| o.checked_add(ssnd_offset))
                        .ok_or(DecoderError::InvalidHeader)?;
                    if audio_start > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    return Ok(audio_start);
                } else {
                    let next_offset = offset
                        .checked_add(8)
                        .and_then(|o| o.checked_add(padded_size))
                        .ok_or(DecoderError::InvalidHeader)?;
                    if next_offset > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    offset = next_offset;
                }
            }
            return Err(DecoderError::NeedMoreData);
        }

        // Raw PCM without container header
        Ok(0)
    }
}

impl AudioDecoder for PcmDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let mut header_len = 0;
        if !self.header_stripped {
            header_len = self.try_strip_container_header(encoded_in)?;
            self.header_stripped = true;
        }

        let pcm_in = &encoded_in[header_len..];

        let bytes_per_sample = match self.bits_per_sample {
            8 => 1,
            16 => 2,
            24 => 3,
            32 => 4,
            _ => return Err(DecoderError::UnsupportedSampleFormat),
        };

        if self.channels != 1 && self.channels != 2 {
            return Err(DecoderError::UnsupportedSampleFormat);
        }
        if let Some(sr) = self.sample_rate {
            if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sr) {
                return Err(DecoderError::UnsupportedFeature);
            }
        }

        let bytes_per_frame = bytes_per_sample * self.channels as usize;
        let max_output_frames = pcm_out.len() / 2; // Always stereo interleaved output
        if max_output_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        let available_frames = pcm_in.len() / bytes_per_frame;
        if available_frames == 0 {
            if header_len > 0 {
                return Ok((header_len, 0));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let frames_to_decode = core::cmp::min(max_output_frames, available_frames);

        match (self.bits_per_sample, self.big_endian, self.channels) {
            // 8-bit Stereo (unsigned PCM centered at 128 for WAV, signed for AIFF)
            (8, true, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 2;
                    let l = (pcm_in[off] as i8 as i32) << 24;
                    let r = (pcm_in[off + 1] as i8 as i32) << 24;
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
                }
            }
            (8, false, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 2;
                    let l = ((pcm_in[off] as i32) - 128) << 24;
                    let r = ((pcm_in[off + 1] as i32) - 128) << 24;
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
                }
            }
            // 8-bit Mono (unsigned PCM centered at 128 for WAV, signed for AIFF)
            (8, true, 1) => {
                for i in 0..frames_to_decode {
                    let off = i;
                    let s = (pcm_in[off] as i8 as i32) << 24;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            (8, false, 1) => {
                for i in 0..frames_to_decode {
                    let off = i;
                    let s = ((pcm_in[off] as i32) - 128) << 24;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 16-bit Stereo Big-Endian (Common Squeezebox PCM format)
            (16, true, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(4)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l = i16::from_be_bytes([src[0], src[1]]) as i32;
                    let r = i16::from_be_bytes([src[2], src[3]]) as i32;
                    dst[0] = l << 16;
                    dst[1] = r << 16;
                }
            }
            // 16-bit Mono Big-Endian (DEC-010: Mono duplication)
            (16, true, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 2;
                    let s = (i16::from_be_bytes([pcm_in[off], pcm_in[off + 1]]) as i32) << 16;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 16-bit Stereo Little-Endian
            (16, false, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(4)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l = i16::from_le_bytes([src[0], src[1]]) as i32;
                    let r = i16::from_le_bytes([src[2], src[3]]) as i32;
                    dst[0] = l << 16;
                    dst[1] = r << 16;
                }
            }
            // 16-bit Mono Little-Endian (DEC-010)
            (16, false, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 2;
                    let s = (i16::from_le_bytes([pcm_in[off], pcm_in[off + 1]]) as i32) << 16;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 24-bit Stereo Big-Endian
            (24, true, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(6)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l =
                        ((src[0] as i32) << 24) | ((src[1] as i32) << 16) | ((src[2] as i32) << 8);
                    let r =
                        ((src[3] as i32) << 24) | ((src[4] as i32) << 16) | ((src[5] as i32) << 8);
                    dst[0] = l;
                    dst[1] = r;
                }
            }
            // 24-bit Mono Big-Endian (DEC-010)
            (24, true, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 3;
                    let s = ((pcm_in[off] as i32) << 24)
                        | ((pcm_in[off + 1] as i32) << 16)
                        | ((pcm_in[off + 2] as i32) << 8);
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 24-bit Stereo Little-Endian
            (24, false, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(6)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l =
                        ((src[0] as i32) << 8) | ((src[1] as i32) << 16) | ((src[2] as i32) << 24);
                    let r =
                        ((src[3] as i32) << 8) | ((src[4] as i32) << 16) | ((src[5] as i32) << 24);
                    dst[0] = l;
                    dst[1] = r;
                }
            }
            // 24-bit Mono Little-Endian (DEC-010)
            (24, false, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 3;
                    let s = ((pcm_in[off] as i32) << 8)
                        | ((pcm_in[off + 1] as i32) << 16)
                        | ((pcm_in[off + 2] as i32) << 24);
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 32-bit Stereo Big-Endian
            (32, true, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(8)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l = i32::from_be_bytes([src[0], src[1], src[2], src[3]]);
                    let r = i32::from_be_bytes([src[4], src[5], src[6], src[7]]);
                    dst[0] = l;
                    dst[1] = r;
                }
            }
            // 32-bit Mono Big-Endian (DEC-010)
            (32, true, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let s = i32::from_be_bytes([
                        pcm_in[off],
                        pcm_in[off + 1],
                        pcm_in[off + 2],
                        pcm_in[off + 3],
                    ]);
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 32-bit Stereo Little-Endian
            (32, false, 2) => {
                for (src, dst) in pcm_in
                    .chunks_exact(8)
                    .zip(pcm_out.chunks_exact_mut(2))
                    .take(frames_to_decode)
                {
                    let l = i32::from_le_bytes([src[0], src[1], src[2], src[3]]);
                    let r = i32::from_le_bytes([src[4], src[5], src[6], src[7]]);
                    dst[0] = l;
                    dst[1] = r;
                }
            }
            // 32-bit Mono Little-Endian (DEC-010)
            (32, false, 1) => {
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let s = i32::from_le_bytes([
                        pcm_in[off],
                        pcm_in[off + 1],
                        pcm_in[off + 2],
                        pcm_in[off + 3],
                    ]);
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            _ => return Err(DecoderError::UnsupportedSampleFormat),
        }

        Ok((
            header_len + frames_to_decode * bytes_per_frame,
            frames_to_decode,
        ))
    }

    fn flush(&mut self) {
        self.header_stripped = false;
        self.sample_rate = None;
    }
}

/// Parsed MP3 Xing/Info VBR and LAME gapless header metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XingLameInfo {
    /// Total number of audio MPEG frames in the stream (excluding the Xing
    /// header frame), if present in the Xing/Info flags (`0x01`).
    pub total_frames: Option<u32>,
    /// Total stream byte length, if present in the Xing/Info flags (`0x02`).
    pub total_bytes: Option<u32>,
    /// Encoder delay in stereo PCM frames from the 24-bit LAME tag.
    pub enc_delay: u16,
    /// Encoder trailing padding in stereo PCM frames from the 24-bit LAME tag.
    pub enc_padding: u16,
    /// Number of PCM frames per MPEG frame (`1152` for MPEG-1, `576` for
    /// MPEG-2/2.5).
    pub samples_per_frame: u16,
}

/// Parses an MPEG Layer III frame for a `"Xing"` or `"Info"` VBR/CBR header
/// and an optional LAME tag containing 24-bit `enc_delay` and `enc_padding`.
#[must_use]
pub fn parse_xing_lame_header(frame: &[u8]) -> Option<XingLameInfo> {
    if frame.len() < 24 || frame[0] != 0xFF || (frame[1] & 0xE0) != 0xE0 {
        return None;
    }
    let version_id = (frame[1] >> 3) & 0x03;
    if version_id == 1 {
        return None;
    }
    let layer = (frame[1] >> 1) & 0x03;
    if layer != 1 {
        return None;
    }
    let has_crc = (frame[1] & 0x01) == 0;
    let channel_mode = (frame[3] >> 6) & 0x03;
    let is_mono = channel_mode == 3;
    let side_info_len = if version_id == 3 {
        if is_mono {
            17
        } else {
            32
        }
    } else if is_mono {
        9
    } else {
        17
    };
    let crc_len = if has_crc { 2 } else { 0 };
    let primary_off = 4 + crc_len + side_info_len;
    let samples_per_frame = if version_id == 3 { 1152 } else { 576 };

    let candidates = [primary_off, 36, 21, 13, 38, 23, 15];
    let mut xing_off = None;
    for &off in &candidates {
        if off + 8 <= frame.len()
            && (&frame[off..off + 4] == b"Xing" || &frame[off..off + 4] == b"Info")
        {
            xing_off = Some(off);
            break;
        }
    }
    let xing_off = xing_off?;

    let flags = u32::from_be_bytes([
        frame[xing_off + 4],
        frame[xing_off + 5],
        frame[xing_off + 6],
        frame[xing_off + 7],
    ]);
    let mut pos = xing_off + 8;

    let mut total_frames = None;
    if (flags & 0x01) != 0 {
        if pos + 4 > frame.len() {
            return None;
        }
        let tf = u32::from_be_bytes([frame[pos], frame[pos + 1], frame[pos + 2], frame[pos + 3]]);
        if tf > 0 {
            total_frames = Some(tf);
        }
        pos += 4;
    }

    let mut total_bytes = None;
    if (flags & 0x02) != 0 {
        if pos + 4 > frame.len() {
            return None;
        }
        total_bytes = Some(u32::from_be_bytes([
            frame[pos],
            frame[pos + 1],
            frame[pos + 2],
            frame[pos + 3],
        ]));
        pos += 4;
    }

    if (flags & 0x04) != 0 {
        if pos + 100 > frame.len() {
            return None;
        }
        pos += 100;
    }

    if (flags & 0x08) != 0 {
        if pos + 4 > frame.len() {
            return None;
        }
        pos += 4;
    }

    let mut enc_delay = 0u16;
    let mut enc_padding = 0u16;

    let is_known_encoder_tag = |tag: &[u8]| -> bool {
        tag == b"LAME"
            || tag == b"Lavc"
            || tag == b"Lavf"
            || tag == b"GOGO"
            || tag == b"L3.9"
            || tag == b"XING"
            || tag == b"Info"
    };

    let lame_start = if pos + 24 <= frame.len() && is_known_encoder_tag(&frame[pos..pos + 4]) {
        Some(pos)
    } else if pos < frame.len() {
        frame[pos..]
            .windows(4)
            .position(is_known_encoder_tag)
            .map(|rel| pos + rel)
    } else {
        None
    };

    if let Some(l_off) = lame_start {
        if l_off + 24 <= frame.len() {
            let b0 = frame[l_off + 21];
            let b1 = frame[l_off + 22];
            let b2 = frame[l_off + 23];
            let delay = ((b0 as u16) << 4) | ((b1 as u16) >> 4);
            let padding = (((b1 as u16) & 0x0F) << 8) | (b2 as u16);
            if delay <= 2880 && padding <= 2880 {
                enc_delay = delay;
                enc_padding = padding;
            }
        }
    }

    Some(XingLameInfo {
        total_frames,
        total_bytes,
        enc_delay,
        enc_padding,
        samples_per_frame,
    })
}

/// Applies gapless start (`lead_trim`) and end (`max_raw_frames`) trimming
/// in-place to an interleaved stereo `pcm_out` buffer for a single decoded
/// MPEG frame.
fn apply_mp3_gapless_trim(
    pcm_out: &mut [i32],
    frames_produced: usize,
    raw_frames_decoded: &mut u64,
    lead_trim: u64,
    max_raw_frames: Option<u64>,
) -> usize {
    let frame_start = *raw_frames_decoded;
    let frame_end = frame_start + frames_produced as u64;
    *raw_frames_decoded = frame_end;

    let keep_start = frame_start.max(lead_trim);
    let keep_end = match max_raw_frames {
        Some(limit) => frame_end.min(limit),
        None => frame_end,
    };
    if keep_start >= keep_end {
        return 0;
    }
    let local_start = (keep_start - frame_start) as usize;
    let local_len = (keep_end - keep_start) as usize;
    if local_start > 0 {
        pcm_out.copy_within(local_start * 2..(local_start + local_len) * 2, 0);
    }
    local_len
}

/// Pure-Rust MP3 Decoder using nanomp3-core (100% safe, no_std).
///
/// Heap-boxes decoder state (22.5KB) and frame scratch buffer (9.2KB)
/// to eliminate FreeRTOS task stack overflows (DEC-001).
#[cfg(feature = "alloc")]
pub struct Mp3Decoder {
    pub sample_rate: u32,
    decoder: Box<nanomp3_core::Decoder>,
    sample_buf: Box<[i32]>,
    id3_skip_remaining: usize,
    xing_checked: bool,
    xing_info: Option<XingLameInfo>,
    include_decoder_delay: bool,
    raw_frames_decoded: u64,
}

#[cfg(feature = "alloc")]
impl Default for Mp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl Mp3Decoder {
    /// Standard Layer III synthesis filterbank + MDCT decoder delay in
    /// stereo frames.
    pub const DECODER_DELAY_FRAMES: u16 = 529;

    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            decoder: Box::new(nanomp3_core::Decoder::new()),
            sample_buf: vec![0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME].into_boxed_slice(),
            id3_skip_remaining: 0,
            xing_checked: false,
            xing_info: None,
            include_decoder_delay: false,
            raw_frames_decoded: 0,
        }
    }

    /// Configures whether the 529-sample MPEG Layer III synthesis filterbank
    /// decoder delay is added to `enc_delay` during leading sample trimming.
    #[must_use]
    pub const fn with_decoder_delay(mut self, enabled: bool) -> Self {
        self.include_decoder_delay = enabled;
        self
    }

    /// Returns the parsed LAME encoder delay (`enc_delay`) in stereo frames.
    #[must_use]
    pub const fn enc_delay_frames(&self) -> u16 {
        match self.xing_info {
            Some(info) => info.enc_delay,
            None => 0,
        }
    }

    /// Returns the parsed LAME encoder padding (`enc_padding`) in stereo
    /// frames.
    #[must_use]
    pub const fn enc_padding_frames(&self) -> u16 {
        match self.xing_info {
            Some(info) => info.enc_padding,
            None => 0,
        }
    }

    /// Returns the total audio frame count from the Xing/Info header, if
    /// present.
    #[must_use]
    pub const fn total_frames(&self) -> Option<u32> {
        match self.xing_info {
            Some(info) => info.total_frames,
            None => None,
        }
    }

    /// Returns the full parsed Xing/Info/LAME metadata block, if present.
    #[must_use]
    pub const fn xing_info(&self) -> Option<XingLameInfo> {
        self.xing_info
    }
}

#[cfg(feature = "alloc")]
impl AudioDecoder for Mp3Decoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        if pcm_out.len() < nanomp3_core::MAX_SAMPLES_PER_FRAME {
            return Err(DecoderError::BufferOverflow);
        }

        if self.id3_skip_remaining > 0 {
            if encoded_in.is_empty() {
                return Err(DecoderError::NeedMoreData);
            }
            let to_skip = core::cmp::min(encoded_in.len(), self.id3_skip_remaining);
            self.id3_skip_remaining -= to_skip;
            return Ok((to_skip, 0));
        }

        if encoded_in.starts_with(b"ID3") {
            if encoded_in.len() < 10 {
                return Err(DecoderError::NeedMoreData);
            }
            let tag_size = ((encoded_in[6] as usize & 0x7F) << 21)
                | ((encoded_in[7] as usize & 0x7F) << 14)
                | ((encoded_in[8] as usize & 0x7F) << 7)
                | (encoded_in[9] as usize & 0x7F);
            let footer = if (encoded_in[5] & 0x10) != 0 { 10 } else { 0 };
            let total_id3 = 10 + tag_size + footer;
            let to_skip = core::cmp::min(encoded_in.len(), total_id3);
            self.id3_skip_remaining = total_id3 - to_skip;
            return Ok((to_skip, 0));
        }

        if encoded_in.len() < 4 {
            return Err(DecoderError::NeedMoreData);
        }

        let mut sync_offset = None;
        let mut idx = 0;
        while idx + 4 <= encoded_in.len() {
            let h = nanomp3_core::__private::Header::read(&encoded_in[idx..]);
            if h.valid() {
                sync_offset = Some((idx, h));
                break;
            }
            idx += 1;
        }

        let (sync_idx, h) = match sync_offset {
            None => return Ok((encoded_in.len(), 0)),
            Some((idx, h)) => (idx, h),
        };

        if sync_idx > 0 {
            return Ok((sync_idx, 0));
        }

        let frame_bytes = h.frame_bytes(0) + h.padding();
        if frame_bytes == 0 || frame_bytes > 2881 {
            return Ok((sync_idx + 1, 0));
        }

        let remaining = encoded_in.len().saturating_sub(sync_idx);
        if remaining < frame_bytes {
            return Err(DecoderError::NeedMoreData);
        }
        if !self.xing_checked {
            if let Some(xing) = parse_xing_lame_header(&encoded_in[..frame_bytes]) {
                self.xing_checked = true;
                self.xing_info = Some(xing);
                let sr = h.sample_rate_hz();
                if sr > 0 {
                    self.sample_rate = sr;
                }
                return Ok((frame_bytes, 0));
            }
            self.xing_checked = true;
        }

        let direct = pcm_out.len() >= nanomp3_core::MAX_SAMPLES_PER_FRAME;
        let (consumed, result) = if direct {
            self.decoder.decode(encoded_in, pcm_out)
        } else {
            self.decoder.decode(encoded_in, &mut self.sample_buf)
        };

        match result {
            Ok(info) => {
                self.xing_checked = true;
                if info.sample_rate > 0 {
                    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&info.sample_rate) {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    self.sample_rate = info.sample_rate;
                }
                let channels = info.channels.num() as usize;
                let frames_produced = info.samples_produced;
                let max_frames = pcm_out.len() / 2;

                if max_frames < frames_produced {
                    return Err(DecoderError::BufferOverflow);
                }

                if direct {
                    if channels == 1 {
                        for i in (0..frames_produced).rev() {
                            let s = pcm_out[i];
                            pcm_out[i * 2] = s;
                            pcm_out[i * 2 + 1] = s;
                        }
                    }
                } else if channels == 2 {
                    let total_samples = frames_produced * 2;
                    if self.sample_buf.len() >= total_samples && pcm_out.len() >= total_samples {
                        pcm_out[..total_samples].copy_from_slice(&self.sample_buf[..total_samples]);
                    }
                } else if channels == 1 {
                    for i in 0..frames_produced {
                        let s = self.sample_buf[i];
                        pcm_out[i * 2] = s;
                        pcm_out[i * 2 + 1] = s;
                    }
                }

                let trimmed_frames = if let Some(xing) = self.xing_info {
                    let extra = if self.include_decoder_delay {
                        Self::DECODER_DELAY_FRAMES as u64
                    } else {
                        0
                    };
                    let lead_trim = (xing.enc_delay as u64) + extra;
                    let max_raw = xing.total_frames.map(|tf| {
                        let audio_frames = (tf as u64).saturating_sub(1);
                        let total_raw = audio_frames * (xing.samples_per_frame as u64);
                        total_raw.saturating_sub(xing.enc_padding as u64)
                    });
                    apply_mp3_gapless_trim(
                        pcm_out,
                        frames_produced,
                        &mut self.raw_frames_decoded,
                        lead_trim,
                        max_raw,
                    )
                } else {
                    self.raw_frames_decoded += frames_produced as u64;
                    frames_produced
                };

                Ok((consumed, trimmed_frames))
            }
            Err(nanomp3_core::DecodeError::NoFrame) => {
                if consumed > 0 {
                    Ok((consumed, 0))
                } else {
                    Err(DecoderError::NeedMoreData)
                }
            }
            Err(nanomp3_core::DecodeError::ReservoirUnavailable(_)) => {
                // DEC-005: Differentiate between 0 consumed (need more bytes)
                // and >0 consumed (absorbed into reservoir)
                if consumed == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((consumed, 0))
                }
            }
            Err(_) => {
                if consumed > 0 {
                    Ok((consumed, 0))
                } else {
                    Err(DecoderError::CorruptedFrame)
                }
            }
        }
    }

    fn flush(&mut self) {
        *self.decoder = nanomp3_core::Decoder::new();
        self.sample_buf.fill(0);
        self.id3_skip_remaining = 0;
        self.xing_checked = false;
        self.xing_info = None;
        self.raw_frames_decoded = 0;
    }
}

/// Zero-heap, `no_std`, `const fn`-initializable MP3 decoder for
/// embedded targets (`xtensa`).
///
/// Decodes directly into `pcm_out` (`>= 2304` `i32` samples) with zero
/// heap allocations and resets in O(1) via `nanomp3_core::__private::init`
/// without constructing a 23 KB temporary.
pub struct StaticMp3Decoder {
    pub sample_rate: u32,
    decoder: nanomp3_core::Decoder,
    id3_skip_remaining: usize,
    xing_checked: bool,
    xing_info: Option<XingLameInfo>,
    include_decoder_delay: bool,
    raw_frames_decoded: u64,
}

impl Default for StaticMp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticMp3Decoder {
    /// Standard Layer III synthesis filterbank + MDCT decoder delay in
    /// stereo frames.
    pub const DECODER_DELAY_FRAMES: u16 = 529;

    pub const fn new() -> Self {
        Self {
            sample_rate: 0,
            decoder: nanomp3_core::Decoder::new(),
            id3_skip_remaining: 0,
            xing_checked: false,
            xing_info: None,
            include_decoder_delay: false,
            raw_frames_decoded: 0,
        }
    }

    /// Configures whether the 529-sample MPEG Layer III synthesis filterbank
    /// decoder delay is added to `enc_delay` during leading sample trimming.
    #[must_use]
    pub const fn with_decoder_delay(mut self, enabled: bool) -> Self {
        self.include_decoder_delay = enabled;
        self
    }

    /// Returns the parsed LAME encoder delay (`enc_delay`) in stereo frames.
    #[must_use]
    pub const fn enc_delay_frames(&self) -> u16 {
        match self.xing_info {
            Some(info) => info.enc_delay,
            None => 0,
        }
    }

    /// Returns the parsed LAME encoder padding (`enc_padding`) in stereo
    /// frames.
    #[must_use]
    pub const fn enc_padding_frames(&self) -> u16 {
        match self.xing_info {
            Some(info) => info.enc_padding,
            None => 0,
        }
    }

    /// Returns the total audio frame count from the Xing/Info header, if
    /// present.
    #[must_use]
    pub const fn total_frames(&self) -> Option<u32> {
        match self.xing_info {
            Some(info) => info.total_frames,
            None => None,
        }
    }

    /// Returns the full parsed Xing/Info/LAME metadata block, if present.
    #[must_use]
    pub const fn xing_info(&self) -> Option<XingLameInfo> {
        self.xing_info
    }

    /// Resets the decoder state in-place without constructing a 23 KB
    /// temporary on the stack.
    pub fn reset_in_place(&mut self) {
        self.sample_rate = 44100;
        nanomp3_core::__private::init(&mut self.decoder);
        self.id3_skip_remaining = 0;
        self.xing_checked = false;
        self.xing_info = None;
        self.raw_frames_decoded = 0;
    }
}

impl AudioDecoder for StaticMp3Decoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        if pcm_out.len() < nanomp3_core::MAX_SAMPLES_PER_FRAME {
            return Err(DecoderError::BufferOverflow);
        }

        if self.id3_skip_remaining > 0 {
            if encoded_in.is_empty() {
                return Err(DecoderError::NeedMoreData);
            }
            let to_skip = core::cmp::min(encoded_in.len(), self.id3_skip_remaining);
            self.id3_skip_remaining -= to_skip;
            return Ok((to_skip, 0));
        }

        if encoded_in.starts_with(b"ID3") {
            if encoded_in.len() < 10 {
                return Err(DecoderError::NeedMoreData);
            }
            let tag_size = ((encoded_in[6] as usize & 0x7F) << 21)
                | ((encoded_in[7] as usize & 0x7F) << 14)
                | ((encoded_in[8] as usize & 0x7F) << 7)
                | (encoded_in[9] as usize & 0x7F);
            let footer = if (encoded_in[5] & 0x10) != 0 { 10 } else { 0 };
            let total_id3 = 10 + tag_size + footer;
            let to_skip = core::cmp::min(encoded_in.len(), total_id3);
            self.id3_skip_remaining = total_id3 - to_skip;
            return Ok((to_skip, 0));
        }

        if encoded_in.len() < 4 {
            return Err(DecoderError::NeedMoreData);
        }

        let mut sync_offset = None;
        let mut idx = 0;
        while idx + 4 <= encoded_in.len() {
            let h = nanomp3_core::__private::Header::read(&encoded_in[idx..]);
            if h.valid() {
                sync_offset = Some((idx, h));
                break;
            }
            idx += 1;
        }

        let (frame_bytes, slice_to_decode) = match sync_offset {
            None => {
                let skip = encoded_in.len().saturating_sub(3);
                if skip > 0 {
                    return Ok((skip, 0));
                }
                return Err(DecoderError::NeedMoreData);
            }
            Some((offset, _)) if offset > 0 => {
                return Ok((offset, 0));
            }
            Some((0, h)) => {
                let frame_bytes = h.frame_bytes(0) + h.padding();
                if frame_bytes == 0 || frame_bytes > 1728 {
                    return Ok((1, 0));
                }
                if encoded_in.len() < frame_bytes {
                    return Err(DecoderError::NeedMoreData);
                }
                if !self.xing_checked {
                    if let Some(xing) = parse_xing_lame_header(&encoded_in[..frame_bytes]) {
                        self.xing_checked = true;
                        self.xing_info = Some(xing);
                        let sr = h.sample_rate_hz();
                        if sr > 0 {
                            self.sample_rate = sr;
                        }
                        return Ok((frame_bytes, 0));
                    }
                    self.xing_checked = true;
                }
                if encoded_in.len() == frame_bytes {
                    (frame_bytes, &encoded_in[..frame_bytes])
                } else if encoded_in.len() < frame_bytes + 4 {
                    if encoded_in.len() >= frame_bytes + 2
                        && (encoded_in[frame_bytes] != 0xFF
                            || (encoded_in[frame_bytes + 1] & 0xE0) != 0xE0)
                    {
                        return Ok((1, 0));
                    }
                    return Err(DecoderError::NeedMoreData);
                } else {
                    let next_h = nanomp3_core::__private::Header::read(&encoded_in[frame_bytes..]);
                    if !h.compare(next_h) {
                        return Ok((1, 0));
                    }
                    (frame_bytes, &encoded_in[..frame_bytes + 4])
                }
            }
            _ => unreachable!(),
        };

        let (consumed, result) = self.decoder.decode(slice_to_decode, pcm_out);

        match result {
            Ok(info) => {
                self.xing_checked = true;
                if info.sample_rate > 0 {
                    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&info.sample_rate) {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    self.sample_rate = info.sample_rate;
                }
                let channels = info.channels.num() as usize;
                let frames_produced = info.samples_produced;
                let max_frames = pcm_out.len() / 2;

                if max_frames < frames_produced {
                    return Err(DecoderError::BufferOverflow);
                }

                if channels == 1 {
                    for i in (0..frames_produced).rev() {
                        let s = pcm_out[i];
                        pcm_out[i * 2] = s;
                        pcm_out[i * 2 + 1] = s;
                    }
                }

                let trimmed_frames = if let Some(xing) = self.xing_info {
                    let extra = if self.include_decoder_delay {
                        Self::DECODER_DELAY_FRAMES as u64
                    } else {
                        0
                    };
                    let lead_trim = (xing.enc_delay as u64) + extra;
                    let max_raw = xing.total_frames.map(|tf| {
                        let audio_frames = (tf as u64).saturating_sub(1);
                        let total_raw = audio_frames * (xing.samples_per_frame as u64);
                        total_raw.saturating_sub(xing.enc_padding as u64)
                    });
                    apply_mp3_gapless_trim(
                        pcm_out,
                        frames_produced,
                        &mut self.raw_frames_decoded,
                        lead_trim,
                        max_raw,
                    )
                } else {
                    self.raw_frames_decoded += frames_produced as u64;
                    frames_produced
                };

                Ok((consumed, trimmed_frames))
            }
            Err(_) => {
                if consumed > 0 {
                    Ok((consumed, 0))
                } else {
                    Ok((frame_bytes.max(1), 0))
                }
            }
        }
    }

    fn flush(&mut self) {
        self.reset_in_place();
    }
}

#[cfg(any(feature = "std", test))]
pub struct FlacByteReader<'a> {
    pub data: &'a [u8],
    pub pos: &'a std::cell::Cell<usize>,
}

#[cfg(any(feature = "std", test))]
impl<'a> std::io::Read for FlacByteReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let cur = self.pos.get();
        let avail = &self.data[cur..];
        let n = avail.len().min(buf.len());
        buf[..n].copy_from_slice(&avail[..n]);
        self.pos.set(cur + n);
        Ok(n)
    }
}

const CRC8_TABLE: [u8; 256] = [
    0x00, 0x07, 0x0e, 0x09, 0x1c, 0x1b, 0x12, 0x15, 0x38, 0x3f, 0x36, 0x31, 0x24, 0x23, 0x2a, 0x2d,
    0x70, 0x77, 0x7e, 0x79, 0x6c, 0x6b, 0x62, 0x65, 0x48, 0x4f, 0x46, 0x41, 0x54, 0x53, 0x5a, 0x5d,
    0xe0, 0xe7, 0xee, 0xe9, 0xfc, 0xfb, 0xf2, 0xf5, 0xd8, 0xdf, 0xd6, 0xd1, 0xc4, 0xc3, 0xca, 0xcd,
    0x90, 0x97, 0x9e, 0x99, 0x8c, 0x8b, 0x82, 0x85, 0xa8, 0xaf, 0xa6, 0xa1, 0xb4, 0xb3, 0xba, 0xbd,
    0xc7, 0xc0, 0xc9, 0xce, 0xdb, 0xdc, 0xd5, 0xd2, 0xff, 0xf8, 0xf1, 0xf6, 0xe3, 0xe4, 0xed, 0xea,
    0xb7, 0xb0, 0xb9, 0xbe, 0xab, 0xac, 0xa5, 0xa2, 0x8f, 0x88, 0x81, 0x86, 0x93, 0x94, 0x9d, 0x9a,
    0x27, 0x20, 0x29, 0x2e, 0x3b, 0x3c, 0x35, 0x32, 0x1f, 0x18, 0x11, 0x16, 0x03, 0x04, 0x0d, 0x0a,
    0x57, 0x50, 0x59, 0x5e, 0x4b, 0x4c, 0x45, 0x42, 0x6f, 0x68, 0x61, 0x66, 0x73, 0x74, 0x7d, 0x7a,
    0x89, 0x8e, 0x87, 0x80, 0x95, 0x92, 0x9b, 0x9c, 0xb1, 0xb6, 0xbf, 0xb8, 0xad, 0xaa, 0xa3, 0xa4,
    0xf9, 0xfe, 0xf7, 0xf0, 0xe5, 0xe2, 0xeb, 0xec, 0xc1, 0xc6, 0xcf, 0xc8, 0xdd, 0xda, 0xd3, 0xd4,
    0x69, 0x6e, 0x67, 0x60, 0x75, 0x72, 0x7b, 0x7c, 0x51, 0x56, 0x5f, 0x58, 0x4d, 0x4a, 0x43, 0x44,
    0x19, 0x1e, 0x17, 0x10, 0x05, 0x02, 0x0b, 0x0c, 0x21, 0x26, 0x2f, 0x28, 0x3d, 0x3a, 0x33, 0x34,
    0x4e, 0x49, 0x40, 0x47, 0x52, 0x55, 0x5c, 0x5b, 0x76, 0x71, 0x78, 0x7f, 0x6a, 0x6d, 0x64, 0x63,
    0x3e, 0x39, 0x30, 0x37, 0x22, 0x25, 0x2c, 0x2b, 0x06, 0x01, 0x08, 0x0f, 0x1a, 0x1d, 0x14, 0x13,
    0xae, 0xa9, 0xa0, 0xa7, 0xb2, 0xb5, 0xbc, 0xbb, 0x96, 0x91, 0x98, 0x9f, 0x8a, 0x8d, 0x84, 0x83,
    0xde, 0xd9, 0xd0, 0xd7, 0xc2, 0xc5, 0xcc, 0xcb, 0xe6, 0xe1, 0xe8, 0xef, 0xfa, 0xfd, 0xf4, 0xf3,
];

const CRC16_TABLE: [u16; 256] = [
    0x0000, 0x8005, 0x800f, 0x000a, 0x801b, 0x001e, 0x0014, 0x8011, 0x8033, 0x0036, 0x003c, 0x8039,
    0x0028, 0x802d, 0x8027, 0x0022, 0x8063, 0x0066, 0x006c, 0x8069, 0x0078, 0x807d, 0x8077, 0x0072,
    0x0050, 0x8055, 0x805f, 0x005a, 0x804b, 0x004e, 0x0044, 0x8041, 0x80c3, 0x00c6, 0x00cc, 0x80c9,
    0x00d8, 0x80dd, 0x80d7, 0x00d2, 0x00f0, 0x80f5, 0x80ff, 0x00fa, 0x80eb, 0x00ee, 0x00e4, 0x80e1,
    0x00a0, 0x80a5, 0x80af, 0x00aa, 0x80bb, 0x00be, 0x00b4, 0x80b1, 0x8093, 0x0096, 0x009c, 0x8099,
    0x0088, 0x808d, 0x8087, 0x0082, 0x8183, 0x0186, 0x018c, 0x8189, 0x0198, 0x819d, 0x8197, 0x0192,
    0x01b0, 0x81b5, 0x81bf, 0x01ba, 0x81ab, 0x01ae, 0x01a4, 0x81a1, 0x01e0, 0x81e5, 0x81ef, 0x01ea,
    0x81fb, 0x01fe, 0x01f4, 0x81f1, 0x81d3, 0x01d6, 0x01dc, 0x81d9, 0x01c8, 0x81cd, 0x81c7, 0x01c2,
    0x0140, 0x8145, 0x814f, 0x014a, 0x815b, 0x015e, 0x0154, 0x8151, 0x8173, 0x0176, 0x017c, 0x8179,
    0x0168, 0x816d, 0x8167, 0x0162, 0x8123, 0x0126, 0x012c, 0x8129, 0x0138, 0x813d, 0x8137, 0x0132,
    0x0110, 0x8115, 0x811f, 0x011a, 0x810b, 0x010e, 0x0104, 0x8101, 0x8303, 0x0306, 0x030c, 0x8309,
    0x0318, 0x831d, 0x8317, 0x0312, 0x0330, 0x8335, 0x833f, 0x033a, 0x832b, 0x032e, 0x0324, 0x8321,
    0x0360, 0x8365, 0x836f, 0x036a, 0x837b, 0x037e, 0x0374, 0x8371, 0x8353, 0x0356, 0x035c, 0x8359,
    0x0348, 0x834d, 0x8347, 0x0342, 0x03c0, 0x83c5, 0x83cf, 0x03ca, 0x83db, 0x03de, 0x03d4, 0x83d1,
    0x83f3, 0x03f6, 0x03fc, 0x83f9, 0x03e8, 0x83ed, 0x83e7, 0x03e2, 0x83a3, 0x03a6, 0x03ac, 0x83a9,
    0x03b8, 0x83bd, 0x83b7, 0x03b2, 0x0390, 0x8395, 0x839f, 0x039a, 0x838b, 0x038e, 0x0384, 0x8381,
    0x0280, 0x8285, 0x828f, 0x028a, 0x829b, 0x029e, 0x0294, 0x8291, 0x82b3, 0x02b6, 0x02bc, 0x82b9,
    0x02a8, 0x82ad, 0x82a7, 0x02a2, 0x82e3, 0x02e6, 0x02ec, 0x82e9, 0x02f8, 0x82fd, 0x82f7, 0x02f2,
    0x02d0, 0x82d5, 0x82df, 0x02da, 0x82cb, 0x02ce, 0x02c4, 0x82c1, 0x8243, 0x0246, 0x024c, 0x8249,
    0x0258, 0x825d, 0x8257, 0x0252, 0x0270, 0x8275, 0x827f, 0x027a, 0x826b, 0x026e, 0x0264, 0x8261,
    0x0220, 0x8225, 0x822f, 0x022a, 0x823b, 0x023e, 0x0234, 0x8231, 0x8213, 0x0216, 0x021c, 0x8219,
    0x0208, 0x820d, 0x8207, 0x0202,
];

#[inline(always)]
fn flac_crc8(data: &[u8]) -> u8 {
    let mut state = 0u8;
    for &byte in data {
        state = CRC8_TABLE[(state ^ byte) as usize];
    }
    state
}

#[inline(always)]
fn flac_crc16(data: &[u8]) -> u16 {
    let mut state = 0u16;
    for &byte in data {
        state = (state << 8) ^ CRC16_TABLE[((state >> 8) as u8 ^ byte) as usize];
    }
    state
}

struct BitReader<'a> {
    bytes: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    #[inline(always)]
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    #[inline(always)]
    fn read_bit(&mut self) -> Result<u32, ()> {
        if self.byte_pos >= self.bytes.len() {
            return Err(());
        }
        let bit = ((self.bytes[self.byte_pos] >> (7 - self.bit_pos)) & 1) as u32;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit)
    }

    #[inline(always)]
    fn read_bits(&mut self, mut n: usize) -> Result<u32, ()> {
        if n == 0 {
            return Ok(0);
        }
        let mut val = 0u32;
        while n > 0 {
            if self.byte_pos >= self.bytes.len() {
                return Err(());
            }
            if self.bit_pos == 0 && n >= 8 {
                val = (val << 8) | (self.bytes[self.byte_pos] as u32);
                self.byte_pos += 1;
                n -= 8;
                continue;
            }
            let avail = 8 - self.bit_pos as usize;
            let take = n.min(avail);
            let shift = avail - take;
            let mask = ((1u32 << take) - 1) as u8;
            let bits = (self.bytes[self.byte_pos] >> shift) & mask;
            val = (val << take) | (bits as u32);
            self.bit_pos += take as u8;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
            n -= take;
        }
        Ok(val)
    }

    #[inline(always)]
    fn read_bits_u64(&mut self, mut n: usize) -> Result<u64, ()> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err(());
        }
        let mut val = 0u64;
        while n > 0 {
            if self.byte_pos >= self.bytes.len() {
                return Err(());
            }
            if self.bit_pos == 0 && n >= 8 {
                val = (val << 8) | (self.bytes[self.byte_pos] as u64);
                self.byte_pos += 1;
                n -= 8;
                continue;
            }
            let avail = 8 - self.bit_pos as usize;
            let take = n.min(avail);
            let shift = avail - take;
            let mask = ((1u32 << take) - 1) as u8;
            let bits = (self.bytes[self.byte_pos] >> shift) & mask;
            val = (val << take) | (bits as u64);
            self.bit_pos += take as u8;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
            n -= take;
        }
        Ok(val)
    }

    #[inline(always)]
    fn read_signed(&mut self, n: usize) -> Result<i64, ()> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err(());
        }
        let u = self.read_bits_u64(n)?;
        if n == 64 {
            return Ok(u as i64);
        }
        let sign_bit = 1u64 << (n - 1);
        if (u & sign_bit) != 0 {
            let mask = !((1u64 << n) - 1);
            Ok((u | mask) as i64)
        } else {
            Ok(u as i64)
        }
    }

    #[inline(always)]
    fn read_unary_zeros(&mut self) -> Result<u32, ()> {
        let mut count = 0u32;
        while self.bit_pos > 0 {
            let b = self.read_bit()?;
            if b == 1 {
                return Ok(count);
            }
            count += 1;
        }
        while self.byte_pos < self.bytes.len() && self.bytes[self.byte_pos] == 0 {
            count += 8;
            self.byte_pos += 1;
        }
        if self.byte_pos >= self.bytes.len() {
            return Err(());
        }
        let byte = self.bytes[self.byte_pos];
        let leading_zeros = byte.leading_zeros();
        count += leading_zeros;
        self.bit_pos = (leading_zeros as u8) + 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(count)
    }

    #[inline(always)]
    fn align_to_byte(&mut self) {
        if self.bit_pos > 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }
}

fn read_utf8_number(buf: &[u8], idx: usize) -> Result<(u64, usize), ()> {
    if idx >= buf.len() {
        return Err(());
    }
    let u0 = buf[idx];
    if (u0 & 0x80) == 0 {
        return Ok((u0 as u64, idx + 1));
    }
    if (u0 & 0xE0) == 0xC0 {
        if idx + 2 > buf.len() {
            return Err(());
        }
        let u1 = buf[idx + 1];
        if (u1 & 0xC0) != 0x80 {
            return Err(());
        }
        let val = ((u0 as u64 & 0x1F) << 6) | (u1 as u64 & 0x3F);
        return Ok((val, idx + 2));
    }
    if (u0 & 0xF0) == 0xE0 {
        if idx + 3 > buf.len() {
            return Err(());
        }
        let (u1, u2) = (buf[idx + 1], buf[idx + 2]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 {
            return Err(());
        }
        let val = ((u0 as u64 & 0x0F) << 12) | ((u1 as u64 & 0x3F) << 6) | (u2 as u64 & 0x3F);
        return Ok((val, idx + 3));
    }
    if (u0 & 0xF8) == 0xF0 {
        if idx + 4 > buf.len() {
            return Err(());
        }
        let (u1, u2, u3) = (buf[idx + 1], buf[idx + 2], buf[idx + 3]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 {
            return Err(());
        }
        let val = ((u0 as u64 & 0x07) << 18)
            | ((u1 as u64 & 0x3F) << 12)
            | ((u2 as u64 & 0x3F) << 6)
            | (u3 as u64 & 0x3F);
        return Ok((val, idx + 4));
    }
    if (u0 & 0xFC) == 0xF8 {
        if idx + 5 > buf.len() {
            return Err(());
        }
        let (u1, u2, u3, u4) = (buf[idx + 1], buf[idx + 2], buf[idx + 3], buf[idx + 4]);
        let valid = (u1 & 0xC0) == 0x80
            && (u2 & 0xC0) == 0x80
            && (u3 & 0xC0) == 0x80
            && (u4 & 0xC0) == 0x80;
        if !valid {
            return Err(());
        }
        let val = ((u0 as u64 & 0x03) << 24)
            | ((u1 as u64 & 0x3F) << 18)
            | ((u2 as u64 & 0x3F) << 12)
            | ((u3 as u64 & 0x3F) << 6)
            | (u4 as u64 & 0x3F);
        return Ok((val, idx + 5));
    }
    if (u0 & 0xFE) == 0xFC {
        if idx + 6 > buf.len() {
            return Err(());
        }
        let (u1, u2, u3, u4, u5) = (
            buf[idx + 1],
            buf[idx + 2],
            buf[idx + 3],
            buf[idx + 4],
            buf[idx + 5],
        );
        let valid = (u1 & 0xC0) == 0x80
            && (u2 & 0xC0) == 0x80
            && (u3 & 0xC0) == 0x80
            && (u4 & 0xC0) == 0x80
            && (u5 & 0xC0) == 0x80;
        if !valid {
            return Err(());
        }
        let val = ((u0 as u64 & 0x01) << 30)
            | ((u1 as u64 & 0x3F) << 24)
            | ((u2 as u64 & 0x3F) << 18)
            | ((u3 as u64 & 0x3F) << 12)
            | ((u4 as u64 & 0x3F) << 6)
            | (u5 as u64 & 0x3F);
        return Ok((val, idx + 6));
    }
    if u0 == 0xFE {
        if idx + 7 > buf.len() {
            return Err(());
        }
        let (u1, u2, u3, u4, u5, u6) = (
            buf[idx + 1],
            buf[idx + 2],
            buf[idx + 3],
            buf[idx + 4],
            buf[idx + 5],
            buf[idx + 6],
        );
        let valid = (u1 & 0xC0) == 0x80
            && (u2 & 0xC0) == 0x80
            && (u3 & 0xC0) == 0x80
            && (u4 & 0xC0) == 0x80
            && (u5 & 0xC0) == 0x80
            && (u6 & 0xC0) == 0x80;
        if !valid {
            return Err(());
        }
        let val = ((u1 as u64 & 0x3F) << 30)
            | ((u2 as u64 & 0x3F) << 24)
            | ((u3 as u64 & 0x3F) << 18)
            | ((u4 as u64 & 0x3F) << 12)
            | ((u5 as u64 & 0x3F) << 6)
            | (u6 as u64 & 0x3F);
        return Ok((val, idx + 7));
    }
    Err(())
}

struct FrameHeader {
    block_size: usize,
    sample_rate: u32,
    channels: u32,
    channel_assignment: u8,
    bits_per_sample: u32,
    header_bytes: usize,
}

fn parse_frame_header(
    buf: &[u8],
    stream_sr: u32,
    stream_bps: u32,
    min_bs: u16,
) -> Result<Option<FrameHeader>, ()> {
    if buf.len() < 4 {
        return Ok(None);
    }
    if buf[0] != 0xFF || (buf[1] & 0xFE) != 0xF8 {
        return Err(());
    }
    let block_size_code = (buf[2] >> 4) & 0x0F;
    let sample_rate_code = buf[2] & 0x0F;
    let channel_assignment = (buf[3] >> 4) & 0x0F;
    let sample_size_code = (buf[3] >> 1) & 0x07;
    let reserved = buf[3] & 1;
    if reserved != 0 || sample_rate_code == 15 || sample_size_code == 3 {
        return Err(());
    }
    let num_channels = match channel_assignment {
        0 => 1,
        1 | 8 | 9 | 10 => 2,
        _ => return Err(()),
    };

    let (_, mut idx) = match read_utf8_number(buf, 4) {
        Ok(res) => res,
        Err(_) => {
            if buf.len() < 12 {
                return Ok(None);
            }
            return Err(());
        }
    };

    let block_size = match block_size_code {
        0 => {
            if min_bs > 0 {
                min_bs as usize
            } else {
                return Err(());
            }
        }
        1 => 192,
        2..=5 => 576 * (1usize << (block_size_code - 2)),
        6 => {
            if idx >= buf.len() {
                return Ok(None);
            }
            let bs = (buf[idx] as usize) + 1;
            idx += 1;
            bs
        }
        7 => {
            if idx + 2 > buf.len() {
                return Ok(None);
            }
            let bs = (u16::from_be_bytes([buf[idx], buf[idx + 1]]) as usize) + 1;
            idx += 2;
            bs
        }
        8..=15 => 256 * (1usize << (block_size_code - 8)),
        _ => return Err(()),
    };
    if block_size == 0 || block_size > MAX_FLAC_BLOCK_SAMPLES {
        return Err(());
    }

    let sample_rate = match sample_rate_code {
        0 => stream_sr,
        1 => 88200,
        2 => 176400,
        3 => 192000,
        4 => 8000,
        5 => 16000,
        6 => 22050,
        7 => 24000,
        8 => 32000,
        9 => 44100,
        10 => 48000,
        11 => 96000,
        12 => {
            if idx >= buf.len() {
                return Ok(None);
            }
            let sr = (buf[idx] as u32) * 1000;
            idx += 1;
            sr
        }
        13 => {
            if idx + 2 > buf.len() {
                return Ok(None);
            }
            let sr = u16::from_be_bytes([buf[idx], buf[idx + 1]]) as u32;
            idx += 2;
            sr
        }
        14 => {
            if idx + 2 > buf.len() {
                return Ok(None);
            }
            let sr = (u16::from_be_bytes([buf[idx], buf[idx + 1]]) as u32) * 10;
            idx += 2;
            sr
        }
        _ => return Err(()),
    };

    let bits_per_sample = match sample_size_code {
        0 => stream_bps,
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => return Err(()),
    };

    if idx >= buf.len() {
        return Ok(None);
    }
    let expected_crc = buf[idx];
    let actual_crc = flac_crc8(&buf[0..idx]);
    if expected_crc != actual_crc {
        return Err(());
    }
    idx += 1;

    Ok(Some(FrameHeader {
        block_size,
        sample_rate,
        channels: num_channels,
        channel_assignment,
        bits_per_sample,
        header_bytes: idx,
    }))
}

fn decode_subframe(
    reader: &mut BitReader<'_>,
    subframe_bps: u32,
    block_size: usize,
    out: &mut [i64],
) -> Result<(), ()> {
    let pad = reader.read_bit()?;
    if pad != 0 {
        return Err(());
    }
    let sf_type = reader.read_bits(6)?;
    let wasted_flag = reader.read_bit()?;
    let wasted_bits = if wasted_flag == 1 {
        (reader.read_unary_zeros()? + 1) as usize
    } else {
        0
    };
    if wasted_bits >= subframe_bps as usize {
        return Err(());
    }
    let eff_bps = (subframe_bps as usize) - wasted_bits;

    match sf_type {
        0 => {
            let val = reader.read_signed(eff_bps)?;
            out[..block_size].fill(val);
        }
        1 => {
            for sample in out[..block_size].iter_mut() {
                *sample = reader.read_signed(eff_bps)?;
            }
        }
        8..=12 => {
            let order = (sf_type - 8) as usize;
            if block_size < order {
                return Err(());
            }
            for sample in out[..order].iter_mut() {
                *sample = reader.read_signed(eff_bps)?;
            }
            decode_residual(reader, block_size, order, &mut out[order..block_size])?;
            match order {
                0 => {}
                1 => {
                    for i in 1..block_size {
                        out[i] = out[i].wrapping_add(out[i - 1]);
                    }
                }
                2 => {
                    for i in 2..block_size {
                        let term = (2i64).wrapping_mul(out[i - 1]).wrapping_sub(out[i - 2]);
                        out[i] = out[i].wrapping_add(term);
                    }
                }
                3 => {
                    for i in 3..block_size {
                        let term = (3i64)
                            .wrapping_mul(out[i - 1])
                            .wrapping_sub((3i64).wrapping_mul(out[i - 2]))
                            .wrapping_add(out[i - 3]);
                        out[i] = out[i].wrapping_add(term);
                    }
                }
                4 => {
                    for i in 4..block_size {
                        let term = (4i64)
                            .wrapping_mul(out[i - 1])
                            .wrapping_sub((6i64).wrapping_mul(out[i - 2]))
                            .wrapping_add((4i64).wrapping_mul(out[i - 3]))
                            .wrapping_sub(out[i - 4]);
                        out[i] = out[i].wrapping_add(term);
                    }
                }
                _ => return Err(()),
            }
        }
        32..=63 => {
            let order = ((sf_type & 0x1F) + 1) as usize;
            if block_size < order {
                return Err(());
            }
            for sample in out[..order].iter_mut() {
                *sample = reader.read_signed(eff_bps)?;
            }
            let raw_prec = reader.read_bits(4)?;
            let qlp_prec = (raw_prec + 1) as usize;
            if qlp_prec == 1 || qlp_prec > 16 {
                return Err(());
            }
            let qlp_shift = reader.read_signed(5)? as i32;
            if qlp_shift < 0 {
                return Err(());
            }
            let mut coeffs = [0i64; 32];
            for coeff in coeffs[..order].iter_mut() {
                *coeff = reader.read_signed(qlp_prec)?;
            }
            decode_residual(reader, block_size, order, &mut out[order..block_size])?;

            if eff_bps <= 16 && qlp_prec <= 15 && order <= 12 {
                for i in order..block_size {
                    let mut sum: i32 = 0;
                    for j in 0..order {
                        sum = sum
                            .wrapping_add((coeffs[j] as i32).wrapping_mul(out[i - 1 - j] as i32));
                    }
                    let predicted = (sum >> qlp_shift) as i64;
                    out[i] = out[i].wrapping_add(predicted);
                }
            } else {
                for i in order..block_size {
                    let mut sum: i64 = 0;
                    for j in 0..order {
                        sum = sum.wrapping_add(coeffs[j].wrapping_mul(out[i - 1 - j]));
                    }
                    let predicted = sum >> qlp_shift;
                    out[i] = out[i].wrapping_add(predicted);
                }
            }
        }
        _ => return Err(()),
    }

    if wasted_bits > 0 {
        for s in out[..block_size].iter_mut() {
            *s <<= wasted_bits;
        }
    }

    Ok(())
}

fn decode_residual(
    reader: &mut BitReader<'_>,
    block_size: usize,
    order: usize,
    out: &mut [i64],
) -> Result<(), ()> {
    let method = reader.read_bits(2)?;
    if method != 0 && method != 1 {
        return Err(());
    }
    let p_order = reader.read_bits(4)? as usize;
    let num_partitions = 1usize << p_order;
    let partition_samples = block_size >> p_order;

    if block_size & (num_partitions - 1) != 0 {
        return Err(());
    }
    if partition_samples < order {
        return Err(());
    }

    let param_bits = if method == 0 { 4 } else { 5 };
    let escape_param = if method == 0 { 0b1111 } else { 0b11111 };

    let mut sample_idx = 0;
    for p in 0..num_partitions {
        let n_samples = if p == 0 {
            partition_samples - order
        } else {
            partition_samples
        };

        let k = reader.read_bits(param_bits)?;
        if k == escape_param {
            let b = reader.read_bits(5)? as usize;
            for _ in 0..n_samples {
                if sample_idx >= out.len() {
                    return Err(());
                }
                out[sample_idx] = reader.read_signed(b)?;
                sample_idx += 1;
            }
        } else {
            for _ in 0..n_samples {
                if sample_idx >= out.len() {
                    return Err(());
                }
                let q = reader.read_unary_zeros()? as u64;
                if q > 65535 {
                    return Err(());
                }
                let rem = if k > 0 {
                    reader.read_bits(k as usize)? as u64
                } else {
                    0
                };
                let u = match q.checked_shl(k) {
                    Some(shifted) => shifted | rem,
                    None => return Err(()),
                };
                let val = if (u & 1) != 0 {
                    -1 - (u >> 1) as i64
                } else {
                    (u >> 1) as i64
                };
                out[sample_idx] = val;
                sample_idx += 1;
            }
        }
    }

    if sample_idx == out.len() {
        Ok(())
    } else {
        Err(())
    }
}

#[inline(always)]
fn decorrelate_samples(
    scratch: &[i64],
    i: usize,
    channel_assignment: u8,
    shift: u32,
) -> (i32, i32) {
    match channel_assignment {
        0 => {
            let s = (scratch[i] as i32) << shift;
            (s, s)
        }
        1 => {
            let l = (scratch[i] as i32) << shift;
            let r = (scratch[MAX_FLAC_BLOCK_SAMPLES + i] as i32) << shift;
            (l, r)
        }
        8 => {
            let ch0 = scratch[i];
            let ch1 = scratch[MAX_FLAC_BLOCK_SAMPLES + i];
            let l = (ch0 as i32) << shift;
            let r = (ch0.wrapping_sub(ch1) as i32) << shift;
            (l, r)
        }
        9 => {
            let ch0 = scratch[i];
            let ch1 = scratch[MAX_FLAC_BLOCK_SAMPLES + i];
            let l = (ch1.wrapping_add(ch0) as i32) << shift;
            let r = (ch1 as i32) << shift;
            (l, r)
        }
        10 => {
            let ch0 = scratch[i];
            let ch1 = scratch[MAX_FLAC_BLOCK_SAMPLES + i];
            let mid = ch0.wrapping_shl(1) | (ch1 & 1);
            let l = (((mid.wrapping_add(ch1)) >> 1) as i32) << shift;
            let r = (((mid.wrapping_sub(ch1)) >> 1) as i32) << shift;
            (l, r)
        }
        _ => {
            let l = (scratch[i] as i32) << shift;
            let r = (scratch[MAX_FLAC_BLOCK_SAMPLES + i] as i32) << shift;
            (l, r)
        }
    }
}

const fn make_ogg_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut r = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            if (r & 0x8000_0000) != 0 {
                r = (r << 1) ^ 0x04C1_1DB7;
            } else {
                r <<= 1;
            }
            bit += 1;
        }
        table[i] = r;
        i += 1;
    }
    table
}

pub const OGG_CRC32_TABLE: [u32; 256] = make_ogg_crc32_table();

/// Computes the RFC 3533 Ogg page CRC-32 checksum (direct polynomial
/// `0x04C11DB7`, zeroing the checksum field at bytes `22..26`).
#[must_use]
pub const fn ogg_crc32(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    let mut idx = 0usize;
    while idx < page.len() {
        let b = if idx >= 22 && idx < 26 {
            0u8
        } else {
            page[idx]
        };
        let table_idx = (((crc >> 24) as u8) ^ b) as usize;
        crc = (crc << 8) ^ OGG_CRC32_TABLE[table_idx];
        idx += 1;
    }
    crc
}

#[cfg(any(feature = "alloc", feature = "std"))]
/// Pure-Rust RFC 9639 Lossless FLAC Decoder with 64KB External PSRAM buffer
/// placement and native Ogg-FLAC (`"OggS"` / `\x7fFLAC\x01\x00`) container
/// unwrapping.
pub struct FlacDecoder {
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
    pub header_len: usize,
    pub stream_buf: vec::Vec<u8>,
    pub pending_pcm: vec::Vec<i32>,
    pub pending_offset: usize,
    subframe_scratch: Box<[i64]>,
    meta_skip_remaining: usize,
    in_metadata: bool,
    min_block_size: u16,
    max_block_size: u16,
    need_more_input: bool,
    is_ogg_flac: Option<bool>,
    ogg_raw_buf: vec::Vec<u8>,
    ogg_partial_pkt: vec::Vec<u8>,
}

#[cfg(any(feature = "alloc", feature = "std"))]
impl Default for FlacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(feature = "alloc", feature = "std"))]
impl FlacDecoder {
    /// Maps FLAC bit depth to 32-bit output shift (e.g. 8-bit shifts left by 24).
    pub const fn sample_shift(bits_per_sample: u32) -> u32 {
        match bits_per_sample {
            8 => 24,
            12 => 20,
            16 => 16,
            20 => 12,
            24 => 8,
            32 => 0,
            _ => 16,
        }
    }

    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            bits_per_sample: 16,
            channels: 2,
            header_len: 0,
            stream_buf: vec::Vec::with_capacity(MAX_STREAM_BUF),
            pending_pcm: vec::Vec::with_capacity(16384),
            pending_offset: 0,
            subframe_scratch: vec![0i64; MAX_FLAC_BLOCK_SAMPLES * 2].into_boxed_slice(),
            meta_skip_remaining: 0,
            in_metadata: false,
            min_block_size: 0,
            max_block_size: 0,
            need_more_input: true,
            is_ogg_flac: None,
            ogg_raw_buf: vec::Vec::with_capacity(4096),
            ogg_partial_pkt: vec::Vec::with_capacity(4096),
        }
    }

    pub fn has_pending_pcm(&self) -> bool {
        if self.pending_offset < self.pending_pcm.len() {
            return true;
        }
        if !self.need_more_input
            && self.header_len > 0
            && self.meta_skip_remaining == 0
            && self.stream_buf.len() > self.header_len + 4
        {
            let buf = &self.stream_buf[self.header_len..];
            for i in 0..buf.len() - 1 {
                if buf[i] == 0xFF && (buf[i + 1] & 0xFE) == 0xF8 {
                    return true;
                }
            }
        }
        false
    }

    pub fn flush(&mut self) {
        self.pending_pcm.clear();
        self.pending_offset = 0;
        self.meta_skip_remaining = 0;
        self.in_metadata = false;
        self.need_more_input = true;
        self.is_ogg_flac = None;
        self.ogg_raw_buf.clear();
        self.ogg_partial_pkt.clear();
        if self.header_len > 0 && self.stream_buf.len() >= self.header_len {
            self.stream_buf.truncate(self.header_len);
        } else {
            self.stream_buf.clear();
            self.header_len = 0;
        }
    }

    fn ingest_ogg_flac_bytes(&mut self, encoded_in: &[u8]) -> Result<usize, DecoderError> {
        if self.ogg_raw_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
            self.flush();
            self.stream_buf.clear();
            self.header_len = 0;
            return Err(DecoderError::CorruptedFrame);
        }
        let input_taken = encoded_in.len();
        self.ogg_raw_buf.extend_from_slice(encoded_in);

        while let Some(pos) = self.ogg_raw_buf.windows(4).position(|w| w == b"OggS") {
            if pos > 0 {
                self.ogg_raw_buf.drain(..pos);
            }
            if self.ogg_raw_buf.len() < 27 {
                break;
            }
            if self.ogg_raw_buf[4] != 0 {
                self.ogg_raw_buf.drain(..4);
                return Err(DecoderError::InvalidHeader);
            }
            let n_segs = self.ogg_raw_buf[26] as usize;
            let page_hdr_len = 27 + n_segs;
            if self.ogg_raw_buf.len() < page_hdr_len {
                break;
            }
            let mut body_len = 0usize;
            for &seg in &self.ogg_raw_buf[27..page_hdr_len] {
                body_len += seg as usize;
            }
            let total_page_len = page_hdr_len + body_len;
            if self.ogg_raw_buf.len() < total_page_len {
                break;
            }

            let expected_crc = u32::from_le_bytes([
                self.ogg_raw_buf[22],
                self.ogg_raw_buf[23],
                self.ogg_raw_buf[24],
                self.ogg_raw_buf[25],
            ]);
            let actual_crc = ogg_crc32(&self.ogg_raw_buf[..total_page_len]);
            if expected_crc != actual_crc {
                self.ogg_raw_buf.drain(..total_page_len);
                return Err(DecoderError::CorruptedFrame);
            }

            let header_type_flag = self.ogg_raw_buf[5];
            let is_continued = (header_type_flag & 0x01) != 0;
            if !is_continued && !self.ogg_partial_pkt.is_empty() {
                self.ogg_partial_pkt.clear();
            }

            let mut seg_table_buf = [0u8; 255];
            seg_table_buf[..n_segs].copy_from_slice(&self.ogg_raw_buf[27..page_hdr_len]);

            let mut body_off = page_hdr_len;
            for &seg_u8 in &seg_table_buf[..n_segs] {
                let seg_len = seg_u8 as usize;
                if self.ogg_partial_pkt.len().saturating_add(seg_len) > MAX_STREAM_BUF {
                    self.ogg_partial_pkt.clear();
                    self.ogg_raw_buf.drain(..total_page_len);
                    return Err(DecoderError::CorruptedFrame);
                }
                self.ogg_partial_pkt
                    .extend_from_slice(&self.ogg_raw_buf[body_off..body_off + seg_len]);
                body_off += seg_len;

                if seg_u8 < 255 {
                    if self.ogg_partial_pkt.starts_with(b"\x7fFLAC") {
                        if self.ogg_partial_pkt.len() < 51
                            || self.ogg_partial_pkt[5] != 1
                            || self.ogg_partial_pkt[6] != 0
                        {
                            self.ogg_partial_pkt.clear();
                            self.ogg_raw_buf.drain(..total_page_len);
                            return Err(DecoderError::InvalidHeader);
                        }
                        if self.header_len > 0 {
                            self.stream_buf.clear();
                            self.header_len = 0;
                        }
                        let flac_magic_needed = self.ogg_partial_pkt.len() >= 13
                            && &self.ogg_partial_pkt[9..13] != b"fLaC";
                        let extra_len = if flac_magic_needed { 4 } else { 0 };
                        if self.stream_buf.len() + (self.ogg_partial_pkt.len() - 9) + extra_len
                            > MAX_STREAM_BUF
                        {
                            self.flush();
                            return Err(DecoderError::CorruptedFrame);
                        }
                        if flac_magic_needed {
                            self.stream_buf.extend_from_slice(b"fLaC");
                        }
                        self.stream_buf
                            .extend_from_slice(&self.ogg_partial_pkt[9..]);
                    } else if !self.ogg_partial_pkt.is_empty() {
                        if self.stream_buf.len() + self.ogg_partial_pkt.len() > MAX_STREAM_BUF {
                            self.flush();
                            return Err(DecoderError::CorruptedFrame);
                        }
                        self.stream_buf.extend_from_slice(&self.ogg_partial_pkt);
                    }
                    self.ogg_partial_pkt.clear();
                }
            }

            self.ogg_raw_buf.drain(..total_page_len);
        }

        Ok(input_taken)
    }
}

#[cfg(any(feature = "alloc", feature = "std"))]
impl AudioDecoder for FlacDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let max_frames = pcm_out.len() / 2;
        if max_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        if self.pending_offset < self.pending_pcm.len() {
            let pending_samples = self.pending_pcm.len() - self.pending_offset;
            let pending_frames = pending_samples / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            let sample_count = to_copy * 2;
            pcm_out[..sample_count].copy_from_slice(
                &self.pending_pcm[self.pending_offset..self.pending_offset + sample_count],
            );
            self.pending_offset += sample_count;
            if self.pending_offset >= self.pending_pcm.len() {
                self.pending_pcm.clear();
                self.pending_offset = 0;
            }
            return Ok((0, to_copy));
        }

        if self.header_len > 0
            && (encoded_in.starts_with(b"fLaC")
                || encoded_in.starts_with(b"\x7fFLAC")
                || encoded_in.starts_with(b"OggS"))
        {
            self.stream_buf.clear();
            self.header_len = 0;
            self.is_ogg_flac = None;
        }

        if self.is_ogg_flac.is_none() {
            let total_avail = self.ogg_raw_buf.len() + self.stream_buf.len() + encoded_in.len();
            if total_avail < 4 {
                return Err(DecoderError::NeedMoreData);
            }
            let mut probe = [0u8; 4];
            let mut filled = 0usize;
            for &b in self
                .ogg_raw_buf
                .iter()
                .chain(self.stream_buf.iter())
                .chain(encoded_in.iter())
            {
                probe[filled] = b;
                filled += 1;
                if filled == 4 {
                    break;
                }
            }
            self.is_ogg_flac = Some(&probe == b"OggS");
        }

        let input_taken = if self.is_ogg_flac == Some(true) {
            let taken = self.ingest_ogg_flac_bytes(encoded_in)?;
            if self.stream_buf.len() < 4 {
                self.need_more_input = true;
                if taken > 0 {
                    return Ok((taken, 0));
                }
                return Err(DecoderError::NeedMoreData);
            }
            taken
        } else {
            if self.stream_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
                self.stream_buf.clear();
                self.header_len = 0;
                self.pending_pcm.clear();
                self.pending_offset = 0;
                self.meta_skip_remaining = 0;
                self.in_metadata = false;
                return Err(DecoderError::CorruptedFrame);
            }

            if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
                return Err(DecoderError::NeedMoreData);
            }

            let taken = encoded_in.len();
            self.stream_buf.extend_from_slice(encoded_in);
            taken
        };

        if self.meta_skip_remaining > 0 {
            let start = self.header_len;
            let avail = self.stream_buf.len().saturating_sub(start);
            let to_skip = avail.min(self.meta_skip_remaining);
            self.stream_buf.drain(start..start + to_skip);
            self.meta_skip_remaining -= to_skip;
            if self.meta_skip_remaining > 0 {
                self.need_more_input = true;
                return Ok((input_taken, 0));
            }
        }

        if self.header_len == 0 {
            if let Some(pos) = self.stream_buf.windows(4).position(|w| w == b"fLaC") {
                if pos > 0 {
                    self.stream_buf.drain(..pos);
                }
                if self.stream_buf.len() < 42 {
                    self.need_more_input = true;
                    if input_taken > 0 {
                        return Ok((input_taken, 0));
                    } else {
                        return Err(DecoderError::NeedMoreData);
                    }
                }
                let b0 = self.stream_buf[4];
                let is_last = (b0 & 0x80) != 0;
                let block_type = b0 & 0x7F;
                let block_len = ((self.stream_buf[5] as usize) << 16)
                    | ((self.stream_buf[6] as usize) << 8)
                    | (self.stream_buf[7] as usize);
                if block_type != 0 || block_len != 34 {
                    return Err(DecoderError::InvalidHeader);
                }
                let si = &self.stream_buf[8..42];
                self.min_block_size = u16::from_be_bytes([si[0], si[1]]);
                self.max_block_size = u16::from_be_bytes([si[2], si[3]]);
                let b10 = si[10] as u32;
                let b11 = si[11] as u32;
                let b12 = si[12] as u32;
                let b13 = si[13] as u32;
                self.sample_rate = (b10 << 12) | (b11 << 4) | (b12 >> 4);
                self.channels = ((b12 >> 1) & 0x07) + 1;
                self.bits_per_sample = (((b12 & 1) << 4) | (b13 >> 4)) + 1;

                if self.channels != 1 && self.channels != 2 {
                    return Err(DecoderError::UnsupportedFeature);
                }
                if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate) {
                    return Err(DecoderError::UnsupportedFeature);
                }
                if self.max_block_size as usize > MAX_FLAC_BLOCK_SAMPLES {
                    return Err(DecoderError::UnsupportedFeature);
                }

                self.header_len = 42;
                self.in_metadata = !is_last;
            }
        }

        while self.in_metadata {
            if self.stream_buf.len() < self.header_len + 4 {
                self.need_more_input = true;
                if input_taken > 0 {
                    return Ok((input_taken, 0));
                } else {
                    return Err(DecoderError::NeedMoreData);
                }
            }
            let b0 = self.stream_buf[self.header_len];
            let is_last = (b0 & 0x80) != 0;
            let block_type = b0 & 0x7F;
            // Allow secondary metadata blocks (1..=6) to be skipped cleanly
            // (AUD12-LOGIC-004)
            if block_type == 0 || block_type > 6 {
                return Err(DecoderError::InvalidHeader);
            }
            let block_len = ((self.stream_buf[self.header_len + 1] as usize) << 16)
                | ((self.stream_buf[self.header_len + 2] as usize) << 8)
                | (self.stream_buf[self.header_len + 3] as usize);
            let total_block_bytes = 4 + block_len;
            let avail = self.stream_buf.len() - self.header_len;
            if avail >= total_block_bytes {
                self.stream_buf
                    .drain(self.header_len..self.header_len + total_block_bytes);
                if is_last {
                    self.in_metadata = false;
                }
            } else {
                self.stream_buf.drain(self.header_len..);
                self.meta_skip_remaining = total_block_bytes - avail;
                self.in_metadata = !is_last;
                self.need_more_input = true;
                return Ok((input_taken, 0));
            }
        }

        let mut frames_out = 0;
        let mut corrupted_block = false;

        loop {
            if frames_out >= max_frames {
                break;
            }
            let avail_bytes = self.stream_buf.len().saturating_sub(self.header_len);
            if avail_bytes < 4 {
                self.need_more_input = true;
                break;
            }
            let buf = &self.stream_buf[self.header_len..];

            let mut sync_pos = None;
            for i in 0..buf.len() - 1 {
                if buf[i] == 0xFF && (buf[i + 1] & 0xFE) == 0xF8 {
                    sync_pos = Some(i);
                    break;
                }
            }

            let sync_idx = match sync_pos {
                Some(idx) => idx,
                None => {
                    self.need_more_input = true;
                    break;
                }
            };

            if sync_idx > 0 {
                self.stream_buf
                    .drain(self.header_len..self.header_len + sync_idx);
            }

            let frame_buf = &self.stream_buf[self.header_len..];
            if frame_buf.len() < 4 {
                self.need_more_input = true;
                break;
            }

            let hdr = match parse_frame_header(
                frame_buf,
                self.sample_rate,
                self.bits_per_sample,
                self.min_block_size,
            ) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    self.need_more_input = true;
                    break;
                }
                Err(()) => {
                    self.stream_buf.drain(self.header_len..self.header_len + 1);
                    corrupted_block = true;
                    continue;
                }
            };

            self.sample_rate = hdr.sample_rate;
            self.bits_per_sample = hdr.bits_per_sample;
            self.channels = hdr.channels;

            let mut reader = BitReader::new(&frame_buf[hdr.header_bytes..]);
            let mut subframe_err = false;
            let mut subframe_incomplete = false;

            for ch in 0..hdr.channels as usize {
                let subframe_bps = match hdr.channel_assignment {
                    8 if ch == 1 => hdr.bits_per_sample + 1,
                    9 if ch == 0 => hdr.bits_per_sample + 1,
                    10 if ch == 1 => hdr.bits_per_sample + 1,
                    _ => hdr.bits_per_sample,
                };
                let ch_start = ch * MAX_FLAC_BLOCK_SAMPLES;
                let ch_slice = &mut self.subframe_scratch[ch_start..ch_start + hdr.block_size];
                if let Err(()) =
                    decode_subframe(&mut reader, subframe_bps, hdr.block_size, ch_slice)
                {
                    if reader.byte_pos >= reader.bytes.len() {
                        subframe_incomplete = true;
                    } else {
                        subframe_err = true;
                    }
                    break;
                }
            }

            if subframe_incomplete {
                self.need_more_input = true;
                break;
            }
            if subframe_err {
                self.stream_buf.drain(self.header_len..self.header_len + 1);
                corrupted_block = true;
                continue;
            }

            reader.align_to_byte();
            let frame_body_len = hdr.header_bytes + reader.byte_pos;
            if frame_buf.len() < frame_body_len + 2 {
                self.need_more_input = true;
                break;
            }

            let expected_crc =
                u16::from_be_bytes([frame_buf[frame_body_len], frame_buf[frame_body_len + 1]]);
            let actual_crc = flac_crc16(&frame_buf[0..frame_body_len]);
            if expected_crc != actual_crc {
                self.stream_buf.drain(self.header_len..self.header_len + 1);
                corrupted_block = true;
                continue;
            }

            let total_frame_bytes = frame_body_len + 2;
            let shift = Self::sample_shift(self.bits_per_sample);
            let block_size = hdr.block_size;
            let available_space = max_frames.saturating_sub(frames_out);
            let to_write = core::cmp::min(block_size, available_space);

            for i in 0..to_write {
                let (l, r) =
                    decorrelate_samples(&self.subframe_scratch, i, hdr.channel_assignment, shift);
                pcm_out[(frames_out + i) * 2] = l;
                pcm_out[(frames_out + i) * 2 + 1] = r;
            }
            frames_out += to_write;

            self.pending_pcm.reserve((block_size - to_write) * 2);
            for i in to_write..block_size {
                let (l, r) =
                    decorrelate_samples(&self.subframe_scratch, i, hdr.channel_assignment, shift);
                self.pending_pcm.push(l);
                self.pending_pcm.push(r);
            }

            self.stream_buf
                .drain(self.header_len..self.header_len + total_frame_bytes);
            self.need_more_input = false;

            if !self.pending_pcm.is_empty() {
                break;
            }
        }

        if corrupted_block && frames_out == 0 {
            return Err(DecoderError::CorruptedFrame);
        }

        if frames_out == 0 && input_taken == 0 {
            self.need_more_input = true;
            Err(DecoderError::NeedMoreData)
        } else {
            Ok((input_taken, frames_out))
        }
    }

    fn flush(&mut self) {
        self.flush();
    }
}

/// Standard ISO/IEC 14496-3 / 13818-7 ADTS sampling frequency table (indices
/// `0..13`).
pub const AAC_SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

/// Parsed ISO-BMFF `esds` `AudioSpecificConfig` metadata for ADTS header
/// synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AacAudioSpecificConfig {
    /// Audio Object Type (`1` = Main, `2` = LC, `3` = SSR, `4` = LTP,
    /// `5` = SBR/HE-AAC).
    pub audio_object_type: u8,
    /// 2-bit ADTS profile field (`(base_aot - 1) & 0x03`, `1` for AAC-LC).
    pub adts_profile: u8,
    /// 4-bit ADTS sampling frequency index (`0..=11` for `8000..=96000` Hz).
    pub sampling_frequency_index: u8,
    /// 4-bit channel configuration (`1` = mono, `2` = stereo).
    pub channel_configuration: u8,
    /// Resolved sample rate in Hz.
    pub sample_rate: u32,
}

fn read_mp4_descr_len(buf: &[u8], mut idx: usize) -> Option<(usize, usize)> {
    let mut len = 0usize;
    for _ in 0..4 {
        if idx >= buf.len() {
            return None;
        }
        let b = buf[idx];
        idx += 1;
        len = (len << 7) | ((b & 0x7F) as usize);
        if (b & 0x80) == 0 {
            return Some((len, idx));
        }
    }
    None
}

struct AscBitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> AscBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn read_bits(&mut self, n: usize) -> Option<u32> {
        if n == 0 || n > 32 {
            return None;
        }
        if self.bit_pos.saturating_add(n) > self.data.len() * 8 {
            return None;
        }
        let mut val = 0u32;
        for _ in 0..n {
            let byte_idx = self.bit_pos / 8;
            let bit_idx = 7 - (self.bit_pos % 8);
            let bit = ((self.data[byte_idx] >> bit_idx) & 1) as u32;
            val = (val << 1) | bit;
            self.bit_pos += 1;
        }
        Some(val)
    }
}

fn decode_asc_bytes(asc: &[u8]) -> Result<AacAudioSpecificConfig, DecoderError> {
    let mut reader = AscBitReader::new(asc);
    let mut audio_object_type = reader.read_bits(5).ok_or(DecoderError::InvalidHeader)?;
    if audio_object_type == 31 {
        let ext = reader.read_bits(6).ok_or(DecoderError::InvalidHeader)?;
        audio_object_type = 32 + ext;
    }
    if audio_object_type == 0 {
        return Err(DecoderError::InvalidHeader);
    }
    if audio_object_type > 5 {
        return Err(DecoderError::UnsupportedFeature);
    }
    if audio_object_type == 5 {
        let ext_freq_idx = reader.read_bits(4).ok_or(DecoderError::InvalidHeader)?;
        if ext_freq_idx == 0x0F {
            let _ = reader.read_bits(24).ok_or(DecoderError::InvalidHeader)?;
        }
        let base_aot = reader.read_bits(5).ok_or(DecoderError::InvalidHeader)?;
        if base_aot == 0 || base_aot > 4 {
            return Err(DecoderError::UnsupportedFeature);
        }
    }
    let freq_idx = reader.read_bits(4).ok_or(DecoderError::InvalidHeader)?;
    let (sample_rate, sampling_frequency_index) = if freq_idx == 0x0F {
        let sr = reader.read_bits(24).ok_or(DecoderError::InvalidHeader)?;
        let idx = AAC_SAMPLE_RATES
            .iter()
            .position(|&r| r == sr)
            .map(|i| i as u8)
            .unwrap_or(0x0F);
        (sr, idx)
    } else if (freq_idx as usize) < AAC_SAMPLE_RATES.len() {
        (AAC_SAMPLE_RATES[freq_idx as usize], freq_idx as u8)
    } else {
        return Err(DecoderError::UnsupportedFeature);
    };
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(DecoderError::UnsupportedFeature);
    }
    let channel_configuration = reader.read_bits(4).ok_or(DecoderError::InvalidHeader)? as u8;
    if channel_configuration != 1 && channel_configuration != 2 {
        return Err(DecoderError::UnsupportedFeature);
    }
    let adts_profile = if audio_object_type == 5 {
        1
    } else {
        ((audio_object_type.saturating_sub(1)) & 0x03) as u8
    };
    Ok(AacAudioSpecificConfig {
        audio_object_type: audio_object_type as u8,
        adts_profile,
        sampling_frequency_index,
        channel_configuration,
        sample_rate,
    })
}

/// Parses an ISO-BMFF `esds` box payload (after the 8-byte box header) and
/// extracts the `AudioSpecificConfig` from `DecoderSpecificInfo` (tag `0x05`).
pub fn parse_esds_audio_specific_config(
    esds_payload: &[u8],
) -> Result<AacAudioSpecificConfig, DecoderError> {
    if esds_payload.len() < 6 {
        return Err(DecoderError::InvalidHeader);
    }
    let mut idx = if esds_payload.len() >= 4 && esds_payload[0] == 0 {
        4
    } else {
        0
    };

    while idx < esds_payload.len() {
        let tag = esds_payload[idx];
        idx += 1;
        let Some((descr_len, next_idx)) = read_mp4_descr_len(esds_payload, idx) else {
            break;
        };
        idx = next_idx;
        match tag {
            0x03 => {
                if idx + 3 > esds_payload.len() {
                    return Err(DecoderError::InvalidHeader);
                }
                let flags = esds_payload[idx + 2];
                idx += 3;
                if (flags & 0x80) != 0 {
                    idx += 2;
                }
                if (flags & 0x40) != 0 {
                    if idx >= esds_payload.len() {
                        return Err(DecoderError::InvalidHeader);
                    }
                    let url_len = esds_payload[idx] as usize;
                    idx += 1 + url_len;
                }
                if (flags & 0x20) != 0 {
                    idx += 2;
                }
            }
            0x04 => {
                if idx + 13 > esds_payload.len() {
                    return Err(DecoderError::InvalidHeader);
                }
                let obj_type = esds_payload[idx];
                if obj_type != 0x40 && obj_type != 0x66 && obj_type != 0x67 && obj_type != 0x68 {
                    return Err(DecoderError::UnsupportedFeature);
                }
                idx += 13;
            }
            0x05 => {
                if descr_len < 2 || idx + descr_len > esds_payload.len() {
                    return Err(DecoderError::InvalidHeader);
                }
                return decode_asc_bytes(&esds_payload[idx..idx + descr_len]);
            }
            _ => {
                idx = idx.saturating_add(descr_len);
            }
        }
    }

    for pos in 0..esds_payload.len().saturating_sub(2) {
        if esds_payload[pos] == 0x05 {
            if let Some((d_len, asc_start)) = read_mp4_descr_len(esds_payload, pos + 1) {
                if d_len >= 2 && asc_start + d_len <= esds_payload.len() {
                    return decode_asc_bytes(&esds_payload[asc_start..asc_start + d_len]);
                }
            }
        }
    }

    Err(DecoderError::InvalidHeader)
}

/// Synthesizes a 7-byte MPEG-4 ADTS header (`protection_absent = 1`) around a
/// raw `mdat` AAC access unit of `aac_payload_len` bytes.
#[must_use]
pub const fn synthesize_adts_header(
    asc: AacAudioSpecificConfig,
    aac_payload_len: usize,
) -> [u8; 7] {
    let raw_len = aac_payload_len.saturating_add(7);
    let full_len = if raw_len > 8191 { 8191 } else { raw_len };
    let b0 = 0xFF;
    let b1 = 0xF1;
    let b2 = ((asc.adts_profile & 0x03) << 6)
        | ((asc.sampling_frequency_index & 0x0F) << 2)
        | ((asc.channel_configuration >> 2) & 0x01);
    let b3 = ((asc.channel_configuration & 0x03) << 6) | (((full_len >> 11) as u8) & 0x03);
    let b4 = ((full_len >> 3) & 0xFF) as u8;
    let b5 = (((full_len & 0x07) as u8) << 5) | 0x1F;
    let b6 = 0xFC;
    [b0, b1, b2, b3, b4, b5, b6]
}

#[cfg(feature = "alloc")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Mp4DemuxState {
    SearchingAtoms,
    ReadingMdat,
    Eof,
}

#[cfg(feature = "alloc")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Mp4StscEntry {
    first_chunk: u32,
    samples_per_chunk: u32,
}

#[cfg(feature = "alloc")]
struct AacMp4Demuxer {
    state: Mp4DemuxState,
    stream_buf: vec::Vec<u8>,
    asc: Option<AacAudioSpecificConfig>,
    sample_sizes: vec::Vec<usize>,
    stsc_entries: vec::Vec<Mp4StscEntry>,
    chunk_offsets: vec::Vec<usize>,
    sample_idx: usize,
    current_chunk_idx: usize,
    sample_in_chunk_idx: usize,
    bytes_to_skip: usize,
    file_pos: usize,
    adts_frame_scratch: vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl AacMp4Demuxer {
    fn new() -> Self {
        Self {
            state: Mp4DemuxState::SearchingAtoms,
            stream_buf: vec::Vec::new(),
            asc: None,
            sample_sizes: vec::Vec::new(),
            stsc_entries: vec::Vec::new(),
            chunk_offsets: vec::Vec::new(),
            sample_idx: 0,
            current_chunk_idx: 0,
            sample_in_chunk_idx: 0,
            bytes_to_skip: 0,
            file_pos: 0,
            adts_frame_scratch: vec::Vec::new(),
        }
    }

    fn samples_in_chunk(&self, chunk_idx: usize) -> usize {
        if self.stsc_entries.is_empty() {
            return self.sample_sizes.len().max(1);
        }
        let chunk_num = (chunk_idx as u32).saturating_add(1);
        let mut spc = self.stsc_entries[0].samples_per_chunk;
        for entry in &self.stsc_entries {
            if entry.first_chunk <= chunk_num {
                spc = entry.samples_per_chunk;
            } else {
                break;
            }
        }
        (spc as usize).max(1)
    }
}

#[cfg(feature = "alloc")]
/// Pure-Rust AAC (ADTS / LC / HE-AAC & ISO-BMFF MP4/M4A `mp4a`/`esds`) Decoder
/// using syom.
///
/// Propagates exact consumed byte counts returned by syom (DEC-004).
/// Available with `alloc` (vendored `no_std + alloc` syom port, WO-199).
pub struct AacDecoder {
    decoder: syom::Decoder,
    pub sample_rate: u32,
    pub channels: u32,
    pending_pcm: vec::Vec<i32>,
    pending_offset: usize,
    is_mp4: Option<bool>,
    mp4: Option<Box<AacMp4Demuxer>>,
}

#[cfg(feature = "alloc")]
impl Default for AacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl AacDecoder {
    pub fn new() -> Self {
        Self {
            decoder: syom::Decoder::new(syom::DecodeOptions::audio()),
            sample_rate: 44100,
            channels: 2,
            pending_pcm: vec::Vec::new(),
            pending_offset: 0,
            is_mp4: None,
            mp4: None,
        }
    }

    pub fn has_pending_pcm(&self) -> bool {
        if self.pending_offset < self.pending_pcm.len() {
            return true;
        }
        if let Some(ref demux) = self.mp4 {
            if demux.state == Mp4DemuxState::ReadingMdat
                && demux.bytes_to_skip == 0
                && demux.sample_idx < demux.sample_sizes.len()
                && demux.stream_buf.len() >= demux.sample_sizes[demux.sample_idx]
            {
                return true;
            }
        }
        false
    }

    /// Returns the parsed ISO-BMFF `esds` `AudioSpecificConfig` when decoding
    /// an MP4/M4A stream.
    #[must_use]
    pub fn audio_specific_config(&self) -> Option<AacAudioSpecificConfig> {
        self.mp4.as_ref().and_then(|d| d.asc)
    }

    pub fn flush(&mut self) {
        self.decoder.reset();
        self.pending_pcm.clear();
        self.pending_offset = 0;
        self.is_mp4 = None;
        self.mp4 = None;
    }

    fn decode_mp4_stream(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
        mut frames_out: usize,
        max_frames: usize,
    ) -> Result<(usize, usize), DecoderError> {
        if self.mp4.is_none() {
            self.mp4 = Some(Box::new(AacMp4Demuxer::new()));
        }
        let Some(ref mut demux) = self.mp4 else {
            return Err(DecoderError::InvalidHeader);
        };

        let max_buf = if demux.state == Mp4DemuxState::ReadingMdat {
            MAX_STREAM_BUF
        } else {
            MAX_MP4_META_BUF
        };

        if demux.stream_buf.len().saturating_add(encoded_in.len()) > max_buf {
            self.flush();
            return Err(DecoderError::CorruptedFrame);
        }

        if demux.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        demux.stream_buf.extend_from_slice(encoded_in);

        if demux.bytes_to_skip > 0 {
            let to_skip = core::cmp::min(demux.bytes_to_skip, demux.stream_buf.len());
            demux.stream_buf.drain(..to_skip);
            demux.bytes_to_skip -= to_skip;
            demux.file_pos = demux.file_pos.saturating_add(to_skip);
            if demux.bytes_to_skip > 0 {
                if frames_out == 0 && input_taken == 0 {
                    return Err(DecoderError::NeedMoreData);
                }
                return Ok((input_taken, frames_out));
            }
        }

        while demux.state == Mp4DemuxState::SearchingAtoms {
            if demux.bytes_to_skip > 0 {
                let to_skip = core::cmp::min(demux.bytes_to_skip, demux.stream_buf.len());
                demux.stream_buf.drain(..to_skip);
                demux.bytes_to_skip -= to_skip;
                demux.file_pos = demux.file_pos.saturating_add(to_skip);
                if demux.bytes_to_skip > 0 {
                    break;
                }
            }

            if demux.stream_buf.len() < 8 {
                break;
            }

            let len = u32::from_be_bytes([
                demux.stream_buf[0],
                demux.stream_buf[1],
                demux.stream_buf[2],
                demux.stream_buf[3],
            ]) as usize;

            let fourcc = [
                demux.stream_buf[4],
                demux.stream_buf[5],
                demux.stream_buf[6],
                demux.stream_buf[7],
            ];

            match &fourcc {
                b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"wave" => {
                    demux.stream_buf.drain(..8);
                    demux.file_pos = demux.file_pos.saturating_add(8);
                }
                b"stsd" => {
                    if demux.stream_buf.len() < 16 {
                        break;
                    }
                    demux.stream_buf.drain(..16);
                    demux.file_pos = demux.file_pos.saturating_add(16);
                }
                b"mp4a" => {
                    if len < 36 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < 36 {
                        break;
                    }
                    let version = u16::from_be_bytes([demux.stream_buf[16], demux.stream_buf[17]]);
                    let hdr_size = match version {
                        1 if len > 52 => 52,
                        2 if len > 72 => 72,
                        _ => 36,
                    };
                    if demux.stream_buf.len() < hdr_size {
                        break;
                    }
                    demux.stream_buf.drain(..hdr_size);
                    demux.file_pos = demux.file_pos.saturating_add(hdr_size);
                }
                b"esds" => {
                    if len < 14 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < len {
                        break;
                    }
                    let asc = parse_esds_audio_specific_config(&demux.stream_buf[8..len])?;
                    self.sample_rate = asc.sample_rate;
                    self.channels = asc.channel_configuration as u32;
                    demux.asc = Some(asc);
                    demux.stream_buf.drain(..len);
                    demux.file_pos = demux.file_pos.saturating_add(len);
                }
                b"stsz" => {
                    if len < 20 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < len {
                        break;
                    }
                    let default_sz = u32::from_be_bytes([
                        demux.stream_buf[12],
                        demux.stream_buf[13],
                        demux.stream_buf[14],
                        demux.stream_buf[15],
                    ]) as usize;
                    let count = u32::from_be_bytes([
                        demux.stream_buf[16],
                        demux.stream_buf[17],
                        demux.stream_buf[18],
                        demux.stream_buf[19],
                    ]) as usize;
                    if count > MAX_MP4_SAMPLES {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    demux.sample_sizes.clear();
                    if default_sz == 0 {
                        if len < 20 + count * 4 {
                            return Err(DecoderError::InvalidHeader);
                        }
                        demux.sample_sizes.reserve(count);
                        for i in 0..count {
                            let off = 20 + i * 4;
                            let sz = u32::from_be_bytes([
                                demux.stream_buf[off],
                                demux.stream_buf[off + 1],
                                demux.stream_buf[off + 2],
                                demux.stream_buf[off + 3],
                            ]) as usize;
                            demux.sample_sizes.push(sz);
                        }
                    } else {
                        demux.sample_sizes = vec![default_sz; count];
                    }
                    demux.stream_buf.drain(..len);
                    demux.file_pos = demux.file_pos.saturating_add(len);
                }
                b"stsc" => {
                    if len < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < len {
                        break;
                    }
                    let count = u32::from_be_bytes([
                        demux.stream_buf[12],
                        demux.stream_buf[13],
                        demux.stream_buf[14],
                        demux.stream_buf[15],
                    ]) as usize;
                    if count > MAX_MP4_SAMPLES {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    if len < 16 + count * 12 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    demux.stsc_entries.clear();
                    demux.stsc_entries.reserve(count);
                    for i in 0..count {
                        let off = 16 + i * 12;
                        let first_chunk = u32::from_be_bytes([
                            demux.stream_buf[off],
                            demux.stream_buf[off + 1],
                            demux.stream_buf[off + 2],
                            demux.stream_buf[off + 3],
                        ]);
                        let samples_per_chunk = u32::from_be_bytes([
                            demux.stream_buf[off + 4],
                            demux.stream_buf[off + 5],
                            demux.stream_buf[off + 6],
                            demux.stream_buf[off + 7],
                        ]);
                        demux.stsc_entries.push(Mp4StscEntry {
                            first_chunk,
                            samples_per_chunk,
                        });
                    }
                    demux.stream_buf.drain(..len);
                    demux.file_pos = demux.file_pos.saturating_add(len);
                }
                b"stco" => {
                    if len < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < len {
                        break;
                    }
                    let count = u32::from_be_bytes([
                        demux.stream_buf[12],
                        demux.stream_buf[13],
                        demux.stream_buf[14],
                        demux.stream_buf[15],
                    ]) as usize;
                    if count > MAX_MP4_SAMPLES {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    if len < 16 + count * 4 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    demux.chunk_offsets.clear();
                    demux.chunk_offsets.reserve(count);
                    for i in 0..count {
                        let off = 16 + i * 4;
                        let chunk_off = u32::from_be_bytes([
                            demux.stream_buf[off],
                            demux.stream_buf[off + 1],
                            demux.stream_buf[off + 2],
                            demux.stream_buf[off + 3],
                        ]) as usize;
                        demux.chunk_offsets.push(chunk_off);
                    }
                    demux.stream_buf.drain(..len);
                    demux.file_pos = demux.file_pos.saturating_add(len);
                }
                b"co64" => {
                    if len < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() < len {
                        break;
                    }
                    let count = u32::from_be_bytes([
                        demux.stream_buf[12],
                        demux.stream_buf[13],
                        demux.stream_buf[14],
                        demux.stream_buf[15],
                    ]) as usize;
                    if count > MAX_MP4_SAMPLES {
                        return Err(DecoderError::UnsupportedFeature);
                    }
                    if len < 16 + count * 8 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    demux.chunk_offsets.clear();
                    demux.chunk_offsets.reserve(count);
                    for i in 0..count {
                        let off = 16 + i * 8;
                        let chunk_off = u64::from_be_bytes([
                            demux.stream_buf[off],
                            demux.stream_buf[off + 1],
                            demux.stream_buf[off + 2],
                            demux.stream_buf[off + 3],
                            demux.stream_buf[off + 4],
                            demux.stream_buf[off + 5],
                            demux.stream_buf[off + 6],
                            demux.stream_buf[off + 7],
                        ]) as usize;
                        demux.chunk_offsets.push(chunk_off);
                    }
                    demux.stream_buf.drain(..len);
                    demux.file_pos = demux.file_pos.saturating_add(len);
                }
                b"mdat" => {
                    if demux.asc.is_none() || demux.sample_sizes.is_empty() {
                        return Err(DecoderError::InvalidHeader);
                    }
                    let header_len = if len == 1 { 16 } else { 8 };
                    if demux.stream_buf.len() < header_len {
                        break;
                    }
                    demux.stream_buf.drain(..header_len);
                    demux.file_pos = demux.file_pos.saturating_add(header_len);
                    if let Some(&first_off) = demux.chunk_offsets.first() {
                        if first_off > demux.file_pos {
                            demux.bytes_to_skip = first_off.saturating_sub(demux.file_pos);
                            let to_skip =
                                core::cmp::min(demux.bytes_to_skip, demux.stream_buf.len());
                            demux.stream_buf.drain(..to_skip);
                            demux.bytes_to_skip -= to_skip;
                            demux.file_pos = demux.file_pos.saturating_add(to_skip);
                        }
                    }
                    demux.state = Mp4DemuxState::ReadingMdat;
                }
                _ => {
                    if len < 8 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if demux.stream_buf.len() >= len {
                        demux.stream_buf.drain(..len);
                        demux.file_pos = demux.file_pos.saturating_add(len);
                    } else {
                        let avail = demux.stream_buf.len();
                        demux.file_pos = demux.file_pos.saturating_add(avail);
                        demux.bytes_to_skip = len.saturating_sub(avail);
                        demux.stream_buf.clear();
                        break;
                    }
                }
            }
        }

        if demux.state == Mp4DemuxState::ReadingMdat {
            let Some(asc) = demux.asc else {
                return Err(DecoderError::InvalidHeader);
            };
            while frames_out < max_frames
                && demux.sample_idx < demux.sample_sizes.len()
                && demux.bytes_to_skip == 0
            {
                let pkt_len = demux.sample_sizes[demux.sample_idx];
                if pkt_len == 0 || pkt_len + 7 >= 8192 {
                    demux.sample_idx += 1;
                    let drain_len = pkt_len.min(demux.stream_buf.len());
                    demux.stream_buf.drain(..drain_len);
                    return Err(DecoderError::CorruptedFrame);
                }
                if demux.stream_buf.len() < pkt_len {
                    break;
                }
                let adts_hdr = synthesize_adts_header(asc, pkt_len);
                demux.adts_frame_scratch.clear();
                demux.adts_frame_scratch.reserve(7 + pkt_len);
                demux.adts_frame_scratch.extend_from_slice(&adts_hdr);
                demux
                    .adts_frame_scratch
                    .extend_from_slice(&demux.stream_buf[..pkt_len]);

                let pending_pcm = &mut self.pending_pcm;
                let sample_rate = &mut self.sample_rate;
                let channels = &mut self.channels;
                let feed_res = self.decoder.feed(&demux.adts_frame_scratch, |frame| {
                    *sample_rate = frame.sample_rate;
                    *channels = frame.planar.len() as u32;
                    if frame.planar.is_empty() {
                        return Ok(());
                    }
                    let left = frame.planar[0];
                    let right = if frame.planar.len() > 1 {
                        frame.planar[1]
                    } else {
                        left
                    };
                    let samples = core::cmp::min(left.len(), right.len());
                    for i in 0..samples {
                        let l_f = left[i].clamp(-1.0, 1.0);
                        let r_f = right[i].clamp(-1.0, 1.0);
                        let l = (l_f * 2147483520.0) as i32;
                        let r = (r_f * 2147483520.0) as i32;
                        if frames_out < max_frames {
                            pcm_out[frames_out * 2] = l;
                            pcm_out[frames_out * 2 + 1] = r;
                            frames_out += 1;
                        } else {
                            pending_pcm.push(l);
                            pending_pcm.push(r);
                        }
                    }
                    Ok(())
                });

                demux.stream_buf.drain(..pkt_len);
                demux.file_pos = demux.file_pos.saturating_add(pkt_len);
                demux.sample_idx += 1;
                demux.sample_in_chunk_idx += 1;

                let spc = demux.samples_in_chunk(demux.current_chunk_idx);
                if demux.sample_in_chunk_idx >= spc {
                    demux.current_chunk_idx += 1;
                    demux.sample_in_chunk_idx = 0;
                    if let Some(&next_off) = demux.chunk_offsets.get(demux.current_chunk_idx) {
                        if next_off > demux.file_pos {
                            demux.bytes_to_skip = next_off.saturating_sub(demux.file_pos);
                            let to_skip =
                                core::cmp::min(demux.bytes_to_skip, demux.stream_buf.len());
                            demux.stream_buf.drain(..to_skip);
                            demux.bytes_to_skip -= to_skip;
                            demux.file_pos = demux.file_pos.saturating_add(to_skip);
                        }
                    }
                }

                if feed_res.is_err() && frames_out == 0 {
                    return Err(DecoderError::CorruptedFrame);
                }

                if !(1..=2).contains(&self.channels)
                    || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate)
                {
                    return Err(DecoderError::UnsupportedSampleFormat);
                }
            }
            if demux.sample_idx >= demux.sample_sizes.len() && !demux.sample_sizes.is_empty() {
                demux.state = Mp4DemuxState::Eof;
            }
        }

        if frames_out == 0 && input_taken == 0 {
            Err(DecoderError::NeedMoreData)
        } else {
            Ok((input_taken, frames_out))
        }
    }
}

/// Pre-warms codec lazy statics into heap memory at startup to avoid runtime
/// latency/stack spikes.
#[cfg(feature = "alloc")]
pub fn warm_up_decoders() {
    syom::warm_up();
}

#[cfg(feature = "alloc")]
impl AudioDecoder for AacDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let max_frames = pcm_out.len() / 2;
        if max_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        let mut frames_out = 0;
        if self.pending_offset < self.pending_pcm.len() {
            let pending_samples = self.pending_pcm.len() - self.pending_offset;
            let pending_frames = pending_samples / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            let samples = to_copy * 2;
            pcm_out[..samples].copy_from_slice(
                &self.pending_pcm[self.pending_offset..self.pending_offset + samples],
            );
            self.pending_offset += samples;
            if self.pending_offset >= self.pending_pcm.len() {
                self.pending_pcm.clear();
                self.pending_offset = 0;
            }
            frames_out += to_copy;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        if self.is_mp4.is_none() {
            if encoded_in.len() >= 2 && encoded_in[0] == 0xFF && (encoded_in[1] & 0xF0) == 0xF0 {
                self.is_mp4 = Some(false);
            } else if encoded_in.len() >= 8 {
                let fourcc = &encoded_in[4..8];
                let is_box = fourcc == b"ftyp"
                    || fourcc == b"moov"
                    || fourcc == b"styp"
                    || fourcc == b"free"
                    || fourcc == b"skip"
                    || fourcc == b"wide"
                    || fourcc == b"mdat";
                self.is_mp4 = Some(is_box);
            } else if encoded_in.len() < 8 {
                if frames_out > 0 {
                    return Ok((0, frames_out));
                }
                return Err(DecoderError::NeedMoreData);
            } else {
                self.is_mp4 = Some(false);
            }
        }

        if self.is_mp4 == Some(true) {
            return self.decode_mp4_stream(encoded_in, pcm_out, frames_out, max_frames);
        }

        if encoded_in.len() < 7 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let pending_pcm = &mut self.pending_pcm;
        let sample_rate = &mut self.sample_rate;
        let channels = &mut self.channels;
        let mut unsupported_format = false;

        let feed_res = self.decoder.feed(encoded_in, |frame| {
            *sample_rate = frame.sample_rate;
            *channels = frame.planar.len() as u32;

            if *channels != 1 && *channels != 2 {
                unsupported_format = true;
                return Ok(());
            }
            if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(sample_rate) {
                unsupported_format = true;
                return Ok(());
            }

            if frame.planar.is_empty() {
                return Ok(());
            }

            let left = frame.planar[0];
            let right = if frame.planar.len() > 1 {
                frame.planar[1]
            } else {
                left
            };

            let samples = core::cmp::min(left.len(), right.len());
            let direct_count = core::cmp::min(samples, max_frames - frames_out);
            for i in 0..direct_count {
                let l_f = left[i].clamp(-1.0, 1.0);
                let r_f = right[i].clamp(-1.0, 1.0);
                // Clamping boundary to avoid overflow (DSP-003)
                let l = (l_f * 2147483520.0) as i32;
                let r = (r_f * 2147483520.0) as i32;
                pcm_out[(frames_out + i) * 2] = l;
                pcm_out[(frames_out + i) * 2 + 1] = r;
            }
            frames_out += direct_count;

            if direct_count < samples {
                pending_pcm.reserve((samples - direct_count) * 2);
                for i in direct_count..samples {
                    let l_f = left[i].clamp(-1.0, 1.0);
                    let r_f = right[i].clamp(-1.0, 1.0);
                    // Clamping boundary to avoid overflow (DSP-003)
                    let l = (l_f * 2147483520.0) as i32;
                    let r = (r_f * 2147483520.0) as i32;
                    pending_pcm.push(l);
                    pending_pcm.push(r);
                }
            }

            Ok(())
        });

        if unsupported_format {
            return Err(DecoderError::UnsupportedFeature);
        }

        match feed_res {
            Ok(consumed) => {
                if frames_out == 0 && consumed == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((consumed, frames_out))
                }
            }
            Err(_) => {
                if frames_out > 0 {
                    Ok((0, frames_out))
                } else {
                    Err(DecoderError::CorruptedFrame)
                }
            }
        }
    }

    fn flush(&mut self) {
        self.flush();
    }
}

#[cfg(feature = "alloc")]
enum VorbisState {
    WaitingIdent,
    WaitingComment {
        ident: Box<lewton::header::IdentHeader>,
    },
    WaitingSetup {
        ident: Box<lewton::header::IdentHeader>,
        _comment: Box<lewton::header::CommentHeader>,
    },
    Ready {
        ident: Box<lewton::header::IdentHeader>,
        setup: Box<lewton::header::SetupHeader>,
        pwr: lewton::audio::PreviousWindowRight,
    },
    Poisoned,
}

#[cfg(feature = "alloc")]
/// Pure-Rust Ogg Vorbis Decoder using lewton with in-memory Ogg page
/// demuxing (DEC-003).
///
/// Promoted to `alloc` (vendored `no_std + alloc` lewton port, WO-203).
pub struct VorbisDecoder {
    pub sample_rate: u32,
    pub channels: u32,
    pub packets_decoded: usize,
    stream_buf: Vec<u8>,
    partial_packet: Vec<u8>,
    packet_queue: VecDeque<Vec<u8>>,
    pending_pcm: Vec<i32>,
    pending_offset: usize,
    state: VorbisState,
}

#[cfg(feature = "alloc")]
impl Default for VorbisDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl VorbisDecoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            packets_decoded: 0,
            stream_buf: Vec::new(),
            partial_packet: Vec::new(),
            packet_queue: VecDeque::new(),
            pending_pcm: Vec::new(),
            pending_offset: 0,
            state: VorbisState::WaitingIdent,
        }
    }

    pub fn has_pending_pcm(&self) -> bool {
        self.pending_offset < self.pending_pcm.len()
            || !self.packet_queue.is_empty()
            || self.stream_buf.len() >= 27
    }

    pub fn flush(&mut self) {
        self.pending_pcm.clear();
        self.pending_offset = 0;
        self.packet_queue.clear();
        self.partial_packet.clear();
        self.stream_buf.clear();
        self.packets_decoded = 0;
        if let VorbisState::Ready { ident, setup, .. } =
            core::mem::replace(&mut self.state, VorbisState::WaitingIdent)
        {
            self.state = VorbisState::Ready {
                ident,
                setup,
                pwr: lewton::audio::PreviousWindowRight::new(),
            };
        }
    }

    fn decode_single_packet(
        &mut self,
        packet: &[u8],
        pcm_out: &mut [i32],
        frames_out: &mut usize,
        max_frames: usize,
    ) -> Result<(), DecoderError> {
        if packet.is_empty() {
            return Ok(());
        }

        // Header packets have LSB 1; audio packets have LSB 0 (Vorbis I spec §1.3.1)
        let is_header = (packet[0] & 1) != 0;

        if is_header {
            if packet.starts_with(b"\x01vorbis") {
                let ident = lewton::header::read_header_ident(packet)
                    .map_err(|_| DecoderError::InvalidHeader)?;
                if ident.audio_channels != 1 && ident.audio_channels != 2 {
                    return Err(DecoderError::UnsupportedFeature);
                }
                if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&ident.audio_sample_rate) {
                    return Err(DecoderError::UnsupportedFeature);
                }
                self.sample_rate = ident.audio_sample_rate;
                self.channels = ident.audio_channels as u32;
                self.state = VorbisState::WaitingComment {
                    ident: Box::new(ident),
                };
                return Ok(());
            } else if packet.starts_with(b"\x03vorbis") {
                let comment = lewton::header::read_header_comment(packet)
                    .map_err(|_| DecoderError::InvalidHeader)?;
                let ident = match core::mem::replace(&mut self.state, VorbisState::Poisoned) {
                    VorbisState::WaitingComment { ident } => ident,
                    VorbisState::WaitingSetup { ident, .. } => ident,
                    VorbisState::Ready { ident, .. } => ident,
                    _ => return Err(DecoderError::InvalidHeader),
                };
                self.state = VorbisState::WaitingSetup {
                    ident,
                    _comment: Box::new(comment),
                };
                return Ok(());
            } else if packet.starts_with(b"\x05vorbis") {
                let ident = match core::mem::replace(&mut self.state, VorbisState::Poisoned) {
                    VorbisState::WaitingSetup { ident, .. } => ident,
                    VorbisState::Ready { ident, .. } => ident,
                    _ => return Err(DecoderError::InvalidHeader),
                };
                let setup = lewton::header::read_header_setup(
                    packet,
                    ident.audio_channels,
                    (ident.blocksize_0, ident.blocksize_1),
                )
                .map_err(|_| DecoderError::InvalidHeader)?;
                self.state = VorbisState::Ready {
                    ident,
                    setup: Box::new(setup),
                    pwr: lewton::audio::PreviousWindowRight::new(),
                };
                return Ok(());
            } else {
                return Err(DecoderError::InvalidHeader);
            }
        }

        // Audio packet (LSB 0)
        let state = core::mem::replace(&mut self.state, VorbisState::Poisoned);
        match state {
            VorbisState::Ready {
                ident,
                setup,
                mut pwr,
            } => {
                let decode_res: Result<lewton::samples::InterleavedSamples<f32>, _> =
                    lewton::audio::read_audio_packet_generic(&ident, &setup, packet, &mut pwr);
                self.state = VorbisState::Ready { ident, setup, pwr };

                match decode_res {
                    Ok(decoded) => {
                        self.packets_decoded += 1;
                        let ch = self.channels as usize;
                        if ch == 0 {
                            return Err(DecoderError::UnsupportedSampleFormat);
                        }
                        let packet_frames = decoded.samples.len() / ch;
                        if packet_frames == 0 {
                            return Ok(());
                        }

                        let available = max_frames.saturating_sub(*frames_out);
                        let to_copy = core::cmp::min(packet_frames, available);
                        let to_i32 = |s: f32| {
                            let scaled = s.clamp(-1.0, 1.0) * 2147483520.0;
                            if scaled >= 0.0 {
                                (scaled + 0.5) as i32
                            } else {
                                (scaled - 0.5) as i32
                            }
                        };

                        if ch == 2 {
                            for i in 0..to_copy {
                                pcm_out[(*frames_out + i) * 2] = to_i32(decoded.samples[i * 2]);
                                pcm_out[(*frames_out + i) * 2 + 1] =
                                    to_i32(decoded.samples[i * 2 + 1]);
                            }
                            if to_copy < packet_frames {
                                for i in to_copy..packet_frames {
                                    self.pending_pcm.push(to_i32(decoded.samples[i * 2]));
                                    self.pending_pcm.push(to_i32(decoded.samples[i * 2 + 1]));
                                }
                            }
                        } else {
                            for i in 0..to_copy {
                                let s = to_i32(decoded.samples[i]);
                                pcm_out[(*frames_out + i) * 2] = s;
                                pcm_out[(*frames_out + i) * 2 + 1] = s;
                            }
                            if to_copy < packet_frames {
                                for i in to_copy..packet_frames {
                                    let s = to_i32(decoded.samples[i]);
                                    self.pending_pcm.push(s);
                                    self.pending_pcm.push(s);
                                }
                            }
                        }
                        *frames_out += to_copy;
                        Ok(())
                    }
                    Err(_) => {
                        // Skip corrupt frame in continuous stream
                        Ok(())
                    }
                }
            }
            other => {
                self.state = other;
                Ok(())
            }
        }
    }
}

#[cfg(feature = "alloc")]
impl AudioDecoder for VorbisDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let max_frames = pcm_out.len() / 2;
        if max_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        let mut frames_out = 0;

        // 1. Copy pending PCM from prior decode
        if self.pending_offset < self.pending_pcm.len() {
            let pending_samples = self.pending_pcm.len() - self.pending_offset;
            let pending_frames = pending_samples / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            let samples = to_copy * 2;
            pcm_out[..samples].copy_from_slice(
                &self.pending_pcm[self.pending_offset..self.pending_offset + samples],
            );
            self.pending_offset += samples;
            if self.pending_offset >= self.pending_pcm.len() {
                self.pending_pcm.clear();
                self.pending_offset = 0;
            }
            frames_out += to_copy;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        // 2. Process queued complete packets
        while frames_out < max_frames && !self.packet_queue.is_empty() {
            let Some(packet) = self.packet_queue.pop_front() else {
                break;
            };
            self.decode_single_packet(&packet, pcm_out, &mut frames_out, max_frames)?;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        // 3. Ingest input buffer
        if self.stream_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
            self.flush();
            return Err(DecoderError::CorruptedFrame);
        }

        if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

        // 4. Demux Ogg pages from stream_buf
        while let Some(pos) = self.stream_buf.windows(4).position(|w| w == b"OggS") {
            if pos > 0 {
                self.stream_buf.drain(..pos);
            }

            if self.stream_buf.len() < 27 {
                break;
            }

            if self.stream_buf[4] != 0 {
                self.stream_buf.drain(..1);
                continue;
            }

            let n_segs = self.stream_buf[26] as usize;
            let header_len = 27 + n_segs;
            if self.stream_buf.len() < header_len {
                break;
            }

            let mut seg_table_buf = [0u8; 255];
            seg_table_buf[..n_segs].copy_from_slice(&self.stream_buf[27..header_len]);
            let seg_table = &seg_table_buf[..n_segs];
            let mut body_len: usize = 0;
            for &s in seg_table {
                body_len += s as usize;
            }
            let total_page_len = header_len + body_len;
            if self.stream_buf.len() < total_page_len {
                break;
            }

            let expected_crc = u32::from_le_bytes([
                self.stream_buf[22],
                self.stream_buf[23],
                self.stream_buf[24],
                self.stream_buf[25],
            ]);
            let actual_crc = ogg_crc32(&self.stream_buf[..total_page_len]);
            if expected_crc != actual_crc {
                if total_page_len == self.stream_buf.len() {
                    self.stream_buf.clear();
                } else {
                    self.stream_buf.drain(..total_page_len);
                }
                return Err(DecoderError::CorruptedFrame);
            }

            let header_type_flag = self.stream_buf[5];
            let is_continued = (header_type_flag & 0x01) != 0;

            if !is_continued && !self.partial_packet.is_empty() {
                self.partial_packet.clear();
            }

            let mut body_offset = header_len;
            let mut page_packets = Vec::new();

            for &seg_len_u8 in seg_table {
                let seg_len = seg_len_u8 as usize;
                if self.partial_packet.len().saturating_add(seg_len) > MAX_STREAM_BUF {
                    self.partial_packet.clear();
                    if total_page_len == self.stream_buf.len() {
                        self.stream_buf.clear();
                    } else {
                        self.stream_buf.drain(..total_page_len);
                    }
                    return Err(DecoderError::CorruptedFrame);
                }
                self.partial_packet
                    .extend_from_slice(&self.stream_buf[body_offset..body_offset + seg_len]);
                body_offset += seg_len;

                if seg_len_u8 < 255 {
                    let packet = core::mem::take(&mut self.partial_packet);
                    if !packet.is_empty() {
                        page_packets.push(packet);
                    }
                }
            }

            if total_page_len == self.stream_buf.len() {
                self.stream_buf.clear();
            } else {
                self.stream_buf.drain(..total_page_len);
            }

            for packet in page_packets {
                if frames_out < max_frames && self.packet_queue.is_empty() {
                    self.decode_single_packet(&packet, pcm_out, &mut frames_out, max_frames)?;
                } else if self.packet_queue.len() < MAX_PACKET_QUEUE {
                    self.packet_queue.push_back(packet);
                }
            }

            if frames_out >= max_frames {
                break;
            }
        }

        if frames_out == 0 && input_taken == 0 {
            Err(DecoderError::NeedMoreData)
        } else {
            Ok((input_taken, frames_out))
        }
    }

    fn flush(&mut self) {
        self.flush();
    }
}

#[cfg(feature = "alloc")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum AlacState {
    SearchingAtoms,
    ReadingMdat,
    Eof,
}

#[cfg(feature = "alloc")]
/// Pure-Rust Apple Lossless Audio Codec (ALAC) Decoder using alac crate
/// with in-memory streaming MP4 atom demuxer.
pub struct AlacDecoder {
    pub sample_rate: u32,
    pub channels: u32,
    pub bit_depth: u8,
    pub packets_decoded: usize,
    stream_buf: Vec<u8>,
    decoder: Option<alac::Decoder>,
    sample_sizes: Vec<usize>,
    sample_idx: usize,
    packet_out_buf: Vec<i32>,
    pending_pcm: Vec<i32>,
    pending_offset: usize,
    state: AlacState,
    bytes_to_skip: usize,
    first_chunk_offset: Option<usize>,
    file_pos: usize,
}

#[cfg(feature = "alloc")]
impl Default for AlacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl AlacDecoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
            packets_decoded: 0,
            stream_buf: Vec::new(),
            decoder: None,
            sample_sizes: Vec::new(),
            sample_idx: 0,
            packet_out_buf: Vec::new(),
            pending_pcm: Vec::new(),
            pending_offset: 0,
            state: AlacState::SearchingAtoms,
            bytes_to_skip: 0,
            first_chunk_offset: None,
            file_pos: 0,
        }
    }

    pub fn has_pending_pcm(&self) -> bool {
        self.pending_offset < self.pending_pcm.len()
            || (self.state == AlacState::ReadingMdat
                && self.sample_idx < self.sample_sizes.len()
                && self.stream_buf.len() >= self.sample_sizes[self.sample_idx])
    }

    pub fn flush(&mut self) {
        self.pending_pcm.clear();
        self.pending_offset = 0;
        self.stream_buf.clear();
        self.sample_sizes.clear();
        self.sample_idx = 0;
        self.bytes_to_skip = 0;
        self.first_chunk_offset = None;
        self.file_pos = 0;
        self.packets_decoded = 0;
        self.state = AlacState::SearchingAtoms;
        self.decoder = None;
    }
}

#[cfg(feature = "alloc")]
impl AudioDecoder for AlacDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let max_frames = pcm_out.len() / 2;
        if max_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        let mut frames_out = 0;

        // 1. Copy pending PCM from prior decode
        if self.pending_offset < self.pending_pcm.len() {
            let pending_samples = self.pending_pcm.len() - self.pending_offset;
            let pending_frames = pending_samples / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            let samples = to_copy * 2;
            pcm_out[..samples].copy_from_slice(
                &self.pending_pcm[self.pending_offset..self.pending_offset + samples],
            );
            self.pending_offset += samples;
            if self.pending_offset >= self.pending_pcm.len() {
                self.pending_pcm.clear();
                self.pending_offset = 0;
            }
            frames_out += to_copy;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        // 2. Ingest input buffer
        let max_buf = if self.state == AlacState::ReadingMdat {
            MAX_STREAM_BUF
        } else {
            MAX_MP4_META_BUF
        };

        if self.stream_buf.len().saturating_add(encoded_in.len()) > max_buf {
            self.flush();
            return Err(DecoderError::CorruptedFrame);
        }

        if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

        // 3. Skip pending bytes if skipping over a large atom
        if self.bytes_to_skip > 0 {
            let to_skip = core::cmp::min(self.bytes_to_skip, self.stream_buf.len());
            self.stream_buf.drain(..to_skip);
            self.bytes_to_skip -= to_skip;
            self.file_pos = self.file_pos.saturating_add(to_skip);
            if self.bytes_to_skip > 0 {
                if frames_out == 0 && input_taken == 0 {
                    return Err(DecoderError::NeedMoreData);
                } else {
                    return Ok((input_taken, frames_out));
                }
            }
        }

        // 4. Parse MP4 atoms if still searching
        while self.state == AlacState::SearchingAtoms {
            if self.bytes_to_skip > 0 {
                let to_skip = core::cmp::min(self.bytes_to_skip, self.stream_buf.len());
                self.stream_buf.drain(..to_skip);
                self.bytes_to_skip -= to_skip;
                self.file_pos = self.file_pos.saturating_add(to_skip);
                if self.bytes_to_skip > 0 {
                    break;
                }
            }

            if self.stream_buf.len() < 8 {
                break;
            }

            let len = u32::from_be_bytes([
                self.stream_buf[0],
                self.stream_buf[1],
                self.stream_buf[2],
                self.stream_buf[3],
            ]) as usize;

            let fourcc = [
                self.stream_buf[4],
                self.stream_buf[5],
                self.stream_buf[6],
                self.stream_buf[7],
            ];

            match &fourcc {
                b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                    self.stream_buf.drain(..8);
                    self.file_pos = self.file_pos.saturating_add(8);
                }
                b"stsd" => {
                    if self.stream_buf.len() < 16 {
                        break;
                    }
                    self.stream_buf.drain(..16);
                    self.file_pos = self.file_pos.saturating_add(16);
                }
                b"mp4a" => {
                    if self.stream_buf.len() < 36 {
                        break;
                    }
                    self.stream_buf.drain(..36);
                    self.file_pos = self.file_pos.saturating_add(36);
                }
                b"alac" => {
                    if len < 36 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if self.stream_buf.len() < len {
                        break;
                    }
                    let cookie = &self.stream_buf[36..len];
                    let info = alac::StreamInfo::from_cookie(cookie).or_else(|_| {
                        if len >= 24 {
                            alac::StreamInfo::from_cookie(&self.stream_buf[..len])
                        } else {
                            alac::StreamInfo::from_cookie(&[])
                        }
                    });
                    match info {
                        Ok(stream_info) => {
                            if stream_info.channels() != 1 && stream_info.channels() != 2 {
                                return Err(DecoderError::UnsupportedFeature);
                            }
                            if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE)
                                .contains(&stream_info.sample_rate())
                            {
                                return Err(DecoderError::UnsupportedFeature);
                            }
                            self.sample_rate = stream_info.sample_rate();
                            self.channels = stream_info.channels() as u32;
                            self.bit_depth = stream_info.bit_depth();
                            let max_samples = stream_info.max_samples_per_packet() as usize;
                            if max_samples > MAX_FLAC_BLOCK_SAMPLES * 2 {
                                return Err(DecoderError::UnsupportedFeature);
                            }
                            self.packet_out_buf.resize(max_samples, 0);
                            self.decoder = Some(alac::Decoder::new(stream_info));
                        }
                        Err(_) => return Err(DecoderError::InvalidHeader),
                    }
                    self.stream_buf.drain(..len);
                    self.file_pos = self.file_pos.saturating_add(len);
                }
                b"stsz" => {
                    if len < 20 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if self.stream_buf.len() < len {
                        break;
                    }
                    let default_sz = u32::from_be_bytes([
                        self.stream_buf[12],
                        self.stream_buf[13],
                        self.stream_buf[14],
                        self.stream_buf[15],
                    ]) as usize;
                    let count = u32::from_be_bytes([
                        self.stream_buf[16],
                        self.stream_buf[17],
                        self.stream_buf[18],
                        self.stream_buf[19],
                    ]) as usize;
                    if count > MAX_MP4_SAMPLES {
                        return Err(DecoderError::UnsupportedFeature);
                    }

                    self.sample_sizes.clear();
                    if default_sz == 0 {
                        if len < 20 + count * 4 {
                            return Err(DecoderError::InvalidHeader);
                        }
                        self.sample_sizes.reserve(count);
                        for i in 0..count {
                            let off = 20 + i * 4;
                            let sz = u32::from_be_bytes([
                                self.stream_buf[off],
                                self.stream_buf[off + 1],
                                self.stream_buf[off + 2],
                                self.stream_buf[off + 3],
                            ]) as usize;
                            self.sample_sizes.push(sz);
                        }
                    } else {
                        self.sample_sizes = vec![default_sz; count];
                    }
                    self.stream_buf.drain(..len);
                    self.file_pos = self.file_pos.saturating_add(len);
                }
                b"stco" => {
                    if len < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if self.stream_buf.len() < len {
                        break;
                    }
                    let count = u32::from_be_bytes([
                        self.stream_buf[12],
                        self.stream_buf[13],
                        self.stream_buf[14],
                        self.stream_buf[15],
                    ]) as usize;
                    if count > 0 && len >= 20 {
                        let off = u32::from_be_bytes([
                            self.stream_buf[16],
                            self.stream_buf[17],
                            self.stream_buf[18],
                            self.stream_buf[19],
                        ]) as usize;
                        self.first_chunk_offset = Some(off);
                    }
                    self.stream_buf.drain(..len);
                    self.file_pos = self.file_pos.saturating_add(len);
                }
                b"co64" => {
                    if len < 16 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if self.stream_buf.len() < len {
                        break;
                    }
                    let count = u32::from_be_bytes([
                        self.stream_buf[12],
                        self.stream_buf[13],
                        self.stream_buf[14],
                        self.stream_buf[15],
                    ]) as usize;
                    if count > 0 && len >= 24 {
                        let off = u64::from_be_bytes([
                            self.stream_buf[16],
                            self.stream_buf[17],
                            self.stream_buf[18],
                            self.stream_buf[19],
                            self.stream_buf[20],
                            self.stream_buf[21],
                            self.stream_buf[22],
                            self.stream_buf[23],
                        ]) as usize;
                        self.first_chunk_offset = Some(off);
                    }
                    self.stream_buf.drain(..len);
                    self.file_pos = self.file_pos.saturating_add(len);
                }
                b"mdat" => {
                    if self.decoder.is_none() {
                        return Err(DecoderError::InvalidHeader);
                    }
                    let header_len = if len == 1 { 16 } else { 8 };
                    if self.stream_buf.len() < header_len {
                        break;
                    }
                    self.stream_buf.drain(..header_len);
                    self.file_pos = self.file_pos.saturating_add(header_len);
                    if let Some(first_off) = self.first_chunk_offset {
                        if first_off > self.file_pos {
                            self.bytes_to_skip = first_off.saturating_sub(self.file_pos);
                            let to_skip = core::cmp::min(self.bytes_to_skip, self.stream_buf.len());
                            self.stream_buf.drain(..to_skip);
                            self.bytes_to_skip -= to_skip;
                            self.file_pos = self.file_pos.saturating_add(to_skip);
                        }
                    }
                    self.state = AlacState::ReadingMdat;
                }
                _ => {
                    if len < 8 {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if self.stream_buf.len() >= len {
                        self.stream_buf.drain(..len);
                        self.file_pos = self.file_pos.saturating_add(len);
                    } else {
                        let avail = self.stream_buf.len();
                        self.file_pos = self.file_pos.saturating_add(avail);
                        self.bytes_to_skip = len.saturating_sub(avail);
                        self.stream_buf.clear();
                        break;
                    }
                }
            }
        }

        // 5. Decode packets from mdat
        if self.state == AlacState::ReadingMdat {
            while frames_out < max_frames && self.sample_idx < self.sample_sizes.len() {
                let pkt_len = self.sample_sizes[self.sample_idx];
                if pkt_len > MAX_STREAM_BUF {
                    self.sample_idx += 1;
                    self.stream_buf.clear();
                    return Err(DecoderError::CorruptedFrame);
                }
                if self.stream_buf.len() < pkt_len {
                    break;
                }
                let packet_slice = &self.stream_buf[..pkt_len];
                if let Some(ref mut dec) = self.decoder {
                    match dec.decode_packet::<i32>(packet_slice, &mut self.packet_out_buf) {
                        Ok(samples) => {
                            self.packets_decoded += 1;
                            let ch = self.channels as usize;
                            if ch == 0 {
                                return Err(DecoderError::UnsupportedSampleFormat);
                            }
                            let packet_frames = samples.len() / ch;
                            let available = max_frames.saturating_sub(frames_out);
                            let to_copy = core::cmp::min(packet_frames, available);
                            if ch == 2 {
                                pcm_out[(frames_out * 2)..((frames_out + to_copy) * 2)]
                                    .copy_from_slice(&samples[..to_copy * 2]);
                                if to_copy < packet_frames {
                                    let rem = &samples[to_copy * 2..packet_frames * 2];
                                    self.pending_pcm.extend_from_slice(rem);
                                }
                            } else {
                                for (i, &s) in samples[..to_copy].iter().enumerate() {
                                    pcm_out[(frames_out + i) * 2] = s;
                                    pcm_out[(frames_out + i) * 2 + 1] = s;
                                }
                                if to_copy < packet_frames {
                                    self.pending_pcm.reserve((packet_frames - to_copy) * 2);
                                    for &s in &samples[to_copy..packet_frames] {
                                        self.pending_pcm.push(s);
                                        self.pending_pcm.push(s);
                                    }
                                }
                            }
                            frames_out += to_copy;
                        }
                        Err(_) => {
                            // Corrupted packet in stream: skip
                        }
                    }
                }
                self.stream_buf.drain(..pkt_len);
                self.file_pos = self.file_pos.saturating_add(pkt_len);
                self.sample_idx += 1;
            }
            if self.sample_idx >= self.sample_sizes.len() && !self.sample_sizes.is_empty() {
                self.state = AlacState::Eof;
            }
        }

        if frames_out == 0 && input_taken == 0 {
            Err(DecoderError::NeedMoreData)
        } else {
            Ok((input_taken, frames_out))
        }
    }

    fn flush(&mut self) {
        self.flush();
    }
}

/// Pre-warms the static Opus CeltMode table in heap memory while the stack
/// is clear.
#[cfg(feature = "alloc")]
pub fn prewarm_opus() {
    let _ = opus_rs::modes::default_mode();
}

#[cfg(feature = "alloc")]
enum OpusState {
    WaitingHead,
    WaitingTags {
        decoder: Box<opus_rs::OpusDecoder>,
        pre_skip: usize,
    },
    Ready {
        decoder: Box<opus_rs::OpusDecoder>,
        pre_skip: usize,
    },
}

#[cfg(feature = "alloc")]
/// Pure-Rust Ogg Opus Decoder using opus-rs with in-memory Ogg page
/// demuxing (DEC-004, RFC 7845).
pub struct OpusDecoder {
    pub sample_rate: u32,
    pub channels: u32,
    pub packets_decoded: usize,
    stream_buf: Vec<u8>,
    partial_packet: Vec<u8>,
    packet_queue: VecDeque<Vec<u8>>,
    pending_pcm: Vec<i32>,
    pending_offset: usize,
    state: OpusState,
    float_buf: Vec<f32>,
}

#[cfg(feature = "alloc")]
impl Default for OpusDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl OpusDecoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            packets_decoded: 0,
            stream_buf: Vec::new(),
            partial_packet: Vec::new(),
            packet_queue: VecDeque::new(),
            pending_pcm: Vec::new(),
            pending_offset: 0,
            state: OpusState::WaitingHead,
            float_buf: vec![0.0f32; 11520],
        }
    }

    pub fn has_pending_pcm(&self) -> bool {
        self.pending_offset < self.pending_pcm.len()
            || !self.packet_queue.is_empty()
            || self.stream_buf.len() >= 27
    }

    pub fn flush(&mut self) {
        self.pending_pcm.clear();
        self.pending_offset = 0;
        self.packet_queue.clear();
        self.partial_packet.clear();
        self.stream_buf.clear();
        self.packets_decoded = 0;
        self.state = OpusState::WaitingHead;
    }

    fn decode_single_packet(
        &mut self,
        packet: &[u8],
        pcm_out: &mut [i32],
        frames_out: &mut usize,
        max_frames: usize,
    ) -> Result<(), DecoderError> {
        if packet.is_empty() {
            return Ok(());
        }

        // 1. Check for OpusHead identification header (RFC 7845 §5.1)
        if packet.starts_with(b"OpusHead") {
            if packet.len() < 19 {
                return Err(DecoderError::InvalidHeader);
            }
            let channels = packet[9] as usize;
            if channels != 1 && channels != 2 {
                return Err(DecoderError::UnsupportedSampleFormat);
            }
            let pre_skip = u16::from_le_bytes([packet[10], packet[11]]) as usize;
            let decoder = opus_rs::OpusDecoder::boxed_new(48000, channels)
                .map_err(|_| DecoderError::InvalidHeader)?;
            self.sample_rate = 48000;
            self.channels = channels as u32;
            self.state = OpusState::WaitingTags { decoder, pre_skip };
            return Ok(());
        }

        // 2. Check for OpusTags comment header (RFC 7845 §5.2)
        if packet.starts_with(b"OpusTags") {
            if let OpusState::WaitingTags { .. } = self.state {
                if let OpusState::WaitingTags { decoder, pre_skip } =
                    core::mem::replace(&mut self.state, OpusState::WaitingHead)
                {
                    self.state = OpusState::Ready { decoder, pre_skip };
                }
            }
            return Ok(());
        }

        // If an audio packet arrives while in WaitingTags, transition to Ready
        if let OpusState::WaitingTags { .. } = self.state {
            if let OpusState::WaitingTags { decoder, pre_skip } =
                core::mem::replace(&mut self.state, OpusState::WaitingHead)
            {
                self.state = OpusState::Ready { decoder, pre_skip };
            }
        }

        // 3. Audio packet decoding
        let OpusState::Ready {
            ref mut decoder,
            ref mut pre_skip,
        } = self.state
        else {
            return Ok(());
        };

        if self.float_buf.len() < 11520 {
            self.float_buf.resize(11520, 0.0f32);
        }

        let samples_per_ch = match decoder.decode(packet, 5760, &mut self.float_buf) {
            Ok(n) => n,
            Err(_) => {
                // In a continuous stream, skip corrupt packet
                return Ok(());
            }
        };

        self.packets_decoded += 1;
        if samples_per_ch == 0 {
            return Ok(());
        }

        let mut start_frame = 0;
        if *pre_skip > 0 {
            if samples_per_ch <= *pre_skip {
                *pre_skip -= samples_per_ch;
                return Ok(());
            } else {
                start_frame = *pre_skip;
                *pre_skip = 0;
            }
        }

        let packet_frames = samples_per_ch - start_frame;
        let available = max_frames.saturating_sub(*frames_out);
        let to_copy = core::cmp::min(packet_frames, available);
        let ch = self.channels as usize;

        let to_i32 = |s: f32| {
            let scaled = s.clamp(-1.0, 1.0) * 2147483520.0;
            if scaled >= 0.0 {
                (scaled + 0.5) as i32
            } else {
                (scaled - 0.5) as i32
            }
        };

        if ch == 2 {
            for i in 0..to_copy {
                let idx = (start_frame + i) * 2;
                let l = to_i32(self.float_buf[idx]);
                let r = to_i32(self.float_buf[idx + 1]);
                pcm_out[(*frames_out + i) * 2] = l;
                pcm_out[(*frames_out + i) * 2 + 1] = r;
            }
            if to_copy < packet_frames {
                for i in to_copy..packet_frames {
                    let idx = (start_frame + i) * 2;
                    let l = to_i32(self.float_buf[idx]);
                    let r = to_i32(self.float_buf[idx + 1]);
                    self.pending_pcm.push(l);
                    self.pending_pcm.push(r);
                }
            }
        } else {
            for i in 0..to_copy {
                let idx = start_frame + i;
                let s = to_i32(self.float_buf[idx]);
                pcm_out[(*frames_out + i) * 2] = s;
                pcm_out[(*frames_out + i) * 2 + 1] = s;
            }
            if to_copy < packet_frames {
                for i in to_copy..packet_frames {
                    let idx = start_frame + i;
                    let s = to_i32(self.float_buf[idx]);
                    self.pending_pcm.push(s);
                    self.pending_pcm.push(s);
                }
            }
        }
        *frames_out += to_copy;
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl AudioDecoder for OpusDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let max_frames = pcm_out.len() / 2;
        if max_frames == 0 {
            return Err(DecoderError::BufferOverflow);
        }

        let mut frames_out = 0;

        // 1. Copy pending PCM from prior decode
        if self.pending_offset < self.pending_pcm.len() {
            let pending_samples = self.pending_pcm.len() - self.pending_offset;
            let pending_frames = pending_samples / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            let samples = to_copy * 2;
            pcm_out[..samples].copy_from_slice(
                &self.pending_pcm[self.pending_offset..self.pending_offset + samples],
            );
            self.pending_offset += samples;
            if self.pending_offset >= self.pending_pcm.len() {
                self.pending_pcm.clear();
                self.pending_offset = 0;
            }
            frames_out += to_copy;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        // 2. Process queued complete packets
        while frames_out < max_frames && !self.packet_queue.is_empty() {
            let Some(packet) = self.packet_queue.pop_front() else {
                break;
            };
            self.decode_single_packet(&packet, pcm_out, &mut frames_out, max_frames)?;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
        }

        // 3. Ingest input buffer
        if self.stream_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
            self.flush();
            return Err(DecoderError::CorruptedFrame);
        }

        if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

        // 4. Demux Ogg pages from stream_buf
        while let Some(pos) = self.stream_buf.windows(4).position(|w| w == b"OggS") {
            if pos > 0 {
                self.stream_buf.drain(..pos);
            }

            if self.stream_buf.len() < 27 {
                break;
            }

            if self.stream_buf[4] != 0 {
                self.stream_buf.drain(..1);
                continue;
            }

            let n_segs = self.stream_buf[26] as usize;
            let header_len = 27 + n_segs;
            if self.stream_buf.len() < header_len {
                break;
            }

            let mut seg_table_buf = [0u8; 255];
            seg_table_buf[..n_segs].copy_from_slice(&self.stream_buf[27..header_len]);
            let seg_table = &seg_table_buf[..n_segs];
            let mut body_len: usize = 0;
            for &s in seg_table {
                body_len += s as usize;
            }
            let total_page_len = header_len + body_len;
            if self.stream_buf.len() < total_page_len {
                break;
            }

            let expected_crc = u32::from_le_bytes([
                self.stream_buf[22],
                self.stream_buf[23],
                self.stream_buf[24],
                self.stream_buf[25],
            ]);
            let actual_crc = ogg_crc32(&self.stream_buf[..total_page_len]);
            if expected_crc != actual_crc {
                if total_page_len == self.stream_buf.len() {
                    self.stream_buf.clear();
                } else {
                    self.stream_buf.drain(..total_page_len);
                }
                return Err(DecoderError::CorruptedFrame);
            }

            let header_type_flag = self.stream_buf[5];
            let is_continued = (header_type_flag & 0x01) != 0;

            if !is_continued && !self.partial_packet.is_empty() {
                self.partial_packet.clear();
            }

            let mut body_offset = header_len;
            let mut page_packets = Vec::new();

            for &seg_len_u8 in seg_table {
                let seg_len = seg_len_u8 as usize;
                if self.partial_packet.len().saturating_add(seg_len) > MAX_STREAM_BUF {
                    self.partial_packet.clear();
                    if total_page_len == self.stream_buf.len() {
                        self.stream_buf.clear();
                    } else {
                        self.stream_buf.drain(..total_page_len);
                    }
                    return Err(DecoderError::CorruptedFrame);
                }
                self.partial_packet
                    .extend_from_slice(&self.stream_buf[body_offset..body_offset + seg_len]);
                body_offset += seg_len;

                if seg_len_u8 < 255 {
                    let packet = core::mem::take(&mut self.partial_packet);
                    if !packet.is_empty() {
                        page_packets.push(packet);
                    }
                }
            }

            if total_page_len == self.stream_buf.len() {
                self.stream_buf.clear();
            } else {
                self.stream_buf.drain(..total_page_len);
            }

            for packet in page_packets {
                if frames_out < max_frames && self.packet_queue.is_empty() {
                    self.decode_single_packet(&packet, pcm_out, &mut frames_out, max_frames)?;
                } else if self.packet_queue.len() < MAX_PACKET_QUEUE {
                    self.packet_queue.push_back(packet);
                }
            }

            if frames_out >= max_frames {
                break;
            }
        }

        if frames_out == 0 && input_taken == 0 {
            Err(DecoderError::NeedMoreData)
        } else {
            Ok((input_taken, frames_out))
        }
    }

    fn flush(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm_16bit_be_stereo_decoding() {
        let mut decoder = PcmDecoder::new(16, 2, true);

        // 2 stereo frames: Frame 1 (1000, -1000), Frame 2 (2000, -2000)
        let f1_l = (1000i16).to_be_bytes();
        let f1_r = (-1000i16).to_be_bytes();
        let f2_l = (2000i16).to_be_bytes();
        let f2_r = (-2000i16).to_be_bytes();

        let encoded = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1], f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        let mut pcm_out = [0i32; 4];
        let (consumed, frames) = decoder.decode(&encoded, &mut pcm_out).unwrap();

        assert_eq!(consumed, 8);
        assert_eq!(frames, 2);
        assert_eq!(pcm_out[0], 1000 << 16);
        assert_eq!(pcm_out[1], -1000 << 16);
        assert_eq!(pcm_out[2], 2000 << 16);
        assert_eq!(pcm_out[3], -2000 << 16);
    }

    #[test]
    fn test_pcm_16bit_be_mono_duplication() {
        let mut decoder = PcmDecoder::new(16, 1, true);

        let f1 = (5000i16).to_be_bytes();
        let f2 = (-5000i16).to_be_bytes();
        let encoded = [f1[0], f1[1], f2[0], f2[1]];

        let mut pcm_out = [0i32; 4];
        let (consumed, frames) = decoder.decode(&encoded, &mut pcm_out).unwrap();

        assert_eq!(consumed, 4);
        assert_eq!(frames, 2);
        // Both channels must have the duplicated mono sample
        assert_eq!(pcm_out[0], 5000 << 16);
        assert_eq!(pcm_out[1], 5000 << 16);
        assert_eq!(pcm_out[2], -5000 << 16);
        assert_eq!(pcm_out[3], -5000 << 16);
    }

    #[test]
    fn test_pcm_buffer_overflow() {
        let mut decoder = PcmDecoder::new(16, 2, true);
        let encoded = [0u8; 8];
        let mut pcm_out = [0i32; 1]; // Less than 1 stereo frame (2 samples)
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::BufferOverflow)
        );
    }

    #[test]
    fn test_pcm_insufficient_data() {
        let mut decoder = PcmDecoder::new(16, 2, true);
        let encoded = [0u8; 3]; // 3 bytes is less than 1 frame (4 bytes)
        let mut pcm_out = [0i32; 4];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
    }

    #[test]
    fn test_mp3_heap_isolation_no_stack_overflow() {
        let mut decoder = Mp3Decoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = vec![0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );

        let mut small_pcm = [0i32; 32];
        assert_eq!(
            decoder.decode(&encoded, &mut small_pcm),
            Err(DecoderError::BufferOverflow)
        );
    }

    #[test]
    fn test_mp3_id3v2_skip_and_no_frame_progress() {
        let mut decoder = Mp3Decoder::new();
        let mut pcm_out = vec![0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME];

        // Incomplete ID3v2 header (< 10 bytes) -> NeedMoreData
        assert_eq!(
            decoder.decode(b"ID3\x04\x00", &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );

        // Complete ID3v2 header with synchsafe size = 20 bytes
        // (total = 10 + 20 = 30 bytes)
        let mut id3_pkt = [0u8; 34];
        id3_pkt[..3].copy_from_slice(b"ID3");
        id3_pkt[3] = 4; // v2.4
        id3_pkt[9] = 20; // size = 20
        let (consumed, frames) = decoder.decode(&id3_pkt, &mut pcm_out).unwrap();
        assert_eq!(consumed, 30);
        assert_eq!(frames, 0);

        // Non-sync padding bytes are skipped with Ok((consumed, 0)) rather
        // than stalling
        let junk = [0u8; 8];
        let (consumed_junk, frames_junk) = decoder.decode(&junk, &mut pcm_out).unwrap();
        assert!(consumed_junk > 0);
        assert_eq!(frames_junk, 0);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_mp3_decoder_partial_frame_guard() {
        let mut dec = Mp3Decoder::new();
        let mut audio_frame = [0u8; 417];
        audio_frame[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        let mut pcm_out = vec![0i32; 2304];

        // Truncated 200-byte slice returns Err(DecoderError::NeedMoreData)
        // without consuming the partial frame.
        let res = dec.decode(&audio_frame[..200], &mut pcm_out);
        assert_eq!(res, Err(DecoderError::NeedMoreData));

        // Full 417-byte frame decodes 1152 samples
        let (consumed, frames) = dec.decode(&audio_frame, &mut pcm_out).unwrap();
        assert_eq!(consumed, 417);
        assert_eq!(frames, 1152);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flac_insufficient_data() {
        let mut decoder = FlacDecoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_aac_insufficient_data() {
        let mut decoder = AacDecoder::new();
        let encoded = [0u8; 5];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
        assert_eq!(decoder.is_mp4, None);

        // 7 non-ADTS bytes (e.g. initial MP4 box fragment) must return
        // NeedMoreData without locking is_mp4
        let encoded7 = [0u8; 7];
        assert_eq!(
            decoder.decode(&encoded7, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
        assert_eq!(decoder.is_mp4, None);

        // 7-byte valid ADTS header locks is_mp4 to Some(false)
        let mut adts7 = [0u8; 7];
        adts7[0] = 0xFF;
        adts7[1] = 0xF1;
        let mut dec2 = AacDecoder::new();
        let _ = dec2.decode(&adts7, &mut pcm_out);
        assert_eq!(dec2.is_mp4, Some(false));
    }

    #[test]
    fn test_decode_asc_bytes_explicit_frequency_and_sbr() {
        // 1. Explicit 24-bit frequency: AOT=2, freq_idx=15, freq=44100 (0x00AC44), ch=2
        let asc_24bit = [0x17, 0x80, 0x56, 0x22, 0x10];
        let cfg = decode_asc_bytes(&asc_24bit).unwrap();
        assert_eq!(cfg.audio_object_type, 2);
        assert_eq!(cfg.sample_rate, 44100);
        assert_eq!(cfg.sampling_frequency_index, 4);
        assert_eq!(cfg.channel_configuration, 2);
        assert_eq!(cfg.adts_profile, 1);

        // 2. Explicit SBR (AOT 5): AOT=5, ext_freq_idx=4, base_aot=2, freq_idx=4, ch=2
        let asc_sbr = [0x2A, 0x09, 0x08];
        let cfg_sbr = decode_asc_bytes(&asc_sbr).unwrap();
        assert_eq!(cfg_sbr.audio_object_type, 5);
        assert_eq!(cfg_sbr.sample_rate, 44100);
        assert_eq!(cfg_sbr.sampling_frequency_index, 4);
        assert_eq!(cfg_sbr.channel_configuration, 2);
        assert_eq!(cfg_sbr.adts_profile, 1);

        // 3. HE-AAC with explicit 24-bit extension frequency (0x0F)
        let mut asc_bits = [0u8; 8];
        let vals: &[(u32, usize)] = &[
            (5, 5),      // AOT 5 (SBR)
            (15, 4),     // extensionSamplingFrequencyIndex = 15
            (48000, 24), // 24-bit extension frequency
            (2, 5),      // base audioObjectType = 2 (AAC-LC)
            (4, 4),      // samplingFrequencyIndex = 4 (44100 Hz)
            (2, 4),      // channelConfiguration = 2 (stereo)
        ];
        let mut bit_pos = 0;
        for &(val, n) in vals {
            for i in (0..n).rev() {
                let bit = ((val >> i) & 1) as u8;
                let byte_idx = bit_pos / 8;
                let bit_idx = 7 - (bit_pos % 8);
                asc_bits[byte_idx] |= bit << bit_idx;
                bit_pos += 1;
            }
        }
        let total_bytes = bit_pos.div_ceil(8);
        let cfg_sbr_24 = decode_asc_bytes(&asc_bits[..total_bytes]).unwrap();
        assert_eq!(cfg_sbr_24.audio_object_type, 5);
        assert_eq!(cfg_sbr_24.sample_rate, 44100);
        assert_eq!(cfg_sbr_24.channel_configuration, 2);
    }

    #[test]
    fn test_pcm_24bit_stereo_and_mono() {
        let mut dec_stereo = PcmDecoder::new(24, 2, true);
        // Stereo 24-bit: [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]
        let encoded_stereo = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut pcm_out = [0i32; 2];
        let (consumed, frames) = dec_stereo.decode(&encoded_stereo, &mut pcm_out).unwrap();
        assert_eq!(consumed, 6);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out[0], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
        assert_eq!(pcm_out[1], (0x78 << 24) | (0x9A << 16) | (0xBC << 8));

        let mut dec_mono = PcmDecoder::new(24, 1, true);
        let encoded_mono = [0x12, 0x34, 0x56];
        let mut pcm_out_mono = [0i32; 2];
        let (consumed, frames) = dec_mono.decode(&encoded_mono, &mut pcm_out_mono).unwrap();
        assert_eq!(consumed, 3);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out_mono[0], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
        assert_eq!(pcm_out_mono[1], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
    }

    #[test]
    fn test_pcm_32bit_stereo_and_mono() {
        let mut dec_stereo = PcmDecoder::new(32, 2, true);
        let encoded_stereo = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];
        let mut pcm_out = [0i32; 2];
        let (consumed, frames) = dec_stereo.decode(&encoded_stereo, &mut pcm_out).unwrap();
        assert_eq!(consumed, 8);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out[0], 0x10203040);
        assert_eq!(pcm_out[1], 0x50607080);

        let mut dec_mono = PcmDecoder::new(32, 1, true);
        let encoded_mono = [0x10, 0x20, 0x30, 0x40];
        let mut pcm_out_mono = [0i32; 2];
        let (consumed, frames) = dec_mono.decode(&encoded_mono, &mut pcm_out_mono).unwrap();
        assert_eq!(consumed, 4);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out_mono[0], 0x10203040);
        assert_eq!(pcm_out_mono[1], 0x10203040);
    }

    #[test]
    fn test_decoder_flushing() {
        let mut mp3 = Mp3Decoder::new();
        mp3.flush();

        #[cfg(feature = "std")]
        {
            let mut flac = FlacDecoder::new();
            flac.stream_buf.extend_from_slice(b"fLaC00001111");
            flac.header_len = 8;
            flac.pending_pcm.extend_from_slice(&[50, -50, 60, -60]);
            let mut flac_out = [0i32; 2];
            assert_eq!(flac.decode(&[], &mut flac_out), Ok((0, 1)));
            assert_eq!(flac_out, [50, -50]);
            flac.flush();
            assert!(flac.pending_pcm.is_empty());
            assert_eq!(flac.stream_buf.len(), 8);

            let mut aac = AacDecoder::new();
            aac.pending_pcm.extend_from_slice(&[100, -100, 200, -200]);
            let mut out = [0i32; 2];
            assert_eq!(aac.decode(&[], &mut out), Ok((0, 1)));
            assert_eq!(out, [100, -100]);
            aac.flush();
            assert!(aac.pending_pcm.is_empty());

            let mut vorbis = VorbisDecoder::new();
            vorbis
                .pending_pcm
                .extend_from_slice(&[10 << 16, (-10) << 16, 20 << 16, (-20) << 16]);
            let mut v_out = [0i32; 2];
            assert_eq!(vorbis.decode(&[], &mut v_out), Ok((0, 1)));
            assert_eq!(v_out, [10 << 16, (-10) << 16]);
            assert!(vorbis.has_pending_pcm());
            vorbis.flush();
            assert!(vorbis.pending_pcm.is_empty());
            assert!(!vorbis.has_pending_pcm());

            let mut alac = AlacDecoder::new();
            alac.pending_pcm
                .extend_from_slice(&[10 << 16, (-10) << 16, 20 << 16, (-20) << 16]);
            let mut a_out = [0i32; 2];
            assert_eq!(alac.decode(&[], &mut a_out), Ok((0, 1)));
            assert_eq!(a_out, [10 << 16, (-10) << 16]);
            assert!(alac.has_pending_pcm());
            alac.flush();
            assert!(alac.pending_pcm.is_empty());
            assert!(!alac.has_pending_pcm());
        }
    }

    #[test]
    fn test_pcm_8bit_stereo_and_mono() {
        let mut dec_stereo = PcmDecoder::new(8, 2, false);
        // Stereo 8-bit: [128, 255, 0, 128]
        // Frame 0: L = 128 (0), R = 255 (127 << 24)
        // Frame 1: L = 0 (-128 << 24), R = 128 (0)
        let encoded_stereo = [128u8, 255, 0, 128];
        let mut pcm_out = [0i32; 4];
        let (consumed, frames) = dec_stereo.decode(&encoded_stereo, &mut pcm_out).unwrap();
        assert_eq!(consumed, 4);
        assert_eq!(frames, 2);
        assert_eq!(pcm_out[0], 0);
        assert_eq!(pcm_out[1], 127 << 24);
        assert_eq!(pcm_out[2], (-128) << 24);
        assert_eq!(pcm_out[3], 0);

        let mut dec_mono = PcmDecoder::new(8, 1, false);
        let encoded_mono = [200u8, 50];
        let mut pcm_out_mono = [0i32; 4];
        let (consumed, frames) = dec_mono.decode(&encoded_mono, &mut pcm_out_mono).unwrap();
        assert_eq!(consumed, 2);
        assert_eq!(frames, 2);
        let expected_0 = (200i32 - 128) << 24;
        let expected_1 = (50i32 - 128) << 24;
        assert_eq!(pcm_out_mono[0], expected_0);
        assert_eq!(pcm_out_mono[1], expected_0);
        assert_eq!(pcm_out_mono[2], expected_1);
        assert_eq!(pcm_out_mono[3], expected_1);
    }

    #[test]
    fn test_pcm_24bit_le_stereo_and_mono() {
        let mut dec_stereo = PcmDecoder::new(24, 2, false);
        let encoded_stereo = [0x56, 0x34, 0x12, 0xBC, 0x9A, 0x78];
        let mut pcm_out = [0i32; 2];
        let (consumed, frames) = dec_stereo.decode(&encoded_stereo, &mut pcm_out).unwrap();
        assert_eq!(consumed, 6);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out[0], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
        assert_eq!(pcm_out[1], (0x78 << 24) | (0x9A << 16) | (0xBC << 8));

        let mut dec_mono = PcmDecoder::new(24, 1, false);
        let encoded_mono = [0x56, 0x34, 0x12];
        let mut pcm_out_mono = [0i32; 2];
        let (consumed, frames) = dec_mono.decode(&encoded_mono, &mut pcm_out_mono).unwrap();
        assert_eq!(consumed, 3);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out_mono[0], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
        assert_eq!(pcm_out_mono[1], (0x12 << 24) | (0x34 << 16) | (0x56 << 8));
    }

    #[test]
    fn test_pcm_32bit_le_stereo_and_mono() {
        let mut dec_stereo = PcmDecoder::new(32, 2, false);
        let encoded_stereo = [0x40, 0x30, 0x20, 0x10, 0x80, 0x70, 0x60, 0x50];
        let mut pcm_out = [0i32; 2];
        let (consumed, frames) = dec_stereo.decode(&encoded_stereo, &mut pcm_out).unwrap();
        assert_eq!(consumed, 8);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out[0], 0x10203040);
        assert_eq!(pcm_out[1], 0x50607080);

        let mut dec_mono = PcmDecoder::new(32, 1, false);
        let encoded_mono = [0x40, 0x30, 0x20, 0x10];
        let mut pcm_out_mono = [0i32; 2];
        let (consumed, frames) = dec_mono.decode(&encoded_mono, &mut pcm_out_mono).unwrap();
        assert_eq!(consumed, 4);
        assert_eq!(frames, 1);
        assert_eq!(pcm_out_mono[0], 0x10203040);
        assert_eq!(pcm_out_mono[1], 0x10203040);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flac_max_stream_buf_overflow() {
        let mut decoder = FlacDecoder::new();
        let oversized = std::vec![0u8; MAX_STREAM_BUF + 1];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&oversized, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );

        // Also test multi-step accumulation exceeding MAX_STREAM_BUF
        let mut dec2 = FlacDecoder::new();
        let chunk1 = std::vec![0u8; 32768];
        assert_eq!(dec2.decode(&chunk1, &mut pcm_out), Ok((32768, 0)));
        let chunk2 = std::vec![0u8; 32769];
        assert_eq!(
            dec2.decode(&chunk2, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_vorbis_insufficient_data() {
        let mut decoder = VorbisDecoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_vorbis_max_stream_buf_overflow() {
        let mut decoder = VorbisDecoder::new();
        let oversized = std::vec![0u8; MAX_STREAM_BUF + 1];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&oversized, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );

        // Multi-step accumulation
        let mut dec2 = VorbisDecoder::new();
        let chunk1 = std::vec![0u8; 32768];
        let _ = dec2.decode(&chunk1, &mut pcm_out);
        let chunk2 = std::vec![0u8; 32769];
        assert_eq!(
            dec2.decode(&chunk2, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_alac_insufficient_data() {
        let mut decoder = AlacDecoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&encoded, &mut pcm_out),
            Err(DecoderError::NeedMoreData)
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_alac_max_stream_buf_overflow() {
        let mut decoder = AlacDecoder::new();
        let oversized = std::vec![0u8; MAX_MP4_META_BUF + 1];
        let mut pcm_out = [0i32; 32];
        assert_eq!(
            decoder.decode(&oversized, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );

        // Multi-step accumulation
        let mut dec2 = AlacDecoder::new();
        let chunk1 = std::vec![0u8; 135_168];
        let _ = dec2.decode(&chunk1, &mut pcm_out);
        let chunk2 = std::vec![0u8; 135_169];
        assert_eq!(
            dec2.decode(&chunk2, &mut pcm_out),
            Err(DecoderError::CorruptedFrame)
        );
    }

    #[test]
    fn test_mp3_flush_zeroes_sample_buf() {
        let mut decoder = Mp3Decoder::new();
        decoder.sample_buf[0] = 42;
        decoder.sample_buf[100] = 99;
        decoder.flush();
        assert_eq!(decoder.sample_buf[0], 0);
        assert_eq!(decoder.sample_buf[100], 0);
    }

    #[test]
    fn test_pcm_decoder_strips_wav_header_and_updates_config() {
        let mut decoder = PcmDecoder::new(16, 2, true);
        assert_eq!(decoder.sample_rate, None);
        assert!(decoder.big_endian);

        // 44-byte WAV header: 48000 Hz, 16-bit LE stereo
        let mut header = [0u8; 44];
        header[0..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(44u32 + 8 - 8).to_le_bytes());
        header[8..12].copy_from_slice(b"WAVE");
        header[12..16].copy_from_slice(b"fmt ");
        header[16..20].copy_from_slice(&16u32.to_le_bytes());
        header[20..22].copy_from_slice(&1u16.to_le_bytes());
        header[22..24].copy_from_slice(&2u16.to_le_bytes());
        header[24..28].copy_from_slice(&48000u32.to_le_bytes());
        header[28..32].copy_from_slice(&(48000u32 * 2 * 2).to_le_bytes());
        header[32..34].copy_from_slice(&4u16.to_le_bytes());
        header[34..36].copy_from_slice(&16u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&8u32.to_le_bytes());

        // 2 stereo frames (8 bytes) of 16-bit LE
        let f1_l = (1000i16).to_le_bytes();
        let f1_r = (-1000i16).to_le_bytes();
        let f2_l = (2000i16).to_le_bytes();
        let f2_r = (-2000i16).to_le_bytes();
        let pcm_data = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1], f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        let mut payload = [0u8; 52];
        payload[0..44].copy_from_slice(&header);
        payload[44..52].copy_from_slice(&pcm_data);

        let mut pcm_out = [0i32; 4];
        let (consumed, frames) = decoder.decode(&payload, &mut pcm_out).unwrap();

        assert_eq!(consumed, 52);
        assert_eq!(frames, 2);
        assert_eq!(decoder.sample_rate, Some(48000));
        assert!(!decoder.big_endian);
        assert_eq!(pcm_out[0], 1000 << 16);
        assert_eq!(pcm_out[1], -1000 << 16);
        assert_eq!(pcm_out[2], 2000 << 16);
        assert_eq!(pcm_out[3], -2000 << 16);
    }

    #[test]
    fn test_pcm_decoder_strips_aiff_header() {
        let mut decoder = PcmDecoder::new(16, 2, false);
        assert_eq!(decoder.sample_rate, None);
        assert!(!decoder.big_endian);

        // 54-byte AIFF header: FORM + COMM + SSND
        let mut header = [0u8; 54];
        header[0..4].copy_from_slice(b"FORM");
        header[4..8].copy_from_slice(&(54u32 + 4 - 8).to_be_bytes());
        header[8..12].copy_from_slice(b"AIFF");
        header[12..16].copy_from_slice(b"COMM");
        header[16..20].copy_from_slice(&18u32.to_be_bytes());
        header[20..22].copy_from_slice(&2i16.to_be_bytes());
        header[22..26].copy_from_slice(&1u32.to_be_bytes());
        header[26..28].copy_from_slice(&16i16.to_be_bytes());
        // 44100 Hz in IEEE 754 80-bit extended:
        header[28..38]
            .copy_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        header[38..42].copy_from_slice(b"SSND");
        header[42..46].copy_from_slice(&12u32.to_be_bytes());
        header[46..50].copy_from_slice(&0u32.to_be_bytes());
        header[50..54].copy_from_slice(&0u32.to_be_bytes());

        // 1 stereo frame (4 bytes) of 16-bit BE:
        let f_l = (500i16).to_be_bytes();
        let f_r = (-500i16).to_be_bytes();
        let pcm_data = [f_l[0], f_l[1], f_r[0], f_r[1]];

        let mut payload = [0u8; 58];
        payload[0..54].copy_from_slice(&header);
        payload[54..58].copy_from_slice(&pcm_data);

        let mut pcm_out = [0i32; 2];
        let (consumed, frames) = decoder.decode(&payload, &mut pcm_out).unwrap();

        assert_eq!(consumed, 58);
        assert_eq!(frames, 1);
        assert!(decoder.big_endian);
        assert_eq!(decoder.sample_rate, Some(44100));
        assert_eq!(pcm_out[0], 500 << 16);
        assert_eq!(pcm_out[1], -500 << 16);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flac_bulk_reader_and_8bit_shift() {
        use std::io::Read;
        let data = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let pos = std::cell::Cell::new(0usize);
        let mut reader = FlacByteReader {
            data: &data[..],
            pos: &pos,
        };
        let mut buf = [0u8; 8];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 8);
        assert_eq!(buf, data);

        assert_eq!(FlacDecoder::sample_shift(8), 24);
        assert_eq!(FlacDecoder::sample_shift(16), 16);
        assert_eq!(FlacDecoder::sample_shift(24), 8);
        assert_eq!(FlacDecoder::sample_shift(32), 0);
    }

    #[test]
    fn test_vorbis_multi_packet_stateful_continuity() {
        let mut mp3 = Mp3Decoder::new();
        assert_eq!(mp3.sample_rate, 44100);
        mp3.sample_rate = 48000;
        assert_eq!(mp3.sample_rate, 48000);

        #[cfg(feature = "std")]
        {
            let mut vorbis = VorbisDecoder::new();
            assert_eq!(vorbis.packets_decoded, 0);
            vorbis.packets_decoded = 42;
            assert_eq!(vorbis.packets_decoded, 42);
            vorbis.flush();
            assert_eq!(vorbis.packets_decoded, 0);

            let mut alac = AlacDecoder::new();
            assert_eq!(alac.packets_decoded, 0);
            alac.packets_decoded = 42;
            assert_eq!(alac.packets_decoded, 42);
            alac.flush();
            assert_eq!(alac.packets_decoded, 0);
        }
    }

    #[test]
    fn test_static_mp3_decoder_partial_frame_and_id3() {
        let mut dec = StaticMp3Decoder::new();
        assert_eq!(dec.sample_rate, 0);
        dec.reset_in_place();
        assert_eq!(dec.sample_rate, 44100);
        dec.sample_rate = 48000;
        dec.flush();
        assert_eq!(dec.sample_rate, 44100);

        // 2) Partial frame: Valid MPEG-1 Layer III 128 kbps 44.1 kHz header
        // Frame size is 144 * 128_000 / 44_100 + 0 = 417 bytes.
        let mut partial = [0u8; 250];
        partial[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        let mut pcm_out = [0i32; 2304];
        let err = dec.decode(&partial, &mut pcm_out);
        assert_eq!(err, Err(DecoderError::NeedMoreData));

        // 3) Leading non-sync bytes before [0xFF, 0xFB, 0x90, 0x00]
        let mut leading = [0u8; 300];
        leading[0] = 0x12;
        leading[1] = 0x34;
        leading[2] = 0x56;
        leading[3..7].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        let (consumed, frames) = dec.decode(&leading, &mut pcm_out).unwrap();
        assert_eq!(consumed, 3);
        assert_eq!(frames, 0);

        // ID3 tag skip test:
        // 10 header + 20 payload = 30 total
        let id3_header = [b'I', b'D', b'3', 3, 0, 0, 0, 0, 0, 20];
        let (consumed_id3, frames_id3) = dec.decode(&id3_header, &mut pcm_out).unwrap();
        assert_eq!(consumed_id3, 10);
        assert_eq!(frames_id3, 0);
        assert_eq!(dec.id3_skip_remaining, 20);
        let id3_tail = [0u8; 20];
        let (consumed_tail, _) = dec.decode(&id3_tail, &mut pcm_out).unwrap();
        assert_eq!(consumed_tail, 20);
        assert_eq!(dec.id3_skip_remaining, 0);
    }

    #[test]
    fn test_static_mp3_decoder_never_deadlocks_on_false_sync_or_reservoir() {
        let mut dec = StaticMp3Decoder::new();
        let mut pcm_out = [0i32; 2304];

        // 1. Two matching valid MP3 headers (MPEG-1 Layer III 128 kbps 44.1 kHz
        // = 417 bytes) but with all-zero/garbage payload that cannot be decoded
        // as valid audio or has no reservoir.
        let mut corrupted_frame = [0u8; 417 + 4];
        corrupted_frame[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        corrupted_frame[4..417].fill(0xFF);
        corrupted_frame[417..417 + 4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);

        // Decoder must NOT return NeedMoreData; it must return
        // Ok((consumed, 0)) to advance past bad frame!
        let res = dec.decode(&corrupted_frame, &mut pcm_out);
        assert!(res.is_ok(), "Expected Ok but got {:?}", res);
        let (consumed, frames) = res.unwrap();
        assert!(
            consumed >= 417,
            "Expected at least 417 bytes consumed, got {}",
            consumed
        );
        assert_eq!(frames, 0);

        // 2. Unmatched second header: first header matches sync, but byte at
        // 417 does not match
        let mut unmatched = [0u8; 417 + 4];
        unmatched[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        unmatched[417] = 0x00; // Not a matching header
        let (consumed_unmatched, frames_unmatched) = dec.decode(&unmatched, &mut pcm_out).unwrap();
        assert_eq!(consumed_unmatched, 1);
        assert_eq!(frames_unmatched, 0);

        // 3. Invalid bitrate nibble (0xF) is rejected by Header::read,
        // skipping non-sync bytes
        let mut invalid_bitrate = [0u8; 16];
        invalid_bitrate[0..4].copy_from_slice(&[0xFF, 0xFB, 0xF0, 0x00]);
        let (consumed_inv, frames_inv) = dec.decode(&invalid_bitrate, &mut pcm_out).unwrap();
        assert_eq!(consumed_inv, 13);
        assert_eq!(frames_inv, 0);
    }

    #[test]
    fn test_flac_cross_validation_all_fixtures() {
        let fixtures: &[(&str, &[u8])] = &[
            (
                "mono_16bit.flac",
                include_bytes!("../tests/fixtures/mono_16bit.flac"),
            ),
            (
                "stereo_16bit.flac",
                include_bytes!("../tests/fixtures/stereo_16bit.flac"),
            ),
            (
                "stereo_24bit.flac",
                include_bytes!("../tests/fixtures/stereo_24bit.flac"),
            ),
            ("fixed.flac", include_bytes!("../tests/fixtures/fixed.flac")),
            ("lpc.flac", include_bytes!("../tests/fixtures/lpc.flac")),
        ];

        for &(name, data) in fixtures {
            // 1. Decode with claxon
            let mut claxon_reader = claxon::FlacReader::new(std::io::Cursor::new(data)).unwrap();
            let claxon_info = claxon_reader.streaminfo();
            let shift = FlacDecoder::sample_shift(claxon_info.bits_per_sample);
            let mut claxon_samples = std::vec::Vec::new();
            for sample in claxon_reader.samples() {
                let s = sample.unwrap();
                claxon_samples.push(s << shift);
            }

            // 2. Decode with native FlacDecoder in chunks of 512 bytes with
            // 1024-sample pcm_out
            let mut decoder = FlacDecoder::new();
            let mut native_samples = std::vec::Vec::new();
            let mut pcm_buf = [0i32; 1024];
            let mut offset = 0;

            while offset < data.len()
                || decoder.has_pending_pcm()
                || decoder.stream_buf.len() > decoder.header_len
            {
                let end = (offset + 512).min(data.len());
                let chunk = &data[offset..end];
                match decoder.decode(chunk, &mut pcm_buf) {
                    Ok((consumed, frames)) => {
                        offset += consumed;
                        if claxon_info.channels == 1 {
                            for i in 0..frames {
                                native_samples.push(pcm_buf[i * 2]);
                            }
                        } else {
                            native_samples.extend_from_slice(&pcm_buf[..frames * 2]);
                        }
                        if consumed == 0 && frames == 0 && offset >= data.len() {
                            break;
                        }
                    }
                    Err(DecoderError::NeedMoreData) => {
                        if offset >= data.len() && !decoder.has_pending_pcm() {
                            break;
                        }
                    }
                    Err(e) => {
                        panic!("Decoder error on {}: {:?}", name, e);
                    }
                }
            }

            assert_eq!(
                claxon_samples.len(),
                native_samples.len(),
                "Sample count mismatch on fixture {}",
                name
            );
            for i in 0..claxon_samples.len() {
                if claxon_samples[i] != native_samples[i] {
                    panic!(
                        "Mismatch on {} at sample {}: claxon={}, native={}",
                        name, i, claxon_samples[i], native_samples[i]
                    );
                }
            }
        }
    }

    #[test]
    fn test_flac_block_draining_across_constrained_pcm_buffer() {
        let data = include_bytes!("../tests/fixtures/stereo_16bit.flac");
        let mut claxon_reader = claxon::FlacReader::new(std::io::Cursor::new(data)).unwrap();
        let claxon_info = claxon_reader.streaminfo();
        let shift = FlacDecoder::sample_shift(claxon_info.bits_per_sample);
        let mut expected = std::vec::Vec::new();
        for sample in claxon_reader.samples() {
            expected.push(sample.unwrap() << shift);
        }

        // Test with 2304-sample buffer (matching Xtensa PCM_SCRATCH_SAMPLES = 2304)
        let mut decoder = FlacDecoder::new();
        let mut output = std::vec::Vec::new();
        let mut pcm_buf = [0i32; 2304];
        let mut offset = 0;

        while offset < data.len()
            || decoder.has_pending_pcm()
            || decoder.stream_buf.len() > decoder.header_len
        {
            let end = (offset + 1024).min(data.len());
            let chunk = &data[offset..end];
            match decoder.decode(chunk, &mut pcm_buf) {
                Ok((consumed, frames)) => {
                    offset += consumed;
                    output.extend_from_slice(&pcm_buf[..frames * 2]);
                    if consumed == 0 && frames == 0 && offset >= data.len() {
                        break;
                    }
                }
                Err(DecoderError::NeedMoreData) => {
                    if offset >= data.len() && !decoder.has_pending_pcm() {
                        break;
                    }
                }
                Err(e) => panic!("Decode error in constrained buffer test: {:?}", e),
            }
        }

        assert_eq!(expected.len(), output.len());
        assert_eq!(expected, output);
    }

    #[test]
    fn test_flac_partial_chunk_ingestion_need_more_input() {
        let data = include_bytes!("../tests/fixtures/stereo_16bit.flac");
        let mut decoder = FlacDecoder::new();
        let mut pcm_buf = [0i32; 1024];

        // Feed first 500 bytes (header + partial Frame 1)
        let res = decoder.decode(&data[..500], &mut pcm_buf);
        assert_eq!(res, Ok((500, 0)));
        assert!(
            !decoder.has_pending_pcm(),
            "Decoder must NOT report pending PCM on partial chunk"
        );

        // Feed next 1036 bytes (matching Xtensa READ_SCRATCH_BYTES chunk)
        let res2 = decoder.decode(&data[500..1536], &mut pcm_buf);
        assert_eq!(res2, Ok((1036, 0)));
        assert!(
            !decoder.has_pending_pcm(),
            "Decoder must NOT report pending PCM on partial chunk"
        );
    }

    #[test]
    fn test_flac_4608_sample_block_decode() {
        let mut flac_data = std::vec::Vec::new();
        flac_data.extend_from_slice(b"fLaC");

        // STREAMINFO block (last block = true, type 0, length 34)
        flac_data.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]);
        let mut streaminfo = [0u8; 34];
        streaminfo[0..2].copy_from_slice(&4608u16.to_be_bytes());
        streaminfo[2..4].copy_from_slice(&4608u16.to_be_bytes());
        // 44100 Hz, 2 channels, 16 bps:
        streaminfo[10] = 0x0A;
        streaminfo[11] = 0xC4;
        streaminfo[12] = 0x42;
        streaminfo[13] = 0xF0;
        streaminfo[16] = 0x12;
        streaminfo[17] = 0x00;
        flac_data.extend_from_slice(&streaminfo);

        // Frame:
        let mut frame_bytes = std::vec![0xFF, 0xF8, 0x79, 0x18, 0x00];
        frame_bytes.extend_from_slice(&4607u16.to_be_bytes()); // 4608 - 1
        let header_crc = flac_crc8(&frame_bytes);
        frame_bytes.push(header_crc);

        // Subframe 0 (Left): Constant 1000
        frame_bytes.push(0x00);
        frame_bytes.extend_from_slice(&(1000i16).to_be_bytes());

        // Subframe 1 (Right): Constant -1000
        frame_bytes.push(0x00);
        frame_bytes.extend_from_slice(&(-1000i16).to_be_bytes());

        let frame_crc = flac_crc16(&frame_bytes);
        frame_bytes.extend_from_slice(&frame_crc.to_be_bytes());
        flac_data.extend_from_slice(&frame_bytes);

        let mut decoder = FlacDecoder::new();
        let mut pcm_out = std::vec![0i32; 9216];
        let (consumed, frames) = decoder.decode(&flac_data, &mut pcm_out).unwrap();

        assert_eq!(consumed, flac_data.len());
        assert_eq!(frames, 4608);
        assert_eq!(decoder.sample_rate, 44100);
        assert_eq!(decoder.channels, 2);
        assert_eq!(decoder.bits_per_sample, 16);

        for i in 0..4608 {
            assert_eq!(pcm_out[i * 2], 1000 << 16);
            assert_eq!(pcm_out[i * 2 + 1], -1000 << 16);
        }
    }

    #[test]
    fn test_flac_stream_switch_across_flush() {
        let flac_44k = include_bytes!("../tests/fixtures/stereo_16bit.flac");
        let flac_48k = include_bytes!("../tests/fixtures/stereo_24bit.flac");

        let mut decoder = FlacDecoder::new();
        let mut pcm_out = [0i32; 1024];

        // 1. Decode stream 1 (44.1 kHz, 16-bit)
        let (consumed1, frames1) = decoder.decode(flac_44k, &mut pcm_out).unwrap();
        assert!(consumed1 > 0);
        assert!(frames1 > 0);
        assert_eq!(decoder.sample_rate, 44100);
        assert_eq!(decoder.bits_per_sample, 16);

        // 2. Flush between tracks
        decoder.flush();

        // 3. Decode stream 2 (48 kHz, 24-bit) starting with b"fLaC"
        let (consumed2, frames2) = decoder.decode(flac_48k, &mut pcm_out).unwrap();
        assert!(consumed2 > 0);
        assert!(frames2 > 0);
        assert_eq!(decoder.sample_rate, 48000);
        assert_eq!(decoder.bits_per_sample, 24);

        // 4. Mid-stream flush: test that flushing followed by raw audio frames
        // (without b"fLaC" header) reuses existing STREAMINFO
        let chunk1_end = 1024.min(flac_48k.len());
        decoder.flush();
        // First feed up to chunk1_end
        let (_c_part1, _f_part1) = decoder
            .decode(&flac_48k[..chunk1_end], &mut pcm_out)
            .unwrap();
        assert_eq!(decoder.sample_rate, 48000);

        // Now flush mid-stream and feed next chunk without fLaC header
        decoder.flush();
        let chunk2_end = (chunk1_end + 2048).min(flac_48k.len());
        let (c_part2, _f_part2) = decoder
            .decode(&flac_48k[chunk1_end..chunk2_end], &mut pcm_out)
            .unwrap();
        assert!(c_part2 > 0);
        assert_eq!(decoder.sample_rate, 48000);
    }

    #[test]
    fn test_mp3_xing_lame_header_parse_and_gapless_trim() {
        let mut xing_frame = [0u8; 417];
        xing_frame[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        xing_frame[36..40].copy_from_slice(b"Xing");
        xing_frame[40..44].copy_from_slice(&0x0000_0003u32.to_be_bytes());
        xing_frame[44..48].copy_from_slice(&4u32.to_be_bytes());
        xing_frame[48..52].copy_from_slice(&(4u32 * 417).to_be_bytes());
        xing_frame[52..61].copy_from_slice(b"LAME3.100");
        // enc_delay = 576 (0x240), enc_padding = 1152 (0x480) at LAME + 21
        // (offset 73..76)
        xing_frame[73] = 0x24;
        xing_frame[74] = 0x04;
        xing_frame[75] = 0x80;

        let parsed = parse_xing_lame_header(&xing_frame).unwrap();
        assert_eq!(parsed.total_frames, Some(4));
        assert_eq!(parsed.total_bytes, Some(4 * 417));
        assert_eq!(parsed.enc_delay, 576);
        assert_eq!(parsed.enc_padding, 1152);
        assert_eq!(parsed.samples_per_frame, 1152);

        let mut audio_frame = [0u8; 417];
        audio_frame[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);

        let mut stream = std::vec::Vec::with_capacity(417 * 4);
        stream.extend_from_slice(&xing_frame);
        stream.extend_from_slice(&audio_frame);
        stream.extend_from_slice(&audio_frame);
        stream.extend_from_slice(&audio_frame);

        // 1. Verify Mp3Decoder gapless trimming (enc_delay = 576, enc_padding = 1152)
        let mut dec = Mp3Decoder::new();
        let mut pcm_out = [0i32; 2304];
        let mut offset = 0usize;
        let mut frame_counts = std::vec::Vec::new();
        while offset < stream.len() {
            let (consumed, frames) = dec.decode(&stream[offset..], &mut pcm_out).unwrap();
            assert!(consumed > 0);
            offset += consumed;
            frame_counts.push(frames);
        }
        assert_eq!(dec.enc_delay_frames(), 576);
        assert_eq!(dec.enc_padding_frames(), 1152);
        assert_eq!(dec.total_frames(), Some(4));
        // Frame 0 (Xing): 0; Frame 1: 1152 - 576 = 576; Frame 2: 1152;
        // Frame 3: 0 (trimmed)
        assert_eq!(frame_counts, std::vec![0, 576, 1152, 0]);
        assert_eq!(frame_counts.iter().sum::<usize>(), 3 * 1152 - 576 - 1152);

        // 2. Verify with_decoder_delay(true) (+529 samples leading trim)
        let mut dec_delay = Mp3Decoder::new().with_decoder_delay(true);
        let mut off2 = 0usize;
        let mut counts_with_delay = std::vec::Vec::new();
        while off2 < stream.len() {
            let (consumed, frames) = dec_delay.decode(&stream[off2..], &mut pcm_out).unwrap();
            off2 += consumed;
            counts_with_delay.push(frames);
        }
        assert_eq!(counts_with_delay, std::vec![0, 1152 - (576 + 529), 1152, 0]);

        // 3. Verify StaticMp3Decoder gapless trimming parity
        let mut static_dec = StaticMp3Decoder::new();
        let mut off3 = 0usize;
        let mut static_counts = std::vec::Vec::new();
        while off3 < stream.len() {
            let (consumed, frames) = static_dec.decode(&stream[off3..], &mut pcm_out).unwrap();
            off3 += consumed;
            static_counts.push(frames);
        }
        assert_eq!(static_dec.enc_delay_frames(), 576);
        assert_eq!(static_dec.enc_padding_frames(), 1152);
        assert_eq!(static_counts, std::vec![0, 576, 1152, 0]);
    }

    fn build_ogg_page(
        header_type: u8,
        granule_pos: u64,
        serial: u32,
        seq_no: u32,
        packets: &[&[u8]],
    ) -> std::vec::Vec<u8> {
        let mut seg_table = std::vec::Vec::new();
        let mut body = std::vec::Vec::new();
        for &pkt in packets {
            let mut rem = pkt.len();
            while rem >= 255 {
                seg_table.push(255u8);
                rem -= 255;
            }
            seg_table.push(rem as u8);
            body.extend_from_slice(pkt);
        }
        let mut page = std::vec::Vec::with_capacity(27 + seg_table.len() + body.len());
        page.extend_from_slice(b"OggS");
        page.push(0); // version
        page.push(header_type);
        page.extend_from_slice(&granule_pos.to_le_bytes());
        page.extend_from_slice(&serial.to_le_bytes());
        page.extend_from_slice(&seq_no.to_le_bytes());
        page.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder at 22..26
        page.push(seg_table.len() as u8);
        page.extend_from_slice(&seg_table);
        page.extend_from_slice(&body);
        let crc = ogg_crc32(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        page
    }

    #[test]
    fn test_ogg_flac_container_unwrap_and_bit_exact_parity() {
        let raw_flac = include_bytes!("../tests/fixtures/stereo_16bit.flac");
        assert_eq!(&raw_flac[0..4], b"fLaC");

        // 1. Decode native FLAC fixture as reference
        let mut ref_dec = FlacDecoder::new();
        let mut ref_pcm = std::vec::Vec::new();
        let mut pcm_buf = [0i32; 1024];
        let mut off = 0usize;
        while off < raw_flac.len()
            || ref_dec.has_pending_pcm()
            || ref_dec.stream_buf.len() > ref_dec.header_len
        {
            let end = (off + 512).min(raw_flac.len());
            match ref_dec.decode(&raw_flac[off..end], &mut pcm_buf) {
                Ok((consumed, frames)) => {
                    off += consumed;
                    ref_pcm.extend_from_slice(&pcm_buf[..frames * 2]);
                    if consumed == 0 && frames == 0 && off >= raw_flac.len() {
                        break;
                    }
                }
                Err(DecoderError::NeedMoreData) => {
                    if off >= raw_flac.len() && !ref_dec.has_pending_pcm() {
                        break;
                    }
                }
                Err(e) => panic!("Reference FLAC decode failed: {:?}", e),
            }
        }
        assert!(!ref_pcm.is_empty());

        // 2. Wrap raw_flac into Ogg-FLAC pages:
        // Page 0 (BOS = 0x02): \x7fFLAC\x01\x00\x00\x00 + fLaC + STREAMINFO (42 bytes)
        let mut bos_pkt = std::vec::Vec::with_capacity(9 + 42);
        bos_pkt.extend_from_slice(b"\x7fFLAC\x01\x00\x00\x00");
        bos_pkt.extend_from_slice(&raw_flac[..42]);

        let mut ogg_stream = std::vec::Vec::new();
        ogg_stream.extend_from_slice(&build_ogg_page(0x02, 0, 0x1234_5678, 0, &[&bos_pkt]));

        // Wrap remaining FLAC bytes across multiple Ogg pages
        let audio_bytes = &raw_flac[42..];
        let mut seq = 1u32;
        let mut pos = 0usize;
        while pos < audio_bytes.len() {
            let chunk_end = (pos + 2048).min(audio_bytes.len());
            let hdr_type = if chunk_end == audio_bytes.len() {
                0x04
            } else {
                0x00
            };
            let page = build_ogg_page(
                hdr_type,
                (seq as u64) * 1024,
                0x1234_5678,
                seq,
                &[&audio_bytes[pos..chunk_end]],
            );
            ogg_stream.extend_from_slice(&page);
            pos = chunk_end;
            seq += 1;
        }

        // 3. Decode Ogg-FLAC stream in 512-byte chunks and assert bit-exact parity
        let mut ogg_dec = FlacDecoder::new();
        let mut ogg_pcm = std::vec::Vec::new();
        let mut ogg_off = 0usize;
        while ogg_off < ogg_stream.len()
            || ogg_dec.has_pending_pcm()
            || ogg_dec.stream_buf.len() > ogg_dec.header_len
        {
            let end = (ogg_off + 512).min(ogg_stream.len());
            match ogg_dec.decode(&ogg_stream[ogg_off..end], &mut pcm_buf) {
                Ok((consumed, frames)) => {
                    ogg_off += consumed;
                    ogg_pcm.extend_from_slice(&pcm_buf[..frames * 2]);
                    if consumed == 0 && frames == 0 && ogg_off >= ogg_stream.len() {
                        break;
                    }
                }
                Err(DecoderError::NeedMoreData) => {
                    if ogg_off >= ogg_stream.len() && !ogg_dec.has_pending_pcm() {
                        break;
                    }
                }
                Err(e) => panic!("Ogg-FLAC decode failed: {:?}", e),
            }
        }
        assert_eq!(ogg_pcm.len(), ref_pcm.len());
        assert_eq!(ogg_pcm, ref_pcm);

        // 4. Verify corrupted Ogg page CRC is rejected with CorruptedFrame
        let mut bad_ogg = ogg_stream.clone();
        bad_ogg[22] ^= 0xFF;
        let mut bad_dec = FlacDecoder::new();
        assert_eq!(
            bad_dec.decode(&bad_ogg, &mut pcm_buf),
            Err(DecoderError::CorruptedFrame)
        );
    }

    #[test]
    fn test_ogg_vorbis_and_opus_crc_and_overflow_protection() {
        let vorbis_fixture = include_bytes!("../tests/fixtures/sine_440_1000_stereo_44k1.ogg");
        let opus_fixture = include_bytes!("../tests/fixtures/test_stereo_05s_48k.opus");
        let mut pcm_buf = [0i32; 1024];

        // 1. SEC-301: Vorbis CRC validation
        let mut bad_vorbis = vorbis_fixture.to_vec();
        bad_vorbis[22] ^= 0xFF;
        let mut vorbis_dec = VorbisDecoder::new();
        assert_eq!(
            vorbis_dec.decode(&bad_vorbis, &mut pcm_buf),
            Err(DecoderError::CorruptedFrame)
        );

        // 2. SEC-301: Opus CRC validation
        let mut bad_opus = opus_fixture.to_vec();
        bad_opus[22] ^= 0xFF;
        let mut opus_dec = OpusDecoder::new();
        assert_eq!(
            opus_dec.decode(&bad_opus, &mut pcm_buf),
            Err(DecoderError::CorruptedFrame)
        );

        // 3. PERF-303: Vorbis constrained decode breaks loop early and keeps
        // remaining pages
        let mut v_dec = VorbisDecoder::new();
        let mut small_pcm = [0i32; 128]; // max_frames = 64
        let (consumed, frames) = v_dec.decode(vorbis_fixture, &mut small_pcm).unwrap();
        assert!(consumed > 0);
        assert_eq!(frames, 64);
        assert!(v_dec.has_pending_pcm());
    }

    fn make_mp4_box(fourcc: &[u8; 4], payload: &[u8]) -> std::vec::Vec<u8> {
        let total = (8 + payload.len()) as u32;
        let mut out = std::vec::Vec::with_capacity(total as usize);
        out.extend_from_slice(&total.to_be_bytes());
        out.extend_from_slice(fourcc);
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn test_aac_mp4_m4a_esds_demux_and_adts_synthesis_parity() {
        warm_up_decoders();
        let adts_fixture = include_bytes!("../tests/fixtures/sine_440_1000_stereo_44k1_lc.aac");

        // 1. Extract raw AAC access units from the ADTS fixture (first 12 frames)
        let mut raw_aus: std::vec::Vec<&[u8]> = std::vec::Vec::new();
        let mut adts_prefix = std::vec::Vec::new();
        let mut pos = 0usize;
        while pos + 7 <= adts_fixture.len() && raw_aus.len() < 12 {
            assert_eq!(adts_fixture[pos], 0xFF);
            assert_eq!(adts_fixture[pos + 1] & 0xF0, 0xF0);
            let frame_len = (((adts_fixture[pos + 3] as usize) & 0x03) << 11)
                | ((adts_fixture[pos + 4] as usize) << 3)
                | (((adts_fixture[pos + 5] as usize) >> 5) & 0x07);
            assert!(frame_len > 7 && pos + frame_len <= adts_fixture.len());
            adts_prefix.extend_from_slice(&adts_fixture[pos..pos + frame_len]);
            raw_aus.push(&adts_fixture[pos + 7..pos + frame_len]);
            pos += frame_len;
        }
        assert_eq!(raw_aus.len(), 12);

        // 2. Decode the 12-frame ADTS stream directly as reference
        let mut ref_dec = AacDecoder::new();
        let mut ref_pcm = std::vec::Vec::new();
        let mut pcm_buf = [0i32; 2048];
        let (consumed_ref, frames_ref) = ref_dec.decode(&adts_prefix, &mut pcm_buf).unwrap();
        assert_eq!(consumed_ref, adts_prefix.len());
        ref_pcm.extend_from_slice(&pcm_buf[..frames_ref * 2]);
        while ref_dec.has_pending_pcm() {
            let (_, f) = ref_dec.decode(&[], &mut pcm_buf).unwrap();
            ref_pcm.extend_from_slice(&pcm_buf[..f * 2]);
        }
        assert_eq!(ref_pcm.len(), 12 * 1024 * 2);

        // 3. Build an ISO-BMFF M4A container with ftyp + moov (trak -> mdia -> minf
        //    -> stbl -> stsd -> mp4a -> esds + stsc + stsz + stco) + mdat
        let ftyp = make_mp4_box(b"ftyp", b"M4A \x00\x00\x02\x00M4A mp42isom");

        // esds payload: version/flags (4) + ES_Descriptor (0x03) + DecoderConfig (0x04)
        // + DecoderSpecificInfo (0x05) with AudioSpecificConfig [0x12, 0x10]
        // (AAC-LC = 2, 44100 Hz index = 4, Stereo = 2)
        let esds_payload: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, // version + flags
            0x03, 23, 0x00, 0x01, 0x00, // ES_Descriptor
            0x04, 15, 0x40, 0x15, 0x00, 0x00, 0x00, 0x00, 0x01, 0xF4, 0x00, 0x00, 0x01, 0xF4,
            0x00, // DecoderConfigDescriptor (objectType = 0x40)
            0x05, 2, 0x12, 0x10, // DecoderSpecificInfo (AudioSpecificConfig)
        ];
        let esds_box = make_mp4_box(b"esds", esds_payload);

        let mut mp4a_payload = std::vec![0u8; 28]; // 36-byte mp4a header = 8 box + 28 payload
        mp4a_payload[6..8].copy_from_slice(&1u16.to_be_bytes()); // data_reference_index
        mp4a_payload[16..18].copy_from_slice(&2u16.to_be_bytes()); // channels = 2
        mp4a_payload[18..20].copy_from_slice(&16u16.to_be_bytes()); // sample_size = 16
        mp4a_payload[24..28].copy_from_slice(&(44100u32 << 16).to_be_bytes());
        mp4a_payload.extend_from_slice(&esds_box);
        let mp4a_box = make_mp4_box(b"mp4a", &mp4a_payload);

        let mut stsd_payload = std::vec![0u8; 8];
        stsd_payload[4..8].copy_from_slice(&1u32.to_be_bytes()); // 1 entry
        stsd_payload.extend_from_slice(&mp4a_box);
        let stsd_box = make_mp4_box(b"stsd", &stsd_payload);

        // stsc: 1 entry -> first_chunk = 1, samples_per_chunk = 6, sample_desc_idx = 1
        let mut stsc_payload = std::vec![0u8; 8];
        stsc_payload[4..8].copy_from_slice(&1u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        stsc_payload.extend_from_slice(&6u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        let stsc_box = make_mp4_box(b"stsc", &stsc_payload);

        // stsz: default_sz = 0, count = 12
        let mut stsz_payload = std::vec![0u8; 12];
        stsz_payload[8..12].copy_from_slice(&(raw_aus.len() as u32).to_be_bytes());
        for au in &raw_aus {
            stsz_payload.extend_from_slice(&(au.len() as u32).to_be_bytes());
        }
        let stsz_box = make_mp4_box(b"stsz", &stsz_payload);

        // Compute chunk offsets for 2 chunks of 6 samples each with a 16-byte gap
        // between chunk 0 and chunk 1 inside mdat
        let chunk0_len: usize = raw_aus[0..6].iter().map(|a| a.len()).sum();
        let inter_chunk_gap = 16usize;
        let placeholder_stco = make_mp4_box(b"stco", &[0u8; 16]);
        let stbl_len =
            8 + stsd_box.len() + stsc_box.len() + stsz_box.len() + placeholder_stco.len();
        let moov_len = 8 + 8 + 8 + 8 + stbl_len; // moov -> trak -> mdia -> minf -> stbl
        let mdat_data_start = ftyp.len() + moov_len + 8 + 12; // +12 leading pad inside mdat
        let chunk0_off = mdat_data_start as u32;
        let chunk1_off = (mdat_data_start + chunk0_len + inter_chunk_gap) as u32;

        let mut stco_payload = std::vec![0u8; 8];
        stco_payload[4..8].copy_from_slice(&2u32.to_be_bytes()); // 2 chunks
        stco_payload.extend_from_slice(&chunk0_off.to_be_bytes());
        stco_payload.extend_from_slice(&chunk1_off.to_be_bytes());
        let stco_box = make_mp4_box(b"stco", &stco_payload);

        let mut stbl_payload = std::vec::Vec::new();
        stbl_payload.extend_from_slice(&stsd_box);
        stbl_payload.extend_from_slice(&stsc_box);
        stbl_payload.extend_from_slice(&stsz_box);
        stbl_payload.extend_from_slice(&stco_box);
        let stbl_box = make_mp4_box(b"stbl", &stbl_payload);
        let minf_box = make_mp4_box(b"minf", &stbl_box);
        let mdia_box = make_mp4_box(b"mdia", &minf_box);
        let trak_box = make_mp4_box(b"trak", &mdia_box);
        let moov_box = make_mp4_box(b"moov", &trak_box);

        let mut mdat_payload = std::vec![0xAAu8; 12]; // 12-byte pre-chunk padding
        for au in &raw_aus[0..6] {
            mdat_payload.extend_from_slice(au);
        }
        mdat_payload.extend_from_slice(&[0xBBu8; 16]); // 16-byte inter-chunk gap
        for au in &raw_aus[6..12] {
            mdat_payload.extend_from_slice(au);
        }
        let mdat_box = make_mp4_box(b"mdat", &mdat_payload);

        let mut m4a_stream = std::vec::Vec::new();
        m4a_stream.extend_from_slice(&ftyp);
        m4a_stream.extend_from_slice(&moov_box);
        m4a_stream.extend_from_slice(&mdat_box);

        // 4. Decode M4A stream in 256-byte chunks and verify bit-exact PCM parity
        let mut m4a_dec = AacDecoder::new();
        let mut m4a_pcm = std::vec::Vec::new();
        let mut m4a_off = 0usize;
        while m4a_off < m4a_stream.len() || m4a_dec.has_pending_pcm() {
            let end = (m4a_off + 256).min(m4a_stream.len());
            let chunk = &m4a_stream[m4a_off..end];
            match m4a_dec.decode(chunk, &mut pcm_buf) {
                Ok((consumed, frames)) => {
                    m4a_off += consumed;
                    m4a_pcm.extend_from_slice(&pcm_buf[..frames * 2]);
                    if consumed == 0 && frames == 0 && m4a_off >= m4a_stream.len() {
                        break;
                    }
                }
                Err(DecoderError::NeedMoreData) => {
                    if m4a_off >= m4a_stream.len() && !m4a_dec.has_pending_pcm() {
                        break;
                    }
                }
                Err(e) => panic!("M4A decode failed: {:?}", e),
            }
        }

        let asc = m4a_dec.audio_specific_config().unwrap();
        assert_eq!(asc.audio_object_type, 2);
        assert_eq!(asc.sampling_frequency_index, 4);
        assert_eq!(asc.channel_configuration, 2);
        assert_eq!(asc.sample_rate, 44100);
        assert_eq!(m4a_dec.sample_rate, 44100);
        assert_eq!(m4a_dec.channels, 2);
        assert_eq!(m4a_pcm.len(), ref_pcm.len());
        assert_eq!(m4a_pcm, ref_pcm);
    }

    #[test]
    fn test_decoder_memory_budget_and_unsupported_feature_ceilings() {
        // 1. DecodeError::UnsupportedFeature aliases
        // DecoderError::UnsupportedSampleFormat
        assert_eq!(
            DecodeError::UnsupportedFeature,
            DecoderError::UnsupportedSampleFormat
        );

        // 2. PcmDecoder rejects sample rate > 192 kHz with UnsupportedFeature
        let mut pcm = PcmDecoder::new(16, 2, false);
        pcm.sample_rate = Some(384_000);
        let mut pcm_out = [0i32; 16];
        assert_eq!(
            pcm.decode(&[0u8; 16], &mut pcm_out),
            Err(DecodeError::UnsupportedFeature)
        );

        // 3. AacDecoder esds rejects >2 channels (5.1ch = 6) with UnsupportedFeature
        let esds_5_1: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x05, 0x02, 0x12, 0x30, // AOT=2, sr_idx=4, ch=6
        ];
        assert_eq!(
            parse_esds_audio_specific_config(esds_5_1),
            Err(DecodeError::UnsupportedFeature)
        );

        // 4. AacDecoder MP4 stsz rejects sample count > MAX_MP4_SAMPLES
        let ftyp = make_mp4_box(b"ftyp", b"M4A \x00\x00\x02\x00");
        let mut stsz_oversized = std::vec![0u8; 12];
        stsz_oversized[0..4].copy_from_slice(&0u32.to_be_bytes());
        stsz_oversized[4..8].copy_from_slice(&256u32.to_be_bytes()); // default_sz = 256
        stsz_oversized[8..12].copy_from_slice(&((MAX_MP4_SAMPLES as u32) + 1).to_be_bytes());
        let stsz_box = make_mp4_box(b"stsz", &stsz_oversized);
        let moov_box = make_mp4_box(b"moov", &stsz_box);
        let mut bad_m4a = std::vec::Vec::new();
        bad_m4a.extend_from_slice(&ftyp);
        bad_m4a.extend_from_slice(&moov_box);

        let mut aac_dec = AacDecoder::new();
        assert_eq!(
            aac_dec.decode(&bad_m4a, &mut pcm_out),
            Err(DecodeError::UnsupportedFeature)
        );
    }

    #[test]
    fn test_bitreader_u64_and_signed() {
        // Test 33-bit and 64-bit signed and unsigned reading
        let data = [
            0x80, 0x00, 0x00, 0x00,
            0x01, // 40 bits; first 33 bits: bit 32 is 1 (sign bit), bit 0 is 0
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // 64 bits of 1s: -1i64
        ];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits_u64(0), Ok(0));
        assert_eq!(br.read_signed(0), Ok(0));

        // Read 33 bits: 0x80, 0x00, 0x00, 0x00, high bit of 0x01 = 0
        // Value: 1u64 << 32
        let val33 = br.read_signed(33).expect("read 33 bits signed");
        // bit 32 is sign bit, so this is negative: -(1i64 << 32)
        assert_eq!(val33, -(1i64 << 32));

        // Read remaining 7 bits of byte 4 + 57 bits of next bytes = 64 bits
        let mut br2 = BitReader::new(&data[5..]);
        let val64 = br2.read_signed(64).expect("read 64 bits signed");
        assert_eq!(val64, -1i64);

        let mut br3 = BitReader::new(&[0x01, 0x00, 0x00, 0x00, 0x00]);
        // 33 bits where sign bit (bit 32) is 0: 0x0100000000 >> 7 = 0x02000000
        let pos33 = br3.read_signed(33).expect("read positive 33 bits");
        assert!(pos33 > 0);
    }
}
