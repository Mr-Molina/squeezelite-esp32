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
        if chunk.is_empty() {
            return;
        }

        let intra_block = offset % 16;
        let base_offset = offset - intra_block;
        let iv = Self::calculate_iv(base_offset);

        let mut ctr = Aes128Ctr::new(&self.audio_key, iv);

        if intra_block > 0 {
            let mut dummy = [0u8; 16];
            let take = core::cmp::min(16 - intra_block, chunk.len());
            ctr.process(&mut dummy[..intra_block]);
            dummy[intra_block..intra_block + take].copy_from_slice(&chunk[..take]);
            ctr.process(&mut dummy[intra_block..intra_block + take]);
            chunk[..take].copy_from_slice(&dummy[intra_block..intra_block + take]);

            if chunk.len() > take {
                ctr.process(&mut chunk[take..]);
            }
        } else {
            ctr.process(chunk);
        }
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
        let mut decrypted = chunk.to_vec();
        self.decryptor.decrypt_chunk(&mut decrypted, offset);

        let slice_to_feed = if !self.header_stripped && offset == 0 {
            if decrypted.len() > SPOTIFY_HEADER_SIZE {
                self.header_stripped = true;
                &decrypted[SPOTIFY_HEADER_SIZE..]
            } else {
                return Ok((chunk.len(), 0));
            }
        } else {
            &decrypted[..]
        };

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
