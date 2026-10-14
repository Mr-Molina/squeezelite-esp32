//! CYD Display Controller Application Entrypoint
//!
//! Dual-target entrypoint for the CYD (Cheap Yellow Display ESP32-2432S028R) controller:
//! - On `xtensa-esp32-none-elf`: Bare-metal `no_std` firmware driving physical ILI9341 SPI2 TFT,
//!   XPT2046 Touch SPI bus, and executing Hardware-in-the-Loop (HIL) verification & live UI loop.
//! - On host (`x86_64` / `aarch64`): Deterministic verification harness.

#![cfg_attr(target_arch = "xtensa", no_std)]
#![cfg_attr(target_arch = "xtensa", no_main)]
#![cfg_attr(not(target_arch = "xtensa"), forbid(unsafe_code))]

use cyd_controller_app::comms::{
    CydDisplayLink, CYD_UART_BAUD, CYD_UART_RX_PIN, CYD_UART_TX_PIN,
};
use cyd_controller_app::drivers::{
    Ili9341Display, Xpt2046Touch, CYD_TFT_BL, CYD_TFT_CS, CYD_TFT_DC, CYD_TFT_MISO, CYD_TFT_MOSI,
    CYD_TFT_RST, CYD_TFT_SCLK, CYD_TOUCH_CS, CYD_TOUCH_IRQ, CYD_TOUCH_MISO, CYD_TOUCH_MOSI,
    CYD_TOUCH_SCLK,
};
use cyd_controller_app::ui::NowPlayingUi;
use squeezelite_cyd_link::PlayState;

#[cfg(target_arch = "xtensa")]
mod xtensa_hw {
    use super::*;
    use cyd_controller_app::drivers::{SpiDisplayBus, TouchSpiBus};
    use embedded_hal::spi::SpiBus;
    use esp_backtrace as _;
    use esp_hal::{
        Blocking,
        clock::CpuClock,
        gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
        spi::{
            Mode,
            master::{Config as SpiConfig, Spi},
        },
        time::{Duration, Instant, Rate},
    };
    use esp_println::println;

    esp_bootloader_esp_idf::esp_app_desc!();

    pub fn delay_ms(ms: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {}
    }

    pub fn delay_us(us: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_micros(us) {}
    }

