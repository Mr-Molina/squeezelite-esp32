//! `louder-esp32-app`
//!
//! Main application engine for Louder-ESP32-Plus audio streamer.
#![cfg_attr(target_arch = "xtensa", no_std)]
#![forbid(unsafe_code)]
#![allow(unknown_lints)]
#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::duplicated_attributes,
    clippy::needless_lifetimes
)]

pub mod amp;
pub mod backend_device;
pub mod board;
pub mod bt_a2dp;
pub mod clock_table;
pub mod config_handle;
pub mod console;
pub mod controls;
pub mod coordinator;
pub mod cyd;
pub mod dac_platform;
pub mod display_gds;
pub mod eth_w5500;
pub mod hal;
pub mod http_control;
pub mod io_traits;
pub mod net_device;
#[cfg(not(target_arch = "xtensa"))]
pub mod network;
pub mod ota_safety;
pub mod pipeline;
pub mod power_boards;
pub mod sink;
pub mod stream_codec;
pub mod wifi;

pub use amp::{AmpManager, AmpMode};
pub use board::{BoardModel, I2sPinConfig, ACTIVE_BOARD};
pub use bt_a2dp::{AvdtpStreamState, BtA2dpSink, HciTransport, InitStep, SbcDecoder};
pub use console::{
    AutoexecRunner, CliCommandDescriptor, CliCommandResult, CliRegistry, CliSideEffect, LogLevel,
    PlayerCliAction, SleepMode, SyslogEntry, SyslogRingBuffer, TelnetSession, BUILTIN_CLI_COMMANDS,
    TELNET_PORT, TELNET_PROMPT,
};
pub use controls::{
    build_butn_frame, build_ir_frame, ActrlsEntry, ActrlsMap, BatteryMonitor, ButtonDebouncer,
    ButtonGesture, ControlAction, EncoderMode, GpioExpanderKind, IrDecoder, IrFrame, IrProtocol,
    LedStripVisualizer, RotaryEncoder, RotaryEvent, StatusLedState,
};
pub use coordinator::{AudioSource, PlayerCoordinator, PlayerState, SourceArbiter};
pub use cyd::CydController;
pub use dac_platform::{
    AudioOutputPolicy, DacConfigSpec, DacModel, SpdifBmcEncoder, SpdifConfigSpec, SpdifPreamble,
};
pub use display_gds::{
    build_anic_frame, build_setd_display_size_frame, DisplayBusArbiter, DisplayColorMode,
    DisplayDriverKind, ExternalMetadataScreen, ExternalSourceTag, GdsDisplayEngine, GdsFramebuffer,
    LmsDisplayCommand, TextScroller, VisualizerMode,
};
pub use eth_w5500::{
    encode_spi_header, EthDuplex, EthLinkSpeed, PhyStatus, SpiAccessMode, W5500Driver, W5500Error,
    MAX_ETHERNET_FRAME_BYTES, W5500_CHIP_VERSION,
};
pub use hal::{
    BitDepth, CydUartBridge, EspI2sSink, EspWifiAdapter, FlashPartition, I2sDmaDriver, I2sFormat,
    LoopbackI2sDma, LoopbackUartPort, LoopbackWifiDriver, MemFlashPartition, NvsFlashBackend,
    TargetI2cBus, UartPort, WifiDriver, FLASH_SECTOR_SIZE,
};
pub use pipeline::{ActiveDecoder, StreamPipeline};
pub use power_boards::{BoardProfile, SleepManager, SleepState};
pub use sink::{NullAudioSink, SimulatedSink};
pub use wifi::{WifiConfig, WifiManager, WifiState};

#[cfg(not(target_arch = "xtensa"))]
pub use network::{HttpAudioStream, LmsDiscovery, RaopStreamClient, SlimprotoClient};
pub use squeezelite_audio_core::{AudioOutputSink, AudioRingBuffer, Equalizer, VolumeScale};
pub use squeezelite_config::{
    AudioConfig, FlashJournal, KeyValueStorage, MemConfigStore, StorageError, WifiCredentials,
};
pub use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, PlayState};
pub use squeezelite_proto::{AudioFormat, SlimprotoBuilder, SlimprotoParser, StatEvent};
pub use squeezelite_tas58xx::{Play, PowerDown, Standby, Tas58xx};
