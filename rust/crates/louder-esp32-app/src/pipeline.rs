//! Real-Time Streaming Audio Pipeline Engine
//!
//! Connects LMS stream buffers, pure-Rust decoders, DSP equalizer,
//! volume scaling, and the I2S audio output sink into an integrated processing loop.

extern crate alloc;
use alloc::boxed::Box;

use squeezelite_audio_core::{AudioOutputSink, AudioRingBuffer, Equalizer, GainStage, RingBufferError};
use squeezelite_audio_core::fade::{cross_sample, ChunkGain, FadeEngine, FadeStep};
use squeezelite_decoders::{AudioDecoder, DecoderError, PcmDecoder};

pub use squeezelite_audio_core::EqPreset;
pub use squeezelite_audio_core::FadeMode;
pub use squeezelite_audio_core::ChannelMode;
pub type ActiveCodec = ActiveDecoder;

// Scratch sizing follows the codec set: the `std-codecs` decoders (FLAC 4096-frame blocks, AAC,
// Vorbis) need the large buffers; the firmware codec set fits the smaller ones.
#[cfg(feature = "std-codecs")]
pub const PCM_SCRATCH_SAMPLES: usize = 8192;
#[cfg(not(feature = "std-codecs"))]
pub const PCM_SCRATCH_SAMPLES: usize = 2304;

#[cfg(feature = "std-codecs")]
pub const READ_SCRATCH_BYTES: usize = 4096;
#[cfg(not(feature = "std-codecs"))]
pub const READ_SCRATCH_BYTES: usize = 1536;
pub const WRITE_SCRATCH_BYTES: usize = 512;
pub const OUTPUT_I32_SCRATCH_SAMPLES: usize = 128;

/// Stream ring of the firmware when PSRAM is mapped: 512 KiB (C `-b 500:2000` asks 500 KiB;
/// C parity: main/Kconfig.projbuild:465).
pub const PSRAM_STREAM_RING_BYTES: usize = 524_288;
/// Output ring of the firmware when PSRAM is mapped: 2000 KiB, about 5.8 s at 44.1 kHz (C
/// parity: main/Kconfig.projbuild:465 `-b 500:2000`; main.c:410 `atoi(o) * 1024`). A crossfade
/// can use up to 90 % of it (C output.c:330), 230,400 frames.
pub const PSRAM_OUTPUT_RING_BYTES: usize = 2_000 * 1024;
/// Size of the external PSRAM heap region (4 MiB, mapped at 0x3F80_0000).
pub const PSRAM_HEAP_BYTES: usize = 4 * 1024 * 1024;

/// Storage of the MP3 decoder carried by [`ActiveDecoder::Mp3`].
///
/// With `std-codecs` each track gets a fresh heap-boxed decoder. Without it (firmware) the
/// decoder is a static singleton handed out by bring-up, so no MP3 state lives on the heap.
#[cfg(feature = "std-codecs")]
pub type Mp3Handle = Box<squeezelite_decoders::Mp3Decoder>;
/// Storage of the MP3 decoder carried by [`ActiveDecoder::Mp3`].
///
/// With `std-codecs` each track gets a fresh heap-boxed decoder. Without it (firmware) the
/// decoder is a static singleton handed out by bring-up, so no MP3 state lives on the heap.
#[cfg(not(feature = "std-codecs"))]
pub type Mp3Handle = &'static mut squeezelite_decoders::StaticMp3Decoder;

pub enum ActiveDecoder {
    Pcm(PcmDecoder),
    Mp3(Mp3Handle),
    Flac(Box<squeezelite_decoders::FlacDecoder>),
    #[cfg(feature = "std-codecs")]
    Aac(Box<squeezelite_decoders::AacDecoder>),
    #[cfg(feature = "std-codecs")]
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
            Self::Mp3(d) => d.decode(encoded_in, pcm_out),
            Self::Flac(d) => d.decode(encoded_in, pcm_out),
            #[cfg(feature = "std-codecs")]
            Self::Aac(d) => d.decode(encoded_in, pcm_out),
            #[cfg(feature = "std-codecs")]
            Self::Vorbis(d) => d.decode(encoded_in, pcm_out),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Pcm(d) => d.flush(),
            Self::Mp3(d) => d.flush(),
            Self::Flac(d) => d.flush(),
            #[cfg(feature = "std-codecs")]
            Self::Aac(d) => d.flush(),
            #[cfg(feature = "std-codecs")]
            Self::Vorbis(d) => d.flush(),
        }
    }
}

impl ActiveDecoder {
    /// Returns the active stream sample rate in Hz reported by the decoder, if known.
    pub fn sample_rate(&self) -> Option<u32> {
        let rate = match self {
            Self::Pcm(d) => return d.sample_rate,
            Self::Mp3(d) => d.sample_rate,
            Self::Flac(d) => d.sample_rate,
            #[cfg(feature = "std-codecs")]
            Self::Aac(d) => d.sample_rate,
            #[cfg(feature = "std-codecs")]
            Self::Vorbis(d) => d.sample_rate,
        };
        if rate > 0 {
            Some(rate)
        } else {
            None
        }
    }

    /// A fresh MP3 decoder, or `None` when this build cannot create one (without `std-codecs`
    /// the static MP3 decoder is owned by bring-up).
    #[cfg(feature = "std-codecs")]
    pub fn new_mp3() -> Option<Self> {
        Some(Self::Mp3(Box::new(squeezelite_decoders::Mp3Decoder::new())))
    }

    /// A fresh MP3 decoder, or `None` when this build cannot create one (without `std-codecs`
    /// the static MP3 decoder is owned by bring-up).
    #[cfg(not(feature = "std-codecs"))]
    pub fn new_mp3() -> Option<Self> {
        None
    }

    /// A fresh AAC decoder, or `None` when the build lacks `std-codecs`.
    #[cfg(feature = "std-codecs")]
    pub fn new_aac() -> Option<Self> {
        Some(Self::Aac(Box::new(squeezelite_decoders::AacDecoder::new())))
    }

    /// A fresh AAC decoder, or `None` when the build lacks `std-codecs`.
    #[cfg(not(feature = "std-codecs"))]
    pub fn new_aac() -> Option<Self> {
        None
    }

    /// A fresh Ogg Vorbis decoder, or `None` when the build lacks `std-codecs`.
    #[cfg(feature = "std-codecs")]
    pub fn new_vorbis() -> Option<Self> {
        Some(Self::Vorbis(Box::new(squeezelite_decoders::VorbisDecoder::new())))
    }

    /// A fresh Ogg Vorbis decoder, or `None` when the build lacks `std-codecs`.
    #[cfg(not(feature = "std-codecs"))]
    pub fn new_vorbis() -> Option<Self> {
        None
    }

    /// A fresh FLAC decoder (available in every build).
    pub fn new_flac() -> Self {
        Self::Flac(Box::new(squeezelite_decoders::FlacDecoder::new()))
    }

    /// Returns whether the active decoder has unconsumed decoded PCM frames waiting in its internal buffer.
    pub fn has_pending_pcm(&self) -> bool {
        match self {
            Self::Flac(d) => d.has_pending_pcm(),
            _ => false,
        }
    }
}

/// Q16.16 unity gain (C `FIXED_ONE`).
const UNITY_Q16: u32 = 0x1_0000;

/// Output-ring bytes per stereo frame (two native-endian `i32` samples; C `BYTES_PER_FRAME`).
const OUTPUT_FRAME_BYTES: usize = 8;

/// The pending track boundary in the output ring: C squeezelite's `output.track_start`
/// pointer, plus the `output.next_*` values that take effect when playback reaches it.
///
/// C keeps exactly one such pointer; a newer track start overwrites one playback has not
/// reached yet (`flac.c:147`, `mad.c:256`, `pcm.c:216`). The pipeline therefore holds a single
/// `Option<TrackMarker>` ([`StreamPipeline::track_marker`]), not a queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackMarker {
    /// Unread output-ring bytes in front of the boundary (C `track_start - readp`). Always a
    /// whole number of frames and never more than the unread bytes: the boundary never sits
    /// behind the read position or past the write position.
    pub bytes_ahead: u64,
    /// Rate of the track that starts here (C `output.next_sample_rate`).
    pub sample_rate: u32,
    /// Replay gain of the track that starts here, 16.16 fixed point, 0 = none (C
    /// `output.next_replay_gain`).
    pub replay_gain: u32,
    /// `strm s` output threshold (C `output.threshold`, tenths of a second) in frames at
    /// [`Self::sample_rate`]. Stored for the output-start gate (Task 15); not applied yet.
    pub output_threshold_frames: u32,
}

/// Track-boundary state of the output side (C `output.track_start`, `next_*`, `current_*`,
/// `delay_active`, `track_started`; `decode.new_stream`). Built by [`TrackBoundary::new`] so
/// firmware bring-up can initialise the whole group with one write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackBoundary {
    marker: Option<TrackMarker>,
    /// The boundary playback crossed and nobody has taken yet (C `output.track_started`).
    crossed: Option<TrackMarker>,
    /// The next decoded frames start a new track (C `decode.new_stream`).
    new_stream: bool,
    /// Rate for a new track whose decoder reports none (raw PCM): the `strm s` rate, or the
    /// rate last set explicitly with [`StreamPipeline::set_sample_rate`].
    stream_rate_hint: u32,
    /// `strm s` output threshold, tenths of a second (C `output.threshold`).
    output_threshold: u8,
    /// Silent frames still to play for the boundary rate-change delay (C `output.pause_frames`
    /// while `OUTPUT_PAUSE_FRAMES` from `output.c:139-147`).
    silence_frames: u32,
    /// The first half of the rate-change delay was played (C `output.delay_active`).
    delay_active: bool,
    /// Replay gain of the playing track, 16.16 fixed point, 0 = none (C
    /// `output.current_replay_gain`). The gain stage applies it to every output sample
    /// outside a crossfade (C `output.c:54-55`); inside one, the crossfade weights carry it.
    pub current_replay_gain: u32,
    /// Replay gain from the latest `strm s` (C `output.next_replay_gain`, `slimproto.c:485`).
    pub next_replay_gain: u32,
    /// Silence in ms played around a track boundary that changes the sample rate, half before
    /// it at the old rate and half after it at the new one (C `output.rate_delay`, the `delay`
    /// of `-r rates:delay`, `main.c:298`, `main.c:509`). Default 0: no silence.
    pub rate_delay_ms: u32,
    /// Rate of the newest track start (C `output.next_sample_rate`), which sizes a fade at
    /// the end of its decoding (C `_checkfade(false)`, `output.c:292`).
    next_sample_rate: u32,
}

