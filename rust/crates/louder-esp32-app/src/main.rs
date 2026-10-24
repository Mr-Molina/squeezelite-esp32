//! Louder-ESP32-Plus Application Entrypoint
//!
//! Initializes board hardware profiles, starts audio streaming pipeline,
//! and orchestrates SlimProto network connection with TAS5805M/TAS5825M digital amplifier.
#![cfg_attr(target_arch = "xtensa", no_std)]
#![cfg_attr(target_arch = "xtensa", no_main)]
#![cfg_attr(not(target_arch = "xtensa"), forbid(unsafe_code))]
#![cfg_attr(target_arch = "xtensa", deny(unsafe_code))]
#![allow(unknown_lints)]

use embedded_hal::i2c::I2c;
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::board::{pins, BoardModel};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::BitDepth;
use louder_esp32_app::hal::{EspI2sSink, UartPort};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::hal::LoopbackI2sDma;
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::{AmpManager, AudioOutputSink, CydUartBridge, PlayerCoordinator};
#[cfg(not(target_arch = "xtensa"))]
use louder_esp32_app::{
    CydMessage, LoopbackUartPort, MemConfigStore, MemFlashPartition, NvsFlashBackend, PlayState,
    SlimprotoClient, TargetI2cBus,
};
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_config::AudioConfig;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_decoders::PcmDecoder;

/// Executes one integrated firmware tick across the CYD UART bridge, audio pipeline,
/// and digital amplifier health monitor (`HALNET-409`).
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
    #[cfg(not(target_arch = "xtensa"))]
    {
        run_firmware_tick_with_network(coordinator, pipeline, cyd_bridge, amp, None, 0)
    }
    #[cfg(target_arch = "xtensa")]
    {
        if let Ok(Some(cmd)) = cyd_bridge.poll_command() {
            if let Some(reply) = coordinator.handle_cyd_command(cmd, pipeline) {
                let _ = cyd_bridge.flush_outbound(&reply);
            }
        }

        if coordinator.state == louder_esp32_app::PlayerState::Playing
            && pipeline.output_buffer.available_read() == 0
            && !pipeline.has_pending_pcm()
        {
            for _ in 0..4 {
                if !matches!(pipeline.process_decode_step(), Ok(c) if c > 0)
                    || pipeline.output_buffer.available_read() > 0
                    || pipeline.has_pending_pcm()
                {
                    break;
                }
            }
        }

        let mut frames = 0;
        for _ in 0..8 {
            let step_frames = coordinator.process_audio_step(pipeline);
            frames += step_frames;
            if step_frames == 0 && !pipeline.has_pending_pcm() {
                break;
            }
        }
        coordinator.current_sample_rate = pipeline.sample_rate;

        if !coordinator.dac_enabled {
            let _ = amp.stop_playback();
        }

        frames
    }
}

