//! CDN audio chunk streaming and Vorbis bitstream feeding.
//!
//! Decrypts AES-128-CTR audio chunks using Spotify audio keys and feeds
//! Ogg Vorbis streams into `squeezelite_decoders::VorbisDecoder`.

extern crate alloc;

use crate::crypto::big_num_add;
use crate::crypto::Aes128Ctr;
use core::fmt;
use squeezelite_decoders::AudioDecoder;
use squeezelite_decoders::DecoderError;
use squeezelite_decoders::VorbisDecoder;

/// Spotify CDN fixed Audio AES IV matching `CDNAudioFile.h`.
pub const AUDIO_AES_IV: [u8; 16] = [
    0x72, 0xe0, 0x67, 0xfb, 0xdd, 0xcb, 0xcf, 0x77, 0xeb, 0xe8, 0xbc, 0x64,
    0x3f, 0x63, 0x0d, 0x93,
];

/// Size of Spotify proprietary container header preceding Ogg Vorbis bitstream.
pub const SPOTIFY_HEADER_SIZE: usize = 167;

/// Error during CDN chunk decryption or audio decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CdnError {
    /// Audio key length must be 16 bytes.
    InvalidKeyLength,
    /// Decoder encountered corrupted bitstream.
    Decoder(DecoderError),
}

impl From<DecoderError> for CdnError {
    fn from(err: DecoderError) -> Self {
        Self::Decoder(err)
    }
}

impl fmt::Display for CdnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKeyLength => write!(f, "invalid AES-128 audio key length"),
            Self::Decoder(e) => write!(f, "vorbis decoder error: {e:?}"),
        }
    }
}

/// AES-128-CTR audio stream decryptor for Spotify CDN chunks
/// (`AudioKeyDecryptor`).
///
/// Uses Spotify's standard 16-byte base IV (`AUDIO_AES_IV`), increments the
/// 128-bit big-endian counter per 16-byte AES block (`byte_offset / 16`), and
/// strips the 167-byte (`0xA7`) Spotify Ogg header when starting/seeking at
/// byte offset `0`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioKeyDecryptor {
    audio_key: [u8; 16],
    byte_offset: usize,
}

impl AudioKeyDecryptor {
    /// Standard Spotify 16-byte AES-128-CTR base IV.
    pub const BASE_IV: [u8; 16] = AUDIO_AES_IV;

    /// Spotify 167-byte (`0xA7`) proprietary Ogg header size.
    pub const OGG_HEADER_OFFSET: usize = SPOTIFY_HEADER_SIZE;

    /// Creates a new `AudioKeyDecryptor` with a 16-byte AES-128 audio key.
    #[must_use]
    pub const fn new(audio_key: [u8; 16]) -> Self {
        Self {
            audio_key,
            byte_offset: 0,
        }
    }

    /// Creates an `AudioKeyDecryptor` from a key slice (must be 16 bytes).
    pub fn from_slice(key_slice: &[u8]) -> Result<Self, CdnError> {
        let key: [u8; 16] = key_slice
            .try_into()
            .map_err(|_| CdnError::InvalidKeyLength)?;
        Ok(Self::new(key))
    }

    /// Returns the 16-byte AES audio key.
    #[must_use]
    pub const fn audio_key(&self) -> &[u8; 16] {
        &self.audio_key
    }

    /// Returns the current stream byte offset.
    #[must_use]
    pub const fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    /// Seeks to an absolute file byte offset (`0` includes the `0xA7` header).
    pub fn seek(&mut self, byte_offset: usize) {
        self.byte_offset = byte_offset;
    }

    /// Seeks to a logical Ogg bitstream offset (automatically adds `0xA7`).
    pub fn seek_ogg(&mut self, ogg_byte_offset: usize) {
        self.byte_offset = SPOTIFY_HEADER_SIZE.saturating_add(ogg_byte_offset);
    }

    /// Computes the 128-bit big-endian counter IV for `byte_offset`
    /// (`block = byte_offset / 16`).
    #[must_use]
    pub const fn counter_iv(byte_offset: usize) -> [u8; 16] {
        let block_index = (byte_offset / 16) as u128;
        let base = u128::from_be_bytes(AUDIO_AES_IV);
        base.wrapping_add(block_index).to_be_bytes()
    }

    /// Strips the 167-byte (`0xA7`) Spotify Ogg header prefix when
    /// `byte_offset < 0xA7`.
    #[must_use]
    pub fn strip_ogg_header(decrypted: &[u8], byte_offset: usize) -> &[u8] {
        if byte_offset < SPOTIFY_HEADER_SIZE {
            let skip =
                core::cmp::min(SPOTIFY_HEADER_SIZE - byte_offset, decrypted.len());
            &decrypted[skip..]
        } else {
            decrypted
        }
    }

    /// Decrypts `data` in-place at arbitrary `byte_offset` without stripping
    /// headers.
    pub fn decrypt_in_place(&self, data: &mut [u8], byte_offset: usize) {
        if data.is_empty() {
            return;
        }

        let intra_block = byte_offset % 16;
        let iv = Self::counter_iv(byte_offset);
        let mut ctr = Aes128Ctr::new(&self.audio_key, iv);

        if intra_block > 0 {
            let mut scratch = [0u8; 16];
            let take = core::cmp::min(16 - intra_block, data.len());
            scratch[intra_block..intra_block + take]
                .copy_from_slice(&data[..take]);
            ctr.process(&mut scratch);
            data[..take]
                .copy_from_slice(&scratch[intra_block..intra_block + take]);

            if data.len() > take {
                ctr.process(&mut data[take..]);
            }
        } else {
            ctr.process(data);
        }
    }

