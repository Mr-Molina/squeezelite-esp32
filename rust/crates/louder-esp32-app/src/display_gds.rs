//! GDS-Equivalent Display Stack & LMS Graphics Protocol Engine (Phase 7).
//!
//! Provides pure-Rust, `no_std`-compatible display controller descriptors,
//! framebuffer conversion (1-bpp monochrome, 4-bpp grayscale, RGB565 tiles),
//! LMS SlimProto display packet handlers (`vfdc`, `grfe`, `grfb`, `grfs`,
//! `grfg`, `grfa`, `visu`, `ANIC`, `SETD 0xFE`), smooth text scrolling, and
//! external source (Bluetooth, AirPlay, Spotify) metadata screen layout.
#![forbid(unsafe_code)]

/// Maximum framebuffer width supported by the static 1-bpp/4-bpp canvas.
pub const MAX_DISPLAY_WIDTH: usize = 256;
/// Maximum framebuffer height supported by the static 1-bpp/4-bpp canvas.
pub const MAX_DISPLAY_HEIGHT: usize = 64;
/// Total pixels in the static GDS canvas (`256 * 64 = 16,384`).
pub const MAX_CANVAS_PIXELS: usize = MAX_DISPLAY_WIDTH * MAX_DISPLAY_HEIGHT;
/// Packed 1-bpp buffer capacity in bytes (`16,384 / 8 = 2,048`).
pub const MONO_BUFFER_BYTES: usize = MAX_CANVAS_PIXELS / 8;
/// SlimProto `SETD` ID used to report graphic display dimensions (`0xFE`).
pub const SETD_DISPLAY_DIMENSIONS_ID: u8 = 0xFE;

/// Supported display controller families (C `gds` driver parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DisplayDriverKind {
    /// Solomon Systech SSD1306 128x64 / 128x32 1-bpp I2C/SPI OLED.
    Ssd1306,
    /// Sino Wealth SH1106 128x64 1-bpp paged OLED (132-column RAM).
    Sh1106,
    /// Sino Wealth SH1122 256x64 4-bpp grayscale OLED.
    Sh1122,
    /// Solomon Systech SSD1322 256x64 4-bpp grayscale OLED.
    Ssd1322,
    /// Solomon Systech SSD1327 128x128 4-bpp grayscale OLED.
    Ssd1327,
    /// Solomon Systech SSD1351 128x128 RGB565 color OLED.
    Ssd1351,
    /// Solomon Systech SSD1675 1-bpp e-Paper controller.
    Ssd1675,
    /// Sitronix ST7735 160x128 RGB565 TFT LCD.
    St7735,
    /// Sitronix ST7789 240x240 / 240x135 RGB565 IPS TFT LCD.
    St7789,
    /// ILITEK ILI9341 320x240 RGB565 TFT LCD.
    Ili9341,
}

/// Native pixel encoding of a [`DisplayDriverKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DisplayColorMode {
    /// 1-bit monochrome (page/column vertical or horizontal packing).
    Monochrome1Bpp,
    /// 4-bit grayscale (2 pixels per byte, high nibble first).
    Grayscale4Bpp,
    /// 16-bit RGB565 big-endian color (rendered on-the-fly by tile).
    Rgb565,
}

const INIT_SSD1306: &[u8] = &[
    0xAE, 0xD5, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0x40, 0x8D, 0x14, 0x20, 0x00, 0xA1, 0xC8, 0xDA,
    0x12, 0x81, 0xCF, 0xD9, 0xF1, 0xDB, 0x40, 0xA4, 0xA6, 0xAF,
];
const INIT_SH1106: &[u8] = &[
    0xAE, 0x02, 0x10, 0x40, 0x81, 0x80, 0xA1, 0xC8, 0xA6, 0xA8, 0x3F, 0xD3, 0x00, 0xD5, 0x80,
    0xD9, 0xF1, 0xDA, 0x12, 0xDB, 0x40, 0xAF,
];
const INIT_SH1122: &[u8] = &[
    0xAE, 0x40, 0xA0, 0xC0, 0x81, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0xD5, 0x50, 0xD9, 0x22, 0xDB,
    0x35, 0xDC, 0x35, 0x30, 0xAF,
];
const INIT_SSD1322: &[u8] = &[
    0xFD, 0x12, 0xAE, 0xB3, 0x91, 0xCA, 0x3F, 0xA2, 0x00, 0xA1, 0x00, 0xA0, 0x14, 0x11, 0xC1,
    0x9F, 0xC7, 0x0F, 0xB9, 0xAF,
];
const INIT_SSD1327: &[u8] = &[
    0xAE, 0x15, 0x00, 0x7F, 0x75, 0x00, 0x7F, 0x81, 0x80, 0xA0, 0x51, 0xA1, 0x00, 0xA2, 0x00,
    0xA4, 0xA8, 0x7F, 0xAB, 0x01, 0xAF,
];
const INIT_SSD1351: &[u8] = &[
    0xFD, 0x12, 0xFD, 0xB1, 0xAE, 0xB3, 0xF1, 0xCA, 0x7F, 0xA0, 0x74, 0xA1, 0x00, 0xA2, 0x00,
    0xAB, 0x01, 0xAF,
];
const INIT_SSD1675: &[u8] = &[
    0x12, 0x01, 0x27, 0x01, 0x00, 0x11, 0x03, 0x44, 0x00, 0x0F, 0x45, 0x00, 0x00, 0x27, 0x01,
    0x20,
];
const INIT_ST7735: &[u8] = &[0x01, 0x11, 0x3A, 0x05, 0x36, 0x60, 0x13, 0x29];
const INIT_ST7789: &[u8] = &[0x01, 0x11, 0x3A, 0x55, 0x36, 0x00, 0x21, 0x13, 0x29];
const INIT_ILI9341: &[u8] = &[0x01, 0x11, 0x3A, 0x55, 0x36, 0x28, 0x29];

