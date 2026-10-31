//! Louder-ESP32-Plus Application Entrypoint
//!
//! Initializes board hardware profiles, starts audio streaming pipeline,
//! and orchestrates SlimProto network connection with TAS5805M/TAS5825M digital amplifier.
#![cfg_attr(target_arch = "xtensa", no_std)]
#![cfg_attr(target_arch = "xtensa", no_main)]
#![cfg_attr(all(target_arch = "xtensa", not(feature = "esp-rtos-core1")), feature(asm_experimental_arch))]
#![cfg_attr(not(target_arch = "xtensa"), forbid(unsafe_code))]
#![cfg_attr(target_arch = "xtensa", deny(unsafe_code))]
#![allow(unknown_lints)]

#[cfg(not(target_arch = "xtensa"))]
use embedded_hal::i2c::I2c;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::board::{pins, BoardModel};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::BitDepth;
use louder_esp32_app::hal::{EspI2sSink, UartPort};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::LoopbackI2sDma;
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::{AmpManager, CydUartBridge, PlayerCoordinator};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::AudioOutputSink;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::{
    CydMessage, HttpAudioStream, LoopbackUartPort, MemConfigStore, MemFlashPartition,
    NvsFlashBackend, PlayState, SlimprotoClient, TargetI2cBus,
};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::io_traits::MockBackend;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::board::first_boot_defaults;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_config::AudioConfig;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_decoders::PcmDecoder;

mod tasks;

/// Executes one integrated firmware tick across the CYD UART bridge, audio pipeline,
/// and digital amplifier health monitor (`HALNET-409`).
///
/// Host only: on the device the same steps now run as embassy phases (`tasks.rs`: `cyd_bridge`
/// command poll, `i2s_feed` DMA-first top-up, `decode_pump` burst, `i2s_feed` final top-up).
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

/// Host network I/O driven by [`run_firmware_tick_with_network`]: the SlimProto control
/// connection, the HTTP audio stream and the (simulated) audio backend.
#[cfg(not(target_arch = "xtensa"))]
pub struct HostNet<'a> {
    pub client: &'a mut SlimprotoClient,
    pub stream: &'a mut HttpAudioStream,
    pub backend: &'a mut MockBackend,
}

