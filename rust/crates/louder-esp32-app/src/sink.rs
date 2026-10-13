//! Audio Output Sink Implementations
//!
//! Provides sinks for audio frame output:
//! - `NullAudioSink`: Lightweight sink for headless tests and benchmarking.
//! - `SimulatedSink`: Sink storing written samples for verification.
//! - `EspI2sSink`: Hardware I2S DMA sink when running on ESP32 (`target_os = "espidf"`).

use squeezelite_audio_core::output::{AudioOutputSink, OutputError};

/// A sink that silently discards audio frames while counting throughput.
#[derive(Debug, Default)]
pub struct NullAudioSink {
    sample_rate: u32,
    muted: bool,
    frames_written: usize,
}

impl NullAudioSink {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            muted: false,
            frames_written: 0,
        }
    }

    pub fn frames_written(&self) -> usize {
        self.frames_written
    }
}

impl AudioOutputSink for NullAudioSink {
    type Error = OutputError;

    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let frame_count = frames.len() / 2;
        self.frames_written += frame_count;
        Ok(frame_count)
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Self::Error> {
        self.sample_rate = rate;
        Ok(())
    }

    fn set_mute(&mut self, muted: bool) {
        self.muted = muted;
    }
}

/// A simulated audio sink for host-side verification, recording written frames.
#[derive(Debug, Default)]
pub struct SimulatedSink {
    sample_rate: u32,
    muted: bool,
    frames_written: usize,
    buffer: heapless::Vec<i32, 1024>,
}

impl SimulatedSink {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            muted: false,
            frames_written: 0,
            buffer: heapless::Vec::new(),
        }
    }

    pub fn recorded_samples(&self) -> &[i32] {
        &self.buffer
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.frames_written = 0;
    }
}

impl AudioOutputSink for SimulatedSink {
    type Error = OutputError;

    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let frame_count = frames.len() / 2;
        for &s in frames {
            if self.buffer.is_full() {
                break;
            }
            let _ = self.buffer.push(s);
        }
        self.frames_written += frame_count;
        Ok(frame_count)
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Self::Error> {
        self.sample_rate = rate;
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
    fn test_null_sink_accounting() {
        let mut sink = NullAudioSink::new(44100);
        let samples = [100i32, -100, 200, -200];
        let written = sink.write_frames(&samples).unwrap();
        assert_eq!(written, 2);
        assert_eq!(sink.frames_written(), 2);

        sink.set_mute(true);
        assert!(sink.muted);
    }

    #[test]
    fn test_simulated_sink_recording() {
        let mut sink = SimulatedSink::new(48000);
        let samples = [10i32, -10, 20, -20];
        let written = sink.write_frames(&samples).unwrap();
        assert_eq!(written, 2);
        assert_eq!(sink.recorded_samples(), &[10, -10, 20, -20]);

        sink.set_mute(true);
        assert!(sink.muted);
    }
}
