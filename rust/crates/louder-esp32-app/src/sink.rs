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
        if !(8_000..=192_000).contains(&rate) {
            return Err(OutputError::UnsupportedSampleRate(rate));
        }
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

    pub fn frames_written(&self) -> usize {
        self.frames_written
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

    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
        let mut stored_frames = 0;
        for pair in frames.chunks_exact(2) {
            if self.buffer.capacity() - self.buffer.len() < 2 {
                break;
            }
            let l = if self.muted { 0 } else { pair[0] };
            let r = if self.muted { 0 } else { pair[1] };
            let _ = self.buffer.push(l);
            let _ = self.buffer.push(r);
            stored_frames += 1;
        }
        self.frames_written += stored_frames;
        Ok(stored_frames)
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Self::Error> {
        if !(8_000..=192_000).contains(&rate) {
            return Err(OutputError::UnsupportedSampleRate(rate));
        }
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
    fn test_null_sink_sample_rate_bounds() {
        let mut sink = NullAudioSink::new(44100);
        assert!(sink.set_sample_rate(8000).is_ok());
        assert!(sink.set_sample_rate(192000).is_ok());
        assert_eq!(
            sink.set_sample_rate(7999),
            Err(OutputError::UnsupportedSampleRate(7999))
        );
        assert_eq!(
            sink.set_sample_rate(192001),
            Err(OutputError::UnsupportedSampleRate(192001))
        );
    }

    #[test]
    fn test_simulated_sink_recording() {
        let mut sink = SimulatedSink::new(48000);
        let samples = [10i32, -10, 20, -20];
        let written = sink.write_frames(&samples).unwrap();
        assert_eq!(written, 2);
        assert_eq!(sink.frames_written(), 2);
        assert_eq!(sink.recorded_samples(), &[10, -10, 20, -20]);

        sink.set_mute(true);
        assert!(sink.muted);
    }

    #[test]
    fn test_simulated_sink_sample_rate_bounds() {
        let mut sink = SimulatedSink::new(48000);
        assert!(sink.set_sample_rate(8000).is_ok());
        assert!(sink.set_sample_rate(192000).is_ok());
        assert_eq!(
            sink.set_sample_rate(7999),
            Err(OutputError::UnsupportedSampleRate(7999))
        );
        assert_eq!(
            sink.set_sample_rate(256000),
            Err(OutputError::UnsupportedSampleRate(256000))
        );
    }

    #[test]
    fn test_simulated_sink_mute_zeroing_and_accounting() {
        let mut sink = SimulatedSink::new(44100);
        sink.set_mute(true);

        let samples = [100i32, -100, 200, -200];
        let written = sink.write_frames(&samples).unwrap();
        assert_eq!(written, 2);
        assert_eq!(sink.frames_written(), 2);
        assert_eq!(sink.recorded_samples(), &[0, 0, 0, 0]);

        sink.set_mute(false);
        let written2 = sink.write_frames(&samples).unwrap();
        assert_eq!(written2, 2);
        assert_eq!(sink.frames_written(), 4);
        assert_eq!(sink.recorded_samples(), &[0, 0, 0, 0, 100, -100, 200, -200]);
    }
}
