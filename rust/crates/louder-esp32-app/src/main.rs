//! Louder-ESP32-Plus Application Entrypoint

use louder_esp32_app::board::{pins, BoardModel};
use louder_esp32_app::PlayerCoordinator;

fn main() {
    let board = BoardModel::active();
    println!("Initializing {}", board.name());
    println!("I2S Pins: BCLK={}, LRCLK={}, DOUT={}", pins::I2S_BCLK, pins::I2S_LRCLK, pins::I2S_DOUT);
    println!("I2C Pins: SDA={}, SCL={}", pins::I2C_SDA, pins::I2C_SCL);
    println!("TAS58xx Powerdown Pin: GPIO {}", pins::TAS58XX_PWDN);

    let mut audio_buffer = [0u8; 1024];
    let coordinator = PlayerCoordinator::new(&mut audio_buffer);
    println!("Player coordinator initialized in state: {:?}", coordinator.state);
}
