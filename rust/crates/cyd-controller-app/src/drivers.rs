//! CYD Display and Touch Drivers (`ili9341` and `xpt2046`)
//!
//! Provides pure-Rust, zero-allocation hardware abstraction and drivers for:
//! - ILI9341 320x240 RGB565 TFT SPI display controller
//! - XPT2046 12-bit resistive touch controller with 5-point median/averaging filter

pub const CYD_TFT_MISO: i8 = 12;
pub const CYD_TFT_MOSI: i8 = 13;
pub const CYD_TFT_SCLK: i8 = 14;
pub const CYD_TFT_CS: i8 = 15;
pub const CYD_TFT_DC: i8 = 2;
pub const CYD_TFT_RST: i8 = -1;
pub const CYD_TFT_BL: i8 = 21;
pub const CYD_TFT_WIDTH: i32 = 320;
pub const CYD_TFT_HEIGHT: i32 = 240;

pub const CYD_TOUCH_MISO: u8 = 39;
pub const CYD_TOUCH_MOSI: u8 = 32;
pub const CYD_TOUCH_SCLK: u8 = 25;
pub const CYD_TOUCH_CS: u8 = 33;
pub const CYD_TOUCH_IRQ: u8 = 36;
pub const XPT2046_RAW_X_MIN: i16 = 300;
pub const XPT2046_RAW_X_MAX: i16 = 3800;
pub const XPT2046_RAW_Y_MIN: i16 = 300;
pub const XPT2046_RAW_Y_MAX: i16 = 3800;
pub const XPT2046_SAMPLE_COUNT: usize = 5;

/// Hardware abstraction interface for the ILI9341 SPI display bus.
pub trait SpiDisplayBus {
    /// Transmit a 1-byte command (DC pin pulled low).
    fn send_cmd(&mut self, cmd: u8);
    /// Transmit pixel/parameter data bytes (DC pin pulled high).
    fn send_data(&mut self, data: &[u8]);
    /// Set backlight PWM duty cycle (0..=1023 for 10-bit resolution).
    fn set_backlight_duty(&mut self, duty_10bit: u16);
}

/// In-memory mock bus recording commands and transmitted bytes for unit tests.
#[derive(Debug, Clone)]
pub struct MockSpiDisplayBus {
    pub commands: heapless::Vec<u8, 64>,
    pub total_data_bytes: usize,
    pub duty_10bit: u16,
}

impl Default for MockSpiDisplayBus {
    fn default() -> Self {
        Self {
            commands: heapless::Vec::new(),
            total_data_bytes: 0,
            duty_10bit: 1023,
        }
    }
}

impl SpiDisplayBus for MockSpiDisplayBus {
    fn send_cmd(&mut self, cmd: u8) {
        let _ = self.commands.push(cmd);
    }

    fn send_data(&mut self, data: &[u8]) {
        self.total_data_bytes += data.len();
    }

    fn set_backlight_duty(&mut self, duty_10bit: u16) {
        self.duty_10bit = duty_10bit;
    }
}

/// ILI9341 320x240 Landscape TFT Display Driver.
pub struct Ili9341Display<B: SpiDisplayBus> {
    bus: B,
    initialized: bool,
    flush_count: u32,
    last_flush_window: (i32, i32, i32, i32),
    backlight_percent: u8,
}

impl<B: SpiDisplayBus> Ili9341Display<B> {
    /// Create a new ILI9341 display instance with given SPI bus adapter.
    pub fn new(bus: B) -> Self {
        Self {
            bus,
            initialized: false,
            flush_count: 0,
            last_flush_window: (0, 0, 0, 0),
            backlight_percent: 100,
        }
    }

    /// Access the underlying SPI display bus.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Mutably access the underlying SPI display bus.
    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Consume the display and return the underlying SPI bus.
    pub fn into_inner(self) -> B {
        self.bus
    }

