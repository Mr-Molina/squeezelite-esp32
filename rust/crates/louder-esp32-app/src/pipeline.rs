//! Real-Time Streaming Audio Pipeline Engine
//!
//! Connects LMS stream buffers, pure-Rust decoders, DSP equalizer,
//! volume scaling, and the I2S audio output sink into an integrated processing loop.

#[cfg(target_arch = "xtensa")]
extern crate alloc;
#[cfg(target_arch = "xtensa")]
use alloc::boxed::Box;

use squeezelite_audio_core::{
    scale_samples_inplace, AudioOutputSink, AudioRingBuffer, Equalizer, RingBufferError,
    VolumeScale,
};
use squeezelite_decoders::{AudioDecoder, DecoderError, PcmDecoder};

pub use squeezelite_audio_core::EqPreset;
pub type ActiveCodec = ActiveDecoder;

#[cfg(not(target_arch = "xtensa"))]
pub const PCM_SCRATCH_SAMPLES: usize = 8192;
#[cfg(target_arch = "xtensa")]
pub const PCM_SCRATCH_SAMPLES: usize = 2304;

#[cfg(not(target_arch = "xtensa"))]
pub const READ_SCRATCH_BYTES: usize = 4096;
#[cfg(target_arch = "xtensa")]
pub const READ_SCRATCH_BYTES: usize = 1536;
pub const WRITE_SCRATCH_BYTES: usize = 512;
pub const OUTPUT_I32_SCRATCH_SAMPLES: usize = 128;

pub enum ActiveDecoder {
    Pcm(PcmDecoder),
    #[cfg(not(target_arch = "xtensa"))]
    Mp3(Box<squeezelite_decoders::Mp3Decoder>),
    #[cfg(target_arch = "xtensa")]
    Mp3(&'static mut squeezelite_decoders::StaticMp3Decoder),
    Flac(Box<squeezelite_decoders::FlacDecoder>),
    #[cfg(not(target_arch = "xtensa"))]
    Aac(Box<squeezelite_decoders::AacDecoder>),
    #[cfg(not(target_arch = "xtensa"))]
    Vorbis(Box<squeezelite_decoders::VorbisDecoder>),
}

impl AudioDecoder for ActiveDecoder {
    fn decode(
        &mut self,
        encoded_in: &[u8],
        pcm_out: &mut [i32],
    ) -> Result<(usize, usize), DecoderError> {
        match self {
            Self::Pcm(d) => d.decode(encoded_in, pcm_out),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Mp3(d) => d.decode(encoded_in, pcm_out),
            #[cfg(target_arch = "xtensa")]
            Self::Mp3(d) => d.decode(encoded_in, pcm_out),
            Self::Flac(d) => d.decode(encoded_in, pcm_out),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Aac(d) => d.decode(encoded_in, pcm_out),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Vorbis(d) => d.decode(encoded_in, pcm_out),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Pcm(d) => d.flush(),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Mp3(d) => d.flush(),
            #[cfg(target_arch = "xtensa")]
            Self::Mp3(d) => d.flush(),
            Self::Flac(d) => d.flush(),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Aac(d) => d.flush(),
            #[cfg(not(target_arch = "xtensa"))]
            Self::Vorbis(d) => d.flush(),
        }
    }
}

impl ActiveDecoder {
    /// Returns the active stream sample rate in Hz reported by the decoder, if known.
    pub fn sample_rate(&self) -> Option<u32> {
        match self {
            Self::Pcm(d) => d.sample_rate,
            #[cfg(not(target_arch = "xtensa"))]
            Self::Mp3(d) => {
                if d.sample_rate > 0 {
                    Some(d.sample_rate)
                } else {
                    None
                }
            }
            #[cfg(target_arch = "xtensa")]
            Self::Mp3(d) => {
                if d.sample_rate > 0 {
                    Some(d.sample_rate)
                } else {
                    None
                }
            }
            Self::Flac(d) => {
                if d.sample_rate > 0 {
                    Some(d.sample_rate)
                } else {
                    None
                }
            }
            #[cfg(not(target_arch = "xtensa"))]
            Self::Aac(d) => {
                if d.sample_rate > 0 {
                    Some(d.sample_rate)
                } else {
                    None
                }
            }
            #[cfg(not(target_arch = "xtensa"))]
            Self::Vorbis(d) => {
                if d.sample_rate > 0 {
                    Some(d.sample_rate)
                } else {
                    None
                }
            }
        }
    }

