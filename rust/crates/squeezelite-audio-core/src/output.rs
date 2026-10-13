//! Audio Output Sink Abstraction
//!
//! Provides a hardware-independent trait for writing audio frames to I2S DMA,
//! DACs, or host mock testing sinks.

use core::fmt;

/// Errors returned by audio output devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputError {
    DeviceNotReady,
    UnsupportedSampleRate(u32),
    BufferUnderrun,
    HardwareFault,
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceNotReady => write!(f, "Audio output device is not initialized"),
            Self::UnsupportedSampleRate(r) => write!(f, "Unsupported sample rate: {} Hz", r),
            Self::BufferUnderrun => write!(f, "Audio DMA buffer underrun"),
            Self::HardwareFault => write!(f, "Hardware fault on audio DAC or amplifier"),
        }
    }
}

/// Abstract audio output sink (e.g. ESP32 I2S DMA peripheral or Mock sink).
pub trait AudioOutputSink {
    type Error;

    /// Writes interleaved 32-bit stereo audio frames to the output device.
    /// Returns the number of frames written.
    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error>;

    /// Reconfigures the device clock tree for a new sample rate (e.g. 44100, 48000, 96000).
    fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), Self::Error>;

    /// Mutes or unmutes the audio output.
    fn set_mute(&mut self, muted: bool);
}

/// In-memory mock audio sink for host-side integration tests.
pub struct MockAudioSink<const CAPACITY: usize> {
    pub sample_rate: u32,
    pub muted: bool,
    pub frames_written: usize,
    pub buffer: [i32; CAPACITY],
}

impl<const CAPACITY: usize> Default for MockAudioSink<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> MockAudioSink<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            sample_rate: 44100,
            muted: false,
            frames_written: 0,
            buffer: [0i32; CAPACITY],
        }
    }

    pub fn recorded_samples(&self) -> &[i32] {
        let count = core::cmp::min(self.frames_written * 2, CAPACITY);
        &self.buffer[..count]
    }
}

impl<const CAPACITY: usize> AudioOutputSink for MockAudioSink<CAPACITY> {
    type Error = OutputError;

    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let samples_to_copy = core::cmp::min(frames.len(), CAPACITY - (self.frames_written * 2));
        let frames_to_copy = samples_to_copy / 2;

        let start = self.frames_written * 2;
        self.buffer[start..start + frames_to_copy * 2].copy_from_slice(&frames[..frames_to_copy * 2]);
        self.frames_written += frames_to_copy;

        Ok(frames_to_copy)
    }

    fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), Self::Error> {
        self.sample_rate = rate_hz;
        Ok(())
    }

    fn set_mute(&mut self, muted: bool) {
        self.muted = muted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_audio_sink_write() {
        let mut sink = MockAudioSink::<16>::new();
        assert_eq!(sink.sample_rate, 44100);

        let stereo_frames = [100, -100, 200, -200];
        let written = sink.write_frames(&stereo_frames).unwrap();

        assert_eq!(written, 2);
        assert_eq!(sink.frames_written, 2);
        assert_eq!(sink.recorded_samples(), &[100, -100, 200, -200]);

        sink.set_sample_rate(96000).unwrap();
        assert_eq!(sink.sample_rate, 96000);
    }
}
