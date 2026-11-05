//! Louder-ESP32-Plus Application Entrypoint
//!
//! Initializes board hardware profiles, starts audio streaming pipeline,
//! and orchestrates SlimProto network connection with TAS5805M/TAS5825M
//! digital amplifier.
#![cfg_attr(target_arch = "xtensa", no_std)]
#![cfg_attr(target_arch = "xtensa", no_main)]
#![cfg_attr(
    all(target_arch = "xtensa", not(feature = "esp-rtos-core1")),
    feature(asm_experimental_arch)
)]
#![cfg_attr(not(target_arch = "xtensa"), forbid(unsafe_code))]
#![cfg_attr(target_arch = "xtensa", deny(unsafe_code))]
#![allow(unknown_lints)]

#[cfg(not(target_arch = "xtensa"))]
use embedded_hal::i2c::I2c;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::board::first_boot_defaults;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::board::{pins, BoardModel};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::BitDepth;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::LoopbackI2sDma;
use louder_esp32_app::hal::{EspI2sSink, UartPort};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::io_traits::MockBackend;
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::AudioOutputSink;
#[cfg(any(target_arch = "xtensa", test))]
use louder_esp32_app::AudioSource;
use louder_esp32_app::{AmpManager, CydUartBridge, PlayerCoordinator};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::{
    CydMessage, HttpAudioStream, LoopbackUartPort, MemConfigStore, MemFlashPartition,
    NvsFlashBackend, PlayState, SlimprotoClient, TargetI2cBus,
};
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_config::AudioConfig;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_decoders::PcmDecoder;

#[allow(dead_code, clippy::duplicated_attributes)]
mod console;
#[allow(dead_code, clippy::duplicated_attributes)]
mod controls;
#[allow(dead_code, clippy::duplicated_attributes)]
mod dac_platform;
#[allow(dead_code, clippy::duplicated_attributes)]
mod display_gds;
#[allow(dead_code, clippy::duplicated_attributes)]
mod eth_w5500;
#[allow(dead_code, clippy::duplicated_attributes)]
mod power_boards;
#[cfg_attr(target_arch = "xtensa", allow(dead_code))]
mod tasks;

/// SqueezeESP32 HELO capability string broadcasted to LMS server during
/// SlimProto handshake (`WO-155`).
pub const BASEMENT_HELO_CAPABILITIES: &str = concat!(
    "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,",
    "HasDigitalOut=1,MaxSampleRate=96000,flc,pcm,wav,aif,mp3,aac,ogg,alc,ops"
);

/// Encodes ESP32 I2S0_CLKM_CONF_REG fractional divider fields
/// (ESP32 TRM Reg 12.6, CODE-MAIN-001):
/// - bits [7:0] = div_num (integral divider)
/// - bits [13:8] = div_b (fractional numerator)
/// - bits [19:14] = div_a (fractional denominator)
#[must_use]
pub const fn encode_i2s0_clkm_conf(orig: u32, div_num: u32, div_b: u32, div_a: u32) -> u32 {
    let cfg = louder_esp32_app::clock_table::I2sClockCfg {
        clkm_div_num: div_num as u8,
        clkm_div_a: div_a as u8,
        clkm_div_b: div_b as u8,
        bck_div: 2,
        use_apll: false,
    };
    louder_esp32_app::backend_device::encode_clkm_conf(orig, &cfg)
}

/// Executes one integrated firmware tick across the CYD UART bridge,
/// audio pipeline, and digital amplifier health monitor (`HALNET-409`).
///
/// Host only: on the device the same steps now run as embassy phases
/// (`tasks.rs`: `cyd_bridge` command poll, `i2s_feed` DMA-first top-up,
/// `decode_pump` burst, `i2s_feed` final top-up).
#[cfg(not(target_arch = "xtensa"))]
pub fn run_firmware_tick<S, U, I>(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut StreamPipeline<'_, S>,
    cyd_bridge: &mut CydUartBridge<U>,
    amp: &mut AmpManager<I>,
) -> usize
where
    S: AudioOutputSink,
    U: UartPort,
    I: I2c,
{
    run_firmware_tick_with_network(coordinator, pipeline, cyd_bridge, amp, None, 0)
}

/// Host network I/O driven by [`run_firmware_tick_with_network`]: the
/// SlimProto control connection, the HTTP audio stream and the (simulated)
/// audio backend.
#[cfg(not(target_arch = "xtensa"))]
pub struct HostNet<'a> {
    pub client: &'a mut SlimprotoClient,
    pub stream: &'a mut HttpAudioStream,
    pub backend: &'a mut MockBackend,
}

/// Executes one integrated firmware tick with SlimProto network integration,
/// dynamic sample rate synchronization, DAC amplifier pause, and hardware
/// fault recovery (`RUST-19`).
#[cfg(not(target_arch = "xtensa"))]
pub fn run_firmware_tick_with_network<S, U, I>(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut StreamPipeline<'_, S>,
    cyd_bridge: &mut CydUartBridge<U>,
    amp: &mut AmpManager<I>,
    mut net: Option<HostNet<'_>>,
    now_ms: u32,
) -> usize
where
    S: AudioOutputSink,
    U: UartPort,
    I: I2c,
{
    if let Ok(Some(cmd)) = cyd_bridge.poll_command() {
        // The host backend when there is one, else a stand-in with no hardware
        // volume, so the level goes into the software gain (C output_embedded.c).
        let mut no_hw = MockBackend {
            hw_volume: false,
            ..MockBackend::default()
        };
        let backend: &mut MockBackend = match &mut net {
            Some(net) => net.backend,
            None => &mut no_hw,
        };
        if let Some(reply) = coordinator.handle_cyd_command(cmd, backend, pipeline) {
            let _ = cyd_bridge.flush_outbound(&reply);
        }
    }

    let frames = if let Some(net) = net {
        let _ = coordinator.tick(net.client, net.stream, net.backend, pipeline, now_ms);
        while let Some(update) = coordinator.take_cyd_message() {
            let _ = cyd_bridge.flush_outbound(&update);
        }
        coordinator.last_tick_frames()
    } else {
        coordinator.process_audio_step(pipeline)
    };

    coordinator.current_sample_rate = pipeline.sample_rate;

    if !coordinator.dac_enabled {
        let _ = amp.stop_playback();
    }

    if let Ok(Some(fault)) = amp.check_faults() {
        if !fault.is_clean() {
            let _ = amp.recover_from_fault();
        }
    }

    frames
}

// Moved to the library (`amp`) so the shared coordinator can map `audg` onto
// the TAS volume registers through `AudioBackend::set_hw_volume` (Task 7);
// re-exported for existing callers.
pub use louder_esp32_app::amp::{
    lms_gain_to_hardware_volume_percent, lms_gain_to_tas5805m_vol_reg,
};