    /// Returns whether the active decoder has unconsumed decoded PCM frames waiting in its internal buffer.
    pub fn has_pending_pcm(&self) -> bool {
        match self {
            Self::Flac(d) => d.has_pending_pcm(),
            _ => false,
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
    pub pcm_pending_offset: usize,
    pub pcm_pending_samples: usize,
    pub read_scratch: [u8; READ_SCRATCH_BYTES],
    pub pcm_scratch: [i32; PCM_SCRATCH_SAMPLES],
    pub write_scratch: [u8; WRITE_SCRATCH_BYTES],
    pub output_i32_scratch: [i32; OUTPUT_I32_SCRATCH_SAMPLES],
    pub current_eq_preset: EqPreset,
    pub user_muted: bool,
}

impl<'a, S: AudioOutputSink> StreamPipeline<'a, S> {
    #[inline(always)]
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
            pcm_pending_offset: 0,
            pcm_pending_samples: 0,
            read_scratch: [0u8; READ_SCRATCH_BYTES],
            pcm_scratch: [0i32; PCM_SCRATCH_SAMPLES],
            write_scratch: [0u8; WRITE_SCRATCH_BYTES],
            output_i32_scratch: [0i32; OUTPUT_I32_SCRATCH_SAMPLES],
            current_eq_preset: EqPreset::Flat,
            user_muted: false,
        }
    }

    /// Sets user mute state on pipeline and output sink.
    pub fn set_mute(&mut self, mute: bool) {
        self.user_muted = mute;
        self.sink.set_mute(mute);
    }

    /// Pushes incoming encoded stream bytes from LMS into the stream ringbuffer.
    pub fn ingest_stream_data(&mut self, data: &[u8]) -> Result<usize, RingBufferError> {
        self.stream_buffer.write(data)
    }

