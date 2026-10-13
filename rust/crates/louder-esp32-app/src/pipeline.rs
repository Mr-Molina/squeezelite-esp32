//! Real-Time Streaming Audio Pipeline Engine
//!
//! Connects LMS stream buffers, pure-Rust decoders, DSP equalizer,
//! volume scaling, and the I2S audio output sink into an integrated processing loop.

use squeezelite_audio_core::{
    scale_samples_inplace, AudioOutputSink, AudioRingBuffer, Equalizer, RingBufferError,
    VolumeScale,
};
use squeezelite_decoders::{AudioDecoder, DecoderError, PcmDecoder};

pub enum ActiveDecoder {
    Pcm(PcmDecoder),
    Mp3(Box<squeezelite_decoders::Mp3Decoder>),
}

impl AudioDecoder for ActiveDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        match self {
            Self::Pcm(d) => d.decode(encoded_in, pcm_out),
            Self::Mp3(d) => d.decode(encoded_in, pcm_out),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Pcm(d) => d.flush(),
            Self::Mp3(d) => d.flush(),
        }
    }
}

/// The integrated audio pipeline engine.
pub struct StreamPipeline<'a, S: AudioOutputSink> {
    pub stream_buffer: AudioRingBuffer<'a>,
    pub output_buffer: AudioRingBuffer<'a>,
    pub decoder: ActiveDecoder,
    pub equalizer: Equalizer<10>,
    pub volume: VolumeScale,
    pub sink: S,
    pub sample_rate: u32,
    pub frames_played: u64,
}

impl<'a, S: AudioOutputSink> StreamPipeline<'a, S> {
    pub fn new(
        stream_storage: &'a mut [u8],
        output_storage: &'a mut [u8],
        decoder: ActiveDecoder,
        sink: S,
        sample_rate: u32,
    ) -> Self {
        Self {
            stream_buffer: AudioRingBuffer::new(stream_storage),
            output_buffer: AudioRingBuffer::new(output_storage),
            decoder,
            equalizer: Equalizer::new(),
            volume: VolumeScale::unity(),
            sink,
            sample_rate,
            frames_played: 0,
        }
    }

    /// Pushes incoming encoded stream bytes from LMS into the stream ringbuffer.
    pub fn ingest_stream_data(&mut self, data: &[u8]) -> Result<usize, RingBufferError> {
        self.stream_buffer.write(data)
    }

    /// Decodes a chunk from `stream_buffer`, processes volume and EQ, and stores in `output_buffer`.
    pub fn process_decode_step(&mut self) -> Result<usize, DecoderError> {
        let mut encoded_scratch = [0u8; 1024];
        let bytes_available = self.stream_buffer.available_read();
        if bytes_available == 0 {
            return Err(DecoderError::NeedMoreData);
        }

        let to_read = core::cmp::min(encoded_scratch.len(), bytes_available);
        let read_bytes = self.stream_buffer.read(&mut encoded_scratch[..to_read])
            .map_err(|_| DecoderError::NeedMoreData)?;

        let mut pcm_scratch = [0i32; 512]; // 256 stereo frames
        let (consumed, frames_produced) = self.decoder.decode(
            &encoded_scratch[..read_bytes],
            &mut pcm_scratch,
        )?;

        if frames_produced > 0 {
            let active_pcm = &mut pcm_scratch[..frames_produced * 2];
            // Apply volume scaling
            scale_samples_inplace(active_pcm, self.volume);
            // Apply equalizer
            self.equalizer.process_interleaved(active_pcm);

            // Convert PCM samples to bytes for output ringbuffer storage
            // 4 bytes per 32-bit sample
            let pcm_bytes = bytemuck_cast_slice(active_pcm);
            let _ = self.output_buffer.write(pcm_bytes);
        }

        Ok(consumed)
    }

    /// Flushes output ringbuffer frames to the audio output sink (I2S DMA).
    pub fn process_output_step(&mut self) -> Result<usize, S::Error> {
        let mut byte_scratch = [0u8; 512]; // 64 stereo frames
        let bytes_avail = self.output_buffer.available_read();
        if bytes_avail < 8 {
            return Ok(0); // Minimum 1 stereo frame (8 bytes)
        }

        let to_read = core::cmp::min(byte_scratch.len(), bytes_avail);
        let aligned_to_frame = (to_read / 8) * 8;

        if let Ok(bytes_read) = self.output_buffer.read(&mut byte_scratch[..aligned_to_frame]) {
            let frame_count = bytes_read / 8;
            let mut sample_scratch = [0i32; 64 * 2];

            for (i, sample) in sample_scratch.iter_mut().take(frame_count * 2).enumerate() {
                let off = i * 4;
                *sample = i32::from_ne_bytes([
                    byte_scratch[off],
                    byte_scratch[off + 1],
                    byte_scratch[off + 2],
                    byte_scratch[off + 3],
                ]);
            }

            let written = self.sink.write_frames(&sample_scratch[..frame_count * 2])?;
            self.frames_played += written as u64;
            Ok(written)
        } else {
            Ok(0)
        }
    }

    /// Sets digital volume level.
    pub fn set_volume(&mut self, left_gain: u32, right_gain: u32) {
        self.volume.gain_left = left_gain;
        self.volume.gain_right = right_gain;
    }

    /// Flushes both stream and output ringbuffers and resets the decoder state.
    pub fn flush(&mut self) {
        self.stream_buffer.flush();
        self.output_buffer.flush();
        self.decoder.flush();
        self.equalizer.reset_history();
    }
}

fn bytemuck_cast_slice(samples: &[i32]) -> &[u8] {
    // SAFETY: Transmuting &[i32] to &[u8] is memory layout safe
    unsafe {
        core::slice::from_raw_parts(
            samples.as_ptr() as *const u8,
            core::mem::size_of_val(samples),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use squeezelite_audio_core::MockAudioSink;

    #[test]
    fn test_pipeline_pcm_playback_flow() {
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_decoder = PcmDecoder::new(16, 2, true); // 16-bit BE Stereo
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        // Feed 2 stereo frames of 16-bit PCM: (1000, -1000), (2000, -2000)
        let f1_l = (1000i16).to_be_bytes();
        let f1_r = (-1000i16).to_be_bytes();
        let f2_l = (2000i16).to_be_bytes();
        let f2_r = (-2000i16).to_be_bytes();

        let raw_stream = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        let ingested = pipeline.ingest_stream_data(&raw_stream).unwrap();
        assert_eq!(ingested, 8);

        // Decode step
        let consumed = pipeline.process_decode_step().unwrap();
        assert_eq!(consumed, 8);

        // Output step
        let frames_out = pipeline.process_output_step().unwrap();
        assert_eq!(frames_out, 2);
        assert_eq!(pipeline.frames_played, 2);

        // Check sink received samples
        let recorded = pipeline.sink.recorded_samples();
        assert_eq!(recorded[0], 1000 << 16);
        assert_eq!(recorded[1], -1000 << 16);
        assert_eq!(recorded[2], 2000 << 16);
        assert_eq!(recorded[3], -2000 << 16);
    }
}