impl DisplayDriverKind {
    /// Parses a driver model identifier from a `display_config` string token.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("SSD1306") {
            Some(Self::Ssd1306)
        } else if trimmed.eq_ignore_ascii_case("SH1106") {
            Some(Self::Sh1106)
        } else if trimmed.eq_ignore_ascii_case("SH1122") {
            Some(Self::Sh1122)
        } else if trimmed.eq_ignore_ascii_case("SSD1322")
            || trimmed.eq_ignore_ascii_case("SSD132X")
        {
            Some(Self::Ssd1322)
        } else if trimmed.eq_ignore_ascii_case("SSD1327") {
            Some(Self::Ssd1327)
        } else if trimmed.eq_ignore_ascii_case("SSD1351") {
            Some(Self::Ssd1351)
        } else if trimmed.eq_ignore_ascii_case("SSD1675") {
            Some(Self::Ssd1675)
        } else if trimmed.eq_ignore_ascii_case("ST7735") {
            Some(Self::St7735)
        } else if trimmed.eq_ignore_ascii_case("ST7789") {
            Some(Self::St7789)
        } else if trimmed.eq_ignore_ascii_case("ILI9341") {
            Some(Self::Ili9341)
        } else {
            None
        }
    }

    /// Canonical controller name matching C `display_config` strings.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ssd1306 => "SSD1306",
            Self::Sh1106 => "SH1106",
            Self::Sh1122 => "SH1122",
            Self::Ssd1322 => "SSD1322",
            Self::Ssd1327 => "SSD1327",
            Self::Ssd1351 => "SSD1351",
            Self::Ssd1675 => "SSD1675",
            Self::St7735 => "ST7735",
            Self::St7789 => "ST7789",
            Self::Ili9341 => "ILI9341",
        }
    }

    /// Native pixel color depth and packing mode for this controller.
    #[must_use]
    pub const fn color_mode(self) -> DisplayColorMode {
        match self {
            Self::Ssd1306 | Self::Sh1106 | Self::Ssd1675 => DisplayColorMode::Monochrome1Bpp,
            Self::Sh1122 | Self::Ssd1322 | Self::Ssd1327 => DisplayColorMode::Grayscale4Bpp,
            Self::Ssd1351 | Self::St7735 | Self::St7789 | Self::Ili9341 => {
                DisplayColorMode::Rgb565
            }
        }
    }

    /// Default geometry `(width, height)` for this controller.
    #[must_use]
    pub const fn default_dimensions(self) -> (u16, u16) {
        match self {
            Self::Ssd1306 | Self::Sh1106 => (128, 64),
            Self::Sh1122 | Self::Ssd1322 => (256, 64),
            Self::Ssd1327 | Self::Ssd1351 => (128, 128),
            Self::Ssd1675 => (250, 122),
            Self::St7735 => (160, 128),
            Self::St7789 => (240, 240),
            Self::Ili9341 => (320, 240),
        }
    }

    /// Static hardware initialization command sequence for this controller.
    #[must_use]
    pub const fn init_commands(self) -> &'static [u8] {
        match self {
            Self::Ssd1306 => INIT_SSD1306,
            Self::Sh1106 => INIT_SH1106,
            Self::Sh1122 => INIT_SH1122,
            Self::Ssd1322 => INIT_SSD1322,
            Self::Ssd1327 => INIT_SSD1327,
            Self::Ssd1351 => INIT_SSD1351,
            Self::Ssd1675 => INIT_SSD1675,
            Self::St7735 => INIT_ST7735,
            Self::St7789 => INIT_ST7789,
            Self::Ili9341 => INIT_ILI9341,
        }
    }
}

/// Shared SPI/I2C display bus arbitration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DisplayBusArbiter {
    bus_locked: bool,
    owner_tag: u8,
    transactions_completed: u32,
}

