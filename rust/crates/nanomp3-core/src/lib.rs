//! The MPEG audio frame decoder behind [nanomp3](https://docs.rs/nanomp3): a
//! safe Rust port of [minimp3](https://github.com/lieff/minimp3) whose output
//! is bit-identical to upstream's.
//!
//! It's `no_std`, allocates nothing and does no buffering: [`Decoder::decode`]
//! finds and decodes one frame at a time from the bytes you give it. Because a
//! frame only counts once the frames after it are seen to be consistent, keep
//! several frames of input available (16 KiB is plenty) and refill as it
//! drains.
//!
//! ```
//! # let data: &[u8] = &[];
//! use nanomp3_core::{DecodeError, Decoder, MAX_SAMPLES_PER_FRAME};
//!
//! let mut decoder = Decoder::new();
//! let mut pcm = [0f32; MAX_SAMPLES_PER_FRAME]; // or i16, u8, i32, ...
//! let mut mp3 = data;
//! while !mp3.is_empty() {
//!     let (consumed, result) = decoder.decode(mp3, &mut pcm);
//!     mp3 = &mp3[consumed..];
//!     match result {
//!         Ok(info) => {
//!             let n = info.samples_produced * info.channels.num() as usize;
//!             let samples = &pcm[..n];
//!             // ... interleaved samples at info.sample_rate
//!         }
//!         // Normal while starting up or after a seek; later frames decode.
//!         Err(DecodeError::ReservoirUnavailable(_)) => {}
//!         Err(e) => eprintln!("skipped: {e}"),
//!     }
//! }
//! ```
//!
//! Most users want the `nanomp3` crate, which re-exports this one and adds a
//! seekable reader with its own buffering, gapless playback, and tag handling.
//!
//! # Features
//!
//! - `wide` (default): explicit SIMD via the `wide` crate. Without it, the
//!   same kernels run on plain arrays and rely on LLVM's auto-vectorizer.
//! - `layer12` (default): decode MPEG Layer I and II frames too. Without it
//!   they are reported as [`DecodeError::UnsupportedLayer`], like minimp3
//!   built with `MINIMP3_ONLY_MP3`.

#![no_std]
#![deny(unsafe_code)]

mod decoder;
mod minimp3;

pub use decoder::{
    Channels, DecodeError, Decoder, FrameInfo, MAX_SAMPLES_PER_FRAME, __private,
};
pub use minimp3::Sample;

#[cfg(test)]
mod tests;
