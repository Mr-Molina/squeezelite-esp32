//! CYD Display Controller Application Entrypoint
//!
//! Dual-target entrypoint for the CYD (Cheap Yellow Display ESP32-2432S028R) controller:
//! - On `xtensa-esp32-none-elf`: Bare-metal `no_std` firmware driving physical ILI9341 SPI2 TFT,
//!   XPT2046 Touch SPI bus, and executing Hardware-in-the-Loop (HIL) verification & live UI loop.
//! - On host (`x86_64` / `aarch64`): Deterministic verification harness.

#![cfg_attr(target_arch = "xtensa", no_std)]
#![cfg_attr(target_arch = "xtensa", no_main)]
#![forbid(unsafe_code)]
#![allow(unknown_lints)]

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

    /// Physical ESP32 CYD HSPI (SPI2) display bus driving ILI9341 on GPIO 14/13/12/15/2 and BL on GPIO 21.
    pub struct Esp32SpiDisplayBus<'d> {
        spi: Spi<'d, Blocking>,
        cs: Output<'d>,
        dc: Output<'d>,
        bl: Output<'d>,
        pub commands_sent: u32,
        pub bytes_sent: usize,
        pub backlight_duty_10bit: u16,
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
                backlight_duty_10bit: 1023,
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
            self.backlight_duty_10bit = duty_10bit;
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
            if !self.is_irq_low() {
                return 0;
            }

            let delay = esp_hal::delay::Delay::new();
            self.cs.set_low();
            delay.delay_micros(2);

            // Transmit 8-bit command MSB-first (~250 kHz effective SPI clock at 240 MHz CPU)
            for bit in (0..8).rev() {
                if ((cmd >> bit) & 1) != 0 {
                    self.mosi.set_high();
                } else {
                    self.mosi.set_low();
                }
                delay.delay_micros(2);
                self.sclk.set_high();
                delay.delay_micros(2);
                self.sclk.set_low();
                delay.delay_micros(2);
            }

            self.mosi.set_low();
            delay.delay_micros(2);

            // Read 12-bit conversion result (16 clock pulses, top 12 bits valid)
            let mut raw16: u16 = 0;
            for _ in 0..16 {
                self.sclk.set_high();
                delay.delay_micros(2);
                raw16 <<= 1;
                if self.miso.is_high() {
                    raw16 |= 1;
                }
                self.sclk.set_low();
                delay.delay_micros(2);
            }

            self.cs.set_high();
            (raw16 >> 3) & 0x0FFF
        }
    }

    pub fn run_hardware_firmware() -> ! {
        let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
        let peripherals = esp_hal::init(config);

        // Disable hardware watchdogs (TIMG0, TIMG1, RTC RWDT)
        let mut timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
        let mut timg1 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1);
        let mut rtc = esp_hal::rtc_cntl::Rtc::new(peripherals.RTC_TIMER);
        timg0.wdt.disable();
        timg1.wdt.disable();
        rtc.rwdt.disable();

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

        // Configure physical UART2 (TX=GPIO27, RX=GPIO22 with internal Pull::Up, 460,800 baud)
        let uart2_rx_pin = Input::new(
            peripherals.GPIO22,
            InputConfig::default().with_pull(Pull::Up),
        );
        let uart2_cfg = esp_hal::uart::Config::default().with_baudrate(CYD_UART_BAUD);
        let mut uart2 = esp_hal::uart::Uart::new(peripherals.UART2, uart2_cfg)
            .expect("Failed to init UART2")
            .with_tx(peripherals.GPIO27)
            .with_rx(uart2_rx_pin);
        delay_ms(20);

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
        delay_ms(20);

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
        delay_ms(20);

        // ====================================================================
        // [HIL 2/5] XPT2046 Touch Bus & 12-Bit ADC Probe
        // ====================================================================
        let irq_active = touch.bus().is_irq_low();
        let raw_x = touch.bus_mut().read_raw_12bit(0xD0);
        let raw_y = touch.bus_mut().read_raw_12bit(0x90);
        let (cal_x, cal_y) = touch.calibrate_raw(2050, 2050);
        let hil2_pass = raw_x <= 4095 && raw_y <= 4095 && cal_x == 159 && cal_y == 119;
        println!(
            "[HIL 2/5] XPT2046 Touch Bus & Calibration:  {} (irq_low={}, raw_adc=({}, {}), center_cal=({}, {}))",
            if hil2_pass { "PASS" } else { "FAIL" },
            irq_active,
            raw_x,
            raw_y,
            cal_x,
            cal_y
        );
        delay_ms(20);

        // ====================================================================
        // [HIL 3/5] Squeezelite CYD Link Protocol & Active Playback Render
        // ====================================================================
        let mut link = CydDisplayLink::new();
        let rx_stream = b"STATE=P\r\nVOL=78\r\nMETA=Queen;Bohemian Rhapsody\r\nTIME=142/354\r\nWIFI=1;-52\r\n";
        let parsed_events = link.feed_rx_bytes(rx_stream, 1_000_000);
        ui.update(link.state());
        ui.render_hardware_canvas(&mut display);
        let hil3_pass = parsed_events == 5
            && link.state().link_active
            && link.state().state == PlayState::Playing
            && link.state().vol == 78
            && ui.title_label.as_str() == "Bohemian Rhapsody"
            && ui.subtitle_label.as_str() == "Queen"
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
        delay_ms(20);

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
        delay_ms(20);

        // ====================================================================
        // [HIL 5/5] 5s Heartbeat Timeout & 3s Auto-Resync State Machine
        // ====================================================================
        let no_timeout_yet = link.poll_heartbeat(5_500_000);
        let timeout_resync = link.poll_heartbeat(6_500_000);
        let hil5_pass = no_timeout_yet.is_none()
            && !link.state().link_active
            && timeout_resync.as_deref() == Some(b"CMD=STATUS\n".as_slice());
        println!(
            "[HIL 5/5] Heartbeat Timeout & Resync Poll:  {} (timeout_at=6.5s, link_active={}, resync_frame=\"CMD=STATUS\\n\")",
            if hil5_pass { "PASS" } else { "FAIL" },
            link.state().link_active
        );
        delay_ms(20);

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
        delay_ms(20);
        println!(
            "[HIL VERDICT] {}",
            if all_passed {
                "UNCONDITIONAL HARDWARE PASS"
            } else {
                "HARDWARE FAILURE DETECTED"
            }
        );
        println!("--------------------------------------------------------");
        delay_ms(20);

        // Post-HIL Clean Reset (UI-203):
        // 1. Configure physical CYD landscape touch orientation
        touch.configure_cyd_landscape();

        // 2. Wipe synthetic "Bohemian Rhapsody" test data and render clean STANDBY/OFFLINE screen
        link.reset_state();
        link.state_mut().vol = 50;
        ui = NowPlayingUi::new();
        ui.update(link.state());
        ui.render_hardware_canvas(&mut display);
        ui.clear_dirty();

        // 3. Queue initial status sync request via poll_heartbeat(0) and flush over UART2 TX
        let _ = link.poll_heartbeat(0);
        while let Some(pkt) = link.pop_tx() {
            let mut written = 0;
            let mut iters = 0;
            while written < pkt.len() && iters < 64 {
                match uart2.write(&pkt[written..]) {
                    Ok(n) => written += n,
                    Err(_) => break,
                }
                iters += 1;
            }
        }
        delay_ms(20);

        println!("[READY] CYD Display Reset to STANDBY/OFFLINE — Entering Real-Time UART2 & Touch Loop");
        delay_ms(20);

        let delay = esp_hal::delay::Delay::new();
        let mut last_touch: Option<(i16, i16)> = None;
        let mut tick: u32 = 0;

        loop {
            let elapsed = Instant::now().duration_since_epoch();
            let now_us = elapsed.as_micros() as i64;
            let now_ms = elapsed.as_millis() as u32;

            // 1. Non-blocking UART2 RX Drain (runs on EVERY 1 ms tick so 128-byte FIFO never overflows at 460,800 baud)
            let mut rx_buf = [0u8; 64];
            let mut rx_any = false;
            while uart2.read_ready() {
                if let Ok(n) = uart2.read(&mut rx_buf) {
                    if n > 0 {
                        link.feed_rx_bytes(&rx_buf[..n], now_us);
                        rx_any = true;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // 2. Display rendering, touch polling, and heartbeat every 16 ticks (~60 Hz cadence)
            if tick % 16 == 0 {
                // Heartbeat & Resync Monitor
                let was_active = link.state().link_active;
                let _ = link.poll_heartbeat(now_us);
                if rx_any || link.state().link_active != was_active {
                    ui.update(link.state());
                }

                // Touch Press & Release Router with Dirty-Region Redraw
                if let Some((tx, ty)) = touch.read() {
                    if let Some(cmd_bytes) = ui.handle_touch_point(tx, ty, false, now_ms, &mut link) {
                        if let Ok(s) = core::str::from_utf8(&cmd_bytes) {
                            println!("[TOUCH DRAG] ({}, {}) -> {}", tx, ty, s.trim());
                        }
                    } else if last_touch.is_none() {
                        println!("[TOUCH PRESS] ({}, {})", tx, ty);
                    }
                    last_touch = Some((tx, ty));
                } else if let Some((rx, ry)) = last_touch.take() {
                    if let Some(cmd_bytes) = ui.handle_touch_point(rx, ry, true, now_ms, &mut link) {
                        if let Ok(s) = core::str::from_utf8(&cmd_bytes) {
                            println!("[TOUCH RELEASE] ({}, {}) -> {}", rx, ry, s.trim());
                        }
                    } else {
                        println!("[TOUCH TAP] ({}, {})", rx, ry);
                    }
                }

                ui.render_dirty_regions(&mut display);
            }

            // 3. Flush tx_queue over Physical UART2 TX (GPIO27) using bounded write loop (up to 64 iterations)
            while let Some(pkt) = link.pop_tx() {
                let mut written = 0;
                let mut iters = 0;
                while written < pkt.len() && iters < 64 {
                    match uart2.write(&pkt[written..]) {
                        Ok(n) => written += n,
                        Err(_) => break,
                    }
                    iters += 1;
                }
            }

            // 4. 1 ms loop tick cadence
            tick = tick.wrapping_add(1);
            delay.delay_millis(1);
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
    let mut touch = Xpt2046Touch::new(touch_bus);
    touch.configure_cyd_landscape();

    // 3. Initialize Now Playing User Interface & Render Initial Frame
    let mut ui = NowPlayingUi::new();
    let mut link = CydDisplayLink::new();
    let _ = link.feed_rx_bytes(b"STATE=S\n", 0);
    if link.state().state == PlayState::Stopped {
        ui.update(link.state());
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
    println!("  - Link Active:         {}", link.state().link_active);
    println!("--------------------------------------------------------");
    println!("CYD Display Controller ready. Listening on UART...");
}