impl Default for DisplayBusArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayBusArbiter {
    /// Creates an unlocked display bus arbiter.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bus_locked: false,
            owner_tag: 0,
            transactions_completed: 0,
        }
    }

    /// Attempts to acquire the display bus for `owner_tag`.
    pub fn try_acquire(&mut self, owner_tag: u8) -> bool {
        if self.bus_locked {
            return false;
        }
        self.bus_locked = true;
        self.owner_tag = owner_tag;
        true
    }

    /// Releases the display bus if held by `owner_tag` and increments counter.
    pub fn release(&mut self, owner_tag: u8) -> bool {
        if !self.bus_locked || self.owner_tag != owner_tag {
            return false;
        }
        self.bus_locked = false;
        self.owner_tag = 0;
        self.transactions_completed = self.transactions_completed.wrapping_add(1);
        true
    }

    /// Returns whether the bus is currently locked.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.bus_locked
    }

    /// Returns the number of completed bus transactions.
    #[must_use]
    pub const fn transactions_completed(&self) -> u32 {
        self.transactions_completed
    }
}

/// GDS-equivalent 1-bpp/4-bpp/RGB565 framebuffer with dirty tracking.
#[derive(Clone)]
#[non_exhaustive]
pub struct GdsFramebuffer {
    driver: DisplayDriverKind,
    width: u16,
    height: u16,
    contrast: u8,
    dirty: bool,
    /// Packed 1-bpp canvas (`256 * 64 / 8 = 2048` bytes, row-major).
    mono_bits: [u8; MONO_BUFFER_BYTES],
    /// Foreground RGB565 color used when exporting RGB565 tiles.
    fg_rgb565: u16,
    /// Background RGB565 color used when exporting RGB565 tiles.
    bg_rgb565: u16,
}

impl GdsFramebuffer {
    /// Creates a new GDS framebuffer for `driver` with clamped dimensions.
    #[must_use]
    pub fn new(driver: DisplayDriverKind, width: u16, height: u16) -> Self {
        let w = width.clamp(1, MAX_DISPLAY_WIDTH as u16);
        let h = height.clamp(1, MAX_DISPLAY_HEIGHT as u16);
        Self {
            driver,
            width: w,
            height: h,
            contrast: 0x80,
            dirty: true,
            mono_bits: [0u8; MONO_BUFFER_BYTES],
            fg_rgb565: 0xFFFF,
            bg_rgb565: 0x0000,
        }
    }

    /// Returns the active display driver kind.
    #[must_use]
    pub const fn driver(&self) -> DisplayDriverKind {
        self.driver
    }

    /// Returns the configured framebuffer width in pixels.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Returns the configured framebuffer height in pixels.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Returns the current display contrast/brightness byte (`0..=255`).
    #[must_use]
    pub const fn contrast(&self) -> u8 {
        self.contrast
    }

    /// Sets the display contrast/brightness byte (`0..=255`).
    pub fn set_contrast(&mut self, contrast: u8) {
        self.contrast = contrast;
        self.dirty = true;
    }

    /// Sets the foreground and background RGB565 colors for color displays.
    pub fn set_palette_rgb565(&mut self, fg: u16, bg: u16) {
        self.fg_rgb565 = fg;
        self.bg_rgb565 = bg;
        self.dirty = true;
    }

    /// Returns whether the framebuffer has un-flushed modifications.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clears the dirty flag after flushing to hardware.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Clears all pixels in the framebuffer to off (`false`).
    pub fn clear(&mut self) {
        self.mono_bits.fill(0);
        self.dirty = true;
    }

    /// Sets a single pixel at `(x, y)` within bounds.
    pub fn set_pixel(&mut self, x: u16, y: u16, on: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        let byte_idx = idx >> 3;
        let bit_mask = 1u8 << (idx & 7);
        if byte_idx < MONO_BUFFER_BYTES {
            if on {
                self.mono_bits[byte_idx] |= bit_mask;
            } else {
                self.mono_bits[byte_idx] &= !bit_mask;
            }
            self.dirty = true;
        }
    }

    /// Reads whether pixel `(x, y)` is lit.
    #[must_use]
    pub fn pixel(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        let byte_idx = idx >> 3;
        let bit_mask = 1u8 << (idx & 7);
        if byte_idx < MONO_BUFFER_BYTES {
            (self.mono_bits[byte_idx] & bit_mask) != 0
        } else {
            false
        }
    }

