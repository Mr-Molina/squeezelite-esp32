//! `squeezelite-audio-core`
//!
//! High-performance, `no_std` audio processing engine:
//! - Lock-free Single-Producer Single-Consumer (SPSC) audio ringbuffers
//! - 10-band IIR biquad parametric equalizer
//! - Horner's polynomial loudness curve compensation and digital volume scaler

#![no_std]
#![allow(unknown_lints)]

pub mod equalizer;
pub mod fade;
pub mod output;
pub mod resampler;
pub mod ringbuffer;
pub mod volume;

pub use equalizer::{
    design_bands, design_peaking, loudness_gains_db, preset_gains_db, BiquadCoeffs, EqPreset,
    Equalizer, EqualizerError, DEFAULT_EQ_Q, EQ_BANDS_HZ,
};
pub use fade::{FadeEngine, FadeMode};
pub use output::{AudioOutputSink, MockAudioSink, OutputError};
pub use resampler::{InterpolationMode, ResampleQuality, Resampler};
pub use ringbuffer::{AudioRingBuffer, RingBufferError};
pub use volume::{scale_samples_inplace, ChannelMode, GainStage, VolumeScale};
