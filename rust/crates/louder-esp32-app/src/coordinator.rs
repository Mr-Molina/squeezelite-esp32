//! Player Coordinator
//!
//! The single player engine shared by host and device. It orchestrates the SlimProto control
//! connection, the HTTP audio stream, the audio decoding pipeline, the amplifier/clock backend
//! and CYD display status updates.
//!
//! All I/O goes through the [`crate::io_traits`] seams: [`SlimTransport`] (control frames),
//! [`StreamSource`] (HTTP audio) and [`AudioBackend`] (I2S clock and amplifier). Platform
//! differences live in the trait implementations and in bring-up, never in this file (enforced by
//! `tests/no_cfg_invariant.rs`). [`PlayerCoordinator::tick`] is the one entry point that drives
//! the protocol; the CYD entry points ([`PlayerCoordinator::handle_cyd_command`],
//! [`PlayerCoordinator::sync_cyd_telemetry`], [`PlayerCoordinator::cyd_playback_state`]) are
//! independent of the network.

extern crate alloc;

use alloc::vec::Vec;

use squeezelite_audio_core::output::AudioOutputSink;
use squeezelite_audio_core::VolumeScale;
use squeezelite_cyd_link::{CydCommand, CydMessage, PlayState};
use squeezelite_decoders::{DecoderError, PcmDecoder};
use squeezelite_proto::{
    sample_rate_from_index, AudioFormat, AutostartMode, DisconnectReason, Endianness,
    ServerCommand, SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent, StreamCommandType,
    STRM_FLAG_SERVER_REBOOT,
};

use crate::amp::{lms_gain_to_hardware_volume_percent, lms_gain_to_tas5805m_vol_reg};
use crate::config_handle::ConfigHandle;
use crate::io_traits::{
    AudioBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
};
use crate::pipeline::{ActiveDecoder, StreamPipeline};

/// Size of the stack buffer [`PlayerCoordinator::tick`] receives one server frame into. A larger
/// frame is dropped by the transport (reported as [`TransportError::Io`] and skipped).
pub const SERVER_FRAME_BUF_LEN: usize = 2048;

/// HTTP request sent when a `strm s` carries no request headers (`COORD-503`).
const DEFAULT_HTTP_REQUEST: &[u8] = b"GET /stream.mp3 HTTP/1.0\r\n\r\n";

/// Free output-ring space [`PlayerCoordinator::pump_decode`] requires before decoding.
pub const PUMP_DECODE_MIN_OUTPUT_SPACE: usize = 1024;

/// Helper trait to construct `PcmDecoder` with standard configuration.
pub trait PcmDecoderExt {
    fn with_config(bits: u8, big_endian: bool, channels: u8) -> Self;
}

impl PcmDecoderExt for PcmDecoder {
    fn with_config(bits: u8, big_endian: bool, channels: u8) -> Self {
        PcmDecoder::new(bits, channels, big_endian)
    }
}

/// Playback operational state of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Disconnected,
    Connected,
    Buffering,
    Playing,
    Paused,
    Stopped,
}

impl PlayerState {
    #[allow(non_upper_case_globals)]
    pub const Streaming: PlayerState = PlayerState::Playing;
}

/// State of the coordinator's use of the [`StreamSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamLink {
    /// No stream (never started, stopped, flushed, or failed).
    Idle,
    /// `connect` returned [`StreamError::WouldBlock`]; it is repeated with the same arguments on
    /// every tick until it resolves.
    Connecting { addr: NetAddr, request: Vec<u8> },
    /// Connected; audio bytes are polled every tick.
    Open,
}

/// Orchestrates network streaming, audio decoding, and player telemetry.
pub struct PlayerCoordinator {
    pub state: PlayerState,
    pub mac: [u8; 6],
    pub model_name: &'static str,
    pub volume: VolumeScale,
    pub frames_played: u64,
    pub last_heartbeat_ms: u32,
    pub heartbeat_interval_ms: u32,
    pub current_format: Option<AudioFormat>,
    pub current_sample_rate: u32,
    pub buffer_threshold_bytes: usize,
    pub autostart: bool,
    pub autostart_mode: AutostartMode,
    pub awaiting_cont: bool,
    pub stream_bytes_received: u64,
    pub decode_complete_sent: bool,
    pub pending_decode_ready: bool,
    pub pending_track_started: bool,
    pub pending_underrun: bool,
    pub pending_dsco: Option<DisconnectReason>,
    /// `STMl` (buffer threshold / ready to start) waits for the next heartbeat.
    pub pending_buffer_threshold: bool,
    /// Response head of the audio stream, forwarded to LMS as `RESP` on the next heartbeat.
    pub pending_resp: Option<Vec<u8>>,
    /// ICY metadata block, forwarded to LMS as `META` on the next heartbeat.
    pub pending_meta: Option<Vec<u8>>,
    pub dac_enabled: bool,
    pub server_rebooting: bool,
    pub player_name: heapless::String<64>,
    pub stream_eof: bool,
    /// IPv4 address of the LMS server. A `strm s` whose `server_ip` is `0.0.0.0` streams from
    /// this address (bring-up sets it to the control connection's peer; defaults to loopback).
    pub lms_ip: [u8; 4],
    /// Decode and sink output are scheduled by the caller through [`Self::pump_decode`] /
    /// [`Self::pump_output`] (device: DMA-first pump phases); [`Self::tick`] then only
    /// evaluates the play state and never decodes or writes to the sink itself. `false`
    /// (default, host): `tick` runs one decode + output step.
    pub external_audio_pump: bool,
    /// Runtime configuration the player name is persisted through (spec §3.4: `setd` id-0
    /// writes persist the name). `None` (host default): the name only lives in this struct.
    pub config: Option<&'static ConfigHandle>,
    stream: StreamLink,
    pending_cyd: Option<CydMessage>,
    last_tick_frames: usize,
    /// `STMs` was reported (or is pending) for the current track.
    track_started_reported: bool,
    /// `STMl` was reported (or is pending) for the current track.
    buffer_threshold_reported: bool,
    /// Buffer server frames are received into (see [`Self::set_frame_buffer`]); `None` uses a
    /// [`SERVER_FRAME_BUF_LEN`]-byte stack buffer.
    frame_buffer: Option<&'static mut [u8]>,
}

impl PlayerCoordinator {
    /// Creates a new coordinator instance.
    pub fn new(mac: [u8; 6], model_name: &'static str) -> Self {
        let mut player_name = heapless::String::new();
        let safe_len = model_name.floor_char_boundary(64.min(model_name.len()));
        let _ = player_name.push_str(&model_name[..safe_len]);
        Self {
            state: PlayerState::Disconnected,
            mac,
            model_name,
            volume: VolumeScale::unity(),
            frames_played: 0,
            last_heartbeat_ms: 0,
            heartbeat_interval_ms: 1000,
            current_format: None,
            current_sample_rate: 44100,
            buffer_threshold_bytes: 0,
            autostart: true,
            autostart_mode: AutostartMode::Auto,
            awaiting_cont: false,
            stream_bytes_received: 0,
            decode_complete_sent: false,
            pending_decode_ready: false,
            pending_track_started: false,
            pending_underrun: false,
            pending_dsco: None,
            pending_buffer_threshold: false,
            pending_resp: None,
            pending_meta: None,
            dac_enabled: true,
            server_rebooting: false,
            player_name,
            stream_eof: false,
            lms_ip: [127, 0, 0, 1],
            external_audio_pump: false,
            config: None,
            stream: StreamLink::Idle,
            pending_cyd: None,
            last_tick_frames: 0,
            track_started_reported: false,
            buffer_threshold_reported: false,
            frame_buffer: None,
        }
    }

    /// Receive server frames into `buffer` instead of a [`SERVER_FRAME_BUF_LEN`]-byte stack
    /// buffer (the firmware passes a static one, keeping 2 KB off the task stack). A frame
    /// longer than `buffer` is dropped by the transport.
    pub fn set_frame_buffer(&mut self, buffer: &'static mut [u8]) {
        self.frame_buffer = Some(buffer);
    }

    /// Announces the player on a freshly connected control link: `HELO` (this player's MAC,
    /// `device_id`, `capabilities`) and the player name (`SETD`), and arms an immediate
    /// heartbeat so LMS gets a `STAT` on the next tick. A player that was never connected
    /// becomes [`PlayerState::Connected`]; a reconnect leaves the play state alone.
    pub fn hello<T: SlimTransport>(
        &mut self,
        transport: &mut T,
        device_id: u8,
        capabilities: &str,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        let mut buf = [0u8; 256];
        let len = SlimprotoBuilder::build_helo(&self.mac, device_id, capabilities, &mut buf)
            .map_err(|_| TransportError::Io)?;
        transport.send(&buf[..len])?;
        send_setd_name(transport, &self.player_name)?;
        if self.state == PlayerState::Disconnected {
            self.state = PlayerState::Connected;
        }
        self.last_heartbeat_ms = now_ms.wrapping_sub(self.heartbeat_interval_ms);
        Ok(())
    }

    /// Another audio source (device: Bluetooth A2DP) starts feeding the pipeline while no LMS
    /// stream plays: [`PlayerState::Playing`], with every per-track LMS event cleared, `STMs`
    /// marked as reported and a finished LMS stream detached, so nothing about it is reported
    /// to LMS.
    pub fn begin_external_playback(&mut self) {
        self.reset();
        self.stream = StreamLink::Idle;
        self.buffer_threshold_bytes = 0;
        self.track_started_reported = true;
        self.buffer_threshold_reported = true;
        self.pending_buffer_threshold = false;
        self.state = PlayerState::Playing;
    }

    /// The external source stopped and its audio drained: [`PlayerState::Stopped`].
    pub fn end_external_playback(&mut self) {
        self.state = PlayerState::Stopped;
    }

    /// Sets the stream EOF indicator.
    pub fn set_stream_eof(&mut self, eof: bool) {
        self.stream_eof = eof;
    }

    /// Returns `true` if the audio stream reached EOF.
    pub fn is_stream_eof(&self) -> bool {
        self.stream_eof
    }

    /// Returns `true` while an audio stream is connecting or connected.
    pub fn stream_active(&self) -> bool {
        !matches!(self.stream, StreamLink::Idle)
    }

    /// Takes the CYD display update produced by the last server command, if any.
    ///
    /// [`Self::tick`] handles at most one server command per call, so calling this after every
    /// tick observes every update.
    pub fn take_cyd_message(&mut self) -> Option<CydMessage> {
        self.pending_cyd.take()
    }

    /// Number of audio frames written to the output sink during the last [`Self::tick`].
    pub fn last_tick_frames(&self) -> usize {
        self.last_tick_frames
    }

    /// Resets transient stream coordinator state.
    pub fn reset(&mut self) {
        self.stream_eof = false;
        self.decode_complete_sent = false;
        self.pending_decode_ready = false;
        self.pending_track_started = false;
        self.pending_underrun = false;
        self.pending_dsco = None;
    }

    /// Decodes SlimProto sample rate code into Hz.
    /// Supports both ASCII '0'..='9', numeric 0..=9 codes, and colon ':' / 10 for 88.2 kHz (`RUST-16`).
    pub fn decode_pcm_sample_rate_hz(code: u8) -> Option<u32> {
        match code {
            b'0' | 0 => Some(11025),
            b'1' | 1 => Some(22050),
            b'2' | 2 => Some(32000),
            b'3' | 3 => Some(44100),
            b'4' | 4 => Some(48000),
            b'5' | 5 => Some(8000),
            b'6' | 6 => Some(12000),
            b'7' | 7 => Some(16000),
            b'8' | 8 => Some(24000),
            b'9' | 9 => Some(96000),
            b':' | 10 => Some(88200),
            b';' | 11 => Some(176400),
            b'<' | 12 => Some(192000),
            _ => None,
        }
    }

    /// Normalizes SlimProto sample size code into bits per sample.
    pub fn normalize_pcm_bits(code: u8) -> u8 {
        match code {
            b'0' | 0 | 8 => 8,
            b'1' | 1 | 16 => 16,
            b'2' | 2 | 24 => 24,
            b'3' | 3 | 32 => 32,
            _ => 16,
        }
    }

    /// Normalizes SlimProto channel code into channel count.
    pub fn normalize_pcm_channels(code: u8) -> u8 {
        match code {
            b'1' | 1 => 1,
            b'2' | 2 => 2,
            _ => 2,
        }
    }

    /// Normalizes SlimProto endianness code (ASCII '0' or 0 is Big Endian).
    pub fn normalize_pcm_big_endian(code: u8) -> bool {
        code == b'0' || code == 0
    }

