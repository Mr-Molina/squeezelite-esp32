//! Embassy task skeleton for the device firmware (Task 8, spec §3.1), driving the shared
//! [`PlayerCoordinator`](louder_esp32_app::PlayerCoordinator) engine (Task 9).
//!
//! # What this module is
//!
//! The firmware's work is split into the embassy tasks of spec §3.1. Since Task 9 the player
//! protocol (SlimProto frames, `STAT` events, HTTP audio fetch, decoder selection, `RESP`/`META`/
//! `DSCO`) is NOT implemented here any more: the `slimproto_session` task runs
//! `PlayerCoordinator::tick` over the device trait implementations ([`net_device`] over the
//! smoltcp sockets, [`backend_device`] over the amplifier and the I2S0 clock), the very engine
//! the host tests and the session-replay fixture exercise. The decode and output tasks drive the
//! pipeline through the coordinator's pump path (`pump_decode` / `pump_output`), so every state
//! transition is seen by the engine.
//!
//! | task                | work                                                                       |
//! |---------------------|----------------------------------------------------------------------------|
//! | `watchdog_lease`    | WDT feed + lap timestamp, Gate 3 network lease, 2 s amp fault/telemetry     |
//! | `bt_a2dp`           | lazy BT controller bring-up, A2DP sink poll (feeds the pipeline when idle)  |
//! | `net_stack`         | Wi-Fi STA connect drive, smoltcp poll + DHCPv4 events                       |
//! | `http_control`      | Gate 4 port-80 server + deferred reboot actions (settings flushed first)    |
//! | `slimproto_session` | LMS control socket: connect, `HELO`/`SETD` (`hello`), `coordinator.tick`    |
//! |                     | (protocol + play state only; never decodes: `external_audio_pump`)         |
//! | `cyd_bridge`        | CYD command poll, 1 s progress update                                      |
//! | `decode_pump`       | `coordinator.pump_decode` (one frame, or a short resync burst)             |
//! | `i2s_feed`          | `coordinator.pump_output` before/after decoding, DAC gate, silence pump    |
//! | `config_writer`     | debounced persistence (2 s quiet), RAM journal; flash: see "Settings"      |
//!
//! The HTTP audio fetch (spec's `audio_fetch` task) is step 3 of `coordinator.tick`, so it runs
//! inside `slimproto_session`; the separate task is gone until the network/audio split.
//!
//! [`net_device`]: louder_esp32_app::net_device
//! [`backend_device`]: louder_esp32_app::backend_device
//!
//! # Why a phase ring
//!
//! The tasks still hand a baton around in a fixed order ([`PHASE_ORDER`]), one block per hand-off,
//! one lap == one iteration of the former superloop. That keeps the DMA-first audio order (top
//! up the I2S DMA BEFORE a ~4.5 ms MP3 frame decode, then again after it) and guarantees that the
//! shared state is only ever borrowed by one block at a time and never across an `.await`, so no
//! network/audio concurrency is introduced here (real concurrency, with the `CTRL`/`EVT`
//! channels below, comes with the core-1 audio migration).
//!
//! # Core placement
//!
//! Spec §3.1 places `decode_pump` and `i2s_feed` on core 1. Core-1 start under esp-rtos is NOT
//! yet validated on hardware (Spike S1 owner gate pending), so in BOTH build configurations the
//! audio tasks run on the core-0 executor as ring members. Core 1 is started by:
//! - default: the battle-tested manual path (park APP_CPU, copy the PRO flash-MMU/cache
//!   registers, `CpuControl::start_app_core` running the IRAM NOP heartbeat loop);
//! - feature `esp-rtos-core1`: `esp_rtos::start_second_core` running an embassy executor whose
//!   only task is `core1_heartbeat`.
//!
//! # Settings persistence (Task 10, spec §3.5)
//!
//! Every runtime change (`setd` name in the coordinator, `/cmd.json` EQ/AGAIN/volume, LMS
//! `audg` volume, CYD volume) goes through the [`ConfigHandle`]. `config_writer` polls it every
//! `CONFIG_WRITER_POLL_MS` (100 ms) and, once a change has been quiet for `DEBOUNCE_MS` (2 s),
//! serializes the snapshot into the RAM journal (DRAM sector image + next sequence; target
//! sector and sequence tracked in RAM since the boot load). Flash is written:
//! - default build: ONLY on the reboot paths (`/recovery.json`, `/reboot.json`,
//!   `/reboot_ota.json`), by `flush_settings_before_reboot` -- `flush_now`, then the certified
//!   pre-reboot write of sector A (0x3FE000) with APP_CPU parked, byte-identical to the
//!   certified build (`main.rs` `SETTINGS_FLASH_TARGET`). No flash I/O at runtime (WO-108/WO-109
//!   deadlock);
//! - feature `runtime-flash-persist` (default OFF): additionally at runtime, hazard-gated, in
//!   `runtime_flash_commit` (rules documented there), with dual-sector ping-pong.
//!
//! [`ConfigHandle`]: louder_esp32_app::config_handle::ConfigHandle
//!
//! # Memory
//!
//! The stack gate is headroom-based (Task 8 ruling). [`FirmwareState`] and the large values it
//! points at live in statics built in place during bring-up, and each block runs in its own
//! `#[inline(never)]` frame. The coordinator keeps its large temporaries (server frame buffer,
//! stream read chunk) in out-of-line helpers, and the device stream source hands the response
//! head / ICY block over by callback, so no 4 KB value is ever built on this stack.
#![cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use squeezelite_proto::AudioFormat;

/// Stream parameters handed from the coordinator to the audio tasks on `strm s` (wired by the
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

/// Coordinator -> audio command channel. Unused while the audio tasks are ring members on core
/// 0 (the linker drops it); wired by the core-1 audio migration.
#[allow(dead_code)]
pub static CTRL: Channel<CriticalSectionRawMutex, CoordinatorCmd, CHANNEL_DEPTH> = Channel::new();

/// Audio -> coordinator event channel. Wired by the core-1 audio migration.
#[allow(dead_code)]
pub static EVT: Channel<CriticalSectionRawMutex, AudioEvent, CHANNEL_DEPTH> = Channel::new();

/// Members of the core-0 phase ring (every spec §3.1 task except the timer-driven `config_writer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingTask {
    WatchdogLease,
    BtA2dp,
    NetStack,
    HttpControl,
    SlimprotoSession,
    CydBridge,
    DecodePump,
    I2sFeed,
}