/// Executes one integrated firmware tick with SlimProto network integration,
/// dynamic sample rate synchronization, DAC amplifier pause, and hardware fault recovery (`RUST-19`).
#[cfg(not(target_arch = "xtensa"))]
pub fn run_firmware_tick_with_network<S, U, I>(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut StreamPipeline<'_, S>,
    cyd_bridge: &mut CydUartBridge<U>,
    amp: &mut AmpManager<I>,
    net: Option<HostNet<'_>>,
    now_ms: u32,
) -> usize
where
    S: AudioOutputSink,
    U: UartPort,
    I: I2c,
{
    if let Ok(Some(cmd)) = cyd_bridge.poll_command() {
        if let Some(reply) = coordinator.handle_cyd_command(cmd, pipeline) {
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

// Moved to the library (`amp`) so the shared coordinator can map `audg` onto the TAS volume
// registers through `AudioBackend::set_hw_volume` (Task 7); re-exported for existing callers.
pub use louder_esp32_app::amp::{lms_gain_to_hardware_volume_percent, lms_gain_to_tas5805m_vol_reg};

#[cfg(not(target_arch = "xtensa"))]
fn main() {
    let board = BoardModel::active();
    println!("========================================================");
    println!("  Starting Pure-Rust Squeezelite Engine on {}", board.name());
    println!("========================================================");
    println!("Hardware Pin Configuration:");
    println!("  - I2S Master Audio: BCLK={}, LRCLK={}, DOUT={}", pins::I2S_BCLK, pins::I2S_LRCLK, pins::I2S_DOUT);
    println!("  - I2C Amp Control:  SDA={}, SCL={}", pins::I2C_SDA, pins::I2C_SCL);
    println!("  - TAS58xx Power:    PWDN={}, FAULT={}", pins::TAS58XX_PWDN, pins::TAS58XX_FAULT);
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
    audio_cfg.save_to(&mut config_store).expect("Failed to serialize AudioConfig");

    let initial_seq = nvs_backend.commit_store(&config_store).expect("Failed to commit dual-sector NVS");
    println!("Dual-Sector Flash NVS Backend:");
    println!("  - Active Sector Sequence: {}", initial_seq);
    println!("  - Key-Value Entries:      {}", config_store.len());

    // Verify persistence across dual-sector flash journal
    let mut verify_store = MemConfigStore::<16>::new();
    nvs_backend.load_store(&mut verify_store).expect("Failed to reload from dual-sector flash");
    let verified_audio = AudioConfig::load_from(&verify_store).expect("Failed to deserialize AudioConfig");
    println!("  - Verified Node Name:     {}", verified_audio.player_name);
    println!("--------------------------------------------------------");

    // 2. Hardware/Target I2C Bus Adapter and TAS5805M Amplifier Lifecycle Manager
    let i2c_bus = TargetI2cBus::new();
    let mut amp = AmpManager::for_active_board(i2c_bus);
    amp.initialize(BitDepth::Bits32.bits()).expect("Failed to initialize TAS5805M amplifier");
    amp.start_playback_via_hiz().expect("Failed to start playback mode via Hi-Z on TAS5805M");
    println!("TAS5805M Digital Amplifier Subsystem:");
    println!("  - Mode:         {:?}", amp.mode());
    println!("  - Volume:       -24 dB (0x{:02X})", amp.current_volume());
    println!("  - Fault Status: {:?}", amp.check_faults().unwrap_or(None));
    println!("--------------------------------------------------------");

    // 3. Allocate heap-backed audio stream and output buffers (APP-001: avoid FreeRTOS task stack overflow)
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

    // 4. Host demo MAC: a deterministic test MAC per board (the device uses its efuse STA MAC).
    let mac = {
        let suffix = match board {
            BoardModel::LouderEsp32Wrover => 0x01,
            BoardModel::LouderEsp32PlusS3 => 0x02,
        };
        [0x24, 0x0A, 0xC4, 0x12, 0x34, suffix]
    };
    let mut coordinator = PlayerCoordinator::new(mac, board.name());
    coordinator.player_name.clear();
    let safe_name_len = audio_cfg.player_name.floor_char_boundary(64.min(audio_cfg.player_name.len()));
    let _ = coordinator.player_name.push_str(&audio_cfg.player_name[..safe_name_len]);

    println!("Player Coordinator initialized:");
    println!("  - State:       {:?}", coordinator.state);
    println!("  - Node Name:   {}", audio_cfg.player_name);
    println!("  - MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    println!("  - Sample Rate: {} Hz", pipeline.sample_rate);
    println!("  - Buffers:     {} bytes stream, {} bytes output", pipeline.stream_buffer_available(), pipeline.output_buffer_available());
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

    // 7. Optional Live LMS Network Connection & Firmware Tick Execution Loop (HALNET-409, HW-PREP-05)
    let lms_ip = std::net::Ipv4Addr::from(audio_cfg.lms_server_ip.unwrap_or(first_boot_defaults::LMS_SERVER_IP));
    let lms_port = audio_cfg.lms_server_port.unwrap_or(first_boot_defaults::LMS_SERVER_PORT);
    let lms_addr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(lms_ip, lms_port));

    let should_connect = std::env::var("SL_LMS_CONNECT").as_deref() == Ok("1") || cfg!(target_arch = "xtensa");
    let mut client = if should_connect {
        println!("Connecting SlimProto client to {}...", lms_addr);
        match SlimprotoClient::connect_with_options(lms_addr, mac, audio_cfg.player_name.as_str(), false, 0) {
            Ok(c) => {
                println!("SlimProto client successfully connected to {} for player '{}'", lms_addr, audio_cfg.player_name);
                Some(c)
            }
            Err(e) => {
                if std::env::var("SL_LMS_CONNECT").as_deref() == Ok("1") {
                    panic!("Failed to connect SlimProto client to {}: {:?}", lms_addr, e);
                } else {
                    eprintln!("Warning: Failed to connect SlimProto client to {}: {:?}", lms_addr, e);
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
            println!("  [Tick {:02}] frames={}, state={:?}", tick, frames, coordinator.state);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(target_arch = "xtensa")]
mod xtensa_hw {
    extern crate alloc;

    use super::*;
    use embedded_hal::i2c::{ErrorType, I2c, Operation};
    use esp_backtrace as _;
    use crate::tasks::{FirmwareState, Shared, UDP_PLACEHOLDER_SOCKETS};
    use esp_hal::{
        clock::CpuClock,
        efuse::{interface_mac_address, InterfaceMacAddress},
        gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
        i2c::master::{Config as I2cConfig, I2c as HalI2c},
        i2s::master::{Channels, DataFormat, I2s, TdmConfig},
        rng::Rng,
        system::{CpuControl, Stack},
        time::{Duration, Instant, Rate},
        timer::timg::{MwdtStage, TimerGroup},
        uart::{Config as UartConfig, Uart},
    };
    use esp_println::println;
    use esp_radio::wifi::{
        sta::StationConfig, AuthenticationMethodConfig, Config as WifiModeConfig, ControllerConfig,
        Interface as WifiInterface, PowerSaveMode, WifiController, WifiRxToken, WifiTxToken,
    };
    use esp_storage::FlashStorage;
    use louder_esp32_app::amp::percent_to_attenuation;
    use louder_esp32_app::backend_device::{
        program_i2s_clock, I2sClockRegisters, I2S0_CLKM_CONF_ADDR, I2S0_SAMPLE_RATE_CONF_ADDR,
    };
    use louder_esp32_app::board::first_boot_defaults;
    use louder_esp32_app::config_handle::{
        resolve_boot_audio, resolve_boot_wifi, ConfigHandle, ConfigSource, JournalSector,
        FlashTargetPolicy, JournalSlot, RamJournal,
    };
    use louder_esp32_app::net_device::{HttpStreamState, SlimLinkState, SlimReconnect};
    use louder_esp32_app::hal::I2sDmaDriver;
    use louder_esp32_app::http_control::{DeadManLeaseWatchdog, HttpControlAction};
    use squeezelite_audio_core::EqPreset;
    use squeezelite_config::{FlashJournal, JournalView};
    use squeezelite_cyd_link::CydMessage;
    use louder_esp32_app::ota_safety::{
        load_nvs_wifi_credentials_split_12kb, resolve_wifi_credentials, verify_otadata_erased,
        OtaSafetyError, WifiStaCredentials, OTADATA_PARTITION_END, OTADATA_PARTITION_OFFSET,
        OTADATA_SECTOR_1_OFFSET,
    };
    use smoltcp::iface::{
        Config as SmolIfaceConfig, Interface as SmolInterface, SocketHandle, SocketSet, SocketStorage,
    };
    use static_cell::{ConstStaticCell, StaticCell};
    use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
    use smoltcp::socket::{dhcpv4, tcp, udp};
    use smoltcp::time::Instant as SmolInstant;
    use smoltcp::wire::{EthernetAddress, HardwareAddress};
    use squeezelite_audio_core::output::OutputError;
    use squeezelite_decoders::PcmDecoder;
    use squeezelite_tas58xx::{registers, VOL_UNITY_0DB};

    // Mandatory Squeezelite-ESP32 OTA binary descriptor (`squeezelite-ota.c:663`).
    // Sets `project_name = "Squeezelite-ESP32"` and `version = "I2S-4MFlash-Rust"` at offset 0x20.
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

    // Gate 2: Custom panic halt hook that resets immediately into factory recovery (`0x10000`)
    // instead of spinning in `loop {}`.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub extern "Rust" fn custom_halt() -> ! {
        println!("[ANTI-BRICK GATE 2] Panic halted -> executing software_reset() to boot recovery (0x10000)");
        esp_hal::system::software_reset()
    }

    static HW_ACK_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    /// Per-operation NACK counters (`/status.json` `i2c_nack_by_op`, spec §4.8). Counting only:
    /// each replaces the single total counter's increment at the same place, so the bus
    /// behaviour is unchanged and the total (`i2c_nacks`) is their sum.
    static HW_NACK_READ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static HW_NACK_WRITE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static HW_NACK_WRITE_READ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    pub struct SmolDevice<'a> {
        pub driver: &'a mut WifiInterface,
    }

    pub struct SmolRxToken(pub WifiRxToken);
    pub struct SmolTxToken(pub WifiTxToken);

    /// Matches `esp-radio`'s `ESP_RADIO_CONFIG_WIFI_MTU = 1492` (`src/wifi/mod.rs:117`).
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
        type RxToken<'b> = SmolRxToken where Self: 'b;
        type TxToken<'b> = SmolTxToken where Self: 'b;

        fn receive(&mut self, _timestamp: SmolInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
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
            by_op.read.wrapping_add(by_op.write).wrapping_add(by_op.write_read)
        }

        pub fn hw_nack_by_op() -> louder_esp32_app::http_control::I2cNackByOp {
            let load = |c: &core::sync::atomic::AtomicU32| c.load(core::sync::atomic::Ordering::Relaxed);
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

        fn write_read(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error> {
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

        fn transaction(&mut self, address: u8, operations: &mut [Operation<'_>]) -> Result<(), Self::Error> {
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

    /// I2S0 clock registers (`backend_device` documents the layout), volatile access.
    pub(crate) struct DeviceClockRegs;

    #[allow(unsafe_code)]
    impl I2sClockRegisters for DeviceClockRegs {
        fn read_clkm_conf(&mut self) -> u32 {
            // SAFETY: fixed, always-mapped I2S0 register; I2S0 is owned by this firmware.
            unsafe { core::ptr::read_volatile(I2S0_CLKM_CONF_ADDR as *const u32) }
        }
        fn write_clkm_conf(&mut self, value: u32) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(I2S0_CLKM_CONF_ADDR as *mut u32, value) }
        }
        fn read_sample_rate_conf(&mut self) -> u32 {
            // SAFETY: as above.
            unsafe { core::ptr::read_volatile(I2S0_SAMPLE_RATE_CONF_ADDR as *const u32) }
        }
        fn write_sample_rate_conf(&mut self, value: u32) {
            // SAFETY: as above.
            unsafe { core::ptr::write_volatile(I2S0_SAMPLE_RATE_CONF_ADDR as *mut u32, value) }
        }
    }

    /// Size of the I2S TX DMA stream buffer (`dma_tx_stream_buffer!` in [`init_i2s_pipeline`]).
    const I2S_DMA_BUFFER_BYTES: usize = 8192;

    /// Stereo frames queued in the I2S DMA buffer, updated by the driver after every push
    /// (`AudioBackend::dma_outstanding_frames`).
    pub(crate) static DMA_OUTSTANDING_FRAMES: core::sync::atomic::AtomicU32 =
        core::sync::atomic::AtomicU32::new(0);

    fn note_dma_fill(available_bytes: usize) {
        let queued = I2S_DMA_BUFFER_BYTES.saturating_sub(available_bytes) / 8;
        DMA_OUTSTANDING_FRAMES.store(queued as u32, core::sync::atomic::Ordering::Relaxed);
    }

    /// Hardware I2S0 + DMA_I2S0 audio output driver (`BCK=GPIO26`, `WS=GPIO25`, `DOUT=GPIO22`).
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
                    if let Ok(mut new_buf) = esp_hal::dma::DmaTxStreamBuf::new(descriptors, buffer) {
                        Self::prefill_silence(&mut new_buf);
                        if let Ok(new_transfer) = i2s_tx.write(new_buf) {
                            self.transfer = Some(new_transfer);
                            let rate = self.sample_rate_hz;
                            self.sample_rate_hz = 0;
                            let _ = self.set_clock_rate(rate);
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
        fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError> {
            let valid_len = data.len() & !7;
            if valid_len == 0 {
                return Ok(0);
            }
            self.ensure_running();

            let transfer = match self.transfer.as_mut() {
                Some(t) => t,
                None => return Ok(0),
            };

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

        /// Programs the I2S0 dividers AND the bit-clock divider from the verified clock table
        /// (`backend_device::program_i2s_clock`); a rate missing from the table is rejected
        /// without touching the hardware (no 44.1 kHz fallback).
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

    /// Boot resolution step 2 (Wi-Fi): the C firmware's NVS at 0x9000, read-only. Boot-time,
    /// single-core read (APP_CPU is parked and esp-radio not started yet).
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

    /// Boot resolution of the Wi-Fi STA credentials, Phase 0/1: the C firmware's NVS, else the
    /// compile-time first-boot credentials; none at all resets into recovery, as before. Journal
    /// `wifi.*` keys are ignored (Wi-Fi provisioning belongs to the C recovery firmware until
    /// Phase 3, and no Rust path can write or clear them).
    /// The result lives in `.dram2_uninit` for the life of the image (`/status.json` shows the
    /// SSID), keeping it off the boot frame and out of `.bss` (which would shrink `.stack`).
    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_wifi_credentials(
        flash: &mut FlashStorage<'_>,
        nvs_scratch: &mut [u8; 12_288],
        sta_mac: &[u8; 6],
    ) -> &'static WifiStaCredentials {
        #[unsafe(link_section = ".dram2_uninit")]
        static mut WIFI_STA_CREDS: core::mem::MaybeUninit<WifiStaCredentials> =
            core::mem::MaybeUninit::uninit();
        // Journal precedence for Wi-Fi returns in Phase 3 with Rust provisioning.
        let resolved = resolve_boot_wifi(
            || load_nvs_wifi_credentials(flash, sta_mac, nvs_scratch).ok(),
            || default_wifi_credentials(sta_mac),
        );
        let Some((creds, source)) = resolved else {
            println!(
                "[ANTI-BRICK] No Wi-Fi credentials in the settings journal, 0x9000 NVS or the build -> resetting to recovery (0x10000)"
            );
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
        // SAFETY: bring-up calls this once, before anything else can name the static; it is
        // fully written before the only (shared) reference is handed out.
        unsafe { &*(*core::ptr::addr_of_mut!(WIFI_STA_CREDS)).write(creds) }
    }

    /// Boot resolution step 3 (Wi-Fi): the compile-time first-boot credentials, only when a
    /// passphrase was compiled in (`SL_WIFI_PASS`, plain or `enc:`), with `SL_WIFI_SSID` or
    /// [`first_boot_defaults::WIFI_SSID`].
    #[inline(never)]
    fn default_wifi_credentials(sta_mac: &[u8; 6]) -> Option<WifiStaCredentials> {
        let ssid = option_env!("SL_WIFI_SSID").unwrap_or(first_boot_defaults::WIFI_SSID);
        resolve_wifi_credentials(&[], sta_mac, Some(ssid), option_env!("SL_WIFI_PASS")).ok()
    }

    static MP3_DEC_TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    /// The firmware's only MP3 decoder (no heap MP3 decoder without `std-codecs`). Taken once at
    /// bring-up and parked in the pipeline's MP3 slot (`StreamPipeline::mp3_spare`), from where
    /// the coordinator uses and returns it for every MP3 track.
    #[allow(unsafe_code)]
    fn take_static_mp3_decoder() -> &'static mut squeezelite_decoders::StaticMp3Decoder {
        assert!(
            !MP3_DEC_TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel),
            "StaticMp3Decoder already taken"
        );
        // SAFETY: Exclusive singleton access to the static MP3 decoder is guaranteed by MP3_DEC_TAKEN check.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_MP3_DEC: squeezelite_decoders::StaticMp3Decoder =
            squeezelite_decoders::StaticMp3Decoder::new();
        unsafe {
            let dec = &mut *core::ptr::addr_of_mut!(STATIC_MP3_DEC);
            dec.reset_in_place();
            dec
        }
    }

    struct FirmwareBssBuffers {
        ring_storage: [u8; 12288],
        http_rx_buf: [u8; 1024],
        slim_rx_buf: [u8; 2048],
        slim_tx_buf: [u8; 2048],
        audio_rx_buf: [u8; 4096],
        audio_tx_buf: [u8; 512],
    }

    impl FirmwareBssBuffers {
        const fn new() -> Self {
            Self {
                ring_storage: [0u8; 12288],
                http_rx_buf: [0u8; 1024],
                slim_rx_buf: [0u8; 2048],
                slim_tx_buf: [0u8; 2048],
                audio_rx_buf: [0u8; 4096],
                audio_tx_buf: [0u8; 512],
            }
        }
    }

    #[allow(unsafe_code)]
    #[link_section = ".dram2_uninit"]
    static mut APP_CORE_STACK: Stack<4096> = Stack::new();
    #[allow(unsafe_code)]
    #[unsafe(link_section = ".dram2_uninit")]
    static mut STATIC_BT_HEAP: core::mem::MaybeUninit<[u8; 18 * 1024]> =
        core::mem::MaybeUninit::uninit();
    #[allow(unsafe_code)]
    #[unsafe(link_section = ".dram2_uninit")]
    static mut STATIC_BT_SINK: core::mem::MaybeUninit<louder_esp32_app::bt_a2dp::BtA2dpSink> =
        core::mem::MaybeUninit::uninit();

    pub(crate) struct Esp32VhciTransport<'d> {
        pub(crate) connector: esp_radio::ble::controller::BleConnector<'d>,
    }

    impl<'d> louder_esp32_app::bt_a2dp::HciTransport for Esp32VhciTransport<'d> {
        fn read_packet(&mut self, buf: &mut [u8]) -> usize {
            self.connector.next(buf).unwrap_or(0)
        }

        fn write_packet(&mut self, data: &[u8]) -> bool {
            matches!(self.connector.write(data), Ok(n) if n == data.len())
        }
    }
    pub(crate) static CORE1_HEARTBEAT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    #[cfg(not(feature = "esp-rtos-core1"))]
    #[esp_hal::ram]
    fn app_core_loop() {
        loop {
            CORE1_HEARTBEAT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            for _ in 0..10_000 {
                #[allow(unsafe_code)]
                unsafe {
                    core::arch::asm!("nop");
                }
            }
        }
    }

    use louder_esp32_app::ota_safety::SETTINGS_JOURNAL_OFFSET;

    /// Settings flash write target. Default build: sector A (0x3FE000) only, byte-identical to
    /// the certified pre-reboot write. 0x3FF000 is page 15 of the C firmware's live `settings`
    /// NVS partition, which the certified build never touched, and a failed C NVS init erases the
    /// whole partition (`components/platform_config/nvs_utilities.c:97-100`). Boot still reads
    /// and validates both sectors.
    #[cfg(not(feature = "runtime-flash-persist"))]
    const SETTINGS_FLASH_TARGET: FlashTargetPolicy = FlashTargetPolicy::FixedA;
    /// Settings flash write target, `runtime-flash-persist`: dual-sector ping-pong at
    /// 0x3FE000 / 0x3FF000 (power loss mid-write keeps the previous journal).
    #[cfg(feature = "runtime-flash-persist")]
    const SETTINGS_FLASH_TARGET: FlashTargetPolicy = FlashTargetPolicy::PingPong;

    /// Boot resolution step 1 + runtime configuration statics (spec §3.4–3.5, Task 10).
    ///
    /// Reads both settings sectors (0x3FE000 / 0x3FF000) into `scratch` -- the only flash read
    /// of the settings journal, at boot, single-core -- picks the newest valid journal, resolves
    /// every audio/identity field (journal, else [`first_boot_defaults`]) and builds, in place in
    /// `.dram2_uninit` (so neither `.stack` nor this frame carries them):
    /// - the [`ConfigHandle`] holding the resolved [`AudioConfig`](squeezelite_config::AudioConfig);
    /// - the [`RamJournal`]: DRAM sector image + which sector holds the newest journal and its
    ///   sequence, so no later flash write ever needs to read flash. Its flash target policy is
    ///   [`SETTINGS_FLASH_TARGET`].
    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_config(
        flash: &mut FlashStorage<'_>,
        scratch: &mut [u8; 12_288],
    ) -> (&'static ConfigHandle, &'static mut RamJournal<'static>) {
        #[unsafe(link_section = ".dram2_uninit")]
        static mut JOURNAL_IMAGE: core::mem::MaybeUninit<JournalSector> = core::mem::MaybeUninit::uninit();
        #[unsafe(link_section = ".dram2_uninit")]
        static mut RAM_JOURNAL: core::mem::MaybeUninit<RamJournal<'static>> = core::mem::MaybeUninit::uninit();
        #[unsafe(link_section = ".dram2_uninit")]
        static mut CONFIG_HANDLE: core::mem::MaybeUninit<ConfigHandle> = core::mem::MaybeUninit::uninit();
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
            JournalView::new(if index == 0 { &sector_a[..] } else { &sector_b[..] }).ok()
        });
        let audio = resolve_boot_audio(view.as_ref(), &first_boot_defaults::audio_config());
        let boot_slot = selected.map(|(index, seq)| (JournalSlot::from_index(index), seq));

        // SAFETY: called once (TAKEN); each static is fully written before the only reference
        // to it is handed out, and nothing else names these statics.
        unsafe {
            let image = (*core::ptr::addr_of_mut!(JOURNAL_IMAGE)).as_mut_ptr();
            image.write_bytes(0xFF, 1);
            let journal = (*core::ptr::addr_of_mut!(RAM_JOURNAL))
                .write(RamJournal::new(&mut *image, boot_slot, SETTINGS_FLASH_TARGET));
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
        // SAFETY: Only a single caller on PRO_CPU takes ownership of the firmware static BSS buffers.
        #[unsafe(link_section = ".dram2_uninit")]
        static mut BSS_BUFFERS: FirmwareBssBuffers = FirmwareBssBuffers::new();
        unsafe { &mut *core::ptr::addr_of_mut!(BSS_BUFFERS) }
    }

    pub(crate) type HwStreamPipeline = StreamPipeline<'static, EspI2sSink<Esp32HardwareI2sDma<'static>>>;

    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_static_pipeline(
        stream_storage: &'static mut [u8],
        output_storage: &'static mut [u8],
        sink: EspI2sSink<Esp32HardwareI2sDma<'static>>,
    ) -> PipelineStatics {
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_PIPELINE: core::mem::MaybeUninit<HwStreamPipeline> =
            core::mem::MaybeUninit::uninit();
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_HTTP_RESP_BUF: core::mem::MaybeUninit<[u8; 1536]> =
            core::mem::MaybeUninit::uninit();
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_HTTP_TX_BUF: core::mem::MaybeUninit<[u8; 1536]> =
            core::mem::MaybeUninit::uninit();
        unsafe {
            let ptr = (*core::ptr::addr_of_mut!(STATIC_PIPELINE)).as_mut_ptr();
            core::ptr::addr_of_mut!((*ptr).stream_buffer)
                .write(squeezelite_audio_core::AudioRingBuffer::new(stream_storage));
            core::ptr::addr_of_mut!((*ptr).output_buffer)
                .write(squeezelite_audio_core::AudioRingBuffer::new(output_storage));
            core::ptr::addr_of_mut!((*ptr).decoder)
                .write(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)));
            core::ptr::addr_of_mut!((*ptr).equalizer)
                .write(squeezelite_audio_core::Equalizer::new());
            core::ptr::addr_of_mut!((*ptr).volume)
                .write(squeezelite_audio_core::VolumeScale::unity());
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

            let http_resp_ptr = (*core::ptr::addr_of_mut!(STATIC_HTTP_RESP_BUF)).as_mut_ptr();
            http_resp_ptr.write_bytes(0, 1);

            let http_tx_ptr = (*core::ptr::addr_of_mut!(STATIC_HTTP_TX_BUF)).as_mut_ptr();
            http_tx_ptr.write_bytes(0, 1);

            (&mut *ptr, &mut *http_resp_ptr, &mut *http_tx_ptr)
        }
    }


    /// Core-1 (APP_CPU) control kept by the firmware for the life of the image.
    ///
    /// Default build: the manual start path (`CpuControl::start_app_core` + NOP heartbeat). The
    /// guard is held forever because dropping an `AppCoreGuard` parks APP_CPU. Feature
    /// `esp-rtos-core1`: core 1 is owned by `esp_rtos::start_second_core` (S1 sequencing note;
    /// unvalidated on hardware until the S1 owner gate).
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

        /// Parks APP_CPU before the reboot-time settings flash write (Core-1 parking rule).
        pub(crate) fn park_for_flash_write(&mut self) {
            #[cfg(not(feature = "esp-rtos-core1"))]
            #[allow(unsafe_code)]
            unsafe {
                self.cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu);
            }
            // `CPU_CTRL` was handed to `esp_rtos::start_second_core`; this runs only right before
            // `software_reset()`, so a stolen handle can not race another owner.
            #[cfg(feature = "esp-rtos-core1")]
            #[allow(unsafe_code)]
            unsafe {
                CpuControl::new(esp_hal::peripherals::CPU_CTRL::steal())
                    .park_core(esp_hal::system::Cpu::AppCpu);
            }
        }

        /// Restarts APP_CPU after a runtime settings flash write (`runtime-flash-persist` only;
        /// see `tasks::runtime_flash_commit`). Default build: the manual path's core 1 runs the
        /// IRAM NOP heartbeat, so it resumes where it was parked.
        #[cfg(feature = "runtime-flash-persist")]
        pub(crate) fn unpark_after_flash_write(&mut self) {
            #[cfg(not(feature = "esp-rtos-core1"))]
            self.cpu_ctrl.unpark_core(esp_hal::system::Cpu::AppCpu);
        }
    }

    // `runtime-flash-persist` parks APP_CPU mid-flight at runtime. Under `esp-rtos-core1` core 1
    // runs an embassy executor that takes critical sections; parked while holding the
    // multicore critical-section lock, it would deadlock core 0's next flash call. Only the
    // manual core-1 path (IRAM NOP loop, never locks) is allowed with the runtime commit.
    #[cfg(all(feature = "runtime-flash-persist", feature = "esp-rtos-core1"))]
    compile_error!(
        "`runtime-flash-persist` requires the manual core-1 path: it can not be combined with `esp-rtos-core1`"
    );

    /// Smoltcp socket slots: DHCP, HTTP server, slimproto, audio + the UDP placeholders (§3.6).
    const SOCKET_SLOTS: usize = 4 + UDP_PLACEHOLDER_SOCKETS;

    /// Payload/metadata sizing of one UDP placeholder socket (P3 re-sizes them; S2 suggested
    /// 4 metadata slots + 512 B payload for the LMS discovery socket).
    const UDP_RX_META: usize = 4;
    const UDP_TX_META: usize = 2;
    const UDP_PAYLOAD: usize = 512;

    struct UdpPlaceholderBuffers {
        rx_meta: [udp::PacketMetadata; UDP_RX_META],
        rx_payload: [u8; UDP_PAYLOAD],
        tx_meta: [udp::PacketMetadata; UDP_TX_META],
        tx_payload: [u8; UDP_PAYLOAD],
    }

    impl UdpPlaceholderBuffers {
        const fn new() -> Self {
            Self {
                rx_meta: [udp::PacketMetadata::EMPTY; UDP_RX_META],
                rx_payload: [0u8; UDP_PAYLOAD],
                tx_meta: [udp::PacketMetadata::EMPTY; UDP_TX_META],
                tx_payload: [0u8; UDP_PAYLOAD],
            }
        }
    }

    // ----------------------------------------------------------------------------------------
    // Boot-stack budget (Task 8 ruling: headroom-based stack gate). Everything the firmware keeps
    // for its whole life is built IN PLACE in a static, inside a small `#[inline(never)]`
    // bring-up stage, instead of as a `bring_up` local that is later moved into the state. That
    // keeps `bring_up`'s frame (which sits under every boot-time callee) small, and each stage's
    // temporaries are released when it returns. The bytes show up in `.bss` instead of on the
    // core-0 stack; the gate is the resulting stack headroom (boot peak and steady state) versus
    // e5b44cd, recorded in the Task 8 report.
    // ----------------------------------------------------------------------------------------

    /// smoltcp socket slots. `ConstStaticCell` hands them out without a stack temporary.
    static SOCKET_STORAGE: ConstStaticCell<[SocketStorage<'static>; SOCKET_SLOTS]> =
        ConstStaticCell::new([SocketStorage::EMPTY; SOCKET_SLOTS]);
    /// Spec §3.6 UDP placeholder buffers (static, never on the stack).
    static UDP_BUFFERS: ConstStaticCell<[UdpPlaceholderBuffers; UDP_PLACEHOLDER_SOCKETS]> =
        ConstStaticCell::new([const { UdpPlaceholderBuffers::new() }; UDP_PLACEHOLDER_SOCKETS]);
    static AMP: StaticCell<AmpManager<Esp32I2cBus<'static>>> = StaticCell::new();
    static CYD_BRIDGE: StaticCell<CydUartBridge<Esp32Uart2Port<'static>>> = StaticCell::new();
    static COORDINATOR: StaticCell<PlayerCoordinator> = StaticCell::new();
    static SMOL_IFACE: StaticCell<SmolInterface> = StaticCell::new();
    /// The superloop state shared by the core-0 tasks.
    static SHARED: StaticCell<Shared> = StaticCell::new();

    /// Bring-up stage: I2C0 (SDA=21, SCL=27, 400 kHz) and the TAS58xx manager over it, built in
    /// [`AMP`]. Construction only; the HIL amp init keeps its legacy place in [`bring_up`].
    #[inline(never)]
    fn init_amp_i2c(
        i2c0: esp_hal::peripherals::I2C0<'static>,
        sda: esp_hal::peripherals::GPIO21<'static>,
        scl: esp_hal::peripherals::GPIO27<'static>,
    ) -> &'static mut AmpManager<Esp32I2cBus<'static>> {
        let i2c_cfg = I2cConfig::default().with_frequency(Rate::from_khz(400));
        let hal_i2c = HalI2c::new(i2c0, i2c_cfg)
            .expect("Failed to initialize I2C0")
            .with_sda(sda)
            .with_scl(scl);
        let i2c_bus = Esp32I2cBus::new(hal_i2c);
        AMP.init(AmpManager::for_active_board(i2c_bus))
    }

    /// Static pipeline + the static HTTP-server buffers handed out with it
    /// (`init_static_pipeline`).
    type PipelineStatics = (
        &'static mut HwStreamPipeline,
        &'static mut [u8; 1536],
        &'static mut [u8; 1536],
    );

    /// Bring-up stage: I2S0 TDM-Philips TX (BCK=26, WS=25, DOUT=22) with streaming DMA prefilled
    /// with silence, then the I2S sink and the static audio pipeline around it. Building the
    /// sink (a ~1 KB value) here keeps it out of [`bring_up`]'s frame; sink/pipeline
    /// construction is pure memory initialisation that used to run a few statements later.
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
        let mut tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024);
        Esp32HardwareI2sDma::prefill_silence(&mut tx_buffer);
        let i2s = I2s::new(i2s0, dma, i2s_tdm_cfg).expect("Failed to initialize I2S0");
        let i2s_tx = i2s.i2s_tx.with_bclk(bclk).with_ws(ws).with_dout(dout).build();
        let i2s_transfer = i2s_tx
            .write(tx_buffer)
            .expect("Failed to start I2S0 TX streaming DMA");
        let hw_i2s_driver = Esp32HardwareI2sDma::new(i2s_transfer, 44_100);
        let sink = EspI2sSink::for_active_board(hw_i2s_driver, 44_100);
        init_static_pipeline(stream_storage, output_storage, sink)
    }

    /// Bring-up stage: UART2 @ 460 800 (TX=4, RX=36) and the CYD bridge over it, built in
    /// [`CYD_BRIDGE`].
    #[inline(never)]
    fn init_cyd_uart(
        uart2: esp_hal::peripherals::UART2<'static>,
        tx: esp_hal::peripherals::GPIO4<'static>,
        rx: esp_hal::peripherals::GPIO36<'static>,
    ) -> &'static mut CydUartBridge<Esp32Uart2Port<'static>> {
        let uart2_rx = Input::new(rx, InputConfig::default().with_pull(Pull::None));
        let uart2_cfg = UartConfig::default().with_baudrate(460_800);
        let hal_uart2 = Uart::new(uart2, uart2_cfg)
            .expect("Failed to initialize UART2")
            .with_tx(tx)
            .with_rx(uart2_rx);
        let uart2_port = Esp32Uart2Port::new(hal_uart2);
        CYD_BRIDGE.init(CydUartBridge::new(uart2_port))
    }

    /// Bring-up stage: the player coordinator, built in [`COORDINATOR`], with this board's
    /// efuse STA MAC (HELO; smoltcp uses the same MAC), the boot-resolved player name (`SETD`
    /// after HELO, `setd` id-0 replies) and LMS address (`strm s` with server IP 0.0.0.0 streams
    /// from it), and the [`ConfigHandle`] a `setd` id-0 name change persists through.
    /// Server frames are received into a static buffer in `.dram2_uninit` (which does not
    /// shrink `.stack`) instead of a 2 KB buffer on the `slimproto_session` stack.
    #[allow(unsafe_code)]
    #[inline(never)]
    fn init_coordinator(
        sta_mac: [u8; 6],
        config: &'static ConfigHandle,
        lms_ip: [u8; 4],
    ) -> &'static mut PlayerCoordinator {
        #[unsafe(link_section = ".dram2_uninit")]
        static mut SERVER_FRAME_BUF: core::mem::MaybeUninit<
            [u8; louder_esp32_app::coordinator::SERVER_FRAME_BUF_LEN],
        > = core::mem::MaybeUninit::uninit();
        // SAFETY: called once at bring-up (`COORDINATOR.init` panics on a second call); the
        // buffer is zeroed before the only reference to it is handed out.
        let frame_buf: &'static mut [u8] = unsafe {
            let ptr = (*core::ptr::addr_of_mut!(SERVER_FRAME_BUF)).as_mut_ptr();
            ptr.write_bytes(0, 1);
            &mut *ptr
        };
        let coordinator = COORDINATOR.init(PlayerCoordinator::new(sta_mac, "SqueezeESP32"));
        config.read(|c| {
            coordinator.player_name.clear();
            let _ = coordinator.player_name.push_str(c.player_name.as_str());
        });
        coordinator.lms_ip = lms_ip;
        coordinator.config = Some(config);
        coordinator.set_frame_buffer(frame_buf);
        // Decode/output run only in the DMA-first pump phases (`tasks.rs`), never inside tick.
        coordinator.external_audio_pump = true;
        coordinator
    }

    /// Bring-up stage: `esp-radio` Wi-Fi controller configured as a station for
    /// `wifi_sta_creds` (any configuration failure resets into recovery, as before).
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

    // The TAS58xx typestate transitions move ~320 B driver values around; when inlined into
    // `bring_up` they made up most of its frame. These wrappers keep the legacy call sequences
    // verbatim but in their own short-lived frames.

    /// HIL step: initialise the TAS58xx (32-bit) and set unity volume.
    #[inline(never)]
    fn amp_hil_init(amp: &mut AmpManager<Esp32I2cBus<'static>>) {
        let _ = amp.initialize(32);
        let _ = amp.set_volume(VOL_UNITY_0DB);
    }

    /// Apply the saved volume register and analog gain.
    #[inline(never)]
    fn amp_apply_boot_config(amp: &mut AmpManager<Esp32I2cBus<'static>>, vol_reg: u8, again_code: u8) {
        let _ = amp.set_volume(vol_reg);
        let _ = amp.set_analog_gain(again_code);
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

    /// Adds one socket to the set from its own frame (each smoltcp socket value is ~312 B and
    /// would otherwise be a separate temporary in [`init_net_stack`]'s frame).
    #[inline(never)]
    fn add_socket<T: smoltcp::socket::AnySocket<'static>>(
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
        udp_placeholder_handles: [SocketHandle; UDP_PLACEHOLDER_SOCKETS],
        local_port_counter: u16,
    }

    /// TCP socket buffers handed to [`init_net_stack`]: (rx, tx) for HTTP, slimproto, audio.
    type TcpBuffers = [(&'static mut [u8], &'static mut [u8]); 3];

    /// Bring-up stage: `smoltcp` Ethernet interface (in [`SMOL_IFACE`]) + DHCPv4 + TCP port 80 +
    /// TCP SlimProto (3483) + audio socket + the spec §3.6 UDP placeholders, all in
    /// [`SOCKET_STORAGE`].
    #[inline(never)]
    fn init_net_stack(sta_iface: &mut WifiInterface, sta_mac: [u8; 6], tcp_bufs: TcpBuffers) -> NetStack {
        let [(http_rx_buf, http_tx_buf), (slim_rx_buf, slim_tx_buf), (audio_rx_buf, audio_tx_buf)] =
            tcp_bufs;
        let hw_rng = Rng::new();
        let rng_seed = (u64::from(hw_rng.random()) << 32) | u64::from(hw_rng.random());
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(sta_mac));
        let mut smol_cfg = SmolIfaceConfig::new(hw_addr);
        smol_cfg.random_seed = rng_seed;
        let local_port_counter: u16 = 49152 + ((hw_rng.random() as u16) & 0x3FFF);
        let mut smol_dev = SmolDevice { driver: sta_iface };
        let smol_iface = SMOL_IFACE.init(SmolInterface::new(
            smol_cfg,
            &mut smol_dev,
            SmolInstant::from_millis(now_millis() as i64),
        ));

        let mut sockets = SocketSet::new(&mut SOCKET_STORAGE.take()[..]);

        let dhcp_handle = add_socket(&mut sockets, || dhcpv4::Socket::new());

        let http_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(tcp::SocketBuffer::new(http_rx_buf), tcp::SocketBuffer::new(http_tx_buf))
        });

        let slim_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(tcp::SocketBuffer::new(slim_rx_buf), tcp::SocketBuffer::new(slim_tx_buf))
        });

        let audio_handle = add_socket(&mut sockets, || {
            tcp::Socket::new(tcp::SocketBuffer::new(audio_rx_buf), tcp::SocketBuffer::new(audio_tx_buf))
        });

        // Spec §3.6: two UDP placeholder sockets (LMS discovery / mDNS). Allocated now so the RAM
        // budget measured from Task 8 on is the real one; never bound, so smoltcp ignores them
        // until P3 binds them.
        let udp_placeholder_handles = UDP_BUFFERS.take().each_mut().map(|bufs| {
            add_socket(&mut sockets, || {
                udp::Socket::new(
                    udp::PacketBuffer::new(&mut bufs.rx_meta[..], &mut bufs.rx_payload[..]),
                    udp::PacketBuffer::new(&mut bufs.tx_meta[..], &mut bufs.tx_payload[..]),
                )
            })
        });

        NetStack {
            smol_iface,
            sockets,
            dhcp_handle,
            http_handle,
            slim_handle,
            audio_handle,
            udp_placeholder_handles,
            local_port_counter,
        }
    }

    /// Device entry: bring-up, then the embassy executor that replaces the legacy superloop.
    pub fn run_hardware_firmware() -> ! {
        crate::tasks::run_core0_executor(bring_up())
    }

    /// Hardware bring-up, in the legacy order, ending in the initial superloop state (in
    /// [`SHARED`]). Heavy steps run in `#[inline(never)]` stages that build their results in
    /// statics, so this frame stays small (see "Boot-stack budget" above).
    #[inline(never)]
    fn bring_up() -> &'static Shared {
        let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
        let peripherals = esp_hal::init(config);

        // Park Core 1 (APP_CPU) immediately on entry in case a warm reboot from
        // dual-core ESP-IDF (`I2S-4MFlash-1681`) left `APP_CPU` unparked (`appcpu_clkgate_en=1`),
        // preventing `FlashStorage::multicore_auto_park()` from unparking a rogue `APP_CPU`
        // and ensuring `cpu_ctrl.start_app_core(...)` sees `is_running(Cpu::AppCpu) == false`.
        #[cfg(not(feature = "esp-rtos-core1"))]
        let mut cpu_ctrl = CpuControl::new(peripherals.CPU_CTRL);
        #[cfg(not(feature = "esp-rtos-core1"))]
        #[allow(unsafe_code)]
        unsafe {
            cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu);
        }
        #[cfg(feature = "esp-rtos-core1")]
        let mut cpu_ctrl_periph = peripherals.CPU_CTRL;
        #[cfg(feature = "esp-rtos-core1")]
        #[allow(unsafe_code)]
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
        // straight back into the untouched factory `recovery` partition (0x10000)!
        // ====================================================================
        let mut flash = FlashStorage::new(peripherals.FLASH).multicore_auto_park();
        if flash.erase(OTADATA_PARTITION_OFFSET, OTADATA_PARTITION_END).is_err() {
            println!("[ANTI-BRICK GATE 1] Failed to erase otadata -> resetting to recovery");
            esp_hal::system::software_reset();
        }
        let mut verify_s0 = [0u8; 64];
        let mut verify_s1 = [0u8; 64];
        if flash.read(OTADATA_PARTITION_OFFSET, &mut verify_s0).is_err()
            || flash.read(OTADATA_SECTOR_1_OFFSET, &mut verify_s1).is_err()
            || verify_otadata_erased(&verify_s0).is_err()
            || verify_otadata_erased(&verify_s1).is_err()
        {
            println!("[ANTI-BRICK GATE 1] otadata 0xFF read-back failed -> resetting to recovery");
            esp_hal::system::software_reset();
        }
        wdt.feed();
        println!("[ANTI-BRICK GATE 1] otadata (0xD000..0xF000) erased & verified 0xFF (Factory recovery pre-armed)");

        // Read 16 KB `nvs` partition (0x9000..0xD000) and `settings` (0x3FE000) while single-core
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

        // Boot config resolution (spec §3.4), each field independently: settings FlashJournal,
        // then (Wi-Fi only) the C firmware's NVS, then the compile-time first-boot defaults.
        let (config, journal) = {
            let boot_scratch: &mut [u8; 12_288] = (&mut ring_storage[..12_288]).try_into().unwrap();
            init_config(&mut flash, boot_scratch)
        };
        wdt.feed();
        let wifi_sta_creds = {
            let boot_scratch: &mut [u8; 12_288] = (&mut ring_storage[..12_288]).try_into().unwrap();
            init_wifi_credentials(&mut flash, boot_scratch, &sta_mac)
        };
        let (lms_ip, lms_port) = config.read(|c| {
            (
                c.lms_server_ip.unwrap_or(first_boot_defaults::LMS_SERVER_IP),
                c.lms_server_port.unwrap_or(first_boot_defaults::LMS_SERVER_PORT),
            )
        });
        ring_storage.fill(0);
        wdt.feed();

        // Allocate 64 KB heap in DRAM for `esp-rtos` and `esp-radio` Wi-Fi driver.
        // External PSRAM (`0x3F80_0000`) is registered only if the hardware MMU
        // (`DPORT_PRO_CACHE_CTRL1_REG` and `SPI0_CACHE_SCTRL_REG`) has already mapped
        // external SRAM (`>= 1_048_576` B), avoiding `esp-hal 1.2.2` `psram_gpio_config`
        // `.text` (`0x4013c190`) XIP instruction fetch corruption on 40 MHz DIO flash
        // and avoiding `esp-alloc` TLSF panic when `psram_size < 1_048_576`.
        #[allow(unsafe_code)]
        let psram_avail_bytes = {
            esp_alloc::heap_allocator!(size: 46 * 1024);
            unsafe {
                esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                    (*core::ptr::addr_of_mut!(STATIC_BT_HEAP)).as_mut_ptr() as *mut u8,
                    core::mem::size_of_val(&*core::ptr::addr_of!(STATIC_BT_HEAP)),
                    esp_alloc::MemoryCapability::Internal.into(),
                ));
            }
            const DPORT_PRO_CACHE_CTRL1_REG: *const u32 = 0x3FF0_0044 as *const u32;
            const SPI0_CACHE_SCTRL_REG: *const u32 = 0x3FF4_303C as *const u32;
            const EXTMEM_ORIGIN: usize = 0x3F80_0000;
            const EXTMEM_SIZE: usize = 4 * 1024 * 1024;
            let cache_ctrl1 = unsafe { core::ptr::read_volatile(DPORT_PRO_CACHE_CTRL1_REG) };
            let spi0_sctrl = unsafe { core::ptr::read_volatile(SPI0_CACHE_SCTRL_REG) };
            let ext_sram_mapped = (cache_ctrl1 & (1 << 1)) == 0 && (spi0_sctrl & (1 << 2)) != 0;
            if ext_sram_mapped {
                unsafe {
                    esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                        EXTMEM_ORIGIN as *mut u8,
                        EXTMEM_SIZE,
                        esp_alloc::MemoryCapability::External.into(),
                    ));
                }
                EXTMEM_SIZE
            } else {
                0
            }
        };
        let (bss_stream_storage, bss_output_storage) = ring_storage.split_at_mut(6144);
        let (stream_storage, output_storage): (&'static mut [u8], &'static mut [u8]) =
            if psram_avail_bytes >= 1_048_576 {
                (
                    alloc::boxed::Box::leak(alloc::vec![0u8; 524_288].into_boxed_slice()),
                    alloc::boxed::Box::leak(alloc::vec![0u8; 131_072].into_boxed_slice()),
                )
            } else {
                (bss_stream_storage, bss_output_storage)
            };
        wdt.feed();

        // Start Core 1 (APP_CPU @ 240 MHz) executing `app_core_loop` in IRAM (`#[esp_hal::ram]`).
        // Sync `DPORT_APP_FLASH_MMU_TABLE` (0x3FF1_2000..0x3FF1_2400) and `DPORT_APP_CACHE_CTRL1_REG`
        // (0x3FF0_005C) from `PRO_CPU` (0x3FF1_0000 and 0x3FF0_0044) while `APP_CPU` cache is disabled
        // so `APP_CPU` can fetch `start_core1_init` / `start_core1_run` from XIP Flash `.text`.
        #[allow(unsafe_code)]
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
        // Default: start core 1 now (manual path). Feature `esp-rtos-core1`: core 1 is started
        // after `esp_rtos::start` below, with the MMU/cache copy above already done (S1 gotcha 4).
        #[cfg(not(feature = "esp-rtos-core1"))]
        let core1 = {
            #[allow(unsafe_code)]
            let guard = cpu_ctrl.start_app_core(
                unsafe { &mut *core::ptr::addr_of_mut!(APP_CORE_STACK) },
                app_core_loop,
            );
            let active = guard.is_ok();
            Core1Ctl { cpu_ctrl, _guard: guard, active }
        };

        // Initialize GPIO33 PWDN, GPIO34 FAULT, I2C0 (SDA=21, SCL=27), I2S0 (BCK=26, WS=25, DOUT=22), UART2
        let mut pwdn = Output::new(peripherals.GPIO33, Level::High, OutputConfig::default());
        let fault = Input::new(peripherals.GPIO34, InputConfig::default().with_pull(Pull::None));

        let amp = init_amp_i2c(peripherals.I2C0, peripherals.GPIO21, peripherals.GPIO27);
        let (pipeline, http_resp_buf, http_tx_buf) = init_i2s_pipeline(
            peripherals.I2S0,
            peripherals.DMA_I2S0,
            peripherals.GPIO26,
            peripherals.GPIO25,
            peripherals.GPIO22,
            stream_storage,
            output_storage,
        );
        let cyd_bridge = init_cyd_uart(peripherals.UART2, peripherals.GPIO4, peripherals.GPIO36);

        // Execute 5-stage HIL self-verification
        pwdn.set_high();
        amp_hil_init(amp);
        let _ = fault.is_high();
        wdt.feed();

        let coordinator = init_coordinator(sta_mac, config, lms_ip);
        println!("[HIL 1/5..5/5] Hardware Peripherals (GPIO33/34, I2C0 0x2D, I2S0 26/25/22, UART2) Initialized");

        // Apply the boot-resolved audio configuration (volume, AGAIN, EQ, loudness).
        let (current_vol_pct, again_code, eq_preset, loudness) = config.read(|c| {
            (c.volume, c.analog_gain_code, c.equalizer_preset, c.loudness_enabled)
        });
        let init_vol_reg = percent_to_attenuation(current_vol_pct);
        amp_apply_boot_config(amp, init_vol_reg, again_code);
        pipeline.apply_eq_preset(EqPreset::from_u8(eq_preset));
        pipeline.set_loudness(loudness);
        let eff_gain = if current_vol_pct > 0 { 65536 } else { 0 };
        pipeline.set_volume(eff_gain, eff_gain);
        let _ = cyd_bridge.send_message(&CydMessage::Volume(current_vol_pct));

        // Start `esp-rtos` scheduler required by `esp-radio`
        esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
        wdt.feed();

        #[cfg(feature = "esp-rtos-core1")]
        let core1 = {
            #[allow(unsafe_code)]
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
        let bt_sink: &'static mut louder_esp32_app::bt_a2dp::BtA2dpSink = unsafe {
            let ptr = (*core::ptr::addr_of_mut!(STATIC_BT_SINK)).as_mut_ptr();
            ptr.write_bytes(0, 1);
            let sink = &mut *ptr;
            sink.reset_in_place();
            sink
        };

        // Initialize `smoltcp` Ethernet Interface + DHCPv4 + TCP Port 80 + TCP SlimProto (3483)
        let NetStack {
            smol_iface,
            sockets,
            dhcp_handle,
            http_handle,
            slim_handle,
            audio_handle,
            udp_placeholder_handles,
            local_port_counter,
        } = init_net_stack(
            &mut sta_iface,
            sta_mac,
            [
                (&mut http_rx_buf[..], &mut http_tx_buf[..]),
                (&mut slim_rx_buf[..], &mut slim_tx_buf[..]),
                (&mut audio_rx_buf[..], &mut audio_tx_buf[..]),
            ],
        );

        let boot_ms = now_millis();
        pipeline.sink.driver.pump_silence_if_needed(false, false);
        let latest_telemetry = amp_start_playback(amp);

        // ====================================================================
        // GATE 3 & GATE 4: Non-Blocking Runtime Loop with 45s/60s Dead-Man
        // Lease Watchdog, Port 80 HTTP Recovery Server, and SlimProto Client.
        // The loop body now runs as the embassy phase ring in `tasks.rs`; this is
        // its initial state (the legacy pre-loop `let mut` initializers).
        // ====================================================================
        SHARED.init(Shared::new(FirmwareState {
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
            _udp_placeholder_handles: udp_placeholder_handles,
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
            wifi_sta_creds,
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
            now_ms: 0,
        }))
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

        // Push PLAY command via UART RX and 1 stereo frame (4 bytes of 16-bit PCM) into stream buffer
        cyd_bridge.port.push_rx(b"CMD=PLAY\n");
        pipeline.ingest_stream_data(&[0x01, 0x00, 0xFF, 0x00]).unwrap();

        let frames = run_firmware_tick(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp);
        assert_eq!(coordinator.state, PlayerState::Playing);
        assert_eq!(frames, 1);
        assert_eq!(cyd_bridge.port.tx_bytes(), b"STATE=P\n");
    }

    #[test]
    fn test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate() {
        use std::io::Read;
        use std::net::TcpListener;
        use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};
        use squeezelite_tas58xx::registers;

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
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
            // check_faults returns a non-clean fault (overcurrent on channel)
            I2cTrans::write_read(addr, vec![registers::CHAN_FAULT], vec![0x01, 0x00, 0x00, 0x00]),
            // recover_from_fault: Hi-Z -> clear fault -> Play
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x02]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
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
        // Task 11: built from what the firmware can do (no HasDigitalOut until SPDIF exists,
        // MaxSampleRate from the verified clock table).
        let caps = PlayerCoordinator::helo_capabilities(None).unwrap();
        assert!(caps.contains(",flc,"), "Capabilities must advertise native FLAC (,flc,)");
        assert!(caps.contains(&format!(
            "MaxSampleRate={}",
            louder_esp32_app::clock_table::max_verified_rate()
        )));
        assert!(caps.contains(",pcm,"), "Capabilities must advertise pcm");
        assert!(caps.contains(",mp3"), "Capabilities must advertise mp3");
        assert!(!caps.contains("HasDigitalOut"));
        assert!(caps.starts_with("Model=squeezeesp32,"));
    }
}