    /// Runs one coordinator cycle (`RUST-01..RUST-10`):
    ///
    /// 1. resumes a pending stream `connect` (one that returned [`StreamError::WouldBlock`]):
    ///    success sends `STMc`, failure is reported like an immediate one (`STMn` + `DSCO(3)`);
    /// 2. receives at most one server frame from `transport` and handles it (any CYD update it
    ///    produces is available from [`Self::take_cyd_message`]);
    /// 3. moves HTTP audio bytes from `source` into the pipeline stream buffer;
    /// 4. evaluates the playback state machine, decodes and writes audio to the sink;
    /// 5. sends deferred `STAT` events (`STMs`/`STMd`/`STMu`), `RESP`, `META`, `DSCO` and the
    ///    periodic heartbeat.
    ///
    /// Malformed or oversized server frames are skipped. On [`TransportError::Disconnected`]
    /// from `transport` the audio steps still run (buffered audio keeps playing) and the error
    /// is returned without sending anything. Errors from sending are returned as is; deferred
    /// events that were not sent stay pending for the next tick.
    pub fn tick<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        self.last_tick_frames = 0;
        self.resume_stream_connect(transport, source, pipeline, now_ms)?;
        let link_lost = self.receive_command(transport, source, backend, pipeline, now_ms)?;
        let _ = self.poll_stream(source, pipeline);
        self.last_tick_frames = if self.external_audio_pump {
            // Decode/output belong to the caller's pump phases; only the play state moves here.
            self.evaluate_state(pipeline);
            0
        } else {
            self.process_audio_step(pipeline)
        };