impl RingTask {
    pub const COUNT: usize = 8;

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One block of the lap. Discriminants follow [`PHASE_ORDER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `wdt.feed()` + the lap timestamp shared by every later phase of the lap.
    WatchdogFeed,
    /// Lazy BT controller bring-up, A2DP sink poll.
    Bluetooth,
    /// Wi-Fi STA association drive.
    WifiConnect,
    /// Gate 3 dead-man network lease.
    LeaseCheck,
    /// smoltcp poll + DHCPv4 events.
    NetPoll,
    /// Gate 4 port-80 server + deferred reboot action.
    HttpServe,
    /// LMS control socket upkeep, `HELO`, then one `PlayerCoordinator::tick` (server frame,
    /// HTTP audio fetch, audio step, deferred `STAT`s and heartbeat).
    SlimSession,
    /// CYD command poll.
    CydCommand,
    /// `pump_output` before decoding (drain pending PCM, top up the I2S DMA).
    OutputTopUp,
    /// `pump_decode`: at most one frame (or a short resync burst).
    DecodeBurst,
    /// `pump_output` again, rate sync, DAC gate, then the silence pump.
    OutputTopUpAndSilence,
    /// 2 s TAS58xx fault recovery + telemetry.
    AmpHealth,
    /// 1 s CYD progress update.
    CydProgress,
}

/// The lap order, one entry per block.
pub const PHASE_ORDER: [Phase; 13] = [
    Phase::WatchdogFeed,
    Phase::Bluetooth,
    Phase::WifiConnect,
    Phase::LeaseCheck,
    Phase::NetPoll,
    Phase::HttpServe,
    Phase::SlimSession,
    Phase::CydCommand,
    Phase::OutputTopUp,
    Phase::DecodeBurst,
    Phase::OutputTopUpAndSilence,
    Phase::AmpHealth,
    Phase::CydProgress,
];

impl Phase {
    /// The ring task that runs this block.
    pub const fn owner(self) -> RingTask {
        match self {
            Phase::WatchdogFeed | Phase::LeaseCheck | Phase::AmpHealth => RingTask::WatchdogLease,
            Phase::Bluetooth => RingTask::BtA2dp,
            Phase::WifiConnect | Phase::NetPoll => RingTask::NetStack,
            Phase::HttpServe => RingTask::HttpControl,
            Phase::SlimSession => RingTask::SlimprotoSession,
            Phase::CydCommand | Phase::CydProgress => RingTask::CydBridge,
            Phase::DecodeBurst => RingTask::DecodePump,
            Phase::OutputTopUp | Phase::OutputTopUpAndSilence => RingTask::I2sFeed,
        }
    }

    /// The phase after this one (wrapping: the last phase hands back to the first).
    pub const fn next(self) -> Phase {
        PHASE_ORDER[(self as usize + 1) % PHASE_ORDER.len()]
    }
}

#[cfg(target_arch = "xtensa")]
pub(crate) use device::{run_core0_executor, FirmwareState, Shared, UDP_PLACEHOLDER_SOCKETS};
#[cfg(all(target_arch = "xtensa", feature = "esp-rtos-core1"))]
pub(crate) use device::run_core1_executor;

/// Device glue: the shared state, the ring tasks, and the blocks they run.
#[cfg(target_arch = "xtensa")]
mod device {
    use super::{Phase, RingTask, PHASE_ORDER};
    use crate::xtensa_hw::{
        now_millis, Core1Ctl, DeviceClockRegs, Esp32I2cBus,
        Esp32Uart2Port, Esp32VhciTransport, HwStreamPipeline, SmolDevice, CORE1_HEARTBEAT,
        DMA_OUTSTANDING_FRAMES,
    };
    use core::cell::RefCell;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Waker};
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use embassy_sync::signal::Signal;
    use esp_hal::gpio::{Input, Output};
    use esp_hal::timer::timg::Wdt;
    use esp_println::println;
    use esp_radio::wifi::{Interface as WifiInterface, WifiController};
    use esp_rtos::embassy::Executor;
    use esp_storage::FlashStorage;
    use louder_esp32_app::backend_device::Esp32AudioBackend;
    use louder_esp32_app::config_handle::{ConfigHandle, ConfigWriter, RamJournal};
    use louder_esp32_app::bt_a2dp::BtA2dpSink;
    use louder_esp32_app::http_control::{
        handle_http_request_streaming, DeadManLeaseWatchdog, HttpControlAction, HttpStatusSnapshot,
        LeaseHealth,
    };
    use louder_esp32_app::io_traits::{NetAddr, SlimTransport};
    use louder_esp32_app::net_device::{
        HttpStreamState, SlimLinkState, SlimReconnect, SmoltcpSlimTransport, SmoltcpStreamSource,
        TcpPhase, TcpSocketOps,
    };
    use louder_esp32_app::ota_safety::{OtaSelectEntry, WifiStaCredentials, OTADATA_PARTITION_OFFSET, SETTINGS_JOURNAL_OFFSET};
    use louder_esp32_app::pipeline::ActiveDecoder;
    use louder_esp32_app::{AmpManager, CydUartBridge, PlayerCoordinator, PlayerState};
    use smoltcp::iface::{Context as SmolContext, Interface as SmolInterface, SocketHandle, SocketSet};
    use smoltcp::socket::{dhcpv4, tcp, AnySocket};
    use smoltcp::time::{Duration as SmolDuration, Instant as SmolInstant};
    use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};
    use squeezelite_audio_core::EqPreset;
    use squeezelite_cyd_link::CydMessage;
    use squeezelite_decoders::PcmDecoder;
    use squeezelite_proto::SlimprotoBuilder;
    use static_cell::StaticCell;

    /// `HELO` device id (12 = SqueezeESP32/squeezelite).
    const HELO_DEVICE_ID: u8 = 12;
    /// Audio-socket connect/retransmit timeout: an unreachable stream server ends in `STMn`
    /// instead of a connect that never resolves.
    const STREAM_SOCKET_TIMEOUT: SmolDuration = SmolDuration::from_secs(10);

    /// Shared firmware state: the hardware, the engine, and the few device-side loop variables
    /// the non-protocol blocks need. The large members are `&'static mut` into statics built in
    /// place during bring-up (see the module docs).
    pub(crate) struct FirmwareState {
        // --- hardware kept alive for the life of the firmware ---
        pub(crate) wdt: Wdt<esp_hal::peripherals::TIMG0<'static>>,
        pub(crate) flash: FlashStorage<'static>,
        pub(crate) core1: Core1Ctl,
        pub(crate) _pwdn: Output<'static>,
        pub(crate) _fault: Input<'static>,
        pub(crate) wifi_controller: WifiController<'static>,
        pub(crate) sta_iface: WifiInterface,
        pub(crate) smol_iface: &'static mut SmolInterface,
        pub(crate) sockets: SocketSet<'static>,
        pub(crate) dhcp_handle: SocketHandle,
        pub(crate) http_handle: SocketHandle,
        pub(crate) slim_handle: SocketHandle,
        pub(crate) audio_handle: SocketHandle,
        /// Spec §3.6 UDP placeholders (LMS discovery / mDNS): allocated, unbound, unused until P3.
        pub(crate) _udp_placeholder_handles: [SocketHandle; UDP_PLACEHOLDER_SOCKETS],
        pub(crate) bt_periph: Option<esp_hal::peripherals::BT<'static>>,
        pub(crate) bt_transport: Option<Esp32VhciTransport<'static>>,
        pub(crate) bt_sink: &'static mut BtA2dpSink,
        pub(crate) amp: &'static mut AmpManager<Esp32I2cBus<'static>>,
        pub(crate) cyd_bridge: &'static mut CydUartBridge<Esp32Uart2Port<'static>>,
        pub(crate) pipeline: &'static mut HwStreamPipeline,
        pub(crate) coordinator: &'static mut PlayerCoordinator,
        // --- device I/O adapter state (net_device) ---
        pub(crate) slim_link: SlimLinkState,
        pub(crate) stream_state: HttpStreamState,
        // --- static buffers ---
        pub(crate) http_resp_buf: &'static mut [u8; 1536],
        // --- configuration (Task 10) ---
        /// Runtime configuration every settings change goes through.
        pub(crate) config: &'static ConfigHandle,
        /// RAM settings journal (DRAM image + target/sequence from the boot load).
        pub(crate) journal: &'static mut RamJournal<'static>,
        /// Boot-resolved Wi-Fi credentials (journal, else NVS, else first-boot default).
        pub(crate) wifi_sta_creds: &'static WifiStaCredentials,
        /// Boot-resolved LMS SlimProto target (journal, else first-boot default).
        pub(crate) lms_ip: [u8; 4],
        pub(crate) lms_port: u16,
        pub(crate) current_vol_pct: u8,
        // --- loop variables ---
        pub(crate) local_port_counter: u16,
        pub(crate) bt_audio_active: bool,
        pub(crate) boot_ms: u64,
        pub(crate) lease_watchdog: DeadManLeaseWatchdog,
        pub(crate) assigned_ip: Option<[u8; 4]>,
        pub(crate) assigned_mask: [u8; 4],
        pub(crate) assigned_gw: [u8; 4],
        pub(crate) pending_http_action: HttpControlAction,
        pub(crate) pending_action_deadline_ms: u64,
        pub(crate) pending_http_body: &'static [u8],
        pub(crate) pending_http_started_ms: u64,
        pub(crate) pending_http_closing: bool,
        pub(crate) pending_close_deadline_ms: u64,
        /// `HELO` went out on the current control connection.
        pub(crate) helo_sent: bool,
        pub(crate) last_connect_attempt_ms: u64,
        /// LMS control-link reconnect policy (C connect loop + MK5 backoff).
        pub(crate) slim_reconnect: SlimReconnect,
        /// The control socket was open at the end of the last `slim_session`.
        pub(crate) slim_was_open: bool,
        /// `serv` target taken from the coordinator, applied by the next `slim_session`.
        pub(crate) slim_switch_to: Option<[u8; 4]>,
        pub(crate) last_fault_check_ms: u64,
        pub(crate) latest_telemetry: Option<squeezelite_tas58xx::Tas5805mTelemetry>,
        pub(crate) last_cyd_progress_ms: u64,
        /// The lap timestamp (`now_millis()` at the top of each lap).
        pub(crate) now_ms: u64,
    }

    /// Number of UDP placeholder sockets in the smoltcp set (spec §3.6).
    pub(crate) const UDP_PLACEHOLDER_SOCKETS: usize = 2;

    /// The state shared by the ring tasks. Only the baton holder borrows it, and never across
    /// an `.await`, so the `RefCell` borrow can not fail.
    pub(crate) struct Shared(RefCell<FirmwareState>);

    impl Shared {
        pub(crate) fn new(state: FirmwareState) -> Self {
            Self(RefCell::new(state))
        }
    }

    /// One baton signal per ring task; the value is the phase it must run next.
    static TURN: [Signal<CriticalSectionRawMutex, Phase>; RingTask::COUNT] =
        [const { Signal::new() }; RingTask::COUNT];

    /// Body shared by every ring task: wait for the baton, run the block, pass the baton on.
    async fn run_ring(me: RingTask, shared: &'static Shared) -> ! {
        loop {
            let phase = TURN[me.index()].wait().await;
            run_phase(&mut shared.0.borrow_mut(), phase);
            let next = phase.next();
            TURN[next.owner().index()].signal(next);
        }
    }

    /// Runs one block. Routing by [`Phase::owner`] guarantees each block executes inside the
    /// task named in the module table.
    fn run_phase(s: &mut FirmwareState, phase: Phase) {
        match phase {
            Phase::WatchdogFeed => watchdog_feed(s),
            Phase::Bluetooth => bt_poll(s),
            Phase::WifiConnect => wifi_connect(s),
            Phase::LeaseCheck => lease_check(s),
            Phase::NetPoll => net_poll(s),
            Phase::HttpServe => http_serve(s),
            Phase::SlimSession => slim_session(s),
            Phase::CydCommand => cyd_command(s),
            Phase::OutputTopUp => output_top_up_first(s),
            Phase::DecodeBurst => decode_burst(s),
            Phase::OutputTopUpAndSilence => output_top_up_final(s),
            Phase::AmpHealth => amp_health(s),
            Phase::CydProgress => cyd_progress(s),
        }
    }

    /// Core 0: WDT feed, Gate 3 network lease, amp health.
    #[embassy_executor::task]
    async fn watchdog_lease(shared: &'static Shared) -> ! {
        run_ring(RingTask::WatchdogLease, shared).await
    }

    /// Core 0: BT Classic A2DP sink.
    #[embassy_executor::task]
    async fn bt_a2dp(shared: &'static Shared) -> ! {
        run_ring(RingTask::BtA2dp, shared).await
    }

    /// Core 0: Wi-Fi association + smoltcp interface/DHCP.
    #[embassy_executor::task]
    async fn net_stack(shared: &'static Shared) -> ! {
        run_ring(RingTask::NetStack, shared).await
    }

    /// Core 0: port-80 control/recovery server.
    #[embassy_executor::task]
    async fn http_control(shared: &'static Shared) -> ! {
        run_ring(RingTask::HttpControl, shared).await
    }

    /// Core 0: LMS slimproto session (`PlayerCoordinator::tick`, including the HTTP audio fetch).
    #[embassy_executor::task]
    async fn slimproto_session(shared: &'static Shared) -> ! {
        run_ring(RingTask::SlimprotoSession, shared).await
    }

    /// Core 0: CYD display UART bridge.
    #[embassy_executor::task]
    async fn cyd_bridge(shared: &'static Shared) -> ! {
        run_ring(RingTask::CydBridge, shared).await
    }

    /// Stream ring -> decoder -> output ring. Spec §3.1 core 1; runs on core 0 until the S1
    /// owner gate validates core-1 bring-up (see module docs).
    #[embassy_executor::task]
    async fn decode_pump(shared: &'static Shared) -> ! {
        run_ring(RingTask::DecodePump, shared).await
    }

    /// Output ring -> I2S DMA, silence when idle. Spec §3.1 core 1; runs on core 0 until the S1
    /// owner gate validates core-1 bring-up (see module docs).
    #[embassy_executor::task]
    async fn i2s_feed(shared: &'static Shared) -> ! {
        run_ring(RingTask::I2sFeed, shared).await
    }

    /// Poll period of `config_writer`: a change is journaled between `DEBOUNCE_MS` and
    /// `DEBOUNCE_MS + CONFIG_WRITER_POLL_MS` after the last update.
    pub(crate) const CONFIG_WRITER_POLL_MS: u64 = 100;

    /// Core 0: debounced config persistence (spec §3.5, module docs "Settings persistence").
    /// Not a ring member: it wakes on its own timer and borrows the shared state only between
    /// ring hand-offs (no ring block holds the borrow across an `.await`).
    #[embassy_executor::task]
    async fn config_writer(shared: &'static Shared) -> ! {
        let mut writer = ConfigWriter::new(shared.0.borrow().config, now_millis());
        #[cfg(feature = "runtime-flash-persist")]
        let mut flash_retry_at_ms = 0u64;
        loop {
            embassy_time::Timer::after_millis(CONFIG_WRITER_POLL_MS).await;
            let Ok(mut s) = shared.0.try_borrow_mut() else {
                continue;
            };
            config_writer_step(&mut s, &mut writer);
            #[cfg(feature = "runtime-flash-persist")]
            runtime_flash_commit(&mut s, &mut flash_retry_at_ms);
        }
    }

    /// `config_writer`: one debounce step; a quiet dirty config is serialized into the RAM
    /// journal (never flash).
    #[inline(never)]
    fn config_writer_step(s: &mut FirmwareState, writer: &mut ConfigWriter) {
        writer.poll(s.config, &mut *s.journal, now_millis());
    }

    /// Feature `runtime-flash-persist` (default OFF): commit the RAM journal to flash at runtime.
    ///
    /// HAZARD (WO-108 / WO-109, hardware-proven deadlock on this board): esp-storage 0.10.0's
    /// `FlashStorage::read_nor` calls `spiflash_read` WITHOUT parking APP_CPU, and
    /// `spiflash_erase_sector` masks interrupts for 45-80 ms and reconfigures SPI1 without
    /// disabling the SPI0 cache, while 4 MB PSRAM, XIP `.text` and esp-radio Wi-Fi interrupts are
    /// live. The default build therefore never touches flash at runtime (the RAM journal reaches
    /// flash only on the certified pre-reboot path). This gate allows a runtime commit only under
    /// ALL of these rules:
    /// 1. never while audio plays: the player is neither `Playing` nor `Buffering` and no
    ///    Bluetooth stream is active;
    /// 2. APP_CPU is parked through the certified `Core1Ctl::park_for_flash_write` for the whole
    ///    erase + write, and unparked after (manual core-1 path only: `main.rs` refuses to build
    ///    this feature with `esp-rtos-core1`);
    /// 3. NO flash read: `erase` + `write_nor` only (no `read_nor`, no read-modify-write
    ///    `FlashStorage::write`). The target sector and sequence come from the RAM journal, which
    ///    carries them from the boot-time load;
    /// 4. DRAM-only buffers: `write_nor` writes straight from the word-aligned RAM journal image
    ///    in `.dram2_uninit` (no bounce buffer, nothing in PSRAM);
    /// 5. dual-sector ping-pong at 0x3FE000 / 0x3FF000 with the FlashJournal sequence logic
    ///    (`FlashTargetPolicy::PingPong`, selected in `main.rs` for this feature only): the
    ///    target is never the sector holding the newest valid journal, so power loss mid-write
    ///    leaves the previous journal intact;
    /// 6. a failed erase/write is retried after 60 s, never in a loop.
    ///
    /// Not hardware-certified: power-cycle persistence with this feature is an OWNER GATE (enable
    /// it, then soak with settings changes during idle and during playback).
    #[cfg(feature = "runtime-flash-persist")]
    #[inline(never)]
    fn runtime_flash_commit(s: &mut FirmwareState, retry_at_ms: &mut u64) {
        const RETRY_MS: u64 = 60_000;
        let now_ms = now_millis();
        if now_ms < *retry_at_ms
            || matches!(s.coordinator.state, PlayerState::Playing | PlayerState::Buffering)
            || s.bt_audio_active
        {
            return;
        }
        let FirmwareState {
            ref mut flash,
            ref mut core1,
            ref mut journal,
            ..
        } = *s;
        let Some(image) = journal.pending_image() else {
            return;
        };
        let offset = image.slot.offset(SETTINGS_JOURNAL_OFFSET);
        let end = offset + louder_esp32_app::config_handle::JOURNAL_SECTOR_SIZE as u32;
        core1.park_for_flash_write();
        let written = flash.erase(offset, end).is_ok()
            && flash.write_nor(offset, &image.sector[..image.used_len]).is_ok();
        core1.unpark_after_flash_write();
        if written {
            journal.mark_flash_written();
        } else {
            *retry_at_ms = now_ms + RETRY_MS;
        }
    }

    /// Before every reboot action: journal any pending change into RAM now (`flush_now`, no
    /// debounce), then write the RAM journal to its target sector with the certified
    /// pre-reboot sequence: APP_CPU parked first, DRAM buffer, sector erase + write. In the
    /// default build this is the ONLY settings flash write (WO-109) and the target is always
    /// sector A (0x3FE000), byte-identical to the certified build.
    #[inline(never)]
    fn flush_settings_before_reboot(s: &mut FirmwareState) {
        s.config.flush_now(&mut *s.journal);
        if let Some(image) = s.journal.pending_image() {
            // Default build: sector A (0x3FE000) as a constant, exactly the certified write
            // (`FlashTargetPolicy::FixedA` keeps `image.slot` at A too). Any image is valid in
            // either sector (the newest sequence wins at boot), so the constant is always safe.
            #[cfg(not(feature = "runtime-flash-persist"))]
            let offset = SETTINGS_JOURNAL_OFFSET;
            #[cfg(feature = "runtime-flash-persist")]
            let offset = image.slot.offset(SETTINGS_JOURNAL_OFFSET);
            s.core1.park_for_flash_write();
            let _ = s.flash.erase(offset, offset + 4096);
            let _ = s.flash.write(offset, image.sector);
        }
    }

    /// Spawns the core-0 tasks, hands the baton to the first phase and runs the executor.
    /// Never returns.
    pub(crate) fn run_core0_executor(shared: &'static Shared) -> ! {
        static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
        EXECUTOR0.init(Executor::new()).run(|spawner| {
            spawner.spawn(watchdog_lease(shared).unwrap());
            spawner.spawn(bt_a2dp(shared).unwrap());
            spawner.spawn(net_stack(shared).unwrap());
            spawner.spawn(http_control(shared).unwrap());
            spawner.spawn(slimproto_session(shared).unwrap());
            spawner.spawn(cyd_bridge(shared).unwrap());
            // Core-0 placement of the audio tasks: see "Core placement" in the module docs.
            spawner.spawn(decode_pump(shared).unwrap());
            spawner.spawn(i2s_feed(shared).unwrap());
            spawner.spawn(config_writer(shared).unwrap());
            let first = PHASE_ORDER[0];
            TURN[first.owner().index()].signal(first);
        })
    }

    /// Core 1 (feature `esp-rtos-core1`): replaces the NOP heartbeat loop of the manual start
    /// path; the audio tasks join this executor once the S1 owner gate passes.
    #[cfg(feature = "esp-rtos-core1")]
    #[embassy_executor::task]
    async fn core1_heartbeat() -> ! {
        loop {
            CORE1_HEARTBEAT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            embassy_time::Timer::after_millis(1).await;
        }
    }

    /// Core-1 entry for `esp_rtos::start_second_core` (feature `esp-rtos-core1`). The executor
    /// must be created on core 1 itself (esp-rtos panics otherwise, Spike S1).
    #[cfg(feature = "esp-rtos-core1")]
    pub(crate) fn run_core1_executor() -> ! {
        static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
        EXECUTOR1.init(Executor::new()).run(|spawner| {
            spawner.spawn(core1_heartbeat().unwrap());
        })
    }

    // ------------------------------------------------------------------------------------------
    // smoltcp binding of the device I/O adapters (`net_device`)
    // ------------------------------------------------------------------------------------------

    /// A smoltcp TCP socket borrowed for one block, as [`TcpSocketOps`]. Only the audio socket
    /// gets `connect` (interface context, local address and the rotating local port).
    struct SmolTcp<'a> {
        sock: &'a mut tcp::Socket<'static>,
        connect: Option<(&'a mut SmolContext, [u8; 4], &'a mut u16)>,
    }

