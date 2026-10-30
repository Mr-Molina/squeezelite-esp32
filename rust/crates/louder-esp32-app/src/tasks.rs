//! Embassy task skeleton for the device firmware (Task 8, spec §3.1).
//!
//! # What this module is
//!
//! The pre-embassy firmware ran ONE cooperative superloop on core 0. Task 8 re-wraps that loop,
//! without changing behavior, into the embassy task layout of spec §3.1:
//!
//! | task                | superloop blocks it now owns                                              |
//! |---------------------|---------------------------------------------------------------------------|
//! | `watchdog_lease`    | WDT feed + loop timestamp, Gate 3 network lease, 2 s amp fault/telemetry  |
//! | `bt_a2dp`           | lazy BT controller bring-up, A2DP sink poll                               |
//! | `net_stack`         | Wi-Fi STA connect drive, smoltcp poll + DHCPv4 events                     |
//! | `http_control`      | Gate 4 port-80 server + deferred reboot actions                           |
//! | `slimproto_session` | LMS control socket (HELO/STAT/frames), `STMs`, `STMd`/`STMu` emission     |
//! | `audio_fetch`       | HTTP audio socket (request, headers/RESP, ICY-aware ingest, EOF)          |
//! | `cyd_bridge`        | CYD command poll, 1 s progress update                                     |
//! | `decode_pump`       | decode burst of the legacy firmware tick, post-EOF tail decode            |
//! | `i2s_feed`          | DMA-first top-ups of the legacy firmware tick, DAC gate, silence pump     |
//! | `config_writer`     | stub (Task 10 adds debounced persistence; persistence stays inline today) |
//!
//! The protocol/stream logic is still the legacy inline code, moved verbatim into one `fn` per
//! block (locals became fields of [`FirmwareState`]); Task 9 replaces it with
//! `PlayerCoordinator::tick` over the device trait impls.
//!
//! # Why a phase ring (behavior preservation)
//!
//! The superloop ran its blocks in a fixed order with one shared timestamp per iteration, and
//! several blocks depend on that order (e.g. the I2S DMA is topped up BEFORE a ~4.5 ms MP3 frame
//! decode, then again after it; `STMs`/`STMd`/`STMu` are evaluated after the audio step of the
//! same iteration). Free-running embassy tasks would interleave in run-queue order instead. So
//! the ring tasks hand a baton around in exactly the legacy order ([`PHASE_ORDER`]): each task
//! waits on its own signal, runs the block named by the phase it received, then signals the
//! owner of the next phase. One full lap == one legacy loop iteration. Exactly one task holds
//! the baton at any time, so the shared state is only ever borrowed by one block at a time and
//! never across an `.await`. Task 9+ replace the baton with the `CTRL`/`EVT` channels below and
//! real wakeups as the inline code is deleted.
//!
//! # Core placement
//!
//! Spec §3.1 places `decode_pump` and `i2s_feed` on core 1. Core-1 start under esp-rtos is NOT
//! yet validated on hardware (Spike S1 owner gate pending), and today the audio path runs on
//! core 0 with core 1 executing only a NOP heartbeat. Therefore, in BOTH build configurations,
//! the audio tasks run on the core-0 executor (they are ring members, which also keeps the
//! DMA-first ordering above). Core 1 is started by:
//! - default: the battle-tested manual path (park APP_CPU, copy the PRO flash-MMU/cache
//!   registers, `CpuControl::start_app_core` running the IRAM NOP heartbeat loop);
//! - feature `esp-rtos-core1`: `esp_rtos::start_second_core` running an embassy executor whose
//!   only task is `core1_heartbeat`. Moving `decode_pump`/`i2s_feed` there (and replacing the
//!   ring hand-off between them and core 0 with `CTRL`/`EVT` + the SPSC rings) is the follow-up
//!   once the S1 owner gate passes.
//!
//! # Memory
//!
//! [`FirmwareState`] lives in the frame of `run_hardware_firmware`, which never returns (the
//! core-0 main stack, where the superloop locals lived), not in `.bss`. The new statics are the
//! task arenas, the executor cell(s), the ring signals and the UDP placeholder buffers; every
//! byte of them comes out of the core-0 `.stack` section (it is the remainder of `dram_seg`
//! after `.data`/`.bss`), which is why they are kept to the minimum (sizes in the Task 8 report).
#![cfg_attr(not(target_arch = "xtensa"), allow(dead_code))]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use squeezelite_proto::AudioFormat;

/// Stream parameters handed from the coordinator to the audio tasks on `strm s` (Task 9 wires it).
#[allow(dead_code)] // Task 9 wires the channels
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
#[allow(dead_code)] // Task 9 wires the channels
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
#[allow(dead_code)] // Task 9 wires the channels
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    FirstFrameWritten,
    TrackBoundaryCrossed,
    OutputDrained,
    Underrun,
    DecoderError(u8),
}

/// Depth of [`CTRL`] and [`EVT`].
#[allow(dead_code)] // Task 9 wires the channels
pub const CHANNEL_DEPTH: usize = 8;

/// Coordinator -> audio command channel. Defined now, wired by Task 9 (until then nothing sends
/// on it, so the linker drops it from the device image).
#[allow(dead_code)] // Task 9 wires the channels
pub static CTRL: Channel<CriticalSectionRawMutex, CoordinatorCmd, CHANNEL_DEPTH> = Channel::new();

/// Audio -> coordinator event channel. Defined now, wired by Task 9.
#[allow(dead_code)] // Task 9 wires the channels
pub static EVT: Channel<CriticalSectionRawMutex, AudioEvent, CHANNEL_DEPTH> = Channel::new();

/// Members of the core-0 phase ring (every spec §3.1 task except the `config_writer` stub).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingTask {
    WatchdogLease,
    BtA2dp,
    NetStack,
    HttpControl,
    SlimprotoSession,
    AudioFetch,
    CydBridge,
    DecodePump,
    I2sFeed,
}