    /// Decodes a chunk from `stream_buffer`, processes volume and EQ, and stores in `output_buffer`.
    ///
    /// # Returns (`PIPE-504`)
    /// - When draining buffered `pcm_pending_samples` from a prior decode step: returns
    ///   `Ok(frames_to_write)` (the number of pending stereo frames written to `output_buffer`,
    ///   or `Ok(0)` if `output_buffer` has fewer than 8 bytes of free space).
    /// - When `output_buffer` is full (`available_write() < 8`): returns `Ok(0)` due to backpressure.
    /// - When decoding new input from `stream_buffer`: returns `Ok(consumed)` (the number of
    ///   encoded stream bytes consumed by the active decoder).
    /// - On `DecoderError::CorruptedFrame` or `DecoderError::InvalidHeader`: consumes 1 byte from
    ///   `stream_buffer` to advance past the corrupted byte and prevent an infinite decode loop (`PIPE-501`).
    pub fn process_decode_step(&mut self) -> Result<usize, DecoderError> {
        // Drain pending PCM frames from previous decode if output buffer was previously constrained (LOGIC-601/602)
        if self.pcm_pending_samples > 0 {
            let avail_frames = self.output_buffer.available_write() / 8;
            if avail_frames == 0 {
                return Ok(0);
            }
            let pending_frames = self.pcm_pending_samples / 2;
            let frames_to_write = pending_frames.min(avail_frames);
            let samples_to_write = frames_to_write * 2;
            let slice = &self.pcm_scratch[self.pcm_pending_offset..self.pcm_pending_offset + samples_to_write];
            let pcm_bytes: &[u8] = bytemuck::cast_slice(slice);
            let written = self.output_buffer.write(pcm_bytes).unwrap_or(0);
            let frames_written = written / 8;
            let samples_written = frames_written * 2;
            self.pcm_pending_offset += samples_written;
            self.pcm_pending_samples -= samples_written;
            if self.pcm_pending_samples == 0 {
                self.pcm_pending_offset = 0;
            }
            return Ok(frames_written);
        }

        // Drain pending PCM frames from decoder if available (e.g. FlacDecoder pending frames)
        if self.decoder.has_pending_pcm() {
            let avail_frames = self.output_buffer.available_write() / 8;
            if avail_frames == 0 {
                return Ok(0);
            }
            let (consumed, frames_produced) = match self.decoder.decode(&[], &mut self.pcm_scratch) {
                Ok(res) => res,
                Err(DecoderError::NeedMoreData) => (0, 0),
                Err(DecoderError::BufferOverflow) => return Err(DecoderError::BufferOverflow),
                Err(e) => return Err(e),
            };

            if frames_produced > 0 {
                let active_pcm = &mut self.pcm_scratch[..frames_produced * 2];
                self.equalizer.process_interleaved(active_pcm);
                scale_samples_inplace(active_pcm, self.volume);

                let frames_to_write = frames_produced.min(avail_frames);
                let frames_written = if frames_to_write > 0 {
                    let pcm_bytes: &[u8] = bytemuck::cast_slice(&self.pcm_scratch[..frames_to_write * 2]);
                    let written = self.output_buffer.write(pcm_bytes).unwrap_or(0);
                    written / 8
                } else {
                    0
                };

                let samples_written = frames_written * 2;
                if samples_written < frames_produced * 2 {
                    self.pcm_pending_offset = samples_written;
                    self.pcm_pending_samples = frames_produced * 2 - samples_written;
                } else {
                    self.pcm_pending_offset = 0;
                    self.pcm_pending_samples = 0;
                }
                return Ok(frames_written);
            }

            return Ok(consumed);
        }

        let bytes_available = self.stream_buffer.available_read();
        if bytes_available == 0 {
            return Err(DecoderError::NeedMoreData);
        }

        // Guarantee output buffer has space for at least 1 stereo frame (8 bytes)
        if self.output_buffer.available_write() < 8 {
            return Ok(0); // Output buffer backpressure
        }

        let to_peek = self.read_scratch.len().min(bytes_available);
        let peeked = self.stream_buffer.peek(&mut self.read_scratch[..to_peek]);
        if peeked == 0 {
            return Err(DecoderError::NeedMoreData);
        }

        let (consumed, frames_produced) = match self.decoder.decode(
            &self.read_scratch[..peeked],
            &mut self.pcm_scratch,
        ) {
            Ok(res) => res,
            Err(DecoderError::NeedMoreData) => return Err(DecoderError::NeedMoreData),
            Err(DecoderError::BufferOverflow) => return Err(DecoderError::BufferOverflow),
            Err(e) => {
                // Consume at least 1 byte on corrupted frame / invalid header to prevent infinite busy loop
                self.stream_buffer.consume(1);
                return Err(e);
            }
        };

        if let Some(rate) = self.decoder.sample_rate() {
            if rate > 0 && rate != self.sample_rate {
                let _ = self.sink.set_sample_rate(rate);
                self.sample_rate = rate;
            }
        }

        // Only consume the exact bytes ingested by decoder (CONC-005)
        if consumed > 0 {
            self.stream_buffer.consume(consumed);
        }

        if frames_produced > 0 {
            // Apply DSP across ALL produced samples exactly once before buffering:
            // 1. Equalizer preset and loudness contour in-place
            // 2. Volume scaling and attenuation in-place
            let active_pcm = &mut self.pcm_scratch[..frames_produced * 2];
            self.equalizer.process_interleaved(active_pcm);
            scale_samples_inplace(active_pcm, self.volume);

            let avail_frames = self.output_buffer.available_write() / 8;
            let frames_to_write = frames_produced.min(avail_frames);

            let frames_written = if frames_to_write > 0 {
                let pcm_bytes: &[u8] = bytemuck::cast_slice(&self.pcm_scratch[..frames_to_write * 2]);
                debug_assert_eq!(pcm_bytes.len() % 8, 0);
                let written = self.output_buffer.write(pcm_bytes).unwrap_or(0);
                written / 8
            } else {
                0
            };

            let samples_written = frames_written * 2;
            if samples_written < frames_produced * 2 {
                self.pcm_pending_offset = samples_written;
                self.pcm_pending_samples = frames_produced * 2 - samples_written;
            } else {
                self.pcm_pending_offset = 0;
                self.pcm_pending_samples = 0;
            }
        }

        Ok(consumed)
    }