    /// Whether display has been initialized with the 21-command sequence.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Total number of successful frame flushes executed.
    pub fn flush_count(&self) -> u32 {
        self.flush_count
    }

    /// The bounding coordinates of the most recent flush: (x1, y1, x2, y2).
    pub fn last_flush_window(&self) -> (i32, i32, i32, i32) {
        self.last_flush_window
    }

    /// Current backlight brightness percentage (0..=100).
    pub fn backlight_percent(&self) -> u8 {
        self.backlight_percent
    }

    /// Execute the full 21-command ILI9341 initialization sequence for 320x240 landscape mode.
    pub fn init(&mut self) {
        // 1. Software Reset
        self.bus.send_cmd(0x01);
        // 2. Display OFF
        self.bus.send_cmd(0x28);
        // 3. Power Control B
        self.bus.send_cmd(0xCF);
        self.bus.send_data(&[0x00, 0xC1, 0x30]);
        // 4. Power on Sequence Control
        self.bus.send_cmd(0xED);
        self.bus.send_data(&[0x64, 0x03, 0x12, 0x81]);
        // 5. Driver Timing Control A
        self.bus.send_cmd(0xE8);
        self.bus.send_data(&[0x85, 0x00, 0x78]);
        // 6. Power Control A
        self.bus.send_cmd(0xCB);
        self.bus.send_data(&[0x39, 0x2C, 0x00, 0x34, 0x02]);
        // 7. Pump Ratio Control
        self.bus.send_cmd(0xF7);
        self.bus.send_data(&[0x20]);
        // 8. Driver Timing Control B
        self.bus.send_cmd(0xEA);
        self.bus.send_data(&[0x00, 0x00]);
        // 9. Power Control 1
        self.bus.send_cmd(0xC0);
        self.bus.send_data(&[0x23]);
        // 10. Power Control 2
        self.bus.send_cmd(0xC1);
        self.bus.send_data(&[0x10]);
        // 11. VCOM Control 1
        self.bus.send_cmd(0xC5);
        self.bus.send_data(&[0x3E, 0x28]);
        // 12. VCOM Control 2
        self.bus.send_cmd(0xC7);
        self.bus.send_data(&[0x86]);
        // 13. MADCTL: Memory Access Control (Landscape MV=1, BGR=1 -> 0x28)
        self.bus.send_cmd(0x36);
        self.bus.send_data(&[0x28]);
        // 14. Pixel Format: 16-bit RGB565 (0x55)
        self.bus.send_cmd(0x3A);
        self.bus.send_data(&[0x55]);
        // 15. Frame Rate Control
        self.bus.send_cmd(0xB1);
        self.bus.send_data(&[0x00, 0x18]);
        // 16. Display Function Control
        self.bus.send_cmd(0xB6);
        self.bus.send_data(&[0x08, 0x82, 0x27]);
        // 17. 3Gamma Function Disable
        self.bus.send_cmd(0xF2);
        self.bus.send_data(&[0x00]);
        // 18. Gamma Curve Selected
        self.bus.send_cmd(0x26);
        self.bus.send_data(&[0x01]);
        // 19. Positive Gamma Correction
        self.bus.send_cmd(0xE0);
        self.bus.send_data(&[
            0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E, 0x09,
            0x00,
        ]);
        // 20. Negative Gamma Correction
        self.bus.send_cmd(0xE1);
        self.bus.send_data(&[
            0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31, 0x36,
            0x0F,
        ]);
        // 21. Sleep OUT
        self.bus.send_cmd(0x11);
        // 22. Display ON
        self.bus.send_cmd(0x29);

        // Turn backlight on to 100% duty
        self.set_backlight(100);
        self.initialized = true;
    }