        if link_lost {
            return Err(TransportError::Disconnected);
        }
        self.check_heartbeat(transport, pipeline, now_ms)
    }

    /// Step 2 of [`Self::tick`]: receives at most one server frame and handles it. Returns
    /// whether the control link is gone. Uses the buffer from [`Self::set_frame_buffer`], or
    /// a stack buffer in a separate out-of-line frame that is gone again before decoding.
    #[inline(never)]
    fn receive_command<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        match self.frame_buffer.take() {
            Some(buffer) => {
                let result =
                    self.receive_into(buffer, transport, source, backend, pipeline, now_ms);
                self.frame_buffer = Some(buffer);
                result
            }
            None => self.receive_on_stack(transport, source, backend, pipeline, now_ms),
        }
    }

    /// [`Self::receive_command`] without a configured frame buffer.
    #[inline(never)]
    fn receive_on_stack<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        let mut frame = [0u8; SERVER_FRAME_BUF_LEN];
        self.receive_into(&mut frame, transport, source, backend, pipeline, now_ms)
    }

    /// Receives at most one server frame into `frame` and handles it.
    #[inline(never)]
    fn receive_into<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        frame: &mut [u8],
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<bool, TransportError> {
        match transport.poll_frame(frame) {
            Ok(Some(len)) => {
                if let Ok((cmd, _)) = SlimprotoParser::parse_frame(&frame[..len]) {
                    if let Some(msg) =
                        self.handle_command(cmd, transport, source, backend, pipeline, now_ms)?
                    {
                        self.pending_cyd = Some(msg);
                    }
                }
                Ok(false)
            }
            Ok(None) | Err(TransportError::WouldBlock) | Err(TransportError::Io) => Ok(false),
            Err(TransportError::Disconnected) => Ok(true),
        }
    }

    /// Repeats a `connect` that previously returned [`StreamError::WouldBlock`].
    fn resume_stream_connect<S: AudioOutputSink, T: SlimTransport, R: StreamSource>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        let result = match &self.stream {
            StreamLink::Connecting { addr, request } => source.connect(*addr, request),
            _ => return Ok(()),
        };
        match result {
            Ok(()) => {
                self.stream = StreamLink::Open;
                self.arm_buffer_threshold();
                self.report_stream_connected(transport, pipeline, now_ms)
            }
            Err(StreamError::WouldBlock) => Ok(()),
            Err(_) => {
                self.pending_cyd = Some(CydMessage::PlaybackState(PlayState::Stopped));
                self.report_stream_unreachable(transport, source, pipeline, now_ms)
            }
        }
    }

    /// `STMc`: the stream connected (response accepted).
    fn report_stream_connected<S: AudioOutputSink, T: SlimTransport>(
        &mut self,
        transport: &mut T,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        send_stat(
            transport,
            StatEvent::Connected,
            pipeline.stream_buffer_available() as u32,
            pipeline.output_buffer_available() as u32,
            pipeline.frames_played as u32,
            now_ms,
        )
    }

    /// A stream that never delivered audio failed to connect (TCP failure, bad status, failed
    /// redirect hop, ...), immediately or after `WouldBlock`: `STMn` + `DSCO(3)`, Stopped. The
    /// same reason is also left in `pending_dsco`, so the heartbeat repeats the `DSCO`.
    fn report_stream_unreachable<S: AudioOutputSink, T: SlimTransport, R: StreamSource>(
        &mut self,
        transport: &mut T,
        source: &mut R,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        source.abort();
        self.stream = StreamLink::Idle;
        self.state = PlayerState::Stopped;
        self.pending_dsco = Some(DisconnectReason::Unreachable);
        send_stat(
            transport,
            StatEvent::NotSupported,
            pipeline.stream_buffer_available() as u32,
            pipeline.output_buffer_available() as u32,
            pipeline.frames_played as u32,
            now_ms,
        )?;
        send_dsco(transport, DisconnectReason::Unreachable)
    }

    /// `STMl` for a stream that does not start by itself (autostart off): once it is connected
    /// and not waiting for `cont`, at most once per track (LMS answers with `strm u`).
    fn arm_buffer_threshold(&mut self) {
        if !self.autostart_mode.is_auto()
            && !self.awaiting_cont
            && self.stream == StreamLink::Open
            && !self.buffer_threshold_reported
        {
            self.buffer_threshold_reported = true;
            self.pending_buffer_threshold = true;
        }
    }

    /// Installs the decoder for `format` (`strm s`, `codc`): MP3 from the pipeline's MP3 slot
    /// (see [`StreamPipeline::use_mp3_decoder`]), FLAC reusing the current FLAC decoder, PCM
    /// with the given layout. A format this build cannot decode gets a PCM decoder with the
    /// output muted (never the previous decoder, which would turn the bytes into noise) until
    /// the next `strm s`. Returns whether the format is decodable.
    fn select_decoder<S: AudioOutputSink>(
        pipeline: &mut StreamPipeline<'_, S>,
        format: AudioFormat,
        bits: u8,
        channels: u8,
        big_endian: bool,
    ) -> bool {
        let supported = match format {
            AudioFormat::Mp3 => pipeline.use_mp3_decoder(),
            AudioFormat::Flac => {
                match &mut pipeline.decoder {
                    // Same codec again (`codc`): restart the decoder at a frame boundary.
                    ActiveDecoder::Flac(current) => current.flush(),
                    _ => pipeline.set_decoder(ActiveDecoder::new_flac()),
                }
                true
            }
            AudioFormat::Aac => ActiveDecoder::new_aac().map(|d| pipeline.set_decoder(d)).is_some(),
            AudioFormat::Ogg => {
                ActiveDecoder::new_vorbis().map(|d| pipeline.set_decoder(d)).is_some()
            }
            AudioFormat::Pcm => {
                pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::with_config(
                    bits, big_endian, channels,
                )));
                true
            }
            AudioFormat::Opus | AudioFormat::Alac | AudioFormat::Unknown(_) => false,
        };
        if !supported {
            pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::with_config(bits, big_endian, channels)));
            pipeline.set_mute(true);
        }
        supported
    }

    /// Drops the stream-ring tail when the stream has ended and the decoder cannot make
    /// anything of what is left (a truncated last frame), so the track can finish
    /// (`STMd`/`STMu`) instead of waiting forever for bytes that will never come.
    fn drop_undecodable_tail<S: AudioOutputSink>(
        &self,
        pipeline: &mut StreamPipeline<'_, S>,
        decoded: &Result<usize, DecoderError>,
    ) {
        if self.stream_eof
            && matches!(decoded, Err(DecoderError::NeedMoreData))
            && !pipeline.has_pending_pcm()
        {
            pipeline.stream_buffer.flush();
        }
    }

    /// Records sink output: the first frames of a track make `STMs` pending.
    fn note_output(&mut self, written: usize) {
        if written > 0 && !self.track_started_reported {
            self.track_started_reported = true;
            self.pending_track_started = true;
        }
    }

    /// Output half of the audio step, for platforms that schedule decoding and output
    /// separately (device: top up the I2S DMA before and after each decode burst). Moves
    /// decoded PCM still pending into the output ring and writes the ring to the sink, up to
    /// `max_rounds` sink writes, while [`PlayerState::Playing`] with the DAC enabled. Returns
    /// the frames written.
    pub fn pump_output<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        max_rounds: usize,
    ) -> usize {
        if self.state != PlayerState::Playing {
            return 0;
        }
        let mut total = 0;
        for _ in 0..max_rounds {
            if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
                let _ = pipeline.process_decode_step();
            }
            if !self.dac_enabled || pipeline.output_buffer.available_read() < 8 {
                break;
            }
            let written = pipeline.process_output_step().unwrap_or(0);
            self.note_output(written);
            total += written;
            if written == 0 {
                break;
            }
        }
        if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
            let _ = pipeline.process_decode_step();
        }
        self.frames_played = pipeline.frames_played;
        total
    }

    /// Decode half of the audio step (see [`Self::pump_output`]): while
    /// [`PlayerState::Playing`], with no decoded PCM pending and at least
    /// [`PUMP_DECODE_MIN_OUTPUT_SPACE`] bytes free in the output ring, decodes until one step
    /// produced audio, or for at most `max_steps` steps that only consumed input (resync,
    /// tag skip). After the stream ended an undecodable tail is dropped.
    pub fn pump_decode<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
        max_steps: usize,
    ) {
        if self.state != PlayerState::Playing {
            return;
        }
        for _ in 0..max_steps {
            if pipeline.has_pending_pcm()
                || pipeline.output_buffer.available_write() < PUMP_DECODE_MIN_OUTPUT_SPACE
                || pipeline.stream_buffer.available_read() == 0
            {
                break;
            }
            let before = pipeline.output_buffer.available_read();
            let decoded = pipeline.process_decode_step();
            self.drop_undecodable_tail(pipeline, &decoded);
            if pipeline.has_pending_pcm() || pipeline.output_buffer.available_read() > before {
                break;
            }
            if !matches!(decoded, Ok(consumed) if consumed > 0) {
                break;
            }
        }
    }

    /// Drops a failed stream and marks it ended.
    fn fail_stream<R: StreamSource>(&mut self, source: &mut R) {
        self.stream_eof = true;
        source.abort();
        self.stream = StreamLink::Idle;
    }

    /// Drops the current stream, if any (`strm s`/`q`/`f`, `flsh`).
    fn close_stream<R: StreamSource>(&mut self, source: &mut R) {
        if self.stream_active() {
            source.abort();
        }
        self.stream = StreamLink::Idle;
    }

    /// Builds the HTTP request for a `strm s` from its header bytes (`COORD-503`): an empty
    /// header block requests `GET /stream.mp3 HTTP/1.0`; non-UTF-8 (e.g. Latin-1) headers are
    /// truncated to 1024 bytes with every non-ASCII byte replaced by `?`; the request is
    /// terminated with a blank line if the server did not include one.
    fn build_http_request(http_headers: &[u8]) -> Vec<u8> {
        let mut request: Vec<u8> = if http_headers.is_empty() {
            return DEFAULT_HTTP_REQUEST.to_vec();
        } else if core::str::from_utf8(http_headers).is_ok() {
            http_headers.to_vec()
        } else {
            http_headers
                .iter()
                .take(1024)
                .map(|&b| if b.is_ascii() { b } else { b'?' })
                .collect()
        };
        if !request.ends_with(b"\r\n\r\n") {
            if request.ends_with(b"\r\n") {
                request.extend_from_slice(b"\r\n");
            } else {
                request.extend_from_slice(b"\r\n\r\n");
            }
        }
        request
    }

    /// Handles one command from LMS, mutating the pipeline and sending the immediate replies.
    fn handle_command<S: AudioOutputSink, T: SlimTransport, R: StreamSource, B: AudioBackend>(
        &mut self,
        cmd: ServerCommand<'_>,
        transport: &mut T,
        source: &mut R,
        backend: &mut B,
        pipeline: &mut StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<Option<CydMessage>, TransportError> {
        match cmd {
            ServerCommand::Stream {
                command,
                autostart,
                format,
                pcm_sample_size,
                pcm_sample_rate,
                pcm_channels,
                pcm_endianness,
                threshold_kb,
                spdif_enable: _,
                trans_duration: _,
                trans_type: _,
                flags,
                output_threshold: _,
                replay_gain,
                server_port,
                server_ip,
                http_headers,
            } => {
                match command {
                    StreamCommandType::Start => {
                        // COORD-509 & COORD-506: Flush pipeline and reset frame counters at track start
                        pipeline.flush();
                        pipeline.frames_played = 0;
                        self.frames_played = 0;
                        self.pending_track_started = false;
                        self.pending_underrun = false;
                        self.pending_decode_ready = false;
                        self.decode_complete_sent = false;
                        self.pending_dsco = None;
                        self.pending_resp = None;
                        self.pending_meta = None;
                        self.pending_buffer_threshold = false;
                        self.track_started_reported = false;
                        self.buffer_threshold_reported = false;
                        self.stream_bytes_received = 0;
                        self.close_stream(source);
                        self.stream_eof = false;
                        // A mute for an undecodable previous track ends with it.
                        pipeline.user_muted = false;

                        self.autostart_mode = AutostartMode::from_strm(autostart, flags);
                        self.autostart = self.autostart_mode.is_auto();
                        self.awaiting_cont = self.autostart_mode.requires_cont();
                        self.server_rebooting = (flags & STRM_FLAG_SERVER_REBOOT) != 0;

                        if server_port > 0 {
                            let ip = if server_ip == [0, 0, 0, 0] { self.lms_ip } else { server_ip };
                            let addr = NetAddr { ip, port: server_port };
                            let request = Self::build_http_request(http_headers);

                            match source.connect(addr, &request) {
                                Ok(()) => {
                                    self.stream = StreamLink::Open;
                                    self.arm_buffer_threshold();
                                    self.report_stream_connected(transport, pipeline, now_ms)?;
                                }
                                // Response still pending: `STMc` (or `STMn`) once it resolves.
                                Err(StreamError::WouldBlock) => {
                                    self.stream = StreamLink::Connecting { addr, request };
                                }
                                Err(_) => {
                                    self.report_stream_unreachable(transport, source, pipeline, now_ms)?;
                                    return Ok(Some(CydMessage::PlaybackState(PlayState::Stopped)));
                                }
                            }
                        }

                        let sample_rate = Self::decode_pcm_sample_rate_hz(pcm_sample_rate)
                            .unwrap_or_else(|| sample_rate_from_index(pcm_sample_rate));
                        self.current_format = Some(format);
                        self.current_sample_rate = sample_rate;

                        let bits = Self::normalize_pcm_bits(pcm_sample_size);
                        let channels = Self::normalize_pcm_channels(pcm_channels);
                        let is_be = matches!(pcm_endianness, Endianness::BigEndian);

                        // Reconfigure the pipeline decoder for the format (a format this build
                        // cannot decode plays muted PCM; see `select_decoder`).
                        Self::select_decoder(pipeline, format, bits, channels, is_be);

                        let _ = pipeline.set_sample_rate(sample_rate);
                        // An unsupported rate keeps the current clock for now (the STMn reply
                        // to an unsupported rate is a separate behavior change).
                        let _ = backend.set_sample_rate(sample_rate);
                        // Track start powers the amplifier (device: Hi-Z -> Play when idle),
                        // unless LMS disabled the DAC (`aude`).
                        if self.dac_enabled {
                            backend.set_power(true);
                        }

                        let requested_threshold = (threshold_kb as usize).saturating_mul(1024);
                        let max_safe_threshold = pipeline.stream_buffer.capacity().saturating_sub(1024);
                        let effective_threshold = if max_safe_threshold > 0 {
                            requested_threshold.min(max_safe_threshold)
                        } else {
                            requested_threshold.min(pipeline.stream_buffer.capacity())
                        };
                        self.buffer_threshold_bytes = effective_threshold;

                        if effective_threshold > 0 || self.awaiting_cont {
                            self.state = PlayerState::Buffering;
                            Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                        } else if self.autostart {
                            self.state = PlayerState::Playing;
                            // RUST-04: STMs deferred until first PCM write in process_audio_step
                            self.pending_track_started = false;
                            Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                        } else {
                            self.state = PlayerState::Paused;
                            send_stat(
                                transport,
                                StatEvent::TrackPaused,
                                pipeline.stream_buffer_available() as u32,
                                pipeline.output_buffer_available() as u32,
                                pipeline.frames_played as u32,
                                now_ms,
                            )?;
                            Ok(Some(CydMessage::PlaybackState(PlayState::Paused)))
                        }
                    }
                    StreamCommandType::Pause => {
                        self.state = PlayerState::Paused;
                        self.buffer_threshold_bytes = 0;
                        pipeline.sink.set_mute(true);
                        backend.set_mute(true);
                        send_stat(
                            transport,
                            StatEvent::TrackPaused,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(Some(CydMessage::PlaybackState(PlayState::Paused)))
                    }
                    StreamCommandType::Resume => {
                        self.state = PlayerState::Playing;
                        pipeline.sink.set_mute(!self.dac_enabled);
                        backend.set_mute(!self.dac_enabled);
                        send_stat(
                            transport,
                            StatEvent::TrackResumed,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                    }
                    StreamCommandType::Stop => {
                        self.state = PlayerState::Stopped;
                        self.buffer_threshold_bytes = 0;
                        self.close_stream(source);
                        self.pending_track_started = false;
                        self.pending_underrun = false;
                        self.pending_decode_ready = false;
                        self.decode_complete_sent = false;
                        pipeline.sink.set_mute(true);
                        backend.set_mute(true);
                        send_stat(
                            transport,
                            StatEvent::TrackStopped,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(Some(CydMessage::PlaybackState(PlayState::Stopped)))
                    }
                    StreamCommandType::Flush => {
                        self.flush_stream(source, pipeline);
                        send_stat(
                            transport,
                            StatEvent::StreamFlushed,
                            0,
                            0,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(None)
                    }
                    StreamCommandType::Unknown(b't') => {
                        // RUST-06: strm 't' timestamp probe immediately echoes replay_gain in server_timestamp
                        send_stat_with_metrics(
                            transport,
                            StatEvent::Heartbeat,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played,
                            self.current_sample_rate,
                            self.stream_bytes_received,
                            now_ms,
                            replay_gain,
                        )?;
                        Ok(None)
                    }
                    StreamCommandType::Unknown(b'a') => {
                        // CODE-PROTO-003: strm 'a' skip-ahead consumes output buffer frames using u64 math
                        let skip_ms = replay_gain;
                        let frames = (u64::from(skip_ms) * u64::from(self.current_sample_rate)) / 1000;
                        let bytes = usize::try_from(frames.saturating_mul(4)).unwrap_or(usize::MAX);
                        pipeline.output_buffer.consume(bytes);
                        Ok(None)
                    }
                    StreamCommandType::Unknown(_) => Ok(None),
                }
            }
            ServerCommand::AudioGain {
                gain_left,
                gain_right,
                digital_atten,
                ..
            } => {
                let pct = if digital_atten == 0 {
                    let (eff_gain_left, eff_gain_right) = if gain_left == 0
                        && gain_right == 0
                        && self.volume.gain_left > 0
                        && self.volume.gain_right > 0
                    {
                        (65536, 65536)
                    } else {
                        let l = if gain_left > 0 { 65536 } else { 0 };
                        let r = if gain_right > 0 { 65536 } else { 0 };
                        (l, r)
                    };
                    self.volume.gain_left = eff_gain_left;
                    self.volume.gain_right = eff_gain_right;
                    pipeline.set_volume(eff_gain_left, eff_gain_right);
                    backend.set_hw_volume(
                        lms_gain_to_tas5805m_vol_reg(gain_left),
                        lms_gain_to_tas5805m_vol_reg(gain_right),
                    );

                    // COORD-507: Map 16.16 fixed-point (65536 = 100%) to 0-100 percentage using u64 to prevent u32 overflow
                    let avg_gain = ((eff_gain_left as u64 + eff_gain_right as u64) / 2) as u32;
                    core::cmp::min(100, ((avg_gain as u64 * 100) / 65536).min(100) as u8)
                } else {
                    // Device policy: the amplifier attenuates (one TAS register code from the
                    // louder channel, applied to both), software gain stays at unity (or 0 for
                    // mute), so the volume is never applied twice.
                    let max_gain = core::cmp::max(gain_left, gain_right);
                    let vol_reg = lms_gain_to_tas5805m_vol_reg(max_gain);
                    backend.set_hw_volume(vol_reg, vol_reg);
                    let eff_gain = if max_gain > 0 { 65536 } else { 0 };
                    self.volume.gain_left = eff_gain;
                    self.volume.gain_right = eff_gain;
                    pipeline.set_volume(eff_gain, eff_gain);
                    lms_gain_to_hardware_volume_percent(max_gain)
                };

                Ok(Some(CydMessage::Volume(pct)))
            }
            ServerCommand::SetDevice { sample_rate } => {
                if sample_rate <= u8::MAX as u32 {
                    if sample_rate == 0 {
                        send_setd_name(transport, &self.player_name)?;
                    }
                    // 1-byte id != 0 query is NOT a sample rate; do not update current_sample_rate
                    return Ok(None);
                }
                let bytes = sample_rate.to_be_bytes();
                if bytes[0] == 0 && bytes[1..].iter().all(|&b| (0x20..=0x7E).contains(&b)) {
                    if let Ok(name) = core::str::from_utf8(&bytes[1..]) {
                        self.set_player_name(name);
                    }
                    send_setd_name(transport, &self.player_name)?;
                    return Ok(None);
                }
                self.current_sample_rate = sample_rate;
                let _ = pipeline.set_sample_rate(sample_rate);
                let _ = backend.set_sample_rate(sample_rate);
                Ok(None)
            }
            ServerCommand::Flush => {
                self.flush_stream(source, pipeline);
                send_stat(
                    transport,
                    StatEvent::StreamFlushed,
                    0,
                    0,
                    pipeline.frames_played as u32,
                    now_ms,
                )?;
                Ok(None)
            }
            ServerCommand::Body { data } => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    let _ = pipeline.ingest_stream_data(data);
                }
                Ok(None)
            }
            ServerCommand::Unknown { opcode, payload } => {
                if let Some((_spdif, dac_enable)) = cmd.audio_enable() {
                    self.dac_enabled = dac_enable;
                    pipeline.sink.set_mute(!dac_enable);
                    backend.set_power(dac_enable);
                } else if let Some(metaint) = cmd.stream_continue() {
                    self.awaiting_cont = false;
                    if metaint > 0 && self.stream_active() {
                        source.set_icy_metaint(metaint as usize);
                    }
                    self.arm_buffer_threshold();
                } else if opcode == *b"codc" && !payload.is_empty() {
                    // Codec change: format, sample size, rate, channels, endianness.
                    let format = AudioFormat::from_byte(payload[0]);
                    let bits = payload.get(1).map_or(16, |&b| Self::normalize_pcm_bits(b));
                    let channels = payload.get(3).map_or(2, |&b| Self::normalize_pcm_channels(b));
                    let is_be = payload
                        .get(4)
                        .is_none_or(|&b| matches!(Endianness::from_byte(b), Endianness::BigEndian));
                    self.current_format = Some(format);
                    Self::select_decoder(pipeline, format, bits, channels, is_be);
                } else if let Some(setting) = cmd.device_setting() {
                    if setting.id == 0 {
                        // LMS NUL-terminates the name; never keep (or echo) the terminator.
                        let data = setting.data.split(|&b| b == 0).next().unwrap_or(&[]);
                        if !data.is_empty() {
                            if let Ok(name) = core::str::from_utf8(data) {
                                self.set_player_name(name);
                            }
                        }
                        send_setd_name(transport, &self.player_name)?;
                    }
                } else if opcode == *b"eqlz" || opcode == *b"loud" {
                    pipeline.equalizer.enabled = true;
                }
                Ok(None)
            }
        }
    }

    /// A `setd` id-0 name change (already cut at its first NUL): announced name, and persisted
    /// through [`Self::config`] (the journal keeps at most 32 bytes, cut on a `char` boundary).
    fn set_player_name(&mut self, name: &str) {
        self.player_name.clear();
        let _ = self.player_name.push_str(name);
        if let Some(config) = self.config {
            config.update(|c| {
                c.set_player_name(name);
            });
        }
    }

    /// Shared state reset of `strm f` and `flsh`.
    fn flush_stream<S: AudioOutputSink, R: StreamSource>(
        &mut self,
        source: &mut R,
        pipeline: &mut StreamPipeline<'_, S>,
    ) {
        self.buffer_threshold_bytes = 0;
        self.close_stream(source);
        self.pending_track_started = false;
        self.pending_underrun = false;
        self.pending_decode_ready = false;
        self.decode_complete_sent = false;
        if self.state == PlayerState::Buffering {
            self.state = PlayerState::Stopped;
        }
        pipeline.flush();
    }

    /// Collects the response head and ICY metadata the source has captured (through the
    /// callback takes, so no 4 KB `HeaderBuf`/`MetaBuf` value lands on this stack).
    fn take_stream_side_data<R: StreamSource>(&mut self, source: &mut R) {
        let resp = &mut self.pending_resp;
        source.take_response_headers_with(&mut |head| *resp = Some(head.to_vec()));
        let meta = &mut self.pending_meta;
        source.take_icy_metadata_with(&mut |block| *meta = Some(block.to_vec()));
    }

    /// Moves HTTP audio bytes from `source` into the pipeline stream buffer.
    ///
    /// Stops when no bytes are available right now (`HALNET-405`) or the stream buffer is full,
    /// records a clean end of stream in `stream_eof`, and drops the stream (also setting
    /// `stream_eof`) when `poll_bytes` fails (`COORD-508`). While a `cont` is awaited only the
    /// response head is collected: the body (and its ICY framing) starts after `cont`. Kept
    /// out of line so its read chunk is off the stack again before decoding starts.
    #[inline(never)]
    fn poll_stream<S: AudioOutputSink, R: StreamSource>(
        &mut self,
        source: &mut R,
        pipeline: &mut StreamPipeline<'_, S>,
    ) -> Result<usize, StreamError> {
        if self.awaiting_cont {
            if self.stream == StreamLink::Open {
                self.take_stream_side_data(source);
            }
            return Ok(0);
        }
        let mut total_written = 0;
        let mut chunk = [0u8; 1024];
        while self.stream == StreamLink::Open {
            self.take_stream_side_data(source);
            let avail_write = pipeline.stream_buffer.available_write();
            if avail_write == 0 {
                break;
            }
            let max_read = avail_write.min(chunk.len());
            match source.poll_bytes(&mut chunk[..max_read]) {
                Ok(0) => {
                    if source.is_eof() {
                        self.stream_eof = true;
                    }
                    break;
                }
                Ok(n) => {
                    self.stream_bytes_received += n as u64;
                    let written = pipeline.ingest_stream_data(&chunk[..n]).unwrap_or(0);
                    total_written += written;
                    if written < n {
                        break;
                    }
                }
                Err(e) => {
                    self.fail_stream(source);
                    return Err(e);
                }
            }
        }
        if self.stream == StreamLink::Open {
            self.take_stream_side_data(source);
            if source.is_eof() {
                self.stream_eof = true;
            }
        }
        Ok(total_written)
    }

    /// Evaluates buffering threshold, short-stream EOF transitions (`COORD-501`),
    /// decoder completion (`STMd` + `DSCO(0)`), and end-of-track underrun (`STMu` + Stopped).
    fn evaluate_state<S: AudioOutputSink>(&mut self, pipeline: &mut StreamPipeline<'_, S>) {
        self.frames_played = pipeline.frames_played;
        let is_eof = self.stream_eof;

        if self.state == PlayerState::Buffering
            && !self.awaiting_cont
            && (pipeline.stream_buffer.available_read() >= self.buffer_threshold_bytes
                || pipeline.stream_buffer.available_write() == 0
                || (is_eof && pipeline.stream_buffer.available_read() > 0))
        {
            if self.autostart {
                self.state = PlayerState::Playing;
            } else {
                self.state = PlayerState::Paused;
            }
        }

        // HTTP stream reached EOF and stream buffer drained: decoder ready for next track (STMd + DSCO(0))
        if (self.state == PlayerState::Playing || self.state == PlayerState::Buffering)
            && is_eof
            && pipeline.stream_buffer.available_read() == 0
            && !self.decode_complete_sent
        {
            self.decode_complete_sent = true;
            self.pending_decode_ready = true;
            self.pending_dsco = Some(DisconnectReason::ConnectionClosed);
        }

        // Output buffer drained and no pending PCM remaining: track playback completed (STMu & Stopped)
        if self.decode_complete_sent
            && self.state == PlayerState::Playing
            && pipeline.output_buffer.available_read() == 0
            && !pipeline.has_pending_pcm()
        {
            self.state = PlayerState::Stopped;
            self.pending_underrun = true;
        }
    }

    /// Executes one cycle of state evaluation, decoding and audio output on data already in the
    /// stream buffer (`tick` fills it from the stream source first). Returns the frames written.
    pub fn process_audio_step<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<'_, S>,
    ) -> usize {
        self.evaluate_state(pipeline);

        if self.state != PlayerState::Playing {
            return 0;
        }

        // Try decoding a frame chunk
        let decoded = pipeline.process_decode_step();
        self.drop_undecodable_tail(pipeline, &decoded);

        // Write frames to the output sink
        let written = pipeline.process_output_step().unwrap_or(0);
        self.note_output(written);
        self.frames_played = pipeline.frames_played;

        // Re-evaluate EOF / underrun transition after draining output buffer
        self.evaluate_state(pipeline);

        written
    }

    /// Builds the full set of CYD telemetry messages (`PlaybackState`, `Volume`, and `Progress`)
    /// for status synchronization (`COORD-505`).
    pub fn sync_cyd_telemetry<S: AudioOutputSink>(
        &self,
        pipeline: &StreamPipeline<S>,
    ) -> heapless::Vec<CydMessage, 4> {
        let mut msgs = heapless::Vec::new();
        let _ = msgs.push(CydMessage::PlaybackState(self.cyd_playback_state()));

        let avg_gain = ((self.volume.gain_left as u64 + self.volume.gain_right as u64) / 2) as u32;
        let pct = core::cmp::min(100, ((avg_gain as u64 * 100) / 65536).min(100) as u8);
        let _ = msgs.push(CydMessage::Volume(pct));

        let elapsed_secs = if self.current_sample_rate > 0 {
            (pipeline.frames_played / (self.current_sample_rate as u64)) as u32
        } else {
            0
        };
        let _ = msgs.push(CydMessage::Progress {
            elapsed_secs,
            duration_secs: 0,
        });

        msgs
    }

    /// Handles an interactive command from the CYD front panel display.
    pub fn handle_cyd_command<S: AudioOutputSink>(
        &mut self,
        cmd: CydCommand,
        pipeline: &mut StreamPipeline<S>,
    ) -> Option<CydMessage> {
        match cmd {
            CydCommand::Play => {
                self.state = PlayerState::Playing;
                pipeline.sink.set_mute(false);
                Some(CydMessage::PlaybackState(PlayState::Playing))
            }
            CydCommand::Pause => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    self.state = PlayerState::Paused;
                    self.buffer_threshold_bytes = 0;
                    pipeline.sink.set_mute(true);
                }
                Some(CydMessage::PlaybackState(PlayState::Paused))
            }
            CydCommand::Toggle => {
                if self.state == PlayerState::Playing || self.state == PlayerState::Buffering {
                    self.state = PlayerState::Paused;
                    self.buffer_threshold_bytes = 0;
                    pipeline.sink.set_mute(true);
                    Some(CydMessage::PlaybackState(PlayState::Paused))
                } else {
                    self.state = PlayerState::Playing;
                    pipeline.sink.set_mute(false);
                    Some(CydMessage::PlaybackState(PlayState::Playing))
                }
            }
            CydCommand::SetVolume(vol) => {
                let gain = ((vol.min(100) as u32) * 65536) / 100;
                self.volume.gain_left = gain;
                self.volume.gain_right = gain;
                pipeline.set_volume(gain, gain);
                Some(CydMessage::Volume(vol))
            }
            CydCommand::QueryStatus => {
                Some(CydMessage::PlaybackState(self.cyd_playback_state()))
            }
            CydCommand::Next | CydCommand::Previous => None,
        }
    }

    /// Emits pending deferred `STAT` transition events (`STMs` / `STMd` / `STMu`),
    /// `RESP`, `META`, `DSCO`, and periodic `STAT` heartbeats.
    fn check_heartbeat<S: AudioOutputSink, T: SlimTransport>(
        &mut self,
        transport: &mut T,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        if let Some(head) = self.pending_resp.take() {
            let (sent, head) = send_resp(transport, head);
            if sent == Err(TransportError::WouldBlock) {
                self.pending_resp = Some(head);
            }
            sent?;
        }

        if let Some(meta) = &self.pending_meta {
            let sent = send_payload_frame(transport, meta, SlimprotoBuilder::build_meta);
            settle_pending(&mut self.pending_meta, sent)?;
        }

        if self.pending_buffer_threshold {
            self.send_metrics_stat(transport, StatEvent::BUFFER_THRESHOLD, pipeline, now_ms)?;
            self.pending_buffer_threshold = false;
        }

        if self.pending_track_started {
            self.send_metrics_stat(transport, StatEvent::TrackStarted, pipeline, now_ms)?;
            self.pending_track_started = false;
        }

        if self.pending_decode_ready {
            self.send_metrics_stat(transport, StatEvent::DecodeReady, pipeline, now_ms)?;
            self.pending_decode_ready = false;
        }

        if let Some(reason) = self.pending_dsco {
            send_dsco(transport, reason)?;
            self.pending_dsco = None;
        }

        if self.pending_underrun {
            self.send_metrics_stat(transport, StatEvent::TrackUnderrun, pipeline, now_ms)?;
            self.pending_underrun = false;
        }

        if now_ms.wrapping_sub(self.last_heartbeat_ms) >= self.heartbeat_interval_ms {
            self.last_heartbeat_ms = now_ms;
            self.send_metrics_stat(transport, StatEvent::Heartbeat, pipeline, now_ms)?;
        }
        Ok(())
    }

    /// Sends a `STAT` carrying the current buffer, frame, rate and byte metrics.
    fn send_metrics_stat<S: AudioOutputSink, T: SlimTransport>(
        &self,
        transport: &mut T,
        event: StatEvent,
        pipeline: &StreamPipeline<'_, S>,
        now_ms: u32,
    ) -> Result<(), TransportError> {
        send_stat_with_metrics(
            transport,
            event,
            pipeline.stream_buffer.available_read() as u32,
            pipeline.output_buffer.available_read() as u32,
            pipeline.frames_played,
            self.current_sample_rate,
            self.stream_bytes_received,
            now_ms,
            0,
        )
    }

    /// Maps current player state to CYD display playback state.
    pub fn cyd_playback_state(&self) -> PlayState {
        match self.state {
            PlayerState::Playing | PlayerState::Buffering => PlayState::Playing,
            PlayerState::Paused => PlayState::Paused,
            _ => PlayState::Stopped,
        }
    }
}

