//! Player Coordinator
//!
//! Orchestrates the SlimProto network client, audio decoding pipeline,
//! hardware amplifier state, and CYD display status updates.

use squeezelite_audio_core::output::AudioOutputSink;
use squeezelite_audio_core::VolumeScale;
use squeezelite_cyd_link::{CydCommand, CydMessage, PlayState};
use squeezelite_decoders::PcmDecoder;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_decoders::{AacDecoder, FlacDecoder, Mp3Decoder, VorbisDecoder};
use squeezelite_proto::{AudioFormat, AutostartMode, DisconnectReason};
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_proto::{
    sample_rate_from_index, Endianness, ServerCommand, StatEvent, StreamCommandType,
    STRM_FLAG_SERVER_REBOOT,
};

#[cfg(not(target_arch = "xtensa"))]
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[cfg(not(target_arch = "xtensa"))]
use crate::network::{HttpAudioStream, SlimprotoClient};
#[cfg(not(target_arch = "xtensa"))]
use crate::pipeline::ActiveDecoder;
use crate::pipeline::StreamPipeline;

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
    #[cfg(not(target_arch = "xtensa"))]
    pub pending_resp: Option<std::vec::Vec<u8>>,
    #[cfg(not(target_arch = "xtensa"))]
    pub pending_meta: Option<std::vec::Vec<u8>>,
    pub dac_enabled: bool,
    pub server_rebooting: bool,
    pub player_name: heapless::String<64>,
    pub stream_eof: bool,
    #[cfg(not(target_arch = "xtensa"))]
    pub audio_stream: Option<HttpAudioStream>,
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
            #[cfg(not(target_arch = "xtensa"))]
            pending_resp: None,
            #[cfg(not(target_arch = "xtensa"))]
            pending_meta: None,
            dac_enabled: true,
            server_rebooting: false,
            player_name,
            stream_eof: false,
            #[cfg(not(target_arch = "xtensa"))]
            audio_stream: None,
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

    /// Handles an incoming command from LMS, mutating the pipeline and sending telemetry.
    #[cfg(not(target_arch = "xtensa"))]
    pub fn handle_command<S: AudioOutputSink>(
        &mut self,
        cmd: ServerCommand<'_>,
        pipeline: &mut StreamPipeline<S>,
        client: &mut SlimprotoClient,
        now_ms: u32,
    ) -> std::io::Result<Option<CydMessage>> {
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
                        self.audio_stream = None;
                        self.stream_eof = false;

                        self.autostart_mode = AutostartMode::from_strm(autostart, flags);
                        self.autostart = self.autostart_mode.is_auto();
                        self.awaiting_cont = self.autostart_mode.requires_cont();
                        self.server_rebooting = (flags & STRM_FLAG_SERVER_REBOOT) != 0;

                        // COORD-503: Allow empty http_headers by defaulting to GET /stream.mp3 HTTP/1.0
                        // and convert non-UTF8 (e.g. Latin-1) header bytes lossily into a stack buffer.
                        if server_port > 0 {
                            let ip = if server_ip == [0, 0, 0, 0] {
                                client
                                    .peer_addr()
                                    .map(|a| a.ip())
                                    .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
                            } else {
                                IpAddr::V4(Ipv4Addr::from(server_ip))
                            };

                            let mut stack_hdr_buf = [0u8; 1024];
                            let headers_str: &str = if http_headers.is_empty() {
                                "GET /stream.mp3 HTTP/1.0\r\n\r\n"
                            } else if let Ok(valid_str) = core::str::from_utf8(http_headers) {
                                valid_str
                            } else {
                                let copy_len = http_headers.len().min(stack_hdr_buf.len());
                                for (dst, &src) in stack_hdr_buf[..copy_len]
                                    .iter_mut()
                                    .zip(http_headers[..copy_len].iter())
                                {
                                    *dst = if src.is_ascii() { src } else { b'?' };
                                }
                                core::str::from_utf8(&stack_hdr_buf[..copy_len])
                                    .unwrap_or("GET /stream.mp3 HTTP/1.0\r\n\r\n")
                            };

                            match HttpAudioStream::connect(SocketAddr::new(ip, server_port), headers_str) {
                                Ok(http_stream) => {
                                    self.audio_stream = Some(http_stream);
                                    client.send_stat(
                                        StatEvent::Connected,
                                        pipeline.stream_buffer_available() as u32,
                                        pipeline.output_buffer_available() as u32,
                                        pipeline.frames_played as u32,
                                        now_ms,
                                    )?;
                                }
                                Err(_) => {
                                    self.state = PlayerState::Stopped;
                                    self.pending_dsco = Some(DisconnectReason::Unreachable);
                                    client.send_stat(
                                        StatEvent::NotSupported,
                                        pipeline.stream_buffer_available() as u32,
                                        pipeline.output_buffer_available() as u32,
                                        pipeline.frames_played as u32,
                                        now_ms,
                                    )?;
                                    client.send_dsco(DisconnectReason::Unreachable)?;
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

                        // Reconfigure pipeline decoder according to format
                        match format {
                            AudioFormat::Pcm => {
                                let dec = PcmDecoder::with_config(bits, is_be, channels);
                                pipeline.set_decoder(ActiveDecoder::Pcm(dec));
                            }
                            AudioFormat::Mp3 => {
                                pipeline.set_decoder(ActiveDecoder::Mp3(Box::new(Mp3Decoder::new())));
                            }
                            AudioFormat::Flac => {
                                pipeline.set_decoder(ActiveDecoder::Flac(Box::new(FlacDecoder::new())));
                            }
                            AudioFormat::Aac => {
                                pipeline.set_decoder(ActiveDecoder::Aac(Box::new(AacDecoder::new())));
                            }
                            AudioFormat::Ogg => {
                                pipeline.set_decoder(ActiveDecoder::Vorbis(Box::new(VorbisDecoder::new())));
                            }
                            _ => {
                                let dec = PcmDecoder::with_config(bits, is_be, channels);
                                pipeline.set_decoder(ActiveDecoder::Pcm(dec));
                            }
                        }

                        let _ = pipeline.set_sample_rate(sample_rate);

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
                            client.send_stat(
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
                        client.send_stat(
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
                        client.send_stat(
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
                        self.audio_stream = None;
                        self.pending_track_started = false;
                        self.pending_underrun = false;
                        self.pending_decode_ready = false;
                        self.decode_complete_sent = false;
                        pipeline.sink.set_mute(true);
                        client.send_stat(
                            StatEvent::TrackStopped,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(Some(CydMessage::PlaybackState(PlayState::Stopped)))
                    }
                    StreamCommandType::Flush => {
                        self.buffer_threshold_bytes = 0;
                        self.audio_stream = None;
                        self.pending_track_started = false;
                        self.pending_underrun = false;
                        self.pending_decode_ready = false;
                        self.decode_complete_sent = false;
                        if self.state == PlayerState::Buffering {
                            self.state = PlayerState::Stopped;
                        }
                        pipeline.flush();
                        client.send_stat(
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
                        client.send_stat_with_metrics(
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

                // COORD-507: Map 16.16 fixed-point (65536 = 100%) to 0-100 percentage using u64 to prevent u32 overflow
                let avg_gain = ((eff_gain_left as u64 + eff_gain_right as u64) / 2) as u32;
                let pct = core::cmp::min(100, ((avg_gain as u64 * 100) / 65536).min(100) as u8);

                Ok(Some(CydMessage::Volume(pct)))
            }
            ServerCommand::SetDevice { sample_rate } => {
                if sample_rate <= u8::MAX as u32 {
                    if sample_rate == 0 {
                        client.send_setd_name(&self.player_name)?;
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
                    client.send_setd_name(&self.player_name)?;
                    return Ok(None);
                }
                self.current_sample_rate = sample_rate;
                let _ = pipeline.set_sample_rate(sample_rate);
                Ok(None)
            }
            ServerCommand::Flush => {
                self.buffer_threshold_bytes = 0;
                self.audio_stream = None;
                self.pending_track_started = false;
                self.pending_underrun = false;
                self.pending_decode_ready = false;
                self.decode_complete_sent = false;
                if self.state == PlayerState::Buffering {
                    self.state = PlayerState::Stopped;
                }
                pipeline.flush();
                client.send_stat(
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
                } else if let Some(metaint) = cmd.stream_continue() {
                    self.awaiting_cont = false;
                    if metaint > 0 {
                        if let Some(ref mut http) = self.audio_stream {
                            http.set_icy_metaint(metaint as usize);
                        }
                    }
                } else if let Some(setting) = cmd.device_setting() {
                    if setting.id == 0 {
                        if !setting.data.is_empty() {
                            if let Ok(name) = core::str::from_utf8(setting.data) {
                                self.player_name.clear();
                                let _ = self.player_name.push_str(name);
                            }
                        }
                        client.send_setd_name(&self.player_name)?;
                    }
                } else if opcode == *b"eqlz" || opcode == *b"loud" {
                    pipeline.equalizer.enabled = true;
                }
                Ok(None)
            }
        }
    }

    /// Polls incoming HTTP audio bytes from the active `HttpAudioStream` and pushes them into the pipeline stream buffer.
    ///
    /// Breaks out of the loop when `n == 0` (`HALNET-405`), and drops `self.audio_stream = None`
    /// if `poll_audio_bytes` returns an error (`COORD-508`).
    #[cfg(not(target_arch = "xtensa"))]
    pub fn poll_http_stream<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<S>,
    ) -> std::io::Result<usize> {
        let mut total_written = 0;
        let mut chunk = [0u8; 1024];
        while let Some(ref mut http) = self.audio_stream {
            if let Some(headers) = http.take_response_headers() {
                self.pending_resp = Some(headers);
            }
            if let Some(meta) = http.take_icy_metadata() {
                self.pending_meta = Some(meta);
            }
            let avail_write = pipeline.stream_buffer.available_write();
            if avail_write == 0 {
                break;
            }
            let max_read = avail_write.min(chunk.len());
            match http.poll_audio_bytes(&mut chunk[..max_read]) {
                Ok(0) => {
                    if http.is_eof() {
                        self.stream_eof = true;
                    }
                    break;
                }
                Ok(n) => {
                    self.stream_bytes_received = http.total_audio_bytes();
                    let written = pipeline.ingest_stream_data(&chunk[..n]).unwrap_or(0);
                    total_written += written;
                    if written < n {
                        break;
                    }
                }
                Err(e) => {
                    self.stream_eof = true;
                    self.audio_stream = None;
                    return Err(e);
                }
            }
        }
        if let Some(ref mut http) = self.audio_stream {
            if let Some(headers) = http.take_response_headers() {
                self.pending_resp = Some(headers);
            }
            if let Some(meta) = http.take_icy_metadata() {
                self.pending_meta = Some(meta);
            }
            self.stream_bytes_received = http.total_audio_bytes();
        }
        Ok(total_written)
    }

    /// Evaluates buffering threshold, short-stream EOF transitions (`COORD-501`),
    /// decoder completion (`STMd` + `DSCO(0)`), and end-of-track underrun (`STMu` + Stopped).
    pub fn evaluate_state<S: AudioOutputSink>(&mut self, pipeline: &mut StreamPipeline<S>) {
        self.frames_played = pipeline.frames_played;
        #[cfg(not(target_arch = "xtensa"))]
        let is_eof = self.stream_eof
            || self
                .audio_stream
                .as_ref()
                .map(|s| s.is_eof())
                .unwrap_or(false);
        #[cfg(target_arch = "xtensa")]
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

    /// Executes one cycle of decoding and audio output.
    pub fn process_audio_step<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<S>,
    ) -> usize {
        #[cfg(not(target_arch = "xtensa"))]
        let _ = self.poll_http_stream(pipeline);
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

    /// Evaluates one integrated coordinator tick with Slimproto client telemetry,
    /// audio decoding, frame output, and deferred STAT updates (`RUST-01..RUST-10`).
    #[cfg(not(target_arch = "xtensa"))]
    pub fn tick<S: AudioOutputSink>(
        &mut self,
        client: &mut SlimprotoClient,
        pipeline: &mut StreamPipeline<S>,
        now_ms: u32,
    ) -> usize {
        let frames = self.process_audio_step(pipeline);
        let _ = self.check_heartbeat(pipeline, client, now_ms);
        frames
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
    #[cfg(not(target_arch = "xtensa"))]
    pub fn check_heartbeat<S: AudioOutputSink>(
        &mut self,
        pipeline: &StreamPipeline<S>,
        client: &mut SlimprotoClient,
        now_ms: u32,
    ) -> std::io::Result<()> {
        if let Some(resp) = self.pending_resp.take() {
            client.send_resp(&resp)?;
        }

        if let Some(meta) = self.pending_meta.take() {
            client.send_meta(&meta)?;
        }

        if self.pending_track_started {
            client.send_stat_with_metrics(
                StatEvent::TrackStarted,
                pipeline.stream_buffer.available_read() as u32,
                pipeline.output_buffer.available_read() as u32,
                pipeline.frames_played,
                self.current_sample_rate,
                self.stream_bytes_received,
                now_ms,
                0,
            )?;
            self.pending_track_started = false;
        }

        if self.pending_decode_ready {
            client.send_stat_with_metrics(
                StatEvent::DecodeReady,
                pipeline.stream_buffer.available_read() as u32,
                pipeline.output_buffer.available_read() as u32,
                pipeline.frames_played,
                self.current_sample_rate,
                self.stream_bytes_received,
                now_ms,
                0,
            )?;
            self.pending_decode_ready = false;
        }

        if let Some(reason) = self.pending_dsco {
            client.send_dsco(reason)?;
            self.pending_dsco = None;
        }

        if self.pending_underrun {
            client.send_stat_with_metrics(
                StatEvent::TrackUnderrun,
                pipeline.stream_buffer.available_read() as u32,
                pipeline.output_buffer.available_read() as u32,
                pipeline.frames_played,
                self.current_sample_rate,
                self.stream_bytes_received,
                now_ms,
                0,
            )?;
            self.pending_underrun = false;
        }

        if now_ms.wrapping_sub(self.last_heartbeat_ms) >= self.heartbeat_interval_ms {
            self.last_heartbeat_ms = now_ms;
            client.send_stat_with_metrics(
                StatEvent::Heartbeat,
                pipeline.stream_buffer.available_read() as u32,
                pipeline.output_buffer.available_read() as u32,
                pipeline.frames_played,
                self.current_sample_rate,
                self.stream_bytes_received,
                now_ms,
                0,
            )?;
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use squeezelite_audio_core::MockAudioSink;
    use squeezelite_proto::TransitionType;

    fn read_slimproto_frame<R: Read>(reader: &mut R) -> std::io::Result<([u8; 4], Vec<u8>)> {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;
        let mut opcode = [0u8; 4];
        opcode.copy_from_slice(&header[0..4]);
        let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let mut payload = vec![0u8; len];
        reader.read_exact(&mut payload)?;
        Ok((opcode, payload))
    }

    #[test]
    fn test_coordinator_stream_and_audio_lifecycle() {
        // 1. Create mock TCP server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

        // Connect client in thread
        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            // Handle Start Stream Command
            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3, // 44100 Hz
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: 0,
                server_ip: [0, 0, 0, 0],
                http_headers: &[],
            };

            let cyd_msg = coordinator
                .handle_command(start_cmd, &mut pipeline, &mut client, 100)
                .unwrap();
            assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
            assert_eq!(coordinator.state, PlayerState::Playing);

            // Feed 2 stereo frames: (1000, -1000), (2000, -2000)
            let f1_l = (1000i16).to_be_bytes();
            let f1_r = (-1000i16).to_be_bytes();
            let f2_l = (2000i16).to_be_bytes();
            let f2_r = (-2000i16).to_be_bytes();
            let raw_audio = [
                f1_l[0], f1_l[1], f1_r[0], f1_r[1],
                f2_l[0], f2_l[1], f2_r[0], f2_r[1],
            ];

            let body_cmd = ServerCommand::Body { data: &raw_audio };
            coordinator
                .handle_command(body_cmd, &mut pipeline, &mut client, 150)
                .unwrap();

            // Run audio processing
            let frames_out = coordinator.process_audio_step(&mut pipeline);
            assert_eq!(frames_out, 2);
            assert!(coordinator.pending_track_started);
            coordinator.check_heartbeat(&pipeline, &mut client, 160).unwrap();

            // Verify recorded samples
            let rec = pipeline.sink.recorded_samples();
            assert_eq!(rec[0], 1000 << 16);
            assert_eq!(rec[1], -1000 << 16);

            // Test Volume adjustment with digital_atten: 1
            let vol_cmd = ServerCommand::AudioGain {
                gain_left: 32768,  // 50%
                gain_right: 32768,
                digital_atten: 1,
                preamp: 0,
            };
            let cyd_vol = coordinator
                .handle_command(vol_cmd, &mut pipeline, &mut client, 200)
                .unwrap();
            assert_eq!(cyd_vol, Some(CydMessage::Volume(50)));

            // Test Pause
            let pause_cmd = ServerCommand::Stream {
                command: StreamCommandType::Pause,
                autostart: false,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: 0,
                server_ip: [0, 0, 0, 0],
                http_headers: &[],
            };
            let cyd_pause = coordinator
                .handle_command(pause_cmd, &mut pipeline, &mut client, 250)
                .unwrap();
            assert_eq!(cyd_pause, Some(CydMessage::PlaybackState(PlayState::Paused)));
            assert_eq!(coordinator.state, PlayerState::Paused);
        });

        // 2. Accept connection on mock server and read packets with frame-delimited reader
        let (mut server_stream, _) = listener.accept().unwrap();

        // Read HELO
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // Read STAT (STMs from TrackStarted)
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMs");

        // Read STAT (STMp from TrackPaused)
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMp");

        handle.join().unwrap();
    }

    #[test]
    fn test_threshold_kb_clamping_and_full_buffer_exit() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

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
    fn test_autostart_false_transitions_to_paused() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 2048];
        let mut output_storage = [0u8; 1024];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

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
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

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
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

        coordinator.state = PlayerState::Buffering;
        coordinator.buffer_threshold_bytes = 1024;

        let _flush_cmd = ServerCommand::Stream {
            command: StreamCommandType::Flush,
            autostart: false,
            format: AudioFormat::Pcm,
            pcm_sample_size: 16,
            pcm_sample_rate: 3,
            pcm_channels: 2,
            pcm_endianness: Endianness::BigEndian,
            threshold_kb: 0,
            spdif_enable: false,
            trans_duration: 0,
            trans_type: TransitionType::None,
            flags: 0,
            output_threshold: 0,
            replay_gain: 0,
            server_port: 0,
            server_ip: [0, 0, 0, 0],
            http_headers: &[],
        };

        // We can verify Flush directly on coordinator fields
        coordinator.buffer_threshold_bytes = 0;
        if coordinator.state == PlayerState::Buffering {
            coordinator.state = PlayerState::Stopped;
        }
        pipeline.flush();

        assert_eq!(coordinator.state, PlayerState::Stopped);
        assert_eq!(coordinator.buffer_threshold_bytes, 0);
    }

    #[test]
    fn test_coordinator_end_to_end_http_audio_streaming() {
        use std::time::Duration;

        // 1. Mock SlimProto server
        let slim_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let slim_addr = slim_listener.local_addr().unwrap();

        // 2. Mock HTTP audio server
        let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x88];

        // HTTP server thread
        let http_handle = std::thread::spawn(move || {
            let (mut stream, _) = http_listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();

            // Prepare 4 stereo frames of 16-bit Big-Endian PCM:
            // Frame 0: (100, -100)
            // Frame 1: (200, -200)
            // Frame 2: (300, -300)
            // Frame 3: (400, -400)
            let f0 = [(100i16).to_be_bytes(), (-100i16).to_be_bytes()].concat();
            let f1 = [(200i16).to_be_bytes(), (-200i16).to_be_bytes()].concat();
            let f2 = [(300i16).to_be_bytes(), (-300i16).to_be_bytes()].concat();
            let f3 = [(400i16).to_be_bytes(), (-400i16).to_be_bytes()].concat();

            let mut pcm_payload = Vec::new();
            pcm_payload.extend_from_slice(&f0);
            pcm_payload.extend_from_slice(&f1);
            pcm_payload.extend_from_slice(&f2);
            pcm_payload.extend_from_slice(&f3);

            let header = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
            stream.write_all(header).unwrap();
            stream.write_all(&pcm_payload).unwrap();
            stream.flush().unwrap();

            std::thread::sleep(Duration::from_millis(50));
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        // Client thread
        let client_handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(slim_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 4096];
            let mut output_storage = [0u8; 4096];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            let http_get_req = format!(
                "GET /stream.pcm HTTP/1.0\r\nHost: 127.0.0.1:{}\r\n\r\n",
                http_addr.port()
            );

            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3, // 44100 Hz
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: http_addr.port(),
                server_ip: [127, 0, 0, 1],
                http_headers: http_get_req.as_bytes(),
            };

            let cyd_msg = coordinator
                .handle_command(start_cmd, &mut pipeline, &mut client, 100)
                .unwrap();
            assert_eq!(cyd_msg, Some(CydMessage::PlaybackState(PlayState::Playing)));
            assert_eq!(coordinator.state, PlayerState::Playing);
            assert!(coordinator.audio_stream.is_some());

            // Process audio steps until all 4 frames are decoded and recorded
            let start = std::time::Instant::now();
            let mut total_frames = 0;
            while total_frames < 4 && start.elapsed() < Duration::from_secs(3) {
                let frames = coordinator.process_audio_step(&mut pipeline);
                total_frames += frames;
                if frames == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
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

            // Test Stop command clears audio_stream
            let stop_cmd = ServerCommand::Stream {
                command: StreamCommandType::Stop,
                autostart: false,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: 0,
                server_ip: [0, 0, 0, 0],
                http_headers: &[],
            };
            let _ = coordinator
                .handle_command(stop_cmd, &mut pipeline, &mut client, 200)
                .unwrap();
            assert!(coordinator.audio_stream.is_none());
            assert_eq!(coordinator.state, PlayerState::Stopped);
        });

        // Mock SlimProto server: accept HELO and STAT
        let (mut server_stream, _) = slim_listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap(); // HELO
        assert_eq!(&opcode, b"HELO");
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap(); // STAT STMc
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMc");

        http_handle.join().unwrap();
        client_handle.join().unwrap();
    }

    #[test]
    fn test_short_eof_stream_exits_buffering_and_emits_stms_stmu() {
        // COORD-501, COORD-502, COORD-504:
        // A short track smaller than threshold_kb that hits HTTP EOF must:
        // 1. Transition out of Buffering -> Playing and set pending_track_started = true
        // 2. Emit STMs via check_heartbeat
        // 3. Once all decoded PCM frames are played out, transition Playing -> Stopped and set pending_underrun = true
        // 4. Emit STMu via check_heartbeat
        use std::time::Duration;

        let slim_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let slim_addr = slim_listener.local_addr().unwrap();

        let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        let mac = [0x22, 0x33, 0x44, 0x55, 0x66, 0x77];

        // HTTP server sends only 2 stereo frames (8 bytes), well below 1024-byte threshold, then closes (EOF)
        let http_handle = std::thread::spawn(move || {
            let (mut stream, _) = http_listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();

            let header = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
            let pcm = [0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00];
            stream.write_all(header).unwrap();
            stream.write_all(&pcm).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let client_handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(slim_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 4096];
            let mut output_storage = [0u8; 4096];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            // Start with threshold_kb = 2 (2048 bytes) and empty http_headers (COORD-503 default header test)
            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 2,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: http_addr.port(),
                server_ip: [127, 0, 0, 1],
                http_headers: &[],
            };

            coordinator
                .handle_command(start_cmd, &mut pipeline, &mut client, 10)
                .unwrap();
            assert_eq!(coordinator.state, PlayerState::Buffering);

            // Wait until short HTTP stream hits EOF and drains all 2 frames
            let start = std::time::Instant::now();
            while coordinator.state != PlayerState::Stopped && start.elapsed() < Duration::from_secs(3) {
                let _ = coordinator.process_audio_step(&mut pipeline);
                if coordinator.pending_track_started
                    || coordinator.pending_underrun
                    || coordinator.pending_decode_ready
                    || coordinator.pending_dsco.is_some()
                    || coordinator.pending_resp.is_some()
                {
                    coordinator
                        .check_heartbeat(&pipeline, &mut client, 20)
                        .unwrap();
                }
                std::thread::sleep(Duration::from_millis(5));
            }

            assert_eq!(coordinator.state, PlayerState::Stopped);
            assert_eq!(pipeline.frames_played, 2);
            assert_eq!(coordinator.frames_played, 2);
        });

        let (mut server_stream, _) = slim_listener.accept().unwrap();
        // 1. HELO
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // 2. STAT STMc
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMc");

        // 3. RESP
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"RESP");
        assert!(payload.starts_with(b"HTTP/1.0 200 OK"));

        // 4. STAT STMs
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMs");

        // 5. STAT STMd
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMd");

        // 6. DSCO (0)
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"DSCO");
        assert_eq!(payload[0], 0);

        // 7. STAT STMu
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMu");

        http_handle.join().unwrap();
        client_handle.join().unwrap();
    }

    #[test]
    fn test_audio_gain_u64_no_overflow_and_sync_cyd_telemetry() {
        // COORD-505, COORD-507: Verify u32::MAX gain does not overflow and sync_cyd_telemetry
        // returns PlaybackState, Volume, and Progress.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x99];

        let server_handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 128];
            let _ = s.read(&mut buf).unwrap();
        });

        let mut client = SlimprotoClient::connect(server_addr, mac, "TestPlayer").unwrap();
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 512];
        let mut output_storage = [0u8; 512];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

        let max_gain_cmd = ServerCommand::AudioGain {
            gain_left: u32::MAX,
            gain_right: u32::MAX,
            digital_atten: 0,
            preamp: 0,
        };
        let msg = coordinator
            .handle_command(max_gain_cmd, &mut pipeline, &mut client, 50)
            .unwrap();
        assert_eq!(msg, Some(CydMessage::Volume(100)));

        coordinator.state = PlayerState::Playing;
        pipeline.frames_played = 44100 * 12; // 12 seconds at 44.1 kHz
        let telemetry = coordinator.sync_cyd_telemetry(&pipeline);
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

        server_handle.join().unwrap();
    }

    #[test]
    fn test_latin1_headers_and_poll_http_error_drops_stream() {
        // COORD-503, COORD-508, COORD-509:
        // Verify non-UTF8 (Latin-1) headers connect via stack lossy conversion, and if the HTTP
        // server returns a 404 error, poll_http_stream drops self.audio_stream = None.
        use std::time::Duration;

        let slim_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let slim_addr = slim_listener.local_addr().unwrap();

        let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        let mac = [0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

        let http_handle = std::thread::spawn(move || {
            let (mut stream, _) = http_listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();
            stream
                .write_all(b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();
        });

        let slim_handle = std::thread::spawn(move || {
            let (mut stream, _) = slim_listener.accept().unwrap();
            let (opcode, _) = read_slimproto_frame(&mut stream).unwrap(); // HELO
            assert_eq!(&opcode, b"HELO");
            let (opcode, payload) = read_slimproto_frame(&mut stream).unwrap(); // STAT STMc
            assert_eq!(&opcode, b"STAT");
            assert_eq!(&payload[0..4], b"STMc");
        });

        let mut client = SlimprotoClient::connect(slim_addr, mac, "TestPlayer").unwrap();
        let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

        // Pre-populate pipeline with dirty state to verify Start flushes and resets frames_played (COORD-506, COORD-509)
        pipeline.ingest_stream_data(&[1, 2, 3, 4]).unwrap();
        pipeline.frames_played = 999;
        coordinator.frames_played = 999;

        // Latin-1 invalid UTF-8 byte 0xE9 ('é') in HTTP request header (COORD-503)
        let latin1_headers = b"GET /caf\xe9.pcm HTTP/1.0\r\n\r\n";
        let start_cmd = ServerCommand::Stream {
            command: StreamCommandType::Start,
            autostart: true,
            format: AudioFormat::Pcm,
            pcm_sample_size: 16,
            pcm_sample_rate: 3,
            pcm_channels: 2,
            pcm_endianness: Endianness::BigEndian,
            threshold_kb: 0,
            spdif_enable: false,
            trans_duration: 0,
            trans_type: TransitionType::None,
            flags: 0,
            output_threshold: 0,
            replay_gain: 0,
            server_port: http_addr.port(),
            server_ip: [127, 0, 0, 1],
            http_headers: latin1_headers,
        };

        coordinator
            .handle_command(start_cmd, &mut pipeline, &mut client, 100)
            .unwrap();
        assert_eq!(pipeline.frames_played, 0);
        assert_eq!(coordinator.frames_played, 0);
        assert_eq!(pipeline.stream_buffer_available(), 0);
        assert!(coordinator.audio_stream.is_some());

        // Poll until 404 response triggers error and drops self.audio_stream = None (COORD-508)
        let start = std::time::Instant::now();
        while coordinator.audio_stream.is_some() && start.elapsed() < Duration::from_secs(3) {
            let _ = coordinator.poll_http_stream(&mut pipeline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(coordinator.audio_stream.is_none());

        http_handle.join().unwrap();
        slim_handle.join().unwrap();
    }

    #[test]
    fn test_coordinator_stream_open_failure_emits_stmn_and_dsco() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x77];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            // server_port = 1 is closed/unreachable on localhost
            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: 1,
                server_ip: [127, 0, 0, 1],
                http_headers: &[],
            };

            let res = coordinator.handle_command(start_cmd, &mut pipeline, &mut client, 100);
            assert_eq!(res.unwrap(), Some(CydMessage::PlaybackState(PlayState::Stopped)));
            assert_eq!(coordinator.state, PlayerState::Stopped);
            assert_eq!(coordinator.pending_dsco, Some(DisconnectReason::Unreachable));
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // STAT STMn
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMn");

        // DSCO Unreachable (3)
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"DSCO");
        assert_eq!(payload[0], DisconnectReason::Unreachable as u8);

        handle.join().unwrap();
    }

    #[test]
    fn test_coordinator_defers_stms_until_pcm_output_written() {
        use std::time::Duration;

        let slim_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let slim_addr = slim_listener.local_addr().unwrap();

        let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xAA];

        let http_handle = std::thread::spawn(move || {
            let (mut stream, _) = http_listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();

            let header = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
            let pcm = [0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00];
            stream.write_all(header).unwrap();
            stream.write_all(&pcm).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let client_handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(slim_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: http_addr.port(),
                server_ip: [127, 0, 0, 1],
                http_headers: &[],
            };

            let res = coordinator.handle_command(start_cmd, &mut pipeline, &mut client, 100);
            assert_eq!(res.unwrap(), Some(CydMessage::PlaybackState(PlayState::Playing)));
            assert_eq!(coordinator.state, PlayerState::Playing);
            // STMs NOT emitted prematurely before PCM output is written
            assert!(!coordinator.pending_track_started);

            // Process audio step: reads HTTP, decodes, and writes PCM frames to sink
            let start = std::time::Instant::now();
            let mut written = 0;
            while written == 0 && start.elapsed() < Duration::from_secs(3) {
                written = coordinator.process_audio_step(&mut pipeline);
                if written == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            assert!(written > 0);
            assert!(coordinator.pending_track_started);

            // check_heartbeat emits the deferred STMs
            coordinator.check_heartbeat(&pipeline, &mut client, 150).unwrap();
            assert!(!coordinator.pending_track_started);
        });

        let (mut server_stream, _) = slim_listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // STMc emitted on successful HTTP connect
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMc");

        // RESP emitted on HTTP header parse
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"RESP");
        assert!(payload.starts_with(b"HTTP/1.0 200 OK"));

        // STMs emitted only AFTER PCM frames written to sink
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMs");

        http_handle.join().unwrap();
        client_handle.join().unwrap();
    }

    #[test]
    fn test_coordinator_eof_emits_stmd_then_stmu_after_output_drain() {
        use std::time::Duration;

        let slim_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let slim_addr = slim_listener.local_addr().unwrap();

        let http_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xBB];

        let http_handle = std::thread::spawn(move || {
            let (mut stream, _) = http_listener.accept().unwrap();
            let mut req = [0u8; 512];
            let _ = stream.read(&mut req).unwrap();

            let header = b"HTTP/1.0 200 OK\r\nContent-Type: audio/basic\r\n\r\n";
            let pcm = [0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00];
            stream.write_all(header).unwrap();
            stream.write_all(&pcm).unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let client_handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(slim_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 2048];
            let mut output_storage = [0u8; 2048];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            let start_cmd = ServerCommand::Stream {
                command: StreamCommandType::Start,
                autostart: true,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0,
                server_port: http_addr.port(),
                server_ip: [127, 0, 0, 1],
                http_headers: &[],
            };

            coordinator.handle_command(start_cmd, &mut pipeline, &mut client, 10).unwrap();

            // 1. Wait until headers parsed and RESP is available
            let start = std::time::Instant::now();
            while coordinator.pending_resp.is_none() && start.elapsed() < Duration::from_secs(3) {
                let _ = coordinator.poll_http_stream(&mut pipeline);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(coordinator.pending_resp.is_some());
            coordinator.check_heartbeat(&pipeline, &mut client, 20).unwrap();

            // 2. Decode stream into output buffer; wait until stream_buffer drains at EOF
            let start = std::time::Instant::now();
            while (!coordinator.decode_complete_sent) && start.elapsed() < Duration::from_secs(3) {
                let _ = coordinator.poll_http_stream(&mut pipeline);
                let _ = pipeline.process_decode_step();
                coordinator.evaluate_state(&mut pipeline);
                std::thread::sleep(Duration::from_millis(5));
            }

            assert!(coordinator.decode_complete_sent);
            assert!(coordinator.pending_decode_ready);
            // Output buffer still holds frames
            assert!(pipeline.output_buffer.available_read() > 0);
            assert_eq!(coordinator.state, PlayerState::Playing);

            // check_heartbeat emits STMd and DSCO(0)
            coordinator.check_heartbeat(&pipeline, &mut client, 30).unwrap();

            // 3. Output buffer drains
            let written = pipeline.process_output_step().unwrap();
            assert_eq!(written, 2);
            assert_eq!(pipeline.output_buffer.available_read(), 0);

            coordinator.evaluate_state(&mut pipeline);
            assert_eq!(coordinator.state, PlayerState::Stopped);
            assert!(coordinator.pending_underrun);

            // check_heartbeat emits STMu
            coordinator.check_heartbeat(&pipeline, &mut client, 40).unwrap();
            assert!(!coordinator.pending_underrun);
        });

        let (mut server_stream, _) = slim_listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // STMc
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMc");

        // RESP
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"RESP");
        assert!(payload.starts_with(b"HTTP/1.0 200 OK"));

        // STMd
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMd");

        // DSCO (0)
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"DSCO");
        assert_eq!(payload[0], DisconnectReason::ConnectionClosed as u8);

        // STMu
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMu");

        http_handle.join().unwrap();
        client_handle.join().unwrap();
    }

    #[test]
    fn test_coordinator_timestamp_probe_and_setd_aude_cont() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xCC];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "TestPlayer").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "TestPlayer");

            let mut stream_storage = [0u8; 1024];
            let mut output_storage = [0u8; 1024];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            // 1. strm 't' (Timestamp probe) with replay_gain = 0x12345678
            let ts_cmd = ServerCommand::Stream {
                command: StreamCommandType::Unknown(b't'),
                autostart: false,
                format: AudioFormat::Pcm,
                pcm_sample_size: 16,
                pcm_sample_rate: 3,
                pcm_channels: 2,
                pcm_endianness: Endianness::BigEndian,
                threshold_kb: 0,
                spdif_enable: false,
                trans_duration: 0,
                trans_type: TransitionType::None,
                flags: 0,
                output_threshold: 0,
                replay_gain: 0x1234_5678,
                server_port: 0,
                server_ip: [0, 0, 0, 0],
                http_headers: &[],
            };
            coordinator.handle_command(ts_cmd, &mut pipeline, &mut client, 100).unwrap();

            // 2. setd (Device setting id = 0, update player name to "Kitchen")
            let setd_cmd = ServerCommand::Unknown {
                opcode: *b"setd",
                payload: b"\x00Kitchen",
            };
            coordinator.handle_command(setd_cmd, &mut pipeline, &mut client, 200).unwrap();
            assert_eq!(coordinator.player_name.as_str(), "Kitchen");

            // 3. aude (Audio enable: spdif = 1, dac = 0 -> mute dac)
            let aude_mute = ServerCommand::Unknown {
                opcode: *b"aude",
                payload: &[1, 0],
            };
            coordinator.handle_command(aude_mute, &mut pipeline, &mut client, 300).unwrap();
            assert!(!coordinator.dac_enabled);
            assert!(pipeline.sink.muted);

            // aude (dac = 1 -> unmute dac)
            let aude_unmute = ServerCommand::Unknown {
                opcode: *b"aude",
                payload: &[1, 1],
            };
            coordinator.handle_command(aude_unmute, &mut pipeline, &mut client, 350).unwrap();
            assert!(coordinator.dac_enabled);
            assert!(!pipeline.sink.muted);

            // 4. cont (Stream continue: releases awaiting_cont)
            coordinator.awaiting_cont = true;
            let cont_cmd = ServerCommand::Unknown {
                opcode: *b"cont",
                payload: &8192u32.to_be_bytes(),
            };
            coordinator.handle_command(cont_cmd, &mut pipeline, &mut client, 400).unwrap();
            assert!(!coordinator.awaiting_cont);
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // 1. STAT with server_timestamp = 0x12345678
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"STAT");
        assert_eq!(&payload[0..4], b"STMt");
        assert_eq!(&payload[47..51], &0x1234_5678u32.to_be_bytes());

        // 2. SETD name reply with "Kitchen"
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"SETD");
        assert_eq!(payload[0], 0x00);
        assert_eq!(&payload[1..], b"Kitchen");

        handle.join().unwrap();
    }

    #[test]
    fn test_setd_one_byte_query_preserves_sample_rate_and_replies_name() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xDD];

        let handle = std::thread::spawn(move || {
            let mut client = SlimprotoClient::connect(server_addr, mac, "Basement").unwrap();
            let mut coordinator = PlayerCoordinator::new(mac, "Basement");
            assert_eq!(coordinator.current_sample_rate, 44100);

            let mut stream_storage = [0u8; 1024];
            let mut output_storage = [0u8; 1024];
            let pcm_dec = PcmDecoder::new(16, 2, true);
            let sink = MockAudioSink::<64>::new();
            let mut pipeline = StreamPipeline::new(
                &mut stream_storage,
                &mut output_storage,
                ActiveDecoder::Pcm(pcm_dec),
                sink,
                44100,
            );

            // 1-byte setd [0x00] query from LMS
            let setd_query = ServerCommand::SetDevice { sample_rate: 0 };
            coordinator
                .handle_command(setd_query, &mut pipeline, &mut client, 100)
                .unwrap();

            assert_eq!(coordinator.current_sample_rate, 44100);
            assert_eq!(coordinator.player_name.as_str(), "Basement");
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let (opcode, _) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"HELO");

        // SETD name reply with "Basement"
        let (opcode, payload) = read_slimproto_frame(&mut server_stream).unwrap();
        assert_eq!(&opcode, b"SETD");
        assert_eq!(payload[0], 0x00);
        assert_eq!(&payload[1..], b"Basement");

        handle.join().unwrap();
    }

    #[test]
    fn test_coordinator_audg_digital_atten_zero_unity_gain_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0xEE];

        let handle = std::thread::spawn(move || {
            let (mut server_stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 128];
            let _ = server_stream.read(&mut buf); // HELO
            let _ = server_stream.read(&mut buf); // SETD reply
        });

        let mut client = SlimprotoClient::connect(server_addr, mac, "Test").unwrap();
        let mut coordinator = PlayerCoordinator::new(mac, "Test");
        let mut stream_storage = [0u8; 1024];
        let mut output_storage = [0u8; 1024];
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let sink = MockAudioSink::<64>::new();
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

        // Initially unity gain (65536, 65536)
        assert_eq!(coordinator.volume.gain_left, 65536);
        assert_eq!(coordinator.volume.gain_right, 65536);

        // When digital_atten == 0 and incoming gain is 0,0 while old gain was > 0, set unity gain (65536)
        let audg_unity = ServerCommand::AudioGain {
            gain_left: 0,
            gain_right: 0,
            digital_atten: 0,
            preamp: 0,
        };
        coordinator.handle_command(audg_unity, &mut pipeline, &mut client, 100).unwrap();
        assert_eq!(coordinator.volume.gain_left, 65536);
        assert_eq!(coordinator.volume.gain_right, 65536);

        // 4-byte player name [0, 'B', 'o', 'b'] updates player_name and replies with SETD without altering current_sample_rate
        let bob_rate = u32::from_be_bytes([0x00, b'B', b'o', b'b']);
        let setd_bob = ServerCommand::SetDevice { sample_rate: bob_rate };
        coordinator.handle_command(setd_bob, &mut pipeline, &mut client, 200).unwrap();
        assert_eq!(coordinator.player_name.as_str(), "Bob");
        assert_eq!(coordinator.current_sample_rate, 44100);

        // 1-byte id=4 DAC query does not alter current_sample_rate
        let setd_dac_query = ServerCommand::SetDevice { sample_rate: 4 };
        coordinator.handle_command(setd_dac_query, &mut pipeline, &mut client, 300).unwrap();
        assert_eq!(coordinator.current_sample_rate, 44100);

        // Sample rate decoding for 176.4k and 192k
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b';'), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(11), Some(176400));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(b'<'), Some(192000));
        assert_eq!(PlayerCoordinator::decode_pcm_sample_rate_hz(12), Some(192000));

        // Stream EOF tracking and reset
        coordinator.set_stream_eof(true);
        assert!(coordinator.is_stream_eof());
        coordinator.reset();
        assert!(!coordinator.is_stream_eof());

        drop(client);
        handle.join().unwrap();
    }
}

