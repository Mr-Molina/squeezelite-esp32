//! `louder-esp32-app`
//!
//! Main application engine for Louder-ESP32-Plus audio streamer.

pub mod board;

pub use squeezelite_audio_core::{AudioRingBuffer, Equalizer, VolumeScale};
pub use squeezelite_cyd_link::{CydLinkProtocol, CydMessage, PlayState};
pub use squeezelite_proto::{AudioFormat, SlimprotoBuilder, SlimprotoParser, StatEvent};
pub use squeezelite_tas58xx::{Tas58xx, PowerDown, Standby, Play};

/// High-level player playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Disconnected,
    Connected,
    Streaming,
    Paused,
}

/// Orchestrates audio pipeline components.
pub struct PlayerCoordinator<'a> {
    pub state: PlayerState,
    pub volume: VolumeScale,
    pub equalizer: Equalizer<10>,
    pub ringbuffer: AudioRingBuffer<'a>,
}

impl<'a> PlayerCoordinator<'a> {
    pub fn new(storage: &'a mut [u8]) -> Self {
        Self {
            state: PlayerState::Disconnected,
            volume: VolumeScale::unity(),
            equalizer: Equalizer::new(),
            ringbuffer: AudioRingBuffer::new(storage),
        }
    }

    pub fn set_volume(&mut self, left_gain: u32, right_gain: u32) {
        self.volume.gain_left = left_gain;
        self.volume.gain_right = right_gain;
    }
}
