//! `cyd-controller-app`
//!
//! Pure-Rust, zero-allocation display and touch firmware for the
//! CYD (Cheap Yellow Display ESP32-2432S028R).
//!
//! Provides ILI9341 display driver, XPT2046 touch driver, UART telemetry link,
//! and Now Playing user interface.

#![no_std]
#![forbid(unsafe_code)]

pub mod comms;
pub mod drivers;
pub mod ui;

pub use comms::{
    CydDisplayLink, CydTelemetryState, CYD_HEARTBEAT_TIMEOUT_US, CYD_RESYNC_INTERVAL_US,
    CYD_UART_BAUD, CYD_UART_RX_PIN, CYD_UART_TX_PIN,
};
pub use drivers::{
    Ili9341Display, MockSpiDisplayBus, MockTouchSpiBus, SpiDisplayBus, TouchSpiBus, Xpt2046Touch,
    CYD_TFT_BL, CYD_TFT_CS, CYD_TFT_DC, CYD_TFT_HEIGHT, CYD_TFT_MISO, CYD_TFT_MOSI, CYD_TFT_RST,
    CYD_TFT_SCLK, CYD_TFT_WIDTH, CYD_TOUCH_CS, CYD_TOUCH_IRQ, CYD_TOUCH_MISO, CYD_TOUCH_MOSI,
    CYD_TOUCH_SCLK, XPT2046_RAW_X_MAX, XPT2046_RAW_X_MIN, XPT2046_RAW_Y_MAX, XPT2046_RAW_Y_MIN,
    XPT2046_SAMPLE_COUNT,
};
pub use ui::{
    rgb888_to_rgb565, NowPlayingUi, UiButtonId, UI_COLOR_HEX_ACCENT, UI_COLOR_HEX_BG,
    UI_COLOR_HEX_OFFLINE, UI_COLOR_HEX_ONLINE, UI_COLOR_HEX_SURFACE, UI_COLOR_HEX_SURFACE_LIGHT,
    UI_COLOR_HEX_TEXT_PRIMARY, UI_COLOR_HEX_TEXT_SECONDARY,
};

pub use squeezelite_cyd_link::{CydCommand, CydLinkProtocol, CydMessage, LinkError, PlayState};