    /// Draws a filled rectangle `[x..x+w, y..y+h)`.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, on: bool) {
        let y_end = y.saturating_add(h).min(self.height);
        let x_end = x.saturating_add(w).min(self.width);
        let mut py = y;
        while py < y_end {
            let mut px = x;
            while px < x_end {
                self.set_pixel(px, py, on);
                px += 1;
            }
            py += 1;
        }
    }

    /// Renders an LMS `grfe` column-major bitmap into the framebuffer.
    ///
    /// In SlimProto `grfe`, each column uses `height / 8` bytes (top-to-bottom,
    /// MSB = topmost pixel of each byte).
    pub fn draw_lms_column_bitmap(&mut self, x_offset: u16, data: &[u8]) {
        let bytes_per_col = (self.height as usize).div_ceil(8);
        if bytes_per_col == 0 {
            return;
        }
        let num_cols = data.len() / bytes_per_col;
        let mut col = 0usize;
        while col < num_cols {
            let x = x_offset.saturating_add(col as u16);
            if x >= self.width {
                break;
            }
            let col_base = col * bytes_per_col;
            let mut row_byte = 0usize;
            while row_byte < bytes_per_col {
                let b = data[col_base + row_byte];
                let mut bit = 0u16;
                while bit < 8 {
                    let y = (row_byte as u16) * 8 + bit;
                    if y < self.height {
                        let on = ((b >> (7 - bit)) & 1) != 0;
                        self.set_pixel(x, y, on);
                    }
                    bit += 1;
                }
                row_byte += 1;
            }
            col += 1;
        }
    }

    /// Exports SSD1306/SH1106 vertical page-major bytes (8 vertical pixels per
    /// byte, LSB = top pixel of page).
    pub fn export_ssd1306_pages(&self, out: &mut [u8]) -> usize {
        let pages = (self.height as usize).div_ceil(8);
        let width = self.width as usize;
        let required = pages * width;
        if out.len() < required {
            return 0;
        }
        let mut written = 0usize;
        let mut page = 0usize;
        while page < pages {
            let mut x = 0usize;
            while x < width {
                let mut page_byte = 0u8;
                let mut bit = 0usize;
                while bit < 8 {
                    let y = page * 8 + bit;
                    if self.pixel(x as u16, y as u16) {
                        page_byte |= 1u8 << bit;
                    }
                    bit += 1;
                }
                out[written] = page_byte;
                written += 1;
                x += 1;
            }
            page += 1;
        }
        written
    }

    /// Exports 4-bpp packed grayscale bytes (2 horizontal pixels per byte,
    /// `0xF` for lit pixels and `0x0` for unlit pixels) for SSD1322/SH1122.
    pub fn export_gray4_packed(&self, out: &mut [u8]) -> usize {
        let total_pixels = (self.width as usize) * (self.height as usize);
        let required = total_pixels.div_ceil(2);
        if out.len() < required {
            return 0;
        }
        let mut written = 0usize;
        let mut y = 0u16;
        let mut high_nibble: Option<u8> = None;
        while y < self.height {
            let mut x = 0u16;
            while x < self.width {
                let nib = if self.pixel(x, y) { 0x0Fu8 } else { 0x00u8 };
                if let Some(hi) = high_nibble.take() {
                    out[written] = (hi << 4) | nib;
                    written += 1;
                } else {
                    high_nibble = Some(nib);
                }
                x += 1;
            }
            y += 1;
        }
        if let Some(hi) = high_nibble {
            out[written] = hi << 4;
            written += 1;
        }
        written
    }

    /// Exports a rectangular tile `[x..x+w, y..y+h)` as big-endian RGB565
    /// bytes for ST7735/ST7789/ILI9341/SSD1351 SPI burst transfers.
    pub fn export_rgb565_tile(&self, x: u16, y: u16, w: u16, h: u16, out: &mut [u8]) -> usize {
        let required = (w as usize) * (h as usize) * 2;
        if out.len() < required {
            return 0;
        }
        let mut written = 0usize;
        let mut py = 0u16;
        while py < h {
            let mut px = 0u16;
            while px < w {
                let color = if self.pixel(x.saturating_add(px), y.saturating_add(py)) {
                    self.fg_rgb565
                } else {
                    self.bg_rgb565
                };
                let bytes = color.to_be_bytes();
                out[written] = bytes[0];
                out[written + 1] = bytes[1];
                written += 2;
                px += 1;
            }
            py += 1;
        }
        written
    }
}

/// Visualizer mode configured by the LMS `visu` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VisualizerMode {
    /// No visualizer overlay active.
    Off,
    /// Stereo or mono VU meter (`visu` mode 1).
    VuMeter {
        /// Number of channels (`1` or `2`).
        channels: u8,
    },
    /// FFT spectrum analyzer (`visu` mode 2).
    Spectrum {
        /// Number of frequency bands (`8..=64`).
        bands: u8,
    },
}

/// Parsed LMS display / graphics SlimProto command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LmsDisplayCommand<'a> {
    /// `vfdc` VFD character/data payload.
    Vfdc {
        /// Raw VFD command payload bytes.
        payload: &'a [u8],
    },
    /// `grfe` standard graphic frame update (`offset: u16`, `transition: u8`,
    /// `param: u8`, followed by column-major 1-bpp bitmap bytes).
    Grfe {
        /// Horizontal column offset.
        offset: u16,
        /// Transition character (e.g., `b'c'`, `b'u'`, `b'd'`, `b'l'`, `b'r'`).
        transition: u8,
        /// Transition parameter.
        param: u8,
        /// Column-major bitmap slice.
        bitmap: &'a [u8],
    },
    /// `grfb` display brightness command (`i16` brightness level).
    Grfb {
        /// Signed brightness code (`-1..=5`).
        brightness: i16,
    },
    /// `grfs` graphic screen size query.
    Grfs,
    /// `grfg` grayscale / partial region graphic update.
    Grfg {
        /// Destination X coordinate.
        x: u16,
        /// Destination width in pixels.
        width: u16,
        /// Raw pixel payload.
        data: &'a [u8],
    },
    /// `grfa` compressed / artwork graphic frame update.
    Grfa {
        /// Frame format/compression flags.
        flags: u16,
        /// Raw artwork/frame payload.
        data: &'a [u8],
    },
    /// `visu` visualizer configuration packet.
    Visu {
        /// Parsed visualizer mode.
        mode: VisualizerMode,
    },
}

