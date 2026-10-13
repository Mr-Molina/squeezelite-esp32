//! `squeezelite-decoders`
//!
//! Pure-Rust audio decoding pipeline for squeezelite-esp32:
//! - Defines unified `AudioDecoder` trait
//! - High-speed zero-copy `PcmDecoder`
//! - Extensible frame routing and sample normalization

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

/// Unified Audio Decoder Error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderError {
    NeedMoreData,
    InvalidHeader,
    CorruptedFrame,
    UnsupportedSampleFormat,
    BufferOverflow,
}

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

/// PCM Passthrough Decoder (16-bit / 24-bit / 32-bit, Big or Little Endian).
pub struct PcmDecoder {
    pub bits_per_sample: u8,
    pub channels: u8,
    pub big_endian: bool,
}

impl PcmDecoder {
    pub const fn new(bits_per_sample: u8, channels: u8, big_endian: bool) -> Self {
        Self {
            bits_per_sample,
            channels,
            big_endian,
        }
    }
}

impl AudioDecoder for PcmDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let bytes_per_sample = match self.bits_per_sample {
            16 => 2,
            24 => 3,
            32 => 4,
            _ => return Err(DecoderError::UnsupportedSampleFormat),
        };

        let bytes_per_frame = bytes_per_sample * self.channels as usize;
        if bytes_per_frame == 0 {
            return Err(DecoderError::UnsupportedSampleFormat);
        }

        let max_frames = pcm_out.len() / self.channels as usize;
        let available_frames = encoded_in.len() / bytes_per_frame;
        let frames_to_decode = core::cmp::min(max_frames, available_frames);

        if frames_to_decode == 0 {
            return Err(DecoderError::NeedMoreData);
        }

        match (self.bits_per_sample, self.big_endian, self.channels) {
            // 16-bit Stereo Big-Endian (Common Squeezebox PCM format)
            (16, true, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let l = i16::from_be_bytes([encoded_in[off], encoded_in[off + 1]]) as i32;
                    let r = i16::from_be_bytes([encoded_in[off + 2], encoded_in[off + 3]]) as i32;
                    pcm_out[i * 2] = l << 16;
                    pcm_out[i * 2 + 1] = r << 16;
                }
            }
            // 16-bit Stereo Little-Endian
            (16, false, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 4;
                    let l = i16::from_le_bytes([encoded_in[off], encoded_in[off + 1]]) as i32;
                    let r = i16::from_le_bytes([encoded_in[off + 2], encoded_in[off + 3]]) as i32;
                    pcm_out[i * 2] = l << 16;
                    pcm_out[i * 2 + 1] = r << 16;
                }
            }
            // 24-bit Stereo Big-Endian
            (24, true, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 6;
                    let l = ((encoded_in[off] as i32) << 24)
                        | ((encoded_in[off + 1] as i32) << 16)
                        | ((encoded_in[off + 2] as i32) << 8);
                    let r = ((encoded_in[off + 3] as i32) << 24)
                        | ((encoded_in[off + 4] as i32) << 16)
                        | ((encoded_in[off + 5] as i32) << 8);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
                }
            }
            // 32-bit Stereo Big-Endian
            (32, true, 2) => {
                for i in 0..frames_to_decode {
                    let off = i * 8;
                    let l = i32::from_be_bytes([
                        encoded_in[off],
                        encoded_in[off + 1],
                        encoded_in[off + 2],
                        encoded_in[off + 3],
                    ]);
                    let r = i32::from_be_bytes([
                        encoded_in[off + 4],
                        encoded_in[off + 5],
                        encoded_in[off + 6],
                        encoded_in[off + 7],
                    ]);
                    pcm_out[i * 2] = l;
                    pcm_out[i * 2 + 1] = r;
                }
            }
            _ => return Err(DecoderError::UnsupportedSampleFormat),
        }

        Ok((frames_to_decode * bytes_per_frame, frames_to_decode))
    }

    fn flush(&mut self) {}
}

/// Pure-Rust MP3 Decoder using nanomp3-core (100% safe, no_std).
pub struct Mp3Decoder {
    decoder: nanomp3_core::Decoder,
}

impl Default for Mp3Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Mp3Decoder {
    pub fn new() -> Self {
        Self {
            decoder: nanomp3_core::Decoder::new(),
        }
    }
}