/// Sends a `RESP`. A head the transport can never take (`Io`, e.g. larger than the device's
/// 2 KB control-socket buffer) is cut at a header-line boundary, halving the limit until it
/// fits, rather than dropped. Returns the outcome and the head as last tried.
fn send_resp<T: SlimTransport>(
    transport: &mut T,
    mut head: Vec<u8>,
) -> (Result<(), TransportError>, Vec<u8>) {
    loop {
        let sent = send_payload_frame(transport, &head, SlimprotoBuilder::build_resp);
        if sent != Err(TransportError::Io) || head.len() <= 64 {
            return (sent, head);
        }
        head = crate::stream_codec::truncate_head(&head, head.len() / 2);
    }
}

/// Outcome of sending a deferred `RESP`/`META`: sent or failed for good clears it, `WouldBlock`
/// keeps it for the next tick (a frame that can never be sent must not block the link).
fn settle_pending(
    pending: &mut Option<Vec<u8>>,
    sent: Result<(), TransportError>,
) -> Result<(), TransportError> {
    if sent != Err(TransportError::WouldBlock) {
        *pending = None;
    }
    sent
}

/// Sends a compact `STAT` (frame counter as byte counter, elapsed time from the timestamp).
fn send_stat<T: SlimTransport>(
    transport: &mut T,
    event: StatEvent,
    stream_buf_fullness: u32,
    output_buf_fullness: u32,
    frames_played: u32,
    timestamp_ms: u32,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 64];
    let len = SlimprotoBuilder::build_stat(
        event,
        stream_buf_fullness,
        output_buf_fullness,
        frames_played,
        timestamp_ms,
        &mut buf,
    )
    .map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `STAT` with explicit metrics (sample rate, 64-bit frames and bytes, server timestamp).
#[allow(clippy::too_many_arguments)]
fn send_stat_with_metrics<T: SlimTransport>(
    transport: &mut T,
    event: StatEvent,
    stream_buf_fullness: u32,
    output_buf_fullness: u32,
    frames_played: u64,
    sample_rate: u32,
    bytes_received: u64,
    jiffies_ms: u32,
    server_timestamp: u32,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 128];
    let len = StatBuilder::build_stat_with_metrics(
        event,
        stream_buf_fullness,
        output_buf_fullness,
        frames_played,
        sample_rate,
        bytes_received,
        jiffies_ms,
        server_timestamp,
        &mut buf,
    )
    .map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `DSCO` disconnect notification.
