//! Player Coordinator
//!
//! Orchestrates the SlimProto network client, audio decoding pipeline,
//! hardware amplifier state, and CYD display status updates.

use squeezelite_audio_core::output::AudioOutputSink;
use squeezelite_audio_core::VolumeScale;
use squeezelite_cyd_link::{CydMessage, PlayState};
use squeezelite_decoders::{Mp3Decoder, PcmDecoder};
use squeezelite_proto::{
    sample_rate_from_index, AudioFormat, Endianness, ServerCommand,
    StatEvent, StreamCommandType,
};

use crate::network::SlimprotoClient;
use crate::pipeline::{ActiveDecoder, StreamPipeline};

/// Playback operational state of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Disconnected,
    Connected,
    Streaming,
    Paused,
    Stopped,
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
        }
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
                format,
                pcm_sample_size,
                pcm_sample_rate,
                pcm_channels,
                pcm_endianness,
                ..
            } => {
                match command {
                    StreamCommandType::Start => {
                        let sample_rate = sample_rate_from_index(pcm_sample_rate);
                        self.current_format = Some(format);
                        self.current_sample_rate = sample_rate;

                        // Reconfigure pipeline decoder according to format
                        match format {
                            AudioFormat::Pcm => {
                                let is_be = matches!(pcm_endianness, Endianness::BigEndian);
                                let dec = PcmDecoder::new(pcm_sample_size, pcm_channels, is_be);
                                pipeline.set_decoder(ActiveDecoder::Pcm(dec));
                            }
                            AudioFormat::Mp3 => {
                                pipeline.set_decoder(ActiveDecoder::Mp3(Box::new(Mp3Decoder::new())));
                            }
                            _ => {
                                // Default to PCM decoder
                                let is_be = matches!(pcm_endianness, Endianness::BigEndian);
                                let dec = PcmDecoder::new(pcm_sample_size, pcm_channels, is_be);
                                pipeline.set_decoder(ActiveDecoder::Pcm(dec));
                            }
                        }

                        let _ = pipeline.set_sample_rate(sample_rate);
                        self.state = PlayerState::Streaming;

                        client.send_stat(
                            StatEvent::TrackStarted,
                            pipeline.stream_buffer_available() as u32,
                            pipeline.output_buffer_available() as u32,
                            pipeline.frames_played as u32,
                            now_ms,
                        )?;

                        Ok(Some(CydMessage::PlaybackState(PlayState::Playing)))
                    }
                    StreamCommandType::Pause => {
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
                    StreamCommandType::Resume => {
                        self.state = PlayerState::Streaming;
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

                // Map 16.16 fixed-point (65536 = 100%) to 0-100 percentage
                let avg_gain = ((gain_left as u64 + gain_right as u64) / 2) as u32;
                let pct = core::cmp::min(100, (avg_gain * 100) / 65536) as u8;

                Ok(Some(CydMessage::Volume(pct)))
            }
            ServerCommand::SetDevice { sample_rate } => {
                self.current_sample_rate = sample_rate;
                let _ = pipeline.set_sample_rate(sample_rate);
                Ok(None)
            }
            ServerCommand::Flush => {
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
                if self.state == PlayerState::Streaming {
                    let _ = pipeline.ingest_stream_data(data);
                }
                Ok(None)
            }
            ServerCommand::Unknown { .. } => Ok(None),
        }
    }

    /// Executes one cycle of decoding and audio output.
    pub fn process_audio_step<S: AudioOutputSink>(
        &mut self,
        pipeline: &mut StreamPipeline<S>,
    ) -> usize {
        if self.state != PlayerState::Streaming {
            return 0;
        }

        // Try decoding a frame chunk
        let _ = pipeline.process_decode_step();

        // Write frames to the output sink
        pipeline.process_output_step().unwrap_or(0)
    }

    /// Emits periodic STAT heartbeats if interval has elapsed.
    pub fn check_heartbeat<S: AudioOutputSink>(
        &mut self,
        pipeline: &StreamPipeline<S>,
        client: &mut SlimprotoClient,
        now_ms: u32,
    ) -> std::io::Result<()> {
        if now_ms.saturating_sub(self.last_heartbeat_ms) >= self.heartbeat_interval_ms {
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
            PlayerState::Streaming => PlayState::Playing,
            PlayerState::Paused => PlayState::Paused,
            _ => PlayState::Stopped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
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
            assert_eq!(coordinator.state, PlayerState::Streaming);

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
}
