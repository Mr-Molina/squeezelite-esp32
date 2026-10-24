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

                if chunk_id == b"fmt " {
                    if offset + 8 + 16 > data.len() {
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

                    let padded_size = chunk_size + (chunk_size & 1);
                    offset += 8 + padded_size;
                } else if chunk_id == b"data" {
                    if !found_fmt {
                        return Err(DecoderError::InvalidHeader);
                    }
                    return Ok(offset + 8);
                } else {
                    let padded_size = chunk_size + (chunk_size & 1);
                    if offset + 8 + padded_size > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    offset += 8 + padded_size;
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

                if chunk_id == b"COMM" {
                    if offset + 8 + 18 > data.len() {
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

                    let padded_size = chunk_size + (chunk_size & 1);
                    offset += 8 + padded_size;
                } else if chunk_id == b"SSND" {
                    if !found_comm {
                        return Err(DecoderError::InvalidHeader);
                    }
                    if offset + 8 + 8 > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let ssnd_payload = &data[offset + 8..];
                    let ssnd_offset = u32::from_be_bytes([
                        ssnd_payload[0],
                        ssnd_payload[1],
                        ssnd_payload[2],
                        ssnd_payload[3],
                    ]) as usize;

                    let audio_start = offset + 8 + 8 + ssnd_offset;
                    if audio_start > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    return Ok(audio_start);
                } else {
                    let padded_size = chunk_size + (chunk_size & 1);
                    if offset + 8 + padded_size > data.len() {
                        return Err(DecoderError::NeedMoreData);
                    }
                    offset += 8 + padded_size;
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
            sample_rate: 44100,
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

        let slice_to_decode = match sync_offset {
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
                if frame_bytes > 0 {
                    if encoded_in.len() < frame_bytes + 4 {
                        return Err(DecoderError::NeedMoreData);
                    }
                    let next_h = nanomp3_core::__private::Header::read(&encoded_in[frame_bytes..]);
                    if !h.compare(next_h) {
                        return Ok((1, 0));
                    }
                    &encoded_in[..frame_bytes + 4]
                } else {
                    encoded_in
                }
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
            Err(nanomp3_core::DecodeError::NoFrame) => {
                if consumed > 0 {
                    Ok((consumed, 0))
                } else {
                    Err(DecoderError::NeedMoreData)
                }
            }
            Err(nanomp3_core::DecodeError::ReservoirUnavailable(_)) => {
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
        self.reset_in_place();
    }
}

#[cfg(feature = "std")]
struct FlacByteReader<'a> {
    data: &'a [u8],
    pos: &'a std::cell::Cell<usize>,
}

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
/// Pure-Rust FLAC Decoder using claxon with stateful streaming bitstream accumulation (DEC-002).
pub struct FlacDecoder {
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
    header_len: usize,
    stream_buf: std::vec::Vec<u8>,
}

#[cfg(feature = "std")]
impl Default for FlacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl FlacDecoder {
    /// Maps FLAC bit depth to 32-bit output shift (e.g. 8-bit shifts left by 24).
    pub const fn sample_shift(bits_per_sample: u32) -> u32 {
        match bits_per_sample {
            8 => 24,
            16 => 16,
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
            stream_buf: std::vec::Vec::new(),
        }
    }
}

#[cfg(feature = "std")]
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

        if self.stream_buf.len().saturating_add(encoded_in.len()) > MAX_STREAM_BUF {
            self.stream_buf.clear();
            self.header_len = 0;
            return Err(DecoderError::CorruptedFrame);
        }

        if self.stream_buf.len().saturating_add(encoded_in.len()) < 4 {
            return Err(DecoderError::NeedMoreData);
        }

        let input_taken = encoded_in.len();
        self.stream_buf.extend_from_slice(encoded_in);

        // Strip optional Ogg page wrapper prefix before b"fLaC" if present in stream_buf
        if self.header_len == 0 {
            if let Some(pos) = self.stream_buf.windows(4).position(|w| w == b"fLaC") {
                if pos > 0 {
                    self.stream_buf.drain(..pos);
                }
            }
        }

        let pos = std::cell::Cell::new(0usize);
        let reader_input = FlacByteReader {
            data: &self.stream_buf[..],
            pos: &pos,
        };

        match claxon::FlacReader::new(reader_input) {
            Ok(mut reader) => {
                let header_end = pos.get();
                if self.header_len == 0 {
                    self.header_len = header_end;
                }

                let info = reader.streaminfo();
                self.sample_rate = info.sample_rate;
                self.bits_per_sample = info.bits_per_sample;
                self.channels = info.channels;

                if self.channels != 1 && self.channels != 2 {
                    return Err(DecoderError::UnsupportedSampleFormat);
                }

                let shift = Self::sample_shift(self.bits_per_sample);

                let mut frames_out = 0;
                let mut block_buf = std::vec::Vec::new();
                let mut last_good_pos = header_end;
                let mut first_block_overflow = false;
                let mut corrupted_block = false;

                {
                    let mut blocks = reader.blocks();
                    loop {
                        if pos.get() >= self.stream_buf.len() {
                            break;
                        }
                        let pos_before_block = pos.get();
                        match blocks.read_next_or_eof(block_buf) {
                            Ok(Some(block)) => {
                                let packet_frames = block.len() as usize;
                                let available_space = max_frames.saturating_sub(frames_out);
                                if packet_frames > available_space {
                                    if frames_out == 0 {
                                        first_block_overflow = true;
                                    }
                                    break;
                                }

                                if self.channels == 2 {
                                    for i in 0..packet_frames {
                                        let l = block.sample(0, i as u32) << shift;
                                        let r = block.sample(1, i as u32) << shift;
                                        pcm_out[(frames_out + i) * 2] = l;
                                        pcm_out[(frames_out + i) * 2 + 1] = r;
                                    }
                                } else {
                                    for i in 0..packet_frames {
                                        let s = block.sample(0, i as u32) << shift;
                                        pcm_out[(frames_out + i) * 2] = s;
                                        pcm_out[(frames_out + i) * 2 + 1] = s;
                                    }
                                }

                                frames_out += packet_frames;
                                last_good_pos = pos.get();
                                block_buf = block.into_buffer();

                                if frames_out >= max_frames {
                                    break;
                                }
                            }
                            Ok(None) | Err(claxon::Error::IoError(_)) => {
                                break;
                            }
                            Err(claxon::Error::FormatError(_))
                            | Err(claxon::Error::Unsupported(_)) => {
                                last_good_pos = pos
                                    .get()
                                    .max(pos_before_block + 1)
                                    .min(self.stream_buf.len());
                                corrupted_block = true;
                                break;
                            }
                        }
                    }
                }

                if first_block_overflow {
                    self.stream_buf
                        .truncate(self.stream_buf.len().saturating_sub(input_taken));
                    return Err(DecoderError::BufferOverflow);
                }

                if last_good_pos > self.header_len {
                    self.stream_buf.drain(self.header_len..last_good_pos);
                }

                if corrupted_block && frames_out == 0 {
                    return Err(DecoderError::CorruptedFrame);
                }

                if frames_out == 0 && input_taken == 0 {
                    Err(DecoderError::NeedMoreData)
                } else {
                    Ok((input_taken, frames_out))
                }
            }
            Err(_) => {
                if input_taken > 0 {
                    Ok((input_taken, 0))
                } else {
                    Err(DecoderError::NeedMoreData)
                }
            }
        }
    }

    fn flush(&mut self) {
        if self.header_len > 0 && self.stream_buf.len() >= self.header_len {
            self.stream_buf.truncate(self.header_len);
        } else {
            self.stream_buf.clear();
            self.header_len = 0;
        }
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
            flac.flush();
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
}
