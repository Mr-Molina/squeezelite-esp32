//! Louder-ESP32-Plus Application Entrypoint
//!
//! Initializes board hardware profiles, starts audio streaming pipeline,
//! and orchestrates SlimProto network connection with TAS5805M/TAS5825M digital amplifier.
#![forbid(unsafe_code)]
#![allow(unknown_lints)]

use embedded_hal::i2c::I2c;
use louder_esp32_app::board::{pins, BoardModel};
use louder_esp32_app::hal::{BitDepth, EspI2sSink, LoopbackI2sDma, UartPort};
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::{
    AmpManager, AudioOutputSink, CydMessage, CydUartBridge, LoopbackUartPort, MemConfigStore,
    MemFlashPartition, NvsFlashBackend, PlayState, PlayerCoordinator, TargetI2cBus,
};
use squeezelite_config::AudioConfig;
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
    if let Ok(Some(cmd)) = cyd_bridge.poll_command() {
        if let Some(reply) = coordinator.handle_cyd_command(cmd, pipeline) {
            let _ = cyd_bridge.flush_outbound(&reply);
        }
    }

    let frames = coordinator.process_audio_step(pipeline);
    let _ = amp.check_faults();
    frames
}

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
    let raw_name = board.name();
    let safe_len = raw_name.floor_char_boundary(32.min(raw_name.len()));
    let _ = audio_cfg.player_name.push_str(&raw_name[..safe_len]);
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
    amp.start_playback().expect("Failed to start playback mode on TAS5805M");
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

    // 4. Derive deterministic MAC address from Sonocotta OUI prefix (24:0A:C4) and board hardware model (INFRA-801)
    let suffix = match board {
        BoardModel::LouderEsp32Wrover => 0x01,
        BoardModel::LouderEsp32PlusS3 => 0x02,
    };
    let mac = [0x24, 0x0A, 0xC4, 0x12, 0x34, suffix];
    let mut coordinator = PlayerCoordinator::new(mac, board.name());

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

    // 7. Firmware Tick Execution Loop (HALNET-409)
    #[cfg(not(target_arch = "xtensa"))]
    {
        for _ in 0..16 {
            let _ = run_firmware_tick(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp);
        }
    }

    #[cfg(target_arch = "xtensa")]
    {
        loop {
            let _ = run_firmware_tick(&mut coordinator, &mut pipeline, &mut cyd_bridge, &mut amp);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
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
}