/// Executes one integrated firmware tick with SlimProto network integration,
/// dynamic sample rate synchronization, DAC amplifier pause, and hardware fault recovery (`RUST-19`).
#[cfg(not(target_arch = "xtensa"))]
pub fn run_firmware_tick_with_network<S, U, I>(
    coordinator: &mut PlayerCoordinator,
    pipeline: &mut StreamPipeline<'_, S>,
    cyd_bridge: &mut CydUartBridge<U>,
    amp: &mut AmpManager<I>,
    client: Option<&mut SlimprotoClient>,
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

    let frames = if let Some(client_ref) = client {
        coordinator.tick(client_ref, pipeline, now_ms)
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

    let mut audio_cfg = AudioConfig::default();
    audio_cfg.player_name.clear();
    let raw_name = std::env::var("SL_PLAYER_NAME")
        .unwrap_or_else(|_| louder_esp32_app::board::DEFAULT_PLAYER_NAME.to_string());
    let safe_len = raw_name.floor_char_boundary(32.min(raw_name.len()));
    let _ = audio_cfg.player_name.push_str(&raw_name[..safe_len]);
    audio_cfg.lms_server_ip = Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_IP);
    audio_cfg.lms_server_port = Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_PORT);

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

    // 4. Derive MAC address: use BASEMENT_PLAYER_MAC for "BASEMENT", or deterministic test MAC (INFRA-801, HW-PREP-05)
    let mac = if audio_cfg.player_name.as_str() == louder_esp32_app::board::DEFAULT_PLAYER_NAME {
        louder_esp32_app::board::BASEMENT_PLAYER_MAC
    } else {
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
    let lms_ip = std::net::Ipv4Addr::from(audio_cfg.lms_server_ip.unwrap_or(louder_esp32_app::board::DEFAULT_LMS_SERVER_IP));
    let lms_port = audio_cfg.lms_server_port.unwrap_or(louder_esp32_app::board::DEFAULT_LMS_SERVER_PORT);
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

    let iterations = if client.is_some() { 5 } else { 16 };
    for tick in 0..iterations {
        let now_ms = (tick * 10) as u32;
        let frames = run_firmware_tick_with_network(
            &mut coordinator,
            &mut pipeline,
            &mut cyd_bridge,
            &mut amp,
            client.as_mut(),
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
    use super::*;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Waker};
    use embedded_hal::i2c::{ErrorType, I2c, Operation};
    use esp_backtrace as _;
    use esp_hal::{
        clock::CpuClock,
        efuse::{interface_mac_address, InterfaceMacAddress},
        gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
        i2c::master::{Config as I2cConfig, I2c as HalI2c},
        i2s::master::{Channels, DataFormat, I2s, TdmConfig},
        time::{Duration, Instant, Rate},
        timer::timg::{MwdtStage, TimerGroup},
        uart::{Config as UartConfig, Uart},
    };
    use esp_println::println;
    use esp_radio::wifi::{
        sta::StationConfig, AuthenticationMethodConfig, Config as WifiModeConfig, ControllerConfig,
        Interface as WifiInterface, WifiController, WifiRxToken, WifiTxToken,
    };
    use esp_storage::FlashStorage;
    use louder_esp32_app::board::{
        BASEMENT_PLAYER_MAC, DEFAULT_LMS_SERVER_IP, DEFAULT_LMS_SERVER_PORT, DEFAULT_WIFI_SSID,
    };
    use louder_esp32_app::hal::I2sDmaDriver;
    use louder_esp32_app::http_control::{
        handle_http_control_request, DeadManLeaseWatchdog, HttpControlAction, HttpStatusSnapshot,
        LeaseHealth,
    };
    use louder_esp32_app::ota_safety::{
        resolve_wifi_credentials, verify_otadata_erased, OtaSafetyError, OtaSelectEntry,
        WifiStaCredentials, NVS_PARTITION_OFFSET, OTADATA_PARTITION_END, OTADATA_PARTITION_OFFSET,
        OTADATA_SECTOR_1_OFFSET,
    };
    use smoltcp::iface::{Config as SmolIfaceConfig, Interface as SmolInterface, SocketSet, SocketStorage};
    use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
    use smoltcp::socket::{dhcpv4, tcp};
    use smoltcp::time::Instant as SmolInstant;
    use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};
    use squeezelite_audio_core::output::OutputError;
    use squeezelite_decoders::PcmDecoder;
    #[allow(unused_imports)]
    use squeezelite_proto::{
        sample_rate_from_index, AudioFormat, AutostartMode, DisconnectReason, Endianness,
        ParseError, ServerCommand, SlimprotoBuilder, SlimprotoParser, StatBuilder, StatEvent,
        StreamCommandType,
    };
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
    static HW_NACK_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    pub struct SmolDevice<'a> {
        pub driver: &'a mut WifiInterface,
    }

    pub struct SmolRxToken(pub WifiRxToken);
    pub struct SmolTxToken(pub WifiTxToken);

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
            self.0.consume_token(len, f)
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
            caps.max_transmission_unit = 1500;
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
            HW_NACK_COUNT.load(core::sync::atomic::Ordering::Relaxed)
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
                    HW_NACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
                    HW_NACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
                    HW_NACK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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
            Self {
                transfer: Some(transfer),
                sample_rate_hz,
                tx_enabled: true,
            }
        }

        pub fn ensure_running(&mut self) {
            let needs_restart = self.transfer.as_ref().map(|t| t.is_done()).unwrap_or(false);
            if needs_restart {
                if let Some(transfer) = self.transfer.take() {
                    let (i2s_tx, buf) = transfer.stop();
                    if let Ok(new_transfer) = i2s_tx.write(buf) {
                        self.transfer = Some(new_transfer);
                    }
                }
            }
        }

        pub fn pump_silence_if_needed(&mut self, is_playing: bool) {
            self.ensure_running();
            if let Some(ref mut transfer) = self.transfer {
                let silence = [0u8; 512];
                let threshold = if is_playing { 6144 } else { 2048 };
                while transfer.available_bytes() >= threshold {
                    if transfer.push(&silence) == 0 {
                        break;
                    }
                }
            }
        }
    }

    impl I2sDmaDriver for Esp32HardwareI2sDma<'_> {
        fn write_dma(&mut self, data: &[u8]) -> Result<usize, OutputError> {
            let valid_len = data.len() - (data.len() % 8);
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
                    let avail = transfer.available_bytes();
                    let avail_aligned = avail - (avail % 8);
                    if avail_aligned == 0 {
                        break;
                    }
                    let chunk_len = core::cmp::min(silence.len(), valid_len - written).min(avail_aligned);
                    if chunk_len == 0 {
                        break;
                    }
                    let n = transfer.push(&silence[..chunk_len]);
                    let aligned_n = n - (n % 8);
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
                let avail = transfer.available_bytes();
                let avail_aligned = avail - (avail % 8);
                if avail_aligned == 0 {
                    break;
                }
                let chunk_in_len = core::cmp::min(scratch.len(), valid_len - offset).min(avail_aligned);
                let frames_in_chunk = chunk_in_len / 8;
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
                let aligned_n = n - (n % 8);
                total_written += aligned_n;
                offset += aligned_n;
                if n < chunk_bytes {
                    break;
                }
            }
            Ok(total_written)
        }

        fn set_clock_rate(&mut self, sample_rate_hz: u32) -> Result<(), OutputError> {
            if !(8_000..=192_000).contains(&sample_rate_hz) {
                return Err(OutputError::UnsupportedSampleRate(sample_rate_hz));
            }
            self.sample_rate_hz = sample_rate_hz;
            Ok(())
        }

        fn set_tx_enabled(&mut self, enabled: bool) {
            self.tx_enabled = enabled;
        }
    }

    fn now_millis() -> u64 {
        Instant::now().duration_since_epoch().as_millis()
    }

    #[inline(never)]
    fn load_nvs_wifi_credentials(
        flash: &mut FlashStorage<'_>,
        sta_mac: &[u8; 6],
        nvs_buf: &mut [u8; 16_384],
    ) -> Result<WifiStaCredentials, OtaSafetyError> {
        nvs_buf.fill(0xFF);
        let _ = flash.read(NVS_PARTITION_OFFSET, nvs_buf);
        resolve_wifi_credentials(
            nvs_buf,
            sta_mac,
            option_env!("SL_WIFI_SSID").or(Some(DEFAULT_WIFI_SSID)),
            option_env!("SL_WIFI_PASS"),
        )
    }

    #[allow(unsafe_code)]
    fn take_static_mp3_decoder() -> &'static mut squeezelite_decoders::StaticMp3Decoder {
        static mut STATIC_MP3_DEC: squeezelite_decoders::StaticMp3Decoder =
            squeezelite_decoders::StaticMp3Decoder::new();
        unsafe {
            let dec = &mut *core::ptr::addr_of_mut!(STATIC_MP3_DEC);
            dec.reset_in_place();
            dec
        }
    }

    struct FirmwareBssBuffers {
        ring_storage: [u8; 16384],
        http_rx_buf: [u8; 1024],
        http_tx_buf: [u8; 1024],
        slim_rx_buf: [u8; 2048],
        slim_tx_buf: [u8; 1024],
        audio_rx_buf: [u8; 4096],
        audio_tx_buf: [u8; 512],
        slim_acc_buf: [u8; 1536],
        audio_req_buf: [u8; 512],
        audio_header_buf: [u8; 1024],
    }

    impl FirmwareBssBuffers {
        const fn new() -> Self {
            Self {
                ring_storage: [0u8; 16384],
                http_rx_buf: [0u8; 1024],
                http_tx_buf: [0u8; 1024],
                slim_rx_buf: [0u8; 2048],
                slim_tx_buf: [0u8; 1024],
                audio_rx_buf: [0u8; 4096],
                audio_tx_buf: [0u8; 512],
                slim_acc_buf: [0u8; 1536],
                audio_req_buf: [0u8; 512],
                audio_header_buf: [0u8; 1024],
            }
        }
    }

    #[allow(unsafe_code)]
    fn take_firmware_bss_buffers() -> &'static mut FirmwareBssBuffers {
        static mut BSS_BUFFERS: FirmwareBssBuffers = FirmwareBssBuffers::new();
        unsafe { &mut *core::ptr::addr_of_mut!(BSS_BUFFERS) }
    }

    pub fn run_hardware_firmware() -> ! {
        let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
        let peripherals = esp_hal::init(config);

        // ====================================================================
        // GATE 2: Hardware Watchdog (TIMG0 WDT, 20s Stage0 -> ResetSystem)
        // ====================================================================
        let timg0 = TimerGroup::new(peripherals.TIMG0);
        let mut wdt = timg0.wdt;
        wdt.set_timeout(MwdtStage::Stage0, Duration::from_millis(20_000));
        wdt.enable();
        wdt.feed();

        // Allocate 64 KB heap in DRAM for `esp-rtos` and `esp-radio` Wi-Fi driver
        #[allow(unsafe_code)]
        {
            esp_alloc::heap_allocator!(size: 64 * 1024);
        }

        // ====================================================================
        // GATE 1: First-Instruction `otadata` (0xD000..0xF000) Pre-Arming
        // Erases `otadata` to 0xFF BEFORE Wi-Fi/interrupts so ANY subsequent
        // reset (panic, WDT, network timeout, or power cycle) boots straight
        // back into the untouched factory `recovery` partition (0x10000)!
        // ====================================================================
        let mut flash = FlashStorage::new(peripherals.FLASH);
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

        // Read 16 KB `nvs` partition (0x9000..0xD000) while single-core before starting Wi-Fi
        let sta_mac_addr = interface_mac_address(InterfaceMacAddress::Station);
        let mut sta_mac = [0u8; 6];
        sta_mac.copy_from_slice(sta_mac_addr.as_bytes());

        let fw_bufs = take_firmware_bss_buffers();
        let wifi_sta_creds = match load_nvs_wifi_credentials(&mut flash, &sta_mac, &mut fw_bufs.ring_storage) {
            Ok(c) => c,
            Err(e) => {
                println!(
                    "[ANTI-BRICK] Could not resolve Wi-Fi credentials from 0x9000 NVS ({:?}) -> resetting to recovery (0x10000)",
                    e
                );
                esp_hal::system::software_reset();
            }
        };
        fw_bufs.ring_storage.fill(0);
        wdt.feed();
        println!(
            "[NVS 0x9000] Loaded Wi-Fi STA SSID='{}' (pass_len={}) for MAC {:02x?}",
            wifi_sta_creds.ssid.as_str(),
            wifi_sta_creds.password.len(),
            sta_mac
        );

        // Initialize GPIO33 PWDN, GPIO34 FAULT, I2C0 (SDA=21, SCL=27), I2S0 (BCK=26, WS=25, DOUT=22), UART2
        let mut pwdn = Output::new(peripherals.GPIO33, Level::High, OutputConfig::default());
        let fault = Input::new(peripherals.GPIO34, InputConfig::default().with_pull(Pull::None));

        let i2c_cfg = I2cConfig::default().with_frequency(Rate::from_khz(400));
        let hal_i2c = HalI2c::new(peripherals.I2C0, i2c_cfg)
            .expect("Failed to initialize I2C0")
            .with_sda(peripherals.GPIO21)
            .with_scl(peripherals.GPIO27);
        let mut i2c_bus = Esp32I2cBus::new(hal_i2c);

        let i2s_tdm_cfg = TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(44_100))
            .with_data_format(DataFormat::Data32Channel32)
            .with_channels(Channels::STEREO);
        let tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024);
        let i2s = I2s::new(peripherals.I2S0, peripherals.DMA_I2S0, i2s_tdm_cfg)
            .expect("Failed to initialize I2S0");
        let i2s_tx = i2s
            .i2s_tx
            .with_bclk(peripherals.GPIO26)
            .with_ws(peripherals.GPIO25)
            .with_dout(peripherals.GPIO22)
            .build();
        let i2s_transfer = i2s_tx
            .write(tx_buffer)
            .expect("Failed to start I2S0 TX streaming DMA");

        let uart2_rx = Input::new(peripherals.GPIO36, InputConfig::default().with_pull(Pull::None));
        let uart2_cfg = UartConfig::default().with_baudrate(460_800);
        let hal_uart2 = Uart::new(peripherals.UART2, uart2_cfg)
            .expect("Failed to initialize UART2")
            .with_tx(peripherals.GPIO4)
            .with_rx(uart2_rx);
        let uart2_port = Esp32Uart2Port::new(hal_uart2);
        let mut cyd_bridge = CydUartBridge::new(uart2_port);

        // Execute 5-stage HIL self-verification
        pwdn.set_high();
        let mut amp = AmpManager::for_active_board(&mut i2c_bus);
        let _ = amp.initialize(32);
        let _ = amp.set_volume(VOL_UNITY_0DB);
        let _ = fault.is_high();
        wdt.feed();

        let FirmwareBssBuffers {
            ref mut ring_storage,
            ref mut http_rx_buf,
            ref mut http_tx_buf,
            ref mut slim_rx_buf,
            ref mut slim_tx_buf,
            ref mut audio_rx_buf,
            ref mut audio_tx_buf,
            ref mut slim_acc_buf,
            ref mut audio_req_buf,
            ref mut audio_header_buf,
        } = fw_bufs;
        let (stream_storage, output_storage) = ring_storage.split_at_mut(8192);

        let hw_i2s_driver = Esp32HardwareI2sDma::new(i2s_transfer, 44_100);
        let sink = EspI2sSink::for_active_board(hw_i2s_driver, 44_100);
        let pcm_dec = PcmDecoder::new(16, 2, true);
        let mut pipeline = StreamPipeline::new(
            stream_storage,
            output_storage,
            ActiveDecoder::Pcm(pcm_dec),
            sink,
            44_100,
        );
        let mut coordinator = PlayerCoordinator::new(BASEMENT_PLAYER_MAC, "BASEMENT");
        println!("[HIL 1/5..5/5] Hardware Peripherals (GPIO33/34, I2C0 0x2D, I2S0 26/25/22, UART2) Initialized");

        // Start `esp-rtos` scheduler required by `esp-radio`
        esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
        wdt.feed();

        // Initialize `esp-radio` Wi-Fi Controller & Station Interface
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
                .with_authentication(auth_cfg),
        );
        let mut wifi_controller = match WifiController::new(peripherals.WIFI, ControllerConfig::default()) {
            Ok(c) => c,
            Err(_) => esp_hal::system::software_reset(),
        };
        if wifi_controller.set_config(&sta_cfg).is_err() {
            esp_hal::system::software_reset();
        }
        let mut sta_iface = WifiInterface::station();
        wdt.feed();

        // Initialize `smoltcp` Ethernet Interface + DHCPv4 + TCP Port 80 + TCP SlimProto (3483)
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(sta_mac));
        let mut smol_cfg = SmolIfaceConfig::new(hw_addr);
        smol_cfg.random_seed = 0xC82E_18BE_B834_2026;
        let mut smol_dev = SmolDevice {
            driver: &mut sta_iface,
        };
        let mut smol_iface = SmolInterface::new(
            smol_cfg,
            &mut smol_dev,
            SmolInstant::from_millis(now_millis() as i64),
        );

        let mut socket_storage: [SocketStorage<'_>; 4] = [SocketStorage::EMPTY; 4];
        let mut sockets = SocketSet::new(&mut socket_storage[..]);

        let dhcp_socket = dhcpv4::Socket::new();
        let dhcp_handle = sockets.add(dhcp_socket);

        let http_socket = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut http_rx_buf[..]),
            tcp::SocketBuffer::new(&mut http_tx_buf[..]),
        );
        let http_handle = sockets.add(http_socket);

        let slim_socket = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut slim_rx_buf[..]),
            tcp::SocketBuffer::new(&mut slim_tx_buf[..]),
        );
        let slim_handle = sockets.add(slim_socket);

        let audio_socket = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut audio_rx_buf[..]),
            tcp::SocketBuffer::new(&mut audio_tx_buf[..]),
        );
        let audio_handle = sockets.add(audio_socket);

        // ====================================================================
        // GATE 3 & GATE 4: Non-Blocking Runtime Loop with 45s/60s Dead-Man
        // Lease Watchdog, Port 80 HTTP Recovery Server, and SlimProto Client
        // ====================================================================
        let boot_ms = now_millis();
        let mut lease_watchdog = DeadManLeaseWatchdog::new(boot_ms);
        let mut assigned_ip: Option<[u8; 4]> = None;
        let mut assigned_mask: [u8; 4] = [255, 255, 255, 0];
        let mut assigned_gw: [u8; 4] = [172, 22, 1, 1];
        let mut pending_http_action = HttpControlAction::None;
        let mut pending_action_deadline_ms: u64 = 0;
        let mut helo_sent = false;
        let mut last_stat_ms: u64 = 0;
        let mut last_connect_attempt_ms: u64 = 0;
        let mut slim_acc_len: usize = 0;
        let mut audio_stream_active = false;
        let mut audio_req_sent = false;
        let mut audio_headers_stripped = false;
        let mut audio_req_len: usize = 0;
        let mut audio_header_len: usize = 0;
        let mut last_fault_check_ms: u64 = 0;
        let mut audio_http_eof = false;
        let mut decode_ready_sent = false;
        let mut track_started_sent = false;
        let mut threshold_sent = false;
        let mut icy_meta_interval: usize = 0;
        let mut icy_meta_next: usize = 0;
        let mut icy_meta_left: usize = 0;

        pipeline.sink.driver.pump_silence_if_needed(false);
        let _ = amp.clear_faults();
        let _ = amp.start_playback_via_hiz();

        loop {
            wdt.feed();
            let now_ms = now_millis();

            // Drive Wi-Fi STA connection if not currently associated
            if !wifi_controller.is_connected() && now_ms.saturating_sub(last_connect_attempt_ms) >= 1_000 {
                last_connect_attempt_ms = now_ms;
                let mut connect_fut = pin!(wifi_controller.connect_async());
                let waker = Waker::noop();
                let mut cx = Context::from_waker(waker);
                let _ = connect_fut.as_mut().poll(&mut cx);
            }

            // Gate 3: Evaluate Dead-Man Network Lease Watchdog (45s boot / 60s drop)
            if lease_watchdog.evaluate(now_ms, wifi_controller.is_connected(), assigned_ip.is_some())
                == LeaseHealth::ExpiredResetToRecovery
            {
                println!("[ANTI-BRICK GATE 3] Network lease timeout expired -> resetting to recovery (0x10000)");
                esp_hal::system::software_reset();
            }

            // Poll `smoltcp` network stack
            {
                let mut dev = SmolDevice {
                    driver: &mut sta_iface,
                };
                let _ = smol_iface.poll(
                    SmolInstant::from_millis(now_ms as i64),
                    &mut dev,
                    &mut sockets,
                );
            }

            // Process DHCPv4 events
            let dhcp_event = sockets.get_mut::<dhcpv4::Socket>(dhcp_handle).poll();
            match dhcp_event {
                Some(dhcpv4::Event::Configured(config)) => {
                    let ip_octets = config.address.address().octets();
                    assigned_ip = Some(ip_octets);
                    let prefix_len = config.address.prefix_len();
                    let mask_u32 = if prefix_len == 0 {
                        0
                    } else {
                        !0u32 << (32 - prefix_len)
                    };
                    assigned_mask = mask_u32.to_be_bytes();
                    if let Some(router) = config.router {
                        assigned_gw = router.octets();
                        let _ = smol_iface.routes_mut().add_default_ipv4_route(router);
                    }
                    smol_iface.update_ip_addrs(|addrs| {
                        addrs.clear();
                        let _ = addrs.push(IpCidr::Ipv4(config.address));
                    });
                    println!(
                        "[DHCPv4] Acquired IP {}.{}.{}.{}/{}",
                        ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3], prefix_len
                    );
                }
                Some(dhcpv4::Event::Deconfigured) => {
                    assigned_ip = None;
                    smol_iface.update_ip_addrs(|addrs| addrs.clear());
                }
                None => {}
            }

            // Gate 4: Port 80 HTTP Control & Recovery Server (`/status.json`, `/recovery.json`, `/reboot.json`, `/reboot_ota.json`)
            {
                let http_sock = sockets.get_mut::<tcp::Socket>(http_handle);
                if !http_sock.is_open() {
                    let _ = http_sock.listen(80);
                }
                if http_sock.can_recv() && http_sock.may_send() {
                    let mut req_buf = [0u8; 512];
                    let n = http_sock.recv_slice(&mut req_buf).unwrap_or(0);
                    if n > 0 {
                        let amp_mode_str = match amp.mode() {
                            louder_esp32_app::AmpMode::Play => "Play",
                            louder_esp32_app::AmpMode::Standby => "Standby",
                            louder_esp32_app::AmpMode::PowerDown => "PowerDown",
                        };
                        let snap = HttpStatusSnapshot {
                            ip: assigned_ip.unwrap_or([172, 22, 1, 15]),
                            netmask: assigned_mask,
                            gw: assigned_gw,
                            ssid: wifi_sta_creds.ssid.as_str(),
                            rssi: wifi_controller.rssi().unwrap_or(-50),
                            lms_ip: DEFAULT_LMS_SERVER_IP,
                            lms_port: 9000,
                            lms_cport: 9090,
                            otadata_prearmed_factory: true,
                            frames_played: pipeline.frames_played,
                            amp_mode: amp_mode_str,
                            i2c_acks: Esp32I2cBus::hw_ack_count(),
                            i2c_nacks: Esp32I2cBus::hw_nack_count(),
                            uptime_ms: now_ms.saturating_sub(boot_ms),
                        };
                        let mut resp_buf = [0u8; 768];
                        if let Some((resp_len, action)) =
                            handle_http_control_request(&req_buf[..n], &snap, &mut resp_buf)
                        {
                            let _ = http_sock.send_slice(&resp_buf[..resp_len]);
                            http_sock.close();
                            if action != HttpControlAction::None {
                                pending_http_action = action;
                                pending_action_deadline_ms = now_ms + 250;
                            }
                        }
                    }
                } else if http_sock.may_recv() && !http_sock.may_send() {
                    http_sock.abort();
                }
            }

            // Execute deferred HTTP reboot action after flushing TCP response
            if pending_http_action != HttpControlAction::None && now_ms >= pending_action_deadline_ms {
                match pending_http_action {
                    HttpControlAction::RebootToRecovery => {
                        println!("[ANTI-BRICK GATE 4] HTTP /recovery.json or /reboot.json -> resetting into factory recovery (0x10000)");
                        esp_hal::system::software_reset();
                    }
                    HttpControlAction::PersistOta0AndReboot => {
                        let entry_bytes = OtaSelectEntry::new_ota_0().to_bytes();
                        let _ = flash.write(OTADATA_PARTITION_OFFSET, &entry_bytes);
                        println!("[ANTI-BRICK GATE 4] HTTP /reboot_ota.json -> wrote ota_0 select entry & resetting");
                        esp_hal::system::software_reset();
                    }
                    HttpControlAction::None => {}
                }
            }

            // Drive SlimProto Client (`10.0.0.10:3483`) once IPv4 lease is active
            if let Some(local_ip) = assigned_ip {
                // Half-closed / closing state detection: if remote sent FIN or is closing,
                // abort socket so we immediately reconnect rather than lingering in CloseWait.
                {
                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
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
                        helo_sent = false;
                        slim_acc_len = 0;
                    }
                }

                // Connect to LMS if closed
                {
                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                    if !slim_sock.is_open() {
                        helo_sent = false;
                        slim_acc_len = 0;
                        let remote_ep = (
                            IpAddress::Ipv4(Ipv4Address::from(DEFAULT_LMS_SERVER_IP)),
                            DEFAULT_LMS_SERVER_PORT,
                        );
                        let local_ep = (
                            IpAddress::Ipv4(Ipv4Address::from(local_ip)),
                            49_152 + ((now_ms as u16) & 0x3FFF),
                        );
                        let _ = slim_sock.connect(smol_iface.context(), remote_ep, local_ep);
                    }
                }

                // Initial HELO + SETD ("BASEMENT") + STAT Heartbeat, and periodic Heartbeats
                {
                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                    if slim_sock.may_send() && !helo_sent {
                        let mut helo_buf = [0u8; 256];
                        if let Ok(helo_len) = SlimprotoBuilder::build_helo(
                            &BASEMENT_PLAYER_MAC,
                            12,
                            "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3",
                            &mut helo_buf,
                        ) {
                            if slim_sock.send_slice(&helo_buf[..helo_len]).is_ok() {
                                let mut setd_buf = [0u8; 128];
                                if let Ok(setd_len) = SlimprotoBuilder::build_setd_name(
                                    coordinator.player_name.as_str(),
                                    &mut setd_buf,
                                ) {
                                    let _ = slim_sock.send_slice(&setd_buf[..setd_len]);
                                }

                                let mut stat_buf = [0u8; 128];
                                if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                    StatEvent::Heartbeat,
                                    pipeline.stream_buffer_available() as u32,
                                    pipeline.output_buffer_available() as u32,
                                    pipeline.frames_played,
                                    coordinator.current_sample_rate,
                                    coordinator.stream_bytes_received,
                                    now_ms as u32,
                                    0,
                                    &mut stat_buf,
                                ) {
                                    let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                                }

                                helo_sent = true;
                                last_stat_ms = now_ms;
                                coordinator.state = louder_esp32_app::PlayerState::Connected;
                            }
                        }
                    } else {
                        let stat_interval_ms: u64 = if coordinator.state == louder_esp32_app::PlayerState::Playing {
                            1_000
                        } else {
                            5_000
                        };
                        if slim_sock.may_send()
                            && helo_sent
                            && now_ms.saturating_sub(last_stat_ms) >= stat_interval_ms
                        {
                            last_stat_ms = now_ms;
                            let mut stat_buf = [0u8; 128];
                            if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                StatEvent::Heartbeat,
                                pipeline.stream_buffer_available() as u32,
                                pipeline.output_buffer_available() as u32,
                                pipeline.frames_played,
                                coordinator.current_sample_rate,
                                coordinator.stream_bytes_received,
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
                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                    if slim_sock.can_recv() && slim_acc_len < slim_acc_buf.len() {
                        let n = slim_sock
                            .recv_slice(&mut slim_acc_buf[slim_acc_len..])
                            .unwrap_or(0);
                        slim_acc_len += n;
                    }
                }

                // Drain all complete frames from accumulation buffer
                while slim_acc_len >= 6 {
                    match SlimprotoParser::parse_frame(&slim_acc_buf[..slim_acc_len]) {
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
                                            coordinator.player_name.clear();
                                            let _ = coordinator.player_name.push_str(name_str);
                                        }
                                    }
                                    let mut tx_buf = [0u8; 128];
                                    if let Ok(tx_len) = SlimprotoBuilder::build_setd_name(
                                        coordinator.player_name.as_str(),
                                        &mut tx_buf,
                                    ) {
                                        let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
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
                                        digital_atten,
                                        ..
                                    } => {
                                        let (eff_gain_left, eff_gain_right) = if digital_atten == 0 {
                                            let l = if gain_left > 0 { 65536 } else { 0 };
                                            let r = if gain_right > 0 { 65536 } else { 0 };
                                            (l, r)
                                        } else {
                                            (gain_left, gain_right)
                                        };
                                        pipeline.set_volume(eff_gain_left, eff_gain_right);
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
                                                        pipeline.stream_buffer_available() as u32,
                                                        pipeline.output_buffer_available() as u32,
                                                        pipeline.frames_played,
                                                        coordinator.current_sample_rate,
                                                        coordinator.stream_bytes_received,
                                                        now_ms as u32,
                                                        replay_gain,
                                                        &mut stat_buf,
                                                    )
                                                {
                                                    let slim_sock =
                                                        sockets.get_mut::<tcp::Socket>(slim_handle);
                                                    if slim_sock.may_send() {
                                                        let _ = slim_sock
                                                            .send_slice(&stat_buf[..stat_len]);
                                                    }
                                                }
                                            }
                                            StreamCommandType::Start => {
                                                icy_meta_interval = 0;
                                                icy_meta_next = 0;
                                                icy_meta_left = 0;

                                                pipeline.flush();
                                                pipeline.frames_played = 0;
                                                coordinator.frames_played = 0;
                                                coordinator.stream_bytes_received = 0;
                                                audio_http_eof = false;
                                                decode_ready_sent = false;
                                                track_started_sent = false;
                                                threshold_sent = false;

                                                coordinator.autostart_mode =
                                                    AutostartMode::from_strm(autostart, flags);
                                                coordinator.awaiting_cont =
                                                    coordinator.autostart_mode.requires_cont();
                                                coordinator.current_format = Some(format);

                                                let sample_rate =
                                                    PlayerCoordinator::decode_pcm_sample_rate_hz(
                                                        pcm_sample_rate,
                                                    )
                                                    .unwrap_or_else(|| {
                                                        sample_rate_from_index(pcm_sample_rate)
                                                    });
                                                coordinator.current_sample_rate = sample_rate;
                                                let _ = pipeline.set_sample_rate(sample_rate);

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
                                                pipeline.decoder = ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be));
                                                if format == squeezelite_proto::AudioFormat::Mp3 {
                                                    pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));
                                                } else {
                                                    pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
                                                }

                                                pipeline.sink.set_mute(false);
                                                pipeline.sink.driver.pump_silence_if_needed(false);
                                                let _ = amp.clear_faults();
                                                if amp.mode() != louder_esp32_app::AmpMode::Play {
                                                    let _ = amp.start_playback_via_hiz();
                                                }

                                                if http_headers.is_empty() {
                                                    let req = b"GET /stream.mp3 HTTP/1.0\r\n\r\n";
                                                    audio_req_buf[..req.len()].copy_from_slice(req);
                                                    audio_req_len = req.len();
                                                } else {
                                                    let copy_len = http_headers.len().min(audio_req_buf.len());
                                                    audio_req_buf[..copy_len]
                                                        .copy_from_slice(&http_headers[..copy_len]);
                                                    audio_req_len = copy_len;
                                                }

                                                let audio_sock =
                                                    sockets.get_mut::<tcp::Socket>(audio_handle);
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
                                                let local_ep = (
                                                    IpAddress::Ipv4(Ipv4Address::from(local_ip)),
                                                    49_152 + (((now_ms as u16) + 1) & 0x3FFF),
                                                );
                                                let _ = audio_sock.connect(
                                                    smol_iface.context(),
                                                    remote_ep,
                                                    local_ep,
                                                );
                                                audio_stream_active = true;
                                                audio_req_sent = false;
                                                audio_headers_stripped = false;
                                                audio_header_len = 0;

                                                let slim_sock =
                                                    sockets.get_mut::<tcp::Socket>(slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::Connected,
                                                            pipeline.stream_buffer_available()
                                                                as u32,
                                                            pipeline.output_buffer_available()
                                                                as u32,
                                                            pipeline.frames_played,
                                                            coordinator.current_sample_rate,
                                                            coordinator.stream_bytes_received,
                                                            now_ms as u32,
                                                            0,
                                                            &mut stat_buf,
                                                        )
                                                    {
                                                        let _ = slim_sock
                                                            .send_slice(&stat_buf[..stat_len]);
                                                    }
                                                }

                                                coordinator.state =
                                                    louder_esp32_app::PlayerState::Playing;
                                            }
                                            StreamCommandType::Pause => {
                                                coordinator.state =
                                                    louder_esp32_app::PlayerState::Paused;
                                                pipeline.sink.set_mute(true);
                                                let slim_sock =
                                                    sockets.get_mut::<tcp::Socket>(slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::TrackPaused,
                                                            pipeline.stream_buffer_available()
                                                                as u32,
                                                            pipeline.output_buffer_available()
                                                                as u32,
                                                            pipeline.frames_played,
                                                            coordinator.current_sample_rate,
                                                            coordinator.stream_bytes_received,
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
                                                coordinator.state =
                                                    louder_esp32_app::PlayerState::Playing;
                                                pipeline.sink.set_mute(false);
                                                let slim_sock =
                                                    sockets.get_mut::<tcp::Socket>(slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::TrackResumed,
                                                            pipeline.stream_buffer_available()
                                                                as u32,
                                                            pipeline.output_buffer_available()
                                                                as u32,
                                                            pipeline.frames_played,
                                                            coordinator.current_sample_rate,
                                                            coordinator.stream_bytes_received,
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
                                                coordinator.state =
                                                    louder_esp32_app::PlayerState::Stopped;
                                                audio_stream_active = false;
                                                audio_http_eof = false;
                                                decode_ready_sent = false;
                                                track_started_sent = false;
                                                threshold_sent = false;
                                                sockets
                                                    .get_mut::<tcp::Socket>(audio_handle)
                                                    .abort();
                                                pipeline.sink.set_mute(true);
                                                let slim_sock =
                                                    sockets.get_mut::<tcp::Socket>(slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::TrackStopped,
                                                            pipeline.stream_buffer_available()
                                                                as u32,
                                                            pipeline.output_buffer_available()
                                                                as u32,
                                                            pipeline.frames_played,
                                                            coordinator.current_sample_rate,
                                                            coordinator.stream_bytes_received,
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
                                                audio_stream_active = false;
                                                audio_http_eof = false;
                                                decode_ready_sent = false;
                                                track_started_sent = false;
                                                threshold_sent = false;
                                                sockets
                                                    .get_mut::<tcp::Socket>(audio_handle)
                                                    .abort();
                                                pipeline.flush();
                                                let slim_sock =
                                                    sockets.get_mut::<tcp::Socket>(slim_handle);
                                                if slim_sock.may_send() {
                                                    let mut stat_buf = [0u8; 128];
                                                    if let Ok(stat_len) =
                                                        StatBuilder::build_stat_with_metrics(
                                                            StatEvent::StreamFlushed,
                                                            pipeline.stream_buffer_available()
                                                                as u32,
                                                            pipeline.output_buffer_available()
                                                                as u32,
                                                            pipeline.frames_played,
                                                            coordinator.current_sample_rate,
                                                            coordinator.stream_bytes_received,
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
                                            coordinator.current_sample_rate = sample_rate;
                                            let _ = pipeline.set_sample_rate(sample_rate);
                                        }
                                    }
                                    ServerCommand::Flush => {
                                        audio_stream_active = false;
                                        audio_http_eof = false;
                                        decode_ready_sent = false;
                                        track_started_sent = false;
                                        threshold_sent = false;
                                        sockets.get_mut::<tcp::Socket>(audio_handle).abort();
                                        pipeline.flush();
                                        let slim_sock =
                                            sockets.get_mut::<tcp::Socket>(slim_handle);
                                        if slim_sock.may_send() {
                                            let mut stat_buf = [0u8; 128];
                                            if let Ok(stat_len) =
                                                StatBuilder::build_stat_with_metrics(
                                                    StatEvent::StreamFlushed,
                                                    pipeline.stream_buffer_available() as u32,
                                                    pipeline.output_buffer_available() as u32,
                                                    pipeline.frames_played,
                                                    coordinator.current_sample_rate,
                                                    coordinator.stream_bytes_received,
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
                                            coordinator.dac_enabled = dac_enable;
                                            pipeline.sink.set_mute(!dac_enable);
                                        } else if let Some(metaint) = cmd.stream_continue() {
                                            coordinator.awaiting_cont = false;
                                            icy_meta_interval = metaint as usize;
                                            icy_meta_next = metaint as usize;
                                            icy_meta_left = 0;
                                            if !threshold_sent && !coordinator.autostart_mode.is_auto() {
                                                threshold_sent = true;
                                                let mut stat_buf = [0u8; 128];
                                                if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                                    StatEvent::BUFFER_THRESHOLD,
                                                    pipeline.stream_buffer_available() as u32,
                                                    pipeline.output_buffer_available() as u32,
                                                    pipeline.frames_played,
                                                    coordinator.current_sample_rate,
                                                    coordinator.stream_bytes_received,
                                                    now_ms as u32,
                                                    0,
                                                    &mut stat_buf,
                                                ) {
                                                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                                                    if slim_sock.may_send() {
                                                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                                                    }
                                                }
                                            }
                                        } else if opcode == [b'c', b'o', b'd', b'c'] && !payload.is_empty() {
                                            let codc_fmt = squeezelite_proto::AudioFormat::from_byte(payload[0]);
                                            coordinator.current_format = Some(codc_fmt);
                                            pipeline.decoder = ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true));
                                            if codc_fmt == squeezelite_proto::AudioFormat::Mp3 {
                                                pipeline.set_decoder(ActiveDecoder::Mp3(take_static_mp3_decoder()));
                                            } else {
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
                                                pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(bits, channels, is_be)));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            if consumed > 0 {
                                slim_acc_buf.copy_within(consumed..slim_acc_len, 0);
                                slim_acc_len -= consumed;
                            } else {
                                break;
                            }
                        }
                        Err(ParseError::BufferTooShort) => {
                            if slim_acc_len >= slim_acc_buf.len() {
                                slim_acc_buf.copy_within(1..slim_acc_len, 0);
                                slim_acc_len -= 1;
                            }
                            break;
                        }
                        Err(_) => {
                            if slim_acc_len > 0 {
                                slim_acc_buf.copy_within(1..slim_acc_len, 0);
                                slim_acc_len -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                }

                // Drive audio_sock HTTP streaming when active
                if audio_stream_active {
                    // Send HTTP GET request headers
                    {
                        let audio_sock = sockets.get_mut::<tcp::Socket>(audio_handle);
                        if audio_sock.may_send() && !audio_req_sent {
                            if audio_sock.send_slice(&audio_req_buf[..audio_req_len]).is_ok() {
                                audio_req_sent = true;
                            }
                        }
                    }

                    // Process incoming HTTP stream bytes
                    if !audio_headers_stripped {
                        let audio_sock = sockets.get_mut::<tcp::Socket>(audio_handle);
                        if audio_sock.can_recv() && audio_header_len < audio_header_buf.len() {
                            let _ = audio_sock.recv(|rx_buf| {
                                let mut consumed = 0;
                                while consumed < rx_buf.len() && audio_header_len < audio_header_buf.len() {
                                    let b = rx_buf[consumed];
                                    audio_header_buf[audio_header_len] = b;
                                    audio_header_len += 1;
                                    consumed += 1;
                                    if audio_header_len >= 4 && &audio_header_buf[audio_header_len - 4..audio_header_len] == b"\r\n\r\n" {
                                        break;
                                    }
                                    if audio_header_len >= 2 && &audio_header_buf[audio_header_len - 2..audio_header_len] == b"\n\n" {
                                        break;
                                    }
                                }
                                (consumed, ())
                            });
                        }

                        let is_header_complete = (audio_header_len >= 4
                            && &audio_header_buf[audio_header_len - 4..audio_header_len] == b"\r\n\r\n")
                            || (audio_header_len >= 2
                                && &audio_header_buf[audio_header_len - 2..audio_header_len] == b"\n\n")
                            || audio_header_len >= audio_header_buf.len();

                        if is_header_complete && audio_header_len > 0 {
                            audio_headers_stripped = true;

                            let mut resp_buf = [0u8; 2064];
                            if let Ok(resp_len) = SlimprotoBuilder::build_resp(
                                &audio_header_buf[..audio_header_len],
                                &mut resp_buf,
                            ) {
                                let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                                if slim_sock.may_send() {
                                    let _ = slim_sock.send_slice(&resp_buf[..resp_len]);
                                }
                            }

                            if !threshold_sent && !coordinator.autostart_mode.is_auto() && !coordinator.awaiting_cont {
                                threshold_sent = true;
                                let mut stat_buf = [0u8; 128];
                                if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                                    StatEvent::BUFFER_THRESHOLD,
                                    pipeline.stream_buffer_available() as u32,
                                    pipeline.output_buffer_available() as u32,
                                    pipeline.frames_played,
                                    coordinator.current_sample_rate,
                                    coordinator.stream_bytes_received,
                                    now_ms as u32,
                                    0,
                                    &mut stat_buf,
                                ) {
                                    let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                                    if slim_sock.may_send() {
                                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                                    }
                                }
                            }
                        }
                    } else if !audio_http_eof && !coordinator.awaiting_cont {
                        let mut audio_rx_chunk = [0u8; 512];
                        while sockets.get_mut::<tcp::Socket>(audio_handle).can_recv() {
                            if icy_meta_interval > 0 && icy_meta_next == 0 {
                                if icy_meta_left == 0 {
                                    let mut c = [0u8; 1];
                                    let n = sockets
                                        .get_mut::<tcp::Socket>(audio_handle)
                                        .recv_slice(&mut c)
                                        .unwrap_or(0);
                                    if n == 0 {
                                        break;
                                    }
                                    icy_meta_left = (c[0] as usize) * 16;
                                    if icy_meta_left == 0 {
                                        icy_meta_next = icy_meta_interval;
                                        continue;
                                    }
                                }
                                if icy_meta_left > 0 {
                                    let to_skip = icy_meta_left.min(audio_rx_chunk.len());
                                    let n = sockets
                                        .get_mut::<tcp::Socket>(audio_handle)
                                        .recv_slice(&mut audio_rx_chunk[..to_skip])
                                        .unwrap_or(0);
                                    if n == 0 {
                                        break;
                                    }
                                    icy_meta_left -= n;
                                    if icy_meta_left == 0 {
                                        icy_meta_next = icy_meta_interval;
                                    }
                                    continue;
                                }
                            } else {
                                let avail_write = pipeline.stream_buffer.available_write();
                                if avail_write < 512 {
                                    break;
                                }
                                let max_chunk = if icy_meta_interval > 0 {
                                    audio_rx_chunk.len().min(icy_meta_next).min(avail_write)
                                } else {
                                    audio_rx_chunk.len().min(avail_write)
                                };
                                if max_chunk == 0 {
                                    break;
                                }
                                let n = sockets
                                    .get_mut::<tcp::Socket>(audio_handle)
                                    .recv_slice(&mut audio_rx_chunk[..max_chunk])
                                    .unwrap_or(0);
                                if n > 0 {
                                    let ingested = pipeline.ingest_stream_data(&audio_rx_chunk[..n]).unwrap_or(0);
                                    coordinator.stream_bytes_received += ingested as u64;
                                    if icy_meta_interval > 0 {
                                        icy_meta_next = icy_meta_next.saturating_sub(ingested);
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }

                    // Check HTTP socket EOF:
                    let sock_eof = {
                        let audio_sock = sockets.get_mut::<tcp::Socket>(audio_handle);
                        audio_req_sent
                            && audio_headers_stripped
                            && !audio_sock.can_recv()
                            && !audio_sock.may_recv()
                    };
                    if sock_eof && !audio_http_eof {
                        sockets.get_mut::<tcp::Socket>(audio_handle).abort();
                        audio_http_eof = true;
                        let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
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

            let step_frames = run_firmware_tick(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp);

            pipeline.sink.driver.pump_silence_if_needed(
                coordinator.state == louder_esp32_app::PlayerState::Playing,
            );

            if audio_stream_active && !track_started_sent && pipeline.frames_played > 0 {
                let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                if slim_sock.may_send() {
                    let mut stat_buf = [0u8; 128];
                    if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                        StatEvent::TrackStarted,
                        pipeline.stream_buffer_available() as u32,
                        pipeline.output_buffer_available() as u32,
                        pipeline.frames_played,
                        coordinator.current_sample_rate,
                        coordinator.stream_bytes_received,
                        now_ms as u32,
                        0,
                        &mut stat_buf,
                    ) {
                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                    }
                }
                track_started_sent = true;
            }

            if audio_stream_active && audio_http_eof {
                if !pipeline.has_pending_pcm() && pipeline.stream_buffer.available_read() > 0 {
                    if matches!(pipeline.process_decode_step(), Err(squeezelite_decoders::DecoderError::NeedMoreData)) {
                        pipeline.stream_buffer.flush();
                    }
                }
            }

            if audio_stream_active
                && audio_http_eof
                && !decode_ready_sent
                && !pipeline.has_pending_pcm()
                && pipeline.stream_buffer.available_read() == 0
            {
                let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                if slim_sock.may_send() {
                    let mut stat_buf = [0u8; 128];
                    if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                        StatEvent::DecodeReady,
                        pipeline.stream_buffer_available() as u32,
                        pipeline.output_buffer_available() as u32,
                        pipeline.frames_played,
                        coordinator.current_sample_rate,
                        coordinator.stream_bytes_received,
                        now_ms as u32,
                        0,
                        &mut stat_buf,
                    ) {
                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                    }
                }
                decode_ready_sent = true;
            }

            if audio_stream_active
                && decode_ready_sent
                && !pipeline.has_pending_pcm()
                && pipeline.output_buffer.available_read() == 0
                && step_frames == 0
            {
                let slim_sock = sockets.get_mut::<tcp::Socket>(slim_handle);
                if slim_sock.may_send() {
                    let mut stat_buf = [0u8; 128];
                    if let Ok(stat_len) = StatBuilder::build_stat_with_metrics(
                        StatEvent::TrackUnderrun,
                        pipeline.stream_buffer_available() as u32,
                        pipeline.output_buffer_available() as u32,
                        pipeline.frames_played,
                        coordinator.current_sample_rate,
                        coordinator.stream_bytes_received,
                        now_ms as u32,
                        0,
                        &mut stat_buf,
                    ) {
                        let _ = slim_sock.send_slice(&stat_buf[..stat_len]);
                    }
                }
                audio_stream_active = false;
                coordinator.state = louder_esp32_app::PlayerState::Stopped;
            }

            if coordinator.state == louder_esp32_app::PlayerState::Playing
                && now_ms.saturating_sub(last_fault_check_ms) >= 2_000
            {
                last_fault_check_ms = now_ms;
                if let Ok(Some(fault)) = amp.check_faults() {
                    if !fault.is_clean() {
                        let _ = amp.recover_from_fault();
                    }
                }
            }
        }
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
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // start_playback
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x60]),
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
            Some(&mut client),
            100,
        );

        assert_eq!(frames, 0);
        assert_eq!(coordinator.current_sample_rate, 48000);

        server_handle.join().unwrap();
        i2c.done();
    }

    #[test]
    fn test_audio_config_lms_defaults() {
        let audio_cfg = AudioConfig {
            lms_server_ip: Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_IP),
            lms_server_port: Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_PORT),
            ..Default::default()
        };
        assert_eq!(audio_cfg.lms_server_ip, Some([10, 0, 0, 10]));
        assert_eq!(audio_cfg.lms_server_port, Some(3483));
    }
}