    /// Flushes output ringbuffer frames to the audio output sink (I2S DMA).
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn process_output_step(&mut self) -> Result<usize, S::Error> {
        let bytes_avail = self.output_buffer.available_read();
        if bytes_avail < 8 {
            return Ok(0); // Minimum 1 stereo frame (8 bytes)
        }

        let to_read = core::cmp::min(self.write_scratch.len(), bytes_avail);
        let bytes_to_read = (to_read / 8) * 8;
        if bytes_to_read == 0 {
            return Ok(0);
        }

        // Non-destructive peek: frames remain in ringbuffer until sink write succeeds (LOGIC-603)
        let bytes_read = self.output_buffer.peek(&mut self.write_scratch[..bytes_to_read]);
        if bytes_read < 8 {
            return Ok(0);
        }

        let frames_to_write = bytes_read / 8;
        let samples_count = frames_to_write * 2;

        for (chunk, sample) in self.write_scratch[..frames_to_write * 8]
            .chunks_exact(4)
            .zip(self.output_i32_scratch[..samples_count].iter_mut())
        {
            *sample = i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        if !self.user_muted {
            self.sink.set_mute(false);
        }
        let written_frames = self.sink.write_frames(&self.output_i32_scratch[..frames_to_write * 2])?;
        // ONLY consume the exact number of frames successfully committed to the sink
        self.output_buffer.consume(written_frames * 8);
        if written_frames > 0 {
            self.frames_played = self.frames_played.wrapping_add(written_frames as u64);
        }

        Ok(written_frames)
    }

    /// Sets digital volume level.
    pub fn set_volume(&mut self, left_gain: u32, right_gain: u32) {
        self.volume.gain_left = left_gain;
        self.volume.gain_right = right_gain;
    }

    /// Sets the active audio decoder.
    pub fn set_decoder(&mut self, decoder: ActiveDecoder) {
        self.decoder = decoder;
    }

    /// Applies a 10-band biquad graphic equalizer preset to the audio pipeline.
    pub fn apply_eq_preset(&mut self, preset: EqPreset) {
        self.current_eq_preset = preset;
        self.equalizer.apply_preset(preset);
    }

    /// Sets the 10-band equalizer preset (alias for `apply_eq_preset`).
    pub fn set_eq_preset(&mut self, preset: EqPreset) {
        self.apply_eq_preset(preset);
    }

    /// Returns the currently active 10-band equalizer preset.
    #[must_use]
    pub const fn eq_preset(&self) -> EqPreset {
        self.current_eq_preset
    }

    /// Enables or disables Horner's polynomial loudness contour compensation.
    pub fn set_loudness(&mut self, enabled: bool) {
        self.equalizer.set_loudness_contour(enabled);
    }

    /// Returns whether the loudness contour is currently enabled.
    #[must_use]
    pub const fn loudness_enabled(&self) -> bool {
        self.equalizer.loudness_enabled()
    }

    /// Dynamically selects and instantiates the audio decoder based on the LMS format character or byte.
    ///
    /// - `'m'` / `'M'`: MP3 (nanomp3-core)
    /// - `'f'` / `'F'`: FLAC (claxon)
    /// - `'a'` / `'A'`: AAC (syom)
    /// - `'o'` / `'O'`: Ogg Vorbis (lewton)
    /// - `'p'` / `'P'`: PCM passthrough
    pub fn select_format(&mut self, format_byte: u8) {
        match format_byte {
            b'm' | b'M' => {
                #[cfg(not(target_arch = "xtensa"))]
                self.set_decoder(ActiveDecoder::Mp3(Box::new(squeezelite_decoders::Mp3Decoder::new())));
            }
            b'f' | b'F' => {
                self.set_decoder(ActiveDecoder::Flac(Box::new(squeezelite_decoders::FlacDecoder::new())));
            }
            b'a' | b'A' => {
                #[cfg(not(target_arch = "xtensa"))]
                self.set_decoder(ActiveDecoder::Aac(Box::new(squeezelite_decoders::AacDecoder::new())));
            }
            b'o' | b'O' => {
                #[cfg(not(target_arch = "xtensa"))]
                self.set_decoder(ActiveDecoder::Vorbis(Box::new(squeezelite_decoders::VorbisDecoder::new())));
            }
            b'p' | b'P' => {
                self.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
            }
            _ => {
                self.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
            }
        }
    }

    /// Executes one step of the audio pipeline (alias for `process_decode_step`).
    #[inline]
    pub fn step_audio_pipeline(&mut self) -> Result<usize, DecoderError> {
        self.process_decode_step()
    }

    /// Sets sample rate on pipeline and output sink.
    pub fn set_sample_rate(&mut self, rate: u32) -> Result<(), S::Error> {
        self.sink.set_sample_rate(rate)?;
        self.sample_rate = rate;
        Ok(())
    }

    /// Returns the number of bytes available in the stream ringbuffer.
    pub fn stream_buffer_available(&self) -> usize {
        self.stream_buffer.available_read()
    }

    /// Returns the number of bytes available in the output ringbuffer.
    pub fn output_buffer_available(&self) -> usize {
        self.output_buffer.available_read()
    }

    /// Returns `true` if decoded PCM samples are still waiting to be drained into `output_buffer` (`COORD-504`).
    pub fn has_pending_pcm(&self) -> bool {
        self.pcm_pending_samples > 0 || self.decoder.has_pending_pcm()
    }

    /// Flushes both stream and output ringbuffers, clears pending PCM, and resets the decoder state.
    pub fn flush(&mut self) {
        self.stream_buffer.flush();
        self.output_buffer.flush();
        self.decoder.flush();
        self.equalizer.reset_history();
        self.sink.set_mute(true);
        self.pcm_pending_offset = 0;
        self.pcm_pending_samples = 0;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use squeezelite_audio_core::{MockAudioSink, OutputError};

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

    #[test]
    fn test_lossless_backpressure_draining() {
        // Stream storage large enough for 8 stereo frames of 16-bit PCM (32 bytes)
        let mut stream_storage = [0u8; 128];
        // Output storage restricted to 32 bytes (capacity 32 -> available_write = 31 bytes -> max 3 frames = 24 bytes)
        let mut output_storage = [0u8; 32];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        // Generate 8 stereo frames (16 samples) of 16-bit PCM
        let mut raw_stream = [0u8; 32];
        for frame in 0..8i16 {
            let l = ((frame + 1) * 100).to_be_bytes();
            let r = (-((frame + 1) * 100)).to_be_bytes();
            let off = frame as usize * 4;
            raw_stream[off..off + 2].copy_from_slice(&l);
            raw_stream[off + 2..off + 4].copy_from_slice(&r);
        }

        let ingested = pipeline.ingest_stream_data(&raw_stream).unwrap();
        assert_eq!(ingested, 32);

        // First decode step:
        // Output buffer only has room for 3 frames (24 bytes).
        // Decoder produces 8 frames. 3 frames are written, 5 frames remain pending!
        let consumed = pipeline.process_decode_step().unwrap();
        assert_eq!(consumed, 32); // All stream bytes ingested by decoder
        assert_eq!(pipeline.pcm_pending_samples, 10); // 5 frames * 2 samples
        assert_eq!(pipeline.pcm_pending_offset, 6);   // 3 frames * 2 samples

        // If we try to decode again immediately while output buffer is full:
        let drained_zero = pipeline.process_decode_step().unwrap();
        assert_eq!(drained_zero, 0); // Backpressure: output buffer has < 8 bytes free

        // Output step 1: drain the 3 frames in output_buffer to the sink
        let out_frames1 = pipeline.process_output_step().unwrap();
        assert_eq!(out_frames1, 3);
        assert_eq!(pipeline.frames_played, 3);

        // Second decode step: drains up to 3 more pending frames
        let drained1 = pipeline.process_decode_step().unwrap();
        assert_eq!(drained1, 3);
        assert_eq!(pipeline.pcm_pending_samples, 4);  // 2 frames * 2 samples remain
        assert_eq!(pipeline.pcm_pending_offset, 12);  // 6 frames * 2 samples

        // Output step 2: drain these 3 frames to sink
        let out_frames2 = pipeline.process_output_step().unwrap();
        assert_eq!(out_frames2, 3);
        assert_eq!(pipeline.frames_played, 6);

        // Third decode step: drains remaining 2 pending frames
        let drained2 = pipeline.process_decode_step().unwrap();
        assert_eq!(drained2, 2);
        assert_eq!(pipeline.pcm_pending_samples, 0); // All pending drained!
        assert_eq!(pipeline.pcm_pending_offset, 0);  // Offset reset to 0

        // Output step 3: drain final 2 frames to sink
        let out_frames3 = pipeline.process_output_step().unwrap();
        assert_eq!(out_frames3, 2);
        assert_eq!(pipeline.frames_played, 8);

        // Verify all 8 stereo frames (16 samples) arrived at sink completely lossless!
        let recorded = pipeline.sink.recorded_samples();
        assert_eq!(recorded.len(), 16);
        for frame in 0..8i16 {
            let expected_l = ((frame + 1) * 100) as i32 * 65536;
            let expected_r = -((frame + 1) * 100) as i32 * 65536;
            assert_eq!(recorded[frame as usize * 2], expected_l);
            assert_eq!(recorded[frame as usize * 2 + 1], expected_r);
        }
    }

    struct FaultyMockSink {
        fail_next: bool,
        max_accept_frames: Option<usize>,
        recorded: std::vec::Vec<i32>,
        muted: bool,
    }

    impl FaultyMockSink {
        fn new() -> Self {
            Self {
                fail_next: false,
                max_accept_frames: None,
                recorded: std::vec::Vec::new(),
                muted: false,
            }
        }
    }

    impl AudioOutputSink for FaultyMockSink {
        type Error = OutputError;

        fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
            if self.fail_next {
                return Err(OutputError::HardwareFault);
            }
            let avail = frames.len() / 2;
            let to_write = match self.max_accept_frames {
                Some(limit) => limit.min(avail),
                None => avail,
            };
            self.recorded.extend_from_slice(&frames[..to_write * 2]);
            Ok(to_write)
        }

        fn set_sample_rate(&mut self, _rate_hz: u32) -> Result<(), Self::Error> {
            Ok(())
        }

        fn set_mute(&mut self, muted: bool) {
            self.muted = muted;
        }
    }

    #[test]
    fn test_non_destructive_output_recovery_and_partial_writes() {
        let mut stream_storage = [0u8; 128];
        let mut output_storage = [0u8; 128];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = FaultyMockSink::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        // Ingest 4 stereo frames
        let mut raw_stream = [0u8; 16];
        for frame in 0..4i16 {
            let l = ((frame + 1) * 111).to_be_bytes();
            let r = (-((frame + 1) * 111)).to_be_bytes();
            let off = frame as usize * 4;
            raw_stream[off..off + 2].copy_from_slice(&l);
            raw_stream[off + 2..off + 4].copy_from_slice(&r);
        }
        pipeline.ingest_stream_data(&raw_stream).unwrap();
        pipeline.process_decode_step().unwrap();

        // Output buffer now holds 4 frames (32 bytes)
        assert_eq!(pipeline.output_buffer_available(), 32);

        // 1. Simulate hardware fault in sink.write_frames
        pipeline.sink.fail_next = true;
        let err = pipeline.process_output_step();
        assert_eq!(err, Err(OutputError::HardwareFault));

        // CRITICAL CHECK: None of the bytes in output_buffer were consumed!
        assert_eq!(pipeline.output_buffer_available(), 32);
        assert_eq!(pipeline.frames_played, 0);

        // 2. Clear failure, but simulate partial write (sink accepts only 1 frame)
        pipeline.sink.fail_next = false;
        pipeline.sink.max_accept_frames = Some(1);

        let written1 = pipeline.process_output_step().unwrap();
        assert_eq!(written1, 1);
        assert_eq!(pipeline.frames_played, 1);
        // Only 1 frame (8 bytes) was consumed; 3 frames (24 bytes) remain!
        assert_eq!(pipeline.output_buffer_available(), 24);

        // 3. Sink accepts all remaining frames
        pipeline.sink.max_accept_frames = None;
        let written2 = pipeline.process_output_step().unwrap();
        assert_eq!(written2, 3);
        assert_eq!(pipeline.frames_played, 4);
        assert_eq!(pipeline.output_buffer_available(), 0);

        // Verify that the recorded samples match all 4 frames in perfect order
        assert_eq!(pipeline.sink.recorded.len(), 8);
        for frame in 0..4i16 {
            let expected_l = ((frame + 1) * 111) as i32 * 65536;
            let expected_r = -((frame + 1) * 111) as i32 * 65536;
            assert_eq!(pipeline.sink.recorded[frame as usize * 2], expected_l);
            assert_eq!(pipeline.sink.recorded[frame as usize * 2 + 1], expected_r);
        }
    }

    #[test]
    fn test_pipeline_flush_resets_pending_and_mutes() {
        let mut stream_storage = [0u8; 128];
        let mut output_storage = [0u8; 32];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        let raw_stream = [1u8; 32];
        pipeline.ingest_stream_data(&raw_stream).unwrap();
        let _ = pipeline.process_decode_step();

        // Pending samples are set
        assert!(pipeline.pcm_pending_samples > 0);
        assert!(pipeline.has_pending_pcm());
        assert!(pipeline.output_buffer_available() > 0);

        pipeline.flush();

        assert_eq!(pipeline.pcm_pending_offset, 0);
        assert_eq!(pipeline.pcm_pending_samples, 0);
        assert!(!pipeline.has_pending_pcm());
        assert_eq!(pipeline.stream_buffer_available(), 0);
        assert_eq!(pipeline.output_buffer_available(), 0);
        assert!(pipeline.sink.muted);
    }

    #[test]
    fn test_pcm_scratch_samples_capacity_for_flac_4096_frames() {
        // PIPE-502: 4096 stereo frames require 8192 i32 samples
        assert_eq!(PCM_SCRATCH_SAMPLES, 8192);
    }

    #[test]
    fn test_corrupted_frame_consumes_byte_and_prevents_infinite_loop() {
        // PIPE-501: When decoder returns an error other than NeedMoreData / BufferOverflow,
        // process_decode_step must consume at least 1 byte so the pipeline progresses.
        let mut stream_storage = [0u8; 64];
        let mut output_storage = [0u8; 128];
        // Invalid bit depth (e.g. 7 bits) causes PcmDecoder::decode to return Err(DecoderError::InvalidHeader)
        let bad_pcm_decoder = PcmDecoder::new(7, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(bad_pcm_decoder),
            sink,
            44100,
        );

        pipeline.ingest_stream_data(&[0xAA, 0xBB, 0xCC, 0xDD]).unwrap();
        assert_eq!(pipeline.stream_buffer_available(), 4);

        let res = pipeline.process_decode_step();
        assert!(res.is_err());
        // Must have consumed 1 byte to prevent infinite busy loop!
        assert_eq!(pipeline.stream_buffer_available(), 3);
    }

    #[test]
    fn test_pipeline_propagates_dynamic_decoder_sample_rate() {
        let mut stream_storage = [0u8; 128];
        let mut output_storage = [0u8; 128];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );
        assert_eq!(pipeline.sample_rate, 44100);
        assert_eq!(pipeline.sink.sample_rate, 44100);

        // 44-byte WAV header: 48000 Hz, 16-bit LE stereo
        let mut header = [0u8; 44];
        header[0..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(44u32 + 8 - 8).to_le_bytes());
        header[8..12].copy_from_slice(b"WAVE");
        header[12..16].copy_from_slice(b"fmt ");
        header[16..20].copy_from_slice(&16u32.to_le_bytes());
        header[20..22].copy_from_slice(&1u16.to_le_bytes());
        header[22..24].copy_from_slice(&2u16.to_le_bytes());
        header[24..28].copy_from_slice(&48000u32.to_le_bytes());
        header[28..32].copy_from_slice(&(48000u32 * 2 * 2).to_le_bytes());
        header[32..34].copy_from_slice(&4u16.to_le_bytes());
        header[34..36].copy_from_slice(&16u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&8u32.to_le_bytes());

        // 2 stereo frames (8 bytes) of 16-bit LE
        let f1_l = (1000i16).to_le_bytes();
        let f1_r = (-1000i16).to_le_bytes();
        let f2_l = (2000i16).to_le_bytes();
        let f2_r = (-2000i16).to_le_bytes();
        let pcm_data = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        let mut payload = [0u8; 52];
        payload[0..44].copy_from_slice(&header);
        payload[44..52].copy_from_slice(&pcm_data);

        pipeline.ingest_stream_data(&payload).unwrap();
        let consumed = pipeline.process_decode_step().unwrap();
        assert_eq!(consumed, 52);

        assert_eq!(pipeline.sample_rate, 48000);
        assert_eq!(pipeline.sink.sample_rate, 48000);
    }