fn send_dsco<T: SlimTransport>(
    transport: &mut T,
    reason: DisconnectReason,
) -> Result<(), TransportError> {
    let mut buf = [0u8; 16];
    let len = SlimprotoBuilder::build_dsco(reason, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends a `SETD` player name (setting id 0).
fn send_setd_name<T: SlimTransport>(transport: &mut T, name: &str) -> Result<(), TransportError> {
    let mut buf = alloc::vec![0u8; 9 + name.len()];
    let len = SlimprotoBuilder::build_setd_name(name, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

/// Sends an 8-byte-header frame wrapping `payload` (`RESP`, `META`).
fn send_payload_frame<T: SlimTransport, E>(
    transport: &mut T,
    payload: &[u8],
    build: fn(&[u8], &mut [u8]) -> Result<usize, E>,
) -> Result<(), TransportError> {
    let mut buf = alloc::vec![0u8; 8 + payload.len()];
    let len = build(payload, &mut buf).map_err(|_| TransportError::Io)?;
    transport.send(&buf[..len])
}

#[cfg(test)]
mod tests {
    //! Coordinator tests drive [`PlayerCoordinator::tick`] over the in-memory I/O fakes
    //! ([`LoopbackTransport`], [`FakeStreamSource`] behind [`GatedSource`], [`MockBackend`]).
    //! End-to-end coverage over real sockets lives in `tests/session_replay.rs`.
    use super::*;
    use crate::io_traits::{
        FakeStreamSource, HeaderBuf, LoopbackTransport, MetaBuf, MockBackend, Segments,
        LOOPBACK_QUEUE_DEPTH,
    };
    use std::boxed::Box;
    use squeezelite_audio_core::MockAudioSink;
    use std::string::{String, ToString};
    use std::vec;

    type TestPipeline<'a> = StreamPipeline<'a, MockAudioSink<64>>;

    fn pcm_pipeline<'a>(stream: &'a mut [u8], output: &'a mut [u8]) -> TestPipeline<'a> {
        StreamPipeline::new(
            stream,
            output,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44100,
        )
    }

    /// Server frame with the 8-byte header (`opcode`, big-endian payload length).
    fn server_frame(opcode: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(8 + payload.len());
        frame.extend_from_slice(opcode);
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    /// `strm` frame for 16-bit / 44.1 kHz (code 3) / stereo / big-endian PCM, flags 0.
    fn strm_frame(
        command: u8,
        autostart: bool,
        threshold_kb: u8,
        replay_gain: u32,
        server_port: u16,
        server_ip: [u8; 4],
        http_headers: &[u8],
    ) -> Vec<u8> {
        let mut payload = vec![0u8; 24];
        payload[0] = command;
        payload[1] = if autostart { b'1' } else { b'0' };
        payload[2] = b'p';
        payload[3] = 16;
        payload[4] = 3;
        payload[5] = 2;
        payload[6] = b'0'; // big endian
        payload[7] = threshold_kb;
        payload[14..18].copy_from_slice(&replay_gain.to_be_bytes());
        payload[18..20].copy_from_slice(&server_port.to_be_bytes());
        payload[20..24].copy_from_slice(&server_ip);
        payload.extend_from_slice(http_headers);
        server_frame(b"strm", &payload)
    }

    fn simple_strm(command: u8) -> Vec<u8> {
        strm_frame(command, false, 0, 0, 0, [0, 0, 0, 0], &[])
    }

    fn audg_frame(gain_left: u32, gain_right: u32, digital_atten: u8) -> Vec<u8> {
        let mut payload = vec![0u8; 18];
        payload[8] = digital_atten;
        payload[10..14].copy_from_slice(&gain_left.to_be_bytes());
        payload[14..18].copy_from_slice(&gain_right.to_be_bytes());
        server_frame(b"audg", &payload)
    }

    /// [`FakeStreamSource`] whose connect and data can be held back, standing in for a server
    /// that has not answered yet. It also records the last HTTP request.
    struct GatedSource {
        inner: FakeStreamSource,
        hold_connect: bool,
        hold_data: bool,
        last_request: Vec<u8>,
    }

    impl GatedSource {
        fn new(path: &'static [u8], segments: Segments) -> Self {
            let mut inner = FakeStreamSource::new();
            assert!(inner.add_route(path, segments));
            Self { inner, hold_connect: false, hold_data: false, last_request: Vec::new() }
        }
    }

    impl StreamSource for GatedSource {
        fn connect(&mut self, addr: NetAddr, http_request: &[u8]) -> Result<(), StreamError> {
            self.last_request = http_request.to_vec();
            if self.hold_connect {
                return Err(StreamError::WouldBlock);
            }
            self.inner.connect(addr, http_request)
        }

        fn poll_bytes(&mut self, out: &mut [u8]) -> Result<usize, StreamError> {
            if self.hold_data {
                return Ok(0);
            }
            self.inner.poll_bytes(out)
        }

        fn take_response_headers(&mut self) -> Option<HeaderBuf> {
            self.inner.take_response_headers()
        }

        fn take_icy_metadata(&mut self) -> Option<MetaBuf> {
            self.inner.take_icy_metadata()
        }

        fn set_icy_metaint(&mut self, interval: usize) {
            self.inner.set_icy_metaint(interval);
        }

        fn is_eof(&self) -> bool {
            !self.hold_data && self.inner.is_eof()
        }

        fn abort(&mut self) {
            self.inner.abort();
        }
    }

    /// One outbound frame: opcode and payload.
    type Sent = ([u8; 4], Vec<u8>);

    /// Coordinator plus its fakes; collects every frame the coordinator sends.
    struct Rig {
        coord: PlayerCoordinator,
        lms: LoopbackTransport,
        src: GatedSource,
        backend: MockBackend,
        sent: Vec<Sent>,
    }

    static NO_ROUTE: Segments = &[];

    impl Rig {
        fn new(mac: [u8; 6], name: &'static str) -> Self {
            Self::with_source(mac, name, GatedSource::new(b"/none", NO_ROUTE))
        }

        fn with_source(mac: [u8; 6], name: &'static str, src: GatedSource) -> Self {
            Self {
                coord: PlayerCoordinator::new(mac, name),
                lms: LoopbackTransport::new(),
                src,
                backend: MockBackend::new(),
                sent: Vec::new(),
            }
        }

        fn drain_sent(&mut self) {
            while let Some(frame) = self.lms.pop_sent() {
                let opcode = [frame[0], frame[1], frame[2], frame[3]];
                self.sent.push((opcode, frame[8..].to_vec()));
            }
        }

        fn tick(&mut self, pipeline: &mut TestPipeline<'_>, now_ms: u32) -> Result<(), TransportError> {
            let result =
                self.coord.tick(&mut self.lms, &mut self.src, &mut self.backend, pipeline, now_ms);
            self.drain_sent();
            result
        }

        /// Delivers one server frame and runs the tick that handles it.
        fn command(
            &mut self,
            pipeline: &mut TestPipeline<'_>,
            frame: &[u8],
            now_ms: u32,
        ) -> Option<CydMessage> {
            self.lms.inject_frame(frame).unwrap();
            self.tick(pipeline, now_ms).unwrap();
            self.coord.take_cyd_message()
        }

        /// Outbound frames as `STAT:STMx`, `DSCO:n`, `RESP`, `SETD`, ...
        fn lines(&self) -> Vec<String> {
            self.sent
                .iter()
                .map(|(op, payload)| match op {
                    b"STAT" => format!("STAT:{}", String::from_utf8_lossy(&payload[0..4])),
                    b"DSCO" => format!("DSCO:{}", payload[0]),
                    other => String::from_utf8_lossy(other).to_string(),
                })
                .collect()
        }
    }

    // Big-endian 16-bit stereo PCM: (1, -256)... two frames, used by the short-stream tests.
    const TWO_FRAMES: &[u8] = &[0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00];
    const OK_HEAD: &[u8] = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
    static SHORT_STREAM: Segments = &[OK_HEAD, TWO_FRAMES];

    #[test]
    fn test_coordinator_stream_and_audio_lifecycle() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig = Rig::new(mac, "TestPlayer");
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Start stream: autostart, PCM 44.1 kHz, threshold 0, no HTTP stream
        let cyd_msg = rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 0, [0, 0, 0, 0], &[]), 100);
        assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert_eq!(rig.backend.rate, 44100);

        // Feed 2 stereo frames: (1000, -1000), (2000, -2000)
        let f1_l = (1000i16).to_be_bytes();
        let f1_r = (-1000i16).to_be_bytes();
        let f2_l = (2000i16).to_be_bytes();
        let f2_r = (-2000i16).to_be_bytes();
        let raw_audio = [
            f1_l[0], f1_l[1], f1_r[0], f1_r[1],
            f2_l[0], f2_l[1], f2_r[0], f2_r[1],
        ];

        // The tick that ingests the body decodes and plays it, then emits the deferred STMs
        rig.command(&mut pipeline, &server_frame(b"body", &raw_audio), 150);
        assert_eq!(rig.coord.last_tick_frames(), 2);
        assert!(!rig.coord.pending_track_started);
        assert_eq!(rig.lines(), ["STAT:STMs"]);

        // Verify recorded samples
        let rec = pipeline.sink.recorded_samples();
        assert_eq!(rec[0], 1000 << 16);
        assert_eq!(rec[1], -1000 << 16);

        // Volume adjustment with digital_atten: 1 (gain 0.5 = -6 dB): the amplifier attenuates,
        // software gain stays at unity (device policy, no double attenuation)
        let cyd_vol = rig.command(&mut pipeline, &audg_frame(32768, 32768, 1), 200);
        assert_eq!(cyd_vol, Some(CydMessage::Volume(91)));
        let reg = lms_gain_to_tas5805m_vol_reg(32768);
        assert_eq!(reg, 0x39);
        assert_eq!(rig.backend.hw_vol, (reg, reg));
        assert_eq!((rig.coord.volume.gain_left, rig.coord.volume.gain_right), (65536, 65536));

        // Pause
        let cyd_pause = rig.command(&mut pipeline, &simple_strm(b'p'), 250);
        assert_eq!(cyd_pause, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(rig.coord.state, PlayerState::Paused);
        assert!(rig.backend.muted);

        assert_eq!(rig.lines(), ["STAT:STMs", "STAT:STMp"]);
    }

    #[test]
    fn test_threshold_kb_clamping_and_full_buffer_exit() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // threshold_kb = 255 -> 255 * 1024 = 261120 bytes requested.
        // Capacity is 2048 bytes. max_safe_threshold = 2048.saturating_sub(1024) = 1024.
        // effective_threshold should clamp to 1024, preventing buffer deadlock.
        let requested_threshold = (255usize).saturating_mul(1024);
        let max_safe_threshold = pipeline.stream_buffer.capacity().saturating_sub(1024);
        let effective_threshold = if max_safe_threshold > 0 {
            requested_threshold.min(max_safe_threshold)
        } else {
            requested_threshold.min(pipeline.stream_buffer.capacity())
        };
        assert_eq!(effective_threshold, 1024);

        coordinator.buffer_threshold_bytes = effective_threshold;
        coordinator.state = PlayerState::Buffering;
        coordinator.autostart = true;

        // With empty buffer, coordinator remains in Buffering
        coordinator.evaluate_state(&mut pipeline);
        assert_eq!(coordinator.state, PlayerState::Buffering);

        // Ingest 1024 bytes (reaching effective threshold)
        let chunk = [0x55u8; 1024];
        pipeline.ingest_stream_data(&chunk).unwrap();
        coordinator.evaluate_state(&mut pipeline);
        // Buffer threshold met: transitions to Playing
        assert_eq!(coordinator.state, PlayerState::Playing);

        // Test full-buffer exit when available_write() == 0 even before reaching threshold
        pipeline.flush();
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 2000;
        coordinator.autostart = true;

        // Fill buffer until completely full (capacity - 1 due to slot reservation)
        let fill_buf = [0xAAu8; 2047];
        pipeline.ingest_stream_data(&fill_buf).unwrap();
        assert_eq!(pipeline.stream_buffer.available_write(), 0);

        // evaluate_state() must exit Buffering to avoid deadlock
        coordinator.evaluate_state(&mut pipeline);
        assert_eq!(coordinator.state, PlayerState::Playing);
    }

    #[test]
    fn test_strm_threshold_kb_is_clamped_to_stream_buffer() {
        // The same clamp, through a real `strm s` (threshold_kb = 255, 2 KiB stream buffer)
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], "TestPlayer");
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let msg = rig.command(&mut pipeline, &strm_frame(b's', true, 255, 0, 0, [0, 0, 0, 0], &[]), 10);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.buffer_threshold_bytes, 1024);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
    }

    #[test]
    fn test_autostart_false_transitions_to_paused() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // autostart = false with buffering
        coordinator.autostart = false;
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 512;

        coordinator.evaluate_state(&mut pipeline);
        assert_eq!(coordinator.state, PlayerState::Buffering);

        // Ingest enough data to satisfy threshold
        let data = [0x77u8; 512];
        pipeline.ingest_stream_data(&data).unwrap();

        coordinator.evaluate_state(&mut pipeline);
        // Because autostart is false, transitions to Paused
        assert_eq!(coordinator.state, PlayerState::Paused);
    }

    #[test]
    fn test_normalize_slimproto_pcm_wire_codes() {
        // 1. Sample rate decoding (both ASCII '0'..='9' and numeric 0..=9)
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'0'), Some(11025));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(0), Some(11025));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'1'), Some(22050));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(1), Some(22050));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'2'), Some(32000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(2), Some(32000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'3'), Some(44100));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(3), Some(44100));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'4'), Some(48000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(4), Some(48000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'5'), Some(8000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(5), Some(8000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'6'), Some(12000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(6), Some(12000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'7'), Some(16000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(7), Some(16000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'8'), Some(24000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(8), Some(24000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'9'), Some(96000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(9), Some(96000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b':'), Some(88200));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(10), Some(88200));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'x'), None);

        // 2. Sample size / bits normalization
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'0'), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(0), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(8), 8);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'1'), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(1), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(16), 16);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'2'), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(2), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(24), 24);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(b'3'), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(3), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(32), 32);
        assert_eq!(PlayerCoordinator::normalize_pcm_bits(99), 16); // Default fallback

        // 3. Channel count normalization
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(b'1'), 1);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(1), 1);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(b'2'), 2);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(2), 2);
        assert_eq!(PlayerCoordinator::normalize_pcm_channels(0), 2); // Default fallback

        // 4. Endianness normalization
        assert!(PlayerCoordinator::normalize_pcm_big_endian(b'0'));
        assert!(PlayerCoordinator::normalize_pcm_big_endian(0));
        assert!(!PlayerCoordinator::normalize_pcm_big_endian(b'1'));
        assert!(!PlayerCoordinator::normalize_pcm_big_endian(1));
    }

    #[test]
    fn test_cyd_command_pause_and_toggle_during_buffering() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1. CydCommand::Pause during Buffering
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 1024;
        pipeline.sink.set_mute(false);

        let msg = coordinator.handle_cyd_command(CydCommand::Pause, &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(coordinator.state, PlayerState::Paused);
        assert_eq!(coordinator.buffer_threshold_bytes, 0);
        assert!(pipeline.sink.muted);

        // 2. CydCommand::Toggle during Buffering
        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 2048;
        pipeline.sink.set_mute(false);

        let msg = coordinator.handle_cyd_command(CydCommand::Toggle, &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Paused)));
        assert_eq!(coordinator.state, PlayerState::Paused);
        assert_eq!(coordinator.buffer_threshold_bytes, 0);
        assert!(pipeline.sink.muted);

        // 3. CydCommand::Toggle while Paused transitions back to Playing
        let msg = coordinator.handle_cyd_command(CydCommand::Toggle, &mut pipeline);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(coordinator.state, PlayerState::Playing);
        assert!(!pipeline.sink.muted);
    }

    #[test]
    fn test_flush_resets_buffer_threshold_and_exits_buffering() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut rig = Rig::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        rig.coord.state = PlayerState::Buffering;
        rig.coord.buffer_threshold_bytes = 1024;

        let msg = rig.command(&mut pipeline, &simple_strm(b'f'), 10);
        assert_eq!(msg, None);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(rig.coord.buffer_threshold_bytes, 0);
        assert_eq!(rig.lines(), ["STAT:STMf"]);
    }

    #[test]
    fn test_coordinator_end_to_end_http_audio_streaming() {
        // 4 stereo frames of 16-bit big-endian PCM: (100,-100) (200,-200) (300,-300) (400,-400)
        static STREAM: Segments = &[
            OK_HEAD,
            b"\x00\x64\xff\x9c\x00\xc8\xff\x38\x01\x2c\xfe\xd4\x01\x90\xfe\x70",
        ];
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x88];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.pcm", STREAM));

        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 4096];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(
            b's',
            true,
            0,
            0,
            9000,
            [127, 0, 0, 1],
            b"GET /stream.pcm HTTP/1.0\r\nHost: 127.0.0.1:9000\r\n\r\n",
        );
        let cyd_msg = rig.command(&mut pipeline, &start, 100);
        assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.state, PlayerState::Playing);
        assert!(rig.coord.stream_active());
        assert_eq!(rig.src.inner.last_addr, Some(NetAddr { ip: [127, 0, 0, 1], port: 9000 }));

        // Tick until all 4 frames are decoded and recorded
        let mut total_frames = rig.coord.last_tick_frames();
        for _ in 0..100 {
            if total_frames >= 4 {
                break;
            }
            rig.tick(&mut pipeline, 100).unwrap();
            total_frames += rig.coord.last_tick_frames();
        }

        assert_eq!(total_frames, 4);
        let samples = pipeline.sink.recorded_samples();
        assert!(samples.len() >= 8);
        assert_eq!(samples[0], 100 << 16);
        assert_eq!(samples[1], -100 << 16);
        assert_eq!(samples[2], 200 << 16);
        assert_eq!(samples[3], -200 << 16);
        assert_eq!(samples[4], 300 << 16);
        assert_eq!(samples[5], -300 << 16);
        assert_eq!(samples[6], 400 << 16);
        assert_eq!(samples[7], -400 << 16);

        // Stop command drops the stream
        rig.command(&mut pipeline, &simple_strm(b'q'), 200);
        assert!(!rig.coord.stream_active());
        assert_eq!(rig.coord.state, PlayerState::Stopped);

        assert_eq!(rig.lines()[0], "STAT:STMc");
    }

    #[test]
    fn test_short_eof_stream_exits_buffering_and_emits_stms_stmu() {
        // COORD-501, COORD-502, COORD-504:
        // A short track smaller than threshold_kb that hits HTTP EOF must:
        // 1. Transition out of Buffering -> Playing and emit STMs once PCM is written
        // 2. Once all decoded PCM frames are played out, transition Playing -> Stopped
        // 3. Emit STMd, DSCO(0) and STMu
        let mac = [0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_data = true;

        let mut stream_storage = [0u8; 4096];
        let mut output_storage = [0u8; 4096];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // threshold_kb = 2 (2048 bytes) and empty http_headers (COORD-503 default request)
        let start = strm_frame(b's', true, 2, 0, 9000, [127, 0, 0, 1], &[]);
        rig.command(&mut pipeline, &start, 10);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        assert_eq!(rig.src.last_request, DEFAULT_HTTP_REQUEST);

        // The 2 frames (8 bytes, far below the threshold) arrive, then EOF
        rig.src.hold_data = false;
        for _ in 0..100 {
            if rig.coord.state == PlayerState::Stopped {
                break;
            }
            rig.tick(&mut pipeline, 20).unwrap();
        }

        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.coord.frames_played, 2);

        assert_eq!(
            rig.lines(),
            ["STAT:STMc", "RESP", "STAT:STMs", "STAT:STMd", "DSCO:0", "STAT:STMu"]
        );
        assert!(rig.sent[1].1.starts_with(b"HTTP/1.0 200 OK"));
    }

    #[test]
    fn test_audio_gain_u64_no_overflow_and_sync_cyd_telemetry() {
        // COORD-505, COORD-507: Verify u32::MAX gain does not overflow and sync_cyd_telemetry
        // returns PlaybackState, Volume, and Progress.
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x99];
        let mut rig = Rig::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 512];
        let mut output_storage = [0u8; 512];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let msg = rig.command(&mut pipeline, &audg_frame(u32::MAX, u32::MAX, 0), 50);
        assert_eq!(msg, Some(CydMessage::Volume(100)));
        assert_eq!(rig.backend.hw_vol, (0x30, 0x30)); // 0 dB

        rig.coord.state = PlayerState::Playing;
        pipeline.frames_played = 44100 * 12; // 12 seconds at 44.1 kHz
        let telemetry = rig.coord.sync_cyd_telemetry(&pipeline);
        assert_eq!(telemetry.len(), 3);
        assert_eq!(telemetry[0], CydMessage::PlaybackState(PlayState::Playing));
        assert_eq!(telemetry[1], CydMessage::Volume(100));
        assert_eq!(
            telemetry[2],
            CydMessage::Progress {
                elapsed_secs: 12,
                duration_secs: 0
            }
        );
    }

    #[test]
    fn test_latin1_headers_and_http_error_reports_unreachable() {
        // COORD-503, COORD-509: Non-UTF8 (Latin-1) headers are sent with each non-ASCII byte
        // replaced by '?'. An HTTP 404 answer arriving after the connect went pending is a
        // connect failure: STMn + DSCO(3), Stopped (no STMc).
        static NOT_FOUND: Segments = &[b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n"];
        let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/caf?.pcm", NOT_FOUND));
        rig.src.hold_connect = true; // the server has not answered yet

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Pre-populate pipeline with dirty state to verify Start flushes and resets frames_played (COORD-506, COORD-509)
        pipeline.ingest_stream_data(&[1, 2, 3, 4]).unwrap();
        pipeline.frames_played = 999;
        rig.coord.frames_played = 999;

        // Latin-1 invalid UTF-8 byte 0xE9 ('é') in HTTP request header (COORD-503)
        let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], b"GET /caf\xe9.pcm HTTP/1.0\r\n\r\n");
        rig.command(&mut pipeline, &start, 100);
        assert_eq!(pipeline.frames_played, 0);
        assert_eq!(rig.coord.frames_played, 0);
        assert_eq!(pipeline.stream_buffer_available(), 0);
        assert!(rig.coord.stream_active());
        assert_eq!(rig.src.last_request, b"GET /caf?.pcm HTTP/1.0\r\n\r\n");
        assert!(rig.sent.is_empty(), "STMc waits for the response");

        // The 404 answer arrives
        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 110).unwrap();
        assert!(!rig.coord.stream_active());
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(rig.coord.take_cyd_message(), Some(CydMessage::PlaybackState(PlayState::Stopped)));
        assert_eq!(rig.lines(), ["STAT:STMn", "DSCO:3", "DSCO:3"]);
    }

    #[test]
    fn test_poll_error_after_connect_drops_stream() {
        // COORD-508: a stream that connected and then fails while polling (truncated chunked
        // body) is dropped and marked ended; it is not a connect failure.
        static TRUNCATED: Segments =
            &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n\x01\x00\xff\x00"];
        let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x89];
        let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", TRUNCATED));

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for _ in 0..10 {
            if !rig.coord.stream_active() {
                break;
            }
            rig.tick(&mut pipeline, 20).unwrap();
        }
        assert!(!rig.coord.stream_active());
        assert!(rig.coord.is_stream_eof());
        assert_eq!(rig.coord.stream_bytes_received, 4);
        assert_eq!(rig.lines()[..2], ["STAT:STMc", "RESP"]);
        assert!(!rig.lines().contains(&"STAT:STMn".to_string()));
    }

    #[test]
    fn test_redirect_failure_is_connect_failure_immediately_or_after_wouldblock() {
        // A redirect whose target cannot be fetched fails the connect. Whether that surfaces on
        // the first `connect` call or after `WouldBlock`, the result is identical.
        static REDIRECT: Segments = &[b"HTTP/1.0 302 Found\r\nLocation: /gone\r\n\r\n"];
        let run = |hold_connect: bool| {
            let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x8A];
            let mut rig = Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/r", REDIRECT));
            rig.src.hold_connect = hold_connect;
            let mut stream_storage = [0u8; 1024];
            let mut output_storage = [0u8; 1024];
            let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

            let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], b"GET /r HTTP/1.0\r\n\r\n");
            let mut cyd = rig.command(&mut pipeline, &start, 10);
            if hold_connect {
                assert_eq!(cyd, Some(CydMessage::PlaybackState(PlayState::Playing)));
                assert!(rig.sent.is_empty());
                rig.src.hold_connect = false;
                rig.tick(&mut pipeline, 20).unwrap();
                cyd = rig.coord.take_cyd_message();
            }
            assert_eq!(rig.src.inner.redirects_followed, 1);
            assert_eq!(cyd, Some(CydMessage::PlaybackState(PlayState::Stopped)));
            assert_eq!(rig.coord.state, PlayerState::Stopped);
            assert!(!rig.coord.stream_active());
            assert!(!rig.coord.is_stream_eof());
            rig.lines()
        };
        let immediate = run(false);
        let resumed = run(true);
        assert_eq!(immediate, ["STAT:STMn", "DSCO:3", "DSCO:3"]);
        assert_eq!(resumed, immediate);
    }

    #[test]
    fn test_coordinator_stream_open_failure_emits_stmn_and_dsco() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x77];
        let mut rig = Rig::new(mac, "TestPlayer");
        rig.src.inner.fail_connect = true; // stream server unreachable

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(b's', true, 0, 0, 1, [127, 0, 0, 1], &[]);
        let msg = rig.command(&mut pipeline, &start, 100);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Stopped)));
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(!rig.coord.stream_active());

        // STMn and DSCO(Unreachable) are sent immediately; the same reason is also left in
        // pending_dsco, which the tick's heartbeat flushes as a second DSCO (pre-existing
        // behavior, pinned here).
        assert_eq!(rig.lines(), ["STAT:STMn", "DSCO:3", "DSCO:3"]);
        assert_eq!(rig.sent[1].1[0], DisconnectReason::Unreachable as u8);
        assert_eq!(rig.coord.pending_dsco, None);
    }

    #[test]
    fn test_coordinator_defers_stms_until_pcm_output_written() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xAA];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_data = true;

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]);
        let msg = rig.command(&mut pipeline, &start, 100);
        assert_eq!(msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
        assert_eq!(rig.coord.state, PlayerState::Playing);
        // STMs NOT emitted prematurely before PCM output is written
        assert!(!rig.coord.pending_track_started);
        assert_eq!(rig.lines(), ["STAT:STMc", "RESP"]);

        // PCM arrives: the tick that writes it to the sink emits the deferred STMs
        rig.src.hold_data = false;
        let mut written = 0;
        for _ in 0..100 {
            rig.tick(&mut pipeline, 150).unwrap();
            written = rig.coord.last_tick_frames();
            if written > 0 {
                break;
            }
        }
        assert!(written > 0);
        assert!(!rig.coord.pending_track_started);

        let lines = rig.lines();
        assert_eq!(lines[..3], ["STAT:STMc", "RESP", "STAT:STMs"]);
        assert!(rig.sent[1].1.starts_with(b"HTTP/1.0 200 OK"));
    }

    #[test]
    fn test_coordinator_eof_emits_stmd_then_stmu_after_output_drain() {
        // Steps the private stages one by one so the output buffer still holds frames when the
        // decoder finishes: STMd + DSCO(0) come first, STMu only after the output drains.
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xBB];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let frame = strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]);
        let (start_cmd, _) = SlimprotoParser::parse_frame(&frame).unwrap();
        rig.coord
            .handle_command(start_cmd, &mut rig.lms, &mut rig.src, &mut rig.backend, &mut pipeline, 10)
            .unwrap();

        // 1. Headers parsed and RESP available
        for _ in 0..100 {
            if rig.coord.pending_resp.is_some() {
                break;
            }
            let _ = rig.coord.poll_stream(&mut rig.src, &mut pipeline);
        }
        assert!(rig.coord.pending_resp.is_some());
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 20).unwrap();

        // 2. Decode stream into output buffer; wait until stream_buffer drains at EOF
        for _ in 0..100 {
            if rig.coord.decode_complete_sent {
                break;
            }
            let _ = rig.coord.poll_stream(&mut rig.src, &mut pipeline);
            let _ = pipeline.process_decode_step();
            rig.coord.evaluate_state(&mut pipeline);
        }

        assert!(rig.coord.decode_complete_sent);
        assert!(rig.coord.pending_decode_ready);
        // Output buffer still holds frames
        assert!(pipeline.output_buffer.available_read() > 0);
        assert_eq!(rig.coord.state, PlayerState::Playing);

        // check_heartbeat emits STMd and DSCO(0)
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 30).unwrap();

        // 3. Output buffer drains
        let written = pipeline.process_output_step().unwrap();
        assert_eq!(written, 2);
        assert_eq!(pipeline.output_buffer.available_read(), 0);

        rig.coord.evaluate_state(&mut pipeline);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert!(rig.coord.pending_underrun);

        // check_heartbeat emits STMu
        rig.coord.check_heartbeat(&mut rig.lms, &pipeline, 40).unwrap();
        assert!(!rig.coord.pending_underrun);

        rig.drain_sent();
        assert_eq!(rig.lines(), ["STAT:STMc", "RESP", "STAT:STMd", "DSCO:0", "STAT:STMu"]);
        assert!(rig.sent[1].1.starts_with(b"HTTP/1.0 200 OK"));
        assert_eq!(rig.sent[3].1[0], DisconnectReason::ConnectionClosed as u8);
    }

    #[test]
    fn test_coordinator_timestamp_probe_and_setd_aude_cont() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xCC];
        let mut rig = Rig::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1. strm 't' (Timestamp probe) with replay_gain = 0x12345678
        rig.command(&mut pipeline, &strm_frame(b't', false, 0, 0x1234_5678, 0, [0, 0, 0, 0], &[]), 100);

        // 2. setd (Device setting id = 0, update player name to "Kitchen")
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen"), 200);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");

        // 3. aude (Audio enable: spdif = 1, dac = 0 -> mute dac, amp off)
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 0]), 300);
        assert!(!rig.coord.dac_enabled);
        assert!(pipeline.sink.muted);
        assert!(!rig.backend.powered);

        // aude (dac = 1 -> unmute dac, amp on)
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 1]), 350);
        assert!(rig.coord.dac_enabled);
        assert!(!pipeline.sink.muted);
        assert!(rig.backend.powered);

        // 4. cont (Stream continue: releases awaiting_cont)
        rig.coord.awaiting_cont = true;
        rig.command(&mut pipeline, &server_frame(b"cont", &8192u32.to_be_bytes()), 400);
        assert!(!rig.coord.awaiting_cont);

        assert_eq!(rig.lines(), ["STAT:STMt", "SETD"]);
        // STAT with server_timestamp = 0x12345678
        assert_eq!(&rig.sent[0].1[47..51], &0x1234_5678u32.to_be_bytes());
        // SETD name reply with "Kitchen"
        assert_eq!(rig.sent[1].1[0], 0x00);
        assert_eq!(&rig.sent[1].1[1..], b"Kitchen");
    }

    #[test]
    fn test_setd_one_byte_query_preserves_sample_rate_and_replies_name() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xDD];
        let mut rig = Rig::new(mac, "Basement");
        assert_eq!(rig.coord.current_sample_rate, 44100);

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // 1-byte setd [0x00] query from LMS
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 100);

        assert_eq!(rig.coord.current_sample_rate, 44100);
        assert_eq!(rig.coord.player_name.as_str(), "Basement");

        // SETD name reply with "Basement"
        assert_eq!(rig.lines(), ["SETD"]);
        assert_eq!(rig.sent[0].1[0], 0x00);
        assert_eq!(&rig.sent[0].1[1..], b"Basement");
    }

    #[test]
    fn test_coordinator_audg_digital_atten_zero_unity_gain_fallback() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xEE];
        let mut rig = Rig::new(mac, "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Initially unity gain (65536, 65536)
        assert_eq!(rig.coord.volume.gain_left, 65536);
        assert_eq!(rig.coord.volume.gain_right, 65536);

        // When digital_atten == 0 and incoming gain is 0,0 while old gain was > 0, set unity gain (65536)
        rig.command(&mut pipeline, &audg_frame(0, 0, 0), 100);
        assert_eq!(rig.coord.volume.gain_left, 65536);
        assert_eq!(rig.coord.volume.gain_right, 65536);
        assert_eq!(rig.backend.hw_vol, (0xFF, 0xFF));

        // 4-byte player name [0, 'B', 'o', 'b'] updates player_name and replies with SETD without altering current_sample_rate
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00, b'B', b'o', b'b']), 200);
        assert_eq!(rig.coord.player_name.as_str(), "Bob");
        assert_eq!(rig.coord.current_sample_rate, 44100);

        // 1-byte id=4 DAC query does not alter current_sample_rate
        rig.command(&mut pipeline, &server_frame(b"setd", &[4]), 300);
        assert_eq!(rig.coord.current_sample_rate, 44100);
        assert_eq!(rig.lines(), ["SETD"]);

        // Sample rate decoding for 176.4k and 192k
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b';'), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(11), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'<'), Some(192000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(12), Some(192000));

        // Stream EOF tracking and reset
        rig.coord.set_stream_eof(true);
        assert!(rig.coord.is_stream_eof());
        rig.coord.reset();
        assert!(!rig.coord.is_stream_eof());
    }

    #[test]
    fn test_audg_hardware_volume_policy() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x04], "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // Unequal channels: the louder one sets both TAS channels; software gain unity.
        // 1842 = LMS 51% slider (-31 dB) -> 55% -> 0x5D (-22.5 dB)
        let msg = rig.command(&mut pipeline, &audg_frame(1842, 900, 1), 10);
        assert_eq!(msg, Some(CydMessage::Volume(55)));
        assert_eq!(rig.backend.hw_vol, (0x5D, 0x5D));
        assert_eq!(pipeline.volume.gain_left, 65536);
        assert_eq!(pipeline.volume.gain_right, 65536);

        // Zero gain: amplifier muted, software gain 0
        let msg = rig.command(&mut pipeline, &audg_frame(0, 0, 1), 20);
        assert_eq!(msg, Some(CydMessage::Volume(0)));
        assert_eq!(rig.backend.hw_vol, (0xFF, 0xFF));
        assert_eq!((rig.coord.volume.gain_left, rig.coord.volume.gain_right), (0, 0));
    }

    #[test]
    fn test_backend_receives_rate_and_keeps_going_on_unsupported_rate() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x01], "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // setd with a 4-byte sample rate: 48 kHz reaches the clock
        rig.command(&mut pipeline, &server_frame(b"setd", &48_000u32.to_be_bytes()), 10);
        assert_eq!(rig.coord.current_sample_rate, 48_000);
        assert_eq!(pipeline.sample_rate, 48_000);
        assert_eq!(rig.backend.rate, 48_000);

        // A rate the clock refuses keeps the previous clock; the coordinator state still
        // follows the server (no STMn yet).
        rig.command(&mut pipeline, &server_frame(b"setd", &12_345u32.to_be_bytes()), 20);
        assert_eq!(rig.coord.current_sample_rate, 12_345);
        assert_eq!(rig.backend.rate, 48_000);
        assert!(rig.sent.is_empty());
    }

    #[test]
    fn test_pending_connect_resumes_on_later_tick() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x02];
        let mut rig =
            Rig::with_source(mac, "TestPlayer", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        rig.src.hold_connect = true;

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        let start = strm_frame(b's', true, 0, 0, 9000, [0, 0, 0, 0], &[]);
        rig.command(&mut pipeline, &start, 10);
        assert!(rig.coord.stream_active());
        assert!(rig.sent.is_empty(), "STMc waits for the response");
        // server_ip 0.0.0.0 streams from the LMS address
        assert_eq!(rig.src.last_request, DEFAULT_HTTP_REQUEST);

        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.src.inner.connects, 0);

        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 30).unwrap();
        assert_eq!(rig.lines(), ["STAT:STMc", "RESP"]);
        assert_eq!(rig.src.inner.connects, 1);
        assert_eq!(rig.src.inner.last_addr, Some(NetAddr { ip: [127, 0, 0, 1], port: 9000 }));
        for _ in 0..10 {
            rig.tick(&mut pipeline, 40).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.lines()[..3], ["STAT:STMc", "RESP", "STAT:STMs"]);
    }

    // -----------------------------------------------------------------------------------------
    // Task 9: device path through the coordinator (pumps, decoder provider, codc, STMl, ...)
    // -----------------------------------------------------------------------------------------

    /// `strm_frame` with another format byte.
    fn strm_frame_fmt(format: u8, autostart: bool, server_port: u16) -> Vec<u8> {
        let mut f = strm_frame(b's', autostart, 0, 0, server_port, [127, 0, 0, 1], &[]);
        f[8 + 2] = format;
        f
    }

    #[test]
    fn test_device_pumps_play_audio_and_stms_is_reported_once() {
        let mut rig = Rig::with_source([1; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.src.hold_data = true;
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        // The device feeds the stream ring outside the tick, then pumps decode/output itself.
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.coord.pump_decode(&mut pipeline, 4);
        assert_eq!(rig.coord.pump_output(&mut pipeline, 16), 2);
        assert_eq!(pipeline.frames_played, 2);
        rig.tick(&mut pipeline, 20).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        let stms = rig.lines().iter().filter(|l| *l == "STAT:STMs").count();
        assert_eq!(stms, 1, "{:?}", rig.lines());
        // Output stays gated by the play state and the DAC enable.
        rig.coord.dac_enabled = false;
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.coord.pump_decode(&mut pipeline, 4);
        assert_eq!(rig.coord.pump_output(&mut pipeline, 16), 0);
    }

    static TRUNCATED_TAIL: Segments = &[OK_HEAD, TWO_FRAMES, &[0x03, 0x00, 0xFD]];

    #[test]
    fn test_eof_with_undecodable_tail_still_ends_the_track() {
        // A stream ending in a partial frame used to leave the tail in the stream ring forever
        // (no STMd/STMu). The device firmware dropped it; the coordinator now does too.
        let mut rig = Rig::with_source([2; 6], "T", GatedSource::new(b"/stream.mp3", TRUNCATED_TAIL));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..20 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        assert_eq!(
            rig.lines(),
            ["STAT:STMc", "RESP", "STAT:STMs", "STAT:STMd", "DSCO:0", "STAT:STMu"]
        );
    }

    #[test]
    fn test_pump_decode_drops_undecodable_tail_after_eof() {
        let mut rig = Rig::with_source([3; 6], "T", GatedSource::new(b"/stream.mp3", TRUNCATED_TAIL));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        rig.src.hold_data = false;
        for _ in 0..10 {
            let _ = rig.coord.poll_stream(&mut rig.src, &mut pipeline);
        }
        assert!(rig.coord.stream_eof);
        for _ in 0..4 {
            rig.coord.pump_decode(&mut pipeline, 4);
            rig.coord.pump_output(&mut pipeline, 16);
        }
        assert_eq!(pipeline.stream_buffer.available_read(), 0, "partial frame dropped");
    }

    #[test]
    fn test_codc_selects_the_decoder() {
        let mut rig = Rig::new([4; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 10);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Flac(_)));
        assert_eq!(rig.coord.current_format, Some(AudioFormat::Flac));
        rig.command(&mut pipeline, &server_frame(b"codc", b"m"), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));
        // PCM: format, sample size, rate, channels, endianness ('1' = little endian).
        rig.command(&mut pipeline, &server_frame(b"codc", b"p231\x01"), 30);
        match &pipeline.decoder {
            ActiveDecoder::Pcm(d) => {
                assert_eq!((d.bits_per_sample, d.channels, d.big_endian), (24, 1, false));
            }
            _ => panic!("PCM decoder expected"),
        }
        assert!(rig.lines().is_empty(), "codc has no reply");
    }

    #[test]
    fn test_unsupported_format_installs_pcm_muted_until_next_track() {
        let mut rig = Rig::new([5; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 10);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));
        // Opus is not decodable: never feed its bytes to the previous (MP3) decoder.
        rig.command(&mut pipeline, &strm_frame_fmt(b'u', true, 0), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));
        assert!(pipeline.user_muted);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.tick(&mut pipeline, 30).unwrap();
        assert!(pipeline.sink.muted, "output stays muted");
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 40);
        assert!(!pipeline.user_muted, "a supported track clears the mute");
    }

    #[test]
    fn test_mp3_decoder_comes_from_the_pipeline_slot_and_returns_to_it() {
        // Firmware builds have no heap MP3 decoder: bring-up parks the static one in the
        // pipeline's MP3 slot. The coordinator must use (and give back) that very decoder.
        let mut rig = Rig::new([6; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let parked = Box::new(squeezelite_decoders::Mp3Decoder::new());
        let id: *const squeezelite_decoders::Mp3Decoder = &*parked;
        pipeline.mp3_spare = Some(parked);
        let current_mp3 = |p: &TestPipeline<'_>| match &p.decoder {
            ActiveDecoder::Mp3(d) => &**d as *const squeezelite_decoders::Mp3Decoder,
            _ => core::ptr::null(),
        };

        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 10);
        assert_eq!(current_mp3(&pipeline), id, "the parked decoder is used");
        assert!(pipeline.mp3_spare.is_none());

        rig.command(&mut pipeline, &strm_frame_fmt(b'f', true, 0), 20);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Flac(_)));
        assert_eq!(pipeline.mp3_spare.as_deref().map(|d| d as *const _), Some(id), "parked again");

        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 30);
        rig.command(&mut pipeline, &strm_frame_fmt(b'm', true, 0), 40);
        assert_eq!(current_mp3(&pipeline), id, "MP3 after MP3 keeps the same decoder");
    }

    #[test]
    fn test_hello_sends_helo_and_setd_and_arms_the_first_heartbeat() {
        let mut rig = Rig::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF], "Kitchen");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.coord.last_heartbeat_ms = 5_000;
        rig.coord.hello(&mut rig.lms, 12, "Model=squeezeesp32,flc,pcm", 5_100).unwrap();
        rig.drain_sent();
        assert_eq!(rig.lines(), ["HELO", "SETD"]);
        assert_eq!(&rig.sent[0].1[2..8], &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert!(rig.sent[0].1.ends_with(b"Model=squeezeesp32,flc,pcm"));
        assert_eq!(&rig.sent[1].1[1..], b"Kitchen");
        assert_eq!(rig.coord.state, PlayerState::Connected);
        rig.tick(&mut pipeline, 5_101).unwrap();
        assert_eq!(rig.lines()[2], "STAT:STMt", "heartbeat right after HELO");
    }

    #[test]
    fn test_resp_waits_for_room_instead_of_being_dropped() {
        let mut rig = Rig::with_source([7; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.src.hold_data = true;
        rig.lms.inject_frame(&strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[])).unwrap();
        // Fill the send queue so everything after STMc would block.
        for _ in 0..LOOPBACK_QUEUE_DEPTH - 1 {
            rig.lms.send(b"FILL\0\0\0\0").unwrap();
        }
        let _ = rig.coord.tick(&mut rig.lms, &mut rig.src, &mut rig.backend, &mut pipeline, 10);
        assert!(rig.coord.pending_resp.is_some(), "RESP kept for the next tick");
        rig.drain_sent();
        rig.sent.clear();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.lines()[0], "RESP");
    }

    #[test]
    fn test_non_autostart_stream_sends_stml_after_resp() {
        let mut rig = Rig::with_source([8; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', false, 1, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..4 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        let lines = rig.lines();
        assert_eq!(lines[..3], ["STAT:STMc", "RESP", "STAT:STMl"], "{lines:?}");
        assert_eq!(lines.iter().filter(|l| *l == "STAT:STMl").count(), 1);
    }

    #[test]
    fn test_cont_wait_holds_the_body_then_releases_it_with_metaint_and_stml() {
        static ICY_BODY: Segments = &[OK_HEAD, b"\x01\x00\xFF\x00\x01Hi\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x02\x00\xFE\x00"];
        let mut rig = Rig::with_source([9; 6], "T", GatedSource::new(b"/stream.mp3", ICY_BODY));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        let mut start = strm_frame(b's', false, 0, 0, 9000, [127, 0, 0, 1], &[]);
        start[8 + 11] = squeezelite_proto::STRM_FLAG_CONT_WAIT; // flags
        rig.command(&mut pipeline, &start, 10);
        assert!(rig.coord.awaiting_cont);
        for t in 0..4 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(pipeline.stream_buffer.available_read(), 0, "no body before cont");
        assert_eq!(rig.lines(), ["STAT:STMc", "RESP"]);
        rig.command(&mut pipeline, &server_frame(b"cont", &4u32.to_be_bytes()), 30);
        for t in 0..4 {
            rig.tick(&mut pipeline, 40 + t).unwrap();
        }
        assert_eq!(rig.coord.stream_bytes_received, 8, "ICY block stripped from the start");
        assert!(rig.lines().contains(&"STAT:STMl".to_string()), "{:?}", rig.lines());
        assert!(rig.lines().contains(&"META".to_string()));
    }

    #[test]
    fn test_track_start_powers_the_amplifier_when_the_dac_is_enabled() {
        let mut rig = Rig::new([10; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 10);
        assert!(rig.backend.powered);
        rig.command(&mut pipeline, &server_frame(b"aude", &[1, 0]), 20);
        rig.command(&mut pipeline, &strm_frame_fmt(b'p', true, 0), 30);
        assert!(!rig.backend.powered, "DAC disabled: track start does not power the amp");
    }

    #[test]
    fn test_external_playback_reports_nothing_to_lms() {
        let mut rig = Rig::with_source([11; 6], "T", GatedSource::new(b"/stream.mp3", SHORT_STREAM));
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        // An LMS track plays to the end (STMd ... STMu leave per-track flags behind).
        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 9000, [127, 0, 0, 1], &[]), 10);
        for t in 0..20 {
            rig.tick(&mut pipeline, 20 + t).unwrap();
        }
        assert_eq!(rig.coord.state, PlayerState::Stopped);
        rig.sent.clear();
        // Bluetooth takes over the pipeline.
        rig.coord.begin_external_playback();
        pipeline.flush();
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        for t in 0..10 {
            rig.coord.pump_decode(&mut pipeline, 4);
            rig.coord.pump_output(&mut pipeline, 16);
            rig.tick(&mut pipeline, 100 + t).unwrap();
        }
        assert_eq!(rig.coord.state, PlayerState::Playing, "no LMS end-of-track logic");
        assert!(rig.lines().iter().all(|l| l == "STAT:STMt"), "{:?}", rig.lines());
        rig.coord.end_external_playback();
        assert_eq!(rig.coord.state, PlayerState::Stopped);
    }

    #[test]
    fn test_external_frame_buffer_receives_frames_and_bounds_their_size() {
        // The firmware hands the coordinator a static frame buffer (off the task stack).
        let mut rig = Rig::new([12; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.coord.set_frame_buffer(Box::leak(Box::new([0u8; 64])));
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Den"), 10);
        assert_eq!(rig.coord.player_name.as_str(), "Den");
        // A frame larger than the buffer is dropped by the transport (Io) and skipped.
        let long_name = [b'x'; 80];
        let mut payload = vec![0u8];
        payload.extend_from_slice(&long_name);
        rig.command(&mut pipeline, &server_frame(b"setd", &payload), 20);
        assert_eq!(rig.coord.player_name.as_str(), "Den");
        assert_eq!(rig.lines(), ["SETD"]);
    }

    #[test]
    fn test_external_audio_pump_keeps_decode_and_output_out_of_tick() {
        // Device: the pump phases own decode/output; tick must not play audio on its own,
        // but state transitions (Buffering -> Playing, STMs from pumped output) still happen.
        let mut rig = Rig::new([13; 6], "T");
        rig.coord.external_audio_pump = true;
        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 2048];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &strm_frame(b's', true, 1, 0, 0, [0, 0, 0, 0], &[]), 10);
        assert_eq!(rig.coord.state, PlayerState::Buffering);
        pipeline.ingest_stream_data(&[0u8; 1100]).unwrap();
        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.coord.state, PlayerState::Playing, "threshold reached in tick");
        assert_eq!(pipeline.frames_played, 0, "tick wrote nothing to the sink");
        assert_eq!(pipeline.sink.recorded_samples().len(), 0);
        assert_eq!(pipeline.stream_buffer.available_read(), 1100, "tick decoded nothing");
        rig.coord.pump_decode(&mut pipeline, 4);
        assert!(rig.coord.pump_output(&mut pipeline, 16) > 0);
        rig.tick(&mut pipeline, 30).unwrap();
        assert!(rig.lines().contains(&"STAT:STMs".to_string()));
    }

    #[test]
    fn test_setd_name_is_cut_at_the_first_nul() {
        let mut rig = Rig::new([14; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen\x00\x00junk"), 10);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");
        assert_eq!(&rig.sent[0].1[1..], b"Kitchen", "no NUL echoed back");
        // A name that is empty before its NUL keeps the current name.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00\x00"), 20);
        assert_eq!(rig.coord.player_name.as_str(), "Kitchen");
    }

    #[test]
    fn test_setd_name_change_persists_trimmed_name_through_config_handle() {
        use crate::config_handle::ConfigHandle;
        let config: &'static ConfigHandle =
            Box::leak(Box::new(ConfigHandle::new(squeezelite_config::AudioConfig::default())));
        let mut rig = Rig::new([16; 6], "T");
        rig.coord.config = Some(config);
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        // id-0 set: the NUL-trimmed name reaches the handle and marks it dirty.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen\x00\x00junk"), 10);
        assert_eq!(config.read(|c| c.player_name.clone()).as_str(), "Kitchen");
        assert!(config.is_dirty());
        assert!(config.snapshot_if_dirty().is_some());
        config.mark_clean();

        // id-0 query: replies, persists nothing.
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00]), 20);
        assert!(!config.is_dirty());
        // The same name again is not a change.
        rig.command(&mut pipeline, &server_frame(b"setd", b"\x00Kitchen"), 30);
        assert!(!config.is_dirty());

        // The 4-byte (`SetDevice`) name form persists too.
        rig.command(&mut pipeline, &server_frame(b"setd", &[0x00, b'B', b'o', b'b']), 40);
        assert_eq!(rig.coord.player_name.as_str(), "Bob");
        assert_eq!(config.read(|c| c.player_name.clone()).as_str(), "Bob");
        assert!(config.is_dirty());
    }

    #[test]
    fn test_codc_reselecting_flac_flushes_the_decoder() {
        let mut rig = Rig::new([15; 6], "T");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 10);
        let id = match &mut pipeline.decoder {
            ActiveDecoder::Flac(d) => {
                d.pending_pcm.extend_from_slice(&[1, 2, 3, 4]);
                d.stream_buf.extend_from_slice(b"stale");
                &**d as *const squeezelite_decoders::FlacDecoder
            }
            _ => panic!("FLAC expected"),
        };
        rig.command(&mut pipeline, &server_frame(b"codc", b"f"), 20);
        match &pipeline.decoder {
            ActiveDecoder::Flac(d) => {
                assert_eq!(&**d as *const _, id, "same decoder kept");
                assert!(d.pending_pcm.is_empty() && d.stream_buf.is_empty(), "and flushed");
            }
            _ => panic!("FLAC expected"),
        }
    }

    /// Transport whose send path refuses frames over `max` bytes (like a 2 KB socket buffer).
    struct SmallTx {
        inner: LoopbackTransport,
        max: usize,
    }

    impl SlimTransport for SmallTx {
        fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
            self.inner.poll_frame(buf)
        }
        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            if frame.len() > self.max {
                return Err(TransportError::Io);
            }
            self.inner.send(frame)
        }
        fn connected(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_oversized_resp_is_sent_truncated_at_a_line_boundary() {
        let mut head = b"HTTP/1.0 200 OK\r\n".to_vec();
        for i in 0..120 {
            head.extend_from_slice(format!("X-Pad-{i:03}: {}E\r\n", "v".repeat(20)).as_bytes());
        }
        head.extend_from_slice(b"\r\n");
        assert!(head.len() > 3000);
        let mut coord = PlayerCoordinator::new([16; 6], "T");
        coord.last_heartbeat_ms = 0;
        coord.pending_resp = Some(head.clone());
        let mut tx = SmallTx { inner: LoopbackTransport::new(), max: 2048 };
        let mut stream_storage = [0u8; 256];
        let mut output_storage = [0u8; 256];
        let pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);
        coord.check_heartbeat(&mut tx, &pipeline, 10).unwrap();
        assert!(coord.pending_resp.is_none());
        let frame = tx.inner.pop_sent().expect("RESP sent");
        assert_eq!(&frame[..4], b"RESP");
        let payload = &frame[8..];
        assert!(frame.len() <= 2048);
        assert!(payload.ends_with(b"E\r\n\r\n"), "cut after a whole header line, blank line added");
        assert!(head.starts_with(&payload[..payload.len() - 2]));
    }

    #[test]
    fn test_tick_reports_disconnect_but_keeps_playing() {
        let mut rig = Rig::new([0x11, 0x22, 0x33, 0x44, 0x55, 0x03], "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let mut pipeline = pcm_pipeline(&mut stream_storage, &mut output_storage);

        rig.command(&mut pipeline, &strm_frame(b's', true, 0, 0, 0, [0, 0, 0, 0], &[]), 10);
        pipeline.ingest_stream_data(TWO_FRAMES).unwrap();
        rig.lms.disconnect();

        assert_eq!(rig.tick(&mut pipeline, 20), Err(TransportError::Disconnected));
        assert_eq!(rig.coord.last_tick_frames(), 2);
        // The deferred STMs stays pending for the reconnected link
        assert!(rig.coord.pending_track_started);
    }
}
