//! `squeezelite-audio-core`
//!
//! High-performance, `no_std` audio processing engine:
//! - Lock-free Single-Producer Single-Consumer (SPSC) audio ringbuffers
//! - 10-band IIR biquad parametric equalizer
//! - Horner's polynomial loudness curve compensation and digital volume scaler

#![no_std]
#![allow(unknown_lints)]

pub mod equalizer;
pub mod output;
pub mod ringbuffer;
pub mod volume;

pub use equalizer::{BiquadCoeffs, Equalizer};
pub use output::{AudioOutputSink, MockAudioSink, OutputError};
pub use ringbuffer::{AudioRingBuffer, RingBufferError};
pub use volume::{scale_samples_inplace, VolumeScale};