    #[test]
    fn test_mp3_corrupt_frame_skips_and_recovers() {
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mp3_decoder = squeezelite_decoders::Mp3Decoder::new();
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Mp3(Box::new(mp3_decoder)),
            sink,
            44100,
        );

        // Ingest corrupted / garbage bytes
        let garbage = [0x55, 0xAA, 0x12, 0x34, 0xFE, 0xDC, 0xBA, 0x98];
        pipeline.ingest_stream_data(&garbage).unwrap();
        assert_eq!(pipeline.stream_buffer_available(), 8);

        // Process decode steps: must consume bytes and progress without deadlock
        for _ in 0..8 {
            if pipeline.stream_buffer_available() == 0 {
                break;
            }
            let _ = pipeline.process_decode_step();
        }

        assert_eq!(pipeline.stream_buffer_available(), 0);
    }

    #[test]
    fn test_pipeline_apply_eq_preset() {
        let mut stream_mem = [0u8; 4096];
        let mut out_mem = [0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44_100,
        );
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::Flat);

        pipeline.apply_eq_preset(squeezelite_audio_core::EqPreset::BassBoost);
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::BassBoost);
        assert!(pipeline.equalizer.is_enabled());

        pipeline.apply_eq_preset(squeezelite_audio_core::EqPreset::Flat);
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::Flat);
        assert!(!pipeline.equalizer.is_enabled());
    }

    #[test]
    fn test_pipeline_loudness_contour() {
        let mut stream_mem = [0u8; 4096];
        let mut out_mem = [0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44_100,
        );
        assert!(!pipeline.loudness_enabled());

        pipeline.set_loudness(true);
        assert!(pipeline.loudness_enabled());

        pipeline.set_loudness(false);
        assert!(!pipeline.loudness_enabled());
    }

    #[test]
    fn test_pipeline_select_format_codecs() {
        let mut stream_mem = [0u8; 4096];
        let mut out_mem = [0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44_100,
        );

        pipeline.select_format(b'm');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));

        pipeline.select_format(b'f');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Flac(_)));

        pipeline.select_format(b'a');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Aac(_)));

        pipeline.select_format(b'o');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Vorbis(_)));

        pipeline.select_format(b'p');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));

        // Unknown format defaults to PCM
        pipeline.select_format(b'?');
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));
    }

    #[test]
    fn test_pipeline_eq_processed_before_volume_scaling() {
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        pipeline.set_eq_preset(EqPreset::BassBoost);
        assert_eq!(pipeline.eq_preset(), EqPreset::BassBoost);

        // 50% digital volume attenuation
        pipeline.set_volume(0x00008000, 0x00008000);

        // Feed 1 stereo frame (4 bytes) of 16-bit PCM: (4000, -4000)
        let f1_l = (4000i16).to_be_bytes();
        let f1_r = (-4000i16).to_be_bytes();
        let raw = [f1_l[0], f1_l[1], f1_r[0], f1_r[1]];

        pipeline.ingest_stream_data(&raw).unwrap();
        let consumed = pipeline.step_audio_pipeline().unwrap();
        assert_eq!(consumed, 4);

        let out = pipeline.process_output_step().unwrap();
        assert_eq!(out, 1);
        assert_eq!(pipeline.sink.recorded_samples().len(), 2);
    }

    #[test]
    fn test_pipeline_all_eq_presets_roundtrip() {
        let mut stream_mem = [0u8; 1024];
        let mut out_mem = [0u8; 1024];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveCodec::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44_100,
        );

        let presets = [
            EqPreset::Flat,
            EqPreset::BassBoost,
            EqPreset::Loudness,
            EqPreset::Vocal,
            EqPreset::Rock,
        ];

        for &p in &presets {
            pipeline.set_eq_preset(p);
            assert_eq!(pipeline.eq_preset(), p);
            if p == EqPreset::Flat {
                assert!(!pipeline.equalizer.is_enabled());
            } else {
                assert!(pipeline.equalizer.is_enabled());
            }
        }
    }

    #[test]
    fn test_user_muted_preserves_mute_state_in_process_output_step() {
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        let f1_l = (1000i16).to_be_bytes();
        let f1_r = (-1000i16).to_be_bytes();
        let raw_stream = [f1_l[0], f1_l[1], f1_r[0], f1_r[1]];

        pipeline.ingest_stream_data(&raw_stream).unwrap();
        pipeline.process_decode_step().unwrap();

        // Mute via user command
        pipeline.set_mute(true);
        assert!(pipeline.user_muted);
        assert!(pipeline.sink.muted);

        // process_output_step must NOT unmute when user_muted is true
        let written = pipeline.process_output_step().unwrap();
        assert_eq!(written, 1);
        assert!(pipeline.sink.muted);

        // Unmute via user command
        pipeline.set_mute(false);
        assert!(!pipeline.user_muted);
        assert!(!pipeline.sink.muted);
    }

    #[test]
    fn test_set_sample_rate_failure_preserves_previous_rate() {
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_decoder = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_decoder),
            sink,
            44100,
        );

        assert_eq!(pipeline.sample_rate, 44100);

        // Attempt invalid sample rate on sink (e.g. 999_999)
        let res = pipeline.set_sample_rate(999_999);
        assert!(res.is_err());
        assert_eq!(pipeline.sample_rate, 44100);

        // Valid sample rate updates both sink and pipeline
        let res = pipeline.set_sample_rate(96000);
        assert!(res.is_ok());
        assert_eq!(pipeline.sample_rate, 96000);
        assert_eq!(pipeline.sink.sample_rate, 96000);
    }

    #[test]
    fn test_pipeline_flac_playback_flow() {
        let flac_bytes = include_bytes!("../../squeezelite-decoders/tests/fixtures/stereo_16bit.flac");
        let mut stream_storage = [0u8; 16384];
        let mut output_storage = [0u8; 16384];
        let flac_decoder = squeezelite_decoders::FlacDecoder::new();
        let sink = MockAudioSink::<32768>::new();

        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Flac(Box::new(flac_decoder)),
            sink,
            44100,
        );

        let mut offset = 0;
        let mut safety_counter = 0;
        while (offset < flac_bytes.len() || pipeline.stream_buffer_available() > 0 || pipeline.has_pending_pcm()) && safety_counter < 100_000 {
            safety_counter += 1;
            if offset < flac_bytes.len() {
                if let Ok(ingested) = pipeline.ingest_stream_data(&flac_bytes[offset..]) {
                    offset += ingested;
                }
            }
            let _ = pipeline.process_decode_step();
            let _ = pipeline.process_output_step();
        }
        while pipeline.output_buffer_available() >= 4 && safety_counter < 100_000 {
            safety_counter += 1;
            let _ = pipeline.process_output_step();
        }

        assert!(pipeline.frames_played > 0, "Frames played should be > 0, got {}", pipeline.frames_played);
        assert_eq!(pipeline.frames_played, 8820);
    }
}