    /// Flush a rectangular pixel buffer to the display memory.
    ///
    /// Validates and normalizes coordinates, rejects fully out-of-bounds windows,
    /// clamps to screen boundaries, and transmits CASET, PASET, and RAMWR chunks.
    pub fn flush(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32, color_data: &[u16]) {
        if color_data.is_empty() {
            return;
        }

        // Normalize inverted coordinate pairs
        if x1 > x2 {
            core::mem::swap(&mut x1, &mut x2);
        }
        if y1 > y2 {
            core::mem::swap(&mut y1, &mut y2);
        }

        // Reject windows completely outside the display canvas
        if x2 < 0 || y2 < 0 || x1 >= CYD_TFT_WIDTH || y1 >= CYD_TFT_HEIGHT {
            return;
        }

        // Clamp to visible viewport [0..319] and [0..239]
        if x1 < 0 {
            x1 = 0;
        }
        if y1 < 0 {
            y1 = 0;
        }
        if x2 >= CYD_TFT_WIDTH {
            x2 = CYD_TFT_WIDTH - 1;
        }
        if y2 >= CYD_TFT_HEIGHT {
            y2 = CYD_TFT_HEIGHT - 1;
        }

        if x1 > x2 || y1 > y2 {
            return;
        }

        self.last_flush_window = (x1, y1, x2, y2);
        self.flush_count += 1;

        // Column Address Set (CASET: 0x2A)
        self.bus.send_cmd(0x2A);
        let col_data = [
            ((x1 >> 8) & 0xFF) as u8,
            (x1 & 0xFF) as u8,
            ((x2 >> 8) & 0xFF) as u8,
            (x2 & 0xFF) as u8,
        ];
        self.bus.send_data(&col_data);

        // Page Address Set (PASET: 0x2B)
        self.bus.send_cmd(0x2B);
        let page_data = [
            ((y1 >> 8) & 0xFF) as u8,
            (y1 & 0xFF) as u8,
            ((y2 >> 8) & 0xFF) as u8,
            (y2 & 0xFF) as u8,
        ];
        self.bus.send_data(&page_data);

        // Memory Write (RAMWR: 0x2C)
        self.bus.send_cmd(0x2C);

        let pixel_count = ((x2 - x1 + 1) as usize) * ((y2 - y1 + 1) as usize);
        let count_to_send = color_data.len().min(pixel_count);

        // Transmit in chunks of up to 1024 bytes (<= 4096 bytes)
        const CHUNK_SIZE: usize = 1024;
        let mut byte_buf = [0u8; CHUNK_SIZE];
        let mut buf_idx = 0;

        for &pixel in &color_data[..count_to_send] {
            let bytes = pixel.to_be_bytes();
            byte_buf[buf_idx] = bytes[0];
            byte_buf[buf_idx + 1] = bytes[1];
            buf_idx += 2;
            if buf_idx == CHUNK_SIZE {
                self.bus.send_data(&byte_buf);
                buf_idx = 0;
            }
        }
        if buf_idx > 0 {
            self.bus.send_data(&byte_buf[..buf_idx]);
        }
    }

    /// Set backlight brightness percentage (0..=100%).
    pub fn set_backlight(&mut self, percent: u8) {
        let clamped = percent.min(100);
        self.backlight_percent = clamped;
        let duty = ((clamped as u32 * 1023) / 100) as u16;
        self.bus.set_backlight_duty(duty);
    }
}

/// Hardware abstraction interface for the XPT2046 touch controller SPI bus.
pub trait TouchSpiBus {
    /// Check whether touch IRQ is asserted low (true = active touch).
    fn is_irq_low(&self) -> bool;
    /// Transmit 1-byte command and read 12-bit ADC response.
    fn read_raw_12bit(&mut self, cmd: u8) -> u16;
}

/// In-memory mock touch bus for unit testing.
#[derive(Debug, Default, Clone)]
pub struct MockTouchSpiBus {
    pub touched: bool,
    pub raw_x: u16,
    pub raw_y: u16,
}