    impl<'a> SmolTcp<'a> {
        fn new(sock: &'a mut tcp::Socket<'static>) -> Self {
            Self { sock, connect: None }
        }
    }

    /// Next local port in 49152..=65535 (the firmware's rotating ephemeral ports).
    fn next_local_port(counter: &mut u16) -> u16 {
        *counter = 49152 + ((counter.wrapping_add(1) - 49152) & 0x3FFF);
        *counter
    }

    impl TcpSocketOps for SmolTcp<'_> {
        fn phase(&self) -> TcpPhase {
            match self.sock.state() {
                tcp::State::Closed | tcp::State::Listen => TcpPhase::Closed,
                tcp::State::SynSent | tcp::State::SynReceived => TcpPhase::Connecting,
                tcp::State::Established | tcp::State::FinWait1 | tcp::State::FinWait2 => {
                    TcpPhase::Established
                }
                tcp::State::CloseWait
                | tcp::State::LastAck
                | tcp::State::Closing
                | tcp::State::TimeWait => TcpPhase::PeerClosed,
            }
        }

        fn connect(&mut self, remote: NetAddr) -> bool {
            let Some((cx, local_ip, port)) = self.connect.as_mut() else {
                return false;
            };
            self.sock.abort();
            self.sock.set_timeout(Some(STREAM_SOCKET_TIMEOUT));
            let remote_ep = (IpAddress::Ipv4(Ipv4Address::from(remote.ip)), remote.port);
            let local_ep = (IpAddress::Ipv4(Ipv4Address::from(*local_ip)), next_local_port(port));
            self.sock.connect(cx, remote_ep, local_ep).is_ok()
        }