impl TrackBoundary {
    /// No boundary pending; the first decoded frames start a track (C `decode_init` sets
    /// `decode.new_stream`, `decode.c:223`).
    pub const fn new(rate_hint: u32) -> Self {
        Self {
            marker: None,
            crossed: None,
            new_stream: true,
            stream_rate_hint: rate_hint,
            output_threshold: 0,
            silence_frames: 0,
            delay_active: false,
            current_replay_gain: 0,
            next_replay_gain: 0,
            rate_delay_ms: 0,
            next_sample_rate: rate_hint,
        }
    }
}

/// The integrated audio pipeline engine.
pub struct StreamPipeline<'a, S: AudioOutputSink> {
    pub stream_buffer: AudioRingBuffer<'a>,
    pub output_buffer: AudioRingBuffer<'a>,
    pub decoder: ActiveDecoder,
    pub equalizer: Equalizer<10>,
    /// The composite output gain (C `output.gainL/R`, `current_replay_gain`, fade gain,
    /// `invert`, `channels`), applied to every chunk the output step writes.
    pub gain_stage: GainStage,
    /// The volume LMS last asked for, Q16.16 per channel (C `set_volume`'s arguments,
    /// slimproto.c:569), see [`Self::requested_volume_q16`].
    pub requested_volume: (u32, u32),
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
    /// MP3 decoder parked while another decoder is active, reused by
    /// [`Self::use_mp3_decoder`]. Firmware bring-up hands its static MP3 decoder in here once
    /// (there is no heap MP3 decoder without `std-codecs`); [`Self::set_decoder`] parks a
    /// replaced MP3 decoder here, so the static one is never lost or duplicated.
    pub mp3_spare: Option<Mp3Handle>,
    /// Gapless track-boundary state (C `output.track_start` and friends).
    pub boundary: TrackBoundary,
    /// Fade-in/out and crossfade state (C `output.fade*`), applied by the output step.
    pub fade: FadeEngine,
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
            gain_stage: GainStage::unity(),
            requested_volume: (UNITY_Q16, UNITY_Q16),
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
            mp3_spare: None,
            boundary: TrackBoundary::new(sample_rate),
            fade: FadeEngine::new(),
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

    /// Decodes a chunk from `stream_buffer`, processes EQ, and stores it in `output_buffer`.
    /// Gain (volume, replay gain, fade, polarity, channel select) is applied when the output
    /// step plays the frames, as C does (output_i2s.c:510 `_apply_gain`).
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
                self.mark_track_start();
                let active_pcm = &mut self.pcm_scratch[..frames_produced * 2];
                self.equalizer.process_interleaved(active_pcm);

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

        // Only consume the exact bytes ingested by decoder (CONC-005)
        if consumed > 0 {
            self.stream_buffer.consume(consumed);
        }

        if frames_produced > 0 {
            // The decoder's rate is the next track's rate, applied when playback reaches the
            // track start (C parity: pcm.c:216-236, mad.c:254-256, flac.c:147-170 set
            // `output.next_sample_rate` with `track_start`; output.c:154 switches). A rate
            // change inside a track is ignored, as in C (only `new_stream` reads the rate).
            self.mark_track_start();
            // Equalizer preset and loudness contour, in place, exactly once before buffering.
            // The gain stage runs at output time (C parity: output.c:54-57, output_i2s.c:510),
            // so a volume change reaches audio already in the output ring at once, as in C.
            let active_pcm = &mut self.pcm_scratch[..frames_produced * 2];
            self.equalizer.process_interleaved(active_pcm);

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
    ///
    /// Track boundaries follow C `_output_frames` (`output.c:126-166`): a chunk never runs past
    /// the pending [`TrackMarker`], and a step that starts on it processes the track start
    /// first ([`Self::active_marker_crossed`] then returns it) and plays on into the new track.
    /// Fades follow `output.c:174-252` ([`Self::prepare_fade`], [`Self::scale_chunk`]): a
    /// chunk never runs past a fade start or end either.
    /// Returns the frames written to the sink, including rate-change silence.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn process_output_step(&mut self) -> Result<usize, S::Error> {
        if self.boundary.silence_frames > 0 {
            return self.write_boundary_silence();
        }

        if self.output_buffer.available_read() < OUTPUT_FRAME_BYTES {
            return Ok(0); // Minimum 1 stereo frame (8 bytes)
        }

        if let Some(marker) = self.boundary.marker {
            if marker.bytes_ahead == 0 {
                // C parity: output.c:127 `track_start == readp`, only with real frames to
                // play (`frames == 0` is silence there, output.c:111-114).
                self.reach_track_start(marker);
                if self.boundary.silence_frames > 0 {
                    return self.write_boundary_silence();
                }
            }
        }

        // C parity: output.c:174-252, after the track start (output.c:126-166).
        let fade = self.prepare_fade();
        let bytes_avail = self.output_buffer.available_read();
        let mut to_read = core::cmp::min(self.write_scratch.len(), bytes_avail);
        if let Some(marker) = self.boundary.marker {
            // C parity: output.c:163-166, the chunk ends exactly on the track start.
            to_read = to_read.min(usize::try_from(marker.bytes_ahead).unwrap_or(usize::MAX));
        }
        if let Some(max_frames) = fade.max_frames {
            // C parity: output.c:178-180, 212-214, the chunk ends exactly on the fade start/end.
            let max_bytes = usize::try_from(max_frames).unwrap_or(usize::MAX).saturating_mul(OUTPUT_FRAME_BYTES);
            to_read = to_read.min(max_bytes);
        }
        let bytes_to_read = (to_read / OUTPUT_FRAME_BYTES) * OUTPUT_FRAME_BYTES;
        if bytes_to_read == 0 {
            return Ok(0);
        }

        // Non-destructive peek: frames remain in ringbuffer until sink write succeeds (LOGIC-603)
        let bytes_read = self.output_buffer.peek(&mut self.write_scratch[..bytes_to_read]);
        if bytes_read < OUTPUT_FRAME_BYTES {
            return Ok(0);
        }

        let frames_to_write = bytes_read / OUTPUT_FRAME_BYTES;
        let samples_count = frames_to_write * 2;

        for (chunk, sample) in self.write_scratch[..frames_to_write * OUTPUT_FRAME_BYTES]
            .chunks_exact(4)
            .zip(self.output_i32_scratch[..samples_count].iter_mut())
        {
            *sample = i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        self.scale_chunk(frames_to_write, fade.gain);

        if !self.user_muted {
            self.sink.set_mute(false);
        }
        let written_frames = self.sink.write_frames(&self.output_i32_scratch[..frames_to_write * 2])?;
        // ONLY consume the exact number of frames successfully committed to the sink
        self.consume_output(written_frames);
        if written_frames > 0 {
            self.frames_played = self.frames_played.wrapping_add(written_frames as u64);
        }

        Ok(written_frames)
    }

    /// The fade part of one C `_output_frames` iteration (output.c:174-252) for the chunk
    /// about to be written. A completed crossfade skips the new track's first `dur_f` frames,
    /// which were already played inside the mix (output.c:196-200: not counted as played),
    /// and makes the next replay gain current (output.c:203).
    fn prepare_fade(&mut self) -> FadeStep {
        let used = (self.output_buffer.available_read() / OUTPUT_FRAME_BYTES) as u64;
        // C `size`: the frames this call wants to write.
        let size = used.min((self.write_scratch.len() / OUTPUT_FRAME_BYTES) as u64);
        let step = self.fade.prepare(used, size, self.boundary.current_replay_gain, self.boundary.next_replay_gain);
        if step.skip_frames > 0 {
            self.consume_output(usize::try_from(step.skip_frames).unwrap_or(usize::MAX));
        }
        if step.crossfade_done {
            self.boundary.current_replay_gain = self.boundary.next_replay_gain;
        }
        step
    }

    /// Scales the chunk in `output_i32_scratch` (C `write_cb`: `_apply_cross` then
    /// `_apply_gain`, output_i2s.c:504-511).
    ///
    /// - Crossfade: each old sample (at the read position) is mixed with the new track's
    ///   sample `dur_f` frames further on (C `cross_ptr = fade_end + cur_f`, output.c:245,
    ///   output_pack.c:366-376), each weighted by its fade weight and its own replay gain.
    /// - Then the gain stage, the one place the output gain is composed: volume x current
    ///   replay gain x fade-up/down gain, negated with `invert`, then channel select (C parity:
    ///   output.c:54-57, :226-228; output_pack.c:383-410).
    ///
    /// Replay gain is applied exactly once per sample: outside a crossfade by the gain stage
    /// (C output.c:54-55); inside one only by the crossfade weights, because C's standard gain
    /// there is the plain volume `output.gainL/R` (output.c:243-245). The level is therefore
    /// continuous across both crossfade edges.
    ///
    /// Deliberate divergence (ruled in Task 13): during a fade up/down C negates the inverted
    /// gain a second time (output.c:57 then :229), so it plays non-inverted for the fade
    /// only, with a polarity flip at each fade edge. Here invert negates exactly once.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn scale_chunk(&mut self, frames: usize, chunk_gain: ChunkGain) {
        let samples = &mut self.output_i32_scratch[..frames * 2];
        let crossfading = if let ChunkGain::Cross { gain_in, gain_out, new_ahead } = chunk_gain {
            let offset = usize::try_from(new_ahead).unwrap_or(usize::MAX).saturating_mul(OUTPUT_FRAME_BYTES);
            // `prepare` checked that the new frames follow (output.c:231): the old samples are
            // already converted, so the byte scratch is free for them.
            let got = self.output_buffer.peek_at(offset, &mut self.write_scratch[..frames * OUTPUT_FRAME_BYTES]);
            // `prepare` guarantees used > dur_f + size, so the whole chunk of new frames is
            // there; a short read would mix only part of the chunk.
            debug_assert_eq!(got, frames * OUTPUT_FRAME_BYTES, "crossfade new-track frames missing");
            for (sample, new) in samples.iter_mut().zip(self.write_scratch[..got].chunks_exact(4)) {
                let new = i32::from_ne_bytes([new[0], new[1], new[2], new[3]]);
                *sample = cross_sample(*sample, new, gain_out, gain_in);
            }
            true
        } else {
            false
        };
        // C parity: output.c:54-55 (replay gain on all output, 0 = none) and :243-245 (plain
        // volume while crossfading: the weights above carry both tracks' replay gains).
        self.gain_stage.replay_gain_q16 = if crossfading { 0 } else { self.boundary.current_replay_gain };
        // C parity: output.c:226-228, the fade gain of this chunk (unity outside a fade
        // up/down). Refreshed once per chunk, so a volume change never touches fade progress.
        self.gain_stage.fade_q16 = self.fade.gain_q16();
        debug_assert!(!matches!(chunk_gain, ChunkGain::Fade(g) if g as u32 != self.gain_stage.fade_q16));
        self.gain_stage.apply(samples);
    }

    /// Playback reached the pending track start (C parity: output.c:127-162).
    ///
    /// With a rate change and [`TrackBoundary::rate_delay_ms`] > 0 the first arrival only
    /// queues `current_rate * delay / 2000` silent frames and leaves the boundary pending
    /// (`delay_active`); the second arrival queues `next_rate * delay / 2000` frames and then
    /// processes the track start, so the silence is split around the rate switch
    /// (output.c:128-148). The track start: `frames_played = 0`, `track_started`, the clock
    /// goes to the next rate, the next replay gain becomes current unless a crossfade holds
    /// the old one until it completes, and the boundary is cleared (output.c:149-162).
    fn reach_track_start(&mut self, marker: TrackMarker) {
        let delay = if self.sample_rate != marker.sample_rate { self.boundary.rate_delay_ms } else { 0 };
        if delay > 0 {
            if !self.boundary.delay_active {
                self.boundary.silence_frames = delay_frames(self.sample_rate, delay);
                self.boundary.delay_active = true;
                return;
            }
            self.boundary.silence_frames = delay_frames(marker.sample_rate, delay);
            self.boundary.delay_active = false;
        }
        self.frames_played = 0;
        self.boundary.crossed = Some(marker);
        // The clock switch is requested here, when the sink reaches the boundary. Frames of
        // the previous track still queued in the sink's DMA then play at the new rate; C has
        // the same window (output_i2s.c:627-643 switches after `_output_frames` and zeroes
        // the DMA). Exact DMA-drain sequencing is Task 15. A refused rate keeps the old clock.
        if marker.sample_rate != self.sample_rate && self.sink.set_sample_rate(marker.sample_rate).is_ok() {
            self.sample_rate = marker.sample_rate;
        }
        // C parity: output.c:157-159, a crossfade keeps the old track's gain for its tail.
        if !self.fade.holds_replay_gain() {
            self.boundary.current_replay_gain = marker.replay_gain;
        }
        self.boundary.marker = None;
    }

    /// Plays queued rate-change silence (C `OUTPUT_PAUSE_FRAMES` silence, output.c:86-96):
    /// zero frames at the sink's current rate, not counted as played (output.c:277-280).
    fn write_boundary_silence(&mut self) -> Result<usize, S::Error> {
        let frames = (self.boundary.silence_frames as usize).min(OUTPUT_I32_SCRATCH_SAMPLES / 2);
        self.output_i32_scratch[..frames * 2].fill(0);
        if !self.user_muted {
            self.sink.set_mute(false);
        }
        let written = self.sink.write_frames(&self.output_i32_scratch[..frames * 2])?;
        self.boundary.silence_frames -= written.min(frames) as u32;
        Ok(written)
    }

    /// The decoder produced frames: if they start a new stream, the track start is the
    /// current write position of the output ring (C parity: pcm.c:214-216, flac.c:145-147,
    /// mad.c:253-256 `output.track_start = outputbuf->writep` when `decode.new_stream`). The
    /// rate is the decoder's, or the `strm s` rate when it reports none (raw PCM, C pcm.c
    /// uses the `strm` rate the codec was opened with). A boundary playback has not reached
    /// yet is overwritten (one C pointer).
    ///
    /// The same moment arms a fade (C parity: flac.c:167/171, pcm.c:233/237, mad.c:257
    /// `if (output.fade_mode) _checkfade(true)`); a crossfade moves the track start to the
    /// fade start (output.c:339).
    fn mark_track_start(&mut self) {
        if !self.boundary.new_stream {
            return;
        }
        self.boundary.new_stream = false;
        let sample_rate = self.decoder.sample_rate().unwrap_or(self.boundary.stream_rate_hint);
        self.boundary.next_sample_rate = sample_rate;
        let output_threshold_frames =
            (u64::from(self.boundary.output_threshold) * u64::from(sample_rate) / 10) as u32;
        let mut bytes_ahead = self.output_buffer.available_read() as u64;
        if self.fade.mode() != FadeMode::None {
            let used = bytes_ahead / OUTPUT_FRAME_BYTES as u64;
            if let Some(start) = self.fade.check_fade(true, used, self.ring_frames(), sample_rate, self.sample_rate) {
                bytes_ahead = start * OUTPUT_FRAME_BYTES as u64;
            }
        }
        self.boundary.marker = Some(TrackMarker {
            bytes_ahead,
            sample_rate,
            replay_gain: self.boundary.next_replay_gain,
            output_threshold_frames,
        });
    }

    /// Output ring size in frames (C `outputbuf->size / BYTES_PER_FRAME`).
    fn ring_frames(&self) -> u64 {
        (self.output_buffer.capacity() / OUTPUT_FRAME_BYTES) as u64
    }

    /// `strm s` transition, polarity and channel select (C parity: slimproto.c:486-489:
    /// `fade_mode = transition_type - '0'`, `fade_secs = transition_period`, `invert`,
    /// `channels`). A fade already armed keeps running, as in C. Polarity and channel select
    /// take effect at once, also on the previous track's tail still playing, as C's
    /// `output.invert` / `output.channels` globals do.
    pub fn set_transition(&mut self, mode: FadeMode, secs: u32, invert: bool, channels: ChannelMode) {
        self.fade.configure(mode, secs);
        self.gain_stage.invert_polarity = invert;
        self.gain_stage.channel_mode = channels;
    }

    /// Decoding of the stream ended, completely or with an error (C parity: decode.c:103
    /// `if (output.fade_mode) _checkfade(false)`): fade-out and fade-in-out arm a fade down
    /// over the last decoded frames (output.c:311-321).
    pub fn fade_track_end(&mut self) {
        if self.fade.mode() == FadeMode::None {
            return;
        }
        let used = (self.output_buffer.available_read() / OUTPUT_FRAME_BYTES) as u64;
        let _ = self.fade.check_fade(false, used, self.ring_frames(), self.boundary.next_sample_rate, self.sample_rate);
    }

    /// Another audio source (Bluetooth) takes the output: no LMS transition applies to it.
    ///
    /// Declared divergence (cleaner): C's Bluetooth start (decode_external.c:147-154)
    /// flushes outputbuf with `_buf_flush`, which leaves `output.fade` and `fade_mode`
    /// untouched, so a fade the LMS track armed can still run over the Bluetooth audio. Here
    /// the mode is cleared (the Rust Bluetooth audio goes through a decoder and would reach
    /// `mark_track_start`), and a fade still due or running is dropped. Polarity inversion is
    /// a player setting and stays, as in C.
    pub fn stop_fades(&mut self) {
        self.fade.configure(FadeMode::None, 0);
        self.fade.reset();
    }

    /// Another audio source (Bluetooth) takes the output: no LMS transition and no LMS replay
    /// gain apply to it ([`Self::stop_fades`], then replay gain none).
    ///
    /// Declared divergence (cleaner): C's Bluetooth start (decode_external.c:147-154) leaves
    /// `output.current_replay_gain` alone, so the last LMS track's replay gain scales the
    /// Bluetooth audio (output.c:54-55). Here both the current and the next replay gain are
    /// cleared. Volume, polarity and channel select are player settings and stay, as in C.
    pub fn begin_external_audio(&mut self) {
        self.stop_fades();
        self.boundary.current_replay_gain = 0;
        self.boundary.next_replay_gain = 0;
        if let Some(marker) = self.boundary.marker.as_mut() {
            marker.replay_gain = 0;
        }
    }

    /// The pending track boundary, if any (C `output.track_start != NULL`).
    pub fn track_marker(&self) -> Option<&TrackMarker> {
        self.boundary.marker.as_ref()
    }

    /// Sets the single pending boundary, replacing (and returning) one playback has not
    /// reached yet, as C overwrites `output.track_start`. `bytes_ahead` is cut to whole
    /// frames within the unread output, so the boundary cannot point past the written data.
    /// The decoder sets it by itself on the first frames of a new stream; this is for
    /// callers that write the output ring directly.
    pub fn set_track_marker(&mut self, mut marker: TrackMarker) -> Option<TrackMarker> {
        let unread = self.output_buffer.available_read() as u64;
        marker.bytes_ahead = marker.bytes_ahead.min(unread) / OUTPUT_FRAME_BYTES as u64 * OUTPUT_FRAME_BYTES as u64;
        self.boundary.marker.replace(marker)
    }

    /// The boundary playback crossed since the last call, once (C `output.track_started`,
    /// which makes the status loop send `STMs`, `slimproto.c:851-855`).
    pub fn active_marker_crossed(&mut self) -> Option<TrackMarker> {
        self.boundary.crossed.take()
    }

    /// `strm s` (C `process_strm`, `slimproto.c:436-489`): the new stream's data replaces the
    /// stream ring (C `stream_sock` flushes `streambuf`, `stream.c:624`, `stream.c:738`), the
    /// old decoder's undelivered PCM goes with it (C `codec_open` closes the codec), and the
    /// next track's values are stored: replay gain (C `next_replay_gain`) and output
    /// threshold (C `output.threshold`), both C globals, so they also update a boundary still
    /// pending. `rate_hint` is the `strm` rate, used when the decoder reports none.
    ///
    /// The output ring is NOT flushed: the previous track's tail keeps playing, which is how
    /// C plays gaplessly.
    pub fn begin_stream(&mut self, rate_hint: u32, replay_gain: u32, output_threshold: u8) {
        self.stream_buffer.flush();
        self.discard_decoder_backlog();
        self.boundary.stream_rate_hint = rate_hint;
        self.boundary.next_replay_gain = replay_gain;
        self.boundary.output_threshold = output_threshold;
        if let Some(marker) = self.boundary.marker.as_mut() {
            marker.replay_gain = replay_gain;
            marker.output_threshold_frames =
                (u64::from(output_threshold) * u64::from(marker.sample_rate) / 10) as u32;
        }
    }

    /// A codec was opened for a new stream (C `codec_open` sets `decode.new_stream`,
    /// `decode.c:287`): its first decoded frames mark the next track start.
    pub fn open_stream(&mut self) {
        self.boundary.new_stream = true;
    }

    /// Drops decoded PCM not yet in the output ring and the decoder's own backlog (C has no
    /// such buffer: its decoders write straight into `outputbuf`, so this is audio C would
    /// not have decoded yet when the decoder stops).
    pub fn discard_decoder_backlog(&mut self) {
        self.decoder.flush();
        self.pcm_pending_offset = 0;
        self.pcm_pending_samples = 0;
    }

    /// `strm f` (C `output_flush_streaming`, output.c:461-470): drops everything written
    /// after the pending track start (C `writep = track_start`) and clears it. Returns
    /// whether there was one (C "flushed"). A track already playing is never touched. A fade
    /// due in the dropped audio is dropped too ([`FadeEngine::discard_from`]: declared
    /// divergence, C leaves it armed).
    pub fn flush_streaming(&mut self) -> bool {
        match self.boundary.marker.take() {
            Some(marker) => {
                let keep = usize::try_from(marker.bytes_ahead).unwrap_or(usize::MAX);
                self.output_buffer.truncate_unread(keep);
                self.fade.discard_from(marker.bytes_ahead / OUTPUT_FRAME_BYTES as u64);
                true
            }
            None => false,
        }
    }

    /// Consumes `frames` played frames from the output ring, keeping the boundary distance.
    fn consume_output(&mut self, frames: usize) {
        let bytes = frames * OUTPUT_FRAME_BYTES;
        self.output_buffer.consume(bytes);
        if let Some(marker) = self.boundary.marker.as_mut() {
            marker.bytes_ahead = marker.bytes_ahead.saturating_sub(bytes as u64);
        }
        self.fade.advance(frames as u64);
    }

    /// `strm a` (C `OUTPUT_SKIP_FRAMES`, output.c:68-82): drops up to `frames` output frames
    /// unplayed and counts them as played. Returns the frames skipped.
    ///
    /// Deliberate divergence: the skip stops at a pending track start, so the output still
    /// processes it (`STMs`, rate, gain). C advances `readp` past `track_start` and then never
    /// sees `track_start == readp` until the ring wraps around (output.c:72-79, 127).
    pub fn skip_output_frames(&mut self, frames: u64) -> u64 {
        let mut held = (self.output_buffer.available_read() / OUTPUT_FRAME_BYTES) as u64;
        if let Some(marker) = self.boundary.marker {
            held = held.min(marker.bytes_ahead / OUTPUT_FRAME_BYTES as u64);
        }
        let skip = frames.min(held);
        self.consume_output(skip as usize);
        self.frames_played = self.frames_played.wrapping_add(skip);
        skip
    }

    /// Moves the output read position back over up to `frames` already played frames (as far
    /// as the ring still holds them) so they play again; a pending boundary moves away by the
    /// same amount, so it stays on the track start. Returns the frames rewound.
    pub fn rewind_output_frames(&mut self, frames: usize) -> usize {
        let before = self.output_buffer.available_read();
        self.output_buffer.rewind_read(frames.saturating_mul(OUTPUT_FRAME_BYTES));
        let mut rewound = self.output_buffer.available_read() - before;
        // Whole frames only: the ring may clamp to a byte count that splits a frame.
        let partial = rewound % OUTPUT_FRAME_BYTES;
        if partial != 0 {
            self.output_buffer.consume(partial);
            rewound -= partial;
        }
        if let Some(marker) = self.boundary.marker.as_mut() {
            marker.bytes_ahead += rewound as u64;
        }
        self.fade.retreat((rewound / OUTPUT_FRAME_BYTES) as u64);
        rewound / OUTPUT_FRAME_BYTES
    }

    /// Records the volume LMS asked for (C `set_volume`'s arguments: the `audg` gains, or
    /// unity when `digital_atten` is 0, slimproto.c:569), whatever attenuates it: the
    /// amplifier or [`Self::set_volume`].
    pub fn set_requested_volume(&mut self, left_gain: u32, right_gain: u32) {
        self.requested_volume = (left_gain, right_gain);
    }

    /// The volume LMS last asked for, Q16.16 per channel, independent of whether the
    /// amplifier or the software gain stage attenuates. C hands it to `equalizer_set_volume`
    /// (output_embedded.c:126) so the loudness contour tracks the volume: Task 16 hooks the
    /// loudness here.
    #[must_use]
    pub const fn requested_volume_q16(&self) -> (u32, u32) {
        self.requested_volume
    }

    /// Sets the software volume, Q16.16 per channel (C `output.gainL` / `gainR`, set by
    /// `set_volume` when no hardware volume handles it, output_embedded.c:118-126). It applies
    /// from the next output chunk on, also to audio already in the output ring, as in C.
    pub fn set_volume(&mut self, left_gain: u32, right_gain: u32) {
        self.gain_stage.volume_l_q16 = left_gain;
        self.gain_stage.volume_r_q16 = right_gain;
    }

    /// Sets the active audio decoder. A replaced MP3 decoder is parked in [`Self::mp3_spare`]
    /// (when the slot is free) for the next MP3 track.
    pub fn set_decoder(&mut self, decoder: ActiveDecoder) {
        if let ActiveDecoder::Mp3(previous) = core::mem::replace(&mut self.decoder, decoder) {
            if self.mp3_spare.is_none() {
                self.mp3_spare = Some(previous);
            }
        }
    }

    /// Makes an MP3 decoder active, reset to the start of a stream: the current one if it is
    /// already MP3, else the parked one, else a fresh one (`std-codecs` builds). Returns `false`
    /// when this build has no MP3 decoder available (decoder left unchanged).
    pub fn use_mp3_decoder(&mut self) -> bool {
        if let ActiveDecoder::Mp3(current) = &mut self.decoder {
            current.flush();
            return true;
        }
        let decoder = match self.mp3_spare.take() {
            Some(parked) => {
                let mut parked = ActiveDecoder::Mp3(parked);
                parked.flush();
                parked
            }
            None => match ActiveDecoder::new_mp3() {
                Some(fresh) => fresh,
                None => return false,
            },
        };
        self.set_decoder(decoder);
        true
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
    ///
    /// A codec missing from this build (MP3/AAC/Vorbis without `std-codecs`) leaves the current
    /// decoder unchanged.
    pub fn select_format(&mut self, format_byte: u8) {
        let decoder = match format_byte {
            b'm' | b'M' => {
                self.use_mp3_decoder();
                return;
            }
            b'f' | b'F' => Some(ActiveDecoder::new_flac()),
            b'a' | b'A' => ActiveDecoder::new_aac(),
            b'o' | b'O' => ActiveDecoder::new_vorbis(),
            _ => Some(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true))),
        };
        if let Some(decoder) = decoder {
            self.set_decoder(decoder);
        }
    }

    /// Executes one step of the audio pipeline (alias for `process_decode_step`).
    #[inline]
    pub fn step_audio_pipeline(&mut self) -> Result<usize, DecoderError> {
        self.process_decode_step()
    }

    /// Sets sample rate on pipeline and output sink now. It is also the rate a new track
    /// whose decoder reports none starts at (external sources such as Bluetooth set it).
    pub fn set_sample_rate(&mut self, rate: u32) -> Result<(), S::Error> {
        self.sink.set_sample_rate(rate)?;
        self.sample_rate = rate;
        self.boundary.stream_rate_hint = rate;
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

    /// Flushes both stream and output ringbuffers, clears pending PCM, and resets the decoder
    /// state (`strm q`: C `decode_flush` + `output_flush`, output.c:440-458). The track
    /// boundary goes too: no pending track start, no rate-change silence, `delay_active`
    /// cleared (output.c:455), and the next decoded frames start a new track.
    pub fn flush(&mut self) {
        self.stream_buffer.flush();
        self.output_buffer.flush();
        self.decoder.flush();
        self.equalizer.reset_history();
        self.sink.set_mute(true);
        self.pcm_pending_offset = 0;
        self.pcm_pending_samples = 0;
        self.boundary.marker = None;
        self.boundary.crossed = None;
        self.boundary.silence_frames = 0;
        self.boundary.delay_active = false;
        self.boundary.new_stream = true;
        // C parity: output.c:448 `output.fade = FADE_INACTIVE`.
        self.fade.reset();
    }
}