impl RingTask {
    pub const COUNT: usize = 9;

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One block of the legacy superloop. Discriminants follow [`PHASE_ORDER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `wdt.feed()` + the iteration timestamp shared by every later phase of the lap.
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
    /// LMS control socket: connect, RESP flush, HELO/STAT, frame receive + dispatch.
    SlimControl,
    /// HTTP audio socket: request, headers/RESP, body ingest, EOF.
    AudioStream,
    /// Legacy firmware tick step 0: CYD command poll.
    CydCommand,
    /// Legacy firmware tick step 1: drain pending PCM + top up the I2S DMA before decoding.
    OutputTopUp,
    /// Legacy firmware tick step 2: decode at most one frame (or a short resync burst).
    DecodeBurst,
    /// Legacy firmware tick step 3 + tail: top up again, DAC gate, then the silence pump.
    OutputTopUpAndSilence,
    /// `STMs` once the first frame has been played.
    StatTrackStarted,
    /// Post-EOF stream-buffer tail decode.
    DecodeEofDrain,
    /// `STMd` then `STMu`.
    StatDecodeReadyUnderrun,
    /// 2 s TAS58xx fault recovery + telemetry.
    AmpHealth,
    /// 1 s CYD progress update.
    CydProgress,
}

/// The legacy superloop order, one entry per block. One lap of the ring == one legacy iteration.
pub const PHASE_ORDER: [Phase; 17] = [
    Phase::WatchdogFeed,
    Phase::Bluetooth,
    Phase::WifiConnect,
    Phase::LeaseCheck,
    Phase::NetPoll,
    Phase::HttpServe,
    Phase::SlimControl,
    Phase::AudioStream,
    Phase::CydCommand,
    Phase::OutputTopUp,
    Phase::DecodeBurst,
    Phase::OutputTopUpAndSilence,
    Phase::StatTrackStarted,
    Phase::DecodeEofDrain,
    Phase::StatDecodeReadyUnderrun,
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
            Phase::SlimControl | Phase::StatTrackStarted | Phase::StatDecodeReadyUnderrun => {
                RingTask::SlimprotoSession
            }
            Phase::AudioStream => RingTask::AudioFetch,
            Phase::CydCommand | Phase::CydProgress => RingTask::CydBridge,
            Phase::DecodeBurst | Phase::DecodeEofDrain => RingTask::DecodePump,
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

/// Device glue: the shared superloop state, the ring tasks and the legacy blocks they run.
#[cfg(target_arch = "xtensa")]
mod device {
    extern crate alloc;

    use super::{Phase, RingTask, PHASE_ORDER};
    use crate::xtensa_hw::{
        now_millis, persist_audio_config, take_static_mp3_decoder, Core1Ctl, Esp32I2cBus,
        Esp32Uart2Port, Esp32VhciTransport, HwStreamPipeline, SmolDevice, CORE1_HEARTBEAT,
        MP3_DEC_TAKEN,
    };
    use crate::{lms_gain_to_tas5805m_vol_reg, parse_icy_metaint_header, BASEMENT_HELO_CAPABILITIES};
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
    use louder_esp32_app::amp::percent_to_attenuation;
    use louder_esp32_app::board::{BASEMENT_PLAYER_MAC, DEFAULT_LMS_SERVER_IP, DEFAULT_LMS_SERVER_PORT};
    use louder_esp32_app::bt_a2dp::BtA2dpSink;
    use louder_esp32_app::http_control::{
        handle_http_request_streaming, DeadManLeaseWatchdog, HttpControlAction, HttpStatusSnapshot,
        LeaseHealth,
    };
    use louder_esp32_app::ota_safety::{OtaSelectEntry, WifiStaCredentials, OTADATA_PARTITION_OFFSET, SETTINGS_JOURNAL_OFFSET};
    use louder_esp32_app::pipeline::ActiveDecoder;
    use louder_esp32_app::{AmpManager, AudioOutputSink, CydUartBridge, PlayerCoordinator};
    use smoltcp::iface::{Interface as SmolInterface, SocketHandle, SocketSet};
    use smoltcp::socket::{dhcpv4, tcp};
    use smoltcp::time::Instant as SmolInstant;
    use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};
    use squeezelite_audio_core::EqPreset;
    use squeezelite_config::AudioConfig;
    use squeezelite_cyd_link::CydMessage;
    use squeezelite_decoders::PcmDecoder;
    use squeezelite_proto::{
        sample_rate_from_index, AutostartMode, DisconnectReason, Endianness, ParseError,
        ServerCommand, SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent,
        StreamCommandType,
    };
    use static_cell::StaticCell;

    /// Every local of the legacy superloop, plus the hardware it drove. Field names are the
    /// legacy local names so the moved blocks read the same as before (`x` became `s.x`).
    pub(crate) struct FirmwareState {
        // --- hardware kept alive for the life of the firmware ---
        pub(crate) wdt: Wdt<esp_hal::peripherals::TIMG0<'static>>,
        pub(crate) flash: FlashStorage<'static>,
        pub(crate) core1: Core1Ctl,
        pub(crate) _pwdn: Output<'static>,
        pub(crate) _fault: Input<'static>,
        pub(crate) wifi_controller: WifiController<'static>,
        pub(crate) sta_iface: WifiInterface,
        pub(crate) smol_iface: SmolInterface,
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
        pub(crate) amp: AmpManager<Esp32I2cBus<'static>>,
        pub(crate) cyd_bridge: CydUartBridge<Esp32Uart2Port<'static>>,
        pub(crate) pipeline: &'static mut HwStreamPipeline,
        pub(crate) coordinator: PlayerCoordinator,
        // --- static buffers ---
        pub(crate) settings_scratch: &'static mut [u8; 4096],
        pub(crate) pending_resp_buf: &'static mut [u8; 2064],
        pub(crate) http_resp_buf: &'static mut [u8; 1536],
        pub(crate) slim_acc_buf: &'static mut [u8; 1536],
        pub(crate) audio_req_buf: &'static mut [u8; 512],
        pub(crate) audio_header_buf: &'static mut [u8; 1024],
        // --- boot configuration ---
        pub(crate) wifi_sta_creds: WifiStaCredentials,
        pub(crate) audio_cfg: AudioConfig,
        pub(crate) settings_seq: u32,
        pub(crate) settings_dirty: bool,
        pub(crate) current_vol_pct: u8,
        // --- legacy loop variables ---
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
        pub(crate) helo_sent: bool,
        pub(crate) last_stat_ms: u64,
        pub(crate) last_connect_attempt_ms: u64,
        pub(crate) last_slim_connect_ms: u64,
        pub(crate) slim_acc_len: usize,
        pub(crate) audio_stream_active: bool,
        pub(crate) audio_req_sent: bool,
        pub(crate) audio_req_sent_offset: usize,
        pub(crate) audio_headers_stripped: bool,
        pub(crate) audio_req_len: usize,
        pub(crate) audio_header_len: usize,
        pub(crate) audio_header_window: u32,
        pub(crate) pending_resp_len: usize,
        pub(crate) pending_resp_sent: usize,
        pub(crate) last_fault_check_ms: u64,
        pub(crate) latest_telemetry: Option<squeezelite_tas58xx::Tas5805mTelemetry>,
        pub(crate) last_cyd_progress_ms: u64,
        pub(crate) audio_http_eof: bool,
        pub(crate) decode_ready_sent: bool,
        pub(crate) track_started_sent: bool,
        pub(crate) threshold_sent: bool,
        pub(crate) icy_meta_interval: usize,
        pub(crate) icy_meta_next: usize,
        pub(crate) icy_meta_left: usize,
        // --- per-lap values (legacy per-iteration locals) ---
        /// Legacy `let now_ms = now_millis();` at the top of each iteration.
        pub(crate) now_ms: u64,
        /// Legacy `step_frames` (frames the firmware tick wrote this iteration).
        pub(crate) step_frames: usize,
        /// Legacy firmware tick's `coordinator.state == Playing` check, taken after the CYD poll.
        pub(crate) tick_playing: bool,
    }

    /// Number of UDP placeholder sockets in the smoltcp set (spec §3.6).
    pub(crate) const UDP_PLACEHOLDER_SOCKETS: usize = 2;

    /// The superloop state shared by the ring tasks. Only the baton holder borrows it, and never
    /// across an `.await`, so the `RefCell` borrow can not fail.
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

    /// Runs one legacy block. Routing by [`Phase::owner`] guarantees each block executes inside
    /// the task named in the module table.
    fn run_phase(s: &mut FirmwareState, phase: Phase) {
        match phase {
            Phase::WatchdogFeed => watchdog_feed(s),
            Phase::Bluetooth => bt_poll(s),
            Phase::WifiConnect => wifi_connect(s),
            Phase::LeaseCheck => lease_check(s),
            Phase::NetPoll => net_poll(s),
            Phase::HttpServe => http_serve(s),
            Phase::SlimControl => slim_control(s),
            Phase::AudioStream => audio_stream(s),
            Phase::CydCommand => cyd_command(s),
            Phase::OutputTopUp => output_top_up_first(s),
            Phase::DecodeBurst => decode_burst(s),
            Phase::OutputTopUpAndSilence => output_top_up_final(s),
            Phase::StatTrackStarted => stat_track_started(s),
            Phase::DecodeEofDrain => decode_eof_drain(s),
            Phase::StatDecodeReadyUnderrun => stat_decode_ready_underrun(s),
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

    /// Core 0: LMS slimproto session.
    #[embassy_executor::task]
    async fn slimproto_session(shared: &'static Shared) -> ! {
        run_ring(RingTask::SlimprotoSession, shared).await
    }

    /// Core 0: HTTP audio stream fetch into the stream ring.
    #[embassy_executor::task]
    async fn audio_fetch(shared: &'static Shared) -> ! {
        run_ring(RingTask::AudioFetch, shared).await
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

    /// Debounced config persistence (spec §3.5). Stub until Task 10: today settings are still
    /// journaled into `settings_scratch` inline and written to flash only before a reboot.
    #[embassy_executor::task]
    async fn config_writer() {
        core::future::pending::<()>().await
    }

    /// Spawns the core-0 tasks, hands the baton to the first phase and runs the executor in
    /// place of the legacy superloop. Never returns.
    pub(crate) fn run_core0_executor(shared: &'static Shared) -> ! {
        static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
        EXECUTOR0.init(Executor::new()).run(|spawner| {
            spawner.spawn(watchdog_lease(shared).unwrap());
            spawner.spawn(bt_a2dp(shared).unwrap());
            spawner.spawn(net_stack(shared).unwrap());
            spawner.spawn(http_control(shared).unwrap());
            spawner.spawn(slimproto_session(shared).unwrap());
            spawner.spawn(audio_fetch(shared).unwrap());
            spawner.spawn(cyd_bridge(shared).unwrap());
            // Core-0 placement of the audio tasks: see "Core placement" in the module docs.
            spawner.spawn(decode_pump(shared).unwrap());
            spawner.spawn(i2s_feed(shared).unwrap());
            spawner.spawn(config_writer().unwrap());
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
    // Legacy superloop blocks, in PHASE_ORDER. Bodies are the pre-embassy inline code verbatim
    // (locals -> `s.` fields); `#[inline(never)]` keeps each block in its own stack frame so only
    // one block's temporaries are live at a time.
    // ------------------------------------------------------------------------------------------

    /// `watchdog_lease`: feed the 20 s TIMG0 WDT and take the lap's timestamp.
    #[inline(never)]
    fn watchdog_feed(s: &mut FirmwareState) {
        s.wdt.feed();
        s.now_ms = now_millis();
    }

    /// `bt_a2dp`: lazy BT controller bring-up after the DHCP lease, then A2DP sink poll.
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

        // Poll Bluetooth Classic A2DP Sink
        if let Some(ref mut transport) = s.bt_transport {
            s.bt_sink.poll(transport, now_ms, |pcm_bytes, sample_rate_hz| {
                if !s.audio_stream_active {
                    if s.coordinator.current_sample_rate != sample_rate_hz
                        || s.pipeline.sample_rate != sample_rate_hz
                    {
                        s.coordinator.current_sample_rate = sample_rate_hz;
                        let _ = s.pipeline.set_sample_rate(sample_rate_hz);
                    }
                    if !s.bt_audio_active {
                        if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                            MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                        }
                        s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)));
                        s.pipeline.sink.set_mute(false);
                        s.coordinator.state = louder_esp32_app::PlayerState::Playing;
                        if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                            let _ = s.amp.start_playback_via_hiz();
                        }
                        s.bt_audio_active = true;
                    }
                    let _ = s.pipeline.ingest_stream_data(pcm_bytes);
                }
            });

            if s.bt_audio_active && !s.bt_sink.is_streaming() && !s.audio_stream_active {
                if s.pipeline.stream_buffer.available_read() == 0
                    && s.pipeline.output_buffer.available_read() == 0
                    && !s.pipeline.has_pending_pcm()
                {
                    s.coordinator.state = louder_esp32_app::PlayerState::Stopped;
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
                s.assigned_ip = None;
                s.smol_iface.update_ip_addrs(|addrs| addrs.clear());
                s.sockets.get_mut::<tcp::Socket>(s.slim_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.http_handle).abort();
                s.helo_sent = false;
                s.audio_stream_active = false;
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

            // Immediate recycling of closing s.sockets once FIN/ACK handshake completes (TimeWait) or on timeout
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
                        let mut free = 3_538_944u32;
                        let mut used = 655_360u32;
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
                        lms_ip: DEFAULT_LMS_SERVER_IP,
                        lms_port: 9000,
                        lms_cport: 9090,
                        otadata_prearmed_factory: true,
                        frames_played: s.pipeline.frames_played,
                        amp_mode: amp_mode_str,
                        vol_pct: s.current_vol_pct,
                        i2c_acks: Esp32I2cBus::hw_ack_count(),
                        i2c_nacks: Esp32I2cBus::hw_nack_count(),
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
                        settings_seq: s.settings_seq,
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
                                s.audio_cfg.equalizer_preset = preset_id;
                                if persist_audio_config(s.settings_scratch, &s.audio_cfg, &mut s.settings_seq) {
                                    s.settings_dirty = true;
                                }
                            }
                            HttpControlAction::SetAnalogGain(again_code) => {
                                let _ = s.amp.set_analog_gain(again_code);
                                s.audio_cfg.analog_gain_code = again_code;
                                if persist_audio_config(s.settings_scratch, &s.audio_cfg, &mut s.settings_seq) {
                                    s.settings_dirty = true;
                                }
                            }
                            HttpControlAction::PlayerIrCommand { ir_code, vol_pct, mute } => {
                                if let Some(pct) = vol_pct {
                                    s.current_vol_pct = pct;
                                    let vol_reg = percent_to_attenuation(pct);
                                    let _ = s.amp.set_volume(vol_reg);
                                    let eff_gain = if pct > 0 { 65536 } else { 0 };
                                    s.pipeline.set_volume(eff_gain, eff_gain);
                                    let _ = s.cyd_bridge.send_message(&CydMessage::Volume(pct));
                                    s.audio_cfg.volume = pct;
                                    if persist_audio_config(s.settings_scratch, &s.audio_cfg, &mut s.settings_seq) {
                                        s.settings_dirty = true;
                                    }
                                }
                                if let Some(m) = mute {
                                    s.pipeline.sink.set_mute(m);
                                }
                                if ir_code != 0 {
                                    let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                    if slim_sock.may_send() {
                                        let mut ir_buf = [0u8; 32];
                                        if let Ok(ir_len) = SlimprotoBuilder::build_ir(
                                            now_ms as u32,
                                            ir_code,
                                            &mut ir_buf,
                                        ) {
                                            let _ = slim_sock.send_slice(&ir_buf[..ir_len]);
                                        }
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
            if s.settings_dirty {
                s.core1.park_for_flash_write();
                let _ = s.flash.erase(SETTINGS_JOURNAL_OFFSET, SETTINGS_JOURNAL_OFFSET + 4096);
                let _ = s.flash.write(SETTINGS_JOURNAL_OFFSET, s.settings_scratch);
            }
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

    /// `slimproto_session`: LMS control connection (half-close reset, connect, RESP flush,
    /// HELO/SETD/STAT heartbeats, frame receive and dispatch) once the IPv4 lease is active.
    #[inline(never)]
    fn slim_control(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        // Drive SlimProto Client (`10.0.0.10:3483`) once IPv4 lease is active
        if let Some(local_ip) = s.assigned_ip {
            // Half-closed / closing state detection: if remote sent FIN or is closing,
            // abort socket so we immediately reconnect rather than lingering in CloseWait.
            {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
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
                    s.helo_sent = false;
                    s.slim_acc_len = 0;
                }
            }

            // Connect to LMS if closed (rate-limited to 1s)
            {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if !slim_sock.is_open()
                    && (s.last_slim_connect_ms == 0
                        || now_ms.saturating_sub(s.last_slim_connect_ms) >= 1_000)
                {
                    s.last_slim_connect_ms = now_ms;
                    s.helo_sent = false;
                    s.slim_acc_len = 0;
                    let remote_ep = (
                        IpAddress::Ipv4(Ipv4Address::from(DEFAULT_LMS_SERVER_IP)),
                        DEFAULT_LMS_SERVER_PORT,
                    );
                    s.local_port_counter = 49152 + ((s.local_port_counter.wrapping_add(1) - 49152) & 0x3FFF);
                    let local_ep = (
                        IpAddress::Ipv4(Ipv4Address::from(local_ip)),
                        s.local_port_counter,
                    );
                    let _ = slim_sock.connect(s.smol_iface.context(), remote_ep, local_ep);
                }
            }

            // Flush pending RESP bytes before any STAT/HELO frames
            if s.pending_resp_sent < s.pending_resp_len {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if slim_sock.may_send() {
                    if let Ok(n) = slim_sock.send_slice(&s.pending_resp_buf[s.pending_resp_sent..s.pending_resp_len]) {
                        s.pending_resp_sent += n;
                    }
                }
            }

            // Initial HELO + SETD ("BASEMENT") + STAT Heartbeat, and periodic Heartbeats
            {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if slim_sock.may_send() && !s.helo_sent && s.pending_resp_sent == s.pending_resp_len {
                    let mut helo_buf = [0u8; 256];
                    if let Ok(helo_len) = SlimprotoBuilder::build_helo(
                        &BASEMENT_PLAYER_MAC,
                        12,
                        BASEMENT_HELO_CAPABILITIES,
                        &mut helo_buf,
                    ) {
                        if slim_sock.send_slice(&helo_buf[..helo_len]).is_ok() {
                            let mut setd_buf = [0u8; 128];
                            if let Ok(setd_len) = SlimprotoBuilder::build_setd_name(
                                s.coordinator.player_name.as_str(),
                                &mut setd_buf,
                            ) {
                                let _ = slim_sock.send_slice(&setd_buf[..setd_len]);
                            }

                            let mut stat_buf = [0u8; 128];
                            if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                StatEvent::Heartbeat,
                                s.pipeline.stream_buffer_available() as u32,
                                s.pipeline.output_buffer_available() as u32,
                                s.pipeline.frames_played,
                                s.coordinator.current_sample_rate,
                                s.coordinator.stream_bytes_received,
                                now_ms as u32,
                                0,
                                &mut stat_buf,
                            ) {
                                let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                            }

                            s.helo_sent = true;
                            s.last_stat_ms = now_ms;
                            s.coordinator.state = louder_esp32_app::PlayerState::Connected;
                        }
                    }
                } else {
                    let stat_interval_ms: u64 = if s.coordinator.state == louder_esp32_app::PlayerState::Playing {
                        1_000
                    } else {
                        5_000
                    };
                    if slim_sock.may_send()
                        && s.helo_sent
                        && s.pending_resp_sent == s.pending_resp_len
                        && now_ms.saturating_sub(s.last_stat_ms) >= stat_interval_ms
                    {
                        s.last_stat_ms = now_ms;
                        let mut stat_buf = [0u8; 128];
                        if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                            StatEvent::Heartbeat,
                            s.pipeline.stream_buffer_available() as u32,
                            s.pipeline.output_buffer_available() as u32,
                            s.pipeline.frames_played,
                            s.coordinator.current_sample_rate,
                            s.coordinator.stream_bytes_received,
                            now_ms as u32,
                            0,
                            &mut stat_buf,
                        ) {
                            let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                        }
                    }
                }
            }

            // Read incoming SlimProto bytes into accumulation buffer
            {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if slim_sock.can_recv() && s.slim_acc_len < s.slim_acc_buf.len() {
                    let n = slim_sock
                        .recv_slice(&mut s.slim_acc_buf[s.slim_acc_len..])
                        .unwrap_or(0);
                    s.slim_acc_len += n;
                }
            }

            // Drain all complete frames from accumulation buffer
            while s.slim_acc_len >= 6 {
                match SlimprotoParser::parse_frame(&s.slim_acc_buf[..s.slim_acc_len]) {
                    Ok((cmd, consumed)) => {
                        if let Some(setting) = cmd.device_setting() {
                            if setting.id == 0 {
                                let name_bytes = if let Some(pos) =
                                    setting.data.iter().position(|&b| b == 0)
                                {
                                    &setting.data[..pos]
                                } else {
                                    setting.data
                                };
                                if !name_bytes.is_empty() {
                                    if let Ok(name_str) = core::str::from_utf8(name_bytes) {
                                        s.coordinator.player_name.clear();
                                        let _ = s.coordinator.player_name.push_str(name_str);
                                    }
                                }
                                let mut tx_buf = [0u8; 128];
                                if let Ok(tx_len) = SlimprotoBuilder::build_setd_name(
                                    s.coordinator.player_name.as_str(),
                                    &mut tx_buf,
                                ) {
                                    let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                    if slim_sock.may_send() {
                                        let _ = slim_sock.send_slice(&tx_buf[..tx_len]);
                                    }
                                }
                            }
                        } else {
                            match cmd {
                                ServerCommand::AudioGain {
                                    gain_left,
                                    gain_right,
                                    ..
                                } => {
                                    let max_gain = core::cmp::max(gain_left, gain_right);
                                    let vol_reg = lms_gain_to_tas5805m_vol_reg(max_gain);
                                    if s.amp.current_volume() != vol_reg {
                                        let _ = s.amp.set_volume(vol_reg);
                                    }
                                    let eff_gain = if max_gain > 0 { 65536 } else { 0 };
                                    s.pipeline.set_volume(eff_gain, eff_gain);
                                    let new_pct = if vol_reg >= 0xFF {
                                        0
                                    } else {
                                        100u8.saturating_sub(vol_reg.saturating_sub(0x30))
                                    };
                                    s.current_vol_pct = new_pct;
                                    s.audio_cfg.volume = new_pct;
                                    if persist_audio_config(s.settings_scratch, &s.audio_cfg, &mut s.settings_seq) {
                                        s.settings_dirty = true;
                                    }
                                    let _ = s.cyd_bridge.send_message(&CydMessage::Volume(new_pct));
                                }
                                ServerCommand::Stream {
                                    command,
                                    autostart,
                                    format,
                                    pcm_sample_size,
                                    pcm_sample_rate,
                                    pcm_channels,
                                    pcm_endianness,
                                    flags,
                                    replay_gain,
                                    server_port,
                                    server_ip,
                                    http_headers,
                                    ..
                                } => {
                                    match command {
                                        StreamCommandType::Timestamp => {
                                            let mut stat_buf = [0u8; 128];
                                            if let Ok(stat_len) =
                                                StatBuilder::build_stat_with_metrics(
                                                    StatEvent::Heartbeat,
                                                    s.pipeline.stream_buffer_available() as u32,
                                                    s.pipeline.output_buffer_available() as u32,
                                                    s.pipeline.frames_played,
                                                    s.coordinator.current_sample_rate,
                                                    s.coordinator.stream_bytes_received,
                                                    now_ms as u32,
                                                    replay_gain,
                                                    &mut stat_buf,
                                                )
                                            {
                                                let slim_sock =
                                                    s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                                if slim_sock.may_send() {
                                                    let _ = slim_sock
                                                        .send_slice(&stat_buf[..stat_len]);
                                                }
                                            }
                                        }
                                        StreamCommandType::Start => {
                                            s.icy_meta_interval = 0;
                                            s.icy_meta_next = 0;
                                            s.icy_meta_left = 0;
                                            s.audio_header_window = 0;
                                            s.pending_resp_len = 0;
                                            s.pending_resp_sent = 0;

                                            s.pipeline.flush();
                                            s.pipeline.frames_played = 0;
                                            s.coordinator.frames_played = 0;
                                            s.coordinator.stream_bytes_received = 0;
                                            s.audio_http_eof = false;
                                            s.decode_ready_sent = false;
                                            s.track_started_sent = false;
                                            s.threshold_sent = false;

                                            s.coordinator.autostart_mode =
                                                AutostartMode::from_strm(autostart, flags);
                                            s.coordinator.awaiting_cont =
                                                s.coordinator.autostart_mode.requires_cont();
                                            s.coordinator.current_format = Some(format);

                                            let sample_rate =
                                                PlayerCoordinator::decode_pcm_sample_rate_hz(
                                                    pcm_sample_rate,
                                                )
                                                .unwrap_or_else(|| {
                                                    sample_rate_from_index(pcm_sample_rate)
                                                });
                                            s.coordinator.current_sample_rate = sample_rate;
                                            let _ = s.pipeline.set_sample_rate(sample_rate);

                                            let bits = PlayerCoordinator::normalize_pcm_bits(
                                                pcm_sample_size,
                                            );
                                            let channels =
                                                PlayerCoordinator::normalize_pcm_channels(
                                                    pcm_channels,
                                                );
                                            let is_be = matches!(
                                                pcm_endianness,
                                                Endianness::BigEndian
                                            );
                                            if format == squeezelite_proto::AudioFormat::Mp3 {
                                                match s.pipeline.decoder {
                                                    ActiveDecoder::Mp3(ref mut dec) => {
                                                        dec.reset_in_place();
                                                    }
                                                    _ => {
                                                        s.pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));
                                                    }
                                                }
                                                s.pipeline.sink.set_mute(false);
                                            } else if format == squeezelite_proto::AudioFormat::Flac {
                                                if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                    MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                                }
                                                match s.pipeline.decoder {
                                                    ActiveDecoder::Flac(ref mut dec) => {
                                                        dec.flush();
                                                    }
                                                    _ => {
                                                        s.pipeline.set_decoder(ActiveDecoder::Flac(alloc::boxed::Box::new(squeezelite_decoders::FlacDecoder::new())));
                                                    }
                                                }
                                                s.pipeline.sink.set_mute(false);
                                            } else if format == squeezelite_proto::AudioFormat::Pcm {
                                                if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                    MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                                }
                                                s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
                                                s.pipeline.sink.set_mute(false);
                                            } else {
                                                if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                    MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                                }
                                                s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
                                                s.pipeline.sink.set_mute(true);
                                            }

                                            s.pipeline.sink.driver.pump_silence_if_needed(false, false);
                                            if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                                                let _ = s.amp.clear_faults();
                                                let _ = s.amp.start_playback_via_hiz();
                                            }

                                            if http_headers.is_empty() {
                                                let req = b"GET /stream.mp3 HTTP/1.0\r\n\r\n";
                                                s.audio_req_buf[..req.len()].copy_from_slice(req);
                                                s.audio_req_len = req.len();
                                            } else {
                                                let copy_len = http_headers.len().min(s.audio_req_buf.len());
                                                s.audio_req_buf[..copy_len]
                                                    .copy_from_slice(&http_headers[..copy_len]);
                                                s.audio_req_len = copy_len;
                                            }

                                            let audio_sock =
                                                s.sockets.get_mut::<tcp::Socket>(s.audio_handle);
                                            audio_sock.abort();
                                            let audio_remote_ip = if server_ip == [0, 0, 0, 0] {
                                                DEFAULT_LMS_SERVER_IP
                                            } else {
                                                server_ip
                                            };
                                            let audio_remote_port = if server_port == 0 {
                                                9000
                                            } else {
                                                server_port
                                            };
                                            let remote_ep = (
                                                IpAddress::Ipv4(Ipv4Address::from(
                                                    audio_remote_ip,
                                                )),
                                                audio_remote_port,
                                            );
                                            s.local_port_counter = 49152 + ((s.local_port_counter.wrapping_add(1) - 49152) & 0x3FFF);
                                            let local_ep = (
                                                IpAddress::Ipv4(Ipv4Address::from(local_ip)),
                                                s.local_port_counter,
                                            );
                                            let _ = audio_sock.connect(
                                                s.smol_iface.context(),
                                                remote_ep,
                                                local_ep,
                                            );
                                            s.audio_stream_active = true;
                                            s.audio_req_sent = false;
                                            s.audio_req_sent_offset = 0;
                                            s.audio_headers_stripped = false;
                                            s.audio_header_len = 0;

                                            if s.pending_resp_sent == s.pending_resp_len {
                                                let slim_sock =
                                                    s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::Connected,
                                                            s.pipeline.stream_buffer_available()
                                                                as u32,
                                                            s.pipeline.output_buffer_available()
                                                                as u32,
                                                            s.pipeline.frames_played,
                                                            s.coordinator.current_sample_rate,
                                                            s.coordinator.stream_bytes_received,
                                                            now_ms as u32,
                                                            0,
                                                            &mut stat_buf,
                                                        )
                                                    {
                                                        let _ = slim_sock
                                                            .send_slice(&stat_buf[..stat_len]);
                                                    }
                                                }
                                            }

                                            s.coordinator.state =
                                                louder_esp32_app::PlayerState::Playing;
                                        }
                                        StreamCommandType::Pause => {
                                            s.coordinator.state =
                                                louder_esp32_app::PlayerState::Paused;
                                            s.pipeline.sink.set_mute(true);
                                            let slim_sock =
                                                s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                            if slim_sock.may_send() {
                                                let mut stat_buf = [0u8; 128];
                                                if let Ok(stat_len) =
                                                    StatBuilder::build_stat_with_metrics(
                                                        StatEvent::TrackPaused,
                                                        s.pipeline.stream_buffer_available()
                                                            as u32,
                                                        s.pipeline.output_buffer_available()
                                                            as u32,
                                                        s.pipeline.frames_played,
                                                        s.coordinator.current_sample_rate,
                                                        s.coordinator.stream_bytes_received,
                                                        now_ms as u32,
                                                        0,
                                                        &mut stat_buf,
                                                    )
                                                {
                                                    let _ = slim_sock
                                                        .send_slice(&stat_buf[..stat_len]);
                                                }
                                            }
                                        }
                                        StreamCommandType::Resume => {
                                            s.coordinator.state =
                                                louder_esp32_app::PlayerState::Playing;
                                            s.pipeline.sink.set_mute(false);
                                            let slim_sock =
                                                s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                            if slim_sock.may_send() {
                                                let mut stat_buf = [0u8; 128];
                                                if let Ok(stat_len) =
                                                    StatBuilder::build_stat_with_metrics(
                                                        StatEvent::TrackResumed,
                                                        s.pipeline.stream_buffer_available()
                                                            as u32,
                                                        s.pipeline.output_buffer_available()
                                                            as u32,
                                                        s.pipeline.frames_played,
                                                        s.coordinator.current_sample_rate,
                                                        s.coordinator.stream_bytes_received,
                                                        now_ms as u32,
                                                        0,
                                                        &mut stat_buf,
                                                    )
                                                {
                                                    let _ = slim_sock
                                                        .send_slice(&stat_buf[..stat_len]);
                                                }
                                            }
                                        }
                                        StreamCommandType::Stop => {
                                            s.coordinator.state =
                                                louder_esp32_app::PlayerState::Stopped;
                                            s.audio_stream_active = false;
                                            s.audio_http_eof = false;
                                            s.decode_ready_sent = false;
                                            s.track_started_sent = false;
                                            s.threshold_sent = false;
                                            s.sockets
                                                .get_mut::<tcp::Socket>(s.audio_handle)
                                                .abort();
                                            s.pipeline.sink.set_mute(true);
                                            let slim_sock =
                                                s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                            if slim_sock.may_send() {
                                                let mut stat_buf = [0u8; 128];
                                                if let Ok(stat_len) =
                                                    StatBuilder::build_stat_with_metrics(
                                                        StatEvent::TrackStopped,
                                                        s.pipeline.stream_buffer_available()
                                                            as u32,
                                                        s.pipeline.output_buffer_available()
                                                            as u32,
                                                        s.pipeline.frames_played,
                                                        s.coordinator.current_sample_rate,
                                                        s.coordinator.stream_bytes_received,
                                                        now_ms as u32,
                                                        0,
                                                        &mut stat_buf,
                                                    )
                                                {
                                                    let _ = slim_sock
                                                        .send_slice(&stat_buf[..stat_len]);
                                                }
                                            }
                                        }
                                        StreamCommandType::Flush => {
                                            s.audio_stream_active = false;
                                            s.audio_http_eof = false;
                                            s.decode_ready_sent = false;
                                            s.track_started_sent = false;
                                            s.threshold_sent = false;
                                            s.sockets
                                                .get_mut::<tcp::Socket>(s.audio_handle)
                                                .abort();
                                            s.pipeline.flush();
                                            let slim_sock =
                                                s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                            if slim_sock.may_send() {
                                                let mut stat_buf = [0u8; 128];
                                                if let Ok(stat_len) =
                                                    StatBuilder::build_stat_with_metrics(
                                                        StatEvent::StreamFlushed,
                                                        s.pipeline.stream_buffer_available()
                                                            as u32,
                                                        s.pipeline.output_buffer_available()
                                                            as u32,
                                                        s.pipeline.frames_played,
                                                        s.coordinator.current_sample_rate,
                                                        s.coordinator.stream_bytes_received,
                                                        now_ms as u32,
                                                        0,
                                                        &mut stat_buf,
                                                    )
                                                {
                                                    let _ = slim_sock
                                                        .send_slice(&stat_buf[..stat_len]);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                ServerCommand::SetDevice { sample_rate } => {
                                    if sample_rate > u8::MAX as u32 {
                                        s.coordinator.current_sample_rate = sample_rate;
                                        let _ = s.pipeline.set_sample_rate(sample_rate);
                                    }
                                }
                                ServerCommand::Flush => {
                                    s.audio_stream_active = false;
                                    s.audio_http_eof = false;
                                    s.decode_ready_sent = false;
                                    s.track_started_sent = false;
                                    s.threshold_sent = false;
                                    s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                                    s.pipeline.flush();
                                    let slim_sock =
                                        s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                    if slim_sock.may_send() {
                                        let mut stat_buf = [0u8; 128];
                                        if let Ok(stat_len) =
                                            StatBuilder::build_stat_with_metrics(
                                                StatEvent::StreamFlushed,
                                                s.pipeline.stream_buffer_available() as u32,
                                                s.pipeline.output_buffer_available() as u32,
                                                s.pipeline.frames_played,
                                                s.coordinator.current_sample_rate,
                                                s.coordinator.stream_bytes_received,
                                                now_ms as u32,
                                                0,
                                                &mut stat_buf,
                                            )
                                        {
                                            let _ =
                                                slim_sock.send_slice(&stat_buf[..stat_len]);
                                        }
                                    }
                                }
                                ServerCommand::Unknown { opcode, payload } => {
                                    if let Some((_spdif, dac_enable)) = cmd.audio_enable() {
                                        s.coordinator.dac_enabled = dac_enable;
                                        s.pipeline.sink.set_mute(!dac_enable);
                                    } else if let Some(metaint) = cmd.stream_continue() {
                                        s.coordinator.awaiting_cont = false;
                                        if metaint > 0 && s.icy_meta_interval == 0 {
                                            let interval = metaint as usize;
                                            let already = (s.coordinator.stream_bytes_received as usize) % interval;
                                            s.icy_meta_interval = interval;
                                            s.icy_meta_next = interval - already;
                                            s.icy_meta_left = 0;
                                        }
                                        if !s.threshold_sent && !s.coordinator.autostart_mode.is_auto() {
                                            let mut stat_buf = [0u8; 128];
                                            if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                                StatEvent::BUFFER_THRESHOLD,
                                                s.pipeline.stream_buffer_available() as u32,
                                                s.pipeline.output_buffer_available() as u32,
                                                s.pipeline.frames_played,
                                                s.coordinator.current_sample_rate,
                                                s.coordinator.stream_bytes_received,
                                                now_ms as u32,
                                                0,
                                                &mut stat_buf,
                                            ) {
                                                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                                if slim_sock.may_send() && slim_sock.send_slice(&stat_buf[..stat_len]) == Ok(stat_len) {
                                                    s.threshold_sent = true;
                                                }
                                            }
                                        }
                                    } else if opcode == [b'c', b'o', b'd', b'c'] && !payload.is_empty() {
                                        let codc_fmt = squeezelite_proto::AudioFormat::from_byte(payload[0]);
                                        s.coordinator.current_format = Some(codc_fmt);
                                        if codc_fmt == squeezelite_proto::AudioFormat::Mp3 {
                                            match s.pipeline.decoder {
                                                ActiveDecoder::Mp3(ref mut dec) => {
                                                    dec.reset_in_place();
                                                }
                                                _ => {
                                                    s.pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));
                                                }
                                            }
                                            s.pipeline.sink.set_mute(false);
                                        } else if codc_fmt == squeezelite_proto::AudioFormat::Flac {
                                            if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                            }
                                            match s.pipeline.decoder {
                                                ActiveDecoder::Flac(ref mut dec) => {
                                                    dec.flush();
                                                }
                                                _ => {
                                                    s.pipeline.set_decoder(ActiveDecoder::Flac(alloc::boxed::Box::new(squeezelite_decoders::FlacDecoder::new())));
                                                }
                                            }
                                            s.pipeline.sink.set_mute(false);
                                        } else if codc_fmt == squeezelite_proto::AudioFormat::Pcm {
                                            if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                            }
                                            let bits = if payload.len() > 1 {
                                                PlayerCoordinator::normalize_pcm_bits(payload[1])
                                            } else {
                                                16
                                            };
                                            let channels = if payload.len() > 3 {
                                                PlayerCoordinator::normalize_pcm_channels(payload[3])
                                            } else {
                                                2
                                            };
                                            let is_be = if payload.len() > 4 {
                                                matches!(Endianness::from_byte(payload[4]), Endianness::BigEndian)
                                            } else {
                                                true
                                            };
                                            s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
                                            s.pipeline.sink.set_mute(false);
                                        } else {
                                            if matches!(s.pipeline.decoder, ActiveDecoder::Mp3(_)) {
                                                MP3_DEC_TAKEN.store(false, core::sync::atomic::Ordering::Release);
                                            }
                                            // Unsupported compressed codec: mute sink to avoid loud PCM static noise
                                            s.pipeline.sink.set_mute(true);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        if consumed > 0 {
                            s.slim_acc_buf.copy_within(consumed..s.slim_acc_len, 0);
                            s.slim_acc_len -= consumed;
                        } else {
                            break;
                        }
                    }
                    Err(ParseError::BufferTooShort) => {
                        if s.slim_acc_len >= s.slim_acc_buf.len() {
                            s.slim_acc_buf.copy_within(1..s.slim_acc_len, 0);
                            s.slim_acc_len -= 1;
                        }
                        break;
                    }
                    Err(_) => {
                        if s.slim_acc_len > 0 {
                            s.slim_acc_buf.copy_within(1..s.slim_acc_len, 0);
                            s.slim_acc_len -= 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    /// `audio_fetch`: HTTP audio socket (request, header strip + RESP, ICY-aware body ingest,
    /// EOF -> DSCO) while a stream is active and the IPv4 lease holds.
    #[inline(never)]
    fn audio_stream(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        if s.assigned_ip.is_none() {
            return;
        }
        // Drive audio_sock HTTP streaming when active
        if s.audio_stream_active {
            // Send HTTP GET request headers with offset tracking
            {
                let audio_sock = s.sockets.get_mut::<tcp::Socket>(s.audio_handle);
                if audio_sock.may_send() && !s.audio_req_sent {
                    if let Ok(n) = audio_sock.send_slice(&s.audio_req_buf[s.audio_req_sent_offset..s.audio_req_len]) {
                        s.audio_req_sent_offset += n;
                        if s.audio_req_sent_offset >= s.audio_req_len {
                            s.audio_req_sent = true;
                        }
                    }
                }
            }

            // Process incoming HTTP stream bytes
            if !s.audio_headers_stripped {
                let mut header_matched = false;
                let audio_sock = s.sockets.get_mut::<tcp::Socket>(s.audio_handle);
                if audio_sock.can_recv() {
                    let _ = audio_sock.recv(|rx_buf| {
                        let mut consumed = 0;
                        while consumed < rx_buf.len() {
                            let b = rx_buf[consumed];
                            consumed += 1;
                            s.audio_header_window = (s.audio_header_window << 8) | (b as u32);
                            if s.audio_header_len < s.audio_header_buf.len() {
                                s.audio_header_buf[s.audio_header_len] = b;
                                s.audio_header_len += 1;
                            }
                            if s.audio_header_window == 0x0D0A_0D0A
                                || (s.audio_header_window & 0xFFFF) == 0x0A0A
                            {
                                header_matched = true;
                                break;
                            }
                        }
                        (consumed, ())
                    });
                }

                if header_matched {
                    s.audio_headers_stripped = true;

                    if s.icy_meta_interval == 0 {
                        if let Some(interval) = parse_icy_metaint_header(&s.audio_header_buf[..s.audio_header_len]) {
                            s.icy_meta_interval = interval;
                            s.icy_meta_next = interval;
                            s.icy_meta_left = 0;
                        }
                    }

                    if let Ok(resp_len) = SlimprotoBuilder::build_resp(
                        &s.audio_header_buf[..s.audio_header_len],
                        s.pending_resp_buf,
                    ) {
                        s.pending_resp_len = resp_len;
                        s.pending_resp_sent = 0;
                        let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                        if slim_sock.may_send() {
                            if let Ok(n) = slim_sock.send_slice(&s.pending_resp_buf[..resp_len]) {
                                s.pending_resp_sent = n;
                            }
                        }
                    }

                    if !s.threshold_sent && !s.coordinator.autostart_mode.is_auto() && !s.coordinator.awaiting_cont {
                        if s.pending_resp_sent == s.pending_resp_len {
                            let mut stat_buf = [0u8; 128];
                            if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                StatEvent::BUFFER_THRESHOLD,
                                s.pipeline.stream_buffer_available() as u32,
                                s.pipeline.output_buffer_available() as u32,
                                s.pipeline.frames_played,
                                s.coordinator.current_sample_rate,
                                s.coordinator.stream_bytes_received,
                                now_ms as u32,
                                0,
                                &mut stat_buf,
                            ) {
                                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                if slim_sock.may_send() && slim_sock.send_slice(&stat_buf[..stat_len]) == Ok(stat_len) {
                                    s.threshold_sent = true;
                                }
                            }
                        }
                    }
                }
            } else if !s.audio_http_eof && !s.coordinator.awaiting_cont {
                let mut audio_rx_chunk = [0u8; 512];
                for _ in 0..32 {
                    if !s.sockets.get_mut::<tcp::Socket>(s.audio_handle).can_recv() {
                        break;
                    }
                    if s.icy_meta_interval > 0 && s.icy_meta_next == 0 {
                        if s.icy_meta_left == 0 {
                            let mut c = [0u8; 1];
                            let n = s.sockets
                                .get_mut::<tcp::Socket>(s.audio_handle)
                                .recv_slice(&mut c)
                                .unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            s.icy_meta_left = (c[0] as usize) * 16;
                            if s.icy_meta_left == 0 {
                                s.icy_meta_next = s.icy_meta_interval;
                                continue;
                            }
                        }
                        if s.icy_meta_left > 0 {
                            let to_skip = s.icy_meta_left.min(audio_rx_chunk.len());
                            let n = s.sockets
                                .get_mut::<tcp::Socket>(s.audio_handle)
                                .recv_slice(&mut audio_rx_chunk[..to_skip])
                                .unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            s.icy_meta_left -= n;
                            if s.icy_meta_left == 0 {
                                s.icy_meta_next = s.icy_meta_interval;
                            }
                            continue;
                        }
                    } else {
                        let avail_write = s.pipeline.stream_buffer.available_write();
                        if avail_write == 0 {
                            break;
                        }
                        let max_chunk = if s.icy_meta_interval > 0 {
                            audio_rx_chunk.len().min(s.icy_meta_next).min(avail_write)
                        } else {
                            audio_rx_chunk.len().min(avail_write)
                        };
                        if max_chunk == 0 {
                            break;
                        }
                        let n = s.sockets
                            .get_mut::<tcp::Socket>(s.audio_handle)
                            .recv_slice(&mut audio_rx_chunk[..max_chunk])
                            .unwrap_or(0);
                        if n > 0 {
                            let ingested = s.pipeline.ingest_stream_data(&audio_rx_chunk[..n]).unwrap_or(0);
                            s.coordinator.stream_bytes_received += ingested as u64;
                            if s.icy_meta_interval > 0 {
                                s.icy_meta_next = s.icy_meta_next.saturating_sub(ingested);
                            }
                        } else {
                            break;
                        }
                    }
                }
            }

            // Check HTTP socket EOF:
            let sock_eof = {
                let audio_sock = s.sockets.get_mut::<tcp::Socket>(s.audio_handle);
                s.audio_req_sent
                    && s.audio_headers_stripped
                    && !audio_sock.can_recv()
                    && !audio_sock.may_recv()
            };
            if sock_eof && !s.audio_http_eof {
                s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                s.audio_http_eof = true;
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if slim_sock.may_send() {
                    let mut dsco_buf = [0u8; 32];
                    if let Ok(dsco_len) = SlimprotoBuilder::build_dsco(
                        DisconnectReason::ConnectionClosed,
                        &mut dsco_buf,
                    ) {
                        let _ = slim_sock.send_slice(&dsco_buf[..dsco_len]);
                    }
                }
            }
        }
    }

    /// `cyd_bridge`: legacy firmware tick step 0 — apply one pending CYD command.
    #[inline(never)]
    fn cyd_command(s: &mut FirmwareState) {
        if let Ok(Some(cmd)) = s.cyd_bridge.poll_command() {
            if let Some(reply) = s.coordinator.handle_cyd_command(cmd, s.pipeline) {
                let _ = s.cyd_bridge.flush_outbound(&reply);
            }
        }
    }

    /// One legacy DMA top-up loop: drain pending decoded PCM into the output ring and push the
    /// output ring into the I2S0 DMA, up to 16 rounds (legacy tick steps 1 and 3).
    #[inline(never)]
    fn output_top_up(s: &mut FirmwareState) {
        for _ in 0..16 {
            if s.pipeline.has_pending_pcm() && s.pipeline.output_buffer.available_write() >= 8 {
                let _ = s.pipeline.process_decode_step();
            }
            if !s.coordinator.dac_enabled || s.pipeline.output_buffer.available_read() < 8 {
                break;
            }
            let written = s.pipeline.process_output_step().unwrap_or(0);
            s.step_frames += written;
            s.coordinator.frames_played = s.pipeline.frames_played;
            if written == 0 {
                break;
            }
        }
    }

    /// `i2s_feed`: legacy firmware tick step 1 — fast O(1) drain of pending decoded PCM and an
    /// I2S0 DMA top-up BEFORE spending ~4.5 ms decoding a new MP3 frame.
    #[inline(never)]
    fn output_top_up_first(s: &mut FirmwareState) {
        s.step_frames = 0;
        s.tick_playing = s.coordinator.state == louder_esp32_app::PlayerState::Playing;
        if s.tick_playing {
            output_top_up(s);
        }
    }

    /// `decode_pump`: legacy firmware tick step 2 — with the DMA topped up, decode at most one
    /// audio frame (or a fast resync/skip of up to 16 chunks) if no pending PCM remains and the
    /// output ring has space.
    #[inline(never)]
    fn decode_burst(s: &mut FirmwareState) {
        if !s.tick_playing {
            return;
        }
        let max_decode_steps = match s.pipeline.decoder {
            ActiveDecoder::Mp3(_) | ActiveDecoder::Flac(_) => 16,
            _ => 4,
        };
        for _ in 0..max_decode_steps {
            if !s.pipeline.has_pending_pcm()
                && s.pipeline.output_buffer.available_write() >= 1024
                && s.pipeline.stream_buffer.available_read() > 0
            {
                let prev_out = s.pipeline.output_buffer.available_read();
                let res = s.pipeline.process_decode_step();
                if s.pipeline.has_pending_pcm() || s.pipeline.output_buffer.available_read() > prev_out {
                    break;
                }
                if !matches!(res, Ok(c) if c > 0) {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// `i2s_feed`: legacy firmware tick step 3 + tail (top up the DMA with the new frames, drain
    /// pending PCM, sync the coordinator rate, stop the amp when the DAC is disabled), then the
    /// legacy loop's silence pump.
    #[inline(never)]
    fn output_top_up_final(s: &mut FirmwareState) {
        if s.tick_playing {
            output_top_up(s);
            if s.pipeline.has_pending_pcm() && s.pipeline.output_buffer.available_write() >= 8 {
                let _ = s.pipeline.process_decode_step();
            }
        }
        s.coordinator.current_sample_rate = s.pipeline.sample_rate;

        if !s.coordinator.dac_enabled {
            let _ = s.amp.stop_playback();
        }

        let has_buffered_audio =
            s.pipeline.output_buffer.available_read() >= 8 || s.pipeline.has_pending_pcm();
        s.pipeline.sink.driver.pump_silence_if_needed(
            s.coordinator.state == louder_esp32_app::PlayerState::Playing,
            has_buffered_audio,
        );
    }

    /// `slimproto_session`: `STMs` once the first decoded frame has been played.
    #[inline(never)]
    fn stat_track_started(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        if s.audio_stream_active
            && !s.track_started_sent
            && s.pipeline.frames_played > 0
            && s.pending_resp_sent == s.pending_resp_len
        {
            let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
            if slim_sock.may_send() {
                let mut stat_buf = [0u8; 128];
                if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                    StatEvent::TrackStarted,
                    s.pipeline.stream_buffer_available() as u32,
                    s.pipeline.output_buffer_available() as u32,
                    s.pipeline.frames_played,
                    s.coordinator.current_sample_rate,
                    s.coordinator.stream_bytes_received,
                    now_ms as u32,
                    0,
                    &mut stat_buf,
                ) {
                    if slim_sock.send_slice(&stat_buf[..stat_len]) == Ok(stat_len) {
                        s.track_started_sent = true;
                    }
                }
            }
        }
    }

    /// `decode_pump`: after HTTP EOF, decode the stream-ring tail (dropping a truncated frame).
    #[inline(never)]
    fn decode_eof_drain(s: &mut FirmwareState) {
        if s.audio_stream_active && s.audio_http_eof {
            if !s.pipeline.has_pending_pcm() && s.pipeline.stream_buffer.available_read() > 0 {
                if matches!(s.pipeline.process_decode_step(), Err(squeezelite_decoders::DecoderError::NeedMoreData)) {
                    s.pipeline.stream_buffer.flush();
                }
            }
        }
    }

    /// `slimproto_session`: `STMd` once the stream is fully decoded, then `STMu` + stop once
    /// the output has drained.
    #[inline(never)]
    fn stat_decode_ready_underrun(s: &mut FirmwareState) {
        let now_ms = s.now_ms;
        if s.audio_stream_active
            && s.audio_http_eof
            && !s.decode_ready_sent
            && !s.pipeline.has_pending_pcm()
            && s.pipeline.stream_buffer.available_read() == 0
            && s.pending_resp_sent == s.pending_resp_len
        {
            let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
            if slim_sock.may_send() {
                let mut stat_buf = [0u8; 128];
                if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                    StatEvent::DecodeReady,
                    s.pipeline.stream_buffer_available() as u32,
                    s.pipeline.output_buffer_available() as u32,
                    s.pipeline.frames_played,
                    s.coordinator.current_sample_rate,
                    s.coordinator.stream_bytes_received,
                    now_ms as u32,
                    0,
                    &mut stat_buf,
                ) {
                    if slim_sock.send_slice(&stat_buf[..stat_len]) == Ok(stat_len) {
                        s.decode_ready_sent = true;
                    }
                }
            }
        }

        if s.audio_stream_active
            && s.audio_http_eof
            && s.decode_ready_sent
            && !s.pipeline.has_pending_pcm()
            && s.pipeline.stream_buffer.available_read() == 0
            && s.pipeline.output_buffer.available_read() == 0
            && s.step_frames == 0
        {
            if s.pending_resp_sent == s.pending_resp_len {
                let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                if slim_sock.may_send() {
                    let mut stat_buf = [0u8; 128];
                    if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                        StatEvent::TrackUnderrun,
                        s.pipeline.stream_buffer_available() as u32,
                        s.pipeline.output_buffer_available() as u32,
                        s.pipeline.frames_played,
                        s.coordinator.current_sample_rate,
                        s.coordinator.stream_bytes_received,
                        now_ms as u32,
                        0,
                        &mut stat_buf,
                    ) {
                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                    }
                }
                s.audio_stream_active = false;
                s.coordinator.state = louder_esp32_app::PlayerState::Stopped;
            }
        }
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
    fn test_phase_ring_matches_legacy_superloop_order() {
        // Owner sequence of one lap == the order the pre-embassy superloop ran its blocks.
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
                RingTask::SlimprotoSession, // LMS control socket
                RingTask::AudioFetch,       // audio socket
                RingTask::CydBridge,        // firmware tick: CYD command
                RingTask::I2sFeed,          // firmware tick: DMA top-up before decode
                RingTask::DecodePump,       // firmware tick: decode burst
                RingTask::I2sFeed,          // firmware tick: top-up + DAC gate, silence pump
                RingTask::SlimprotoSession, // STMs
                RingTask::DecodePump,       // post-EOF tail decode
                RingTask::SlimprotoSession, // STMd / STMu
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
            RingTask::AudioFetch,
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