#[cfg(not(target_arch = "xtensa"))]
fn main() {
    let board = BoardModel::active();
    println!("========================================================");
    println!(
        "  Starting Pure-Rust Squeezelite Engine on {}",
        board.name()
    );
    println!("========================================================");
    println!("Hardware Pin Configuration:");
    println!(
        "  - I2S Master Audio: BCLK={}, LRCLK={}, DOUT={}",
        pins::I2S_BCLK,
        pins::I2S_LRCLK,
        pins::I2S_DOUT
    );
    println!(
        "  - I2C Amp Control:  SDA={}, SCL={}",
        pins::I2C_SDA,
        pins::I2C_SCL
    );
    println!(
        "  - TAS58xx Power:    PWDN={}, FAULT={}",
        pins::TAS58XX_PWDN,
        pins::TAS58XX_FAULT
    );
    println!("--------------------------------------------------------");

    // 1. Dual-Sector Flash NVS Backend & Configuration Storage (INFRA-801)
    let flash_partition = MemFlashPartition::new();
    let mut nvs_backend = NvsFlashBackend::new(flash_partition);
    let mut config_store = MemConfigStore::<16>::new();

    let mut audio_cfg = first_boot_defaults::audio_config();
    if let Ok(raw_name) = std::env::var("SL_PLAYER_NAME") {
        audio_cfg.set_player_name(&raw_name);
    }

    if let Ok(server_override) = std::env::var("SL_LMS_SERVER") {
        if let Ok(sock) = server_override.parse::<std::net::SocketAddr>() {
            if let std::net::IpAddr::V4(v4) = sock.ip() {
                audio_cfg.lms_server_ip = Some(v4.octets());
                audio_cfg.lms_server_port = Some(sock.port());
            }
        } else if let Ok(ip) = server_override.parse::<std::net::Ipv4Addr>() {
            audio_cfg.lms_server_ip = Some(ip.octets());
        }
    }
    audio_cfg
        .save_to(&mut config_store)
        .expect("Failed to serialize AudioConfig");

    let initial_seq = nvs_backend
        .commit_store(&config_store)
        .expect("Failed to commit dual-sector NVS");
    println!("Dual-Sector Flash NVS Backend:");
    println!("  - Active Sector Sequence: {}", initial_seq);
    println!("  - Key-Value Entries:      {}", config_store.len());

    // Verify persistence across dual-sector flash journal
    let mut verify_store = MemConfigStore::<16>::new();
    nvs_backend
        .load_store(&mut verify_store)
        .expect("Failed to reload from dual-sector flash");
    let verified_audio =
        AudioConfig::load_from(&verify_store).expect("Failed to deserialize AudioConfig");
    println!("  - Verified Node Name:     {}", verified_audio.player_name);
    println!("--------------------------------------------------------");

    // 2. Hardware/Target I2C Bus Adapter and TAS5805M Amplifier Lifecycle
    let i2c_bus = TargetI2cBus::new();
    let mut amp = AmpManager::for_active_board(i2c_bus);
    amp.initialize(BitDepth::Bits32.bits())
        .expect("Failed to initialize TAS5805M amplifier");
    amp.start_playback_via_hiz()
        .expect("Failed to start playback mode via Hi-Z on TAS5805M");
    println!("TAS5805M Digital Amplifier Subsystem:");
    println!("  - Mode:         {:?}", amp.mode());
    println!("  - Volume:       -24 dB (0x{:02X})", amp.current_volume());
    println!("  - Fault Status: {:?}", amp.check_faults().unwrap_or(None));
    println!("--------------------------------------------------------");

    // 3. Allocate heap-backed audio stream and output buffers
    // (APP-001: avoid FreeRTOS task stack overflow).
    let mut stream_storage = vec![0u8; 64 * 1024]; // 64 KB heap stream buffer
    let mut output_storage = vec![0u8; 64 * 1024]; // 64 KB heap output buffer

    // Initialize pure-Rust I2S DMA Audio Sink
    let i2s_driver = LoopbackI2sDma::new(44100);
    let sink = EspI2sSink::for_active_board(i2s_driver, 44100);
    let pcm_dec = PcmDecoder::new(16, 2, true);

    let mut pipeline = StreamPipeline::new(
        &mut stream_storage,
        &mut output_storage,
        ActiveDecoder::Pcm(pcm_dec),
        sink,
        44100,
    );

    // 4. Host demo MAC: a deterministic test MAC per board (the device uses
    // its efuse STA MAC).
    let mac = {
        let suffix = match board {
            BoardModel::LouderEsp32Wrover => 0x01,
            BoardModel::LouderEsp32PlusS3 => 0x02,
        };
        [0x24, 0x0A, 0xC4, 0x12, 0x34, suffix]
    };
    let mut coordinator = PlayerCoordinator::new(mac, board.name());
    coordinator.player_name.clear();
    let safe_name_len = audio_cfg
        .player_name
        .floor_char_boundary(64.min(audio_cfg.player_name.len()));
    let _ = coordinator
        .player_name
        .push_str(&audio_cfg.player_name[..safe_name_len]);

    println!("Player Coordinator initialized:");
    println!("  - State:       {:?}", coordinator.state);
    println!("  - Node Name:   {}", audio_cfg.player_name);
    println!(
        "  - MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    println!("  - Sample Rate: {} Hz", pipeline.sample_rate);
    println!(
        "  - Buffers:     {} bytes stream, {} bytes output",
        pipeline.stream_buffer_available(),
        pipeline.output_buffer_available()
    );
    println!("--------------------------------------------------------");

    // 5. Initialize Wi-Fi Manager and check persistent configuration
    let wifi = louder_esp32_app::wifi::WifiManager::with_store(config_store);
    println!("Wi-Fi Subsystem (Key-Value NVS):");
    println!("  - State:       {}", wifi.state());
    if !wifi.config.ssid.is_empty() {
        println!("  - Stored SSID: {}", wifi.config.ssid);
    } else {
        println!("  - Stored SSID: [None - Ready for CYD / Serial Provisioning]");
    }
    println!("--------------------------------------------------------");

    // 6. CYD UART Bridge initialization and initial frame transmission
    let uart_port = LoopbackUartPort::new();
    let mut cyd_bridge = CydUartBridge::new(uart_port);
    let tx_bytes = cyd_bridge
        .flush_outbound(&CydMessage::PlaybackState(PlayState::Stopped))
        .expect("Failed to send initial playback state to CYD");
    println!("CYD Display UART Bridge:");
    println!("  - Sent Initial Frame: STATE=S ({} bytes)", tx_bytes);
    println!("--------------------------------------------------------");
    println!("Ready for LMS auto-discovery on UDP 3483...");

    // 7. Optional Live LMS Network Connection & Firmware Tick Execution Loop
    let lms_ip = std::net::Ipv4Addr::from(
        audio_cfg
            .lms_server_ip
            .unwrap_or(first_boot_defaults::LMS_SERVER_IP),
    );
    let lms_port = audio_cfg
        .lms_server_port
        .unwrap_or(first_boot_defaults::LMS_SERVER_PORT);
    let lms_addr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(lms_ip, lms_port));

    let should_connect =
        std::env::var("SL_LMS_CONNECT").as_deref() == Ok("1") || cfg!(target_arch = "xtensa");
    let mut client = if should_connect {
        println!("Connecting SlimProto client to {}...", lms_addr);
        match SlimprotoClient::connect_with_options(
            lms_addr,
            mac,
            audio_cfg.player_name.as_str(),
            false,
            0,
        ) {
            Ok(c) => {
                println!(
                    "SlimProto client successfully connected to {} for player '{}'",
                    lms_addr, audio_cfg.player_name
                );
                Some(c)
            }
            Err(e) => {
                if std::env::var("SL_LMS_CONNECT").as_deref() == Ok("1") {
                    panic!(
                        "Failed to connect SlimProto client to {}: {:?}",
                        lms_addr, e
                    );
                } else {
                    eprintln!(
                        "Warning: Failed to connect SlimProto client to {}: {:?}",
                        lms_addr, e
                    );
                    None
                }
            }
        }
    } else {
        None
    };

    coordinator.lms_ip = lms_ip.octets();
    let mut http_stream = HttpAudioStream::new();
    let mut backend = MockBackend::new();
    let iterations = if client.is_some() { 5 } else { 16 };
    for tick in 0..iterations {
        let now_ms = (tick * 10) as u32;
        let net = client.as_mut().map(|client| HostNet {
            client,
            stream: &mut http_stream,
            backend: &mut backend,
        });
        let frames = run_firmware_tick_with_network(
            &mut coordinator,
            &mut pipeline,
            &mut cyd_bridge,
            &mut amp,
            net,
            now_ms,
        );
        if client.is_some() {
            println!(
                "  [Tick {:02}] frames={}, state={:?}",
                tick, frames, coordinator.state
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(target_arch = "xtensa")]
mod xtensa_hw {
    extern crate alloc;

    use super::*;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Waker};
    use embedded_hal::i2c::{ErrorType, I2c, Operation};
    use esp_backtrace as _;
    use esp_hal::clock::CpuClock;
    use esp_hal::efuse::{interface_mac_address, InterfaceMacAddress};
    use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
    use esp_hal::i2c::master::{Config as I2cConfig, I2c as HalI2c};
    use esp_hal::i2s::master::{Channels, DataFormat, I2s, TdmConfig};
    use esp_hal::rng::Rng;
    use esp_hal::system::{CpuControl, Stack};
    use esp_hal::time::{Duration, Instant, Rate};
    use esp_hal::timer::timg::{MwdtStage, TimerGroup, Wdt};
    use esp_hal::uart::{Config as UartConfig, Uart};
    use esp_println::println;
    use esp_radio::wifi::sta::StationConfig;
    use esp_radio::wifi::{
        AuthenticationMethodConfig, Config as WifiModeConfig, ControllerConfig,
        Interface as WifiInterface, PowerSaveMode, WifiController, WifiRxToken, WifiTxToken,
    };
    use esp_storage::FlashStorage;
    use louder_esp32_app::backend_device::{
        program_i2s_clock, Esp32AudioBackend, I2sClockRegisters, I2S0_CLKM_CONF_ADDR,
        I2S0_SAMPLE_RATE_CONF_ADDR,
    };
    use louder_esp32_app::board::first_boot_defaults;
    use louder_esp32_app::bt_a2dp::BtA2dpSink;
    use louder_esp32_app::config_handle::{
        resolve_boot_audio, resolve_boot_wifi, ConfigHandle, ConfigSource, ConfigWriter,
        FlashTargetPolicy, JournalSector, JournalSlot, RamJournal,
    };
    use louder_esp32_app::hal::I2sDmaDriver;
    use louder_esp32_app::http_control::{
        handle_http_request_streaming, DeadManLeaseWatchdog, HttpControlAction, HttpStatusSnapshot,
        LeaseHealth,
    };
    use louder_esp32_app::io_traits::{NetAddr, SlimTransport};
    use louder_esp32_app::net_device::{
        HttpStreamState, SlimLinkState, SlimReconnect, SmoltcpSlimTransport, SmoltcpStreamSource,
        TcpPhase, TcpSocketOps,
    };
    use louder_esp32_app::ota_safety::{
        load_nvs_wifi_credentials_split_12kb, resolve_wifi_credentials, verify_otadata_erased,
        OtaSafetyError, OtaSelectEntry, WifiStaCredentials, OTADATA_PARTITION_END,
        OTADATA_PARTITION_OFFSET, OTADATA_SECTOR_1_OFFSET, SETTINGS_JOURNAL_OFFSET,
    };
    use louder_esp32_app::PlayerState;
    use smoltcp::iface::{
        Config as SmolIfaceConfig, Context as SmolContext, Interface as SmolInterface,
        SocketHandle, SocketSet, SocketStorage,
    };
    use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
    use smoltcp::socket::{dhcpv4, tcp, udp, AnySocket};
    use smoltcp::time::{Duration as SmolDuration, Instant as SmolInstant};
    use smoltcp::wire::{
        EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address,
    };
    use squeezelite_audio_core::output::OutputError;
    use squeezelite_audio_core::EqPreset;
    use squeezelite_config::{FlashJournal, JournalView};
    use squeezelite_cspot::spirc::{SpircEvent, SpircFrame, SpircStateMachine};
    use squeezelite_cspot::zeroconf::{SpotifyMdnsBuilder, SpotifyMdnsConfig};
    use squeezelite_cyd_link::CydMessage;
    use squeezelite_decoders::PcmDecoder;
    use squeezelite_proto::SlimprotoBuilder;
    use squeezelite_raop::crypto::airport_express_key;
    use squeezelite_raop::mdns::{MdnsBuilder, MdnsConfig};
    use squeezelite_raop::rtp::RtpAudioReceiver;
    use squeezelite_raop::rtsp::{RtspMethod, RtspRequest, RtspServer, ServerState};
    use squeezelite_tas58xx::{registers, VOL_UNITY_0DB};

    // Mandatory Squeezelite-ESP32 OTA binary descriptor (`squeezelite-ota.c`).
    // Sets `project_name = "Squeezelite-ESP32"` and
    // `version = "I2S-4MFlash-Rust"` at offset 0x20.
    esp_bootloader_esp_idf::esp_app_desc!(
        "I2S-4MFlash-Rust",
        "Squeezelite-ESP32",
        "23:59:00",
        "2026-10-21",
        "4.3.0",
        esp_bootloader_esp_idf::MMU_PAGE_SIZE,
        0,
        u16::MAX,
        0
    );

    // Gate 2: Custom panic halt hook that resets immediately into factory
    // recovery (`0x10000`) instead of spinning in `loop {}`.
    #[allow(unsafe_code)]
    // JUSTIFICATION: Override standard panic halt hook with factory reset
    // into recovery.
    // SAFETY: Symbol exported with exact C/Rust ABI for runtime linkage.
    #[unsafe(no_mangle)]
    pub extern "Rust" fn custom_halt() -> ! {
        println!("[ANTI-BRICK GATE 2] Panic halted -> reset to recovery (0x10000)");
        esp_hal::system::software_reset()
    }

    static HW_ACK_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    /// Per-operation NACK counters (`/status.json` `i2c_nack_by_op`, spec
    /// §4.8). Counting only: each replaces the single total counter's
    /// increment at the same place, so the bus behaviour is unchanged and the
    /// total (`i2c_nacks`) is their sum.
    static HW_NACK_READ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static HW_NACK_WRITE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static HW_NACK_WRITE_READ: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);

    pub struct SmolDevice<'a> {
        pub driver: &'a mut WifiInterface,
    }

    pub struct SmolRxToken(pub WifiRxToken);
    pub struct SmolTxToken(pub WifiTxToken);

    /// Matches `esp-radio`'s `ESP_RADIO_CONFIG_WIFI_MTU = 1492`
    /// (`src/wifi/mod.rs:117`).
    const WIFI_MAX_FRAME_LEN: usize = 1492;

    impl RxToken for SmolRxToken {
        fn consume<R, F>(self, f: F) -> R
        where
            F: FnOnce(&[u8]) -> R,
        {
            self.0.consume_token(|buf| f(&*buf))
        }
    }

    impl TxToken for SmolTxToken {
        fn consume<R, F>(self, len: usize, f: F) -> R
        where
            F: FnOnce(&mut [u8]) -> R,
        {
            self.0.consume_token(len.min(WIFI_MAX_FRAME_LEN), f)
        }
    }

    impl Device for SmolDevice<'_> {
        type RxToken<'b>
            = SmolRxToken
        where
            Self: 'b;
        type TxToken<'b>
            = SmolTxToken
        where
            Self: 'b;

        fn receive(
            &mut self,
            _timestamp: SmolInstant,
        ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
            let (rx, tx) = self.driver.receive()?;
            Some((SmolRxToken(rx), SmolTxToken(tx)))
        }

        fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
            let tx = self.driver.transmit()?;
            Some(SmolTxToken(tx))
        }

        fn capabilities(&self) -> DeviceCapabilities {
            let mut caps = DeviceCapabilities::default();
            caps.max_transmission_unit = WIFI_MAX_FRAME_LEN;
            caps.medium = Medium::Ethernet;
            caps
        }
    }

    pub struct Esp32I2cBus<'d> {
        i2c: HalI2c<'d, esp_hal::Blocking>,
        shadow_regs: [u8; 256],
        current_reg: u8,
    }

    impl<'d> Esp32I2cBus<'d> {
        pub fn new(i2c: HalI2c<'d, esp_hal::Blocking>) -> Self {
            Self {
                i2c,
                shadow_regs: [0u8; 256],
                current_reg: 0,
            }
        }

        pub fn hw_ack_count() -> u32 {
            HW_ACK_COUNT.load(core::sync::atomic::Ordering::Relaxed)
        }

        pub fn hw_nack_count() -> u32 {
            let by_op = Self::hw_nack_by_op();
            by_op
                .read
                .wrapping_add(by_op.write)
                .wrapping_add(by_op.write_read)
        }

        pub fn hw_nack_by_op() -> louder_esp32_app::http_control::I2cNackByOp {
            let load =
                |c: &core::sync::atomic::AtomicU32| c.load(core::sync::atomic::Ordering::Relaxed);
            louder_esp32_app::http_control::I2cNackByOp {
                read: load(&HW_NACK_READ),
                write: load(&HW_NACK_WRITE),
                write_read: load(&HW_NACK_WRITE_READ),
            }
        }
    }

    impl ErrorType for Esp32I2cBus<'_> {
        type Error = core::convert::Infallible;
    }

    impl I2c for Esp32I2cBus<'_> {
        fn read(&mut self, address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            match self.i2c.read(address, read) {
                Ok(()) => {
                    HW_ACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    for &byte in read.iter() {
                        self.shadow_regs[self.current_reg as usize] = byte;
                        self.current_reg = self.current_reg.wrapping_add(1);
                    }
                }
                Err(_) => {
                    HW_NACK_READ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    for byte in read.iter_mut() {
                        *byte = self.shadow_regs[self.current_reg as usize];
                        self.current_reg = self.current_reg.wrapping_add(1);
                    }
                }
            }
            Ok(())
        }

        fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
            if write.is_empty() {
                return Ok(());
            }

            self.current_reg = write[0];
            let mut reg = write[0] as usize;
            for &val in &write[1..] {
                if reg < 256 {
                    self.shadow_regs[reg] = val;
                    reg += 1;
                }
            }

            if write.len() > 1 && write[0] == registers::FAULT_CLEAR && (write[1] & 0x80) != 0 {
                self.shadow_regs[registers::CHAN_FAULT as usize] = 0;
                self.shadow_regs[registers::GLOBAL_FAULT1 as usize] = 0;
                self.shadow_regs[registers::GLOBAL_FAULT2 as usize] = 0;
                self.shadow_regs[registers::OTW as usize] = 0;
            }

            match self.i2c.write(address, write) {
                Ok(()) => {
                    HW_ACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                }
                Err(_) => {
                    HW_NACK_WRITE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                }
            }
            Ok(())
        }

        fn write_read(
            &mut self,
            address: u8,
            write: &[u8],
            read: &mut [u8],
        ) -> Result<(), Self::Error> {
            if !write.is_empty() {
                self.current_reg = write[0];
            }
            match self.i2c.write_read(address, write, read) {
                Ok(()) => {
                    HW_ACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    for &byte in read.iter() {
                        self.shadow_regs[self.current_reg as usize] = byte;
                        self.current_reg = self.current_reg.wrapping_add(1);
                    }
                }
                Err(_) => {
                    HW_NACK_WRITE_READ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    self.write(address, write)?;
                    self.read(address, read)?;
                }
            }
            Ok(())
        }

        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Self::Error> {
            for op in operations {
                match op {
                    Operation::Read(buf) => self.read(address, buf)?,
                    Operation::Write(buf) => self.write(address, buf)?,
                }
            }
            Ok(())
        }
    }

    pub struct Esp32Uart2Port<'d> {
        uart: Uart<'d, esp_hal::Blocking>,
    }

    impl<'d> Esp32Uart2Port<'d> {
        pub fn new(uart: Uart<'d, esp_hal::Blocking>) -> Self {
            Self { uart }
        }
    }

    impl UartPort for Esp32Uart2Port<'_> {
        fn read_rx(&mut self, buf: &mut [u8]) -> usize {
            let mut count = 0;
            while count < buf.len() && self.uart.read_ready() {
                let mut byte = [0u8; 1];
                if let Ok(n) = self.uart.read(&mut byte) {
                    if n > 0 {
                        buf[count] = byte[0];
                        count += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            count
        }

        fn write_tx(&mut self, data: &[u8]) -> usize {
            self.uart.write(data).unwrap_or(0)
        }
    }

    /// I2S0 clock registers (`backend_device` documents the layout), volatile
    /// access.
    pub(crate) struct DeviceClockRegs;

    #[allow(unsafe_code)]
    impl I2sClockRegisters for DeviceClockRegs {
        fn read_clkm_conf(&mut self) -> u32 {
            // JUSTIFICATION: Direct volatile read of ESP32 I2S0_CLKM_CONF_REG.
            // SAFETY: Fixed, always-mapped I2S0 register owned by this firmware.
            unsafe { core::ptr::read_volatile(I2S0_CLKM_CONF_ADDR as *const u32) }
        }
        fn write_clkm_conf(&mut self, value: u32) {
            // JUSTIFICATION: Direct volatile write of ESP32 I2S0_CLKM_CONF_REG.
            // SAFETY: Fixed, always-mapped I2S0 register owned by this firmware.
            unsafe { core::ptr::write_volatile(I2S0_CLKM_CONF_ADDR as *mut u32, value) }
        }
        fn read_sample_rate_conf(&mut self) -> u32 {
            // JUSTIFICATION: Direct volatile read of I2S0_SAMPLE_RATE_CONF_REG.
            // SAFETY: Fixed, always-mapped I2S0 register owned by this firmware.
            unsafe { core::ptr::read_volatile(I2S0_SAMPLE_RATE_CONF_ADDR as *const u32) }
        }
        fn write_sample_rate_conf(&mut self, value: u32) {
            // JUSTIFICATION: Direct volatile write of I2S0_SAMPLE_RATE_CONF_REG.
            // SAFETY: Fixed, always-mapped I2S0 register owned by this firmware.
            unsafe { core::ptr::write_volatile(I2S0_SAMPLE_RATE_CONF_ADDR as *mut u32, value) }
        }
    }

    /// Size of the I2S TX DMA stream buffer (`DMA_TX_BUF` in
    /// [`init_i2s_pipeline`]).
    const I2S_DMA_BUFFER_BYTES: usize = 16384;

    /// Stereo frames queued in the I2S DMA buffer, updated by the driver after
    /// every push (`AudioBackend::dma_outstanding_frames`).
    pub(crate) static DMA_OUTSTANDING_FRAMES: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);

    fn note_dma_fill(available_bytes: usize) {
        let queued = I2S_DMA_BUFFER_BYTES.saturating_sub(available_bytes) / 8;
        DMA_OUTSTANDING_FRAMES.store(queued as u32, core::sync::atomic::Ordering::Relaxed);
    }

    /// Hardware I2S0 + DMA_I2S0 audio output driver (`BCK=GPIO26`,
    /// `WS=GPIO25`, `DOUT=GPIO22`).
    pub struct Esp32HardwareI2sDma<'d> {
        transfer: Option<
            esp_hal::i2s::master::I2sTxDmaTransfer<
                'd,
                esp_hal::Blocking,
                esp_hal::dma::DmaTxStreamBuf,
            >,
        >,
        sample_rate_hz: u32,
        tx_enabled: bool,
    }

    impl<'d> Esp32HardwareI2sDma<'d> {
        pub fn new(
            transfer: esp_hal::i2s::master::I2sTxDmaTransfer<
                'd,
                esp_hal::Blocking,
                esp_hal::dma::DmaTxStreamBuf,
            >,
            sample_rate_hz: u32,
        ) -> Self {
            let mut driver = Self {
                transfer: Some(transfer),
                sample_rate_hz: 0,
                tx_enabled: true,
            };
            let _ = driver.set_clock_rate(sample_rate_hz);
            driver
        }

        #[inline(never)]
        pub fn prefill_silence(buf: &mut esp_hal::dma::DmaTxStreamBuf) {
            let silence = [0u8; 2048];
            for _ in 0..4 {
                let _ = buf.push(&silence);
            }
        }

        #[inline(never)]
        pub fn ensure_running(&mut self) {
            let needs_restart = self.transfer.as_ref().map(|t| t.is_done()).unwrap_or(true);
            if needs_restart {
                if let Some(transfer) = self.transfer.take() {
                    let (i2s_tx, buf) = transfer.stop();
                    let (descriptors, mut buffer) = buf.split();
                    buffer.fill(0);
                    if let Ok(mut new_buf) = esp_hal::dma::DmaTxStreamBuf::new(descriptors, buffer)
                    {
                        Self::prefill_silence(&mut new_buf);
                        match i2s_tx.write(new_buf) {
                            Ok(new_transfer) => {
                                self.transfer = Some(new_transfer);
                                let rate = self.sample_rate_hz;
                                self.sample_rate_hz = 0;
                                let _ = self.set_clock_rate(rate);
                            }
                            Err((_, fallback_tx, fallback_buf)) => {
                                // Preserve self.i2s_tx across transient DMA errors
                                if let Ok(fallback_transfer) = fallback_tx.write(fallback_buf) {
                                    self.transfer = Some(fallback_transfer);
                                }
                            }
                        }
                    }
                }
            }
        }

        pub fn pump_silence_if_needed(&mut self, is_playing: bool, has_buffered_audio: bool) {
            self.ensure_running();
            if is_playing && has_buffered_audio {
                return;
            }
            if let Some(ref mut transfer) = self.transfer {
                let threshold = 4096;
                let avail = transfer.available_bytes();
                let avail_aligned = avail & !7;
                if avail_aligned >= threshold {
                    let silence = [0u8; 512];
                    let pad = avail_aligned % 1024;
                    let pad_aligned = pad & !7;
                    if pad_aligned > 0 {
                        let mut rem = pad_aligned;
                        while rem > 0 {
                            let c = (rem.min(silence.len())) & !7;
                            if c == 0 {
                                break;
                            }
                            let n = transfer.push(&silence[..c]);
                            if n == 0 {
                                break;
                            }
                            rem = rem.saturating_sub(n & !7);
                            if n < c {
                                break;
                            }
                        }
                    }
                    while (transfer.available_bytes() & !7) >= threshold {
                        let avail_now = transfer.available_bytes() & !7;
                        let to_push = avail_now.min(silence.len()) & !7;
                        if to_push == 0 || transfer.push(&silence[..to_push]) == 0 {
                            break;
                        }
                    }
                }
                note_dma_fill(transfer.available_bytes());
            }
        }
    }

    impl I2sDmaDriver for Esp32HardwareI2sDma<'_> {
        #[allow(unsafe_code)]
        // JUSTIFICATION: Hot I2S DMA write loop placed in IRAM (.iram1) to
        // eliminate SPI flash wait states.
        // SAFETY: Placed in executable IRAM section with no side effects.
        #[unsafe(link_section = ".iram1")]
        fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError> {
            if data.is_empty() {
                return Ok(0);
            }
            self.ensure_running();

            let transfer = match self.transfer.as_mut() {
                Some(t) => t,
                None => return Ok(0),
            };

            if data.len() < 8 {
                let n = transfer.push(data);
                note_dma_fill(transfer.available_bytes());
                return Ok(n);
            }

            let valid_len = data.len() & !7;

            if !self.tx_enabled {
                let silence = [0u8; 512];
                let mut written = 0;
                while written < valid_len {
                    let avail_aligned = transfer.available_bytes() & !7;
                    if avail_aligned == 0 {
                        break;
                    }
                    let rem_bytes = (valid_len - written) & !7;
                    let chunk_len = silence.len().min(rem_bytes).min(avail_aligned) & !7;
                    if chunk_len == 0 {
                        break;
                    }
                    let n = transfer.push(&silence[..chunk_len]);
                    let aligned_n = n & !7;
                    written += aligned_n;
                    if n < chunk_len {
                        break;
                    }
                }
                return Ok(written);
            }

            let mut total_written = 0;
            let mut offset = 0;
            let mut scratch = [0u8; 512];

            while offset < valid_len {
                let avail_aligned = transfer.available_bytes() & !7;
                if avail_aligned == 0 {
                    break;
                }
                let rem_frames = (valid_len - offset) / 8;
                let frames_in_chunk = rem_frames.min(64).min(avail_aligned / 8);
                if frames_in_chunk == 0 {
                    break;
                }
                for f in 0..frames_in_chunk {
                    let src_idx = offset + f * 8;
                    let dst_idx = f * 8;
                    scratch[dst_idx..dst_idx + 4].copy_from_slice(&data[src_idx + 4..src_idx + 8]);
                    scratch[dst_idx + 4..dst_idx + 8].copy_from_slice(&data[src_idx..src_idx + 4]);
                }
                let chunk_bytes = frames_in_chunk * 8;
                let n = transfer.push(&scratch[..chunk_bytes]);
                let aligned_n = n & !7;
                total_written += aligned_n;
                offset += aligned_n;
                if n < chunk_bytes {
                    break;
                }
            }
            note_dma_fill(transfer.available_bytes());
            Ok(total_written)
        }

        /// Programs the I2S0 dividers AND the bit-clock divider from the
        /// verified clock table (`backend_device::program_i2s_clock`); a rate
        /// missing from the table is rejected without touching the hardware.
        fn set_clock_rate(&mut self, sample_rate_hz: u32) -> Result<(), OutputError> {
            if self.sample_rate_hz != sample_rate_hz {
                program_i2s_clock(&mut DeviceClockRegs, sample_rate_hz)
                    .map_err(|_| OutputError::UnsupportedSampleRate(sample_rate_hz))?;
                self.sample_rate_hz = sample_rate_hz;
            }
            Ok(())
        }

        fn set_tx_enabled(&mut self, enabled: bool) {
            self.tx_enabled = enabled;
        }
    }

    pub(crate) fn now_millis() -> u64 {
        Instant::now().duration_since_epoch().as_millis()
    }

    /// Boot resolution step 2 (Wi-Fi): the C firmware's NVS at 0x9000,
    /// read-only. Boot-time, single-core read (APP_CPU is parked and esp-radio
    /// not started yet).
    #[inline(never)]
    fn load_nvs_wifi_credentials(
        flash: &mut FlashStorage<'_>,
        sta_mac: &[u8; 6],
        nvs_buf: &mut [u8; 12_288],
    ) -> Result<WifiStaCredentials, OtaSafetyError> {
        load_nvs_wifi_credentials_split_12kb(
            |offset, buf| {
                let _ = flash.read_nor(offset, buf);
            },
            sta_mac,
            nvs_buf,
            None,
            None,
        )
    }

    /// Boot resolution of the Wi-Fi STA credentials, Phase 0/1: the C
    /// firmware's NVS, else the compile-time first-boot credentials; none at
    /// all resets into recovery. The result lives in `.dram2_uninit` for the
    /// life of the image (`/status.json` shows the SSID), keeping it off the
    /// boot frame and out of `.bss` (which would shrink `.stack`).
    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_wifi_credentials(
        flash: &mut FlashStorage<'_>,
        nvs_scratch: &mut [u8; 12_288],
        sta_mac: &[u8; 6],
    ) -> &'static WifiStaCredentials {
        // JUSTIFICATION: Place static Wi-Fi credentials in DRAM2 uninitialized section.
        // SAFETY: Accessed only during single-core initialization before task spawning.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut WIFI_STA_CREDS: core::mem::MaybeUninit<WifiStaCredentials> =
            core::mem::MaybeUninit::uninit();
        // Journal precedence for Wi-Fi returns in Phase 3 with Rust provisioning.
        let resolved = resolve_boot_wifi(
            || load_nvs_wifi_credentials(flash, sta_mac, nvs_scratch).ok(),
            || default_wifi_credentials(sta_mac),
        );
        let Some((creds, source)) = resolved else {
            println!("[ANTI-BRICK] No Wi-Fi credentials found -> resetting to recovery (0x10000)");
            esp_hal::system::software_reset();
        };
        let source = match source {
            ConfigSource::Journal => "settings journal",
            ConfigSource::Nvs => "NVS 0x9000",
            ConfigSource::Default => "first-boot default",
        };
        println!(
            "[CONFIG] Wi-Fi STA SSID='{}' (pass_len={}) from {} for MAC {:02x?}",
            creds.ssid.as_str(),
            creds.password.len(),
            source,
            sta_mac
        );
        // JUSTIFICATION: Store resolved Wi-Fi credentials in DRAM2.
        // SAFETY: Called once at bring-up before any concurrent access.
        unsafe { &*(*core::ptr::addr_of_mut!(WIFI_STA_CREDS)).write(creds) }
    }

    /// Boot resolution step 3 (Wi-Fi): the compile-time first-boot
    /// credentials, only when a passphrase was compiled in (`SL_WIFI_PASS`,
    /// plain or `enc:`), with `SL_WIFI_SSID` or [`first_boot_defaults::WIFI_SSID`].
    #[inline(never)]
    fn default_wifi_credentials(sta_mac: &[u8; 6]) -> Option<WifiStaCredentials> {
        let ssid = option_env!("SL_WIFI_SSID").unwrap_or(first_boot_defaults::WIFI_SSID);
        resolve_wifi_credentials(&[], sta_mac, Some(ssid), option_env!("SL_WIFI_PASS")).ok()
    }

    static MP3_DEC_TAKEN: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    /// The firmware's only MP3 decoder (no heap MP3 decoder without
    /// `std-codecs`). Taken once at bring-up and parked in the pipeline's MP3
    /// slot (`StreamPipeline::mp3_spare`), from where the coordinator uses and
    /// returns it for every MP3 track.
    #[allow(unsafe_code)]
    fn take_static_mp3_decoder() -> &'static mut squeezelite_decoders::StaticMp3Decoder {
        assert!(
            !MP3_DEC_TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel),
            "StaticMp3Decoder already taken"
        );
        // JUSTIFICATION: Place static MP3 decoder in DRAM2 uninitialized section.
        // SAFETY: Guarded by atomic MP3_DEC_TAKEN ensuring exclusive access.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_MP3_DEC: core::mem::MaybeUninit<squeezelite_decoders::StaticMp3Decoder> =
            core::mem::MaybeUninit::uninit();
        // JUSTIFICATION: Access to global static MP3 decoder singleton in DRAM2.
        // SAFETY: Exclusive singleton access is guaranteed by MP3_DEC_TAKEN.
        unsafe {
            let dec = (*core::ptr::addr_of_mut!(STATIC_MP3_DEC))
                .write(squeezelite_decoders::StaticMp3Decoder::new());
            dec.reset_in_place();
            dec
        }
    }

    struct FirmwareBssBuffers {
        ring_storage: [u8; 4096],
        http_rx_buf: [u8; 1024],
        slim_rx_buf: [u8; 2048],
        slim_tx_buf: [u8; 2048],
        audio_rx_buf: [u8; 4096],
        audio_tx_buf: [u8; 512],
    }

    impl FirmwareBssBuffers {
        const fn new() -> Self {
            Self {
                ring_storage: [0u8; 4096],
                http_rx_buf: [0u8; 1024],
                slim_rx_buf: [0u8; 2048],
                slim_tx_buf: [0u8; 2048],
                audio_rx_buf: [0u8; 4096],
                audio_tx_buf: [0u8; 512],
            }
        }
    }

    #[allow(unsafe_code)]
    // JUSTIFICATION: Dedicated execution stack for Core 1 (APP_CPU) in DRAM2.
    // SAFETY: Accessed exclusively by Core 1 runner; stack bounds enforced by
    // hardware and linker.
    #[unsafe(link_section = ".dram2_uninit")]
    static mut APP_CORE_STACK: Stack<4096> = Stack::new();
    #[allow(unsafe_code)]
    // JUSTIFICATION: Dedicated static buffer pool for Bluetooth controller heap
    // in DRAM2.
    // SAFETY: Managed as singleton heap storage by single Bluetooth task context.
    #[unsafe(link_section = ".dram2_uninit")]
    static mut STATIC_BT_HEAP: core::mem::MaybeUninit<[u8; 14 * 1024]> =
        core::mem::MaybeUninit::uninit();
    #[allow(unsafe_code)]
    // JUSTIFICATION: Static allocation for Bluetooth A2DP sink state machine in
    // DRAM2.
    // SAFETY: Exclusive singleton access initialized once before starting
    // Bluetooth runner.
    #[unsafe(link_section = ".dram2_uninit")]
    static mut STATIC_BT_SINK: core::mem::MaybeUninit<BtA2dpSink> =
        core::mem::MaybeUninit::uninit();

    pub(crate) struct Esp32VhciTransport<'d> {
        pub(crate) connector: esp_radio::ble::controller::BleConnector<'d>,
    }

    impl<'d> louder_esp32_app::bt_a2dp::HciTransport for Esp32VhciTransport<'d> {
        fn can_read(&self) -> bool {
            esp_radio::ble::have_hci_read_data()
        }

        fn read_packet(&mut self, buf: &mut [u8]) -> usize {
            self.connector.next(buf).unwrap_or(0)
        }

        fn write_packet(&mut self, data: &[u8]) -> bool {
            matches!(self.connector.write(data), Ok(n) if n == data.len())
        }
    }
    pub(crate) static CORE1_HEARTBEAT: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);
    pub(crate) static CORE1_PARK_REQ: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    pub(crate) static CORE1_PARKED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    #[cfg(not(feature = "esp-rtos-core1"))]
    #[esp_hal::ram]
    fn app_core_loop() {
        loop {
            if CORE1_PARK_REQ.load(core::sync::atomic::Ordering::Acquire) {
                CORE1_PARKED.store(true, core::sync::atomic::Ordering::Release);
                while CORE1_PARK_REQ.load(core::sync::atomic::Ordering::Acquire) {
                    #[allow(unsafe_code)]
                    // JUSTIFICATION: Cooperative park wait loop for Core 1 during flash
                    // erase and write.
                    // SAFETY: Nop instruction has no memory or register side effects.
                    unsafe {
                        core::arch::asm!("nop");
                    }
                }
                CORE1_PARKED.store(false, core::sync::atomic::Ordering::Release);
            }
            CORE1_HEARTBEAT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            for _ in 0..10_000 {
                #[allow(unsafe_code)]
                // JUSTIFICATION: Inline assembly required for nop delay loop.
                // SAFETY: nop instruction has no memory or register side effects.
                unsafe {
                    core::arch::asm!("nop");
                }
            }
        }
    }

    #[esp_hal::ram]
    #[allow(unsafe_code)]
    // JUSTIFICATION: Transmit a single 8-bit PSRAM mode command over SPI1 CS1.
    // SAFETY: Runs in IRAM while SPI0/SPI1 are idle and restores SPI1 state.
    unsafe fn psram_send_cmd_spi1(cmd_byte: u32, qpi_mode: bool) {
        const SPI0_EXT2_REG: *const u32 = 0x3FF4_30F8 as *const u32;
        const SPI1_CMD_REG: *mut u32 = 0x3FF4_2000 as *mut u32;
        const SPI1_ADDR_REG: *mut u32 = 0x3FF4_2004 as *mut u32;
        const SPI1_CTRL_REG: *mut u32 = 0x3FF4_2008 as *mut u32;
        const SPI1_USER_REG: *mut u32 = 0x3FF4_201C as *mut u32;
        const SPI1_USER1_REG: *mut u32 = 0x3FF4_2020 as *mut u32;
        const SPI1_USER2_REG: *mut u32 = 0x3FF4_2024 as *mut u32;
        const SPI1_MOSI_DLEN_REG: *mut u32 = 0x3FF4_2028 as *mut u32;
        const SPI1_MISO_DLEN_REG: *mut u32 = 0x3FF4_202C as *mut u32;
        const SPI1_PIN_REG: *mut u32 = 0x3FF4_2034 as *mut u32;
        const SPI1_EXT2_REG: *const u32 = 0x3FF4_20F8 as *const u32;
        const DPORT_HOST_INF_SEL_REG: *mut u32 = 0x3FF0_00BC as *mut u32;
        const FREAD_MASK: u32 = (1 << 14) | (1 << 20) | (1 << 23) | (1 << 24);
        const FWRITE_MASK: u32 = 0xF << 12;

        // JUSTIFICATION: Direct volatile MMIO access to SPI1 and DPORT registers.
        // SAFETY: All register addresses are verified ESP32 hardware peripherals.
        unsafe {
            while (core::ptr::read_volatile(SPI1_CMD_REG) & (1 << 18)) != 0 {}

            let backup_usr = core::ptr::read_volatile(SPI1_USER_REG);
            let backup_usr1 = core::ptr::read_volatile(SPI1_USER1_REG);
            let backup_usr2 = core::ptr::read_volatile(SPI1_USER2_REG);

            let mut usr = backup_usr & !((1 << 31) | (1 << 29) | (1 << 28) | (1 << 27));
            usr |= 1 << 30;
            let usr1 = (backup_usr1 & !((0x3F << 26) | 0xFF)) | (7 << 26);
            let usr2 = backup_usr2 & !(0xF << 28);

            core::ptr::write_volatile(SPI1_USER_REG, usr);
            core::ptr::write_volatile(SPI1_USER1_REG, usr1);
            core::ptr::write_volatile(SPI1_USER2_REG, usr2);
            core::ptr::write_volatile(SPI1_ADDR_REG, cmd_byte << 24);
            core::ptr::write_volatile(SPI1_MOSI_DLEN_REG, 0);
            core::ptr::write_volatile(SPI1_MISO_DLEN_REG, 0);

            let pin_orig = core::ptr::read_volatile(SPI1_PIN_REG);
            core::ptr::write_volatile(SPI1_PIN_REG, (pin_orig & !(1 << 1)) | (1 << 0));

            let ctrl_orig = core::ptr::read_volatile(SPI1_CTRL_REG);
            let rd_mode_backup = ctrl_orig & FREAD_MASK;

            let mut usr_mode = core::ptr::read_volatile(SPI1_USER_REG) & !FWRITE_MASK;
            let mut ctrl_mode = ctrl_orig & !FREAD_MASK;
            if qpi_mode {
                usr_mode |= 1 << 15;
                ctrl_mode |= 1 << 24;
            }
            core::ptr::write_volatile(SPI1_USER_REG, usr_mode);
            core::ptr::write_volatile(SPI1_CTRL_REG, ctrl_mode);

            while (core::ptr::read_volatile(SPI0_EXT2_REG) & 0x7) != 0 {}
            while (core::ptr::read_volatile(SPI1_EXT2_REG) & 0x7) != 0 {}

            let inf_sel = core::ptr::read_volatile(DPORT_HOST_INF_SEL_REG);
            core::ptr::write_volatile(DPORT_HOST_INF_SEL_REG, inf_sel | (1 << 14));

            core::ptr::write_volatile(
                SPI1_CMD_REG,
                core::ptr::read_volatile(SPI1_CMD_REG) | (1 << 18),
            );
            while (core::ptr::read_volatile(SPI1_CMD_REG) & (1 << 18)) != 0 {}

            core::ptr::write_volatile(
                DPORT_HOST_INF_SEL_REG,
                core::ptr::read_volatile(DPORT_HOST_INF_SEL_REG) & !(1 << 14),
            );

            let ctrl_after = core::ptr::read_volatile(SPI1_CTRL_REG) & !FREAD_MASK;
            core::ptr::write_volatile(SPI1_CTRL_REG, ctrl_after | rd_mode_backup);
            core::ptr::write_volatile(SPI1_PIN_REG, (pin_orig | (1 << 1)) & !(1 << 0));

            core::ptr::write_volatile(SPI1_USER_REG, backup_usr);
            core::ptr::write_volatile(SPI1_USER1_REG, backup_usr1);
            core::ptr::write_volatile(SPI1_USER2_REG, backup_usr2);
        }
    }

    #[esp_hal::ram]
    #[allow(unsafe_code)]
    // JUSTIFICATION: Initialize and MMU-map 4 MB ESP32 PSRAM 100% from IRAM.
    // SAFETY: Avoids all XIP flash instruction/rodata fetches during SPI0 init.
    unsafe fn init_and_map_psram_4mb() -> usize {
        extern "C" {
            fn esp_rom_gpio_connect_out_signal(
                gpio_num: u32,
                signal_idx: u32,
                out_inv: bool,
                oen_inv: bool,
            );
            fn esp_rom_gpio_connect_in_signal(gpio_num: u32, signal_idx: u32, inv: bool);
            fn esp_rom_spiflash_config_clk(freqdiv: u8, spi: u8) -> i32;
            fn cache_sram_mmu_set_rom(
                cpu_no: u32,
                pid: u32,
                vaddr: u32,
                paddr: u32,
                psize: u32,
                num: u32,
            ) -> i32;
            static mut g_rom_spiflash_dummy_len_plus: [u8; 2];
        }

        const SPI0_CTRL_REG: *mut u32 = 0x3FF4_3008 as *mut u32;
        const SPI0_CTRL2_REG: *mut u32 = 0x3FF4_3014 as *mut u32;
        const SPI0_USER_REG: *mut u32 = 0x3FF4_301C as *mut u32;
        const SPI0_USER1_REG: *mut u32 = 0x3FF4_3020 as *mut u32;
        const SPI0_PIN_REG: *mut u32 = 0x3FF4_3034 as *mut u32;
        const SPI0_CACHE_SCTRL_REG: *mut u32 = 0x3FF4_3054 as *mut u32;
        const SPI0_SRAM_DRD_CMD_REG: *mut u32 = 0x3FF4_305C as *mut u32;
        const SPI0_SRAM_DWR_CMD_REG: *mut u32 = 0x3FF4_3060 as *mut u32;
        const SPI0_EXT2_REG: *const u32 = 0x3FF4_30F8 as *const u32;
        const SPI0_EXT3_REG: *mut u32 = 0x3FF4_30FC as *mut u32;
        const SPI0_DATE_REG: *mut u32 = 0x3FF4_33FC as *mut u32;

        const SPI1_CMD_REG: *mut u32 = 0x3FF4_2000 as *mut u32;
        const SPI1_CTRL_REG: *mut u32 = 0x3FF4_2008 as *mut u32;
        const SPI1_CTRL2_REG: *mut u32 = 0x3FF4_2014 as *mut u32;
        const SPI1_USER_REG: *mut u32 = 0x3FF4_201C as *mut u32;
        const SPI1_USER1_REG: *mut u32 = 0x3FF4_2020 as *mut u32;
        const SPI1_PIN_REG: *mut u32 = 0x3FF4_2034 as *mut u32;
        const SPI1_SLAVE_REG: *mut u32 = 0x3FF4_2038 as *mut u32;
        const SPI1_W0_REG: *mut u32 = 0x3FF4_2080 as *mut u32;
        const SPI1_EXT2_REG: *const u32 = 0x3FF4_20F8 as *const u32;

        const DPORT_PRO_CACHE_CTRL_REG: *mut u32 = 0x3FF0_0040 as *mut u32;
        const DPORT_PRO_CACHE_CTRL1_REG: *mut u32 = 0x3FF0_0044 as *mut u32;
        const DPORT_APP_CACHE_CTRL_REG: *mut u32 = 0x3FF0_0058 as *mut u32;
        const DPORT_APP_CACHE_CTRL1_REG: *mut u32 = 0x3FF0_005C as *mut u32;

        const IO_MUX_GPIO16_REG: *mut u32 = 0x3FF4_904C as *mut u32;
        const IO_MUX_GPIO17_REG: *mut u32 = 0x3FF4_9050 as *mut u32;
        const IO_MUX_GPIO9_REG: *mut u32 = 0x3FF4_9054 as *mut u32;
        const IO_MUX_GPIO10_REG: *mut u32 = 0x3FF4_9058 as *mut u32;
        const IO_MUX_GPIO11_REG: *mut u32 = 0x3FF4_905C as *mut u32;
        const IO_MUX_GPIO6_REG: *mut u32 = 0x3FF4_9060 as *mut u32;
        const IO_MUX_GPIO7_REG: *mut u32 = 0x3FF4_9064 as *mut u32;
        const IO_MUX_GPIO8_REG: *mut u32 = 0x3FF4_9068 as *mut u32;

        const EXTMEM_ORIGIN: usize = 0x3F80_0000;
        const MAX_MEM_SIZE: usize = 4 * 1024 * 1024;

        // JUSTIFICATION: Configure SPI0/SPI1, GPIO matrix, IO_MUX, and SRAM MMU.
        // SAFETY: Runs on PRO_CPU in IRAM with APP_CPU parked and SPI buses idle.
        unsafe {
            while (core::ptr::read_volatile(SPI0_EXT2_REG) & 0x7) != 0 {}
            while (core::ptr::read_volatile(SPI1_EXT2_REG) & 0x7) != 0 {}
            while (core::ptr::read_volatile(SPI1_CMD_REG) & (1 << 18)) != 0 {}

            core::ptr::write_volatile(SPI0_EXT3_REG, 0x1);
            core::ptr::write_volatile(
                SPI1_USER_REG,
                core::ptr::read_volatile(SPI1_USER_REG) & !(1 << 23),
            );

            let slave = core::ptr::read_volatile(SPI1_SLAVE_REG);
            core::ptr::write_volatile(SPI1_SLAVE_REG, slave & !((1 << 9) | (1 << 30)));
            let pin1 = core::ptr::read_volatile(SPI1_PIN_REG);
            core::ptr::write_volatile(SPI1_PIN_REG, pin1 & !(1 << 29));
            let usr1_reg = core::ptr::read_volatile(SPI1_USER_REG);
            core::ptr::write_volatile(
                SPI1_USER_REG,
                (usr1_reg & !((1 << 7) | (1 << 0))) | (1 << 4) | (1 << 5),
            );
            let ctrl1_reg = core::ptr::read_volatile(SPI1_CTRL_REG);
            core::ptr::write_volatile(SPI1_CTRL_REG, ctrl1_reg & !((1 << 26) | (1 << 25)));
            core::ptr::write_volatile(SPI1_USER1_REG, 0);

            let mut w_idx: usize = 0;
            while w_idx < 16 {
                SPI1_W0_REG.add(w_idx).write_volatile(0);
                w_idx += 1;
            }
            core::ptr::write_volatile(
                SPI1_CTRL2_REG,
                core::ptr::read_volatile(SPI1_CTRL2_REG) & !0xFF,
            );

            esp_rom_gpio_connect_out_signal(17, 0, false, false);

            let rd_mode_reg = core::ptr::read_volatile(SPI0_CTRL_REG);
            let spi_cache_dummy: u32 = if (rd_mode_reg & (1 << 24)) != 0 {
                3
            } else if (rd_mode_reg & (1 << 23)) != 0 {
                let u1 = core::ptr::read_volatile(SPI0_USER1_REG);
                core::ptr::write_volatile(SPI0_USER1_REG, (u1 & !(0x3F << 26)) | (27 << 26));
                1
            } else {
                7
            };

            let dummy_ptr = core::ptr::addr_of_mut!(g_rom_spiflash_dummy_len_plus) as *mut u8;
            dummy_ptr.add(0).write_volatile(1);
            dummy_ptr.add(1).write_volatile(1);

            let u1_after = core::ptr::read_volatile(SPI0_USER1_REG);
            core::ptr::write_volatile(
                SPI0_USER1_REG,
                (u1_after & !0xFF) | ((spi_cache_dummy + 1) & 0xFF),
            );
            core::ptr::write_volatile(
                SPI0_USER_REG,
                core::ptr::read_volatile(SPI0_USER_REG) | (1 << 29),
            );

            let _ = esp_rom_spiflash_config_clk(2, 0);
            let _ = esp_rom_spiflash_config_clk(2, 1);

            let io6 = core::ptr::read_volatile(IO_MUX_GPIO6_REG);
            core::ptr::write_volatile(IO_MUX_GPIO6_REG, (io6 & !(0x3 << 10)) | (2 << 10));
            let io17 = core::ptr::read_volatile(IO_MUX_GPIO17_REG);
            core::ptr::write_volatile(IO_MUX_GPIO17_REG, (io17 & !(0x3 << 10)) | (2 << 10));

            esp_rom_gpio_connect_out_signal(11, 5, false, false);
            esp_rom_gpio_connect_out_signal(16, 6, false, false);
            esp_rom_gpio_connect_out_signal(7, 1, false, false);
            esp_rom_gpio_connect_in_signal(7, 1, false);
            esp_rom_gpio_connect_out_signal(8, 2, false, false);
            esp_rom_gpio_connect_in_signal(8, 2, false);
            esp_rom_gpio_connect_out_signal(10, 4, false, false);
            esp_rom_gpio_connect_in_signal(10, 4, false);
            esp_rom_gpio_connect_out_signal(9, 3, false, false);
            esp_rom_gpio_connect_in_signal(9, 3, false);

            let io6_fn = core::ptr::read_volatile(IO_MUX_GPIO6_REG);
            core::ptr::write_volatile(IO_MUX_GPIO6_REG, (io6_fn & !(0x7 << 12)) | (1 << 12));
            let io11_fn = core::ptr::read_volatile(IO_MUX_GPIO11_REG);
            core::ptr::write_volatile(IO_MUX_GPIO11_REG, (io11_fn & !(0x7 << 12)) | (2 << 12));
            let io16_fn = core::ptr::read_volatile(IO_MUX_GPIO16_REG);
            core::ptr::write_volatile(IO_MUX_GPIO16_REG, (io16_fn & !(0x7 << 12)) | (2 << 12));
            let io17_fn = core::ptr::read_volatile(IO_MUX_GPIO17_REG);
            core::ptr::write_volatile(IO_MUX_GPIO17_REG, (io17_fn & !(0x7 << 12)) | (2 << 12));

            let io7_fn = core::ptr::read_volatile(IO_MUX_GPIO7_REG);
            core::ptr::write_volatile(
                IO_MUX_GPIO7_REG,
                ((io7_fn & !(0x7 << 12)) | (2 << 12)) | (1 << 9),
            );
            let io8_fn = core::ptr::read_volatile(IO_MUX_GPIO8_REG);
            core::ptr::write_volatile(
                IO_MUX_GPIO8_REG,
                ((io8_fn & !(0x7 << 12)) | (2 << 12)) | (1 << 9),
            );
            let io9_fn = core::ptr::read_volatile(IO_MUX_GPIO9_REG);
            core::ptr::write_volatile(
                IO_MUX_GPIO9_REG,
                ((io9_fn & !(0x7 << 12)) | (2 << 12)) | (1 << 9),
            );
            let io10_fn = core::ptr::read_volatile(IO_MUX_GPIO10_REG);
            core::ptr::write_volatile(
                IO_MUX_GPIO10_REG,
                ((io10_fn & !(0x7 << 12)) | (2 << 12)) | (1 << 9),
            );

            esp_rom_gpio_connect_out_signal(28, 256, false, false);
            esp_rom_gpio_connect_out_signal(29, 256, false, false);
            esp_rom_gpio_connect_out_signal(17, 0, false, false);

            core::ptr::write_volatile(
                SPI1_USER_REG,
                core::ptr::read_volatile(SPI1_USER_REG) | (1 << 4) | (1 << 5),
            );
            core::ptr::write_volatile(
                SPI1_CTRL2_REG,
                core::ptr::read_volatile(SPI1_CTRL2_REG) & !0xFF,
            );
            core::ptr::write_volatile(
                SPI0_USER_REG,
                core::ptr::read_volatile(SPI0_USER_REG) | (1 << 4) | (1 << 5),
            );
            core::ptr::write_volatile(
                SPI0_CTRL2_REG,
                core::ptr::read_volatile(SPI0_CTRL2_REG) & !0xFF,
            );

            psram_send_cmd_spi1(0xF5, true);
            psram_send_cmd_spi1(0x35, false);

            core::ptr::write_volatile(
                SPI0_DATE_REG,
                core::ptr::read_volatile(SPI0_DATE_REG) & !((1 << 31) | (1 << 30)),
            );

            let mut sctrl = core::ptr::read_volatile(SPI0_CACHE_SCTRL_REG);
            sctrl &= !((1 << 1) | (0xFF << 14) | (0x3F << 22));
            sctrl |= (1 << 2) | (1 << 4) | (1 << 5) | (6 << 14) | (23 << 22) | (1 << 28);
            core::ptr::write_volatile(SPI0_CACHE_SCTRL_REG, sctrl);

            core::ptr::write_volatile(SPI0_SRAM_DRD_CMD_REG, (7 << 28) | 0xEB);
            core::ptr::write_volatile(SPI0_SRAM_DWR_CMD_REG, (7 << 28) | 0x38);

            core::ptr::write_volatile(
                DPORT_PRO_CACHE_CTRL_REG,
                core::ptr::read_volatile(DPORT_PRO_CACHE_CTRL_REG) & !((1 << 16) | (1 << 11)),
            );
            core::ptr::write_volatile(
                DPORT_APP_CACHE_CTRL_REG,
                core::ptr::read_volatile(DPORT_APP_CACHE_CTRL_REG) & !((1 << 16) | (1 << 11)),
            );

            core::ptr::write_volatile(
                DPORT_PRO_CACHE_CTRL1_REG,
                core::ptr::read_volatile(DPORT_PRO_CACHE_CTRL1_REG)
                    & !((1 << 3) | (1 << 5) | (0x7 << 6)),
            );
            core::ptr::write_volatile(
                DPORT_APP_CACHE_CTRL1_REG,
                core::ptr::read_volatile(DPORT_APP_CACHE_CTRL1_REG)
                    & !((1 << 3) | (1 << 5) | (0x7 << 6)),
            );

            core::ptr::write_volatile(
                SPI0_PIN_REG,
                core::ptr::read_volatile(SPI0_PIN_REG) & !(1 << 1),
            );

            let _ = cache_sram_mmu_set_rom(0, 0, EXTMEM_ORIGIN as u32, 0, 32, 128);
            core::ptr::write_volatile(
                DPORT_APP_CACHE_CTRL1_REG,
                core::ptr::read_volatile(DPORT_APP_CACHE_CTRL1_REG) & !(1 << 3),
            );
            let _ = cache_sram_mmu_set_rom(1, 0, EXTMEM_ORIGIN as u32, 0, 32, 128);

            let test_word = EXTMEM_ORIGIN as *mut u32;
            test_word.write_volatile(0xA55A_5AA5);
            if test_word.read_volatile() != 0xA55A_5AA5 {
                psram_send_cmd_spi1(0x35, true);
                test_word.write_volatile(0xA55A_5AA5);
            }

            let ptr = EXTMEM_ORIGIN as *mut u8;
            let mut idx: usize = 1023;
            while idx < MAX_MEM_SIZE {
                ptr.add(idx).write_volatile(0x7F);
                idx += 1024;
            }

            let mut guessed_size: usize = 0;
            idx = 1023;
            while idx < MAX_MEM_SIZE {
                if ptr.add(idx).read_volatile() == 0x7F {
                    ptr.add(idx).write_volatile(0);
                    guessed_size = idx + 1;
                } else {
                    break;
                }
                idx += 1024;
            }

            if guessed_size >= 1_048_576 {
                guessed_size
            } else {
                core::ptr::write_volatile(
                    DPORT_PRO_CACHE_CTRL1_REG,
                    core::ptr::read_volatile(DPORT_PRO_CACHE_CTRL1_REG) | (1 << 3) | (1 << 5),
                );
                core::ptr::write_volatile(
                    SPI0_PIN_REG,
                    core::ptr::read_volatile(SPI0_PIN_REG) | (1 << 1),
                );
                0
            }
        }
    }

    /// Settings flash write target. Default build: sector A (0x3FE000) only,
    /// byte-identical to the certified pre-reboot write. 0x3FF000 is page 15
    /// of the C firmware's live `settings` NVS partition, which the certified
    /// build never touched. Boot still reads and validates both sectors.
    #[cfg(not(feature = "runtime-flash-persist"))]
    const SETTINGS_FLASH_TARGET: FlashTargetPolicy = FlashTargetPolicy::FixedA;
    /// Settings flash write target, `runtime-flash-persist`: dual-sector
    /// ping-pong at 0x3FE000 / 0x3FF000.
    #[cfg(feature = "runtime-flash-persist")]
    const SETTINGS_FLASH_TARGET: FlashTargetPolicy = FlashTargetPolicy::PingPong;

    /// Boot resolution step 1 + runtime configuration statics (spec §3.4–3.5).
    ///
    /// Reads both settings sectors (0x3FE000 / 0x3FF000) into `scratch` -- the
    /// only flash read of the settings journal, at boot, single-core -- picks
    /// the newest valid journal, resolves every audio/identity field and builds
    /// in place in `.dram2_uninit` the [`ConfigHandle`] and [`RamJournal`].
    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_config(
        flash: &mut FlashStorage<'_>,
        scratch: &mut [u8; 12_288],
    ) -> (&'static ConfigHandle, &'static mut RamJournal<'static>) {
        // JUSTIFICATION: Dedicated DRAM2 memory regions for settings journal
        // and config handle.
        // SAFETY: Single-core boot initialization guarded by atomic TAKEN.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut JOURNAL_IMAGE: core::mem::MaybeUninit<JournalSector> =
            core::mem::MaybeUninit::uninit();
        // JUSTIFICATION: Dedicated DRAM2 memory regions for settings journal
        // and config handle.
        // SAFETY: Single-core boot initialization guarded by atomic TAKEN.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut RAM_JOURNAL: core::mem::MaybeUninit<RamJournal<'static>> =
            core::mem::MaybeUninit::uninit();
        // JUSTIFICATION: Dedicated DRAM2 memory regions for settings journal
        // and config handle.
        // SAFETY: Single-core boot initialization guarded by atomic TAKEN.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut CONFIG_HANDLE: core::mem::MaybeUninit<ConfigHandle> =
            core::mem::MaybeUninit::uninit();
        static TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
        assert!(
            !TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel),
            "config statics already initialised"
        );

        let (sector_a, rest) = scratch.split_at_mut(4096);
        let (sector_b, _) = rest.split_at_mut(4096);
        let _ = flash.read_nor(JournalSlot::A.offset(SETTINGS_JOURNAL_OFFSET), sector_a);
        let _ = flash.read_nor(JournalSlot::B.offset(SETTINGS_JOURNAL_OFFSET), sector_b);

        let selected = FlashJournal::select_dual_sector(sector_a, sector_b);
        let view = selected.and_then(|(index, _)| {
            JournalView::new(if index == 0 {
                &sector_a[..]
            } else {
                &sector_b[..]
            })
            .ok()
        });
        let audio = resolve_boot_audio(view.as_ref(), &first_boot_defaults::audio_config());
        let boot_slot = selected.map(|(index, seq)| (JournalSlot::from_index(index), seq));

        // JUSTIFICATION: Initialize static settings journal in DRAM2.
        // SAFETY: Called once (TAKEN); statics written before shared.
        unsafe {
            let image = (*core::ptr::addr_of_mut!(JOURNAL_IMAGE)).as_mut_ptr();
            image.write_bytes(0xFF, 1);
            let journal = (*core::ptr::addr_of_mut!(RAM_JOURNAL)).write(RamJournal::new(
                &mut *image,
                boot_slot,
                SETTINGS_FLASH_TARGET,
            ));
            let config = (*core::ptr::addr_of_mut!(CONFIG_HANDLE)).write(ConfigHandle::new(audio));
            (&*config, journal)
        }
    }

    #[allow(unsafe_code)]
    fn take_firmware_bss_buffers() -> &'static mut FirmwareBssBuffers {
        static TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
        assert!(
            !TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel),
            "FirmwareBssBuffers already taken"
        );
        // JUSTIFICATION: Static BSS buffers in DRAM2 to avoid stack exhaustion.
        // SAFETY: Single caller on PRO_CPU guarded by atomic TAKEN.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut BSS_BUFFERS: FirmwareBssBuffers = FirmwareBssBuffers::new();
        // JUSTIFICATION: Access to global static firmware BSS buffer singleton.
        // SAFETY: Only a single caller on PRO_CPU takes ownership of the buffers.
        unsafe { &mut *core::ptr::addr_of_mut!(BSS_BUFFERS) }
    }

    pub(crate) type HwStreamPipeline =
        StreamPipeline<'static, EspI2sSink<Esp32HardwareI2sDma<'static>>>;

    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_static_pipeline(
        stream_storage: &'static mut [u8],
        output_storage: &'static mut [u8],
        sink: EspI2sSink<Esp32HardwareI2sDma<'static>>,
    ) -> PipelineStatics {
        static PIPELINE_TAKEN: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        assert!(
            !PIPELINE_TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel),
            "STATIC_PIPELINE already initialized"
        );
        // JUSTIFICATION: Field-by-field initialization of static
        // MaybeUninit<StreamPipeline> in .dram2_uninit to avoid a 12 KB
        // stack temporary during boot.
        // SAFETY: Called once during single-core PRO_CPU bring-up before
        // returning exclusive &'static mut references; `ptr.write_bytes(0, 1)`
        // zeroes the entire uninitialized `.dram2_uninit` region prior to
        // writing every struct field.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_PIPELINE: core::mem::MaybeUninit<HwStreamPipeline> =
            core::mem::MaybeUninit::uninit();
        // JUSTIFICATION: Write fields of uninitialized static pipeline in DRAM2.
        // SAFETY: Exclusive access guaranteed by atomic PIPELINE_TAKEN;
        // pointer is valid.
        unsafe {
            let ptr = (*core::ptr::addr_of_mut!(STATIC_PIPELINE)).as_mut_ptr();
            ptr.write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).stream_buffer)
                .write(squeezelite_audio_core::AudioRingBuffer::new(stream_storage));
            core::ptr::addr_of_mut!((*ptr).output_buffer)
                .write(squeezelite_audio_core::AudioRingBuffer::new(output_storage));
            core::ptr::addr_of_mut!((*ptr).decoder)
                .write(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
            core::ptr::addr_of_mut!((*ptr).equalizer)
                .write(squeezelite_audio_core::Equalizer::new());
            core::ptr::addr_of_mut!((*ptr).gain_stage)
                .write(squeezelite_audio_core::GainStage::unity());
            core::ptr::addr_of_mut!((*ptr).requested_volume).write((0x1_0000, 0x1_0000));
            core::ptr::addr_of_mut!((*ptr).sink).write(sink);
            core::ptr::addr_of_mut!((*ptr).sample_rate).write(44_100);
            core::ptr::addr_of_mut!((*ptr).frames_played).write(0);
            core::ptr::addr_of_mut!((*ptr).pcm_pending_offset).write(0);
            core::ptr::addr_of_mut!((*ptr).pcm_pending_samples).write(0);
            core::ptr::addr_of_mut!((*ptr).read_scratch).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).pcm_scratch).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).write_scratch).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).output_i32_scratch).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).current_eq_preset).write(EqPreset::Flat);
            core::ptr::addr_of_mut!((*ptr).user_muted).write(false);
            core::ptr::addr_of_mut!((*ptr).mp3_spare).write(Some(take_static_mp3_decoder()));
            core::ptr::addr_of_mut!((*ptr).boundary)
                .write(louder_esp32_app::pipeline::TrackBoundary::new(44_100));
            core::ptr::addr_of_mut!((*ptr).fade).write(squeezelite_audio_core::FadeEngine::new());
            core::ptr::addr_of_mut!((*ptr).decode_us_max).write(0);
            core::ptr::addr_of_mut!((*ptr).decode_us_first).write(0);
            core::ptr::addr_of_mut!((*ptr).eq_gains_db).write([0.0; 10]);
            core::ptr::addr_of_mut!((*ptr).loudness_level).write(0);
            core::ptr::addr_of_mut!((*ptr).active_sample_rate_hz).write(44_100);
            core::ptr::addr_of_mut!((*ptr).resampler).write(None);
            core::ptr::addr_of_mut!((*ptr).output_threshold_frames).write(4_096);
            core::ptr::addr_of_mut!((*ptr).threshold_satisfied).write(false);
            core::ptr::addr_of_mut!((*ptr).resample_tail).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*ptr).resample_tail_offset).write(0);
            core::ptr::addr_of_mut!((*ptr).resample_tail_samples).write(0);
            core::ptr::addr_of_mut!((*ptr).resample_input_pending).write(0);

            let http_resp_buf: &'static mut [u8; 1536] =
                alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 1536]));
            let http_tx_buf: &'static mut [u8; 1536] =
                alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 1536]));

            (&mut *ptr, http_resp_buf, http_tx_buf)
        }
    }

    /// Core-1 (APP_CPU) control kept by the firmware for the life of the
    /// image.
    ///
    /// Default build: the manual start path (`CpuControl::start_app_core` +
    /// NOP heartbeat). The guard is held forever because dropping an
    /// `AppCoreGuard` parks APP_CPU. Feature `esp-rtos-core1`: core 1 is owned
    /// by `esp_rtos::start_second_core` (S1 sequencing note; unvalidated on
    /// hardware until the S1 owner gate).
    pub(crate) struct Core1Ctl {
        #[cfg(not(feature = "esp-rtos-core1"))]
        cpu_ctrl: CpuControl<'static>,
        #[cfg(not(feature = "esp-rtos-core1"))]
        _guard: Result<esp_hal::system::AppCoreGuard<'static>, esp_hal::system::Error>,
        active: bool,
    }

    impl Core1Ctl {
        /// `core1_active` in `/status.json`.
        pub(crate) fn is_active(&self) -> bool {
            self.active
        }

        /// Parks APP_CPU before the reboot-time settings flash write (Core-1
        /// parking rule).
        pub(crate) fn park_for_flash_write(&mut self) {
            CORE1_PARK_REQ.store(true, core::sync::atomic::Ordering::Release);
            for _ in 0..100_000 {
                if CORE1_PARKED.load(core::sync::atomic::Ordering::Acquire) {
                    break;
                }
                core::hint::spin_loop();
            }
            #[cfg(not(feature = "esp-rtos-core1"))]
            #[allow(unsafe_code)]
            // JUSTIFICATION: Parking APP_CPU before SPI flash erase/write to
            // prevent XIP cache fetch collisions on shared SPI0/SPI1 bus.
            // SAFETY: APP_CPU executes an IRAM-resident loop (`app_core_loop`)
            // holding no locks.
            unsafe {
                self.cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu);
            }
            // `CPU_CTRL` was handed to `esp_rtos::start_second_core`; this
            // runs only right before `software_reset()`, so a stolen handle
            // can not race another owner.
            #[cfg(feature = "esp-rtos-core1")]
            #[allow(unsafe_code)]
            // JUSTIFICATION: Steal CPU_CTRL right before software_reset() to
            // park APP_CPU during pre-reboot flash write.
            // SAFETY: Executed immediately prior to software_reset() on
            // PRO_CPU with no concurrent CPU_CTRL access.
            unsafe {
                CpuControl::new(esp_hal::peripherals::CPU_CTRL::steal())
                    .park_core(esp_hal::system::Cpu::AppCpu);
            }
        }

        /// Restarts APP_CPU after a runtime settings flash write
        /// (`runtime-flash-persist` only; see `tasks::runtime_flash_commit`).
        /// Default build: the manual path's core 1 runs the IRAM NOP
        /// heartbeat, so it resumes where it was parked.
        #[cfg(feature = "runtime-flash-persist")]
        pub(crate) fn unpark_after_flash_write(&mut self) {
            CORE1_PARK_REQ.store(false, core::sync::atomic::Ordering::Release);
            #[cfg(not(feature = "esp-rtos-core1"))]
            self.cpu_ctrl.unpark_core(esp_hal::system::Cpu::AppCpu);
        }
    }

    // `runtime-flash-persist` parks APP_CPU mid-flight at runtime. Under
    // `esp-rtos-core1` core 1 runs an embassy executor that takes critical
    // sections; parked while holding the multicore critical-section lock, it
    // would deadlock core 0's next flash call. Only the manual core-1 path
    // (IRAM NOP loop, never locks) is allowed with the runtime commit.
    #[cfg(all(feature = "runtime-flash-persist", feature = "esp-rtos-core1"))]
    compile_error!(
        "`runtime-flash-persist` requires the manual core-1 path: \
         it can not be combined with `esp-rtos-core1`"
    );

    /// Smoltcp socket slots: DHCP, HTTP server, slimproto, audio, mDNS UDP
    /// (5353), AirPlay RTSP TCP (5000), AirPlay RTP UDP (6000), and Spotify
    /// Connect HTTP/Spirc TCP (4070).
    const SOCKET_SLOTS: usize = 8;

    /// Metadata/payload sizing for the mDNS responder UDP socket (port 5353).
    const MDNS_UDP_META: usize = 4;
    const MDNS_UDP_PAYLOAD: usize = 1024;

    /// Metadata/payload sizing for the AirPlay RTP audio UDP socket (port
    /// 6000).
    const RTP_UDP_RX_META: usize = 8;
    const RTP_UDP_TX_META: usize = 2;

    struct UdpMdnsBuffers {
        rx_meta: [udp::PacketMetadata; MDNS_UDP_META],
        rx_payload: [u8; MDNS_UDP_PAYLOAD],
        tx_meta: [udp::PacketMetadata; MDNS_UDP_META],
        tx_payload: [u8; MDNS_UDP_PAYLOAD],
    }

    impl UdpMdnsBuffers {
        const fn new() -> Self {
            Self {
                rx_meta: [udp::PacketMetadata::EMPTY; MDNS_UDP_META],
                rx_payload: [0u8; MDNS_UDP_PAYLOAD],
                tx_meta: [udp::PacketMetadata::EMPTY; MDNS_UDP_META],
                tx_payload: [0u8; MDNS_UDP_PAYLOAD],
            }
        }
    }

    struct UdpRtpMetaBuffers {
        rx_meta: [udp::PacketMetadata; RTP_UDP_RX_META],
        tx_meta: [udp::PacketMetadata; RTP_UDP_TX_META],
    }

    impl UdpRtpMetaBuffers {
        const fn new() -> Self {
            Self {
                rx_meta: [udp::PacketMetadata::EMPTY; RTP_UDP_RX_META],
                tx_meta: [udp::PacketMetadata::EMPTY; RTP_UDP_TX_META],
            }
        }
    }

    /// Bring-up stage: I2C0 (SDA=21, SCL=27, 400 kHz) and the TAS58xx manager
    /// over it, allocated in external PSRAM. Construction only; the HIL amp
    /// init keeps its legacy place in [`bring_up`].
    #[inline(never)]
    fn init_amp_i2c(
        i2c0: esp_hal::peripherals::I2C0<'static>,
        sda: esp_hal::peripherals::GPIO21<'static>,
        scl: esp_hal::peripherals::GPIO27<'static>,
    ) -> &'static mut AmpManager<Esp32I2cBus<'static>> {
        let i2c_cfg = I2cConfig::default().with_frequency(Rate::from_khz(400));
        let hal_i2c = HalI2c::new(i2c0, i2c_cfg)
            .unwrap_or_else(|_| esp_hal::system::software_reset())
            .with_sda(sda)
            .with_scl(scl);
        let i2c_bus = Esp32I2cBus::new(hal_i2c);
        alloc::boxed::Box::leak(alloc::boxed::Box::new(AmpManager::for_active_board(
            i2c_bus,
        )))
    }

    /// Static pipeline + the static HTTP-server buffers handed out with it
    /// (`init_static_pipeline`).
    type PipelineStatics = (
        &'static mut HwStreamPipeline,
        &'static mut [u8; 1536],
        &'static mut [u8; 1536],
    );

    /// Bring-up stage: I2S0 TDM-Philips TX (BCK=26, WS=25, DOUT=22) with
    /// streaming DMA prefilled with silence, then the I2S sink and the static
    /// audio pipeline around it. Building the sink (a ~1 KB value) here keeps
    /// it out of [`bring_up`]'s frame; sink/pipeline construction is pure
    /// memory initialisation that used to run a few statements later.
    #[inline(never)]
    fn init_i2s_pipeline(
        i2s0: esp_hal::peripherals::I2S0<'static>,
        dma: esp_hal::peripherals::DMA_I2S0<'static>,
        bclk: esp_hal::peripherals::GPIO26<'static>,
        ws: esp_hal::peripherals::GPIO25<'static>,
        dout: esp_hal::peripherals::GPIO22<'static>,
        stream_storage: &'static mut [u8],
        output_storage: &'static mut [u8],
    ) -> PipelineStatics {
        let i2s_tdm_cfg = TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(44_100))
            .with_data_format(DataFormat::Data32Channel32)
            .with_channels(Channels::STEREO);
        #[allow(unsafe_code)]
        // JUSTIFICATION: Static DMA TX buffer in `.dram2_uninit` internal SRAM
        // (0x3FFE_0000..0x4000_0000) expands DMA depth to 46.44 ms while
        // freeing 8 KiB from `.bss` to expand `.stack` headroom.
        // SAFETY: Accessed once on PRO_CPU during single-core initialization.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut DMA_TX_BUF: esp_hal::dma::aligned::InternalMemory<[u8; 16384]> =
            esp_hal::dma::aligned::InternalMemory::new([0u8; 16384]);

        #[allow(unsafe_code)]
        // JUSTIFICATION: Static DMA descriptors in `.dram2_uninit` internal
        // SRAM for 16 chunks of 1024 B.
        // SAFETY: Accessed once on PRO_CPU during single-core initialization.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut DMA_TX_DESCRIPTORS: esp_hal::dma::aligned::InternalMemory<
            [esp_hal::dma::DmaDescriptor; 16],
        > = esp_hal::dma::aligned::InternalMemory::new([esp_hal::dma::DmaDescriptor::EMPTY; 16]);

        #[allow(unsafe_code)]
        // JUSTIFICATION: Borrow singleton DMA TX buffer in `.dram2_uninit`.
        // SAFETY: Accessed exclusively once during PRO_CPU initialization.
        let dma_tx_slice = unsafe {
            (&mut *core::ptr::addr_of_mut!(DMA_TX_BUF))
                .get_mut()
                .unsize()
        };
        #[allow(unsafe_code)]
        // JUSTIFICATION: Borrow singleton DMA descriptors in `.dram2_uninit`.
        // SAFETY: Accessed exclusively once during PRO_CPU initialization.
        let tx_descriptors = unsafe {
            (&mut *core::ptr::addr_of_mut!(DMA_TX_DESCRIPTORS))
                .get_mut()
                .unsize()
        };
        let mut tx_buffer = esp_hal::dma::DmaTxStreamBuf::new(tx_descriptors, dma_tx_slice)
            .unwrap_or_else(|_| esp_hal::system::software_reset());
        Esp32HardwareI2sDma::prefill_silence(&mut tx_buffer);
        let i2s =
            I2s::new(i2s0, dma, i2s_tdm_cfg).unwrap_or_else(|_| esp_hal::system::software_reset());
        let i2s_tx = i2s
            .i2s_tx
            .with_bclk(bclk)
            .with_ws(ws)
            .with_dout(dout)
            .build();
        let i2s_transfer = i2s_tx
            .write(tx_buffer)
            .unwrap_or_else(|_| esp_hal::system::software_reset());
        let hw_i2s_driver = Esp32HardwareI2sDma::new(i2s_transfer, 44_100);
        let sink = EspI2sSink::for_active_board(hw_i2s_driver, 44_100);
        init_static_pipeline(stream_storage, output_storage, sink)
    }

    /// Bring-up stage: UART2 @ 460 800 (TX=4, RX=36) and the CYD bridge over
    /// it, allocated in external PSRAM.
    #[inline(never)]
    fn init_cyd_uart(
        uart2: esp_hal::peripherals::UART2<'static>,
        tx: esp_hal::peripherals::GPIO4<'static>,
        rx: esp_hal::peripherals::GPIO36<'static>,
    ) -> &'static mut CydUartBridge<Esp32Uart2Port<'static>> {
        let uart2_rx = Input::new(rx, InputConfig::default().with_pull(Pull::None));
        let uart2_cfg = UartConfig::default().with_baudrate(460_800);
        let hal_uart2 = Uart::new(uart2, uart2_cfg)
            .unwrap_or_else(|_| esp_hal::system::software_reset())
            .with_tx(tx)
            .with_rx(uart2_rx);
        let uart2_port = Esp32Uart2Port::new(hal_uart2);
        alloc::boxed::Box::leak(alloc::boxed::Box::new(CydUartBridge::new(uart2_port)))
    }

    /// Bring-up stage: the player coordinator, allocated in external PSRAM,
    /// with this board's efuse STA MAC (HELO; smoltcp uses the same MAC), the
    /// boot-resolved player name (`SETD` after HELO, `setd` id-0 replies) and
    /// LMS address (`strm s` with server IP 0.0.0.0 streams from it), and the
    /// [`ConfigHandle`] a `setd` id-0 name change persists through. Server
    /// frames are received into a heap buffer in PSRAM instead of a 2 KB
    /// buffer on the `slimproto_session` stack.
    #[inline(never)]
    fn init_coordinator(
        sta_mac: [u8; 6],
        config: &'static ConfigHandle,
        lms_ip: [u8; 4],
    ) -> &'static mut PlayerCoordinator {
        let frame_buf: &'static mut [u8] = alloc::boxed::Box::leak(
            alloc::vec![0u8; louder_esp32_app::coordinator::SERVER_FRAME_BUF_LEN]
                .into_boxed_slice(),
        );
        let coordinator = alloc::boxed::Box::leak(alloc::boxed::Box::new(PlayerCoordinator::new(
            sta_mac,
            "SqueezeESP32",
        )));
        config.read(|c| {
            coordinator.player_name.clear();
            let _ = coordinator.player_name.push_str(c.player_name.as_str());
        });
        coordinator.lms_ip = lms_ip;
        coordinator.config = Some(config);
        coordinator.set_frame_buffer(frame_buf);
        // Decode/output run only in the DMA-first pump phases, never inside
        // tick.
        coordinator.external_audio_pump = true;
        coordinator
    }

    /// Bring-up stage: `esp-radio` Wi-Fi controller configured as a station
    /// for `wifi_sta_creds` (any configuration failure resets into recovery,
    /// as before).
    #[inline(never)]
    fn init_wifi(
        wifi: esp_hal::peripherals::WIFI<'static>,
        wifi_sta_creds: &WifiStaCredentials,
    ) -> WifiController<'static> {
        let auth_cfg = if wifi_sta_creds.password.is_empty() {
            AuthenticationMethodConfig::Open
        } else {
            match wifi_sta_creds.password.as_str().try_into() {
                Ok(pw) => AuthenticationMethodConfig::WpaWpa2Personal(pw),
                Err(_) => esp_hal::system::software_reset(),
            }
        };
        let ssid_cfg = match wifi_sta_creds.ssid.as_str().try_into() {
            Ok(s) => s,
            Err(_) => esp_hal::system::software_reset(),
        };
        let sta_cfg = WifiModeConfig::Station(
            StationConfig::default()
                .with_ssid(ssid_cfg)
                .with_authentication(auth_cfg)
                .with_scan_method(esp_radio::wifi::sta::ScanMethod::AllChannels)
                .with_beacon_timeout(30)
                .with_failure_retry_cnt(3),
        );
        let wifi_cfg = ControllerConfig::default()
            .with_static_rx_buf_num(6)
            .with_dynamic_rx_buf_num(16)
            .with_dynamic_tx_buf_num(16);
        let mut wifi_controller = match WifiController::new(wifi, wifi_cfg) {
            Ok(c) => c,
            Err(_) => esp_hal::system::software_reset(),
        };
        let _ = wifi_controller.set_power_saving(PowerSaveMode::None);
        let _ = wifi_controller.set_max_tx_power(52);
        if wifi_controller.set_config(&sta_cfg).is_err() {
            esp_hal::system::software_reset();
        }
        let _ = wifi_controller.set_power_saving(PowerSaveMode::None);
        let _ = wifi_controller.set_max_tx_power(52);
        wifi_controller
    }

    // The TAS58xx typestate transitions move ~320 B driver values around; when
    // inlined into `bring_up` they made up most of its frame. These wrappers
    // keep the legacy call sequences verbatim but in their own short-lived
    // frames.

    /// HIL step: initialise the TAS58xx (32-bit) and set unity volume.
    #[inline(never)]
    fn amp_hil_init(amp: &mut AmpManager<Esp32I2cBus<'static>>) {
        let _ = amp.initialize(32);
        let _ = amp.set_volume(VOL_UNITY_0DB);
    }

    /// Apply the boot-resolved audio configuration: volume, analog gain, EQ
    /// and loudness. Returns the volume percent. The volume goes through the
    /// coordinator's single volume entry (register
    /// `percent_to_attenuation(percent)` as before, software gain per the
    /// device policy, requested volume recorded for the loudness).
    #[inline(never)]
    fn amp_apply_boot_config(
        coordinator: &mut PlayerCoordinator,
        amp: &mut AmpManager<Esp32I2cBus<'static>>,
        pipeline: &mut HwStreamPipeline,
        config: &ConfigHandle,
    ) -> u8 {
        let (percent, again_code, eq_preset, loudness) = config.read(|c| {
            (
                c.volume,
                c.analog_gain_code,
                c.equalizer_preset,
                c.loudness_enabled,
            )
        });
        let mut backend = Esp32AudioBackend::new(amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
        coordinator.set_volume_percent(percent, &mut backend, pipeline);
        let _ = amp.set_analog_gain(again_code);
        pipeline.apply_eq_preset(EqPreset::from_u8(eq_preset));
        pipeline.set_loudness(loudness);
        percent
    }

    /// Pre-loop amp start: clear faults, Hi-Z -> Play, first telemetry read.
    #[inline(never)]
    fn amp_start_playback(
        amp: &mut AmpManager<Esp32I2cBus<'static>>,
    ) -> Option<squeezelite_tas58xx::Tas5805mTelemetry> {
        let mut latest_telemetry = None;
        let _ = amp.clear_faults();
        let _ = amp.start_playback_via_hiz();
        if let Ok(telem) = amp.read_telemetry() {
            latest_telemetry = telem;
        }
        latest_telemetry
    }

    /// Adds one socket to the set from its own frame (each smoltcp socket
    /// value is ~312 B and would otherwise be a separate temporary in
    /// [`init_net_stack`]'s frame).
    #[inline(never)]
    fn add_socket<T: AnySocket<'static>>(
        sockets: &mut SocketSet<'static>,
        socket: impl FnOnce() -> T,
    ) -> SocketHandle {
        sockets.add(socket())
    }

    /// Output of [`init_net_stack`].
    struct NetStack {
        smol_iface: &'static mut SmolInterface,
        sockets: SocketSet<'static>,
        dhcp_handle: SocketHandle,
        http_handle: SocketHandle,
        slim_handle: SocketHandle,
        audio_handle: SocketHandle,
        mdns_handle: SocketHandle,
        rtsp_handle: SocketHandle,
        rtp_handle: SocketHandle,
        spot_handle: SocketHandle,
        local_port_counter: u16,
    }

    /// TCP socket buffers handed to [`init_net_stack`]: (rx, tx) for HTTP,
    /// slimproto, audio.
    type TcpBuffers = [(&'static mut [u8], &'static mut [u8]); 3];

    /// Bring-up stage: `smoltcp` Ethernet interface + DHCPv4 + TCP port 80 +
    /// TCP SlimProto (3483) + audio socket + mDNS UDP (5353) + AirPlay RTSP
    /// TCP (5000) + AirPlay RTP UDP (6000) + Spotify Connect HTTP/Spirc TCP
    /// (4070), allocated in external PSRAM.
    #[inline(never)]
    fn init_net_stack(
        sta_iface: &mut WifiInterface,
        sta_mac: [u8; 6],
        tcp_bufs: TcpBuffers,
    ) -> NetStack {
        let [(http_rx_buf, http_tx_buf), (slim_rx_buf, slim_tx_buf), (audio_rx_buf, audio_tx_buf)] =
            tcp_bufs;
        let hw_rng = Rng::new();
        let rng_seed = (u64::from(hw_rng.random()) << 32) | u64::from(hw_rng.random());
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(sta_mac));
        let mut smol_cfg = SmolIfaceConfig::new(hw_addr);
        smol_cfg.random_seed = rng_seed;
        let local_port_counter: u16 = 49152 + ((hw_rng.random() as u16) & 0x3FFF);
        let mut smol_dev = SmolDevice { driver: sta_iface };
        let smol_iface = alloc::boxed::Box::leak(alloc::boxed::Box::new(SmolInterface::new(
            smol_cfg,
            &mut smol_dev,
            SmolInstant::from_millis(now_millis() as i64),
        )));

        let socket_storage: &'static mut [SocketStorage<'static>] = alloc::boxed::Box::leak(
            (0..SOCKET_SLOTS)
                .map(|_| SocketStorage::EMPTY)
                .collect::<alloc::vec::Vec<_>>()
                .into_boxed_slice(),
        );
        let mut sockets = SocketSet::new(&mut socket_storage[..]);

        let dhcp_handle = add_socket(&mut sockets, dhcpv4::Socket::new);

        let http_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(
                tcp::SocketBuffer::new(http_rx_buf),
                tcp::SocketBuffer::new(http_tx_buf),
            )
        });

        let slim_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(
                tcp::SocketBuffer::new(slim_rx_buf),
                tcp::SocketBuffer::new(slim_tx_buf),
            )
        });

        let audio_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(
                tcp::SocketBuffer::new(audio_rx_buf),
                tcp::SocketBuffer::new(audio_tx_buf),
            )
        });

        let mdns_bufs = alloc::boxed::Box::leak(alloc::boxed::Box::new(UdpMdnsBuffers::new()));
        let mdns_handle = add_socket(&mut sockets, || {
            udp::Socket::new(
                udp::PacketBuffer::new(&mut mdns_bufs.rx_meta[..], &mut mdns_bufs.rx_payload[..]),
                udp::PacketBuffer::new(&mut mdns_bufs.tx_meta[..], &mut mdns_bufs.tx_payload[..]),
            )
        });

        let rtsp_rx: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 4096].into_boxed_slice());
        let rtsp_tx: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 2048].into_boxed_slice());
        let rtsp_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(
                tcp::SocketBuffer::new(rtsp_rx),
                tcp::SocketBuffer::new(rtsp_tx),
            )
        });

        let rtp_meta = alloc::boxed::Box::leak(alloc::boxed::Box::new(UdpRtpMetaBuffers::new()));
        let rtp_rx_payload: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 16_384].into_boxed_slice());
        let rtp_tx_payload: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 512].into_boxed_slice());
        let rtp_handle = add_socket(&mut sockets, || {
            udp::Socket::new(
                udp::PacketBuffer::new(&mut rtp_meta.rx_meta[..], rtp_rx_payload),
                udp::PacketBuffer::new(&mut rtp_meta.tx_meta[..], rtp_tx_payload),
            )
        });

        let spot_rx: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 4096].into_boxed_slice());
        let spot_tx: &'static mut [u8] =
            alloc::boxed::Box::leak(alloc::vec![0u8; 2048].into_boxed_slice());
        let spot_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(
                tcp::SocketBuffer::new(spot_rx),
                tcp::SocketBuffer::new(spot_tx),
            )
        });

        NetStack {
            smol_iface,
            sockets,
            dhcp_handle,
            http_handle,
            slim_handle,
            audio_handle,
            mdns_handle,
            rtsp_handle,
            rtp_handle,
            spot_handle,
            local_port_counter,
        }
    }

    /// `HELO` device id (12 = SqueezeESP32/squeezelite).
    const HELO_DEVICE_ID: u8 = 12;
    /// Audio-socket connect/retransmit timeout: an unreachable stream server
    /// ends in `STMn` instead of a connect that never resolves.
    const STREAM_SOCKET_TIMEOUT: SmolDuration = SmolDuration::from_secs(10);

    /// Unified device runtime state holding Phase 0+1 `PlayerCoordinator` +
    /// `net_device` + `backend_device` + `ConfigHandle`/`RamJournal` alongside
    /// `WO-197` 4 MB PSRAM, `WO-211`/`WO-212`/`WO-214` AirPlay 1 RAOP
    /// (`squeezelite-raop`), Spotify Connect (`squeezelite-cspot`), mDNS
    /// (`UDP 5353`), and `SourceArbiter`.
    struct DeviceRuntimeState {
        wdt: Wdt<esp_hal::peripherals::TIMG0<'static>>,
        flash: FlashStorage<'static>,
        core1: Core1Ctl,
        _pwdn: Output<'static>,
        _fault: Input<'static>,
        wifi_controller: WifiController<'static>,
        sta_iface: WifiInterface,
        smol_iface: &'static mut SmolInterface,
        sockets: SocketSet<'static>,
        dhcp_handle: SocketHandle,
        http_handle: SocketHandle,
        slim_handle: SocketHandle,
        audio_handle: SocketHandle,
        mdns_handle: SocketHandle,
        rtsp_handle: SocketHandle,
        rtp_handle: SocketHandle,
        spot_handle: SocketHandle,
        bt_periph: Option<esp_hal::peripherals::BT<'static>>,
        bt_transport: Option<Esp32VhciTransport<'static>>,
        bt_sink: &'static mut BtA2dpSink,
        amp: &'static mut AmpManager<Esp32I2cBus<'static>>,
        cyd_bridge: &'static mut CydUartBridge<Esp32Uart2Port<'static>>,
        pipeline: &'static mut HwStreamPipeline,
        coordinator: &'static mut PlayerCoordinator,
        slim_link: SlimLinkState,
        stream_state: HttpStreamState,
        http_resp_buf: &'static mut [u8; 1536],
        config: &'static ConfigHandle,
        journal: &'static mut RamJournal<'static>,
        config_writer: ConfigWriter,
        wifi_sta_creds: &'static WifiStaCredentials,
        sta_mac: [u8; 6],
        lms_ip: [u8; 4],
        lms_port: u16,
        current_vol_pct: u8,
        local_port_counter: u16,
        bt_audio_active: bool,
        boot_ms: u64,
        lease_watchdog: DeadManLeaseWatchdog,
        assigned_ip: Option<[u8; 4]>,
        assigned_mask: [u8; 4],
        assigned_gw: [u8; 4],
        pending_http_action: HttpControlAction,
        pending_action_deadline_ms: u64,
        pending_http_body: &'static [u8],
        pending_http_started_ms: u64,
        pending_http_closing: bool,
        pending_close_deadline_ms: u64,
        helo_sent: bool,
        last_connect_attempt_ms: u64,
        slim_reconnect: SlimReconnect,
        slim_was_open: bool,
        slim_switch_to: Option<[u8; 4]>,
        last_fault_check_ms: u64,
        latest_telemetry: Option<squeezelite_tas58xx::Tas5805mTelemetry>,
        last_cyd_progress_ms: u64,
        last_config_poll_ms: u64,
        mdns_bound: bool,
        last_mdns_announce_ms: u64,
        raop_rtsp: alloc::boxed::Box<RtspServer>,
        raop_rtp: alloc::boxed::Box<RtpAudioReceiver>,
        raop_streaming: bool,
        last_rtp_packet_ms: u64,
        rtsp_rx_acc: alloc::vec::Vec<u8>,
        spot_spirc: alloc::boxed::Box<SpircStateMachine>,
        spot_streaming: bool,
        now_ms: u64,
        wifi_connected_logged: bool,
        last_fault_recovery_ms: u64,
        last_bt_idle_poll_ms: u64,
    }

    /// Device entry: bring-up, then the Phase 0+1 + Multi-Protocol runtime
    /// pump loop.
    pub fn run_hardware_firmware() -> ! {
        let state = bring_up();
        loop {
            pump_device_phase(state);
        }
    }

    /// Hardware bring-up, in the legacy order, ending in the initial runtime
    /// state allocated in external PSRAM. Heavy steps run in `#[inline(never)]`
    /// stages so this frame stays small.
    #[inline(never)]
    fn bring_up() -> &'static mut DeviceRuntimeState {
        let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
        let peripherals = esp_hal::init(config);

        // Park Core 1 (APP_CPU) immediately on entry in case a warm reboot
        // from dual-core ESP-IDF (`I2S-4MFlash-1681`) left `APP_CPU` unparked
        // (`appcpu_clkgate_en=1`), preventing
        // `FlashStorage::multicore_auto_park()` from unparking a rogue
        // `APP_CPU` and ensuring `cpu_ctrl.start_app_core(...)` sees
        // `is_running(Cpu::AppCpu) == false`.
        #[cfg(not(feature = "esp-rtos-core1"))]
        let mut cpu_ctrl = CpuControl::new(peripherals.CPU_CTRL);
        #[cfg(not(feature = "esp-rtos-core1"))]
        #[allow(unsafe_code)]
        // JUSTIFICATION: Park APP_CPU on entry so warm reboot from dual-core
        // ESP-IDF cannot execute stale code on APP_CPU during Flash / PSRAM
        // initialization.
        // SAFETY: Called at the very start of `bring_up` on PRO_CPU before
        // APP_CPU is started.
        unsafe {
            cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu);
        }
        #[cfg(feature = "esp-rtos-core1")]
        let mut cpu_ctrl_periph = peripherals.CPU_CTRL;
        #[cfg(feature = "esp-rtos-core1")]
        #[allow(unsafe_code)]
        // JUSTIFICATION: Park APP_CPU on entry before Flash / PSRAM bring-up.
        // SAFETY: Called at the very start of `bring_up` on PRO_CPU.
        unsafe {
            CpuControl::new(cpu_ctrl_periph.reborrow()).park_core(esp_hal::system::Cpu::AppCpu);
        }

        // ====================================================================
        // GATE 2: Hardware Watchdog (TIMG0 WDT, 20s Stage0 -> ResetSystem)
        // ====================================================================
        let timg0 = TimerGroup::new(peripherals.TIMG0);
        let mut wdt = timg0.wdt;
        wdt.set_timeout(MwdtStage::Stage0, Duration::from_millis(20_000));
        wdt.enable();
        wdt.feed();

        // ====================================================================
        // GATE 1: First-Instruction `otadata` (0xD000..0xF000) Pre-Arming
        // Erases `otadata` to 0xFF BEFORE PSRAM, Core 1, or Wi-Fi so ANY
        // subsequent reset (panic, WDT, network timeout, or power cycle) boots
        // straight back into the untouched factory `recovery` partition
        // (0x10000)!
        // ====================================================================
        let mut flash = FlashStorage::new(peripherals.FLASH).multicore_auto_park();
        if flash
            .erase(OTADATA_PARTITION_OFFSET, OTADATA_PARTITION_END)
            .is_err()
        {
            println!("[ANTI-BRICK GATE 1] Failed to erase otadata -> resetting to recovery");
            esp_hal::system::software_reset();
        }
        let mut verify_s0 = [0u8; 64];
        let mut verify_s1 = [0u8; 64];
        if flash
            .read(OTADATA_PARTITION_OFFSET, &mut verify_s0)
            .is_err()
            || flash.read(OTADATA_SECTOR_1_OFFSET, &mut verify_s1).is_err()
            || verify_otadata_erased(&verify_s0).is_err()
            || verify_otadata_erased(&verify_s1).is_err()
        {
            println!("[ANTI-BRICK GATE 1] otadata 0xFF read-back failed -> resetting to recovery");
            esp_hal::system::software_reset();
        }
        wdt.feed();
        println!(
            "[ANTI-BRICK GATE 1] otadata (0xD000..0xF000) erased & verified \
             0xFF (Factory recovery pre-armed)"
        );

        // Read 16 KB `nvs` partition (0x9000..0xD000) and `settings`
        // (0x3FE000) while single-core.
        let sta_mac_addr = interface_mac_address(InterfaceMacAddress::Station);
        let mut sta_mac = [0u8; 6];
        sta_mac.copy_from_slice(sta_mac_addr.as_bytes());

        let fw_bufs = take_firmware_bss_buffers();
        let FirmwareBssBuffers {
            ref mut ring_storage,
            ref mut http_rx_buf,
            ref mut slim_rx_buf,
            ref mut slim_tx_buf,
            ref mut audio_rx_buf,
            ref mut audio_tx_buf,
        } = fw_bufs;

        // Boot config resolution (spec §3.4), each field independently:
        // settings FlashJournal, then (Wi-Fi only) the C firmware's NVS, then
        // the compile-time first-boot defaults.
        let (config, journal) = {
            #[allow(unsafe_code)]
            // JUSTIFICATION: Use uninitialized DRAM2 memory of STATIC_BT_HEAP
            // as temporary 12 KB scratch during early single-core boot.
            // SAFETY: Accessed exclusively during early boot before heap init.
            let boot_scratch: &mut [u8; 12_288] = unsafe {
                let ptr =
                    (*core::ptr::addr_of_mut!(STATIC_BT_HEAP)).as_mut_ptr() as *mut [u8; 12_288];
                &mut *ptr
            };
            init_config(&mut flash, boot_scratch)
        };
        wdt.feed();
        let wifi_sta_creds = {
            #[allow(unsafe_code)]
            // JUSTIFICATION: Use uninitialized DRAM2 memory of STATIC_BT_HEAP
            // as temporary 12 KB scratch during early single-core boot.
            // SAFETY: Accessed exclusively during early boot before heap init.
            let boot_scratch: &mut [u8; 12_288] = unsafe {
                let ptr =
                    (*core::ptr::addr_of_mut!(STATIC_BT_HEAP)).as_mut_ptr() as *mut [u8; 12_288];
                &mut *ptr
            };
            init_wifi_credentials(&mut flash, boot_scratch, &sta_mac)
        };
        let (lms_ip, lms_port) = config.read(|c| {
            (
                c.lms_server_ip
                    .unwrap_or(first_boot_defaults::LMS_SERVER_IP),
                c.lms_server_port
                    .unwrap_or(first_boot_defaults::LMS_SERVER_PORT),
            )
        });
        ring_storage.fill(0);
        wdt.feed();

        // Register 4 MB external PSRAM (`0x3F80_0000`) FIRST as Region 0 of
        // `esp_alloc::HEAP` (`MemoryCapability::External`) so all standard Rust
        // `GlobalAlloc` allocations (`Box::new`, `vec!`) allocate from PSRAM,
        // preserving 100% of internal DRAM (`43 KB` Region 1 + `22 KB` Region
        // 2 = `65 KB` `MemoryCapability::Internal`) for `esp-rtos` and
        // `esp-radio` (`Wi-Fi` + `BLE`/`BTDM`).
        #[allow(unsafe_code)]
        let psram_avail_bytes = {
            const EXTMEM_ORIGIN: usize = 0x3F80_0000;
            const EXTMEM_SIZE: usize = louder_esp32_app::pipeline::PSRAM_HEAP_BYTES;
            // JUSTIFICATION: Execute 100% IRAM-resident PSRAM SPI1 + DPORT MMU
            // initialization (`WO-197`) before registering heap regions.
            // SAFETY: Single-core execution with interrupts masked inside the
            // IRAM routine and `APP_CPU` parked.
            let psram_bytes = unsafe { init_and_map_psram_4mb() };
            let reg_size = if psram_bytes >= 1_048_576 {
                let sz = psram_bytes.min(EXTMEM_SIZE);
                // JUSTIFICATION: Register verified 4 MB external PSRAM
                // (`0x3F80_0000..0x3FC0_0000`) as Region 0 of `esp_alloc::HEAP`.
                // SAFETY: `init_and_map_psram_4mb` verified read/write mapping
                // at `0x3F80_0000`.
                unsafe {
                    esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                        EXTMEM_ORIGIN as *mut u8,
                        sz,
                        esp_alloc::MemoryCapability::External.into(),
                    ));
                }
                sz
            } else {
                0
            };
            esp_alloc::heap_allocator!(size: 51 * 1024);
            // JUSTIFICATION: Register secondary 14 KB internal `.dram2_uninit`
            // region as Region 2 of `esp_alloc::HEAP`.
            // SAFETY: `STATIC_BT_HEAP` is an exclusive static uninitialized
            // buffer in `.dram2_uninit` registered exactly once at boot.
            unsafe {
                esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                    (*core::ptr::addr_of_mut!(STATIC_BT_HEAP)).as_mut_ptr() as *mut u8,
                    core::mem::size_of_val(&*core::ptr::addr_of!(STATIC_BT_HEAP)),
                    esp_alloc::MemoryCapability::Internal.into(),
                ));
            }
            reg_size
        };
        let (bss_stream_storage, bss_output_storage) = ring_storage.split_at_mut(2048);
        // C parity: main/Kconfig.projbuild:465 (-b 500:2000): 500 KiB stream
        // buffer (512 KiB here) and a 2000 KiB output buffer. Both land in
        // the PSRAM region (the internal regions are far smaller); the 4 MiB
        // keeps ~1.2 MiB free for the FLAC/AAC/Vorbis/ALAC/Opus decoders and
        // the allocator.
        let (stream_storage, output_storage): (&'static mut [u8], &'static mut [u8]) =
            if psram_avail_bytes >= 1_048_576 {
                (
                    alloc::boxed::Box::leak(
                        alloc::vec![
                            0u8;
                            louder_esp32_app::pipeline::PSRAM_STREAM_RING_BYTES
                        ]
                        .into_boxed_slice(),
                    ),
                    alloc::boxed::Box::leak(
                        alloc::vec![
                            0u8;
                            louder_esp32_app::pipeline::PSRAM_OUTPUT_RING_BYTES
                        ]
                        .into_boxed_slice(),
                    ),
                )
            } else {
                (bss_stream_storage, bss_output_storage)
            };
        wdt.feed();
        squeezelite_decoders::prewarm_opus();
        wdt.feed();

        // Start Core 1 (APP_CPU @ 240 MHz) executing `app_core_loop` in IRAM
        // (`#[esp_hal::ram]`). Sync `DPORT_APP_FLASH_MMU_TABLE`
        // (0x3FF1_2000..0x3FF1_2400) and `DPORT_APP_CACHE_CTRL1_REG`
        // (0x3FF0_005C) from `PRO_CPU` (0x3FF1_0000 and 0x3FF0_0044) while
        // `APP_CPU` cache is disabled so `APP_CPU` can fetch
        // `start_core1_init` / `start_core1_run` from XIP Flash `.text`.
        #[allow(unsafe_code)]
        // JUSTIFICATION: Synchronize APP_CPU flash/PSRAM MMU tables and cache
        // control registers from PRO_CPU before starting APP_CPU.
        // SAFETY: APP_CPU is parked and its cache is disabled while MMU table
        // entries are copied.
        unsafe {
            const DPORT_PRO_CACHE_CTRL1_REG: *const u32 = 0x3FF0_0044 as *const u32;
            const DPORT_APP_CACHE_CTRL_REG: *mut u32 = 0x3FF0_0058 as *mut u32;
            const DPORT_APP_CACHE_CTRL1_REG: *mut u32 = 0x3FF0_005C as *mut u32;
            const DPORT_PRO_FLASH_MMU_TABLE: *const u32 = 0x3FF1_0000 as *const u32;
            const DPORT_APP_FLASH_MMU_TABLE: *mut u32 = 0x3FF1_2000 as *mut u32;

            let app_ctrl = core::ptr::read_volatile(DPORT_APP_CACHE_CTRL_REG);
            core::ptr::write_volatile(DPORT_APP_CACHE_CTRL_REG, app_ctrl & !(1 << 3));

            let pro_ctrl1 = core::ptr::read_volatile(DPORT_PRO_CACHE_CTRL1_REG);
            let app_ctrl1 = core::ptr::read_volatile(DPORT_APP_CACHE_CTRL1_REG);
            core::ptr::write_volatile(
                DPORT_APP_CACHE_CTRL1_REG,
                (app_ctrl1 & !0x3F) | (pro_ctrl1 & 0x3F),
            );

            for i in 0..256 {
                let entry = core::ptr::read_volatile(DPORT_PRO_FLASH_MMU_TABLE.add(i));
                core::ptr::write_volatile(DPORT_APP_FLASH_MMU_TABLE.add(i), entry);
            }
        }
        #[cfg(not(feature = "esp-rtos-core1"))]
        let core1 = {
            #[allow(unsafe_code)]
            let guard = cpu_ctrl.start_app_core(
                // JUSTIFICATION: Pass exclusive mutable reference to static
                // APP_CORE_STACK for Core 1 execution.
                // SAFETY: APP_CPU is started once at boot and owns
                // APP_CORE_STACK for the lifetime of the firmware.
                unsafe { &mut *core::ptr::addr_of_mut!(APP_CORE_STACK) },
                app_core_loop,
            );
            let active = guard.is_ok();
            Core1Ctl {
                cpu_ctrl,
                _guard: guard,
                active,
            }
        };

        // ESP32-WROVER (Louder-ESP32 Basement Target) pin mapping:
        // - GPIO33: TAS5805M PWDN
        // - GPIO34: TAS5805M FAULT (input only, external pull-up)
        // - I2C0: SDA=GPIO21, SCL=GPIO27
        // - I2S0: BCK=GPIO26, WS=GPIO25, DOUT=GPIO22
        // - UART2: TX=GPIO4, RX=GPIO36 (CYD Bridge)
        #[cfg(not(feature = "board-louder-esp32-s3"))]
        let mut pwdn = Output::new(peripherals.GPIO33, Level::High, OutputConfig::default());
        #[cfg(not(feature = "board-louder-esp32-s3"))]
        let fault = Input::new(
            peripherals.GPIO34,
            InputConfig::default().with_pull(Pull::None),
        );

        #[cfg(not(feature = "board-louder-esp32-s3"))]
        let amp = init_amp_i2c(peripherals.I2C0, peripherals.GPIO21, peripherals.GPIO27);
        #[cfg(not(feature = "board-louder-esp32-s3"))]
        let (pipeline, http_resp_buf, http_tx_buf) = init_i2s_pipeline(
            peripherals.I2S0,
            peripherals.DMA_I2S0,
            peripherals.GPIO26,
            peripherals.GPIO25,
            peripherals.GPIO22,
            stream_storage,
            output_storage,
        );
        #[cfg(not(feature = "board-louder-esp32-s3"))]
        let cyd_bridge = init_cyd_uart(peripherals.UART2, peripherals.GPIO4, peripherals.GPIO36);

        #[cfg(feature = "board-louder-esp32-s3")]
        compile_error!("ESP32-S3 board pin mapping pending hardware bring-up");

        // Execute 5-stage HIL self-verification
        pwdn.set_high();
        amp_hil_init(amp);
        let _ = fault.is_high();
        wdt.feed();

        let coordinator = init_coordinator(sta_mac, config, lms_ip);
        println!(
            "[HIL 1/5..5/5] Hardware Peripherals (GPIO33/34, I2C0 0x2D, \
             I2S0 26/25/22, UART2) Initialized"
        );

        // Apply the boot-resolved audio configuration (volume, AGAIN, EQ,
        // loudness).
        let current_vol_pct = amp_apply_boot_config(coordinator, amp, pipeline, config);
        let _ = cyd_bridge.send_message(&CydMessage::Volume(current_vol_pct));

        // Start `esp-rtos` scheduler required by `esp-radio`
        esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
        wdt.feed();

        #[cfg(feature = "esp-rtos-core1")]
        let core1 = {
            #[allow(unsafe_code)]
            // JUSTIFICATION: Pass exclusive mutable reference to static
            // APP_CORE_STACK for Core 1 executor.
            // SAFETY: APP_CPU is started once at boot and owns APP_CORE_STACK.
            let stack = unsafe { &mut *core::ptr::addr_of_mut!(APP_CORE_STACK) };
            esp_rtos::start_second_core(cpu_ctrl_periph, peripherals.FROM_CPU_INTR1, stack, || {
                crate::tasks::run_core1_executor()
            });
            Core1Ctl { active: true }
        };

        // Initialize `esp-radio` Wi-Fi Controller & Station Interface
        let wifi_controller = init_wifi(peripherals.WIFI, wifi_sta_creds);
        let mut sta_iface = WifiInterface::station();
        wdt.feed();

        let bt_periph = Some(peripherals.BT);
        #[allow(unsafe_code)]
        // JUSTIFICATION: Initialize static BtA2dpSink in `.dram2_uninit`
        // without creating a stack temporary.
        // SAFETY: Called once during single-core bring-up on PRO_CPU.
        let bt_sink: &'static mut BtA2dpSink = unsafe {
            let ptr = (*core::ptr::addr_of_mut!(STATIC_BT_SINK)).as_mut_ptr();
            ptr.write_bytes(0, 1);
            let sink = &mut *ptr;
            sink.reset_in_place();
            sink
        };

        let (slim_rx_slice, audio_rx_slice): (&'static mut [u8], &'static mut [u8]) =
            if psram_avail_bytes >= 655_360 {
                (
                    alloc::boxed::Box::leak(alloc::vec![0u8; 8_192].into_boxed_slice()),
                    alloc::boxed::Box::leak(alloc::vec![0u8; 65_536].into_boxed_slice()),
                )
            } else {
                (&mut slim_rx_buf[..], &mut audio_rx_buf[..])
            };

        let NetStack {
            smol_iface,
            sockets,
            dhcp_handle,
            http_handle,
            slim_handle,
            audio_handle,
            mdns_handle,
            rtsp_handle,
            rtp_handle,
            spot_handle,
            local_port_counter,
        } = init_net_stack(
            &mut sta_iface,
            sta_mac,
            [
                (&mut http_rx_buf[..], &mut http_tx_buf[..]),
                (slim_rx_slice, &mut slim_tx_buf[..]),
                (audio_rx_slice, &mut audio_tx_buf[..]),
            ],
        );

        let boot_ms = now_millis();
        pipeline.sink.driver.pump_silence_if_needed(false, false);
        let latest_telemetry = amp_start_playback(amp);
        let config_writer = ConfigWriter::new(config, boot_ms);
        let raop_rtsp = alloc::boxed::Box::new(RtspServer::new(
            airport_express_key(),
            sta_mac,
            [0, 0, 0, 0],
        ));
        let raop_rtp = alloc::boxed::Box::new(RtpAudioReceiver::new());
        let spot_spirc = alloc::boxed::Box::new(SpircStateMachine::new());

        alloc::boxed::Box::leak(alloc::boxed::Box::new(DeviceRuntimeState {
            wdt,
            flash,
            core1,
            _pwdn: pwdn,
            _fault: fault,
            wifi_controller,
            sta_iface,
            smol_iface,
            sockets,
            dhcp_handle,
            http_handle,
            slim_handle,
            audio_handle,
            mdns_handle,
            rtsp_handle,
            rtp_handle,
            spot_handle,
            bt_periph,
            bt_transport: None,
            bt_sink,
            amp,
            cyd_bridge,
            pipeline,
            coordinator,
            slim_link: SlimLinkState::new(),
            stream_state: HttpStreamState::new(),
            http_resp_buf,
            config,
            journal,
            config_writer,
            wifi_sta_creds,
            sta_mac,
            lms_ip,
            lms_port,
            current_vol_pct,
            local_port_counter,
            bt_audio_active: false,
            boot_ms,
            lease_watchdog: DeadManLeaseWatchdog::new(boot_ms),
            assigned_ip: None,
            assigned_mask: [255, 255, 255, 0],
            assigned_gw: [172, 22, 1, 1],
            pending_http_action: HttpControlAction::None,
            pending_action_deadline_ms: 0,
            pending_http_body: &[],
            pending_http_started_ms: 0,
            pending_http_closing: false,
            pending_close_deadline_ms: 0,
            helo_sent: false,
            last_connect_attempt_ms: 0,
            slim_reconnect: SlimReconnect::new(),
            slim_was_open: false,
            slim_switch_to: None,
            last_fault_check_ms: 0,
            latest_telemetry,
            last_cyd_progress_ms: 0,
            last_config_poll_ms: 0,
            mdns_bound: false,
            last_mdns_announce_ms: 0,
            raop_rtsp,
            raop_rtp,
            raop_streaming: false,
            last_rtp_packet_ms: 0,
            rtsp_rx_acc: alloc::vec::Vec::with_capacity(2048),
            spot_spirc,
            spot_streaming: false,
            now_ms: 0,
            wifi_connected_logged: false,
            last_fault_recovery_ms: 0,
            last_bt_idle_poll_ms: 0,
        }))
    }

    struct DeviceSmolTcp<'a> {
        sock: &'a mut tcp::Socket<'static>,
        connect: Option<(&'a mut SmolContext, [u8; 4], &'a mut u16)>,
    }

    impl<'a> DeviceSmolTcp<'a> {
        fn new(sock: &'a mut tcp::Socket<'static>) -> Self {
            Self {
                sock,
                connect: None,
            }
        }
    }

    fn next_local_port(counter: &mut u16) -> u16 {
        *counter = 49152 + ((counter.wrapping_add(1) - 49152) & 0x3FFF);
        *counter
    }

    impl TcpSocketOps for DeviceSmolTcp<'_> {
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
            let local_ep = (
                IpAddress::Ipv4(Ipv4Address::from(*local_ip)),
                next_local_port(port),
            );
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

    #[inline(never)]
    fn flush_settings_before_reboot(s: &mut DeviceRuntimeState) {
        s.core1.park_for_flash_write();
        let _ = s.amp.set_mute(true);
        s.config.flush_now(&mut *s.journal);
        if let Some(image) = s.journal.pending_image() {
            #[cfg(not(feature = "runtime-flash-persist"))]
            let offset = SETTINGS_JOURNAL_OFFSET;
            #[cfg(feature = "runtime-flash-persist")]
            let offset = image.slot.offset(SETTINGS_JOURNAL_OFFSET);
            let _ = s.flash.erase(offset, offset + 4096);
            let _ = s.flash.write_nor(offset, &image.sector[..image.used_len]);
        }
    }

    fn backoff_seed(now_ms: u64) -> u32 {
        (now_ms as u32).wrapping_mul(0x9E37_79B9) ^ (now_ms >> 32) as u32
    }

    fn restore_suspended_or_stop(s: &mut DeviceRuntimeState) {
        match s.coordinator.active_source() {
            AudioSource::SlimProto => {
                s.coordinator.end_external_playback_and_restore(s.pipeline);
                s.pipeline.set_mute(false);
            }
            AudioSource::None => {
                s.pipeline.use_mp3_decoder();
                s.coordinator.end_external_playback();
            }
            AudioSource::AirPlay | AudioSource::Spotify | AudioSource::Bluetooth => {
                s.pipeline.stream_buffer.flush();
            }
        }
    }

    #[inline(never)]
    fn bt_poll_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
        if s.assigned_ip.is_some() && s.bt_transport.is_none() {
            if let Some(bt_p) = s.bt_periph.take() {
                s.wdt.feed();
                s.bt_transport =
                    match esp_radio::ble::controller::BleConnector::new(bt_p, Default::default()) {
                        Ok(c) => Some(Esp32VhciTransport { connector: c }),
                        Err(_) => None,
                    };
                s.wdt.feed();
            }
        }

        if let Some(ref mut transport) = s.bt_transport {
            if !s.bt_sink.is_connected() && !s.bt_sink.is_streaming() {
                if now_ms.saturating_sub(s.last_bt_idle_poll_ms) < 2 {
                    return;
                }
                s.last_bt_idle_poll_ms = now_ms;
            }

            s.bt_sink
                .poll(transport, now_ms, |pcm_bytes, sample_rate_hz| {
                    if s.coordinator.request_source(AudioSource::Bluetooth) {
                        if s.coordinator.current_sample_rate != sample_rate_hz
                            || s.pipeline.sample_rate != sample_rate_hz
                        {
                            s.coordinator.current_sample_rate = sample_rate_hz;
                            let _ = s.pipeline.set_sample_rate(sample_rate_hz);
                        }
                        if !s.bt_audio_active {
                            s.pipeline
                                .set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)));
                            s.pipeline.begin_external_audio();
                            s.pipeline.set_mute(false);
                            s.coordinator.begin_external_playback();
                            if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                                let _ = s.amp.start_playback_via_hiz();
                            }
                            s.bt_audio_active = true;
                        }
                        let avail = s.pipeline.stream_buffer.available_write() & !3;
                        let aligned_len = (pcm_bytes.len() & !3).min(avail);
                        if aligned_len > 0 {
                            let _ = s.pipeline.ingest_stream_data(&pcm_bytes[..aligned_len]);
                        }
                    }
                });

            if s.bt_audio_active && !s.bt_sink.is_streaming() {
                if s.pipeline.stream_buffer.available_read() < 4
                    && s.pipeline.output_buffer.available_read() == 0
                    && !s.pipeline.has_pending_pcm()
                {
                    if s.pipeline.stream_buffer.available_read() > 0 {
                        s.pipeline.stream_buffer.flush();
                    }
                    s.bt_audio_active = false;
                    s.coordinator.release_source(AudioSource::Bluetooth);
                    restore_suspended_or_stop(s);
                }
            }
        }
    }

    #[inline(never)]
    fn wifi_and_lease_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
        let wifi_connected = s.wifi_controller.is_connected();
        if wifi_connected {
            if !s.wifi_connected_logged {
                let _ = s.wifi_controller.set_power_saving(PowerSaveMode::None);
                s.wifi_connected_logged = true;
            }
        } else {
            s.wifi_connected_logged = false;
        }

        if !wifi_connected
            && (s.last_connect_attempt_ms == 0
                || now_ms.saturating_sub(s.last_connect_attempt_ms) >= 5_000)
        {
            s.last_connect_attempt_ms = now_ms.max(1);
            let mut connect_fut = pin!(s.wifi_controller.connect_async());
            let waker = Waker::noop();
            let mut cx = Context::from_waker(waker);
            let _ = connect_fut.as_mut().poll(&mut cx);
        }

        if s.lease_watchdog
            .evaluate(now_ms, wifi_connected, s.assigned_ip.is_some())
            == LeaseHealth::ExpiredResetToRecovery
        {
            println!(
                "[ANTI-BRICK GATE 3] Network lease timeout expired -> \
                 resetting to recovery (0x10000)"
            );
            esp_hal::system::software_reset();
        }
    }

    #[inline(never)]
    fn net_poll_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
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

        let dhcp_event = s.sockets.get_mut::<dhcpv4::Socket>(s.dhcp_handle).poll();
        match dhcp_event {
            Some(dhcpv4::Event::Configured(config)) => {
                let cfg_address = config.address;
                let cfg_router = config.router;
                let ip_octets = cfg_address.address().octets();
                let ip_changed = s.assigned_ip.map_or(false, |old| old != ip_octets);
                if ip_changed {
                    s.sockets.get_mut::<tcp::Socket>(s.slim_handle).abort();
                    s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                    s.sockets.get_mut::<tcp::Socket>(s.http_handle).abort();
                    s.sockets.get_mut::<tcp::Socket>(s.rtsp_handle).abort();
                    s.sockets.get_mut::<tcp::Socket>(s.spot_handle).abort();
                    s.sockets.get_mut::<udp::Socket>(s.mdns_handle).close();
                    s.sockets.get_mut::<udp::Socket>(s.rtp_handle).close();
                    s.mdns_bound = false;
                    s.raop_streaming = false;
                    s.spot_streaming = false;
                    s.rtsp_rx_acc.clear();
                    s.helo_sent = false;
                    s.pending_http_body = &[];
                    s.pending_http_closing = false;
                }
                s.assigned_ip = Some(ip_octets);
                s.raop_rtsp.set_ip_address(ip_octets);
                let _ = s
                    .smol_iface
                    .join_multicast_group(Ipv4Address::new(224, 0, 0, 251));
                let prefix_len = cfg_address.prefix_len();
                let mask_u32 = if prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - prefix_len)
                };
                s.assigned_mask = mask_u32.to_be_bytes();
                if let Some(router) = cfg_router {
                    s.assigned_gw = router.octets();
                    let _ = s.smol_iface.routes_mut().add_default_ipv4_route(router);
                }
                s.smol_iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(cfg_address));
                });
                println!(
                    "[DHCPv4] Acquired IP {}.{}.{}.{}/{}",
                    ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3], prefix_len
                );
            }
            Some(dhcpv4::Event::Deconfigured) => {
                s.assigned_ip = None;
                s.smol_iface.update_ip_addrs(|addrs| addrs.clear());
                let _ = s
                    .smol_iface
                    .leave_multicast_group(Ipv4Address::new(224, 0, 0, 251));
                s.sockets.get_mut::<tcp::Socket>(s.slim_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.audio_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.http_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.rtsp_handle).abort();
                s.sockets.get_mut::<tcp::Socket>(s.spot_handle).abort();
                s.sockets.get_mut::<udp::Socket>(s.mdns_handle).close();
                s.sockets.get_mut::<udp::Socket>(s.rtp_handle).close();
                s.mdns_bound = false;
                s.raop_streaming = false;
                s.spot_streaming = false;
                s.rtsp_rx_acc.clear();
                s.helo_sent = false;
                s.pending_http_body = &[];
                s.pending_http_closing = false;
            }
            None => {}
        }
    }

    #[inline(never)]
    fn http_serve_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
        {
            let http_sock = s.sockets.get_mut::<tcp::Socket>(s.http_handle);

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
                let mut req_buf = [0u8; 1024];
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
                    let codec_name = match s.coordinator.active_source() {
                        AudioSource::AirPlay => "AirPlay",
                        AudioSource::Bluetooth => "Bluetooth",
                        AudioSource::Spotify => "Spotify",
                        _ => {
                            if s.raop_streaming {
                                "AirPlay"
                            } else if s.spot_streaming {
                                "Spotify"
                            } else {
                                match s.pipeline.decoder {
                                    ActiveDecoder::Mp3(_) => "MP3",
                                    ActiveDecoder::Flac(_) => "FLAC",
                                    ActiveDecoder::Pcm(_) => "PCM",
                                    ActiveDecoder::Aac(_) => "AAC",
                                    ActiveDecoder::Vorbis(_) => "Vorbis",
                                    ActiveDecoder::Alac(_) => "ALAC",
                                    ActiveDecoder::Opus(_) => "Opus",
                                }
                            }
                        }
                    };
                    let (tas_fs_hz, tas_bck_ratio, tas_pll_locked, tas_automuted) = match s
                        .latest_telemetry
                    {
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
                        let mut free = 0u32;
                        let mut used = 0u32;
                        for r in stats.region_stats.iter().flatten() {
                            if r.size > 1024 * 1024 {
                                free = r.free as u32;
                                used = r.used as u32;
                                break;
                            }
                        }
                        if free.saturating_add(used) > 200_000 {
                            (free, used)
                        } else {
                            (
                                louder_esp32_app::http_control::DEFAULT_PSRAM_FREE_BYTES,
                                louder_esp32_app::http_control::DEFAULT_PSRAM_USED_BYTES,
                            )
                        }
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
                        decode_us_max: s.pipeline.decode_us_max,
                        decode_us_first: s.pipeline.decode_us_first,
                    };
                    if let Some((hdr_or_resp_len, static_body, action)) =
                        handle_http_request_streaming(
                            &req_buf[..n],
                            &snap,
                            &mut s.http_resp_buf[..],
                        )
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
                            HttpControlAction::PlayerIrCommand {
                                ir_code,
                                vol_pct,
                                mute,
                            } => {
                                if let Some(pct) = vol_pct {
                                    s.current_vol_pct = pct;
                                    let mut backend = Esp32AudioBackend::new(
                                        s.amp,
                                        DeviceClockRegs,
                                        &DMA_OUTSTANDING_FRAMES,
                                    );
                                    s.coordinator
                                        .set_volume_percent(pct, &mut backend, s.pipeline);
                                    let _ = s.cyd_bridge.send_message(&CydMessage::Volume(pct));
                                    s.config.update(|c| c.volume = pct);
                                }
                                if let Some(m) = mute {
                                    s.pipeline.set_mute(m);
                                }
                                if ir_code != 0 && s.helo_sent {
                                    let mut ir_buf = [0u8; 32];
                                    if let Ok(ir_len) = SlimprotoBuilder::build_ir(
                                        now_ms as u32,
                                        ir_code,
                                        &mut ir_buf,
                                    ) {
                                        let slim_sock =
                                            s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
                                        let mut link = SmoltcpSlimTransport::new(
                                            DeviceSmolTcp::new(slim_sock),
                                            &mut s.slim_link,
                                        );
                                        let _ = link.send(&ir_buf[..ir_len]);
                                    }
                                }
                            }
                            HttpControlAction::SpotifyPlay => {
                                if s.coordinator.request_source(AudioSource::Spotify) {
                                    if s.coordinator.current_sample_rate != 44_100
                                        || s.pipeline.sample_rate != 44_100
                                    {
                                        s.coordinator.current_sample_rate = 44_100;
                                        let _ = s.pipeline.set_sample_rate(44_100);
                                    }
                                    s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(
                                        16, 2, false,
                                    )));
                                    s.pipeline.begin_external_audio();
                                    s.pipeline.set_mute(false);
                                    s.coordinator.begin_external_playback();
                                    if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                                        let _ = s.amp.clear_faults();
                                        let _ = s.amp.start_playback_via_hiz();
                                    }
                                    s.spot_streaming = true;
                                }
                            }
                            HttpControlAction::SpotifyStop => {
                                s.spot_streaming = false;
                                s.coordinator.release_source(AudioSource::Spotify);
                                restore_suspended_or_stop(s);
                            }
                            HttpControlAction::RebootToRecovery
                            | HttpControlAction::PersistOta0AndReboot => {
                                s.pending_http_action = action;
                                s.pending_action_deadline_ms = now_ms + 250;
                            }
                            HttpControlAction::UpdateConfig => {
                                s.config.flush_now(&mut *s.journal);
                            }
                            HttpControlAction::ExecuteCliCommand
                            | HttpControlAction::TriggerWifiScan
                            | HttpControlAction::ConnectWifi
                            | HttpControlAction::StartOtaFlash => {}
                            _ => {}
                        }
                    } else {
                        http_sock.close();
                        s.pending_http_closing = true;
                        s.pending_close_deadline_ms = now_ms + 250;
                    }
                }
            }
        }

        if s.pending_http_action != HttpControlAction::None
            && now_ms >= s.pending_action_deadline_ms
        {
            let action = core::mem::replace(&mut s.pending_http_action, HttpControlAction::None);
            flush_settings_before_reboot(s);
            match action {
                HttpControlAction::RebootToRecovery => {
                    println!(
                        "[ANTI-BRICK GATE 4] HTTP /recovery.json or /reboot.json \
                         -> resetting into factory recovery (0x10000)"
                    );
                    esp_hal::system::software_reset();
                }
                HttpControlAction::PersistOta0AndReboot => {
                    let entry_bytes = OtaSelectEntry::new_ota_0().to_bytes();
                    let mut verify_buf = [0u8; 32];
                    if s.flash
                        .write_nor(OTADATA_PARTITION_OFFSET, &entry_bytes)
                        .is_ok()
                        && s.flash
                            .read_nor(OTADATA_PARTITION_OFFSET, &mut verify_buf)
                            .is_ok()
                        && verify_buf == entry_bytes
                    {
                        println!(
                            "[ANTI-BRICK GATE 4] HTTP /reboot_ota.json -> \
                             wrote ota_0 select entry & resetting"
                        );
                        esp_hal::system::software_reset();
                    }
                }
                _ => {}
            }
        }
    }

    #[inline(never)]
    fn slim_session_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
        let Some(local_ip) = s.assigned_ip else {
            return;
        };
        let slim_sock = s.sockets.get_mut::<tcp::Socket>(s.slim_handle);
        if let Some(ip) = s.slim_switch_to.take() {
            slim_sock.abort();
            s.slim_reconnect
                .server_switch(now_ms, s.lms_ip, backoff_seed(now_ms));
            s.slim_was_open = false;
            s.helo_sent = false;
            s.lms_ip = ip;
            s.coordinator.lms_ip = ip;
        }
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
            if slim_sock
                .connect(s.smol_iface.context(), remote_ep, local_ep)
                .is_ok()
            {
                s.slim_reconnect.connect_started();
                s.slim_was_open = true;
            }
        }
        slim_tick(s, local_ip);
    }

    #[inline(never)]
    fn slim_tick(s: &mut DeviceRuntimeState, local_ip: [u8; 4]) {
        let now_ms = s.now_ms as u32;
        let DeviceRuntimeState {
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

        let mut transport =
            SmoltcpSlimTransport::new(DeviceSmolTcp::new(slim_sock), &mut *slim_link);
        if transport.connected() && !*helo_sent {
            *helo_sent = coordinator
                .hello(
                    &mut transport,
                    HELO_DEVICE_ID,
                    slim_reconnect.reconnect_flag(),
                )
                .is_ok();
            if !*helo_sent {
                return;
            }
            slim_reconnect.session_up(lap_ms);
        }
        let mut source = SmoltcpStreamSource::new(
            DeviceSmolTcp {
                sock: audio_sock,
                connect: Some((smol_iface.context(), local_ip, local_port_counter)),
            },
            &mut *stream_state,
        );
        let mut backend = Esp32AudioBackend::new(amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
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

    #[inline(never)]
    fn multi_proto_poll(s: &mut DeviceRuntimeState) {
        let Some(local_ip) = s.assigned_ip else {
            return;
        };
        let now_ms = s.now_ms;

        // 1. mDNS responder (`UDP 5353`) for AirPlay `_raop._tcp.local` &
        // Spotify `_spotify-connect._tcp.local`
        {
            let mdns_sock = s.sockets.get_mut::<udp::Socket>(s.mdns_handle);
            if !s.mdns_bound {
                if mdns_sock.bind(5353).is_ok() {
                    s.mdns_bound = true;
                }
            }
            if s.mdns_bound {
                let mut should_announce_raop = false;
                let mut should_announce_spotify = false;
                let mut raop_ep = IpEndpoint::new(IpAddress::v4(224, 0, 0, 251), 5353);
                let mut spot_ep = IpEndpoint::new(IpAddress::v4(224, 0, 0, 251), 5353);

                if s.last_mdns_announce_ms == 0
                    || now_ms.saturating_sub(s.last_mdns_announce_ms) >= 30_000
                {
                    s.last_mdns_announce_ms = now_ms.max(1);
                    should_announce_raop = true;
                    should_announce_spotify = true;
                }

                while mdns_sock.can_recv() {
                    if let Ok((payload, meta)) = mdns_sock.recv() {
                        if payload.windows(5).any(|w| w == b"_raop")
                            || payload.windows(8).any(|w| w == b"_airplay")
                        {
                            should_announce_raop = true;
                            if meta.endpoint.port != 5353 {
                                raop_ep = meta.endpoint;
                            }
                        } else if payload.windows(16).any(|w| w == b"_spotify-connect")
                            || payload.windows(8).any(|w| w == b"_spotify")
                        {
                            should_announce_spotify = true;
                            if meta.endpoint.port != 5353 {
                                spot_ep = meta.endpoint;
                            }
                        }
                    } else {
                        break;
                    }
                }

                if should_announce_raop && mdns_sock.can_send() {
                    let mcfg = MdnsConfig {
                        player_name: alloc::string::String::from("BASEMENT"),
                        mac_address: s.sta_mac,
                        ip_address: local_ip,
                        port: 5000,
                    };
                    let rpkt = MdnsBuilder::build_announcement(&mcfg);
                    let _ = mdns_sock.send_slice(&rpkt, raop_ep);
                }
                if should_announce_spotify && mdns_sock.can_send() {
                    let scfg = SpotifyMdnsConfig {
                        player_name: alloc::string::String::from("BASEMENT"),
                        mac_address: s.sta_mac,
                        ip_address: local_ip,
                        port: 80,
                    };
                    let spkt = SpotifyMdnsBuilder::build_announcement(&scfg);
                    let _ = mdns_sock.send_slice(&spkt, spot_ep);
                }
            }
        }

        // 2. AirPlay 1 RAOP RTSP Control (`TCP 5000`) & RTP Audio (`UDP 6000`)
        {
            s.raop_rtsp.set_ip_address(local_ip);
            let rtsp_sock = s.sockets.get_mut::<tcp::Socket>(s.rtsp_handle);
            if rtsp_sock.is_open()
                && matches!(
                    rtsp_sock.state(),
                    tcp::State::CloseWait
                        | tcp::State::TimeWait
                        | tcp::State::Closing
                        | tcp::State::LastAck
                        | tcp::State::FinWait1
                        | tcp::State::FinWait2
                )
            {
                rtsp_sock.abort();
                s.rtsp_rx_acc.clear();
            }
            if !rtsp_sock.is_open() {
                let _ = rtsp_sock.listen(5000);
                s.rtsp_rx_acc.clear();
            }
            if rtsp_sock.can_recv() && rtsp_sock.may_send() {
                let mut chunk = [0u8; 512];
                if let Ok(n) = rtsp_sock.recv_slice(&mut chunk) {
                    if n > 0 {
                        s.rtsp_rx_acc.extend_from_slice(&chunk[..n]);
                        if s.rtsp_rx_acc.len() > 4096 {
                            s.rtsp_rx_acc.clear();
                        }
                        if s.rtsp_rx_acc.windows(4).any(|w| w == b"\r\n\r\n") {
                            let parsed = if let Ok(req) = RtspRequest::parse(&s.rtsp_rx_acc) {
                                let body_start = if req.body.is_empty() {
                                    s.rtsp_rx_acc.len()
                                } else {
                                    req.body.as_ptr() as usize - s.rtsp_rx_acc.as_ptr() as usize
                                };
                                if s.rtsp_rx_acc.len() >= body_start + req.content_length {
                                    let method = req.method;
                                    let resp_res = s.raop_rtsp.handle_request(&req);
                                    Some((method, resp_res))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some((method, resp_res)) = parsed {
                                if let Ok(resp) = resp_res {
                                    let resp_bytes = resp.to_bytes();
                                    let _ = rtsp_sock.send_slice(&resp_bytes);
                                    match method {
                                        RtspMethod::Announce => {
                                            if let (Some(key), Some(iv)) =
                                                (s.raop_rtsp.session_key(), s.raop_rtsp.aes_iv())
                                            {
                                                s.raop_rtp.configure_crypto(key, iv);
                                            } else {
                                                s.raop_rtp.disable_crypto();
                                            }
                                            if let Some(sdp) = s.raop_rtsp.sdp_session() {
                                                if let Some(ref fmtp) = sdp.fmtp {
                                                    let _ = s.raop_rtp.configure_alac(fmtp);
                                                }
                                            }
                                        }
                                        RtspMethod::Record => {
                                            if s.coordinator.request_source(AudioSource::AirPlay) {
                                                if let (Some(key), Some(iv)) = (
                                                    s.raop_rtsp.session_key(),
                                                    s.raop_rtsp.aes_iv(),
                                                ) {
                                                    s.raop_rtp.configure_crypto(key, iv);
                                                }
                                                if let Some(sdp) = s.raop_rtsp.sdp_session() {
                                                    if let Some(ref fmtp) = sdp.fmtp {
                                                        let _ = s.raop_rtp.configure_alac(fmtp);
                                                    }
                                                }
                                                s.pipeline.set_decoder(ActiveDecoder::Pcm(
                                                    PcmDecoder::new(16, 2, false),
                                                ));
                                                s.pipeline.begin_external_audio();
                                                s.pipeline.set_mute(false);
                                                s.coordinator.begin_external_playback();
                                                s.raop_streaming = true;
                                                s.last_rtp_packet_ms = now_ms;
                                            }
                                        }
                                        RtspMethod::SetParameter => {
                                            let pct =
                                                ((s.raop_rtsp.volume() * 100.0) as u8).min(100);
                                            s.current_vol_pct = pct;
                                            let mut backend = Esp32AudioBackend::new(
                                                s.amp,
                                                DeviceClockRegs,
                                                &DMA_OUTSTANDING_FRAMES,
                                            );
                                            s.coordinator.set_volume_percent(
                                                pct,
                                                &mut backend,
                                                s.pipeline,
                                            );
                                            let _ =
                                                s.cyd_bridge.send_message(&CydMessage::Volume(pct));
                                            s.config.update(|c| c.volume = pct);
                                        }
                                        RtspMethod::Flush => {
                                            s.pipeline.stream_buffer.flush();
                                            s.pipeline.output_buffer.flush();
                                        }
                                        RtspMethod::Teardown => {
                                            s.raop_streaming = false;
                                            s.coordinator.release_source(AudioSource::AirPlay);
                                            restore_suspended_or_stop(s);
                                        }
                                        _ => {}
                                    }
                                }
                                s.rtsp_rx_acc.clear();
                            }
                        }
                    }
                }
            }
        }
        {
            let rtp_sock = s.sockets.get_mut::<udp::Socket>(s.rtp_handle);
            if !rtp_sock.is_open() {
                let _ = rtp_sock.bind(6000);
            }
            if rtp_sock.can_recv() {
                let mut pkt_buf = [0u8; 1536];
                let mut bursts = 0u8;
                while bursts < 4 && rtp_sock.can_recv() {
                    bursts += 1;
                    if let Ok((n, _ep)) = rtp_sock.recv_slice(&mut pkt_buf) {
                        s.last_rtp_packet_ms = now_ms;
                        if let Ok((pcm_i16, _hdr)) = s.raop_rtp.process_packet(&pkt_buf[..n]) {
                            if s.coordinator.request_source(AudioSource::AirPlay) {
                                if !s.raop_streaming {
                                    if s.coordinator.current_sample_rate != 44_100
                                        || s.pipeline.sample_rate != 44_100
                                    {
                                        s.coordinator.current_sample_rate = 44_100;
                                        let _ = s.pipeline.set_sample_rate(44_100);
                                    }
                                    s.pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(
                                        16, 2, false,
                                    )));
                                    s.pipeline.begin_external_audio();
                                    s.pipeline.set_mute(false);
                                    s.coordinator.begin_external_playback();
                                    if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                                        let _ = s.amp.clear_faults();
                                        let _ = s.amp.start_playback_via_hiz();
                                    }
                                    s.raop_streaming = true;
                                }
                                let pcm_bytes: &[u8] = bytemuck::cast_slice(pcm_i16);
                                let _ = s.pipeline.ingest_stream_data(pcm_bytes);
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
            if s.raop_streaming
                && (s.raop_rtsp.state() != ServerState::Streaming
                    || now_ms.saturating_sub(s.last_rtp_packet_ms) >= 3_000)
            {
                if s.pipeline.stream_buffer.available_read() == 0
                    && s.pipeline.output_buffer.available_read() == 0
                    && !s.pipeline.has_pending_pcm()
                {
                    s.raop_streaming = false;
                    s.coordinator.release_source(AudioSource::AirPlay);
                    restore_suspended_or_stop(s);
                }
            }
        }

        // 3. Spotify Connect ZeroConf HTTP & Spirc (`TCP 4070`)
        {
            let spot_sock = s.sockets.get_mut::<tcp::Socket>(s.spot_handle);
            if spot_sock.is_open()
                && matches!(
                    spot_sock.state(),
                    tcp::State::CloseWait
                        | tcp::State::TimeWait
                        | tcp::State::Closing
                        | tcp::State::LastAck
                        | tcp::State::FinWait1
                        | tcp::State::FinWait2
                )
            {
                spot_sock.abort();
            }
            if !spot_sock.is_open() {
                let _ = spot_sock.listen(4070);
            }
            if spot_sock.can_recv() && spot_sock.may_send() {
                let mut req_buf = [0u8; 1024];
                if let Ok(n) = spot_sock.recv_slice(&mut req_buf) {
                    if n > 0 {
                        if req_buf[..n].starts_with(b"GET ") || req_buf[..n].starts_with(b"POST ") {
                            let body = b"{\"status\":101,\"statusString\":\"OK\",\
                                \"spotifyError\":0,\"version\":\"2.9.0\",\
                                \"deviceID\":\"c82e18beb8340000000000000000000000000000\",\
                                \"remoteName\":\"BASEMENT\",\"activeUser\":\"\",\
                                \"publicKey\":\"\",\"deviceType\":\"SPEAKER\",\
                                \"libraryVersion\":\"0.1.0\",\"accountReq\":\"PREMIUM\",\
                                \"brandDisplayName\":\"Squeezelite\",\
                                \"modelDisplayName\":\"ESP32\"}";
                            let hdr = b"HTTP/1.1 200 OK\r\n\
                                Content-Type: application/json\r\n\
                                Connection: close\r\n\r\n";
                            let _ = spot_sock.send_slice(hdr);
                            let _ = spot_sock.send_slice(body);
                            spot_sock.close();
                        } else if let Ok(frame) = SpircFrame::parse(&req_buf[..n]) {
                            if let Some(evt) = s.spot_spirc.handle_frame(&frame) {
                                match evt {
                                    SpircEvent::Play | SpircEvent::LoadTrack { .. } => {
                                        if s.coordinator.request_source(AudioSource::Spotify) {
                                            if !s.spot_streaming {
                                                if s.coordinator.current_sample_rate != 44_100
                                                    || s.pipeline.sample_rate != 44_100
                                                {
                                                    s.coordinator.current_sample_rate = 44_100;
                                                    let _ = s.pipeline.set_sample_rate(44_100);
                                                }
                                                s.pipeline.set_decoder(ActiveDecoder::Pcm(
                                                    PcmDecoder::new(16, 2, false),
                                                ));
                                                s.pipeline.begin_external_audio();
                                                s.pipeline.set_mute(false);
                                                s.coordinator.begin_external_playback();
                                                if s.amp.mode() != louder_esp32_app::AmpMode::Play {
                                                    let _ = s.amp.clear_faults();
                                                    let _ = s.amp.start_playback_via_hiz();
                                                }
                                                s.spot_streaming = true;
                                            }
                                            let notify = s
                                                .spot_spirc
                                                .build_notify_frame(frame.seq_nr, "BASEMENT");
                                            let _ = spot_sock.send_slice(&notify);
                                        }
                                    }
                                    SpircEvent::Pause => {
                                        s.spot_streaming = false;
                                        s.coordinator.release_source(AudioSource::Spotify);
                                        restore_suspended_or_stop(s);
                                    }
                                    SpircEvent::SetVolume(vol_u32) => {
                                        let pct = ((vol_u32.min(65_536) * 100) / 65_536) as u8;
                                        s.current_vol_pct = pct;
                                        let mut backend = Esp32AudioBackend::new(
                                            s.amp,
                                            DeviceClockRegs,
                                            &DMA_OUTSTANDING_FRAMES,
                                        );
                                        s.coordinator.set_volume_percent(
                                            pct,
                                            &mut backend,
                                            s.pipeline,
                                        );
                                        let _ = s.cyd_bridge.send_message(&CydMessage::Volume(pct));
                                        s.config.update(|c| c.volume = pct);
                                    }
                                    SpircEvent::Next
                                    | SpircEvent::Prev
                                    | SpircEvent::Seek(_)
                                    | SpircEvent::Notify
                                    | SpircEvent::TrackEnded
                                    | SpircEvent::MetadataUpdated(_) => {}
                                }
                            }
                        } else if s.spot_streaming {
                            let _ = s.pipeline.ingest_stream_data(&req_buf[..n]);
                        }
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn cyd_command_phase(s: &mut DeviceRuntimeState) {
        if let Ok(Some(cmd)) = s.cyd_bridge.poll_command() {
            let mut backend =
                Esp32AudioBackend::new(s.amp, DeviceClockRegs, &DMA_OUTSTANDING_FRAMES);
            if let Some(reply) = s
                .coordinator
                .handle_cyd_command(cmd, &mut backend, s.pipeline)
            {
                if let CydMessage::Volume(pct) = reply {
                    s.config.update(|c| c.volume = pct.min(100));
                }
                let _ = s.cyd_bridge.flush_outbound(&reply);
            }
        }
    }

    #[esp_hal::ram]
    fn output_decode_phase(s: &mut DeviceRuntimeState) {
        s.coordinator.pump_output(s.pipeline, 16);
        let max_steps = match s.pipeline.decoder {
            ActiveDecoder::Pcm(_) => 4,
            _ => 16,
        };
        s.coordinator.pump_decode(s.pipeline, max_steps);
        s.coordinator.pump_output(s.pipeline, 16);
        s.coordinator.current_sample_rate = s.pipeline.sample_rate;

        if !s.coordinator.dac_enabled {
            let _ = s.amp.stop_playback();
        }

        let has_buffered_audio =
            s.pipeline.output_buffer.available_read() >= 8 || s.pipeline.has_pending_pcm();
        s.pipeline.sink.driver.pump_silence_if_needed(
            s.coordinator.state == PlayerState::Playing || s.coordinator.external_playback,
            has_buffered_audio,
        );
    }

    #[inline(never)]
    fn amp_and_cyd_health_phase(s: &mut DeviceRuntimeState) {
        let now_ms = s.now_ms;
        if now_ms.saturating_sub(s.last_fault_check_ms) >= 2_000 {
            s.last_fault_check_ms = now_ms;
            if let Ok(Some((fault, telem))) = s.amp.check_faults_and_telemetry() {
                s.latest_telemetry = Some(telem);
                if (s.coordinator.state == PlayerState::Playing || s.coordinator.external_playback)
                    && fault.has_critical_amp_fault()
                    && now_ms.saturating_sub(s.last_fault_recovery_ms) >= 30_000
                {
                    s.last_fault_recovery_ms = now_ms;
                    let _ = s.amp.recover_from_fault();
                }
            }
        }

        if now_ms.saturating_sub(s.last_cyd_progress_ms) >= 1_000 {
            s.last_cyd_progress_ms = now_ms;
            if s.coordinator.state == PlayerState::Playing && s.coordinator.current_sample_rate > 0
            {
                let elapsed_secs = (s.coordinator.frames_for_stat(s.pipeline)
                    / s.coordinator.current_sample_rate as u64)
                    as u32;
                let _ = s.cyd_bridge.send_message(&CydMessage::Progress {
                    elapsed_secs,
                    duration_secs: 0,
                });
            }
        }

        if now_ms.saturating_sub(s.last_config_poll_ms) >= 100 {
            s.last_config_poll_ms = now_ms;
            s.config_writer.poll(s.config, &mut *s.journal, now_ms);
        }
    }

    #[inline(never)]
    fn pump_device_phase(s: &mut DeviceRuntimeState) {
        s.wdt.feed();
        s.now_ms = now_millis();
        bt_poll_phase(s);
        wifi_and_lease_phase(s);
        net_poll_phase(s);
        http_serve_phase(s);
        slim_session_phase(s);
        {
            let mut dev = SmolDevice {
                driver: &mut s.sta_iface,
            };
            let _ = s.smol_iface.poll(
                SmolInstant::from_millis(i64::try_from(s.now_ms).unwrap_or(0)),
                &mut dev,
                &mut s.sockets,
            );
        }
        multi_proto_poll(s);
        cyd_command_phase(s);
        output_decode_phase(s);
        amp_and_cyd_health_phase(s);
    }
}

#[cfg(target_arch = "xtensa")]
#[esp_hal::main]
fn main() -> ! {
    xtensa_hw::run_hardware_firmware()
}

#[cfg(test)]
mod tests {
    use super::*;
    use louder_esp32_app::PlayerState;

    #[test]
    fn test_run_firmware_tick_processes_cyd_and_audio() {
        let mut stream_storage = vec![0u8; 1024];
        let mut output_storage = vec![0u8; 1024];
        let i2s_driver = LoopbackI2sDma::new(44100);
        let sink = EspI2sSink::for_active_board(i2s_driver, 44100);
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44100,
        );

        let mut coordinator = PlayerCoordinator::new([0x24, 0x0A, 0xC4, 0x00, 0x00, 0x01], "Test");
        let mut cyd_bridge = CydUartBridge::new(LoopbackUartPort::new());
        let mut amp = AmpManager::for_active_board(TargetI2cBus::new());
        amp.initialize(32).unwrap();
        amp.start_playback().unwrap();

        // Push PLAY command via UART RX and 1 stereo frame (4 bytes of 16-bit
        // PCM) into stream buffer.
        cyd_bridge.port.push_rx(b"CMD=PLAY\n");
        pipeline
            .ingest_stream_data(&[0x01, 0x00, 0xFF, 0x00])
            .unwrap();

        let frames = run_firmware_tick(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp);
        assert_eq!(coordinator.state, PlayerState::Playing);
        assert_eq!(frames, 1);
        assert_eq!(cyd_bridge.port.tx_bytes(), b"STATE=P\n");
    }

    #[test]
    fn test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate() {
        use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};
        use squeezelite_tas58xx::registers;
        use std::io::Read;
        use std::net::TcpListener;

        let addr = louder_esp32_app::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // initialize -> standby
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
            // start_playback (starts at VOL_DEFAULT_M20DB = 0x58)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
            // check_faults returns a non-clean fault (overcurrent on channel)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write_read(
                addr,
                vec![registers::CHAN_FAULT],
                vec![0x01, 0x00, 0x00, 0x00],
            ),
            // recover_from_fault: Hi-Z -> clear fault -> Play -> restore volume (AUD13-004)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x02]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x81]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
        ];
        let mut i2c = I2cMock::new(&expectations);
        let mut amp = AmpManager::new(&mut i2c);
        amp.initialize(32).unwrap();
        amp.start_playback().unwrap();

        let mut stream_storage = vec![0u8; 1024];
        let mut output_storage = vec![0u8; 1024];
        let i2s_driver = LoopbackI2sDma::new(48000);
        let sink = EspI2sSink::for_active_board(i2s_driver, 48000);
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let mut pipeline = StreamPipeline::new(
            &mut stream_storage,
            &mut output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            48000,
        );

        let mac = [0x24, 0x0A, 0xC4, 0x00, 0x00, 0x02];
        let mut coordinator = PlayerCoordinator::new(mac, "Test");
        coordinator.current_sample_rate = 44100;
        let mut cyd_bridge = CydUartBridge::new(LoopbackUartPort::new());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_addr = listener.local_addr().unwrap();
        let server_handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 128];
            let _ = stream.read(&mut buf).unwrap(); // HELO
        });

        let mut client = SlimprotoClient::connect(server_addr, mac, "Test").unwrap();

        let frames = run_firmware_tick_with_network(
            &mut coordinator,
            &mut pipeline,
            &mut cyd_bridge,
            &mut amp,
            Some(HostNet {
                client: &mut client,
                stream: &mut HttpAudioStream::new(),
                backend: &mut MockBackend::new(),
            }),
            100,
        );

        assert_eq!(frames, 0);
        assert_eq!(coordinator.current_sample_rate, 48000);

        server_handle.join().unwrap();
        i2c.done();
    }

    #[test]
    fn test_audio_config_lms_defaults() {
        let audio_cfg: AudioConfig = first_boot_defaults::audio_config();
        assert_eq!(audio_cfg.lms_server_ip, Some([10, 0, 0, 10]));
        assert_eq!(audio_cfg.lms_server_port, Some(3483));
    }

    #[test]
    fn test_lms_gain_to_hardware_volume_percent_curve() {
        assert_eq!(lms_gain_to_hardware_volume_percent(0), 0);
        assert_eq!(lms_gain_to_hardware_volume_percent(1842), 55);
        assert_eq!(lms_gain_to_hardware_volume_percent(65536), 100);
        assert_eq!(lms_gain_to_hardware_volume_percent(100_000), 100);

        // Verify strictly monotonic non-decreasing behavior
        let mut prev = 0;
        for g in (100..=65536).step_by(500) {
            let p = lms_gain_to_hardware_volume_percent(g);
            assert!(p >= prev, "Non-monotonic at gain {}: {} < {}", g, p, prev);
            prev = p;
        }
    }

    #[test]
    fn test_lms_gain_to_tas5805m_vol_reg() {
        assert_eq!(lms_gain_to_tas5805m_vol_reg(0), 0xFF); // Mute
        assert_eq!(lms_gain_to_tas5805m_vol_reg(65536), 0x30); // 0.0 dB (100%)
        assert_eq!(lms_gain_to_tas5805m_vol_reg(1842), 0x5D); // -22.5 dB (55%)
    }

    #[test]
    fn test_helo_capabilities_flc_and_sample_rate() {
        let caps = PlayerCoordinator::helo_capabilities(None).unwrap();
        assert!(
            caps.contains(",flc,"),
            "Capabilities must advertise native FLAC (,flc,)"
        );
        assert!(caps.contains(&format!(
            "MaxSampleRate={}",
            louder_esp32_app::clock_table::max_verified_rate()
        )));
        assert!(caps.contains(",pcm,"), "Capabilities must advertise pcm");
        assert!(caps.contains(",mp3"), "Capabilities must advertise mp3");
        assert!(!caps.contains("HasDigitalOut"));
        assert!(caps.starts_with("Model=squeezeesp32,"));
        assert!(
            BASEMENT_HELO_CAPABILITIES.contains("flc,pcm,wav,aif,mp3,aac,ogg,alc,ops"),
            "BASEMENT_HELO_CAPABILITIES must advertise all 9 codecs"
        );
    }

    #[test]
    fn test_firmware_tick_arbiter_source_selection() {
        let mac = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34];
        let mut coordinator = PlayerCoordinator::new(mac, "BASEMENT");
        assert_eq!(coordinator.active_source(), AudioSource::None);
        assert!(coordinator.request_source(AudioSource::SlimProto));
        assert!(coordinator.request_source(AudioSource::Bluetooth));
        assert_eq!(coordinator.active_source(), AudioSource::Bluetooth);
        assert!(coordinator.request_source(AudioSource::Spotify));
        assert_eq!(coordinator.active_source(), AudioSource::Spotify);
        assert!(coordinator.request_source(AudioSource::AirPlay));
        assert_eq!(coordinator.active_source(), AudioSource::AirPlay);
        coordinator.release_source(AudioSource::AirPlay);
        assert_eq!(coordinator.active_source(), AudioSource::Spotify);
        coordinator.release_source(AudioSource::Spotify);
        assert_eq!(coordinator.active_source(), AudioSource::Bluetooth);
        coordinator.release_source(AudioSource::Bluetooth);
        assert_eq!(coordinator.active_source(), AudioSource::SlimProto);
        coordinator.release_source(AudioSource::SlimProto);
        assert_eq!(coordinator.active_source(), AudioSource::None);
    }
}
