//! What is left of the embassy task skeleton after F2
//! (`foundation/single-loop`).
//!
//! The device firmware has one path: the loop in `main.rs`
//! (`pump_device_phase`). The core-0 phase ring that used to live here was
//! never started by `main.rs` and was removed. This module keeps only what the
//! core-1 audio migration (D1) builds on:
//!
//! - [`StreamParams`], [`CoordinatorCmd`], [`AudioEvent`] and the [`CTRL`] /
//!   [`EVT`] channels (depth [`CHANNEL_DEPTH`]) between the core-0 coordinator
//!   and the future core-1 audio tasks. Nothing sends on them yet.
//! - `device::run_core1_executor` and its `core1_heartbeat` task (feature
//!   `esp-rtos-core1`): `esp_rtos::start_second_core` runs an embassy executor
//!   on core 1 whose only task is the heartbeat. The default build starts
//!   core 1 by the manual path in `main.rs` and uses nothing from `device`.
#![cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use squeezelite_proto::AudioFormat;

/// Stream parameters handed from the coordinator to the audio tasks on `strm s`
/// (wired by the
/// core-1 audio migration).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamParams {
    pub format: AudioFormat,
    pub sample_rate_hz: u32,
    pub bits: u8,
    pub channels: u8,
    pub big_endian: bool,
    pub replay_gain: u32,
}

/// Coordinator (core 0) -> audio tasks command.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinatorCmd {
    Start(StreamParams),
    Pause,
    Unpause,
    Flush,
    SkipAheadMs(u32),
    SetGain { l: u32, r: u32 },
    SetMute(bool),
}

/// Audio tasks -> coordinator event.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    FirstFrameWritten,
    TrackBoundaryCrossed,
    OutputDrained,
    Underrun,
    DecoderError(u8),
}

/// Depth of [`CTRL`] and [`EVT`].
#[allow(dead_code)]
pub const CHANNEL_DEPTH: usize = 8;

/// Coordinator -> audio command channel. Unused until the core-1 audio
/// migration wires it (the linker drops it).
#[allow(dead_code)]
pub static CTRL: Channel<CriticalSectionRawMutex, CoordinatorCmd, CHANNEL_DEPTH> = Channel::new();

/// Audio -> coordinator event channel. Wired by the core-1 audio migration.
#[allow(dead_code)]
pub static EVT: Channel<CriticalSectionRawMutex, AudioEvent, CHANNEL_DEPTH> = Channel::new();

#[cfg(all(target_arch = "xtensa", feature = "esp-rtos-core1"))]
pub(crate) use device::run_core1_executor;

/// Device glue for the `esp-rtos-core1` build: the core-1 executor.
#[cfg(all(target_arch = "xtensa", feature = "esp-rtos-core1"))]
mod device {
    use crate::xtensa_hw::service_core1_park_request;
    use crate::xtensa_hw::CORE1_HEARTBEAT;
    use esp_rtos::embassy::Executor;
    use static_cell::StaticCell;

    /// Replaces the NOP heartbeat loop of the manual start path; polls
    /// `CORE1_PARK_REQ`/`CORE1_PARKED` in IRAM outside critical sections.
    #[embassy_executor::task]
    async fn core1_heartbeat() -> ! {
        loop {
            service_core1_park_request();
            CORE1_HEARTBEAT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            embassy_time::Timer::after_millis(1).await;
        }
    }

    /// Core-1 entry for `esp_rtos::start_second_core`. The executor must be
    /// created on core 1 itself (esp-rtos panics otherwise, Spike S1).
    pub(crate) fn run_core1_executor() -> ! {
        static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
        EXECUTOR1.init(Executor::new()).run(|spawner| {
            spawner.spawn(core1_heartbeat().unwrap_or_else(|_| esp_hal::system::software_reset()));
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_and_evt_channels_hold_eight_messages_in_order() {
        let params = StreamParams {
            format: AudioFormat::Flac,
            sample_rate_hz: 96_000,
            bits: 24,
            channels: 2,
            big_endian: false,
            replay_gain: 0,
        };
        let cmds = [
            CoordinatorCmd::Start(params),
            CoordinatorCmd::Pause,
            CoordinatorCmd::Unpause,
            CoordinatorCmd::Flush,
            CoordinatorCmd::SkipAheadMs(1500),
            CoordinatorCmd::SetGain {
                l: 65_536,
                r: 32_768,
            },
            CoordinatorCmd::SetMute(true),
            CoordinatorCmd::SetMute(false),
        ];
        for cmd in cmds {
            CTRL.try_send(cmd)
                .expect("CTRL must hold CHANNEL_DEPTH commands");
        }
        assert!(
            CTRL.try_send(CoordinatorCmd::Pause).is_err(),
            "CTRL depth is {CHANNEL_DEPTH}"
        );
        for cmd in cmds {
            assert_eq!(CTRL.try_receive(), Ok(cmd));
        }
        assert!(CTRL.try_receive().is_err());

        let evts = [
            AudioEvent::FirstFrameWritten,
            AudioEvent::TrackBoundaryCrossed,
            AudioEvent::OutputDrained,
            AudioEvent::Underrun,
            AudioEvent::DecoderError(3),
        ];
        for evt in evts {
            EVT.try_send(evt).unwrap();
        }
        for evt in evts {
            assert_eq!(EVT.try_receive(), Ok(evt));
        }
        assert!(EVT.try_receive().is_err());
    }

    #[test]
    fn test_wo255_bt_source_arbiter_priority_transitions() {
        use louder_esp32_app::{AudioSource, SourceArbiter};
        let mut arb = SourceArbiter::new();

        // SlimProto plays (priority 1)
        assert!(arb.request_source(AudioSource::SlimProto));
        assert_eq!(arb.active_source(), AudioSource::SlimProto);

        // Bluetooth starts (priority 2): preempts SlimProto (priority 1)
        assert!(arb.request_source(AudioSource::Bluetooth));
        assert_eq!(arb.active_source(), AudioSource::Bluetooth);
        assert!(arb.is_suspended(AudioSource::SlimProto));

        // Spotify starts (priority 3): preempts Bluetooth
        assert!(arb.request_source(AudioSource::Spotify));
        assert_eq!(arb.active_source(), AudioSource::Spotify);
        assert!(arb.is_suspended(AudioSource::Bluetooth));
        assert!(arb.is_suspended(AudioSource::SlimProto));

        // Bluetooth tries to request while Spotify plays: rejected (yields to Spotify)
        assert!(!arb.request_source(AudioSource::Bluetooth));
        assert_eq!(arb.active_source(), AudioSource::Spotify);

        // Spotify ends: releases and promotes Bluetooth
        arb.release_source(AudioSource::Spotify);
        assert_eq!(arb.active_source(), AudioSource::Bluetooth);

        // Bluetooth ends: releases and promotes SlimProto
        arb.release_source(AudioSource::Bluetooth);
        assert_eq!(arb.active_source(), AudioSource::SlimProto);
    }
}