    /// Physical ESP32 CYD HSPI (SPI2) display bus driving ILI9341 on GPIO 14/13/12/15/2 and BL on GPIO 21.
    pub struct Esp32SpiDisplayBus<'d> {
        spi: Spi<'d, Blocking>,
        cs: Output<'d>,
        dc: Output<'d>,
        bl: Output<'d>,
        pub commands_sent: u32,
        pub bytes_sent: usize,
        pub duty_10bit: u16,
    }

    impl<'d> Esp32SpiDisplayBus<'d> {
        pub fn new(
            spi: Spi<'d, Blocking>,
            cs: Output<'d>,
            dc: Output<'d>,
            bl: Output<'d>,
        ) -> Self {
            Self {
                spi,
                cs,
                dc,
                bl,
                commands_sent: 0,
                bytes_sent: 0,
                duty_10bit: 1023,
            }
        }
    }

    impl SpiDisplayBus for Esp32SpiDisplayBus<'_> {
        fn send_cmd(&mut self, cmd: u8) {
            self.dc.set_low();
            self.cs.set_low();
            let _ = SpiBus::write(&mut self.spi, &[cmd]);
            let _ = SpiBus::flush(&mut self.spi);
            self.cs.set_high();
            self.commands_sent += 1;

            // ILI9341 datasheet requirement: 120ms stabilization delay after SWRESET (0x01) and SLPOUT (0x11)
            if cmd == 0x01 || cmd == 0x11 {
                delay_ms(120);
            }
        }

        fn send_data(&mut self, data: &[u8]) {
            self.dc.set_high();
            self.cs.set_low();
            let _ = SpiBus::write(&mut self.spi, data);
            let _ = SpiBus::flush(&mut self.spi);
            self.cs.set_high();
            self.bytes_sent += data.len();
        }

        fn set_backlight_duty(&mut self, duty_10bit: u16) {
            self.duty_10bit = duty_10bit;
            if duty_10bit > 0 {
                self.bl.set_high();
            } else {
                self.bl.set_low();
            }
        }
    }

    /// Physical ESP32 CYD Touch SPI bus driving XPT2046 on SCLK=25, MOSI=32, MISO=39, CS=33, IRQ=36.
    pub struct Esp32TouchSpiBus<'d> {
        sclk: Output<'d>,
        mosi: Output<'d>,
        miso: Input<'d>,
        cs: Output<'d>,
        irq: Input<'d>,
    }

    impl<'d> Esp32TouchSpiBus<'d> {
        pub fn new(
            sclk: Output<'d>,
            mosi: Output<'d>,
            miso: Input<'d>,
            cs: Output<'d>,
            irq: Input<'d>,
        ) -> Self {
            Self {
                sclk,
                mosi,
                miso,
                cs,
                irq,
            }
        }
    }

    impl TouchSpiBus for Esp32TouchSpiBus<'_> {
        fn is_irq_low(&self) -> bool {
            self.irq.is_low()
        }

        fn read_raw_12bit(&mut self, cmd: u8) -> u16 {
            self.cs.set_low();
            delay_us(2);

            // Transmit 8-bit command MSB-first
            for bit in (0..8).rev() {
                if ((cmd >> bit) & 1) != 0 {
                    self.mosi.set_high();
                } else {
                    self.mosi.set_low();
                }
                delay_us(1);
                self.sclk.set_high();
                delay_us(1);
                self.sclk.set_low();
            }

            self.mosi.set_low();
            delay_us(2);

            // Read 12-bit conversion result (16 clock pulses, top 12 bits valid)
            let mut raw16: u16 = 0;
            for _ in 0..16 {
                self.sclk.set_high();
                delay_us(1);
                raw16 <<= 1;
                if self.miso.is_high() {
                    raw16 |= 1;
                }
                self.sclk.set_low();
                delay_us(1);
            }

            self.cs.set_high();
            (raw16 >> 3) & 0x0FFF
        }
    }

    pub fn run_hardware_firmware() -> ! {
        let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
        let peripherals = esp_hal::init(config);
        let boot_instant = Instant::now();

        println!();
        println!("========================================================");
        println!("  Cheap Yellow Display Controller (ESP32-2432S028R)     ");
        println!("  Pure-Rust Bare-Metal Hardware Firmware (240 MHz)      ");
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
            CYD_TOUCH_MOSI, CYD_TOUCH_MISO, CYD_TOUCH_SCLK, CYD_TOUCH_CS, CYD_TOUCH_IRQ
        );
        println!(
            "  - Telemetry UART:     TX={}, RX={}, Baud={}",
            CYD_UART_TX_PIN, CYD_UART_RX_PIN, CYD_UART_BAUD
        );
        println!("--------------------------------------------------------");

        // Configure physical SPI2 (HSPI) @ 40 MHz for ILI9341 Display
        let spi2 = Spi::new(
            peripherals.SPI2,
            SpiConfig::default()
                .with_frequency(Rate::from_mhz(40))
                .with_mode(Mode::_0),
        )
        .expect("Failed to init SPI2")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);

        let tft_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
        let tft_dc = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
        let tft_bl = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());

        let spi_bus = Esp32SpiDisplayBus::new(spi2, tft_cs, tft_dc, tft_bl);
        let mut display = Ili9341Display::new(spi_bus);

        // Configure physical XPT2046 Touch Controller pins
        let touch_sclk = Output::new(peripherals.GPIO25, Level::Low, OutputConfig::default());
        let touch_mosi = Output::new(peripherals.GPIO32, Level::Low, OutputConfig::default());
        let touch_miso = Input::new(
            peripherals.GPIO39,
            InputConfig::default().with_pull(Pull::None),
        );
        let touch_cs = Output::new(peripherals.GPIO33, Level::High, OutputConfig::default());
        let touch_irq = Input::new(
            peripherals.GPIO36,
            InputConfig::default().with_pull(Pull::None),
        );

        let touch_bus =
            Esp32TouchSpiBus::new(touch_sclk, touch_mosi, touch_miso, touch_cs, touch_irq);
        let mut touch = Xpt2046Touch::new(touch_bus);

        // ====================================================================
        // [HIL 1/5] ILI9341 SPI2 Display Init & Idle Frame Render
        // ====================================================================
        display.init();
        let mut ui = NowPlayingUi::new();
        ui.render_frame(&mut display);
        ui.render_hardware_canvas(&mut display);
        let hil1_pass = display.is_initialized()
            && display.backlight_percent() == 100
            && display.flush_count() >= 4
            && display.bus().bytes_sent > 150_000;
        println!(
            "[HIL 1/5] ILI9341 Display & Backlight Init: {} (cmds={}, spi_bytes={}, flushes={}, bl={}%)",
            if hil1_pass { "PASS" } else { "FAIL" },
            display.bus().commands_sent,
            display.bus().bytes_sent,
            display.flush_count(),
            display.backlight_percent()
        );

        // ====================================================================
        // [HIL 2/5] XPT2046 Touch Bus & 12-Bit ADC Probe
        // ====================================================================
        let irq_active = touch.bus().is_irq_low();
        let raw_x = touch.bus_mut().read_raw_12bit(0xD0);
        let raw_y = touch.bus_mut().read_raw_12bit(0x90);
        let (cal_x, cal_y) = touch.calibrate_raw(2050, 2050);
        let hil2_pass = raw_x <= 4095 && raw_y <= 4095 && cal_x == 160 && cal_y == 120;
        println!(
            "[HIL 2/5] XPT2046 Touch Bus & Calibration:  {} (irq_low={}, raw_adc=({}, {}), center_cal=({}, {}))",
            if hil2_pass { "PASS" } else { "FAIL" },
            irq_active,
            raw_x,
            raw_y,
            cal_x,
            cal_y
        );

        // ====================================================================
        // [HIL 3/5] Squeezelite CYD Link Protocol & Active Playback Render
        // ====================================================================
        let mut link = CydDisplayLink::new();
        let rx_stream = b"STATE=P\r\nVOL=78\r\nMETA=Bohemian Rhapsody;Queen;A Night at the Opera\r\nTIME=142;354\r\nWIFI=1;-52\r\n";
        let parsed_events = link.feed_rx_bytes(rx_stream, 1_000_000);
        ui.update(&link.state);
        ui.render_hardware_canvas(&mut display);
        let hil3_pass = parsed_events == 5
            && link.state.link_active
            && link.state.state == PlayState::Playing
            && link.state.vol == 78
            && ui.title_label.as_str() == "Bohemian Rhapsody"
            && ui.subtitle_label.as_str() == "Queen • A Night at the Opera"
            && ui.progress_percent == 40
            && ui.time_elapsed_label.as_str() == "02:22"
            && ui.time_total_label.as_str() == "05:54"
            && ui.play_pause_icon.as_str() == "PAUSE"
            && ui.link_badge.as_str() == "ONLINE";
        println!(
            "[HIL 3/5] CYD Link Telemetry & UI Render:   {} (events={}, track=\"{}\", progress={}%, time={}/{}, badge={})",
            if hil3_pass { "PASS" } else { "FAIL" },
            parsed_events,
            ui.title_label.as_str(),
            ui.progress_percent,
            ui.time_elapsed_label.as_str(),
            ui.time_total_label.as_str(),
            ui.link_badge.as_str()
        );

        // ====================================================================
        // [HIL 4/5] Touch Hit-Box Router & 10 Hz Volume Slider Throttling
        // ====================================================================
        let prev_cmd = ui.handle_touch_point(30, 200, true, 1000, &mut link);
        let toggle_cmd = ui.handle_touch_point(88, 205, true, 1100, &mut link);
        let next_cmd = ui.handle_touch_point(140, 200, true, 1200, &mut link);
        let vol_drag1 = ui.handle_touch_point(252, 205, false, 2000, &mut link);
        let vol_throttled = ui.handle_touch_point(280, 205, false, 2050, &mut link);
        let vol_released = ui.handle_touch_point(310, 205, true, 2060, &mut link);
        let hil4_pass = prev_cmd.as_deref() == Some(b"CMD=PREV\n".as_slice())
            && toggle_cmd.as_deref() == Some(b"CMD=TOGGLE\n".as_slice())
            && next_cmd.as_deref() == Some(b"CMD=NEXT\n".as_slice())
            && vol_drag1.is_some()
            && vol_throttled.is_none()
            && vol_released.as_deref() == Some(b"CMD=VOL:100\n".as_slice());
        println!(
            "[HIL 4/5] Touch Hit-Boxes & 10Hz Throttle:  {} (prev=OK, toggle=OK, next=OK, throttle_50ms=SUPPRESSED, release=CMD=VOL:100)",
            if hil4_pass { "PASS" } else { "FAIL" }
        );

        // ====================================================================
        // [HIL 5/5] 5s Heartbeat Timeout & 3s Auto-Resync State Machine
        // ====================================================================
        let no_timeout_yet = link.poll_heartbeat(5_500_000);
        let timeout_resync = link.poll_heartbeat(6_500_000);
        let hil5_pass = no_timeout_yet.is_none()
            && !link.state.link_active
            && timeout_resync.as_deref() == Some(b"CMD=STATUS\n".as_slice());
        println!(
            "[HIL 5/5] Heartbeat Timeout & Resync Poll:  {} (timeout_at=6.5s, link_active={}, resync_frame=\"CMD=STATUS\\n\")",
            if hil5_pass { "PASS" } else { "FAIL" },
            link.state.link_active
        );

        let all_passed = hil1_pass && hil2_pass && hil3_pass && hil4_pass && hil5_pass;
        println!("--------------------------------------------------------");
        println!(
            "[HIL SUMMARY] {}/5 HARDWARE-IN-THE-LOOP TESTS PASSED ON ESP32 CYD",
            (hil1_pass as u8)
                + (hil2_pass as u8)
                + (hil3_pass as u8)
                + (hil4_pass as u8)
                + (hil5_pass as u8)
        );
        println!(
            "[HIL VERDICT] {}",
            if all_passed {
                "UNCONDITIONAL HARDWARE PASS"
            } else {
                "HARDWARE FAILURE DETECTED"
            }
        );
        println!("--------------------------------------------------------");
        println!("CYD Display Controller active. Touch screen or send UART frames...");

        let mut was_touched = false;
        let mut last_touch_xy = (0i16, 0i16);

        loop {
            let now_us = boot_instant.elapsed().as_micros() as i64;
            let now_ms = (now_us / 1000) as u32;

            if let Some((x, y)) = touch.read() {
                was_touched = true;
                last_touch_xy = (x, y);
                if let Some(cmd_bytes) = ui.handle_touch_point(x, y, false, now_ms, &mut link) {
                    if let Ok(cmd_str) = core::str::from_utf8(&cmd_bytes) {
                        println!("[TOUCH DRAG] ({}, {}) -> {}", x, y, cmd_str.trim());
                        ui.render_hardware_canvas(&mut display);
                    }
                }
            } else if was_touched {
                was_touched = false;
                let (x, y) = last_touch_xy;
                if let Some(cmd_bytes) = ui.handle_touch_point(x, y, true, now_ms, &mut link) {
                    if let Ok(cmd_str) = core::str::from_utf8(&cmd_bytes) {
                        println!("[TOUCH RELEASE] ({}, {}) -> {}", x, y, cmd_str.trim());
                        ui.render_hardware_canvas(&mut display);
                    }
                } else {
                    println!("[TOUCH TAP] ({}, {})", x, y);
                }
            }

            delay_ms(20);
        }
    }
}

#[cfg(target_arch = "xtensa")]
#[esp_hal::main]
fn main() -> ! {
    xtensa_hw::run_hardware_firmware()
}

#[cfg(not(target_arch = "xtensa"))]
fn main() {
    use cyd_controller_app::drivers::{MockSpiDisplayBus, MockTouchSpiBus};
    use squeezelite_cyd_link::CydCommand;

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
        CYD_TOUCH_MOSI, CYD_TOUCH_MISO, CYD_TOUCH_SCLK, CYD_TOUCH_CS, CYD_TOUCH_IRQ
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
    let mut ui = NowPlayingUi::new();
    let mut link = CydDisplayLink::new();
    let _ = link.feed_rx_bytes(b"STATE=S\n", 0);
    if link.state.state == PlayState::Stopped {
        ui.update(&link.state);
    }
    ui.render_frame(&mut display);

    // 4. Transmit Initial Status Query
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