impl AudioDecoder for Mp3Decoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        let mut sample_buf = [0i32; nanomp3_core::MAX_SAMPLES_PER_FRAME];
        let (consumed, result) = self.decoder.decode(encoded_in, &mut sample_buf);

        match result {
            Ok(info) => {
                let channels = info.channels.num() as usize;
                let frames_produced = info.samples_produced;
                let max_frames = pcm_out.len() / 2;
                let frames_to_copy = core::cmp::min(frames_produced, max_frames);

                if channels == 2 {
                    pcm_out[..frames_to_copy * 2].copy_from_slice(&sample_buf[..frames_to_copy * 2]);
                } else if channels == 1 {
                    for i in 0..frames_to_copy {
                        let s = sample_buf[i];
                        pcm_out[i * 2] = s;
                        pcm_out[i * 2 + 1] = s;
                    }
                }
                Ok((consumed, frames_to_copy))
            }
            Err(nanomp3_core::DecodeError::NoFrame) => Err(DecoderError::NeedMoreData),
            Err(nanomp3_core::DecodeError::ReservoirUnavailable(_)) => Ok((consumed, 0)),
            Err(_) => Err(DecoderError::CorruptedFrame),
        }
    }

    fn flush(&mut self) {
        self.decoder = nanomp3_core::Decoder::new();
    }
}

#[cfg(feature = "std")]
/// Pure-Rust FLAC Decoder using claxon.
pub struct FlacDecoder {
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
}

#[cfg(feature = "std")]
impl Default for FlacDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl FlacDecoder {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100,
            bits_per_sample: 16,
            channels: 2,
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
        if encoded_in.len() < 4 {
            return Err(DecoderError::NeedMoreData);
        }

        let cursor = std::io::Cursor::new(encoded_in);
        match claxon::FlacReader::new(cursor) {
            Ok(mut reader) => {
                let info = reader.streaminfo();
                self.sample_rate = info.sample_rate;
                self.bits_per_sample = info.bits_per_sample;
                self.channels = info.channels;

                let mut frames_out = 0;
                let max_frames = pcm_out.len() / 2;
                let mut block_buf = std::vec::Vec::new();

                let shift = match self.bits_per_sample {
                    16 => 16,
                    24 => 8,
                    32 => 0,
                    _ => 16,
                };

                let mut blocks = reader.blocks();
                while let Ok(Some(block)) = blocks.read_next_or_eof(block_buf) {
                    let block_len = block.len() as usize;
                    let to_copy = core::cmp::min(block_len, max_frames.saturating_sub(frames_out));

                    if self.channels == 2 {
                        for i in 0..to_copy {
                            let l = block.sample(0, i as u32) << shift;
                            let r = block.sample(1, i as u32) << shift;
                            pcm_out[(frames_out + i) * 2] = l;
                            pcm_out[(frames_out + i) * 2 + 1] = r;
                        }
                    } else {
                        for i in 0..to_copy {
                            let s = block.sample(0, i as u32) << shift;
                            pcm_out[(frames_out + i) * 2] = s;
                            pcm_out[(frames_out + i) * 2 + 1] = s;
                        }
                    }

                    frames_out += to_copy;
                    block_buf = block.into_buffer();
                    if frames_out >= max_frames {
                        break;
                    }
                }

                let consumed = encoded_in.len();
                Ok((consumed, frames_out))
            }
            Err(claxon::Error::FormatError(_)) => Err(DecoderError::CorruptedFrame),
            Err(claxon::Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Err(DecoderError::NeedMoreData)
            }
            Err(_) => Err(DecoderError::NeedMoreData),
        }
    }

    fn flush(&mut self) {}
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
    fn test_pcm_insufficient_data() {
        let mut decoder = PcmDecoder::new(16, 2, true);
        let encoded = [0u8; 3]; // 3 bytes is less than 1 frame (4 bytes)
        let mut pcm_out = [0i32; 4];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
    }

    #[test]
    fn test_mp3_insufficient_data() {
        let mut decoder = Mp3Decoder::new();
        let encoded = [0u8; 4];
        let mut pcm_out = [0i32; 32];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_flac_insufficient_data() {
        let mut decoder = FlacDecoder::new();
        let encoded = [0u8; 3];
        let mut pcm_out = [0i32; 32];
        assert_eq!(decoder.decode(&encoded, &mut pcm_out), Err(DecoderError::NeedMoreData));
    }
}