impl<'a> LmsDisplayCommand<'a> {
    /// Parses a 4-byte SlimProto command tag and payload into an
    /// [`LmsDisplayCommand`].
    #[must_use]
    pub fn parse(tag: &[u8; 4], payload: &'a [u8]) -> Option<Self> {
        match tag {
            b"vfdc" => Some(Self::Vfdc { payload }),
            b"grfe" => {
                if payload.len() < 4 {
                    return None;
                }
                let offset = u16::from_be_bytes([payload[0], payload[1]]);
                let transition = payload[2];
                let param = payload[3];
                Some(Self::Grfe {
                    offset,
                    transition,
                    param,
                    bitmap: &payload[4..],
                })
            }
            b"grfb" => {
                if payload.len() < 2 {
                    return None;
                }
                let brightness = i16::from_be_bytes([payload[0], payload[1]]);
                Some(Self::Grfb { brightness })
            }
            b"grfs" => Some(Self::Grfs),
            b"grfg" => {
                if payload.len() < 4 {
                    return None;
                }
                let x = u16::from_be_bytes([payload[0], payload[1]]);
                let width = u16::from_be_bytes([payload[2], payload[3]]);
                Some(Self::Grfg {
                    x,
                    width,
                    data: &payload[4..],
                })
            }
            b"grfa" => {
                if payload.len() < 2 {
                    return None;
                }
                let flags = u16::from_be_bytes([payload[0], payload[1]]);
                Some(Self::Grfa {
                    flags,
                    data: &payload[2..],
                })
            }
            b"visu" => {
                let which = payload.first().copied().unwrap_or(0);
                let count = payload.get(1).copied().unwrap_or(2);
                let mode = match which {
                    1 => VisualizerMode::VuMeter {
                        channels: count.clamp(1, 2),
                    },
                    2 => VisualizerMode::Spectrum {
                        bands: count.clamp(8, 64),
                    },
                    _ => VisualizerMode::Off,
                };
                Some(Self::Visu { mode })
            }
            _ => None,
        }
    }
}

/// Builds a SlimProto `ANIC` (animation complete) client-to-server packet.
///
/// Wire layout: `"ANIC"` (4B) + `len = 1` (4B BE) + `flags` (1B) = 9 bytes.
#[must_use]
pub fn build_anic_frame(flags: u8, out: &mut [u8]) -> Option<usize> {
    if out.len() < 9 {
        return None;
    }
    out[0..4].copy_from_slice(b"ANIC");
    out[4..8].copy_from_slice(&1u32.to_be_bytes());
    out[8] = flags;
    Some(9)
}

/// Builds a SlimProto `SETD` ID `0xFE` client-to-server packet reporting
/// graphic display dimensions `(width, height)`.
///
/// Wire layout: `"SETD"` (4B) + `len = 5` (4B BE) + `0xFE` (1B) + `width`
/// (2B BE) + `height` (2B BE) = 13 bytes.
#[must_use]
pub fn build_setd_display_size_frame(width: u16, height: u16, out: &mut [u8]) -> Option<usize> {
    if out.len() < 13 {
        return None;
    }
    out[0..4].copy_from_slice(b"SETD");
    out[4..8].copy_from_slice(&5u32.to_be_bytes());
    out[8] = SETD_DISPLAY_DIMENSIONS_ID;
    out[9..11].copy_from_slice(&width.to_be_bytes());
    out[11..13].copy_from_slice(&height.to_be_bytes());
    Some(13)
}

/// Stateful LMS Display Engine coordinating [`GdsFramebuffer`], `ANIC`, and
/// `SETD 0xFE`.
#[non_exhaustive]
pub struct GdsDisplayEngine {
    fb: GdsFramebuffer,
    visualizer: VisualizerMode,
    anic_pending: bool,
    size_report_pending: bool,
    frames_rendered: u32,
}

impl GdsDisplayEngine {
    /// Creates a new [`GdsDisplayEngine`] wrapping a [`GdsFramebuffer`].
    #[must_use]
    pub fn new(fb: GdsFramebuffer) -> Self {
        Self {
            fb,
            visualizer: VisualizerMode::Off,
            anic_pending: false,
            size_report_pending: true,
            frames_rendered: 0,
        }
    }

    /// Borrows the underlying [`GdsFramebuffer`].
    #[must_use]
    pub const fn framebuffer(&self) -> &GdsFramebuffer {
        &self.fb
    }

    /// Mutably borrows the underlying [`GdsFramebuffer`].
    pub fn framebuffer_mut(&mut self) -> &mut GdsFramebuffer {
        &mut self.fb
    }