impl MockTouchSpiBus {
    /// Create a mock touch bus with configured touch status and raw ADC coordinates.
    pub fn new(touched: bool, raw_x: u16, raw_y: u16) -> Self {
        Self { touched, raw_x, raw_y }
    }

    /// Update mock touch status and raw ADC coordinates.
    pub fn set_touch(&mut self, touched: bool, raw_x: u16, raw_y: u16) {
        self.touched = touched;
        self.raw_x = raw_x;
        self.raw_y = raw_y;
    }
}

impl TouchSpiBus for MockTouchSpiBus {
    fn is_irq_low(&self) -> bool {
        self.touched
    }

    fn read_raw_12bit(&mut self, cmd: u8) -> u16 {
        if !self.touched {
            0
        } else if cmd == 0xD0 {
            self.raw_x
        } else if cmd == 0x90 {
            self.raw_y
        } else {
            0
        }
    }
}

/// XPT2046 Resistive Touch Controller Driver.
pub struct Xpt2046Touch<B: TouchSpiBus> {
    bus: B,
    cal_x_min: i16,
    cal_x_max: i16,
    cal_y_min: i16,
    cal_y_max: i16,
}

impl<B: TouchSpiBus> Xpt2046Touch<B> {
    /// Create a new XPT2046 touch driver with default CYD calibration.
    pub fn new(bus: B) -> Self {
        Self {
            bus,
            cal_x_min: XPT2046_RAW_X_MIN,
            cal_x_max: XPT2046_RAW_X_MAX,
            cal_y_min: XPT2046_RAW_Y_MIN,
            cal_y_max: XPT2046_RAW_Y_MAX,
        }
    }

    /// Access the underlying touch SPI bus.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Mutably access the underlying touch SPI bus.
    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    /// Set custom ADC calibration boundaries.
    pub fn set_calibration(&mut self, x_min: i16, x_max: i16, y_min: i16, y_max: i16) {
        self.cal_x_min = x_min;
        self.cal_x_max = x_max;
        self.cal_y_min = y_min;
        self.cal_y_max = y_max;
    }

    /// Retrieve current calibration boundaries: (x_min, x_max, y_min, y_max).
    pub fn calibration(&self) -> (i16, i16, i16, i16) {
        (self.cal_x_min, self.cal_x_max, self.cal_y_min, self.cal_y_max)
    }

    /// Map raw 12-bit ADC coordinates into calibrated 320x240 landscape screen coordinates.
    pub fn calibrate_raw(&self, raw_x: i16, raw_y: i16) -> (i16, i16) {
        let mut x_range = (self.cal_x_max as i32) - (self.cal_x_min as i32);
        let mut y_range = (self.cal_y_max as i32) - (self.cal_y_min as i32);

        if x_range == 0 {
            x_range = 1;
        }
        if y_range == 0 {
            y_range = 1;
        }

        let mut cal_x = ((raw_x as i32) - (self.cal_x_min as i32)) * (CYD_TFT_WIDTH - 1) / x_range;
        let mut cal_y = ((raw_y as i32) - (self.cal_y_min as i32)) * (CYD_TFT_HEIGHT - 1) / y_range;

        // Clamp to screen boundaries
        if cal_x < 0 {
            cal_x = 0;
        }
        if cal_x >= CYD_TFT_WIDTH {
            cal_x = CYD_TFT_WIDTH - 1;
        }

        if cal_y < 0 {
            cal_y = 0;
        }
        if cal_y >= CYD_TFT_HEIGHT {
            cal_y = CYD_TFT_HEIGHT - 1;
        }

        (cal_x as i16, cal_y as i16)
    }

