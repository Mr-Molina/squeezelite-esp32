//! CYD Display Controller Application Entrypoint
//!
//! Standalone entrypoint for the CYD (Cheap Yellow Display ESP32-2432S028R) controller.
//! Initializes ILI9341 TFT display, XPT2046 touch controller, Now Playing UI,
//! and Squeezelite UART telemetry link.

#![forbid(unsafe_code)]

use cyd_controller_app::comms::{
    CydDisplayLink, CYD_UART_BAUD, CYD_UART_RX_PIN, CYD_UART_TX_PIN,
};
use cyd_controller_app::drivers::{
    Ili9341Display, MockSpiDisplayBus, MockTouchSpiBus, Xpt2046Touch, CYD_TFT_BL, CYD_TFT_CS,
    CYD_TFT_DC, CYD_TFT_MISO, CYD_TFT_MOSI, CYD_TFT_RST, CYD_TFT_SCLK, CYD_TOUCH_CS,
    CYD_TOUCH_IRQ, CYD_TOUCH_MISO, CYD_TOUCH_MOSI, CYD_TOUCH_SCLK,
};
use cyd_controller_app::ui::NowPlayingUi;
use squeezelite_cyd_link::CydCommand;

fn main() {
    println!("========================================================");
    println!("  Cheap Yellow Display Controller (ESP32-2432S028R)     ");
    println!("  Pure-Rust Firmware Application                        ");
    println!("========================================================");
    println!("Hardware Pin Configuration:");
    println!(
        "  - ILI9341 TFT (HSPI): MOSI={}, MISO={}, SCLK={}, CS={}, DC={}, RST={}, BL={}",
        CYD_TFT_MOSI,
        CYD_TFT_MISO,
        CYD_TFT_SCLK,
        CYD_TFT_CS,
        CYD_TFT_DC,
        CYD_TFT_RST,
        CYD_TFT_BL
    );
    println!(
        "  - XPT2046 Touch (VSPI): MOSI={}, MISO={}, SCLK={}, CS={}, IRQ={}",
        CYD_TOUCH_MOSI,
        CYD_TOUCH_MISO,
        CYD_TOUCH_SCLK,
        CYD_TOUCH_CS,
        CYD_TOUCH_IRQ
    );
    println!(
        "  - Telemetry UART:     TX={}, RX={}, Baud={}",
        CYD_UART_TX_PIN, CYD_UART_RX_PIN, CYD_UART_BAUD
    );
    println!("--------------------------------------------------------");

    // 1. Initialize ILI9341 320x240 Landscape Display Driver
    let spi_bus = MockSpiDisplayBus::default();
    let mut display = Ili9341Display::new(spi_bus);
    display.init();

    // 2. Initialize XPT2046 Resistive Touch Controller
    let touch_bus = MockTouchSpiBus::default();
    let _touch = Xpt2046Touch::new(touch_bus);

    // 3. Initialize Now Playing User Interface & Render Initial Frame
    let ui = NowPlayingUi::new();
    ui.render_frame(&mut display);

    // 4. Initialize CYD Display UART Link & Transmit Initial Status Query
    let mut link = CydDisplayLink::new();
    let status_frame = link
        .send_command(CydCommand::QueryStatus)
        .expect("Failed to encode status command");

    println!("CYD Initialization Status:");
    println!("  - Display Initialized: {}", display.is_initialized());
    println!("  - Backlight Duty:      {}%", display.backlight_percent());
    println!("  - Flush Count:         {}", display.flush_count());
    println!(
        "  - Outbound Sync Frame: {:?}",
        core::str::from_utf8(&status_frame).unwrap_or("<invalid utf-8>")
    );
    println!("  - Link Active:         {}", link.state.link_active);
    println!("--------------------------------------------------------");
    println!("CYD Display Controller ready. Listening on UART...");
}