    /// Returns the active [`VisualizerMode`].
    #[must_use]
    pub const fn visualizer(&self) -> VisualizerMode {
        self.visualizer
    }

    /// Returns the total count of LMS display frames rendered.
    #[must_use]
    pub const fn frames_rendered(&self) -> u32 {
        self.frames_rendered
    }

    /// Returns whether an `ANIC` acknowledgement is waiting to be sent.
    #[must_use]
    pub const fn is_anic_pending(&self) -> bool {
        self.anic_pending
    }

    /// Consumes a parsed [`LmsDisplayCommand`] and updates the framebuffer.
    pub fn handle_command(&mut self, cmd: LmsDisplayCommand<'_>) {
        match cmd {
            LmsDisplayCommand::Vfdc { payload } => {
                self.fb.clear();
                let mut idx = 0usize;
                while idx < payload.len() && (idx as u16) < self.fb.width() {
                    if payload[idx] != 0 && payload[idx] != b' ' {
                        self.fb.set_pixel(idx as u16, 0, true);
                    }
                    idx += 1;
                }
                self.frames_rendered = self.frames_rendered.wrapping_add(1);
            }
            LmsDisplayCommand::Grfe {
                offset,
                transition,
                param: _,
                bitmap,
            } => {
                self.fb.draw_lms_column_bitmap(offset, bitmap);
                if transition != b'c' && transition != 0 {
                    self.anic_pending = true;
                }
                self.frames_rendered = self.frames_rendered.wrapping_add(1);
            }
            LmsDisplayCommand::Grfb { brightness } => {
                let contrast = match brightness {
                    b if b <= 0 => 0x00,
                    1 => 0x20,
                    2 => 0x50,
                    3 => 0x90,
                    4 => 0xC8,
                    _ => 0xFF,
                };
                self.fb.set_contrast(contrast);
            }
            LmsDisplayCommand::Grfs => {
                self.size_report_pending = true;
            }
            LmsDisplayCommand::Grfg { x, width: _, data } => {
                self.fb.draw_lms_column_bitmap(x, data);
                self.frames_rendered = self.frames_rendered.wrapping_add(1);
            }
            LmsDisplayCommand::Grfa { flags, data } => {
                self.fb.draw_lms_column_bitmap(0, data);
                if (flags & 1) != 0 {
                    self.anic_pending = true;
                }
                self.frames_rendered = self.frames_rendered.wrapping_add(1);
            }
            LmsDisplayCommand::Visu { mode } => {
                self.visualizer = mode;
            }
        }
    }

    /// Pops and encodes a pending `ANIC` packet into `out`, if any.
    pub fn poll_anic_response(&mut self, out: &mut [u8]) -> Option<usize> {
        if !self.anic_pending {
            return None;
        }
        let n = build_anic_frame(0x01, out)?;
        self.anic_pending = false;
        Some(n)
    }

    /// Pops and encodes a pending `SETD 0xFE` display size report into `out`.
    pub fn poll_size_report(&mut self, out: &mut [u8]) -> Option<usize> {
        if !self.size_report_pending {
            return None;
        }
        let n = build_setd_display_size_frame(self.fb.width(), self.fb.height(), out)?;
        self.size_report_pending = false;
        Some(n)
    }
}

/// Horizontal text marquee scroller for long track titles on narrow displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextScroller {
    text_len: usize,
    viewport_chars: usize,
    offset: usize,
    pause_ticks_remaining: u8,
    pause_ticks_config: u8,
}

impl TextScroller {
    /// Creates a new [`TextScroller`] for `text_len` characters in a
    /// `viewport_chars` window, pausing `pause_ticks` at the start/end.
    #[must_use]
    pub const fn new(text_len: usize, viewport_chars: usize, pause_ticks: u8) -> Self {
        Self {
            text_len,
            viewport_chars,
            offset: 0,
            pause_ticks_remaining: pause_ticks,
            pause_ticks_config: pause_ticks,
        }
    }

    /// Resets the scroller for a new string of length `text_len`.
    pub fn reset(&mut self, text_len: usize) {
        self.text_len = text_len;
        self.offset = 0;
        self.pause_ticks_remaining = self.pause_ticks_config;
    }

    /// Advances the scroller by one tick and returns the current character
    /// start offset.
    pub fn tick(&mut self) -> usize {
        if self.text_len <= self.viewport_chars {
            self.offset = 0;
            return 0;
        }
        if self.pause_ticks_remaining > 0 {
            self.pause_ticks_remaining -= 1;
            return self.offset;
        }
        let max_offset = self.text_len - self.viewport_chars;
        if self.offset < max_offset {
            self.offset += 1;
            if self.offset == max_offset {
                self.pause_ticks_remaining = self.pause_ticks_config;
            }
        } else {
            self.offset = 0;
            self.pause_ticks_remaining = self.pause_ticks_config;
        }
        self.offset
    }

    /// Returns the visible character slice `[start..end]` from `text`.
    #[must_use]
    pub fn visible_slice<'a>(&self, text: &'a [u8]) -> &'a [u8] {
        let start = self.offset.min(text.len());
        let end = start.saturating_add(self.viewport_chars).min(text.len());
        &text[start..end]
    }

    /// Returns the current character scroll offset.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }
}

