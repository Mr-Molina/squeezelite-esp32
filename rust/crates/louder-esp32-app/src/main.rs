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
use squeezelite_config::AudioConfig;
#[cfg(not(target_arch = "xtensa"))]
use squeezelite_decoders::PcmDecoder;

mod tasks;

/// SqueezeESP32 HELO capability string broadcasted to LMS server during SlimProto handshake (`WO-155`).
pub const BASEMENT_HELO_CAPABILITIES: &str =
    "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=96000,flc,pcm,wav,aif,mp3";

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
        if let Some(update) = coordinator.take_cyd_message() {
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

/// Case-insensitively searches for `icy-metaint:` header in raw HTTP response headers
/// and parses the integer metadata interval if present.
pub fn parse_icy_metaint_header(headers: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"icy-metaint:";
    let nlen = NEEDLE.len();
    if headers.len() < nlen {
        return None;
    }
    let max_pos = headers.len() - nlen;
    for i in 0..=max_pos {
        let mut matches = true;
        for j in 0..nlen {
            if headers[i + j].to_ascii_lowercase() != NEEDLE[j] {
                matches = false;
                break;
            }
        }
        if matches {
            let mut pos = i + nlen;
            while pos < headers.len() && (headers[pos] == b' ' || headers[pos] == b'\t') {
                pos += 1;
            }
            let mut val: usize = 0;
            let mut found_digit = false;
            while pos < headers.len() && headers[pos].is_ascii_digit() {
                val = val.saturating_mul(10).saturating_add((headers[pos] - b'0') as usize);
                found_digit = true;
                pos += 1;
            }
            if found_digit && val > 0 {
                return Some(val);
            }
        }
    }
    None
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

/// Encodes ESP32 I2S0_CLKM_CONF_REG fractional divider fields (ESP32 TRM Reg 12.6, CODE-MAIN-001):
/// - bits [7:0] = div_num (integral divider)
/// - bits [13:8] = div_b (fractional numerator)
/// - bits [19:14] = div_a (fractional denominator)
pub const fn encode_i2s0_clkm_conf(orig: u32, div_num: u32, div_b: u32, div_a: u32) -> u32 {
    (orig & !0x000F_FFFF)
        | (div_num & 0xFF)
        | ((div_b & 0x3F) << 8)
        | ((div_a & 0x3F) << 14)
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
    use louder_esp32_app::board::{
        BASEMENT_PLAYER_MAC, DEFAULT_WIFI_SSID,
    };
    use louder_esp32_app::hal::I2sDmaDriver;
    use louder_esp32_app::http_control::{DeadManLeaseWatchdog, HttpControlAction};
    use louder_esp32_app::MemConfigStore;
    use squeezelite_audio_core::EqPreset;
    use squeezelite_config::AudioConfig;
    use squeezelite_cyd_link::CydMessage;
    use louder_esp32_app::ota_safety::{
        load_nvs_wifi_credentials_split_12kb, verify_otadata_erased, OtaSafetyError,
        WifiStaCredentials, OTADATA_PARTITION_END, OTADATA_PARTITION_OFFSET,
        OTADATA_SECTOR_1_OFFSET,
    };
    use smoltcp::iface::{Config as SmolIfaceConfig, Interface as SmolInterface, SocketSet, SocketStorage};
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
    static HW_NACK_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

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
            Ok(total_written)
        }

        #[allow(unsafe_code)]
        fn set_clock_rate(&mut self, sample_rate_hz: u32) -> Result<(), OutputError> {
            if !(8_000..=192_000).contains(&sample_rate_hz) {
                return Err(OutputError::UnsupportedSampleRate(sample_rate_hz));
            }
            if self.sample_rate_hz != sample_rate_hz {
                self.sample_rate_hz = sample_rate_hz;
                #[cfg(target_arch = "xtensa")]
                {
                    const I2S0_CLKM_CONF_REG: *mut u32 = 0x3FF4_F0AC as *mut u32;
                    let (div_num, div_b, div_a): (u32, u32, u32) = match sample_rate_hz {
                        44_100 => (14, 11, 63),
                        48_000 => (13, 1, 48),
                        32_000 => (19, 17, 32),
                        24_000 => (26, 1, 24),
                        22_050 => (28, 22, 63),
                        16_000 => (39, 1, 16),
                        12_000 => (52, 1, 12),
                        11_025 => (56, 44, 63),
                        88_200 => (7, 11, 63),
                        96_000 => (6, 25, 48),
                        _ => (14, 11, 63),
                    };
                    unsafe {
                        let cur = core::ptr::read_volatile(I2S0_CLKM_CONF_REG);
                        let new_val = encode_i2s0_clkm_conf(cur, div_num, div_b, div_a);
                        core::ptr::write_volatile(I2S0_CLKM_CONF_REG, new_val);
                    }
                }
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
            option_env!("SL_WIFI_SSID").or(Some(DEFAULT_WIFI_SSID)),
            option_env!("SL_WIFI_PASS"),
        )
    }

    pub(crate) static MP3_DEC_TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    #[allow(unsafe_code)]
    pub(crate) fn take_static_mp3_decoder() -> &'static mut squeezelite_decoders::StaticMp3Decoder {
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
        slim_acc_buf: [u8; 1536],
        audio_req_buf: [u8; 512],
        audio_header_buf: [u8; 1024],
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
                slim_acc_buf: [0u8; 1536],
                audio_req_buf: [0u8; 512],
                audio_header_buf: [0u8; 1024],
            }
        }
    }

    #[allow(unsafe_code)]
    #[link_section = ".dram2_uninit"]
    static mut APP_CORE_STACK: Stack<4096> = Stack::new();
    #[allow(unsafe_code)]
    #[link_section = ".dram2_uninit"]
    static mut SETTINGS_SCRATCH: [u8; 4096] = [0u8; 4096];
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

    #[inline(never)]
    fn load_boot_audio_config(
        flash: &mut FlashStorage<'_>,
        scratch: &mut [u8; 12_288],
    ) -> (AudioConfig, u32) {
        let (sector_a, rest) = scratch.split_at_mut(4096);
        let (sector_b, _) = rest.split_at_mut(4096);
        let _ = flash.read_nor(SETTINGS_JOURNAL_OFFSET, sector_a);
        let _ = flash.read_nor(SETTINGS_JOURNAL_OFFSET + 4096, sector_b);

        let meta_a = squeezelite_config::FlashJournal::read_sector_meta(sector_a);
        let meta_b = squeezelite_config::FlashJournal::read_sector_meta(sector_b);
        let active_seq = match (meta_a, meta_b) {
            (Ok((seq_a, _, _)), Ok((seq_b, _, _))) => {
                if squeezelite_config::FlashJournal::is_seq_newer_or_equal(seq_a, seq_b) {
                    seq_a
                } else {
                    seq_b
                }
            }
            (Ok((seq_a, _, _)), Err(_)) => seq_a,
            (Err(_), Ok((seq_b, _, _))) => seq_b,
            (Err(_), Err(_)) => 0,
        };

        let mut store = MemConfigStore::<8>::new();
        if squeezelite_config::FlashJournal::load_from_dual_sectors(sector_a, sector_b, &mut store)
            .is_ok()
        {
            (AudioConfig::load_from(&store).unwrap_or_default(), active_seq)
        } else {
            (AudioConfig::default(), 0)
        }
    }

    #[inline(never)]
    pub(crate) fn persist_audio_config(
        scratch: &mut [u8; 4096],
        cfg: &AudioConfig,
        current_seq: &mut u32,
    ) -> bool {
        let next_seq = current_seq.wrapping_add(1);
        let mut store = MemConfigStore::<8>::new();
        if cfg.save_to(&mut store).is_err() {
            return false;
        }
        scratch.fill(0xFF);
        if squeezelite_config::FlashJournal::save_to_sector_with_seq(&store, scratch, next_seq).is_ok() {
            *current_seq = next_seq;
            true
        } else {
            false
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
    ) -> (
        &'static mut HwStreamPipeline,
        &'static mut [u8; 2064],
        &'static mut [u8; 1536],
        &'static mut [u8; 1536],
    ) {
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_PIPELINE: core::mem::MaybeUninit<HwStreamPipeline> =
            core::mem::MaybeUninit::uninit();
        #[unsafe(link_section = ".dram2_uninit")]
        static mut STATIC_RESP_BUF: core::mem::MaybeUninit<[u8; 2064]> =
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

            let resp_ptr = (*core::ptr::addr_of_mut!(STATIC_RESP_BUF)).as_mut_ptr();
            resp_ptr.write_bytes(0, 1);

            let http_resp_ptr = (*core::ptr::addr_of_mut!(STATIC_HTTP_RESP_BUF)).as_mut_ptr();
            http_resp_ptr.write_bytes(0, 1);

            let http_tx_ptr = (*core::ptr::addr_of_mut!(STATIC_HTTP_TX_BUF)).as_mut_ptr();
            http_tx_ptr.write_bytes(0, 1);

            (&mut *ptr, &mut *resp_ptr, &mut *http_resp_ptr, &mut *http_tx_ptr)
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
    }

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

    /// Static (not stack) storage for the UDP placeholder sockets, so their RAM cost is visible
    /// in `.bss` from Task 8 on.
    fn take_udp_placeholder_buffers() -> &'static mut [UdpPlaceholderBuffers; UDP_PLACEHOLDER_SOCKETS] {
        static UDP_BUFFERS: static_cell::StaticCell<[UdpPlaceholderBuffers; UDP_PLACEHOLDER_SOCKETS]> =
            static_cell::StaticCell::new();
        UDP_BUFFERS.init_with(|| [const { UdpPlaceholderBuffers::new() }; UDP_PLACEHOLDER_SOCKETS])
    }

    /// Upgrades a borrow of a local of [`run_hardware_firmware`] to `'static`.
    ///
    /// This is the third option the esp-rtos `Executor::run` docs list ("a local variable in a
    /// function you know never returns, upgrading its lifetime"). It keeps the superloop state
    /// where the superloop kept its locals (the core-0 main stack) instead of growing `.bss`.
    ///
    /// # Safety
    /// `local` must live in the frame of a function that never returns (`-> !`, panic = abort so
    /// no unwinding), and the caller must not touch the local again except through the returned
    /// reference.
    #[allow(unsafe_code)]
    unsafe fn upgrade_never_returning_local<T>(local: &mut T) -> &'static mut T {
        // SAFETY: forwarded to the caller (see above).
        unsafe { &mut *(local as *mut T) }
    }

    /// Device entry: bring-up, then the embassy executor that replaces the legacy superloop.
    pub fn run_hardware_firmware() -> ! {
        let mut socket_storage: [SocketStorage<'static>; SOCKET_SLOTS] =
            [SocketStorage::EMPTY; SOCKET_SLOTS];
        // SAFETY: this function never returns (`-> !`, panic = abort) and `socket_storage` is
        // only reached through the upgraded borrow from here on.
        #[allow(unsafe_code)]
        let socket_storage = unsafe { upgrade_never_returning_local(&mut socket_storage) };
        let mut shared = bring_up(socket_storage);
        // SAFETY: as above, for the superloop state the core-0 tasks share.
        #[allow(unsafe_code)]
        let shared: &'static Shared = unsafe { upgrade_never_returning_local(&mut shared) };
        crate::tasks::run_core0_executor(shared)
    }

    /// Hardware bring-up, in the legacy order, ending in the initial superloop state. Its own
    /// frame (bring-up temporaries) is released before the executor starts.
    #[inline(never)]
    fn bring_up(socket_storage: &'static mut [SocketStorage<'static>; SOCKET_SLOTS]) -> Shared {
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
            ref mut slim_acc_buf,
            ref mut audio_req_buf,
            ref mut audio_header_buf,
        } = fw_bufs;

        #[allow(unsafe_code)]
        let settings_scratch: &mut [u8; 4096] =
            unsafe { &mut *core::ptr::addr_of_mut!(SETTINGS_SCRATCH) };

        let wifi_sta_creds = {
            let boot_scratch: &mut [u8; 12_288] = (&mut ring_storage[..12_288]).try_into().unwrap();
            match load_nvs_wifi_credentials(&mut flash, &sta_mac, boot_scratch) {
                Ok(c) => c,
                Err(e) => {
                    println!(
                        "[ANTI-BRICK] Could not resolve Wi-Fi credentials from 0x9000 NVS ({:?}) -> resetting to recovery (0x10000)",
                        e
                    );
                    esp_hal::system::software_reset();
                }
            }
        };
        wdt.feed();
        println!(
            "[NVS 0x9000] Loaded Wi-Fi STA SSID='{}' (pass_len={}) for MAC {:02x?}",
            wifi_sta_creds.ssid.as_str(),
            wifi_sta_creds.password.len(),
            sta_mac
        );

        let (audio_cfg, settings_seq) = {
            let boot_scratch: &mut [u8; 12_288] = (&mut ring_storage[..12_288]).try_into().unwrap();
            load_boot_audio_config(&mut flash, boot_scratch)
        };
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

        let i2c_cfg = I2cConfig::default().with_frequency(Rate::from_khz(400));
        let hal_i2c = HalI2c::new(peripherals.I2C0, i2c_cfg)
            .expect("Failed to initialize I2C0")
            .with_sda(peripherals.GPIO21)
            .with_scl(peripherals.GPIO27);
        let i2c_bus = Esp32I2cBus::new(hal_i2c);

        let i2s_tdm_cfg = TdmConfig::new_tdm_philips()
            .with_sample_rate(Rate::from_hz(44_100))
            .with_data_format(DataFormat::Data32Channel32)
            .with_channels(Channels::STEREO);
        let mut tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024);
        Esp32HardwareI2sDma::prefill_silence(&mut tx_buffer);
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
        let hw_i2s_driver = Esp32HardwareI2sDma::new(i2s_transfer, 44_100);

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
        let mut amp = AmpManager::for_active_board(i2c_bus);
        let _ = amp.initialize(32);
        let _ = amp.set_volume(VOL_UNITY_0DB);
        let _ = fault.is_high();
        wdt.feed();

        let sink = EspI2sSink::for_active_board(hw_i2s_driver, 44_100);
        let (pipeline, pending_resp_buf, http_resp_buf, http_tx_buf) =
            init_static_pipeline(stream_storage, output_storage, sink);
        let coordinator = PlayerCoordinator::new(BASEMENT_PLAYER_MAC, "BASEMENT");
        println!("[HIL 1/5..5/5] Hardware Peripherals (GPIO33/34, I2C0 0x2D, I2S0 26/25/22, UART2) Initialized");

        // Apply saved audio configuration from 0x3F0000 settings partition
        let current_vol_pct = audio_cfg.volume;
        let init_vol_reg = percent_to_attenuation(current_vol_pct);
        let _ = amp.set_volume(init_vol_reg);
        let _ = amp.set_analog_gain(audio_cfg.analog_gain_code);
        pipeline.apply_eq_preset(EqPreset::from_u8(audio_cfg.equalizer_preset));
        pipeline.set_loudness(audio_cfg.loudness_enabled);
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
        let mut wifi_controller = match WifiController::new(peripherals.WIFI, wifi_cfg) {
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
        let hw_rng = Rng::new();
        let rng_seed = (u64::from(hw_rng.random()) << 32) | u64::from(hw_rng.random());
        let hw_addr = HardwareAddress::Ethernet(EthernetAddress(sta_mac));
        let mut smol_cfg = SmolIfaceConfig::new(hw_addr);
        smol_cfg.random_seed = rng_seed;
        let local_port_counter: u16 = 49152 + ((hw_rng.random() as u16) & 0x3FFF);
        let mut smol_dev = SmolDevice {
            driver: &mut sta_iface,
        };
        let smol_iface = SmolInterface::new(
            smol_cfg,
            &mut smol_dev,
            SmolInstant::from_millis(now_millis() as i64),
        );

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

        // Spec §3.6: two UDP placeholder sockets (LMS discovery / mDNS). Allocated now so the RAM
        // budget measured from Task 8 on is the real one; never bound, so smoltcp ignores them
        // until P3 binds them.
        let udp_placeholder_handles = take_udp_placeholder_buffers().each_mut().map(|bufs| {
            sockets.add(udp::Socket::new(
                udp::PacketBuffer::new(&mut bufs.rx_meta[..], &mut bufs.rx_payload[..]),
                udp::PacketBuffer::new(&mut bufs.tx_meta[..], &mut bufs.tx_payload[..]),
            ))
        });

        let boot_ms = now_millis();
        let mut latest_telemetry: Option<squeezelite_tas58xx::Tas5805mTelemetry> = None;
        pipeline.sink.driver.pump_silence_if_needed(false, false);
        let _ = amp.clear_faults();
        let _ = amp.start_playback_via_hiz();
        if let Ok(telem) = amp.read_telemetry() {
            latest_telemetry = telem;
        }

        // ====================================================================
        // GATE 3 & GATE 4: Non-Blocking Runtime Loop with 45s/60s Dead-Man
        // Lease Watchdog, Port 80 HTTP Recovery Server, and SlimProto Client.
        // The loop body now runs as the embassy phase ring in `tasks.rs`; this is
        // its initial state (the legacy pre-loop `let mut` initializers).
        // ====================================================================
        Shared::new(FirmwareState {
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
            settings_scratch,
            pending_resp_buf,
            http_resp_buf,
            slim_acc_buf,
            audio_req_buf,
            audio_header_buf,
            wifi_sta_creds,
            audio_cfg,
            settings_seq,
            settings_dirty: false,
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
            last_stat_ms: 0,
            last_connect_attempt_ms: 0,
            last_slim_connect_ms: 0,
            slim_acc_len: 0,
            audio_stream_active: false,
            audio_req_sent: false,
            audio_req_sent_offset: 0,
            audio_headers_stripped: false,
            audio_req_len: 0,
            audio_header_len: 0,
            audio_header_window: 0,
            pending_resp_len: 0,
            pending_resp_sent: 0,
            last_fault_check_ms: 0,
            latest_telemetry,
            last_cyd_progress_ms: 0,
            audio_http_eof: false,
            decode_ready_sent: false,
            track_started_sent: false,
            threshold_sent: false,
            icy_meta_interval: 0,
            icy_meta_next: 0,
            icy_meta_left: 0,
            now_ms: 0,
            step_frames: 0,
            tick_playing: false,
        })
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
        let audio_cfg = AudioConfig {
            lms_server_ip: Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_IP),
            lms_server_port: Some(louder_esp32_app::board::DEFAULT_LMS_SERVER_PORT),
            ..Default::default()
        };
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
    fn test_parse_icy_metaint_header() {
        let h1 = b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\nicy-metaint: 16000\r\n\r\n";
        assert_eq!(parse_icy_metaint_header(h1), Some(16000));

        let h2 = b"HTTP/1.1 200 OK\r\nICY-METAINT:\t32768\r\nServer: Icecast\r\n\r\n";
        assert_eq!(parse_icy_metaint_header(h2), Some(32768));

        let h3 = b"HTTP/1.0 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n";
        assert_eq!(parse_icy_metaint_header(h3), None);

        let h4 = b"icy-metaint: 0\r\n";
        assert_eq!(parse_icy_metaint_header(h4), None);
    }

    #[test]
    fn test_lms_gain_to_tas5805m_vol_reg() {
        assert_eq!(lms_gain_to_tas5805m_vol_reg(0), 0xFF); // Mute
        assert_eq!(lms_gain_to_tas5805m_vol_reg(65536), 0x30); // 0.0 dB (100%)
        assert_eq!(lms_gain_to_tas5805m_vol_reg(1842), 0x5D); // -22.5 dB (55%)
    }

    #[test]
    fn test_encode_i2s0_clkm_conf() {
        let orig = 0x1230_0000;
        let encoded = encode_i2s0_clkm_conf(orig, 6, 1, 2);
        assert_eq!(encoded & 0xFF, 6);
        assert_eq!((encoded >> 8) & 0x3F, 1);
        assert_eq!((encoded >> 14) & 0x3F, 2);
        assert_eq!(encoded & !0x000F_FFFF, orig);
    }

    #[test]
    fn test_basement_helo_capabilities_flc_and_sample_rate() {
        assert!(
            BASEMENT_HELO_CAPABILITIES.contains(",flc,"),
            "Capabilities must advertise native FLAC (,flc,)"
        );
        assert!(
            BASEMENT_HELO_CAPABILITIES.contains("MaxSampleRate=96000"),
            "Capabilities must advertise MaxSampleRate=96000"
        );
        assert!(
            BASEMENT_HELO_CAPABILITIES.contains(",pcm,"),
            "Capabilities must advertise pcm"
        );
        assert!(
            BASEMENT_HELO_CAPABILITIES.contains(",mp3"),
            "Capabilities must advertise mp3"
        );
    }
}