/// C `rate * delay / 2000`: half of a `delay` ms rate-change silence, in frames.
fn delay_frames(rate: u32, delay_ms: u32) -> u32 {
    u32::try_from(u64::from(rate) * u64::from(delay_ms) / 2000).unwrap_or(u32::MAX)
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

        // Task 12 (C parity): the decoder's rate is the next track's rate (C
        // `output.next_sample_rate`, pcm.c:231-237); the clock switches when playback reaches the
        // track start (output.c:154), not when the frames are decoded.
        assert_eq!(pipeline.track_marker().map(|m| m.sample_rate), Some(48000));
        assert_eq!(pipeline.sink.sample_rate, 44100);
        pipeline.process_output_step().unwrap();

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

    // ---------------------------------------------------------------------------------------
    // Task 12: gapless track boundary (C `output.track_start`, output.c:126-166)
    // ---------------------------------------------------------------------------------------

    /// Sink recording every sample plus the clock rate each write was made at.
    struct RecSink {
        samples: std::vec::Vec<i32>,
        /// (rate at the time of the write, frames written)
        writes: std::vec::Vec<(u32, usize)>,
        rate: u32,
        rate_changes: std::vec::Vec<u32>,
    }

    impl RecSink {
        fn new() -> Self {
            Self {
                samples: std::vec::Vec::new(),
                writes: std::vec::Vec::new(),
                rate: 44_100,
                rate_changes: std::vec::Vec::new(),
            }
        }
    }

    impl AudioOutputSink for RecSink {
        type Error = OutputError;

        fn write_frames(&mut self, frames: &[i32]) -> Result<usize, Self::Error> {
            self.samples.extend_from_slice(frames);
            self.writes.push((self.rate, frames.len() / 2));
            Ok(frames.len() / 2)
        }

        fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), Self::Error> {
            self.rate = rate_hz;
            self.rate_changes.push(rate_hz);
            Ok(())
        }

        fn set_mute(&mut self, _muted: bool) {}
    }

    /// 16-bit big-endian stereo PCM: frame `i` of `track` is (track * 1000 + i, -(track * 1000 + i)).
    fn track_pcm(track: i16, frames: usize) -> std::vec::Vec<u8> {
        let mut out = std::vec::Vec::new();
        for i in 0..frames as i16 {
            let v = track * 1000 + i;
            out.extend_from_slice(&v.to_be_bytes());
            out.extend_from_slice(&(-v).to_be_bytes());
        }
        out
    }

    /// The left samples a track_pcm() stream decodes to.
    fn track_left(track: i16, frames: usize) -> std::vec::Vec<i32> {
        (0..frames as i16).map(|i| i32::from(track * 1000 + i) << 16).collect()
    }

    fn left(samples: &[i32]) -> std::vec::Vec<i32> {
        samples.iter().step_by(2).copied().collect()
    }

    fn rec_pipeline<'a>(stream: &'a mut [u8], output: &'a mut [u8]) -> StreamPipeline<'a, RecSink> {
        StreamPipeline::new(stream, output, ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)), RecSink::new(), 44_100)
    }

    /// Ingests `bytes` and decodes until the stream ring is empty.
    fn feed<S: AudioOutputSink>(p: &mut StreamPipeline<'_, S>, bytes: &[u8]) {
        assert_eq!(p.ingest_stream_data(bytes).unwrap(), bytes.len());
        while p.stream_buffer_available() > 0 || p.has_pending_pcm() {
            p.process_decode_step().unwrap();
        }
    }

    /// `strm s` + codec open for a PCM track, as the coordinator does it.
    fn next_stream<S: AudioOutputSink>(p: &mut StreamPipeline<'_, S>, rate: u32, replay_gain: u32) {
        p.begin_stream(rate, replay_gain, 0);
        p.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
        p.open_stream();
    }

    fn drain_output<S: AudioOutputSink>(p: &mut StreamPipeline<'_, S>) -> std::vec::Vec<Option<TrackMarker>> {
        let mut crossings = std::vec::Vec::new();
        for _ in 0..10_000 {
            let written = p.process_output_step().unwrap_or(0);
            crossings.push(p.active_marker_crossed());
            if written == 0 && p.output_buffer_available() < 8 {
                break;
            }
        }
        crossings
    }

    #[test]
    fn test_first_frames_of_new_stream_set_boundary_at_write_position() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        // C decode_init: new_stream = true, so the very first frames mark a track start.
        feed(&mut p, &track_pcm(1, 100));
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(0));
        // Play 40 frames of track 1 (crossing its boundary), then the next track arrives.
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        let played = p.frames_played as usize;
        next_stream(&mut p, 44_100, 0);
        // strm s keeps the output ring (C slimproto.c:436-489 never flushes outputbuf).
        assert_eq!(p.output_buffer_available(), (100 - played) * 8);
        assert!(p.track_marker().is_none(), "no boundary until the new decoder writes");
        feed(&mut p, &track_pcm(2, 50));
        let marker = p.track_marker().copied().unwrap();
        assert_eq!(marker.bytes_ahead, ((100 - played) * 8) as u64);
        assert_eq!(marker.sample_rate, 44_100);
    }

    #[test]
    fn test_output_is_clipped_at_boundary_and_crossing_resets_frames_played() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 70)); // 70 frames: one 64-frame step, then 6 frames up to the boundary
        assert_eq!(p.process_output_step().unwrap(), 64);
        assert!(p.active_marker_crossed().is_some());
        next_stream(&mut p, 44_100, 0);
        feed(&mut p, &track_pcm(2, 10));
        // The chunk stops exactly at the boundary (C output.c:163-166).
        assert_eq!(p.process_output_step().unwrap(), 6);
        assert!(p.active_marker_crossed().is_none());
        assert_eq!(p.frames_played, 70);
        // The next step crosses first (frames_played = 0, C output.c:151), then plays track 2.
        assert_eq!(p.process_output_step().unwrap(), 10);
        assert!(p.active_marker_crossed().is_some());
        assert_eq!(p.frames_played, 10);
        let mut expected = track_left(1, 70);
        expected.extend(track_left(2, 10));
        assert_eq!(left(&p.sink.samples), expected);
    }

    #[test]
    fn test_flush_clears_markers() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 20));
        assert!(p.track_marker().is_some());
        p.flush();
        assert!(p.track_marker().is_none());
        assert!(p.active_marker_crossed().is_none());
        assert_eq!(p.output_buffer_available(), 0);
        // After a full flush the next decoded frames start a new track again.
        feed(&mut p, &track_pcm(2, 4));
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(0));
    }

    #[test]
    fn test_rate_change_at_boundary_with_rate_delay_inserts_split_silence() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.boundary.rate_delay_ms = 20; // C `-r 44100-48000:20`
        feed(&mut p, &track_pcm(1, 30));
        drain_output(&mut p);
        next_stream(&mut p, 48_000, 0);
        feed(&mut p, &track_pcm(2, 30));
        assert_eq!(p.track_marker().map(|m| m.sample_rate), Some(48_000));
        let crossings = drain_output(&mut p);
        assert_eq!(crossings.iter().filter(|c| c.is_some()).count(), 1);
        // C output.c:138-148: current_rate * delay / 2000 silent frames at the old rate, the
        // track start (rate switch), then next_rate * delay / 2000 at the new rate.
        let first_half = 44_100 * 20 / 2000;
        let second_half = 48_000 * 20 / 2000;
        let l = left(&p.sink.samples);
        assert_eq!(&l[..30], &track_left(1, 30)[..]);
        assert!(l[30..30 + first_half + second_half].iter().all(|&v| v == 0));
        assert_eq!(&l[30 + first_half + second_half..], &track_left(2, 30)[..]);
        let frames_at = |rate: u32| -> usize {
            p.sink.writes.iter().filter(|(r, _)| *r == rate).map(|(_, n)| n).sum()
        };
        assert_eq!(frames_at(44_100), 30 + first_half);
        assert_eq!(frames_at(48_000), second_half + 30);
        assert_eq!(p.sink.rate_changes, [48_000]);
        assert_eq!(p.sample_rate, 48_000);
        // Silence is not played audio (C output.c:277-280 counts only real frames).
        assert_eq!(p.frames_played, 30);
    }

    #[test]
    fn test_rate_change_without_rate_delay_switches_at_boundary_without_silence() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 30));
        p.process_output_step().unwrap();
        next_stream(&mut p, 48_000, 0);
        feed(&mut p, &track_pcm(2, 30));
        assert_eq!(p.sample_rate, 44_100, "decode must not switch the clock early");
        drain_output(&mut p);
        let mut expected = track_left(1, 30);
        expected.extend(track_left(2, 30));
        assert_eq!(left(&p.sink.samples), expected);
        assert_eq!(p.sink.writes.iter().filter(|(r, _)| *r == 44_100).map(|(_, n)| n).sum::<usize>(), 30);
        assert_eq!(p.sample_rate, 48_000);
    }

    #[test]
    fn test_replay_gain_switches_at_the_boundary() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.begin_stream(44_100, 0x1_0000, 0);
        p.open_stream();
        feed(&mut p, &track_pcm(1, 10));
        p.process_output_step().unwrap();
        assert_eq!(p.boundary.current_replay_gain, 0x1_0000);
        next_stream(&mut p, 44_100, 0x8000);
        feed(&mut p, &track_pcm(2, 10));
        // C slimproto.c:485 stores next_replay_gain at strm s; output.c:158-160 applies it at the
        // boundary. Until then the playing track keeps its gain.
        assert_eq!(p.boundary.next_replay_gain, 0x8000);
        assert_eq!(p.boundary.current_replay_gain, 0x1_0000);
        drain_output(&mut p);
        assert_eq!(p.boundary.current_replay_gain, 0x8000);
    }

    #[test]
    fn test_flush_streaming_discards_only_the_next_track() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 100));
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        next_stream(&mut p, 44_100, 0);
        feed(&mut p, &track_pcm(2, 50));
        // C output_flush_streaming (output.c:461-470): writep = track_start, flushed.
        assert!(p.flush_streaming());
        assert!(p.track_marker().is_none());
        assert_eq!(p.output_buffer_available(), 36 * 8);
        // Nothing left to flush: not "flushed".
        assert!(!p.flush_streaming());
        let crossings = drain_output(&mut p);
        assert!(crossings.iter().all(Option::is_none));
        assert_eq!(left(&p.sink.samples), track_left(1, 100));
    }

    #[test]
    fn test_flush_streaming_with_no_boundary_keeps_the_playing_track() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 100));
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        assert!(!p.flush_streaming());
        assert_eq!(p.output_buffer_available(), 36 * 8);
    }

    #[test]
    fn test_second_stream_before_boundary_overwrites_it() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 100));
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        next_stream(&mut p, 44_100, 0);
        feed(&mut p, &track_pcm(2, 20));
        let first = p.track_marker().copied().unwrap();
        next_stream(&mut p, 44_100, 0);
        feed(&mut p, &track_pcm(3, 20));
        // One slot (C output.track_start): the newer track start replaces the unreached one.
        let second = p.track_marker().copied().unwrap();
        assert_eq!(second.bytes_ahead, first.bytes_ahead + 20 * 8);
        let crossings = drain_output(&mut p);
        assert_eq!(crossings.iter().filter(|c| c.is_some()).count(), 1);
        // strm s never flushes outputbuf: track 2's decoded part still plays (C), then track 3.
        let mut expected = track_left(1, 100);
        expected.extend(track_left(2, 20));
        expected.extend(track_left(3, 20));
        assert_eq!(left(&p.sink.samples), expected);
        assert_eq!(p.frames_played, 20);
    }

    #[test]
    fn test_set_track_marker_is_single_slot_and_clamped_to_written_frames() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.boundary.new_stream = false;
        feed(&mut p, &track_pcm(1, 10));
        assert!(p.track_marker().is_none());
        let m = |bytes_ahead| TrackMarker { bytes_ahead, sample_rate: 48_000, replay_gain: 0, output_threshold_frames: 0 };
        assert_eq!(p.set_track_marker(m(40)), None);
        // A second one replaces the first (C overwrites output.track_start).
        assert_eq!(p.set_track_marker(m(1_000)), Some(m(40)));
        // Never past the written frames, always on a frame boundary.
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(80));
        p.set_track_marker(m(13));
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(8));
    }

    #[test]
    fn test_skip_and_rewind_keep_the_boundary_on_the_track_start() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 8192]);
        let mut p = rec_pipeline(&mut s, &mut o);
        feed(&mut p, &track_pcm(1, 100));
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        next_stream(&mut p, 44_100, 0);
        feed(&mut p, &track_pcm(2, 10));
        // Rewinding replays 16 already-played frames: the boundary moves 16 frames further away.
        assert_eq!(p.rewind_output_frames(16), 16);
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some((36 + 16) * 8));
        // A skip stops at the boundary, so the track start is still processed by the output.
        assert_eq!(p.skip_output_frames(1_000), 52);
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(0));
        assert_eq!(p.process_output_step().unwrap(), 10);
        assert!(p.active_marker_crossed().is_some());
        assert_eq!(&left(&p.sink.samples)[64..], &track_left(2, 10)[..]);
    }

    // ---------------------------------------------------------------------------------------
    // Task 13: fades and crossfade (C output.c:174-252, :288-345)
    // ---------------------------------------------------------------------------------------

    use squeezelite_audio_core::fade::{gain, to_gain, FadeDir, FadeState, FIXED_ONE};

    /// Both samples of frames `range` of a `track_pcm(track, ..)` stream, each scaled by `g`.
    fn scaled(track: i16, range: core::ops::Range<usize>, g: i32) -> std::vec::Vec<i32> {
        range
            .flat_map(|i| {
                let v = i32::from(track * 1000 + i as i16);
                [gain(g, v << 16), gain(g, -v << 16)]
            })
            .collect()
    }

    /// `strm s` with a transition. Tests run at a 100 Hz "rate", so one second is 100 frames.
    fn next_stream_with(p: &mut StreamPipeline<'_, RecSink>, rate: u32, rg: u32, mode: FadeMode, secs: u32, invert: bool) {
        next_stream(p, rate, rg);
        p.set_transition(mode, secs, invert, ChannelMode::Stereo);
    }

    fn write_sizes(p: &StreamPipeline<'_, RecSink>) -> std::vec::Vec<usize> {
        p.sink.writes.iter().map(|(_, n)| *n).collect()
    }

    #[test]
    fn test_inout_on_empty_buffer_fades_up() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        // INOUT, 2 s at 100 Hz: 100 frames each way (output.c:292-296). The buffer is empty,
        // so the first frames fade up from the write position (output.c:298-309).
        next_stream_with(&mut p, 100, 0, FadeMode::InOut, 2, false);
        feed(&mut p, &track_pcm(1, 300));
        assert_eq!((p.fade.state(), p.fade.dir(), p.fade.duration_frames()), (FadeState::Due, FadeDir::Up, 100));
        drain_output(&mut p);
        let mut expected = scaled(1, 0..64, 0);
        expected.extend(scaled(1, 64..100, to_gain(0.64)));
        expected.extend(scaled(1, 100..300, FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(write_sizes(&p), [64, 36, 64, 64, 64, 8]);
        assert_eq!(p.fade.state(), FadeState::Inactive);
    }

    #[test]
    fn test_inout_at_track_end_fades_down_then_up() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        next_stream_with(&mut p, 100, 0, FadeMode::InOut, 2, false);
        feed(&mut p, &track_pcm(1, 300));
        assert_eq!(p.process_output_step().unwrap(), 64);
        assert_eq!(p.process_output_step().unwrap(), 36);
        assert!(p.active_marker_crossed().is_some(), "track 1's own start");
        // Decode complete (C decode.c:103 `_checkfade(false)`): fade down over the last
        // 100 frames before the write position (output.c:311-321).
        p.fade_track_end();
        assert_eq!((p.fade.state(), p.fade.dir(), p.fade.start_ahead()), (FadeState::Due, FadeDir::Down, 100));
        // The next track: INOUT on a non-empty buffer arms nothing at its start.
        next_stream_with(&mut p, 100, 0, FadeMode::InOut, 2, false);
        feed(&mut p, &track_pcm(2, 300));
        assert_eq!(p.fade.dir(), FadeDir::Down);
        let crossings = drain_output(&mut p);
        assert_eq!(crossings.iter().filter(|c| c.is_some()).count(), 1);
        let mut expected = scaled(1, 0..64, 0);
        expected.extend(scaled(1, 64..100, to_gain(0.64)));
        expected.extend(scaled(1, 100..200, FIXED_ONE));
        expected.extend(scaled(1, 200..264, FIXED_ONE));
        expected.extend(scaled(1, 264..300, to_gain(0.36)));
        // Down complete at the track start: up over the same duration (output.c:186-193).
        expected.extend(scaled(2, 0..64, 0));
        expected.extend(scaled(2, 64..100, to_gain(0.64)));
        expected.extend(scaled(2, 100..300, FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(write_sizes(&p), [64, 36, 64, 36, 64, 36, 64, 36, 64, 64, 64, 8]);
    }

    /// Track 1 (300 frames, crossed after one 64-frame step), then track 2 with `t2_frames`
    /// and replay gain 0x8000, at `rate1` / `rate2`, both with a 1 s crossfade.
    fn crossfade_setup<'a>(
        s: &'a mut [u8],
        o: &'a mut [u8],
        rate1: u32,
        rate2: u32,
        t2_frames: usize,
    ) -> StreamPipeline<'a, RecSink> {
        let mut p = rec_pipeline(s, o);
        next_stream_with(&mut p, rate1, 0, FadeMode::Cross, 1, false);
        feed(&mut p, &track_pcm(1, 300));
        assert_eq!(p.fade.state(), FadeState::Inactive, "nothing to crossfade from");
        assert_eq!(p.process_output_step().unwrap(), 64);
        assert!(p.active_marker_crossed().is_some());
        next_stream_with(&mut p, rate2, 0x8000, FadeMode::Cross, 1, false);
        feed(&mut p, &track_pcm(2, t2_frames));
        p
    }

    #[test]
    fn test_crossfade_skips_dur_of_new_track_on_completion() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = crossfade_setup(&mut s, &mut o, 100, 100, 400);
        // C output.c:328-339: 100 frames over the old track's last 236; the track start moves
        // to the fade start.
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(136 * 8));
        assert_eq!((p.fade.state(), p.fade.dir(), p.fade.duration_frames()), (FadeState::Due, FadeDir::Cross, 100));
        let mut crossings = std::vec::Vec::new();
        for _ in 0..100 {
            let n = p.process_output_step().unwrap();
            if p.active_marker_crossed().is_some() {
                crossings.push(p.sink.samples.len() / 2 - n);
                // The old track keeps its replay gain into the fade (output.c:157).
                assert_eq!(p.boundary.current_replay_gain, 0);
            }
            if n == 0 {
                break;
            }
        }
        assert_eq!(crossings, [200], "STMs at the fade start");
        // Old tail plain up to the fade start, then old x (1 - in) + new x in x next replay
        // gain, per chunk (output.c:231-246), then the new track from dur_f on.
        let mut expected = scaled(1, 0..200, FIXED_ONE);
        for (start, len) in [(0usize, 64usize), (64, 36)] {
            let gin = to_gain(start as f32 / 100.0);
            let (gout, gin) = (FIXED_ONE - gin, gain(gin, 0x8000));
            let old = scaled(1, 200 + start..200 + start + len, FIXED_ONE);
            let new = scaled(2, start..start + len, FIXED_ONE);
            expected.extend(old.iter().zip(&new).map(|(&a, &b)| gain(gout, a).saturating_add(gain(gin, b))));
        }
        // Task 14: after the crossfade the new track's replay gain applies to all its output
        // (C output.c:54-55), continuous with its weight inside the mix.
        expected.extend(scaled(2, 100..400, 0x8000));
        assert_eq!(p.sink.samples.len(), expected.len(), "300 + 400 - 100 frames: none missing, none twice");
        assert_eq!(p.sink.samples, expected);
        // The first chunk is all old track (in = 0).
        assert_eq!(&p.sink.samples[400..528], &scaled(1, 200..264, FIXED_ONE)[..]);
        // Completion: the new replay gain is current (output.c:203); the skipped frames are not
        // counted as played (output.c:199).
        assert_eq!(p.boundary.current_replay_gain, 0x8000);
        assert_eq!(p.frames_played, 400);
        assert_eq!(p.output_buffer_available(), 0);
    }

    #[test]
    fn test_crossfade_aborts_when_buffer_is_too_short() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = crossfade_setup(&mut s, &mut o, 100, 100, 50);
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(136 * 8));
        let crossings = drain_output(&mut p);
        assert_eq!(crossings.iter().filter(|c| c.is_some()).count(), 1);
        // used (150) <= dur_f + size (164): no mix, no skip (output.c:231, 247-249).
        let mut expected = scaled(1, 0..300, FIXED_ONE);
        expected.extend(scaled(2, 0..50, FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(p.fade.state(), FadeState::Inactive);
        // C parity: an aborted crossfade never switches the replay gain.
        assert_eq!(p.boundary.current_replay_gain, 0);
    }

    #[test]
    fn test_crossfade_at_different_rates_does_not_arm() {
        // Re-pinned to C (output.c:325-327): no fade at all, plain gapless.
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = crossfade_setup(&mut s, &mut o, 100, 200, 400);
        assert_eq!(p.fade.state(), FadeState::Inactive);
        assert_eq!(p.track_marker().map(|m| m.bytes_ahead), Some(236 * 8), "boundary unchanged");
        drain_output(&mut p);
        let mut expected = scaled(1, 0..300, FIXED_ONE);
        // Task 14: track 2's replay gain 0x8000 applies from its start (C output.c:54-55, 158).
        expected.extend(scaled(2, 0..400, 0x8000));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(p.sink.rate_changes, [100, 200]);
        assert_eq!(p.boundary.current_replay_gain, 0x8000);
    }

    #[test]
    fn test_fade_with_invert_negates() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        // strm s flags & 0x03 == 0x03 (slimproto.c:488): the standard gain is negated
        // (output.c:57) and the fade gain multiplies it (output.c:224-226).
        next_stream_with(&mut p, 100, 0, FadeMode::In, 1, true);
        feed(&mut p, &track_pcm(1, 200));
        drain_output(&mut p);
        let mut expected = scaled(1, 0..64, 0);
        expected.extend(scaled(1, 64..100, -to_gain(0.64)));
        expected.extend(scaled(1, 100..200, -FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(p.sink.samples[200 * 2 - 2], -(i32::from(1199i16) << 16));
    }

    #[test]
    fn test_fade_boundary_lands_exactly_on_chunk_boundaries() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        next_stream(&mut p, 100, 0);
        feed(&mut p, &track_pcm(1, 300));
        p.process_output_step().unwrap();
        // Fade in of the next track, due 236 frames ahead (its track start).
        next_stream_with(&mut p, 100, 0, FadeMode::In, 1, false);
        feed(&mut p, &track_pcm(2, 300));
        assert_eq!(p.fade.start_ahead(), 236);
        drain_output(&mut p);
        // Chunks end on the fade start (output.c:178-180) and the fade end (:212-214).
        assert_eq!(write_sizes(&p), [64, 64, 64, 64, 44, 64, 36, 64, 64, 64, 8]);
        let mut expected = scaled(1, 0..300, FIXED_ONE);
        expected.extend(scaled(2, 0..64, 0));
        expected.extend(scaled(2, 64..100, to_gain(0.64)));
        expected.extend(scaled(2, 100..300, FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
    }

    #[test]
    fn test_flushes_clear_fades() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        next_stream(&mut p, 100, 0);
        feed(&mut p, &track_pcm(1, 300));
        p.process_output_step().unwrap();
        next_stream_with(&mut p, 100, 0, FadeMode::Cross, 1, false);
        feed(&mut p, &track_pcm(2, 300));
        assert_eq!(p.fade.state(), FadeState::Due);
        // strm f drops the next track and the old tail from the fade start (C writep =
        // track_start = fade_start); the crossfade over dropped audio goes too.
        assert!(p.flush_streaming());
        assert_eq!(p.output_buffer_available(), 136 * 8);
        assert_eq!(p.fade.state(), FadeState::Inactive);
        drain_output(&mut p);
        assert_eq!(p.sink.samples, scaled(1, 0..200, FIXED_ONE));
        // strm q (C output_flush, output.c:448).
        next_stream_with(&mut p, 100, 0, FadeMode::In, 1, false);
        feed(&mut p, &track_pcm(3, 10));
        assert_eq!(p.fade.state(), FadeState::Due);
        p.flush();
        assert_eq!(p.fade.state(), FadeState::Inactive);
    }

    // ---- Task 14: composite gain stage ------------------------------------------------------

    /// 16-bit big-endian stereo PCM of a sine: `amp * sin(2 pi i / period)` on both channels.
    fn sine_pcm(frames: usize, period: usize, amp: f64) -> std::vec::Vec<u8> {
        let mut out = std::vec::Vec::new();
        for i in 0..frames {
            let v = (amp * (2.0 * core::f64::consts::PI * i as f64 / period as f64).sin()).round() as i16;
            out.extend_from_slice(&v.to_be_bytes());
            out.extend_from_slice(&v.to_be_bytes());
        }
        out
    }

    #[test]
    fn test_volume_applies_at_output_to_buffered_audio() {
        // C applies output.gainL/R when frames are played (output.c:54, output_i2s.c:510), so
        // a volume change reaches audio already in the output ring.
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        next_stream(&mut p, 100, 0);
        feed(&mut p, &track_pcm(1, 200));
        p.set_volume(0x8000, 0x4000);
        drain_output(&mut p);
        let expected: std::vec::Vec<i32> = (0..200)
            .flat_map(|i| {
                let v = i32::from(1000 + i as i16) << 16;
                [gain(0x8000, v), gain(0x4000, -v)]
            })
            .collect();
        assert_eq!(p.sink.samples, expected);
    }

    #[test]
    fn test_replay_gain_applies_to_all_output_composed_with_volume() {
        // C output.c:54-55: gainL = gain(output.gainL, current_replay_gain), one composed
        // gain per channel; the brief's fixture: volume 0.5 x replay gain 2.0 = unity.
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.set_volume(0x8000, 0x8000);
        next_stream(&mut p, 100, 0x2_0000);
        feed(&mut p, &track_pcm(1, 100));
        drain_output(&mut p);
        assert_eq!(p.sink.samples, scaled(1, 0..100, FIXED_ONE));
        // Replay gain 0 means none (output.c:54): the volume alone.
        next_stream(&mut p, 100, 0);
        feed(&mut p, &track_pcm(2, 100));
        p.sink.samples.clear();
        drain_output(&mut p);
        assert_eq!(p.sink.samples, scaled(2, 0..100, 0x8000));
        // An odd composition: gain(gain(vol, rg), s), truncating at each step as C.
        next_stream(&mut p, 100, 0x1_6A09);
        feed(&mut p, &track_pcm(3, 100));
        p.sink.samples.clear();
        drain_output(&mut p);
        assert_eq!(p.sink.samples, scaled(3, 0..100, gain(0x8000, 0x1_6A09)));
    }

    #[test]
    fn test_gain_change_during_fade_preserves_fade_progress() {
        // Review Focus #3. Fade in over 128 frames (64 Hz, 2 s); the first 64-frame chunk
        // leaves the fade at 50 %.
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        next_stream_with(&mut p, 64, 0, FadeMode::In, 2, false);
        feed(&mut p, &track_pcm(1, 300));
        assert_eq!(p.process_output_step().unwrap(), 64);
        assert_eq!((p.fade.state(), p.fade.duration_frames()), (FadeState::Active, 128));
        let fade_before = p.fade;
        // Volume change mid-fade: the fade's position, length and gain are untouched.
        p.set_volume(0x8000, 0x8000);
        assert_eq!(p.fade, fade_before);
        assert_eq!(p.process_output_step().unwrap(), 64);
        // The chunk reflects both: fade 50 % x volume 50 % (output.c:54, :226-228).
        assert_eq!(p.gain_stage.fade_q16, 0x8000);
        assert_eq!(&p.sink.samples[128..256], &scaled(1, 64..128, gain(0x8000, 0x8000))[..]);
        // Another change at the last fade frame changes nothing in the fade either; it ends on
        // schedule (no restart, no jump to unity).
        p.set_volume(0x1_0000, 0x1_0000);
        drain_output(&mut p);
        assert_eq!(p.fade.state(), FadeState::Inactive);
        assert_eq!(write_sizes(&p), [64, 64, 64, 64, 44]);
        let mut expected = scaled(1, 0..64, 0);
        expected.extend(scaled(1, 64..128, gain(0x8000, 0x8000)));
        expected.extend(scaled(1, 128..300, FIXED_ONE));
        assert_eq!(p.sink.samples, expected);
    }

    #[test]
    fn test_invert_with_volume_replay_gain_and_fade_negates_once() {
        // C output.c:54-57 then :224-226; the inversion is applied exactly once under the fade
        // (declared divergence from C's second negation, ruled in Task 13).
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.set_volume(0x8000, 0x8000);
        next_stream_with(&mut p, 100, 0x2_0000, FadeMode::In, 1, true);
        feed(&mut p, &track_pcm(1, 200));
        drain_output(&mut p);
        let standard = -gain(0x8000, 0x2_0000);
        let mut expected = scaled(1, 0..64, gain(standard, 0));
        expected.extend(scaled(1, 64..100, gain(standard, to_gain(0.64))));
        expected.extend(scaled(1, 100..200, standard));
        assert_eq!(p.sink.samples, expected);
        // Inverted before, during and after the fade: left samples (positive in the source)
        // are never positive.
        assert!(left(&p.sink.samples).iter().all(|&v| v <= 0));
        assert!(left(&p.sink.samples[128..]).iter().all(|&v| v < 0));
    }

    #[test]
    fn test_channel_select_applies_to_output() {
        // C slimproto.c:489 output.channels, output_pack.c:383-410.
        let expect = |mode: ChannelMode, l: i32, r: i32| -> (i32, i32) {
            let (l, r) = (gain(0x8000, l), gain(0x4000, r));
            match mode {
                ChannelMode::LeftOnly => (l, l),
                ChannelMode::RightOnly => (r, r),
                ChannelMode::MonoMix => ((l + r) / 2, (l + r) / 2),
                ChannelMode::Stereo => (l, r),
            }
        };
        for mode in [ChannelMode::LeftOnly, ChannelMode::RightOnly, ChannelMode::MonoMix, ChannelMode::Stereo] {
            let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
            let mut p = rec_pipeline(&mut s, &mut o);
            p.set_volume(0x8000, 0x4000);
            next_stream(&mut p, 100, 0);
            p.set_transition(FadeMode::None, 0, false, mode);
            feed(&mut p, &track_pcm(1, 100));
            drain_output(&mut p);
            let expected: std::vec::Vec<i32> = (0..100)
                .flat_map(|i| {
                    let v = i32::from(1000 + i as i16) << 16;
                    let (l, r) = expect(mode, v, -v);
                    [l, r]
                })
                .collect();
            assert_eq!(p.sink.samples, expected, "{mode:?}");
        }
    }

    /// Track 1 (300 frames, replay gain `rg1`) crossed after one 64-frame step, then track 2
    /// (400 frames, `rg2`) with a 1 s crossfade at 100 Hz: the mix spans track 1's frames
    /// 200..300 and track 2's 0..100.
    fn crossfade_rg<'a>(s: &'a mut [u8], o: &'a mut [u8], rg1: u32, rg2: u32, t1: &[u8], t2: &[u8]) -> StreamPipeline<'a, RecSink> {
        let mut p = rec_pipeline(s, o);
        next_stream_with(&mut p, 100, rg1, FadeMode::Cross, 1, false);
        feed(&mut p, t1);
        assert_eq!(p.process_output_step().unwrap(), 64);
        assert!(p.active_marker_crossed().is_some());
        next_stream_with(&mut p, 100, rg2, FadeMode::Cross, 1, false);
        feed(&mut p, t2);
        assert_eq!((p.fade.state(), p.fade.dir()), (FadeState::Due, FadeDir::Cross));
        p
    }

    #[test]
    fn test_crossfade_applies_replay_gain_once() {
        // Inside a crossfade C's standard gain is the plain volume (output.c:243-245) and each
        // weight carries its track's replay gain (:234-239): replay gain is never doubled.
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = crossfade_rg(&mut s, &mut o, 0x8000, 0xC000, &track_pcm(1, 300), &track_pcm(2, 400));
        p.set_volume(0x8000, 0x8000);
        drain_output(&mut p);
        // Before the fade: volume x track 1's replay gain (the first 64 frames were played at
        // unity volume, before set_volume).
        let mut expected = scaled(1, 0..64, gain(FIXED_ONE, 0x8000));
        expected.extend(scaled(1, 64..200, gain(0x8000, 0x8000)));
        for (start, len) in [(0usize, 64usize), (64, 36)] {
            let gin = to_gain(start as f32 / 100.0);
            let (gout, gin) = (gain(FIXED_ONE - gin, 0x8000), gain(gin, 0xC000));
            let old = scaled(1, 200 + start..200 + start + len, FIXED_ONE);
            let new = scaled(2, start..start + len, FIXED_ONE);
            // Mix with the weights, then the volume alone.
            expected.extend(old.iter().zip(&new).map(|(&a, &b)| gain(0x8000, gain(gout, a).saturating_add(gain(gin, b)))));
        }
        expected.extend(scaled(2, 100..400, gain(0x8000, 0xC000)));
        assert_eq!(p.sink.samples, expected);
        assert_eq!(p.boundary.current_replay_gain, 0xC000);
        // The first mixed chunk (in = 0) equals the old track at volume x replay gain within
        // the truncation of the two-step product, not volume x replay gain squared.
        let first_mixed = &p.sink.samples[400..402];
        let v = i32::from(1200i16) << 16;
        assert!((first_mixed[0] - gain(gain(0x8000, 0x8000), v)).abs() <= 2, "{first_mixed:?}");
    }

    #[test]
    fn test_crossfade_edges_keep_constant_level_with_replay_gain() {
        // A constant-level sine through a crossfade between two tracks with the same replay
        // gain (0.5): the level must not step at either edge. The period (20 frames) divides
        // the fade start (200), so old and new samples are in phase inside the mix.
        const AMP: f64 = 20_000.0;
        let (t1, t2) = (sine_pcm(300, 20, AMP), sine_pcm(400, 20, AMP));
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = crossfade_rg(&mut s, &mut o, 0x8000, 0x8000, &t1, &t2);
        drain_output(&mut p);
        let out = left(&p.sink.samples);
        assert_eq!(out.len(), 600);
        // Every frame: half the source level (2^16 scale), to within the Q16 truncations.
        let ideal = |i: usize| 0.5 * 65536.0 * (AMP * (2.0 * core::f64::consts::PI * i as f64 / 20.0).sin()).round();
        let tolerance = 65536.0 * AMP * 4.0 / 65536.0 + 4.0;
        for (i, &v) in out.iter().enumerate() {
            assert!((f64::from(v) - ideal(i)).abs() <= tolerance, "frame {i}: {v} vs {}", ideal(i));
        }
        // Peak level just before each edge, inside and just after: equal.
        let peak = |r: core::ops::Range<usize>| out[r].iter().map(|v| v.unsigned_abs()).max().unwrap();
        let (before, inside, after) = (peak(180..200), peak(240..260), peak(300..320));
        let full = (AMP * 65536.0 * 0.5) as u32;
        for (name, level) in [("before", before), ("inside", inside), ("after", after)] {
            assert!(level.abs_diff(full) <= full / 1000, "{name}: {level} vs {full}");
        }
    }

    #[test]
    fn test_external_audio_clears_replay_gain_keeps_player_settings() {
        let (mut s, mut o) = ([0u8; 4096], [0u8; 16384]);
        let mut p = rec_pipeline(&mut s, &mut o);
        p.set_volume(0x8000, 0x8000);
        next_stream_with(&mut p, 100, 0x2_0000, FadeMode::In, 1, true);
        feed(&mut p, &track_pcm(1, 100));
        p.process_output_step().unwrap();
        assert!(p.active_marker_crossed().is_some());
        // The next LMS track is queued behind the playing one.
        next_stream(&mut p, 100, 0x3000);
        feed(&mut p, &track_pcm(2, 10));
        assert_eq!(p.boundary.current_replay_gain, 0x2_0000);
        assert_eq!(p.track_marker().map(|m| m.replay_gain), Some(0x3000));
        p.begin_external_audio();
        assert_eq!((p.boundary.current_replay_gain, p.boundary.next_replay_gain), (0, 0));
        assert_eq!(p.track_marker().map(|m| m.replay_gain), Some(0));
        assert_eq!((p.fade.mode(), p.fade.state()), (FadeMode::None, FadeState::Inactive));
        // Volume and polarity are player settings (C keeps output.gainL/R and invert).
        assert_eq!((p.gain_stage.volume_l_q16, p.gain_stage.invert_polarity), (0x8000, true));
    }
}