        fn abort(&mut self) {
            self.sock.abort();
        }

        fn send_capacity(&self) -> usize {
            self.sock.send_capacity()
        }

        fn send_queue(&self) -> usize {
            self.sock.send_queue()
        }

        fn send_slice(&mut self, data: &[u8]) -> usize {
            self.sock.send_slice(data).unwrap_or(0)
        }

        fn recv_queue(&self) -> usize {
            self.sock.recv_queue()
        }

        fn recv_capacity(&self) -> usize {
            self.sock.recv_capacity()
        }

        fn peek_slice(&mut self, buf: &mut [u8]) -> usize {
            self.sock.peek_slice(buf).unwrap_or(0)
        }

        fn recv_slice(&mut self, buf: &mut [u8]) -> usize {
            self.sock.recv_slice(buf).unwrap_or(0)
        }
    }

    /// The control and audio sockets, borrowed together (disjoint) out of the socket set.
    fn slim_and_audio_sockets<'a>(
        sockets: &'a mut SocketSet<'static>,
        slim_handle: SocketHandle,
        audio_handle: SocketHandle,
    ) -> Option<(&'a mut tcp::Socket<'static>, &'a mut tcp::Socket<'static>)> {
        let (mut slim, mut audio) = (None, None);
        for (handle, socket) in sockets.iter_mut() {
            if handle == slim_handle {
                slim = tcp::Socket::downcast_mut(socket);
            } else if handle == audio_handle {
                audio = tcp::Socket::downcast_mut(socket);
            }
        }
        Some((slim?, audio?))
    }

    /// LMS still owns the output: an LMS stream is connecting/open and its track has not ended.
    fn lms_audio_active(coordinator: &PlayerCoordinator) -> bool {
        coordinator.stream_active() && coordinator.state != PlayerState::Stopped
    }

    // ------------------------------------------------------------------------------------------
    // Blocks, in PHASE_ORDER. `#[inline(never)]` keeps each block in its own stack frame so only
    // one block's temporaries are live at a time.
    // ------------------------------------------------------------------------------------------

    /// `watchdog_lease`: feed the 20 s TIMG0 WDT and take the lap's timestamp.
    #[inline(never)]
    fn watchdog_feed(s: &mut FirmwareState) {
        s.wdt.feed();
        s.now_ms = now_millis();
    }

    /// `bt_a2dp`: lazy BT controller bring-up after the DHCP lease, then A2DP sink poll. A2DP
    /// audio feeds the pipeline only while LMS does not own the output; the coordinator is told
    /// (`begin_external_playback`) so it reports nothing about it to LMS.
    #[inline(never)]
    fn bt_poll(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Lazy initialization of Bluetooth after Wi-Fi association and DHCPv4 lease
        if s.assigned_ip.is_some() && s.bt_transport.is_none() {
            if let Some(bt_p) = s.bt_periph.take() {
                s.wdt.feed();
                s.bt_transport = match esp_radio::ble::controller::BleConnector::new(
                    bt_p,
                    Default::default(),
                ) {
                    Ok(c) => Some(Esp32VhciTransport { connector: c }),
                    Err(_) => None,
                };
                s.wdt.feed();
            }
        }

        // An LMS track took over the output: A2DP has to start over when LMS is done.
        if s.bt_audio_active && lms_audio_active(s.coordinator) {
            s.bt_audio_active = false;
        }

        // Poll Bluetooth Classic A2DP Sink
        if let Some(ref mut transport) = s.bt_transport {
            s.bt_sink.poll(transport, now_ms, |pcm_bytes, sample_rate_hz| {
                if !lms_audio_active(s.coordinator) {
                    if s.coordinator.current_sample_rate != sample_rate_hz
                        || s.pipeline.sample_rate != sample_rate_hz
                    {
                        s.coordinator.current_sample_rate = sample_rate_hz;
                        let _ = s.pipeline.set_sample_rate(sample_rate_hz);
                    }
                    if !s.bt_audio_active {
                        // A replaced MP3 decoder is parked by `set_decoder` for the next track.
                        s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)));
                        // Declared divergences (cleaner): no LMS fade and no LMS replay gain on
                        // Bluetooth audio (C's `_buf_flush` leaves an armed fade running and
                        // `current_replay_gain` set, decode_external.c:147-154).
                        s.pipeline.begin_external_audio();
                        s.pipeline.set_mute(false);
                        s.coordinator.begin_external_playback();
                        if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                            let _ = s.amp.start_playback_via_hiz();
                        }
                        s.bt_audio_active = true;
                    }
                    let _ = s.pipeline.ingest_stream_data(pcm_bytes);
                }
            });

            if s.bt_audio_active && !s.bt_sink.is_streaming() && !lms_audio_active(s.coordinator) {
                if s.pipeline.stream_buffer.available_read() == 0
                    && s.pipeline.output_buffer.available_read() == 0
                    && !s.pipeline.has_pending_pcm()
                {
                    s.coordinator.end_external_playback();
                    s.bt_audio_active = false;
                }
            }
        }
    }

    /// `net_stack`: drive the Wi-Fi STA association (one attempt per 5 s at most).
    #[inline(never)]
    fn wifi_connect(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Drive Wi-Fi STA connection if not currently associated
        if !s.wifi_controller.is_connected()
            && (s.last_connect_attempt_ms == 0
                || now_ms.saturating_sub(s.last_connect_attempt_ms) >= 5_000)
        {
            s.last_connect_attempt_ms = now_ms.max(1);
            let mut connect_fut = pin!(s.wifi_controller.connect_async());
            let waker = Waker::noop();
            let mut cx = Context::from_waker(waker);
            let _ = connect_fut.as_mut().poll(&mut cx);
        }
    }

    /// `watchdog_lease`: Gate 3 dead-man network lease (45 s boot / 60 s drop -> recovery).
    #[inline(never)]
    fn lease_check(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Gate 3: Evaluate Dead-Man Network Lease Watchdog (45s boot / 60s drop)
        if s.lease_watchdog.evaluate(now_ms, s.wifi_controller.is_connected(), s.assigned_ip.is_some())
            == LeaseHealth::ExpiredResetToRecovery
        {
            println!("[ANTI-BRICK GATE 3] Network lease timeout expired -> resetting to recovery (0x10000)");
            esp_hal::system::software_reset();
        }
    }

    /// `net_stack`: poll the smoltcp interface and apply DHCPv4 events.
    #[inline(never)]
    fn net_poll(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Poll `smoltcp` network stack
        {
            let mut dev = SmolDevice {
                driver: &mut s.sta_iface,
            };
            let _ = s.smol_iface.poll(
                SmolInstant::from_millis(now_ms as i64),
                &mut dev,
                &mut s.sockets,
            );
        }

        // Process DHCPv4 events
        let dhcp_event = s.sockets.get_mut::<dhcpv4::Socket>(s.dhcp_handle).poll();
        match dhcp_event {
            Some(dhcpv4::Event::Configured(config)) => {
                let ip_octets = config.address.address().octets();
                s.assigned_ip = Some(ip_octets);
                let prefix_len = config.address.prefix_len();
                let mask_u32 = if prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - prefix_len)
                };
                s.assigned_mask = mask_u32.to_be_bytes();
                if let Some(router) = config.router {
                    s.assigned_gw = router.octets();
                    let _ = s.smol_iface.routes_mut().add_default_ipv4_route(router);
                }
                s.smol_iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(config.address));
                });
                println!(
                    "[DHCPv4] Acquired IP {}.{}.{}.{}/{}",
                    ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3], prefix_len
                );
            }
            Some(dhcpv4::Event::Deconfigured) => {
                // Every connection is gone with the address; the coordinator sees the audio
                // stream fail on its next poll.
                s.assigned_ip = None;
                s.smol_iface.update_ip_addrs(|addrs| addrs.clear());
                s.sockets.get_mut::<tcp::Socket>(s.slim_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.http_handle).abort();
                s.helo_sent = false;
                s.pending_http_body = &[];
                s.pending_http_closing = false;
            }
            None => {}
        }
    }
    /// `http_control`: Gate 4 port-80 control/recovery server, then its deferred reboot action.
    #[inline(never)]
    fn http_serve(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Gate 4: Port 80 HTTP Control & Recovery Server (`/status.json`, `/recovery.json`, `/reboot.json`, `/reboot_ota.json`)
        {
            let http_sock = s.sockets.get_mut::<tcp::Socket>(s.http_handle);

            // Multi-chunk streaming of static body (.rodata)
            if !s.pending_http_body.is_empty() {
                if now_ms.saturating_sub(s.pending_http_started_ms) >= 2_500
                    || matches!(
                        http_sock.state(),
                        tcp::State::CloseWait
                            | tcp::State::TimeWait
                            | tcp::State::Closing
                            | tcp::State::LastAck
                            | tcp::State::FinWait1
                            | tcp::State::FinWait2
                            | tcp::State::Closed
                    )
                {
                    s.pending_http_body = &[];
                    http_sock.abort();
                    s.pending_http_closing = false;
                } else if http_sock.may_send() {
                    if let Ok(sent) = http_sock.send_slice(s.pending_http_body) {
                        s.pending_http_body = &s.pending_http_body[sent..];
                    }
                    if s.pending_http_body.is_empty() {
                        http_sock.close();
                        s.pending_http_closing = true;
                        s.pending_close_deadline_ms = now_ms + 500;
                    }
                } else {
                    s.pending_http_body = &[];
                    http_sock.abort();
                    s.pending_http_closing = false;
                }
            }

            // Immediate recycling of closing sockets once FIN/ACK handshake completes (TimeWait) or on timeout
            if s.pending_http_closing {
                if matches!(http_sock.state(), tcp::State::TimeWait | tcp::State::Closed)
                    || now_ms >= s.pending_close_deadline_ms
                {
                    http_sock.abort();
                    s.pending_http_closing = false;
                }
            }

            if s.pending_http_body.is_empty()
                && !s.pending_http_closing
                && http_sock.is_open()
                && matches!(
                    http_sock.state(),
                    tcp::State::CloseWait
                        | tcp::State::TimeWait
                        | tcp::State::Closing
                        | tcp::State::LastAck
                        | tcp::State::FinWait1
                        | tcp::State::FinWait2
                )
            {
                http_sock.abort();
            }

            if !http_sock.is_open() {
                s.pending_http_body = &[];
                s.pending_http_closing = false;
                let _ = http_sock.listen(80);
            }

            if s.pending_http_body.is_empty()
                && !s.pending_http_closing
                && http_sock.can_recv()
                && http_sock.may_send()
            {
                let mut req_buf = [0u8; 256];
                let n = http_sock.recv_slice(&mut req_buf).unwrap_or(0);
                while http_sock.can_recv() {
                    let _ = http_sock.recv(|buf| (buf.len(), ()));
                }
                if n > 0 {
                    let amp_mode_str = match s.amp.mode() {
                        louder_esp32_app::AmpMode::Play => "Play",
                        louder_esp32_app::AmpMode::Standby => "Standby",
                        louder_esp32_app::AmpMode::PowerDown => "PowerDown",
                    };
                    let codec_name = match s.pipeline.decoder {
                        ActiveDecoder::Mp3(_) => "MP3",
                        ActiveDecoder::Flac(_) => "FLAC",
                        ActiveDecoder::Pcm(_) => "PCM",
                        #[cfg(not(target_arch = "xtensa"))]
                        ActiveDecoder::Aac(_) => "AAC",
                        #[cfg(not(target_arch = "xtensa"))]
                        ActiveDecoder::Vorbis(_) => "Vorbis",
                    };
                    let (tas_fs_hz, tas_bck_ratio, tas_pll_locked, tas_automuted) = match s.latest_telemetry {
                        Some(t) => (
                            t.detected_sample_rate_hz_with_hint(s.coordinator.current_sample_rate),
                            t.bck_ratio(),
                            t.is_pll_locked(),
                            t.is_automuted(),
                        ),
                        None => (s.coordinator.current_sample_rate, 64, true, false),
                    };
                    let (psram_free_bytes, psram_used_bytes) = {
                        let stats = esp_alloc::HEAP.stats();
                        let mut free = louder_esp32_app::http_control::DEFAULT_PSRAM_FREE_BYTES;
                        let mut used = louder_esp32_app::http_control::DEFAULT_PSRAM_USED_BYTES;
                        for r in stats.region_stats.iter().flatten() {
                            if r.size > 1024 * 1024 {
                                free = r.free as u32;
                                used = r.used as u32;
                                break;
                            }
                        }
                        (free, used)
                    };
                    let snap = HttpStatusSnapshot {
                        ip: s.assigned_ip.unwrap_or([172, 22, 1, 15]),
                        netmask: s.assigned_mask,
                        gw: s.assigned_gw,
                        ssid: s.wifi_sta_creds.ssid.as_str(),
                        rssi: s.wifi_controller.rssi().unwrap_or(-50),
                        lms_ip: s.lms_ip,
                        lms_port: 9000,
                        lms_cport: 9090,
                        otadata_prearmed_factory: true,
                        frames_played: s.pipeline.frames_played,
                        amp_mode: amp_mode_str,
                        vol_pct: s.current_vol_pct,
                        i2c_acks: Esp32I2cBus::hw_ack_count(),
                        i2c_nacks: Esp32I2cBus::hw_nack_count(),
                        i2c_nack_by_op: Esp32I2cBus::hw_nack_by_op(),
                        uptime_ms: now_ms.saturating_sub(s.boot_ms),
                        psram_free_bytes,
                        psram_used_bytes,
                        core1_active: s.core1.is_active(),
                        core1_loops: CORE1_HEARTBEAT.load(core::sync::atomic::Ordering::Relaxed),
                        codec: codec_name,
                        codec_sample_rate: s.coordinator.current_sample_rate,
                        codec_channels: 2,
                        codec_bit_depth: 16,
                        eq_preset: s.pipeline.current_eq_preset.as_str(),
                        eq_enabled: s.pipeline.equalizer.enabled,
                        tas_fs_hz,
                        tas_bck_ratio,
                        tas_pll_locked,
                        tas_automuted,
                        again_code: s.amp.analog_gain_code(),
                        settings_seq: s.journal.sequence(),
                        bt_enabled: s.bt_transport.is_some() && s.bt_sink.is_initialized(),
                        bt_connected: s.bt_sink.is_connected(),
                        bt_streaming: s.bt_sink.is_streaming(),
                        bt_rx_pkts: s.bt_sink.rx_packets,
                        bt_frames: s.bt_sink.decoded_frames,
                    };
                    if let Some((hdr_or_resp_len, static_body, action)) =
                        handle_http_request_streaming(&req_buf[..n], &snap, &mut s.http_resp_buf[..])
                    {
                        let _ = http_sock.send_slice(&s.http_resp_buf[..hdr_or_resp_len]);
                        if !static_body.is_empty() {
                            s.pending_http_body = static_body;
                            s.pending_http_started_ms = now_ms;
                            if let Ok(sent) = http_sock.send_slice(s.pending_http_body) {
                                s.pending_http_body = &s.pending_http_body[sent..];
                            }
                            if s.pending_http_body.is_empty() {
                                http_sock.close();
                                s.pending_http_closing = true;
                                s.pending_close_deadline_ms = now_ms + 500;
                            }
                        } else {
                            http_sock.close();
                            s.pending_http_closing = true;
                            s.pending_close_deadline_ms = now_ms + 500;
                        }
                        match action {
                            HttpControlAction::SetEqPreset(preset_id) => {
                                let preset = EqPreset::from_u8(preset_id);
                                s.pipeline.apply_eq_preset(preset);
                                s.config.update(|c| c.equalizer_preset = preset_id);
                            }
                            HttpControlAction::SetAnalogGain(again_code) => {
                                let _ = s.amp.set_analog_gain(again_code);
                                s.config.update(|c| c.analog_gain_code = again_code);
                            }
                            HttpControlAction::PlayerIrCommand { ir_code, vol_pct, mute } => {
                                if let Some(pct) = vol_pct {
                                    s.current_vol_pct = pct;
                                    // The single volume entry: the TAS register for `pct`
                                    // (as percent_to_attenuation), LMS's balance kept.
                                    let mut backend =
                                        Esp32AudioBackend::new(s.amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
                                    s.coordinator.set_volume_percent(pct, &mut backend, s.pipeline);
                                    let _ = s.cyd_bridge.send_message(&CydMessage::Volume(pct));
                                    s.config.update(|c| c.volume = pct);
                                }
                                if let Some(m) = mute {
                                    // User mute: survives the next output write.
                                    s.pipeline.set_mute(m);
                                }
                                if ir_code != 0 && s.helo_sent {
                                    let mut ir_buf = [0u8; 32];
                                    if let Ok(ir_len) =
                                        SlimprotoBuilder::build_ir(now_ms as u32, ir_code, &mut ir_buf)
                                    {
                                        let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                        let mut link = SmoltcpSlimTransport::new(
                                            SmolTcp::new(slim_sock),
                                            &mut s.slim_link,
                                        );
                                        let _ = link.send(&ir_buf[..ir_len]);
                                    }
                                }
                            }
                            HttpControlAction::RebootToRecovery | HttpControlAction::PersistOta0AndReboot => {
                                s.pending_http_action = action;
                                s.pending_action_deadline_ms = now_ms + 250;
                            }
                            HttpControlAction::None => {}
                        }
                    } else {
                        http_sock.close();
                        s.pending_http_closing = true;
                        s.pending_close_deadline_ms = now_ms + 250;
                    }
                }
            }
        }

        // Execute deferred HTTP reboot action after flushing TCP response
        if s.pending_http_action != HttpControlAction::None && now_ms >= s.pending_action_deadline_ms {
            flush_settings_before_reboot(s);
            match s.pending_http_action {
                HttpControlAction::RebootToRecovery => {
                    println!("[ANTI-BRICK GATE 4] HTTP /recovery.json or /reboot.json -> resetting into factory recovery (0x10000)");
                    esp_hal::system::software_reset();
                }
                HttpControlAction::PersistOta0AndReboot => {
                    let entry_bytes = OtaSelectEntry::new_ota_0().to_bytes();
                    let _ = s.flash.write(OTADATA_PARTITION_OFFSET, &entry_bytes);
                    println!("[ANTI-BRICK GATE 4] HTTP /reboot_ota.json -> wrote ota_0 select entry & resetting");
                    esp_hal::system::software_reset();
                }
                _ => {}
            }
        }
    }

    /// `slimproto_session`: control-link upkeep, then [`slim_tick`] once the IPv4 lease is
    /// active. Upkeep: a `serv` switch drops the link and targets the new server; a half-closed
    /// link is reset; a link that went down is reported to [`SlimReconnect`] (backoff, or the
    /// fall-back to the previous server); a connect starts when the policy allows it.
    #[inline(never)]
    fn slim_session(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        let Some(local_ip) = s.assigned_ip else {
            return;
        };
        let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
        // C parity: `serv` ends the session (`slimproto_run` exits on `new_server`,
        // slimproto.c:708), the post-session backoff delay runs (slimproto.c:1187-1210), and
        // only then the new server is connected (slimproto.c:1096-1103): `SlimReconnect`.
        if let Some(ip) = s.slim_switch_to.take() {
            slim_sock.abort();
            s.slim_reconnect.server_switch(now_ms, s.lms_ip, backoff_seed(now_ms));
            s.slim_was_open = false;
            s.helo_sent = false;
            s.lms_ip = ip;
            s.coordinator.lms_ip = ip;
        }
        // Half-closed / closing control link: abort it so it reconnects instead of lingering
        // in CloseWait.
        if slim_sock.is_open()
            && matches!(
                slim_sock.state(),
                tcp::State::CloseWait
                    | tcp::State::TimeWait
                    | tcp::State::Closing
                    | tcp::State::LastAck
                    | tcp::State::FinWait1
                    | tcp::State::FinWait2
            )
        {
            slim_sock.abort();
        }
        let open = slim_sock.is_open();
        if s.slim_was_open && !open {
            s.helo_sent = false;
            if let Some(previous) = s.slim_reconnect.link_down(now_ms, backoff_seed(now_ms)) {
                // The `serv` target was unreachable: back to the previous server.
                s.lms_ip = previous;
                s.coordinator.lms_ip = previous;
            }
        }
        s.slim_was_open = open;
        if !open && s.slim_reconnect.may_connect(now_ms) {
            s.helo_sent = false;
            s.slim_link.reset();
            let remote_ep = (IpAddress::Ipv4(Ipv4Address::from(s.lms_ip)), s.lms_port);
            let local_ep = (
                IpAddress::Ipv4(Ipv4Address::from(local_ip)),
                next_local_port(&mut s.local_port_counter),
            );
            if slim_sock.connect(s.smol_iface.context(), remote_ep, local_ep).is_ok() {
                s.slim_reconnect.connect_started();
                s.slim_was_open = true;
            }
        }
        slim_tick(s, local_ip);
    }

    /// Jitter source for the reconnect backoff (C uses `rand()`, `slimproto.c:1196`): the
    /// scrambled lap clock.
    fn backoff_seed(now_ms: u64) -> u32 {
        (now_ms as u32).wrapping_mul(0x9E37_79B9) ^ (now_ms >> 32) as u32
    }

    /// `HELO` + `SETD` once per control connection (`PlayerCoordinator::hello`, with the
    /// [`SlimReconnect`] reconnect flag), then one
    /// `PlayerCoordinator::tick` over the device adapters: at most one server frame, the HTTP
    /// audio fetch, play-state evaluation, deferred `STAT`/`RESP`/`META`/`DSCO` and the
    /// heartbeat. It never decodes or writes to the sink (`external_audio_pump`, set at
    /// bring-up): the pump phases own that, DMA first. The tick also runs while the control
    /// link is down, so the stream keeps being fetched. A server volume change (`audg`) is mirrored to the CYD and the [`ConfigHandle`] (debounced into the journal).
    #[inline(never)]
    fn slim_tick(s: &mut FirmwareState, local_ip: [u8; 4]) {
        let now_ms = s.now_ms as u32;
        let FirmwareState {
            ref mut sockets,
            ref mut smol_iface,
            slim_handle,
            audio_handle,
            ref mut coordinator,
            ref mut pipeline,
            ref mut amp,
            ref mut cyd_bridge,
            ref mut slim_link,
            ref mut stream_state,
            ref mut local_port_counter,
            ref mut helo_sent,
            ref mut current_vol_pct,
            ref mut slim_reconnect,
            ref mut slim_switch_to,
            config,
            now_ms: lap_ms,
            ..
        } = *s;
        let Some((slim_sock, audio_sock)) =
            slim_and_audio_sockets(sockets, slim_handle, audio_handle)
        else {
            return;
        };

        let mut transport = SmoltcpSlimTransport::new(SmolTcp::new(slim_sock), &mut *slim_link);
        if transport.connected() && !*helo_sent {
            *helo_sent = coordinator
                .hello(&mut transport, HELO_DEVICE_ID, slim_reconnect.reconnect_flag())
                .is_ok();
            if !*helo_sent {
                // Never let a `STAT` reach LMS before the `HELO`: retry next lap.
                return;
            }
            slim_reconnect.session_up(lap_ms);
        }
        let mut source = SmoltcpStreamSource::new(
            SmolTcp {
                sock: audio_sock,
                connect: Some((smol_iface.context(), local_ip, local_port_counter)),
            },
            &mut *stream_state,
        );
        let mut backend = Esp32AudioBackend::new(amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
        // A dead control link is handled by `slim_session` (reconnect + HELO); the tick's own
        // errors need no action here.
        let _ = coordinator.tick(&mut transport, &mut source, &mut backend, pipeline, now_ms);
        if let Some(ip) = coordinator.take_server_switch() {
            *slim_switch_to = Some(ip);
        }

        while let Some(msg) = coordinator.take_cyd_message() {
            if let CydMessage::Volume(pct) = msg {
                *current_vol_pct = pct;
                config.update(|c| c.volume = pct);
            }
            let _ = cyd_bridge.send_message(&msg);
        }
    }

    /// `cyd_bridge`: apply one pending CYD command.
    #[inline(never)]
    fn cyd_command(s: &mut FirmwareState) {
        if let Ok(Some(cmd)) = s.cyd_bridge.poll_command() {
            // CYD SetVolume goes through the single volume entry (device policy, balance kept).
            let mut backend = Esp32AudioBackend::new(s.amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
            if let Some(reply) = s.coordinator.handle_cyd_command(cmd, &mut backend, s.pipeline) {
                if let CydMessage::Volume(pct) = reply {
                    // A CYD volume change marks the config dirty (spec §3.5); debounced.
                    s.config.update(|c| c.volume = pct.min(100));
                }
                let _ = s.cyd_bridge.flush_outbound(&reply);
            }
        }
    }

    /// `i2s_feed`: drain decoded PCM and top up the I2S0 DMA (up to 16 sink writes) BEFORE
    /// spending ~4.5 ms decoding a new MP3 frame.
    #[inline(never)]
    fn output_top_up_first(s: &mut FirmwareState) {
        s.coordinator.pump_output(s.pipeline, 16);
    }

    /// `decode_pump`: with the DMA topped up, decode at most one audio frame (or a resync /
    /// tag-skip burst of up to 16 steps for MP3 and FLAC, 4 otherwise).
    #[inline(never)]
    fn decode_burst(s: &mut FirmwareState) {
        let max_steps = match s.pipeline.decoder {
            ActiveDecoder::Mp3(_) | ActiveDecoder::Flac(_) => 16,
            _ => 4,
        };
        s.coordinator.pump_decode(s.pipeline, max_steps);
    }

    /// `i2s_feed`: top up the DMA with the new frames, sync the coordinator rate, keep the amp
    /// stopped while the DAC is disabled, then pump silence when there is nothing to play.
    #[inline(never)]
    fn output_top_up_final(s: &mut FirmwareState) {
        s.coordinator.pump_output(s.pipeline, 16);
        s.coordinator.current_sample_rate = s.pipeline.sample_rate;

        if !s.coordinator.dac_enabled {
            let _ = s.amp.stop_playback();
        }

        let has_buffered_audio =
            s.pipeline.output_buffer.available_read() >= 8 || s.pipeline.has_pending_pcm();
        s.pipeline.sink.driver.pump_silence_if_needed(
            s.coordinator.state == PlayerState::Playing,
            has_buffered_audio,
        );
    }

    /// `watchdog_lease`: TAS58xx fault recovery + telemetry refresh every 2 s.
    #[inline(never)]
    fn amp_health(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        if now_ms.saturating_sub(s.last_fault_check_ms) >= 2_000 {
            s.last_fault_check_ms = now_ms;
            if s.coordinator.state == louder_esp32_app::PlayerState::Playing {
                if let Ok(Some(fault)) = s.amp.check_faults() {
                    if !fault.is_clean() {
                        let _ = s.amp.recover_from_fault();
                    }
                }
            }
            if let Ok(telem) = s.amp.read_telemetry() {
                s.latest_telemetry = telem;
            }
        }
    }

    /// `cyd_bridge`: once-per-second elapsed-time update to the CYD display.
    #[inline(never)]
    fn cyd_progress(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        if now_ms.saturating_sub(s.last_cyd_progress_ms) >= 1_000 {
            s.last_cyd_progress_ms = now_ms;
            if s.coordinator.state == louder_esp32_app::PlayerState::Playing && s.coordinator.current_sample_rate > 0 {
                let elapsed_secs = (s.pipeline.frames_played / s.coordinator.current_sample_rate as u64) as u32;
                let _ = s.cyd_bridge.send_message(&CydMessage::Progress {
                    elapsed_secs,
                    duration_secs: 0,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_discriminants_follow_phase_order() {
        for (i, phase) in PHASE_ORDER.iter().enumerate() {
            assert_eq!(*phase as usize, i, "{phase:?} is out of place in PHASE_ORDER");
        }
    }

    #[test]
    fn test_phase_ring_keeps_dma_first_audio_order() {
        // Owner sequence of one lap. The audio steps keep the pre-Task-9 superloop order: the
        // I2S DMA is topped up before and after the decode burst, after the slimproto session
        // (whose coordinator tick also fetches the HTTP audio).
        let owners: Vec<RingTask> = PHASE_ORDER.iter().map(|p| p.owner()).collect();
        assert_eq!(
            owners,
            vec![
                RingTask::WatchdogLease,    // wdt.feed + now_ms
                RingTask::BtA2dp,           // BT lazy init + poll
                RingTask::NetStack,         // Wi-Fi connect drive
                RingTask::WatchdogLease,    // Gate 3 lease
                RingTask::NetStack,         // smoltcp poll + DHCP
                RingTask::HttpControl,      // Gate 4 server + deferred action
                RingTask::SlimprotoSession, // HELO + PlayerCoordinator::tick
                RingTask::CydBridge,        // CYD command
                RingTask::I2sFeed,          // pump_output before decode
                RingTask::DecodePump,       // pump_decode
                RingTask::I2sFeed,          // pump_output + DAC gate, silence pump
                RingTask::WatchdogLease,    // amp fault + telemetry
                RingTask::CydBridge,        // CYD progress
            ]
        );
    }

    #[test]
    fn test_phase_ring_visits_every_phase_once_per_lap_and_wraps() {
        let mut phase = PHASE_ORDER[0];
        let mut seen = Vec::new();
        for _ in 0..PHASE_ORDER.len() {
            seen.push(phase);
            phase = phase.next();
        }
        assert_eq!(seen, PHASE_ORDER.to_vec());
        assert_eq!(phase, PHASE_ORDER[0], "the lap must wrap back to the WDT feed");
        assert_eq!(PHASE_ORDER[0], Phase::WatchdogFeed, "every lap starts by feeding the WDT");
    }

    #[test]
    fn test_every_ring_task_owns_a_phase_and_indexes_are_dense() {
        const ALL: [RingTask; RingTask::COUNT] = [
            RingTask::WatchdogLease,
            RingTask::BtA2dp,
            RingTask::NetStack,
            RingTask::HttpControl,
            RingTask::SlimprotoSession,
            RingTask::CydBridge,
            RingTask::DecodePump,
            RingTask::I2sFeed,
        ];
        for (i, task) in ALL.iter().enumerate() {
            assert_eq!(task.index(), i);
            assert!(
                PHASE_ORDER.iter().any(|p| p.owner() == *task),
                "{task:?} would never receive the baton"
            );
        }
    }

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
            CoordinatorCmd::SetGain { l: 65_536, r: 32_768 },
            CoordinatorCmd::SetMute(true),
            CoordinatorCmd::SetMute(false),
        ];
        for cmd in cmds {
            CTRL.try_send(cmd).expect("CTRL must hold CHANNEL_DEPTH commands");
        }
        assert!(CTRL.try_send(CoordinatorCmd::Pause).is_err(), "CTRL depth is {CHANNEL_DEPTH}");
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
}
