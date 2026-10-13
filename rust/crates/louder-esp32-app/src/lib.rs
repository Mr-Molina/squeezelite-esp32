//! `louder-esp32-app`
//!
//! Main application engine for Louder-ESP32-Plus audio streamer.

pub mod amp;
pub mod board;
pub mod coordinator;
pub mod cyd;
pub mod network;
pub mod pipeline;
pub mod sink;
pub mod wifi;

pub use amp::{AmpManager, AmpMode};
pub use coordinator::{PlayerCoordinator, PlayerState};
pub use cyd::CydController;
pub use pipeline::{ActiveDecoder, StreamPipeline};
pub use sink::{NullAudioSink, SimulatedSink};
pub use wifi::{WifiConfig, WifiManager, WifiState};

pub use squeezelite_audio_core::{AudioOutputSink, AudioRingBuffer, Equalizer, VolumeScale};
pub use squeezelite_config::{AudioConfig, FlashJournal, KeyValueStorage, MemConfigStore, StorageError, WifiCredentials};
pub use network::{LmsDiscovery, SlimprotoClient};
pub use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, PlayState};
pub use squeezelite_proto::{AudioFormat, SlimprotoBuilder, SlimprotoParser, StatEvent};
pub use squeezelite_tas58xx::{Play, PowerDown, Standby, Tas58xx};