/// External audio source tag for the local metadata screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExternalSourceTag {
    /// Bluetooth A2DP sink.
    Bluetooth,
    /// Apple AirPlay (RAOP) receiver.
    AirPlay,
    /// Spotify Connect (cspot) receiver.
    Spotify,
}

impl ExternalSourceTag {
    /// Short display badge label for the source.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Bluetooth => "BT",
            Self::AirPlay => "AIRPLAY",
            Self::Spotify => "SPOTIFY",
        }
    }
}

/// Metadata displayer for Bluetooth, AirPlay, and Spotify Connect playback.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ExternalMetadataScreen {
    source: ExternalSourceTag,
    title: [u8; 64],
    title_len: usize,
    artist: [u8; 64],
    artist_len: usize,
    album: [u8; 64],
    album_len: usize,
    elapsed_ms: u32,
    duration_ms: u32,
    scroller: TextScroller,
}

impl ExternalMetadataScreen {
    /// Creates a new [`ExternalMetadataScreen`] for `source`.
    #[must_use]
    pub const fn new(source: ExternalSourceTag, viewport_chars: usize) -> Self {
        Self {
            source,
            title: [0u8; 64],
            title_len: 0,
            artist: [0u8; 64],
            artist_len: 0,
            album: [0u8; 64],
            album_len: 0,
            elapsed_ms: 0,
            duration_ms: 0,
            scroller: TextScroller::new(0, viewport_chars, 3),
        }
    }

    /// Updates track title, artist, and album strings.
    pub fn set_metadata(&mut self, title: &str, artist: &str, album: &str) {
        let t_bytes = title.as_bytes();
        self.title_len = t_bytes.len().min(self.title.len());
        self.title[..self.title_len].copy_from_slice(&t_bytes[..self.title_len]);
        self.scroller.reset(self.title_len);

        let ar_bytes = artist.as_bytes();
        self.artist_len = ar_bytes.len().min(self.artist.len());
        self.artist[..self.artist_len].copy_from_slice(&ar_bytes[..self.artist_len]);

        let al_bytes = album.as_bytes();
        self.album_len = al_bytes.len().min(self.album.len());
        self.album[..self.album_len].copy_from_slice(&al_bytes[..self.album_len]);
    }

    /// Updates track playback progress (`elapsed_ms`, `duration_ms`).
    pub fn set_progress(&mut self, elapsed_ms: u32, duration_ms: u32) {
        self.elapsed_ms = elapsed_ms;
        self.duration_ms = duration_ms;
    }

    /// Returns the active [`ExternalSourceTag`].
    #[must_use]
    pub const fn source(&self) -> ExternalSourceTag {
        self.source
    }

    /// Computes the progress bar fill width in pixels for `bar_width`.
    #[must_use]
    pub const fn progress_fill_px(&self, bar_width: u16) -> u16 {
        if self.duration_ms == 0 {
            return 0;
        }
        let clamped_elapsed = if self.elapsed_ms > self.duration_ms {
            self.duration_ms
        } else {
            self.elapsed_ms
        };
        let num = (clamped_elapsed as u64) * (bar_width as u64);
        (num / (self.duration_ms as u64)) as u16
    }

