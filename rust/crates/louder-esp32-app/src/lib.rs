//! `louder-esp32-app`
//!
//! Main application engine for Louder-ESP32-Plus audio streamer.
#![cfg_attr(target_arch = "xtensa", no_std)]
#![forbid(unsafe_code)]
#![allow(unknown_lints)]
#![allow(clippy::chunks_exact_to_as_chunks, clippy::needless_lifetimes)]

pub mod amp;
pub mod board;
pub mod bt_a2dp;
pub mod clock_table;
pub mod coordinator;
pub mod cyd;
pub mod hal;
pub mod http_control;
pub mod io_traits;
#[cfg(not(target_arch = "xtensa"))]
pub mod network;
pub mod ota_safety;
pub mod pipeline;
pub mod sink;
pub mod stream_codec;
pub mod wifi;

pub use amp::{AmpManager, AmpMode};
pub use board::{BoardModel, I2sPinConfig, ACTIVE_BOARD};
pub use bt_a2dp::{AvdtpStreamState, BtA2dpSink, HciTransport, InitStep, SbcDecoder};
pub use coordinator::{PlayerCoordinator, PlayerState};
pub use cyd::CydController;
pub use hal::{
    BitDepth, CydUartBridge, EspI2sSink, EspWifiAdapter, FlashPartition, I2sDmaDriver, I2sFormat,
    LoopbackI2sDma, LoopbackUartPort, LoopbackWifiDriver, MemFlashPartition, NvsFlashBackend,
    TargetI2cBus, UartPort, WifiDriver, FLASH_SECTOR_SIZE,
};
pub use pipeline::{ActiveDecoder, StreamPipeline};
pub use sink::{NullAudioSink, SimulatedSink};
pub use wifi::{WifiConfig, WifiManager, WifiState};

pub use squeezelite_audio_core::{AudioOutputSink, AudioRingBuffer, Equalizer, VolumeScale};
pub use squeezelite_config::{AudioConfig, FlashJournal, KeyValueStorage, MemConfigStore, StorageError, WifiCredentials};
#[cfg(not(target_arch = "xtensa"))]
pub use network::{HttpAudioStream, LmsDiscovery, SlimprotoClient};
pub use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, PlayState};
pub use squeezelite_proto::{AudioFormat, SlimprotoBuilder, SlimprotoParser, StatEvent};
pub use squeezelite_tas58xx::{Play, PowerDown, Standby, Tas58xx};

