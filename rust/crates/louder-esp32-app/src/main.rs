//! Louder-ESP32-Plus Application Entrypoint
//!
//! Initializes board hardware profiles, starts audio streaming pipeline,
//! and orchestrates SlimProto network connection with TAS5825M digital amplifier.

use louder_esp32_app::board::{pins, BoardModel};
use louder_esp32_app::pipeline::{ActiveDecoder, StreamPipeline};
use louder_esp32_app::sink::NullAudioSink;
use louder_esp32_app::PlayerCoordinator;
use squeezelite_config::{AudioConfig, MemConfigStore};
use squeezelite_decoders::PcmDecoder;

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

    // Allocate heap-backed audio stream and output buffers (APP-001: avoid FreeRTOS task stack overflow)
    let mut stream_storage = vec![0u8; 64 * 1024]; // 64 KB heap stream buffer
    let mut output_storage = vec![0u8; 64 * 1024]; // 64 KB heap output buffer

    // Initialize audio sink (Null sink for host / fallback, hardware I2S on target)
    let sink = NullAudioSink::new(44100);
    let pcm_dec = PcmDecoder::new(16, 2, true);

    let pipeline = StreamPipeline::new(
        &mut stream_storage,
        &mut output_storage,
        ActiveDecoder::Pcm(pcm_dec),
        sink,
        44100,
    );

    // Deterministic node initialization and configuration storage (INFRA-801)
    let mut config_store = MemConfigStore::<16>::new();
    let mut audio_cfg = AudioConfig::default();
    audio_cfg.player_name.clear();
    let _ = audio_cfg.player_name.push_str(board.name());
    let _ = audio_cfg.save_to(&mut config_store);

    // Derive deterministic MAC address from Sonocotta OUI prefix (24:0A:C4) and board hardware model (INFRA-801)
    let suffix = match board {
        BoardModel::LouderEsp32Wrover => 0x01,
        BoardModel::LouderEsp32PlusS3 => 0x02,
    };
    let mac = [0x24, 0x0A, 0xC4, 0x12, 0x34, suffix];
    let coordinator = PlayerCoordinator::new(mac, board.name());

    println!("Player Coordinator initialized:");
    println!("  - State:       {:?}", coordinator.state);
    println!("  - Node Name:   {}", audio_cfg.player_name);
    println!("  - MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    println!("  - Sample Rate: {} Hz", pipeline.sample_rate);
    println!("  - Buffers:     {} bytes stream, {} bytes output", pipeline.stream_buffer_available(), pipeline.output_buffer_available());
    println!("--------------------------------------------------------");

    // Initialize Wi-Fi Manager and check persistent configuration
    let wifi = louder_esp32_app::wifi::WifiManager::with_store(config_store);
    println!("Wi-Fi Subsystem (Key-Value NVS):");
    println!("  - State:       {}", wifi.state());
    if !wifi.config.ssid.is_empty() {
        println!("  - Stored SSID: {}", wifi.config.ssid);
    } else {
        println!("  - Stored SSID: [None - Ready for CYD / Serial Provisioning]");
    }
    println!("--------------------------------------------------------");
    println!("Ready for LMS auto-discovery on UDP 3483...");
}