    /// Renders a stylized glyph bar + progress bar into `fb`.
    pub fn render(&mut self, fb: &mut GdsFramebuffer) {
        fb.clear();
        let w = fb.width();
        let h = fb.height();
        let _ = self.scroller.tick();
        let visible_title = self.scroller.visible_slice(&self.title[..self.title_len]);
        let mut idx = 0usize;
        while idx < visible_title.len() {
            let x = (idx as u16).saturating_mul(6);
            if x + 4 < w && visible_title[idx] != b' ' {
                fb.fill_rect(x, 2, 4, 6, true);
            }
            idx += 1;
        }
        if h >= 8 {
            let bar_y = h - 4;
            fb.fill_rect(0, bar_y, w, 1, true);
            fb.fill_rect(0, bar_y + 3, w, 1, true);
            let fill = self.progress_fill_px(w);
            if fill > 0 {
                fb.fill_rect(0, bar_y + 1, fill, 2, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_lookup_and_init_tables() {
        let all = [
            ("ssd1306", DisplayDriverKind::Ssd1306, DisplayColorMode::Monochrome1Bpp),
            ("SH1106", DisplayDriverKind::Sh1106, DisplayColorMode::Monochrome1Bpp),
            ("SH1122", DisplayDriverKind::Sh1122, DisplayColorMode::Grayscale4Bpp),
            ("SSD1322", DisplayDriverKind::Ssd1322, DisplayColorMode::Grayscale4Bpp),
            ("SSD1327", DisplayDriverKind::Ssd1327, DisplayColorMode::Grayscale4Bpp),
            ("SSD1351", DisplayDriverKind::Ssd1351, DisplayColorMode::Rgb565),
            ("SSD1675", DisplayDriverKind::Ssd1675, DisplayColorMode::Monochrome1Bpp),
            ("ST7735", DisplayDriverKind::St7735, DisplayColorMode::Rgb565),
            ("ST7789", DisplayDriverKind::St7789, DisplayColorMode::Rgb565),
            ("ILI9341", DisplayDriverKind::Ili9341, DisplayColorMode::Rgb565),
        ];
        for (name, expected, mode) in all {
            let parsed = DisplayDriverKind::from_name(name).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.color_mode(), mode);
            assert!(!parsed.init_commands().is_empty());
            let (w, h) = parsed.default_dimensions();
            assert!(w >= 128 && h >= 64);
        }
        assert!(DisplayDriverKind::from_name("UNKNOWN").is_none());
    }

    #[test]
    fn test_framebuffer_exports_mono_gray4_and_rgb565() {
        let mut fb = GdsFramebuffer::new(DisplayDriverKind::Ssd1306, 8, 8);
        fb.clear();
        fb.set_pixel(0, 0, true);
        fb.set_pixel(1, 0, true);
        assert!(fb.pixel(0, 0));
        assert!(!fb.pixel(2, 0));

        let mut pages = [0u8; 8];
        assert_eq!(fb.export_ssd1306_pages(&mut pages), 8);
        assert_eq!(pages[0] & 0x01, 0x01);
        assert_eq!(pages[1] & 0x01, 0x01);
        assert_eq!(pages[2], 0x00);

        let mut gray = [0u8; 32];
        assert_eq!(fb.export_gray4_packed(&mut gray), 32);
        assert_eq!(gray[0], 0xFF);
        assert_eq!(gray[1], 0x00);

        fb.set_palette_rgb565(0xF800, 0x001F);
        let mut rgb = [0u8; 8];
        assert_eq!(fb.export_rgb565_tile(0, 0, 2, 2, &mut rgb), 8);
        assert_eq!(&rgb[0..2], &0xF800u16.to_be_bytes());
        assert_eq!(&rgb[4..6], &0x001Fu16.to_be_bytes());
    }

    #[test]
    fn test_lms_protocol_handlers_and_anic_setd_builders() {
        let fb = GdsFramebuffer::new(DisplayDriverKind::Ssd1322, 128, 32);
        let mut engine = GdsDisplayEngine::new(fb);

        let mut setd_buf = [0u8; 16];
        let setd_len = engine.poll_size_report(&mut setd_buf).unwrap();
        assert_eq!(setd_len, 13);
        assert_eq!(&setd_buf[0..4], b"SETD");
        assert_eq!(setd_buf[8], 0xFE);
        assert_eq!(u16::from_be_bytes([setd_buf[9], setd_buf[10]]), 128);
        assert_eq!(u16::from_be_bytes([setd_buf[11], setd_buf[12]]), 32);

        let grfe_payload = [0x00, 0x00, b'l', 0x01, 0x80, 0x00, 0x00, 0x00];
        let cmd = LmsDisplayCommand::parse(b"grfe", &grfe_payload).unwrap();
        engine.handle_command(cmd);
        assert!(engine.framebuffer().pixel(0, 0));
        assert!(engine.is_anic_pending());

        let mut anic_buf = [0u8; 12];
        let anic_len = engine.poll_anic_response(&mut anic_buf).unwrap();
        assert_eq!(anic_len, 9);
        assert_eq!(&anic_buf[0..4], b"ANIC");
        assert!(!engine.is_anic_pending());

        let grfb_cmd = LmsDisplayCommand::parse(b"grfb", &[0x00, 0x04]).unwrap();
        engine.handle_command(grfb_cmd);
        assert_eq!(engine.framebuffer().contrast(), 0xC8);

        let visu_cmd = LmsDisplayCommand::parse(b"visu", &[2, 16]).unwrap();
        engine.handle_command(visu_cmd);
        assert_eq!(
            engine.visualizer(),
            VisualizerMode::Spectrum { bands: 16 }
        );
    }

    #[test]
    fn test_scroller_and_external_metadata_screen() {
        let mut screen = ExternalMetadataScreen::new(ExternalSourceTag::Spotify, 6);
        assert_eq!(screen.source().badge(), "SPOTIFY");
        screen.set_metadata("Dire Straits - Sultans", "Dire Straits", "Alchemy");
        screen.set_progress(60_000, 120_000);
        assert_eq!(screen.progress_fill_px(128), 64);

        let mut fb = GdsFramebuffer::new(DisplayDriverKind::Ssd1306, 128, 32);
        screen.render(&mut fb);
        assert!(fb.pixel(0, 30));
        assert!(fb.pixel(63, 29));
        assert!(!fb.pixel(80, 29));
    }

    #[test]
    fn test_display_bus_arbiter() {
        let mut arb = DisplayBusArbiter::new();
        assert!(arb.try_acquire(1));
        assert!(!arb.try_acquire(2));
        assert!(!arb.release(2));
        assert!(arb.release(1));
        assert_eq!(arb.transactions_completed(), 1);
    }
}