    /// Read touch sensor with multi-sample filtering.
    ///
    /// Reads 5 samples, discards invalid values outside [100, 4050],
    /// requires at least 3 valid samples, averages them, and returns calibrated coordinates.
    pub fn read(&mut self) -> Option<(i16, i16)> {
        if !self.bus.is_irq_low() {
            return None;
        }

        let mut sum_x: i32 = 0;
        let mut sum_y: i32 = 0;
        let mut valid_count = 0;

        for _ in 0..XPT2046_SAMPLE_COUNT {
            let sx = self.bus.read_raw_12bit(0xD0);
            let sy = self.bus.read_raw_12bit(0x90);

            if (100..=4050).contains(&sx) && (100..=4050).contains(&sy) {
                sum_x += sx as i32;
                sum_y += sy as i32;
                valid_count += 1;
            }
        }

        if valid_count < 3 {
            return None;
        }

        let avg_x = (sum_x / valid_count) as i16;
        let avg_y = (sum_y / valid_count) as i16;
        Some(self.calibrate_raw(avg_x, avg_y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xpt2046_calibrate_raw_normal() {
        let bus = MockTouchSpiBus::default();
        let touch = Xpt2046Touch::new(bus);

        // Minimum raw coordinates should map exactly to (0, 0)
        let (out_x, out_y) = touch.calibrate_raw(300, 300);
        assert_eq!(out_x, 0);
        assert_eq!(out_y, 0);

        // Maximum raw coordinates should map exactly to (319, 239)
        let (out_x, out_y) = touch.calibrate_raw(3800, 3800);
        assert_eq!(out_x, (CYD_TFT_WIDTH - 1) as i16);
        assert_eq!(out_y, (CYD_TFT_HEIGHT - 1) as i16);

        // Midpoint coordinates (300 + 3800) / 2 = 2050 -> (159, 119)
        let (out_x, out_y) = touch.calibrate_raw(2050, 2050);
        assert_eq!(out_x, 159);
        assert_eq!(out_y, 119);
    }

    #[test]
    fn test_xpt2046_calibrate_raw_clamping() {
        let bus = MockTouchSpiBus::default();
        let touch = Xpt2046Touch::new(bus);

        // Below minimum bounds should clamp to 0
        let (out_x, out_y) = touch.calibrate_raw(100, 50);
        assert_eq!(out_x, 0);
        assert_eq!(out_y, 0);

        // Negative raw ADC should clamp to 0
        let (out_x, out_y) = touch.calibrate_raw(-200, -500);
        assert_eq!(out_x, 0);
        assert_eq!(out_y, 0);

        // Above maximum bounds should clamp to max landscape coordinates
        let (out_x, out_y) = touch.calibrate_raw(4000, 4200);
        assert_eq!(out_x, (CYD_TFT_WIDTH - 1) as i16);
        assert_eq!(out_y, (CYD_TFT_HEIGHT - 1) as i16);

        // Mixed out of bounds: X below min, Y above max
        let (out_x, out_y) = touch.calibrate_raw(50, 4500);
        assert_eq!(out_x, 0);
        assert_eq!(out_y, (CYD_TFT_HEIGHT - 1) as i16);

        // Mixed out of bounds: X above max, Y below min
        let (out_x, out_y) = touch.calibrate_raw(4500, 50);
        assert_eq!(out_x, (CYD_TFT_WIDTH - 1) as i16);
        assert_eq!(out_y, 0);
    }

    #[test]
    fn test_xpt2046_custom_calibration() {
        let bus = MockTouchSpiBus::default();
        let mut touch = Xpt2046Touch::new(bus);

        touch.set_calibration(500, 3500, 400, 3600);
        let (out_x, out_y) = touch.calibrate_raw(500, 400);
        assert_eq!(out_x, 0);
        assert_eq!(out_y, 0);

        let (out_x, out_y) = touch.calibrate_raw(3500, 3600);
        assert_eq!(out_x, (CYD_TFT_WIDTH - 1) as i16);
        assert_eq!(out_y, (CYD_TFT_HEIGHT - 1) as i16);
    }

    #[test]
    fn test_xpt2046_read_mock() {
        let mut bus = MockTouchSpiBus::default();
        bus.set_touch(false, 0, 0);
        let mut touch = Xpt2046Touch::new(bus);

        // When screen is untouched
        assert_eq!(touch.read(), None);

        // When screen is touched at center (2050, 2050)
        touch.bus_mut().set_touch(true, 2050, 2050);
        assert_eq!(touch.read(), Some((159, 119)));

        // When raw ADC values are out-of-range (< 100), filter rejects them
        touch.bus_mut().set_touch(true, 50, 50);
        assert_eq!(touch.read(), None);
    }

    #[test]
    fn test_ili9341_init_sequence() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        assert!(!display.is_initialized());

        display.init();
        assert!(display.is_initialized());
        assert_eq!(display.backlight_percent(), 100);
        assert_eq!(display.bus().duty_10bit, 1023);
        assert!(display.bus().commands.len() >= 21);
        assert_eq!(display.bus().commands[0], 0x01); // Reset
        assert_eq!(display.bus().commands[1], 0x28); // Display OFF
        assert_eq!(*display.bus().commands.last().unwrap(), 0x29); // Display ON
    }

    #[test]
    fn test_ili9341_flush_normal_window() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        let dummy_pixels = [0x1DB9u16; 640];

        let count_before = display.flush_count();
        display.flush(10, 20, 100, 150, &dummy_pixels);

        assert_eq!(display.last_flush_window(), (10, 20, 100, 150));
        assert_eq!(display.flush_count(), count_before + 1);
        assert!(display.bus().total_data_bytes > 0);
    }

    #[test]
    fn test_ili9341_flush_bounds_clamping() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        let dummy_pixels = [0xFFFFu16; 640];
        let count_before = display.flush_count();

        // Negative coordinates clamped to 0
        display.flush(-25, -40, 150, 100, &dummy_pixels);
        assert_eq!(display.last_flush_window(), (0, 0, 150, 100));

        // Oversized coordinates clamped to screen width-1 and height-1
        display.flush(50, 60, 400, 350, &dummy_pixels);
        assert_eq!(display.last_flush_window(), (50, 60, 319, 239));

        // Full screen overflow clamped to complete display area
        display.flush(-100, -100, 500, 500, &dummy_pixels);
        assert_eq!(display.last_flush_window(), (0, 0, 319, 239));

        assert_eq!(display.flush_count(), count_before + 3);
    }

