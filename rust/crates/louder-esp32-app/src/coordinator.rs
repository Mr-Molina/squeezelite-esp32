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
use squeezelite_decoders::PcmDecoder;
use squeezelite_proto::{
    sample_rate_from_index, AudioFormat, AutostartMode, DisconnectReason, Endianness,
    ServerCommand, SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent, StreamCommandType,
    STRM_FLAG_SERVER_REBOOT,
};

use crate::amp::lms_gain_to_tas5805m_vol_reg;
use crate::io_traits::{
    AudioBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
};
use crate::pipeline::{ActiveDecoder, StreamPipeline};

/// Size of the stack buffer [`PlayerCoordinator::tick`] receives one server frame into. A larger
/// frame is dropped by the transport (reported as [`TransportError::Io`] and skipped).
pub const SERVER_FRAME_BUF_LEN: usize = 2048;

/// HTTP request sent when a `strm s` carries no request headers (`COORD-503`).
const DEFAULT_HTTP_REQUEST: &[u8] = b"GET /stream.mp3 HTTP/1.0\r\n\r\n";

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
    stream: StreamLink,
    pending_cyd: Option<CydMessage>,
    last_tick_frames: usize,
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
            pending_resp: None,
            pending_meta: None,
            dac_enabled: true,
            server_rebooting: false,
            player_name,
            stream_eof: false,
            lms_ip: [127, 0, 0, 1],
            stream: StreamLink::Idle,
            pending_cyd: None,
            last_tick_frames: 0,
        }
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
    /// 1. resumes a pending stream `connect` (one that returned [`StreamError::WouldBlock`]);
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
        self.resume_stream_connect(source);

        let mut link_lost = false;
        let mut frame = [0u8; SERVER_FRAME_BUF_LEN];
        match transport.poll_frame(&mut frame) {
            Ok(Some(len)) => {
                if let Ok((cmd, _)) = SlimprotoParser::parse_frame(&frame[..len]) {
                    if let Some(msg) =
                        self.handle_command(cmd, transport, source, backend, pipeline, now_ms)?
                    {
                        self.pending_cyd = Some(msg);
                    }
                }
            }
            Ok(None) | Err(TransportError::WouldBlock) | Err(TransportError::Io) => {}
            Err(TransportError::Disconnected) => link_lost = true,
        }

        let _ = self.poll_stream(source, pipeline);
        self.last_tick_frames = self.process_audio_step(pipeline);

        if link_lost {
            return Err(TransportError::Disconnected);
        }
        self.check_heartbeat(transport, pipeline, now_ms)
    }

    /// Repeats a `connect` that previously returned [`StreamError::WouldBlock`].
    fn resume_stream_connect<R: StreamSource>(&mut self, source: &mut R) {
        let result = match &self.stream {
            StreamLink::Connecting { addr, request } => source.connect(*addr, request),
            _ => return,
        };
        match result {
            Ok(()) => self.stream = StreamLink::Open,
            Err(StreamError::WouldBlock) => {}
            // The request was sent but the response failed (bad status, closed, too many
            // redirects): the stream ends, as when polling fails (`COORD-508`).
            Err(_) => self.fail_stream(source),
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
                        self.stream_bytes_received = 0;
                        self.close_stream(source);
                        self.stream_eof = false;

                        self.autostart_mode = AutostartMode::from_strm(autostart, flags);
                        self.autostart = self.autostart_mode.is_auto();
                        self.awaiting_cont = self.autostart_mode.requires_cont();
                        self.server_rebooting = (flags & STRM_FLAG_SERVER_REBOOT) != 0;

                        if server_port > 0 {
                            let ip = if server_ip == [0, 0, 0, 0] { self.lms_ip } else { server_ip };
                            let addr = NetAddr { ip, port: server_port };
                            let request = Self::build_http_request(http_headers);

                            match source.connect(addr, &request) {
                                Err(StreamError::ConnectFailed) => {
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
                                    send_dsco(transport, DisconnectReason::Unreachable)?;
                                    return Ok(Some(CydMessage::PlaybackState(PlayState::Stopped)));
                                }
                                result => {
                                    // The connection to the stream server is up (`STMc`); the
                                    // response may still be pending or may already have failed.
                                    match result {
                                        Ok(()) => self.stream = StreamLink::Open,
                                        Err(StreamError::WouldBlock) => {
                                            self.stream = StreamLink::Connecting { addr, request };
                                        }
                                        Err(_) => self.fail_stream(source),
                                    }
                                    send_stat(
                                        transport,
                                        StatEvent::Connected,
                                        pipeline.stream_buffer_available() as u32,
                                        pipeline.output_buffer_available() as u32,
                                        pipeline.frames_played as u32,
                                        now_ms,
                                    )?;
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

                        // Reconfigure pipeline decoder according to format. A codec missing from
                        // this build (see `std-codecs`) leaves the current decoder in place.
                        let decoder = match format {
                            AudioFormat::Mp3 => ActiveDecoder::new_mp3(),
                            AudioFormat::Flac => Some(ActiveDecoder::new_flac()),
                            AudioFormat::Aac => ActiveDecoder::new_aac(),
                            AudioFormat::Ogg => ActiveDecoder::new_vorbis(),
                            _ => Some(ActiveDecoder::Pcm(PcmDecoder::with_config(bits, is_be, channels))),
                        };
                        if let Some(decoder) = decoder {
                            pipeline.set_decoder(decoder);
                        }

                        let _ = pipeline.set_sample_rate(sample_rate);
                        // An unsupported rate keeps the current clock for now (the STMn reply
                        // to an unsupported rate is a separate behavior change).
                        let _ = backend.set_sample_rate(sample_rate);

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
                let (eff_gain_left, eff_gain_right) = if digital_atten == 0 {
                    if gain_left == 0
                        && gain_right == 0
                        && self.volume.gain_left > 0
                        && self.volume.gain_right > 0
                    {
                        (65536, 65536)
                    } else {
                        let l = if gain_left > 0 { 65536 } else { 0 };
                        let r = if gain_right > 0 { 65536 } else { 0 };
                        (l, r)
                    }
                } else {
                    (gain_left, gain_right)
                };
                self.volume.gain_left = eff_gain_left;
                self.volume.gain_right = eff_gain_right;
                pipeline.set_volume(eff_gain_left, eff_gain_right);
                // The amplifier tracks the server's requested gain (TAS 0x4C/0x4D).
                backend.set_hw_volume(
                    lms_gain_to_tas5805m_vol_reg(gain_left),
                    lms_gain_to_tas5805m_vol_reg(gain_right),
                );

                // COORD-507: Map 16.16 fixed-point (65536 = 100%) to 0-100 percentage using u64 to prevent u32 overflow
                let avg_gain = ((eff_gain_left as u64 + eff_gain_right as u64) / 2) as u32;
                let pct = core::cmp::min(100, ((avg_gain as u64 * 100) / 65536).min(100) as u8);

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
                        self.player_name.clear();
                        let _ = self.player_name.push_str(name);
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
            ServerCommand::Unknown { opcode, payload: _ } => {
                if let Some((_spdif, dac_enable)) = cmd.audio_enable() {
                    self.dac_enabled = dac_enable;
                    pipeline.sink.set_mute(!dac_enable);
                    backend.set_power(dac_enable);
                } else if let Some(metaint) = cmd.stream_continue() {
                    self.awaiting_cont = false;
                    if metaint > 0 && self.stream_active() {
                        source.set_icy_metaint(metaint as usize);
                    }
                } else if let Some(setting) = cmd.device_setting() {
                    if setting.id == 0 {
                        if !setting.data.is_empty() {
                            if let Ok(name) = core::str::from_utf8(setting.data) {
                                self.player_name.clear();
                                let _ = self.player_name.push_str(name);
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

    /// Collects the response head and ICY metadata the source has captured.
    fn take_stream_side_data<R: StreamSource>(&mut self, source: &mut R) {
        if let Some(headers) = source.take_response_headers() {
            self.pending_resp = Some(headers.to_vec());
        }
        if let Some(meta) = source.take_icy_metadata() {
            self.pending_meta = Some(meta.to_vec());
        }
    }

    /// Moves HTTP audio bytes from `source` into the pipeline stream buffer.
    ///
    /// Stops when no bytes are available right now (`HALNET-405`) or the stream buffer is full,
    /// records a clean end of stream in `stream_eof`, and drops the stream (also setting
    /// `stream_eof`) when `poll_bytes` fails (`COORD-508`).
    fn poll_stream<S: AudioOutputSink, R: StreamSource>(
        &mut self,
        source: &mut R,
        pipeline: &mut StreamPipeline<'_, S>,
    ) -> Result<usize, StreamError> {
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
        let _ = pipeline.process_decode_step();

        // Write frames to the output sink
        let is_first_write = self.frames_played == 0;
        let written = pipeline.process_output_step().unwrap_or(0);
        if written > 0 && is_first_write {
            self.pending_track_started = true;
        }
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
        if let Some(resp) = self.pending_resp.take() {
            send_payload_frame(transport, &resp, SlimprotoBuilder::build_resp)?;
        }

        if let Some(meta) = self.pending_meta.take() {
            send_payload_frame(transport, &meta, SlimprotoBuilder::build_meta)?;
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
    use crate::io_traits::{FakeStreamSource, LoopbackTransport, MockBackend, Segments};
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

        fn take_response_headers(&mut self) -> Option<heapless::Vec<u8, 1024>> {
            self.inner.take_response_headers()
        }

        fn take_icy_metadata(&mut self) -> Option<heapless::Vec<u8, 256>> {
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

        // Volume adjustment with digital_atten: 1 (50%)
        let cyd_vol = rig.command(&mut pipeline, &audg_frame(32768, 32768, 1), 200);
        assert_eq!(cyd_vol, Some(CydMessage::Volume(50)));
        let reg = lms_gain_to_tas5805m_vol_reg(32768);
        assert_eq!(rig.backend.hw_vol, (reg, reg));

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
    fn test_latin1_headers_and_poll_http_error_drops_stream() {
        // COORD-503, COORD-508, COORD-509:
        // Non-UTF8 (Latin-1) headers are sent with each non-ASCII byte replaced by '?', and an
        // HTTP 404 answer drops the stream and ends it.
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

        // The 404 answer arrives: the stream is dropped (COORD-508)
        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 110).unwrap();
        assert!(!rig.coord.stream_active());
        assert!(rig.coord.is_stream_eof());

        assert_eq!(rig.lines()[0], "STAT:STMc");
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
        assert_eq!(rig.lines(), ["STAT:STMc"]);
        // server_ip 0.0.0.0 streams from the LMS address
        assert_eq!(rig.src.last_request, DEFAULT_HTTP_REQUEST);

        rig.tick(&mut pipeline, 20).unwrap();
        assert_eq!(rig.src.inner.connects, 0);

        rig.src.hold_connect = false;
        rig.tick(&mut pipeline, 30).unwrap();
        assert_eq!(rig.src.inner.connects, 1);
        assert_eq!(rig.src.inner.last_addr, Some(NetAddr { ip: [127, 0, 0, 1], port: 9000 }));
        for _ in 0..10 {
            rig.tick(&mut pipeline, 40).unwrap();
        }
        assert_eq!(pipeline.frames_played, 2);
        assert_eq!(rig.lines()[..3], ["STAT:STMc", "RESP", "STAT:STMs"]);
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