    /// Decrypts `chunk` in-place at `byte_offset` and returns the Ogg Vorbis
    /// slice with the 167-byte (`0xA7`) header stripped if `byte_offset < 0xA7`.
    pub fn decrypt_chunk<'a>(
        &self,
        chunk: &'a mut [u8],
        byte_offset: usize,
    ) -> &'a [u8] {
        self.decrypt_in_place(chunk, byte_offset);
        Self::strip_ogg_header(chunk, byte_offset)
    }

    /// Decrypts `chunk` at the current stateful `byte_offset`, advances the
    /// stream cursor, and returns the Ogg Vorbis payload with `0xA7` stripped.
    pub fn decrypt_next<'a>(&mut self, chunk: &'a mut [u8]) -> &'a [u8] {
        let current = self.byte_offset;
        self.byte_offset = self.byte_offset.saturating_add(chunk.len());
        self.decrypt_chunk(chunk, current)
    }
}

/// AES-128-CTR audio decryptor for Spotify CDN chunks.
#[derive(Clone, Debug)]
pub struct CdnAudioDecryptor {
    audio_key: [u8; 16],
}

impl CdnAudioDecryptor {
    /// Creates a new decryptor with 16-byte Spotify AES audio key.
    pub fn new(audio_key: [u8; 16]) -> Self {
        Self { audio_key }
    }

    /// Calculates 16-byte IV for given byte position in audio file.
    pub fn calculate_iv(offset: usize) -> [u8; 16] {
        let block_idx = offset / 16;
        let iv_vec = big_num_add(&AUDIO_AES_IV, block_idx);
        let mut iv = [0u8; 16];
        if iv_vec.len() >= 16 {
            let start = iv_vec.len() - 16;
            iv.copy_from_slice(&iv_vec[start..]);
        } else {
            let pad = 16 - iv_vec.len();
            iv[pad..].copy_from_slice(&iv_vec);
        }
        iv
    }

    /// Decrypts a chunk in-place given absolute file byte offset.
    pub fn decrypt_chunk(&self, chunk: &mut [u8], offset: usize) {
        let decryptor = AudioKeyDecryptor::new(self.audio_key);
        decryptor.decrypt_in_place(chunk, offset);
    }
}

/// Streaming Ogg Vorbis feeder for Spotify Connect audio playback.
pub struct CdnStreamFeeder {
    decryptor: CdnAudioDecryptor,
    decoder: VorbisDecoder,
    total_bytes_fed: usize,
    header_stripped: bool,
}

impl CdnStreamFeeder {
    /// Creates a new feeder with 16-byte audio key.
    pub fn new(audio_key: [u8; 16]) -> Self {
        Self {
            decryptor: CdnAudioDecryptor::new(audio_key),
            decoder: VorbisDecoder::new(),
            total_bytes_fed: 0,
            header_stripped: false,
        }
    }

    /// Returns sample rate of underlying Vorbis decoder.
    pub fn sample_rate(&self) -> u32 {
        self.decoder.sample_rate
    }

    /// Returns channel count (typically 2 for stereo).
    pub fn channels(&self) -> u32 {
        self.decoder.channels
    }

    /// Flushes internal decoder buffers.
    pub fn flush(&mut self) {
        self.decoder.flush();
        self.total_bytes_fed = 0;
        self.header_stripped = false;
    }

    /// Feeds raw (encrypted) CDN chunk, decrypts it, and decodes PCM frames.
    pub fn feed_and_decode(
        &mut self,
        chunk: &[u8],
        offset: usize,
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), CdnError> {
        if offset == 0 {
            self.header_stripped = false;
        }
        let mut decrypted = chunk.to_vec();
        self.decryptor.decrypt_chunk(&mut decrypted, offset);

        let slice_to_feed = if !self.header_stripped && offset < SPOTIFY_HEADER_SIZE {
            let strip = (SPOTIFY_HEADER_SIZE - offset).min(decrypted.len());
            if offset + decrypted.len() >= SPOTIFY_HEADER_SIZE {
                self.header_stripped = true;
            }
            &decrypted[strip..]
        } else {
            &decrypted[..]
        };

        if slice_to_feed.is_empty() {
            return Ok((chunk.len(), 0));
        }

        self.total_bytes_fed += slice_to_feed.len();
        let (consumed, samples) = self.decoder.decode(slice_to_feed, pcm_out)?;
        Ok((consumed, samples))
    }

    /// Converts 32-bit internal pipeline samples to 16-bit interleaved PCM.
    pub fn convert_to_i16_pcm(samples_i32: &[i32], pcm_16: &mut [i16]) -> usize {
        let count = core::cmp::min(samples_i32.len(), pcm_16.len());
        for i in 0..count {
            pcm_16[i] = (samples_i32[i] >> 16) as i16;
        }
        count
    }
}
