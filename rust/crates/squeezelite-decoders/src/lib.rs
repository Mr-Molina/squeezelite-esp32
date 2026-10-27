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
use std::vec;

#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::boxed::Box;
#[cfg(all(feature = "alloc", not(feature = "std")))]
use alloc::vec;

#[cfg(not(feature = "std"))]
use core::fmt;

/// Maximum stream buffer size for compressed audio decoders (FLAC, Vorbis) (PERF-201).
pub const MAX_STREAM_BUF: usize = 65536;

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
    #[cfg_attr(feature = "std", error("Target PCM buffer too small for decoded frame"))]
    BufferOverflow,
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
    /// Decodes encoded audio bytes from `encoded_in` and outputs interleaved 32-bit PCM samples to `pcm_out`.
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
        bytes[2], bytes[3], bytes[4], bytes[5],
        bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    if exponent >= 0 && exponent <= 31 {
        (mantissa >> (63 - exponent)) as u32
    } else {
        0
    }
}

/// PCM Passthrough Decoder (16-bit / 24-bit / 32-bit, Big or Little Endian, Mono or Stereo).
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

    /// Automatically detects and strips WAV (RIFF...WAVE) and AIFF (FORM...AIFF) container headers.
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
                    if offset.checked_add(24).is_none_or(|end| end > data.len()) {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let fmt_payload = &data[offset + 8..];
                    let num_channels = u16::from_le_bytes([fmt_payload[2], fmt_payload[3]]);
                    let sample_rate = u32::from_le_bytes([
                        fmt_payload[4],
                        fmt_payload[5],
                        fmt_payload[6],
                        fmt_payload[7],
                    ]);
                    let bits_per_sample = u16::from_le_bytes([fmt_payload[14], fmt_payload[15]]);

                    if num_channels == 1 || num_channels == 2 {
                        self.channels = num_channels as u8;
                    }
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
                    if offset.checked_add(26).is_none_or(|end| end > data.len()) {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let comm_payload = &data[offset + 8..];
                    let num_channels = i16::from_be_bytes([comm_payload[0], comm_payload[1]]);
                    let sample_size = i16::from_be_bytes([comm_payload[6], comm_payload[7]]);

                    let mut ext = [0u8; 10];
                    ext.copy_from_slice(&comm_payload[8..18]);
                    let sample_rate = parse_aiff_extended_float(&ext);

                    if num_channels == 1 || num_channels == 2 {
                        self.channels = num_channels as u8;
                    }
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
            // 8-bit Stereo (unsigned PCM centered at 128)
            (8, _, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 2;
                    let l = ((pcm_in[off] as i32) - 128) << 24;
                    let r = ((pcm_in[off + 1] as i32) - 128) << 24;
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
                }
            }
            // 8-bit Mono (unsigned PCM centered at 128, duplicated to stereo)
            (8, _, 1) => {
                for i in 0..frames_to_decode {
                    let off = i;
                    let s = ((pcm_in[off] as i32) - 128) << 24;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            // 16-bit Stereo Big-Endian (Common Squeezebox PCM format)
            (16, true, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let l = i16::from_be_bytes([pcm_in[off], pcm_in[off + 1]]) as i32;
                    let r = i16::from_be_bytes([pcm_in[off + 2], pcm_in[off + 3]]) as i32;
                    pcm_out[i * 2] = l << 16;
                    pcm_out[i * 2 + 1] = r << 16;
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
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let l = i16::from_le_bytes([pcm_in[off], pcm_in[off + 1]]) as i32;
                    let r = i16::from_le_bytes([pcm_in[off + 2], pcm_in[off + 3]]) as i32;
                    pcm_out[i * 2] = l << 16;
                    pcm_out[i * 2 + 1] = r << 16;
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
                for i in 0..frames_to_decode {
                    let off = i * 6;
                    let l = ((pcm_in[off] as i32) << 24)
                        | ((pcm_in[off + 1] as i32) << 16)
                        | ((pcm_in[off + 2] as i32) << 8);
                    let r = ((pcm_in[off + 3] as i32) << 24)
                        | ((pcm_in[off + 4] as i32) << 16)
                        | ((pcm_in[off + 5] as i32) << 8);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
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
                for i in 0..frames_to_decode {
                    let off = i * 6;
                    let l = ((pcm_in[off] as i32) << 8)
                        | ((pcm_in[off + 1] as i32) << 16)
                        | ((pcm_in[off + 2] as i32) << 24);
                    let r = ((pcm_in[off + 3] as i32) << 8)
                        | ((pcm_in[off + 4] as i32) << 16)
                        | ((pcm_in[off + 5] as i32) << 24);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
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
                for i in 0..frames_to_decode {
                    let off = i * 8;
                    let l = i32::from_be_bytes([
                        pcm_in[off],
                        pcm_in[off + 1],
                        pcm_in[off + 2],
                        pcm_in[off + 3],
                    ]);
                    let r = i32::from_be_bytes([
                        pcm_in[off + 4],
                        pcm_in[off + 5],
                        pcm_in[off + 6],
                        pcm_in[off + 7],
                    ]);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
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
                for i in 0..frames_to_decode {
                    let off = i * 8;
                    let l = i32::from_le_bytes([
                        pcm_in[off],
                        pcm_in[off + 1],
                        pcm_in[off + 2],
                        pcm_in[off + 3],
                    ]);
                    let r = i32::from_le_bytes([
                        pcm_in[off + 4],
                        pcm_in[off + 5],
                        pcm_in[off + 6],
                        pcm_in[off + 7],
                    ]);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
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

        Ok((header_len + frames_to_decode * bytes_per_frame, frames_to_decode))
    }

    fn flush(&mut self) {
        self.header_stripped = false;
        self.sample_rate = None;
    }
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
}

#[cfg(feature = "alloc")]
impl Default for Mp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
impl Mp3Decoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            decoder: Box::new(nanomp3_core::Decoder::new()),
            sample_buf: vec![0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME].into_boxed_slice(),
            id3_skip_remaining: 0,
        }
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

        let (consumed, result) = self.decoder.decode(encoded_in, &mut self.sample_buf);

        match result {
            Ok(info) => {
                if info.sample_rate > 0 {
                    self.sample_rate = info.sample_rate;
                }
                let channels = info.channels.num() as usize;
                let frames_produced = info.samples_produced;
                let max_frames = pcm_out.len() / 2;

                if max_frames < frames_produced {
                    return Err(DecoderError::BufferOverflow);
                }

                if channels == 2 {
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
                Ok((consumed, frames_produced))
            }
            Err(nanomp3_core::DecodeError::NoFrame) => {
                if consumed > 0 {
                    Ok((consumed, 0))
                } else {
                    Err(DecoderError::NeedMoreData)
                }
            }
            Err(nanomp3_core::DecodeError::ReservoirUnavailable(_)) => {
                // DEC-005: Differentiate between 0 consumed (need more bytes) and >0 consumed (absorbed into reservoir)
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
    }
}

/// Zero-heap, `no_std`, `const fn`-initializable MP3 decoder for embedded targets (`xtensa`).
///
/// Decodes directly into `pcm_out` (`>= 2304` `i32` samples) with zero heap allocations
/// and resets in O(1) via `nanomp3_core::__private::init` without constructing a 23 KB temporary.
pub struct StaticMp3Decoder {
    pub sample_rate: u32,
    decoder: nanomp3_core::Decoder,
    id3_skip_remaining: usize,
}

impl Default for StaticMp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticMp3Decoder {
    pub const fn new() -> Self {
        Self {
            sample_rate: 0,
            decoder: nanomp3_core::Decoder::new(),
            id3_skip_remaining: 0,
        }
    }

    /// Resets the decoder state in-place without constructing a 23 KB temporary on the stack.
    pub fn reset_in_place(&mut self) {
        self.sample_rate = 44100;
        nanomp3_core::__private::init(&mut self.decoder);
        self.id3_skip_remaining = 0;
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
                if frame_bytes == 0 || frame_bytes + 4 > 1728 {
                    return Ok((1, 0));
                }
                if encoded_in.len() < frame_bytes + 4 {
                    if encoded_in.len() >= frame_bytes + 2
                        && (encoded_in[frame_bytes] != 0xFF
                            || (encoded_in[frame_bytes + 1] & 0xE0) != 0xE0)
                    {
                        return Ok((1, 0));
                    }
                    return Err(DecoderError::NeedMoreData);
                }
                let next_h = nanomp3_core::__private::Header::read(&encoded_in[frame_bytes..]);
                if !h.compare(next_h) {
                    return Ok((1, 0));
                }
                (frame_bytes, &encoded_in[..frame_bytes + 4])
            }
            _ => unreachable!(),
        };

        let (consumed, result) = self.decoder.decode(slice_to_decode, pcm_out);

        match result {
            Ok(info) => {
                if info.sample_rate > 0 {
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
                Ok((consumed, frames_produced))
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
    0x00, 0x07, 0x0e, 0x09, 0x1c, 0x1b, 0x12, 0x15, 0x38, 0x3f, 0x36, 0x31, 0x24, 0x23, 0x2a,
    0x2d, 0x70, 0x77, 0x7e, 0x79, 0x6c, 0x6b, 0x62, 0x65, 0x48, 0x4f, 0x46, 0x41, 0x54, 0x53,
    0x5a, 0x5d, 0xe0, 0xe7, 0xee, 0xe9, 0xfc, 0xfb, 0xf2, 0xf5, 0xd8, 0xdf, 0xd6, 0xd1, 0xc4,
    0xc3, 0xca, 0xcd, 0x90, 0x97, 0x9e, 0x99, 0x8c, 0x8b, 0x82, 0x85, 0xa8, 0xaf, 0xa6, 0xa1,
    0xb4, 0xb3, 0xba, 0xbd, 0xc7, 0xc0, 0xc9, 0xce, 0xdb, 0xdc, 0xd5, 0xd2, 0xff, 0xf8, 0xf1,
    0xf6, 0xe3, 0xe4, 0xed, 0xea, 0xb7, 0xb0, 0xb9, 0xbe, 0xab, 0xac, 0xa5, 0xa2, 0x8f, 0x88,
    0x81, 0x86, 0x93, 0x94, 0x9d, 0x9a, 0x27, 0x20, 0x29, 0x2e, 0x3b, 0x3c, 0x35, 0x32, 0x1f,
    0x18, 0x11, 0x16, 0x03, 0x04, 0x0d, 0x0a, 0x57, 0x50, 0x59, 0x5e, 0x4b, 0x4c, 0x45, 0x42,
    0x6f, 0x68, 0x61, 0x66, 0x73, 0x74, 0x7d, 0x7a, 0x89, 0x8e, 0x87, 0x80, 0x95, 0x92, 0x9b,
    0x9c, 0xb1, 0xb6, 0xbf, 0xb8, 0xad, 0xaa, 0xa3, 0xa4, 0xf9, 0xfe, 0xf7, 0xf0, 0xe5, 0xe2,
    0xeb, 0xec, 0xc1, 0xc6, 0xcf, 0xc8, 0xdd, 0xda, 0xd3, 0xd4, 0x69, 0x6e, 0x67, 0x60, 0x75,
    0x72, 0x7b, 0x7c, 0x51, 0x56, 0x5f, 0x58, 0x4d, 0x4a, 0x43, 0x44, 0x19, 0x1e, 0x17, 0x10,
    0x05, 0x02, 0x0b, 0x0c, 0x21, 0x26, 0x2f, 0x28, 0x3d, 0x3a, 0x33, 0x34, 0x4e, 0x49, 0x40,
    0x47, 0x52, 0x55, 0x5c, 0x5b, 0x76, 0x71, 0x78, 0x7f, 0x6a, 0x6d, 0x64, 0x63, 0x3e, 0x39,
    0x30, 0x37, 0x22, 0x25, 0x2c, 0x2b, 0x06, 0x01, 0x08, 0x0f, 0x1a, 0x1d, 0x14, 0x13, 0xae,
    0xa9, 0xa0, 0xa7, 0xb2, 0xb5, 0xbc, 0xbb, 0x96, 0x91, 0x98, 0x9f, 0x8a, 0x8d, 0x84, 0x83,
    0xde, 0xd9, 0xd0, 0xd7, 0xc2, 0xc5, 0xcc, 0xcb, 0xe6, 0xe1, 0xe8, 0xef, 0xfa, 0xfd, 0xf4,
    0xf3,
];

const CRC16_TABLE: [u16; 256] = [
    0x0000, 0x8005, 0x800f, 0x000a, 0x801b, 0x001e, 0x0014, 0x8011, 0x8033, 0x0036, 0x003c,
    0x8039, 0x0028, 0x802d, 0x8027, 0x0022, 0x8063, 0x0066, 0x006c, 0x8069, 0x0078, 0x807d,
    0x8077, 0x0072, 0x0050, 0x8055, 0x805f, 0x005a, 0x804b, 0x004e, 0x0044, 0x8041, 0x80c3,
    0x00c6, 0x00cc, 0x80c9, 0x00d8, 0x80dd, 0x80d7, 0x00d2, 0x00f0, 0x80f5, 0x80ff, 0x00fa,
    0x80eb, 0x00ee, 0x00e4, 0x80e1, 0x00a0, 0x80a5, 0x80af, 0x00aa, 0x80bb, 0x00be, 0x00b4,
    0x80b1, 0x8093, 0x0096, 0x009c, 0x8099, 0x0088, 0x808d, 0x8087, 0x0082, 0x8183, 0x0186,
    0x018c, 0x8189, 0x0198, 0x819d, 0x8197, 0x0192, 0x01b0, 0x81b5, 0x81bf, 0x01ba, 0x81ab,
    0x01ae, 0x01a4, 0x81a1, 0x01e0, 0x81e5, 0x81ef, 0x01ea, 0x81fb, 0x01fe, 0x01f4, 0x81f1,
    0x81d3, 0x01d6, 0x01dc, 0x81d9, 0x01c8, 0x81cd, 0x81c7, 0x01c2, 0x0140, 0x8145, 0x814f,
    0x014a, 0x815b, 0x015e, 0x0154, 0x8151, 0x8173, 0x0176, 0x017c, 0x8179, 0x0168, 0x816d,
    0x8167, 0x0162, 0x8123, 0x0126, 0x012c, 0x8129, 0x0138, 0x813d, 0x8137, 0x0132, 0x0110,
    0x8115, 0x811f, 0x011a, 0x810b, 0x010e, 0x0104, 0x8101, 0x8303, 0x0306, 0x030c, 0x8309,
    0x0318, 0x831d, 0x8317, 0x0312, 0x0330, 0x8335, 0x833f, 0x033a, 0x832b, 0x032e, 0x0324,
    0x8321, 0x0360, 0x8365, 0x836f, 0x036a, 0x837b, 0x037e, 0x0374, 0x8371, 0x8353, 0x0356,
    0x035c, 0x8359, 0x0348, 0x834d, 0x8347, 0x0342, 0x03c0, 0x83c5, 0x83cf, 0x03ca, 0x83db,
    0x03de, 0x03d4, 0x83d1, 0x83f3, 0x03f6, 0x03fc, 0x83f9, 0x03e8, 0x83ed, 0x83e7, 0x03e2,
    0x83a3, 0x03a6, 0x03ac, 0x83a9, 0x03b8, 0x83bd, 0x83b7, 0x03b2, 0x0390, 0x8395, 0x839f,
    0x039a, 0x838b, 0x038e, 0x0384, 0x8381, 0x0280, 0x8285, 0x828f, 0x028a, 0x829b, 0x029e,
    0x0294, 0x8291, 0x82b3, 0x02b6, 0x02bc, 0x82b9, 0x02a8, 0x82ad, 0x82a7, 0x02a2, 0x82e3,
    0x02e6, 0x02ec, 0x82e9, 0x02f8, 0x82fd, 0x82f7, 0x02f2, 0x02d0, 0x82d5, 0x82df, 0x02da,
    0x82cb, 0x02ce, 0x02c4, 0x82c1, 0x8243, 0x0246, 0x024c, 0x8249, 0x0258, 0x825d, 0x8257,
    0x0252, 0x0270, 0x8275, 0x827f, 0x027a, 0x826b, 0x026e, 0x0264, 0x8261, 0x0220, 0x8225,
    0x822f, 0x022a, 0x823b, 0x023e, 0x0234, 0x8231, 0x8213, 0x0216, 0x021c, 0x8219, 0x0208,
    0x820d, 0x8207, 0x0202,
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
    fn read_signed(&mut self, n: usize) -> Result<i64, ()> {
        if n == 0 {
            return Ok(0);
        }
        let u = self.read_bits(n)? as u64;
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
        if idx + 2 > buf.len() { return Err(()); }
        let u1 = buf[idx + 1];
        if (u1 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u0 as u64 & 0x1F) << 6) | (u1 as u64 & 0x3F);
        return Ok((val, idx + 2));
    }
    if (u0 & 0xF0) == 0xE0 {
        if idx + 3 > buf.len() { return Err(()); }
        let (u1, u2) = (buf[idx + 1], buf[idx + 2]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u0 as u64 & 0x0F) << 12) | ((u1 as u64 & 0x3F) << 6) | (u2 as u64 & 0x3F);
        return Ok((val, idx + 3));
    }
    if (u0 & 0xF8) == 0xF0 {
        if idx + 4 > buf.len() { return Err(()); }
        let (u1, u2, u3) = (buf[idx + 1], buf[idx + 2], buf[idx + 3]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u0 as u64 & 0x07) << 18) | ((u1 as u64 & 0x3F) << 12) | ((u2 as u64 & 0x3F) << 6) | (u3 as u64 & 0x3F);
        return Ok((val, idx + 4));
    }
    if (u0 & 0xFC) == 0xF8 {
        if idx + 5 > buf.len() { return Err(()); }
        let (u1, u2, u3, u4) = (buf[idx + 1], buf[idx + 2], buf[idx + 3], buf[idx + 4]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 || (u4 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u0 as u64 & 0x03) << 24) | ((u1 as u64 & 0x3F) << 18) | ((u2 as u64 & 0x3F) << 12) | ((u3 as u64 & 0x3F) << 6) | (u4 as u64 & 0x3F);
        return Ok((val, idx + 5));
    }
    if (u0 & 0xFE) == 0xFC {
        if idx + 6 > buf.len() { return Err(()); }
        let (u1, u2, u3, u4, u5) = (buf[idx + 1], buf[idx + 2], buf[idx + 3], buf[idx + 4], buf[idx + 5]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 || (u4 & 0xC0) != 0x80 || (u5 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u0 as u64 & 0x01) << 30) | ((u1 as u64 & 0x3F) << 24) | ((u2 as u64 & 0x3F) << 18) | ((u3 as u64 & 0x3F) << 12) | ((u4 as u64 & 0x3F) << 6) | (u5 as u64 & 0x3F);
        return Ok((val, idx + 6));
    }
    if u0 == 0xFE {
        if idx + 7 > buf.len() { return Err(()); }
        let (u1, u2, u3, u4, u5, u6) = (buf[idx + 1], buf[idx + 2], buf[idx + 3], buf[idx + 4], buf[idx + 5], buf[idx + 6]);
        if (u1 & 0xC0) != 0x80 || (u2 & 0xC0) != 0x80 || (u3 & 0xC0) != 0x80 || (u4 & 0xC0) != 0x80 || (u5 & 0xC0) != 0x80 || (u6 & 0xC0) != 0x80 { return Err(()); }
        let val = ((u1 as u64 & 0x3F) << 30) | ((u2 as u64 & 0x3F) << 24) | ((u3 as u64 & 0x3F) << 18) | ((u4 as u64 & 0x3F) << 12) | ((u5 as u64 & 0x3F) << 6) | (u6 as u64 & 0x3F);
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
            if buf.len() < 12 { return Ok(None); }
            return Err(());
        }
    };

    let block_size = match block_size_code {
        0 => {
            if min_bs > 0 { min_bs as usize } else { return Err(()); }
        }
        1 => 192,
        2..=5 => 576 * (1usize << (block_size_code - 2)),
        6 => {
            if idx >= buf.len() { return Ok(None); }
            let bs = (buf[idx] as usize) + 1;
            idx += 1;
            bs
        }
        7 => {
            if idx + 2 > buf.len() { return Ok(None); }
            let bs = (u16::from_be_bytes([buf[idx], buf[idx + 1]]) as usize) + 1;
            idx += 2;
            bs
        }
        8..=15 => 256 * (1usize << (block_size_code - 8)),
        _ => return Err(()),
    };
    if block_size == 0 || block_size > 4096 {
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
            if idx >= buf.len() { return Ok(None); }
            let sr = (buf[idx] as u32) * 1000;
            idx += 1;
            sr
        }
        13 => {
            if idx + 2 > buf.len() { return Ok(None); }
            let sr = u16::from_be_bytes([buf[idx], buf[idx + 1]]) as u32;
            idx += 2;
            sr
        }
        14 => {
            if idx + 2 > buf.len() { return Ok(None); }
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
                        out[i] += out[i - 1];
                    }
                }
                2 => {
                    for i in 2..block_size {
                        out[i] += 2 * out[i - 1] - out[i - 2];
                    }
                }
                3 => {
                    for i in 3..block_size {
                        out[i] += 3 * out[i - 1] - 3 * out[i - 2] + out[i - 3];
                    }
                }
                4 => {
                    for i in 4..block_size {
                        out[i] += 4 * out[i - 1] - 6 * out[i - 2] + 4 * out[i - 3] - out[i - 4];
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
            let qlp_prec = (reader.read_bits(4)? + 1) as usize;
            if qlp_prec > 15 {
                return Err(());
            }
            let qlp_shift = reader.read_signed(5)? as i32;
            let mut coeffs = [0i64; 32];
            for coeff in coeffs[..order].iter_mut() {
                *coeff = reader.read_signed(qlp_prec)?;
            }
            decode_residual(reader, block_size, order, &mut out[order..block_size])?;

            for i in order..block_size {
                let mut sum: i64 = 0;
                for j in 0..order {
                    sum += coeffs[j] * out[i - 1 - j];
                }
                let predicted = if qlp_shift >= 0 {
                    sum >> qlp_shift
                } else {
                    sum << (-qlp_shift)
                };
                out[i] += predicted;
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
                let rem = if k > 0 {
                    reader.read_bits(k as usize)? as u64
                } else {
                    0
                };
                let u = (q << k) | rem;
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
            let r = (scratch[4096 + i] as i32) << shift;
            (l, r)
        }
        8 => {
            let ch0 = scratch[i];
            let ch1 = scratch[4096 + i];
            let l = (ch0 as i32) << shift;
            let r = ((ch0 - ch1) as i32) << shift;
            (l, r)
        }
        9 => {
            let ch0 = scratch[i];
            let ch1 = scratch[4096 + i];
            let l = ((ch1 + ch0) as i32) << shift;
            let r = (ch1 as i32) << shift;
            (l, r)
        }
        10 => {
            let ch0 = scratch[i];
            let ch1 = scratch[4096 + i];
            let mid = (ch0 << 1) | (ch1 & 1);
            let l = (((mid + ch1) >> 1) as i32) << shift;
            let r = (((mid - ch1) >> 1) as i32) << shift;
            (l, r)
        }
        _ => {
            let l = (scratch[i] as i32) << shift;
            let r = (scratch[4096 + i] as i32) << shift;
            (l, r)
        }
    }
}

#[cfg(any(feature = "alloc", feature = "std"))]
/// Pure-Rust RFC 9639 Lossless FLAC Decoder with 64KB External PSRAM buffer placement.
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
            subframe_scratch: vec![0i64; 8192].into_boxed_slice(),
            meta_skip_remaining: 0,
            in_metadata: false,
            min_block_size: 0,
            max_block_size: 0,
            need_more_input: true,
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
        if self.header_len > 0 && self.stream_buf.len() >= self.header_len {
            self.stream_buf.truncate(self.header_len);
        } else {
            self.stream_buf.clear();
            self.header_len = 0;
        }
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

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

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

        if self.header_len > 0 && self.stream_buf.len() > self.header_len {
            if let Some(pos) = self.stream_buf[self.header_len..].windows(4).position(|w| w == b"fLaC") {
                let abs_pos = self.header_len + pos;
                self.stream_buf.drain(..abs_pos);
                self.header_len = 0;
                self.in_metadata = false;
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
                    return Err(DecoderError::UnsupportedSampleFormat);
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
            let block_len = ((self.stream_buf[self.header_len + 1] as usize) << 16)
                | ((self.stream_buf[self.header_len + 2] as usize) << 8)
                | (self.stream_buf[self.header_len + 3] as usize);
            let total_block_bytes = 4 + block_len;
            let avail = self.stream_buf.len() - self.header_len;
            if avail >= total_block_bytes {
                self.stream_buf.drain(self.header_len..self.header_len + total_block_bytes);
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
                self.stream_buf.drain(self.header_len..self.header_len + sync_idx);
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
                let ch_start = ch * 4096;
                let ch_slice = &mut self.subframe_scratch[ch_start..ch_start + hdr.block_size];
                if let Err(()) = decode_subframe(&mut reader, subframe_bps, hdr.block_size, ch_slice) {
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

            let expected_crc = u16::from_be_bytes([frame_buf[frame_body_len], frame_buf[frame_body_len + 1]]);
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
                let (l, r) = decorrelate_samples(
                    &self.subframe_scratch,
                    i,
                    hdr.channel_assignment,
                    shift,
                );
                pcm_out[(frames_out + i) * 2] = l;
                pcm_out[(frames_out + i) * 2 + 1] = r;
            }
            frames_out += to_write;

            for i in to_write..block_size {
                let (l, r) = decorrelate_samples(
                    &self.subframe_scratch,
                    i,
                    hdr.channel_assignment,
                    shift,
                );
                self.pending_pcm.push(l);
                self.pending_pcm.push(r);
            }

            self.stream_buf.drain(self.header_len..self.header_len + total_frame_bytes);
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

#[cfg(feature = "std")]
/// Pure-Rust AAC (ADTS / LC / HE-AAC) Decoder using syom.
///
/// Propagates exact consumed byte counts returned by syom (DEC-004).
pub struct AacDecoder {
    decoder: syom::Decoder,
    pub sample_rate: u32,
    pub channels: u32,
    pending_pcm: std::vec::Vec<i32>,
}

#[cfg(feature = "std")]
impl Default for AacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl AacDecoder {
    pub fn new() -> Self {
        Self {
            decoder: syom::Decoder::new(syom::DecodeOptions::default()),
            sample_rate: 44100,
            channels: 2,
            pending_pcm: std::vec::Vec::new(),
        }
    }
}

#[cfg(feature = "std")]
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
        if !self.pending_pcm.is_empty() {
            let pending_frames = self.pending_pcm.len() / 2;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            pcm_out[..to_copy * 2].copy_from_slice(&self.pending_pcm[..to_copy * 2]);
            self.pending_pcm.drain(..to_copy * 2);
            frames_out += to_copy;
            if frames_out >= max_frames {
                return Ok((0, frames_out));
            }
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

        let feed_res = self.decoder.feed(encoded_in, |frame| {
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
                let l = (l_f * 2147483520.0) as i32; // Clamping boundary to avoid overflow (DSP-003)
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
        self.decoder = syom::Decoder::new(syom::DecodeOptions::default());
        self.pending_pcm.clear();
    }
}

#[cfg(feature = "std")]
/// Pure-Rust Ogg Vorbis Decoder using lewton with header/audio packet differentiation (DEC-003).
pub struct VorbisDecoder {
    pub sample_rate: u32,
    pub channels: u32,
    pub packets_decoded: usize,
    header_len: usize,
    stream_buf: std::vec::Vec<u8>,
    pending_samples: std::vec::Vec<i16>,
    pending_channels: usize,
}

#[cfg(feature = "std")]
impl Default for VorbisDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl VorbisDecoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            packets_decoded: 0,
            header_len: 0,
            stream_buf: std::vec::Vec::new(),
            pending_samples: std::vec::Vec::new(),
            pending_channels: 2,
        }
    }
}

#[cfg(feature = "std")]
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

        if self.stream_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
            self.stream_buf.clear();
            self.header_len = 0;
            self.pending_samples.clear();
            self.packets_decoded = 0;
            return Err(DecoderError::CorruptedFrame);
        }

        let mut frames_out = 0;
        if !self.pending_samples.is_empty() {
            let ch = if self.pending_channels == 2 { 2 } else { 1 };
            let pending_frames = self.pending_samples.len() / ch;
            let to_copy = core::cmp::min(pending_frames, max_frames);
            if ch == 2 {
                for i in 0..to_copy {
                    pcm_out[i * 2] = (self.pending_samples[i * 2] as i32) << 16;
                    pcm_out[i * 2 + 1] = (self.pending_samples[i * 2 + 1] as i32) << 16;
                }
            } else {
                for i in 0..to_copy {
                    let s = (self.pending_samples[i] as i32) << 16;
                    pcm_out[i * 2] = s;
                    pcm_out[i * 2 + 1] = s;
                }
            }
            self.pending_samples.drain(..to_copy * ch);
            frames_out += to_copy;
        }

        if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            if frames_out > 0 {
                return Ok((0, frames_out));
            }
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

        if frames_out >= max_frames {
            return Ok((input_taken, frames_out));
        }

        let cursor = std::io::Cursor::new(&self.stream_buf[..]);
        match lewton::inside_ogg::OggStreamReader::new(cursor) {
            Ok(mut reader) => {
                if self.header_len == 0 {
                    if let Ok(pr) = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(&self.stream_buf[..])) {
                        self.header_len = pr.into_inner().into_inner().position() as usize;
                    }
                }

                self.sample_rate = reader.ident_hdr.audio_sample_rate;
                self.channels = reader.ident_hdr.audio_channels as u32;

                if self.channels != 1 && self.channels != 2 {
                    return Err(DecoderError::UnsupportedSampleFormat);
                }

                let ch = self.channels as usize;

                // Advance reader past already-decoded packets to preserve MDCT overlap-add state
                let mut skipped = 0;
                while skipped < self.packets_decoded {
                    match reader.read_dec_packet_itl() {
                        Ok(Some(_)) => {
                            skipped += 1;
                        }
                        _ => break,
                    }
                }

                let mut stream_ended = false;
                loop {
                    if frames_out >= max_frames {
                        break;
                    }
                    match reader.read_dec_packet_itl() {
                        Ok(Some(samples_itl)) => {
                            self.packets_decoded += 1;
                            let packet_frames = samples_itl.len() / ch;
                            let available_space = max_frames.saturating_sub(frames_out);
                            let frames_to_write = core::cmp::min(packet_frames, available_space);

                            if ch == 2 {
                                for i in 0..frames_to_write {
                                    pcm_out[(frames_out + i) * 2] = (samples_itl[i * 2] as i32) << 16;
                                    pcm_out[(frames_out + i) * 2 + 1] =
                                        (samples_itl[i * 2 + 1] as i32) << 16;
                                }
                            } else {
                                for i in 0..frames_to_write {
                                    let s = (samples_itl[i] as i32) << 16;
                                    pcm_out[(frames_out + i) * 2] = s;
                                    pcm_out[(frames_out + i) * 2 + 1] = s;
                                }
                            }

                            frames_out += frames_to_write;
                            if frames_to_write < packet_frames {
                                self.pending_channels = ch;
                                self.pending_samples
                                    .extend_from_slice(&samples_itl[frames_to_write * ch..]);
                                break;
                            }
                        }
                        Ok(None) => {
                            stream_ended = true;
                            break;
                        }
                        Err(_) => break,
                    }
                }

                // Compact stream_buf only when an entire Ogg stream finishes (or flush is called)
                if stream_ended {
                    let consumed = reader.into_inner().into_inner().position() as usize;
                    if consumed > 0 && consumed <= self.stream_buf.len() {
                        self.stream_buf.drain(..consumed);
                    } else {
                        self.stream_buf.clear();
                    }
                    self.header_len = 0;
                    self.packets_decoded = 0;
                }

                if frames_out == 0 && input_taken == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((input_taken, frames_out))
                }
            }
            Err(lewton::VorbisError::BadHeader(lewton::header::HeaderReadError::EndOfPacket))
            | Err(lewton::VorbisError::OggError(_)) => {
                // Header incomplete: absorbed data into stream buffer, waiting for more data (DEC-003)
                if frames_out == 0 && input_taken == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((input_taken, frames_out))
                }
            }
            Err(_) => {
                if self.stream_buf.len() > MAX_STREAM_BUF {
                    self.stream_buf.clear();
                    self.header_len = 0;
                    self.packets_decoded = 0;
                    Err(DecoderError::CorruptedFrame)
                } else if frames_out == 0 && input_taken == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((input_taken, frames_out))
                }
            }
        }
    }

    fn flush(&mut self) {
        self.pending_samples.clear();
        self.packets_decoded = 0;
        if self.header_len > 0 && self.stream_buf.len() >= self.header_len {
            self.stream_buf.truncate(self.header_len);
        } else {
            self.stream_buf.clear();
            self.header_len = 0;
        }
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
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
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
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::BufferOverflow));
    }

    #[test]
    fn test_pcm_insufficient_data() {
        let mut decoder = PcmDecoder::new(16, 2, true);
        let encoded = [0u8; 3]; // 3 bytes is less than 1 frame (4 bytes)
        let mut pcm_out = [0i32; 4];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
    }

    #[test]
    fn test_mp3_heap_isolation_no_stack_overflow() {
        let mut decoder = Mp3Decoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = vec![0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));

        let mut small_pcm = [0i32; 32];
        assert_eq!(decoder.decode(&encoded, &mut small_pcm), Err(DecoderError::BufferOverflow));
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

        // Complete ID3v2 header with synchsafe size = 20 bytes (total = 10 + 20 = 30 bytes)
        let mut id3_pkt = [0u8; 34];
        id3_pkt[..3].copy_from_slice(b"ID3");
        id3_pkt[3] = 4; // v2.4
        id3_pkt[9] = 20; // size = 20
        let (consumed, frames) = decoder.decode(&id3_pkt, &mut pcm_out).unwrap();
        assert_eq!(consumed, 30);
        assert_eq!(frames, 0);

        // Non-sync padding bytes are skipped with Ok((consumed, 0)) rather than stalling
        let junk = [0u8; 8];
        let (consumed_junk, frames_junk) = decoder.decode(&junk, &mut pcm_out).unwrap();
        assert!(consumed_junk > 0);
        assert_eq!(frames_junk, 0);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flac_insufficient_data() {
        let mut decoder = FlacDecoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = [0i32; 32];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_aac_insufficient_data() {
        let mut decoder = AacDecoder::new();
        let encoded = [0u8; 5];
        let mut pcm_out = [0i32; 32];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
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
        let encoded_stereo = [
            0x10, 0x20, 0x30, 0x40,
            0x50, 0x60, 0x70, 0x80,
        ];
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
            vorbis.pending_samples.extend_from_slice(&[10, -10, 20, -20]);
            vorbis.pending_channels = 2;
            vorbis.stream_buf.extend_from_slice(b"OggS00001111");
            vorbis.header_len = 8;
            let mut v_out = [0i32; 2];
            assert_eq!(vorbis.decode(&[], &mut v_out), Ok((0, 1)));
            assert_eq!(v_out, [10 << 16, (-10) << 16]);
            vorbis.flush();
            assert!(vorbis.pending_samples.is_empty());
            assert_eq!(vorbis.stream_buf.len(), 8);
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
        let encoded_stereo = [
            0x40, 0x30, 0x20, 0x10,
            0x80, 0x70, 0x60, 0x50,
        ];
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
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
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
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
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
        header[28..38].copy_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
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
        let id3_header = [b'I', b'D', b'3', 3, 0, 0, 0, 0, 0, 20]; // 10 header + 20 payload = 30 total
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

        // 1. Two matching valid MP3 headers (MPEG-1 Layer III 128 kbps 44.1 kHz = 417 bytes)
        // but with all-zero/garbage payload that cannot be decoded as valid audio or has no reservoir.
        let mut corrupted_frame = [0u8; 417 + 4];
        corrupted_frame[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        corrupted_frame[4..417].fill(0xFF);
        corrupted_frame[417..417 + 4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);

        // Decoder must NOT return NeedMoreData; it must return Ok((consumed, 0)) to advance past bad frame!
        let res = dec.decode(&corrupted_frame, &mut pcm_out);
        assert!(res.is_ok(), "Expected Ok but got {:?}", res);
        let (consumed, frames) = res.unwrap();
        assert!(consumed >= 417, "Expected at least 417 bytes consumed, got {}", consumed);
        assert_eq!(frames, 0);

        // 2. Unmatched second header: first header matches sync, but byte at 417 does not match
        let mut unmatched = [0u8; 417 + 4];
        unmatched[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        unmatched[417] = 0x00; // Not a matching header
        let (consumed_unmatched, frames_unmatched) = dec.decode(&unmatched, &mut pcm_out).unwrap();
        assert_eq!(consumed_unmatched, 1);
        assert_eq!(frames_unmatched, 0);

        // 3. Invalid bitrate nibble (0xF) is rejected by Header::read, skipping non-sync bytes
        let mut invalid_bitrate = [0u8; 16];
        invalid_bitrate[0..4].copy_from_slice(&[0xFF, 0xFB, 0xF0, 0x00]);
        let (consumed_inv, frames_inv) = dec.decode(&invalid_bitrate, &mut pcm_out).unwrap();
        assert_eq!(consumed_inv, 13);
        assert_eq!(frames_inv, 0);
    }

    #[test]
    fn test_flac_cross_validation_all_fixtures() {
        let fixtures: &[(&str, &[u8])] = &[
            ("mono_16bit.flac", include_bytes!("../tests/fixtures/mono_16bit.flac")),
            ("stereo_16bit.flac", include_bytes!("../tests/fixtures/stereo_16bit.flac")),
            ("stereo_24bit.flac", include_bytes!("../tests/fixtures/stereo_24bit.flac")),
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

            // 2. Decode with native FlacDecoder in chunks of 512 bytes with 1024-sample pcm_out
            let mut decoder = FlacDecoder::new();
            let mut native_samples = std::vec::Vec::new();
            let mut pcm_buf = [0i32; 1024];
            let mut offset = 0;

            while offset < data.len() || decoder.has_pending_pcm() || decoder.stream_buf.len() > decoder.header_len {
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

        while offset < data.len() || decoder.has_pending_pcm() || decoder.stream_buf.len() > decoder.header_len {
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
        assert!(!decoder.has_pending_pcm(), "Decoder must NOT report pending PCM on partial chunk");

        // Feed next 1036 bytes (total 1536 bytes, matching Xtensa READ_SCRATCH_BYTES chunk)
        let res2 = decoder.decode(&data[500..1536], &mut pcm_buf);
        assert_eq!(res2, Ok((1036, 0)));
        assert!(!decoder.has_pending_pcm(), "Decoder must NOT report pending PCM on partial chunk");
    }
}