    #[test]
    fn test_ili9341_flush_inverted_coordinates() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        let dummy_pixels = [0x0000u16; 100];

        display.flush(150, 180, 20, 30, &dummy_pixels);
        assert_eq!(display.last_flush_window(), (20, 30, 150, 180));
    }

    #[test]
    fn test_ili9341_flush_invalid_inputs() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        let count_before = display.flush_count();

        // Empty pixel slice should be rejected immediately
        display.flush(0, 0, 50, 50, &[]);
        assert_eq!(display.flush_count(), count_before);

        // Completely out-of-bounds window (all beyond width/height)
        let dummy = [0u16; 10];
        display.flush(350, 260, 400, 300, &dummy);
        assert_eq!(display.flush_count(), count_before);

        // Completely negative window
        display.flush(-200, -200, -50, -50, &dummy);
        assert_eq!(display.flush_count(), count_before);
    }

    #[test]
    fn test_ili9341_backlight_control() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);

        display.set_backlight(75);
        assert_eq!(display.backlight_percent(), 75);
        assert_eq!(display.bus().duty_10bit, 767); // (75 * 1023) / 100 = 767

        display.set_backlight(0);
        assert_eq!(display.backlight_percent(), 0);
        assert_eq!(display.bus().duty_10bit, 0);

        display.set_backlight(100);
        assert_eq!(display.backlight_percent(), 100);
        assert_eq!(display.bus().duty_10bit, 1023);

        // Over 100% clamped to 100%
        display.set_backlight(180);
        assert_eq!(display.backlight_percent(), 100);
        assert_eq!(display.bus().duty_10bit, 1023);
    }
}
