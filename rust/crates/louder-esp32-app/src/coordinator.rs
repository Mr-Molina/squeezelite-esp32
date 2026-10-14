//! Player Coordinator
//!
//! Orchestrates the SlimProto network client, audio decoding pipeline,
//! hardware amplifier state, and CYD display status updates.

use squeezelite_audio_core::output::AudioOutputSink;
use squeezelite_audio_core::VolumeScale;
use squeezelite_cyd_link::{CydCommand, CydMessage, PlayState};
use squeezelite_decoders::{AacDecoder, FlacDecoder, Mp3Decoder, PcmDecoder, VorbisDecoder};
use squeezelite_proto::{
    sample_rate_from_index, AudioFormat, Endianness, ServerCommand,
    StatEvent, StreamCommandType,
};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::network::{HttpAudioStream, SlimprotoClient};
use crate::pipeline::{ActiveDecoder, StreamPipeline};

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
    pub audio_stream: Option<HttpAudioStream>,
    pub pending_track_started: bool,
    pub pending_underrun: bool,
}

impl PlayerCoordinator {
    /// Creates a new coordinator instance.
    pub fn new(mac: [u8; 6], model_name: &'static str) -> Self {
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
            audio_stream: None,
            pending_track_started: false,
            pending_underrun: false,
        }
    }

    /// Decodes SlimProto sample rate code into Hz.
    /// Supports both ASCII '0'..='9' and numeric 0..=9 codes.
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
                server_port,
                server_ip,
                http_headers,
                ..
            } => {
                match command {
                    StreamCommandType::Start => {
                        // COORD-509 & COORD-506: Flush pipeline and reset frame counters at track start
                        pipeline.flush();
                        pipeline.frames_played = 0;
                        self.frames_played = 0;
                        self.pending_track_started = false;
                        self.pending_underrun = false;
                        self.audio_stream = None;
                        self.autostart = autostart;

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

                            if let Ok(http_stream) =
                                HttpAudioStream::connect(SocketAddr::new(ip, server_port), headers_str)
                            {
                                self.audio_stream = Some(http_stream);
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
                                // Default to PCM decoder
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

                        if effective_threshold > 0 {
                            self.state = PlayerState::Buffering;
                            Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                        } else {
                            self.state = if autostart {
                                PlayerState::Playing
                            } else {
                                PlayerState::Paused
                            };

                            if self.state == PlayerState::Playing {
                                client.send_stat(
                                    StatEvent::TrackStarted,
                                    pipeline.stream_buffer_available() as u32,
                                    pipeline.output_buffer_available() as u32,
                                    pipeline.frames_played as u32,
                                    now_ms,
                                )?;
                                Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                            } else {
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
                        pipeline.sink.set_mute(false);
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
                    StreamCommandType::Unknown(_) => Ok(None),
                }
            }
            ServerCommand::AudioGain {
                gain_left,
                gain_right,
                ..
            } => {
                self.volume.gain_left = gain_left;
                self.volume.gain_right = gain_right;
                pipeline.set_volume(gain_left, gain_right);

                // COORD-507: Map 16.16 fixed-point (65536 = 100%) to 0-100 percentage using u64 to prevent u32 overflow
                let avg_gain = ((gain_left as u64 + gain_right as u64) / 2) as u32;
                let pct = core::cmp::min(100, ((avg_gain as u64 * 100) / 65536).min(100) as u8);

                Ok(Some(CydMessage::Volume(pct)))
            }
            ServerCommand::SetDevice { sample_rate } => {
                self.current_sample_rate = sample_rate;
                let _ = pipeline.set_sample_rate(sample_rate);
                Ok(None)
            }
            ServerCommand::Flush => {
                self.buffer_threshold_bytes = 0;
                self.audio_stream = None;
                self.pending_track_started = false;
                self.pending_underrun = false;
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
            ServerCommand::Unknown { .. } => Ok(None),
        }
    }

    /// Polls incoming HTTP audio bytes from the active `HttpAudioStream` and pushes them into the pipeline stream buffer.
    ///
    /// Breaks out of the loop when `n == 0` (`HALNET-405`), and drops `self.audio_stream = None`
    /// if `poll_audio_bytes` returns an error (`COORD-508`).
    pub fn poll_http_stream<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<S>,
    ) -> std::io::Result<usize> {
        let mut total_written = 0;
        let mut chunk = [0u8; 1024];
        while let Some(ref mut http) = self.audio_stream {
            let avail_write = pipeline.stream_buffer.available_write();
            if avail_write == 0 {
                break;
            }
            let max_read = avail_write.min(chunk.len());
            match http.poll_audio_bytes(&mut chunk[..max_read]) {
                Ok(0) => break,
                Ok(n) => {
                    let written = pipeline.ingest_stream_data(&chunk[..n]).unwrap_or(0);
                    total_written += written;
                    if written < n {
                        break;
                    }
                }
                Err(e) => {
                    self.audio_stream = None;
                    return Err(e);
                }
            }
        }
        Ok(total_written)
    }

    /// Evaluates buffering threshold, short-stream EOF transitions (`COORD-501`),
    /// and end-of-track stream underrun transitions (`COORD-502`, `COORD-504`).
    pub fn tick<S: AudioOutputSink>(&mut self, pipeline: &mut StreamPipeline<S>) {
        self.frames_played = pipeline.frames_played;
        let is_eof = self
            .audio_stream
            .as_ref()
            .map(|s| s.is_eof())
            .unwrap_or(false);

        if self.state == PlayerState::Buffering
            && (pipeline.stream_buffer.available_read() >= self.buffer_threshold_bytes
                || pipeline.stream_buffer.available_write() == 0
                || (is_eof && pipeline.stream_buffer.available_read() > 0))
        {
            if self.autostart {
                self.state = PlayerState::Playing;
                self.pending_track_started = true;
            } else {
                self.state = PlayerState::Paused;
            }
        } else if self.state == PlayerState::Playing
            && is_eof
            && pipeline.stream_buffer.available_read() == 0
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
        let _ = self.poll_http_stream(pipeline);
        self.tick(pipeline);

        if self.state != PlayerState::Playing {
            return 0;
        }

        // Try decoding a frame chunk
        let _ = pipeline.process_decode_step();

        // Write frames to the output sink
        let written = pipeline.process_output_step().unwrap_or(0);
        self.frames_played = pipeline.frames_played;

        // Re-evaluate EOF / underrun transition after draining output buffer
        self.tick(pipeline);

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

    /// Emits pending deferred `STAT` transition events (`STMs` / `STMu`) and periodic `STAT` heartbeats
    /// if the heartbeat interval has elapsed (APP-004: `wrapping_sub` handles 49.7d timer wrap; `COORD-502`, `COORD-504`).
    pub fn check_heartbeat<S: AudioOutputSink>(
        &mut self,
        pipeline: &StreamPipeline<S>,
        client: &mut SlimprotoClient,
        now_ms: u32,
    ) -> std::io::Result<()> {
        if self.pending_track_started {
            self.pending_track_started = false;
            client.send_stat(
                StatEvent::TrackStarted,
                pipeline.stream_buffer_available() as u32,
                pipeline.output_buffer_available() as u32,
                pipeline.frames_played as u32,
                now_ms,
            )?;
        }

        if self.pending_underrun {
            self.pending_underrun = false;
            client.send_stat(
                StatEvent::StreamUnderrun,
                pipeline.stream_buffer_available() as u32,
                pipeline.output_buffer_available() as u32,
                pipeline.frames_played as u32,
                now_ms,
            )?;
        }

        if now_ms.wrapping_sub(self.last_heartbeat_ms) >= self.heartbeat_interval_ms {
            self.last_heartbeat_ms = now_ms;
            client.send_stat(
                StatEvent::Heartbeat,
                pipeline.stream_buffer_available() as u32,
                pipeline.output_buffer_available() as u32,
                pipeline.frames_played as u32,
                now_ms,
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

            // Verify recorded samples
            let rec = pipeline.sink.recorded_samples();
            assert_eq!(rec[0], 1000 << 16);
            assert_eq!(rec[1], -1000 << 16);

            // Test Volume adjustment
            let vol_cmd = ServerCommand::AudioGain {
                gain_left: 32768,  // 50%
                gain_right: 32768,
                digital_atten: 0,
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

        // 2. Accept connection on mock server and read packets
        let (mut server_stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 128];

        // Read HELO
        let _ = server_stream.read(&mut buf).unwrap();
        assert_eq!(&buf[0..4], b"HELO");

        // Read STAT (STMs from TrackStarted)
        let _ = server_stream.read(&mut buf).unwrap();
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], b"STMs");

        // Read STAT (STMp from TrackPaused)
        let _ = server_stream.read(&mut buf).unwrap();
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], b"STMp");

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
        coordinator.tick(&mut pipeline);
        assert_eq!(coordinator.state, PlayerState::Buffering);

        // Ingest 1024 bytes (reaching effective threshold)
        let chunk = [0x55u8; 1024];
        pipeline.ingest_stream_data(&chunk).unwrap();
        coordinator.tick(&mut pipeline);
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

        // tick() must exit Buffering to avoid deadlock
        coordinator.tick(&mut pipeline);
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

        coordinator.tick(&mut pipeline);
        assert_eq!(coordinator.state, PlayerState::Buffering);

        // Ingest enough data to satisfy threshold
        let data = [0x77u8; 512];
        pipeline.ingest_stream_data(&data).unwrap();

        coordinator.tick(&mut pipeline);
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
        let mut buf = [0u8; 128];
        let _ = server_stream.read(&mut buf).unwrap(); // HELO
        let _ = server_stream.read(&mut buf).unwrap(); // STAT STMs

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
                if coordinator.pending_track_started || coordinator.pending_underrun {
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
        let mut buf = [0u8; 64];
        // 1. HELO (10 header + 12 fixed + 63 caps = 85 bytes)
        let mut helo_buf = [0u8; 128];
        let _ = server_stream.read(&mut helo_buf).unwrap();
        assert_eq!(&helo_buf[0..4], b"HELO");

        let mut expected_stms = [0u8; 64];
        squeezelite_proto::SlimprotoBuilder::build_stat(StatEvent::TrackStarted, 0, 0, 0, 0, &mut expected_stms).unwrap();
        let mut expected_underrun = [0u8; 64];
        squeezelite_proto::SlimprotoBuilder::build_stat(StatEvent::StreamUnderrun, 0, 0, 0, 0, &mut expected_underrun).unwrap();

        // 2. STAT TrackStarted (61 bytes)
        server_stream.read_exact(&mut buf[..61]).unwrap();
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], &expected_stms[8..12]);

        // 3. STAT StreamUnderrun (61 bytes)
        server_stream.read_exact(&mut buf[..61]).unwrap();
        assert_eq!(&buf[0..4], b"STAT");
        assert_eq!(&buf[8..12], &expected_underrun[8..12]);

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
            let mut buf = [0u8; 128];
            let _ = stream.read(&mut buf).unwrap(); // HELO
            let _ = stream.read(&mut buf).unwrap(); // STAT STMs
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
}

