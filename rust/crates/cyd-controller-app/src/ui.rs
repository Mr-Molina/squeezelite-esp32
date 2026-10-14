//! 320x240 Landscape Now Playing UI and Touch Hit-Box Router
//!
//! Provides widget caching, string formatting, rate-limited volume dispatch,
//! and touch screen hit-box routing for the Cheap Yellow Display.

use core::fmt::Write;
use heapless::{String, Vec};
use squeezelite_cyd_link::{CydCommand, LinkError, PlayState};

use crate::comms::{CydDisplayLink, CydTelemetryState};
use crate::drivers::{Ili9341Display, SpiDisplayBus};

pub const UI_COLOR_HEX_BG: u32 = 0x121212;
pub const UI_COLOR_HEX_SURFACE: u32 = 0x1E1E1E;
pub const UI_COLOR_HEX_SURFACE_LIGHT: u32 = 0x282828;
pub const UI_COLOR_HEX_ACCENT: u32 = 0x1DB954;
pub const UI_COLOR_HEX_TEXT_PRIMARY: u32 = 0xFFFFFF;
pub const UI_COLOR_HEX_TEXT_SECONDARY: u32 = 0xAAAAAA;
pub const UI_COLOR_HEX_OFFLINE: u32 = 0xE74C3C;
pub const UI_COLOR_HEX_ONLINE: u32 = 0x1DB954;

/// Convert a 24-bit 0xRRGGBB color into a 16-bit RGB565 pixel.
pub const fn rgb888_to_rgb565(hex: u32) -> u16 {
    let r = ((hex >> 16) & 0xFF) as u16;
    let g = ((hex >> 8) & 0xFF) as u16;
    let b = (hex & 0xFF) as u16;
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}

pub const UI_COLOR_BG_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_BG);
pub const UI_COLOR_SURFACE_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_SURFACE);
pub const UI_COLOR_SURFACE_LIGHT_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_SURFACE_LIGHT);
pub const UI_COLOR_ACCENT_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_ACCENT);
pub const UI_COLOR_TEXT_PRIMARY_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_TEXT_PRIMARY);
pub const UI_COLOR_TEXT_SECONDARY_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_TEXT_SECONDARY);
pub const UI_COLOR_OFFLINE_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_OFFLINE);
pub const UI_COLOR_ONLINE_565: u16 = rgb888_to_rgb565(UI_COLOR_HEX_ONLINE);

pub const DIRTY_STATUS: u8 = 1 << 0;
pub const DIRTY_TRACK: u8 = 1 << 1;
pub const DIRTY_PROGRESS: u8 = 1 << 2;
pub const DIRTY_TRANSPORT: u8 = 1 << 3;
pub const DIRTY_VOLUME: u8 = 1 << 4;
pub const DIRTY_ALL: u8 =
    DIRTY_STATUS | DIRTY_TRACK | DIRTY_PROGRESS | DIRTY_TRANSPORT | DIRTY_VOLUME;

/// Standard 8x8 printable ASCII bitmap font (`0x20` `' '` through `0x7E` `'~'`).
/// Each `u8` row has bit 7 (`0x80`) as the leftmost pixel (`gx = 0`).
pub const FONT_8X8: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 0x20 ' '
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // 0x21 '!'
    [0x66, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00], // 0x22 '"'
    [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00], // 0x23 '#'
    [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00], // 0x24 '$'
    [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00], // 0x25 '%'
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00], // 0x26 '&'
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00], // 0x27 '\''
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00], // 0x28 '('
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00], // 0x29 ')'
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // 0x2A '*'
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00], // 0x2B '+'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // 0x2C ','
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00], // 0x2D '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // 0x2E '.'
    [0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00], // 0x2F '/'
    [0x7C, 0xC6, 0xCE, 0xDE, 0xF6, 0xE6, 0x7C, 0x00], // 0x30 '0'
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // 0x31 '1'
    [0x7C, 0xC6, 0x06, 0x1C, 0x30, 0x66, 0xFE, 0x00], // 0x32 '2'
    [0x7C, 0xC6, 0x06, 0x3C, 0x06, 0xC6, 0x7C, 0x00], // 0x33 '3'
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00], // 0x34 '4'
    [0xFE, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00], // 0x35 '5'
    [0x38, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00], // 0x36 '6'
    [0xFE, 0xC6, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00], // 0x37 '7'
    [0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00], // 0x38 '8'
    [0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x78, 0x00], // 0x39 '9'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00], // 0x3A ':'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30], // 0x3B ';'
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00], // 0x3C '<'
    [0x00, 0x00, 0x7E, 0x00, 0x00, 0x7E, 0x00, 0x00], // 0x3D '='
    [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00], // 0x3E '>'
    [0x7C, 0xC6, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00], // 0x3F '?'
    [0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x78, 0x00], // 0x40 '@'
    [0x38, 0x6C, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00], // 0x41 'A'
    [0xFC, 0x66, 0x66, 0x7C, 0x66, 0x66, 0xFC, 0x00], // 0x42 'B'
    [0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0x66, 0x3C, 0x00], // 0x43 'C'
    [0xF8, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00], // 0x44 'D'
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x62, 0xFE, 0x00], // 0x45 'E'
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x60, 0xF0, 0x00], // 0x46 'F'
    [0x3C, 0x66, 0xC0, 0xC0, 0xCE, 0x66, 0x3E, 0x00], // 0x47 'G'
    [0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00], // 0x48 'H'
    [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 0x49 'I'
    [0x1E, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00], // 0x4A 'J'
    [0xE6, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0xE6, 0x00], // 0x4B 'K'
    [0xF0, 0x60, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00], // 0x4C 'L'
    [0xC6, 0xEE, 0xFE, 0xFE, 0xD6, 0xC6, 0xC6, 0x00], // 0x4D 'M'
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00], // 0x4E 'N'
    [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 0x4F 'O'
    [0xFC, 0x66, 0x66, 0x7C, 0x60, 0x60, 0xF0, 0x00], // 0x50 'P'
    [0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x0E], // 0x51 'Q'
    [0xFC, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0xE6, 0x00], // 0x52 'R'
    [0x7C, 0xC6, 0x60, 0x38, 0x0C, 0xC6, 0x7C, 0x00], // 0x53 'S'
    [0x7E, 0x5A, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 0x54 'T'
    [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 0x55 'U'
    [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00], // 0x56 'V'
    [0xC6, 0xC6, 0xD6, 0xFE, 0xFE, 0xEE, 0xC6, 0x00], // 0x57 'W'
    [0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00], // 0x58 'X'
    [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x3C, 0x00], // 0x59 'Y'
    [0xFE, 0xC6, 0x8C, 0x18, 0x32, 0x66, 0xFE, 0x00], // 0x5A 'Z'
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00], // 0x5B '['
    [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00], // 0x5C '\\'
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00], // 0x5D ']'
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00], // 0x5E '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // 0x5F '_'
    [0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00], // 0x60 '`'
    [0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0x76, 0x00], // 0x61 'a'
    [0xE0, 0x60, 0x7C, 0x66, 0x66, 0x66, 0xDC, 0x00], // 0x62 'b'
    [0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC6, 0x7C, 0x00], // 0x63 'c'
    [0x1C, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0x76, 0x00], // 0x64 'd'
    [0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0x7C, 0x00], // 0x65 'e'
    [0x3C, 0x66, 0x60, 0xF8, 0x60, 0x60, 0xF0, 0x00], // 0x66 'f'
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0xF8], // 0x67 'g'
    [0xE0, 0x60, 0x6C, 0x76, 0x66, 0x66, 0xE6, 0x00], // 0x68 'h'
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00], // 0x69 'i'
    [0x06, 0x00, 0x0E, 0x06, 0x06, 0x66, 0x66, 0x3C], // 0x6A 'j'
    [0xE0, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0xE6, 0x00], // 0x6B 'k'
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 0x6C 'l'
    [0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xC6, 0xC6, 0x00], // 0x6D 'm'
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x00], // 0x6E 'n'
    [0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 0x6F 'o'
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x7C, 0x60, 0xF0], // 0x70 'p'
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0x1E], // 0x71 'q'
    [0x00, 0x00, 0xDC, 0x76, 0x60, 0x60, 0xF0, 0x00], // 0x72 'r'
    [0x00, 0x00, 0x7E, 0xC0, 0x7C, 0x06, 0xFC, 0x00], // 0x73 's'
    [0x30, 0x30, 0xFC, 0x30, 0x30, 0x36, 0x1C, 0x00], // 0x74 't'
    [0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00], // 0x75 'u'
    [0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00], // 0x76 'v'
    [0x00, 0x00, 0xC6, 0xD6, 0xFE, 0xFE, 0x6C, 0x00], // 0x77 'w'
    [0x00, 0x00, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0x00], // 0x78 'x'
    [0x00, 0x00, 0xC6, 0xC6, 0xCE, 0x76, 0x06, 0x7C], // 0x79 'y'
    [0x00, 0x00, 0xFE, 0x8C, 0x18, 0x32, 0xFE, 0x00], // 0x7A 'z'
    [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00], // 0x7B '{'
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // 0x7C '|'
    [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00], // 0x7D '}'
    [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 0x7E '~'
];

/// Look up whether pixel `(gx, gy)` (`0..8`) is set for character `ch`.
/// Non-ASCII or non-printable characters outside `0x20..=0x7E` map to `'?'`.
pub const fn glyph_pixel(ch: char, gx: usize, gy: usize) -> bool {
    if gx >= 8 || gy >= 8 {
        return false;
    }
    let code = ch as u32;
    let idx = if code >= 0x20 && code <= 0x7E {
        (code - 0x20) as usize
    } else {
        (b'?' - 0x20) as usize
    };
    (FONT_8X8[idx][gy] & (0x80 >> gx)) != 0
}

/// Zero-heap scanline text box rasterizer.
///
/// Renders `text` into the rectangle `(x1..=x2, y1..=y2)` row-by-row using a
/// 320-pixel stack scanline buffer (`row_buf`) and flushes each scanline to `display`.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_box<B: SpiDisplayBus>(
    display: &mut Ili9341Display<B>,
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    text: &str,
    fg: u16,
    bg: u16,
    scale: i32,
    align_center: bool,
) {
    if x1 > x2 {
        core::mem::swap(&mut x1, &mut x2);
    }
    if y1 > y2 {
        core::mem::swap(&mut y1, &mut y2);
    }
    x1 = x1.max(0);
    y1 = y1.max(0);
    x2 = x2.min(319);
    y2 = y2.min(239);
    if x1 > x2 || y1 > y2 {
        return;
    }

    let scale = scale.max(1);
    let width = (x2 - x1 + 1) as usize;
    let height = y2 - y1 + 1;
    let glyph_w = 8 * scale;
    let glyph_h = 8 * scale;

    let mut chars = [' '; 40];
    let mut char_count = 0usize;
    for ch in text.chars() {
        if char_count >= 40 {
            break;
        }
        chars[char_count] = ch;
        char_count += 1;
    }

    let text_w = (char_count as i32) * glyph_w;
    let start_x = if align_center {
        (width as i32 - text_w) / 2
    } else {
        0
    };
    let text_y0 = y1 + ((height - glyph_h) / 2).max(0);

    for y in y1..=y2 {
        let mut row_buf = [bg; 320];
        if y >= text_y0 && y < text_y0 + glyph_h {
            let gy = ((y - text_y0) / scale) as usize;
            for (ch_idx, &ch) in chars[..char_count].iter().enumerate() {
                let ch_x0 = start_x + (ch_idx as i32) * glyph_w;
                for gx in 0..8 {
                    if glyph_pixel(ch, gx, gy) {
                        let px_start = ch_x0 + (gx as i32) * scale;
                        for sx in 0..scale {
                            let rx = px_start + sx;
                            if rx >= 0 && (rx as usize) < width {
                                row_buf[rx as usize] = fg;
                            }
                        }
                    }
                }
            }
        }
        display.flush(x1, y, x2, y, &row_buf[..width]);
    }
}

/// Transport button identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiButtonId {
    Prev,
    PlayPause,
    Next,
}

/// 320x240 Landscape Now Playing UI state, cache, and input handler.
#[derive(Debug, Clone)]
pub struct NowPlayingUi {
    pub mode_label: String<32>,
    pub ip_label: String<32>,
    pub link_badge: String<32>,
    pub title_label: String<64>,
    pub subtitle_label: String<128>,
    pub progress_percent: u8,
    pub time_elapsed_label: String<16>,
    pub time_total_label: String<16>,
    pub play_pause_icon: String<16>,
    pub volume_slider: u8,
    pub last_vol_sent_ms: u32,
    pub has_sent_first_vol: bool,
    pub dirty_flags: u8,
    pub cached_state: CydTelemetryState,
}

impl Default for NowPlayingUi {
    fn default() -> Self {
        let mut ui = Self {
            mode_label: String::new(),
            ip_label: String::new(),
            link_badge: String::new(),
            title_label: String::new(),
            subtitle_label: String::new(),
            progress_percent: 0,
            time_elapsed_label: String::new(),
            time_total_label: String::new(),
            play_pause_icon: String::new(),
            volume_slider: 50,
            last_vol_sent_ms: 0,
            has_sent_first_vol: false,
            dirty_flags: 0,
            cached_state: CydTelemetryState::default(),
        };
        let _ = ui.mode_label.push_str("[STANDBY]");
        let _ = ui.ip_label.push_str("---.---.---.---");
        let _ = ui.link_badge.push_str("OFFLINE");
        let _ = ui.title_label.push_str("No Track Playing");
        let _ = ui.subtitle_label.push_str("Ready to stream");
        let _ = ui.time_elapsed_label.push_str("00:00");
        let _ = ui.time_total_label.push_str("00:00");
        let _ = ui.play_pause_icon.push_str("PLAY");
        ui
    }
}

impl NowPlayingUi {
    /// Create a new UI instance with initial standby labels.
    pub fn new() -> Self {
        Self::default()
    }

    /// Access the internal cached telemetry state.
    pub fn cached_state(&self) -> &CydTelemetryState {
        &self.cached_state
    }

    /// Retrieve current mode pill text (e.g. `"[STANDBY]"`).
    pub fn mode_label(&self) -> &str {
        &self.mode_label
    }

    /// Retrieve current IP address label text.
    pub fn ip_label(&self) -> &str {
        &self.ip_label
    }

    /// Retrieve current link status badge text (`"ONLINE"` or `"OFFLINE"`).
    pub fn link_badge(&self) -> &str {
        &self.link_badge
    }

    /// Retrieve current track title text.
    pub fn title_label(&self) -> &str {
        &self.title_label
    }

    /// Retrieve current track subtitle text (`"Artist - Album"`).
    pub fn subtitle_label(&self) -> &str {
        &self.subtitle_label
    }

    /// Retrieve current progress percentage (0..=100).
    pub fn progress_percent(&self) -> u8 {
        self.progress_percent
    }

    /// Retrieve current elapsed time string (`"MM:SS"`).
    pub fn time_elapsed_label(&self) -> &str {
        &self.time_elapsed_label
    }

    /// Retrieve current total duration string (`"MM:SS"`).
    pub fn time_total_label(&self) -> &str {
        &self.time_total_label
    }

    /// Retrieve current symbol / text on Play/Pause button ("PLAY" or "PAUSE").
    pub fn play_pause_icon(&self) -> &str {
        &self.play_pause_icon
    }

    /// Retrieve current dirty region bitflags.
    pub fn dirty_flags(&self) -> u8 {
        self.dirty_flags
    }

    /// Clear all dirty region bitflags.
    pub fn clear_dirty(&mut self) {
        self.dirty_flags = 0;
    }

    /// Check whether the link is currently offline.
    pub fn is_offline(&self) -> bool {
        !self.cached_state.link_active
    }

    /// Format duration in seconds into `MM:SS` or `HH:MM:SS` (if >= 3600 seconds).
    pub fn format_time(seconds: u32) -> String<16> {
        let mut s = String::new();
        if seconds >= 3600 {
            let h = seconds / 3600;
            let m = (seconds % 3600) / 60;
            let sec = seconds % 60;
            let _ = write!(s, "{:02}:{:02}:{:02}", h, m, sec);
        } else {
            let m = seconds / 60;
            let sec = seconds % 60;
            let _ = write!(s, "{:02}:{:02}", m, sec);
        }
        s
    }

    /// Check if volume command should be throttled (10 Hz rate limit = 100ms window).
    pub fn should_throttle_volume(now_ms: u32, last_sent_ms: u32) -> bool {
        now_ms.wrapping_sub(last_sent_ms) < 100
    }

    /// Update UI widget labels, progress, and controls from received telemetry state,
    /// setting dirty region flags only for sections whose displayed values changed.
    pub fn update(&mut self, state: &CydTelemetryState) {
        self.cached_state = state.clone();

        // 1. Mode and IP
        let mut new_mode: String<32> = String::new();
        if state.mode.is_empty() {
            let _ = new_mode.push_str("[STANDBY]");
        } else {
            let _ = new_mode.push('[');
            let _ = new_mode.push_str(&state.mode);
            let _ = new_mode.push(']');
        }

        let mut new_ip: String<32> = String::new();
        if state.ip.is_empty() {
            let _ = new_ip.push_str("---.---.---.---");
        } else {
            let _ = new_ip.push_str(&state.ip);
        }

        // 2. Link Badge
        let mut new_badge: String<32> = String::new();
        if state.link_active {
            let _ = new_badge.push_str("ONLINE");
        } else {
            let _ = new_badge.push_str("OFFLINE");
        }

        if new_mode != self.mode_label || new_ip != self.ip_label || new_badge != self.link_badge {
            self.mode_label = new_mode;
            self.ip_label = new_ip;
            self.link_badge = new_badge;
            self.dirty_flags |= DIRTY_STATUS;
        }

        // 3. Track Title
        let mut new_title: String<64> = String::new();
        if state.title.is_empty() {
            let _ = new_title.push_str("No Track Playing");
        } else {
            let _ = new_title.push_str(&state.title);
        }

        // 4. Subtitle (Artist - Album, pure ASCII separator per UI-206)
        let mut new_subtitle: String<128> = String::new();
        if !state.artist.is_empty() && !state.album.is_empty() {
            let _ = new_subtitle.push_str(&state.artist);
            let _ = new_subtitle.push_str(" - ");
            let _ = new_subtitle.push_str(&state.album);
        } else if !state.artist.is_empty() {
            let _ = new_subtitle.push_str(&state.artist);
        } else if !state.album.is_empty() {
            let _ = new_subtitle.push_str(&state.album);
        } else {
            let _ = new_subtitle.push_str("Ready to stream");
        }

        // 5. Progress percent and time strings
        let new_progress = if state.duration_secs > 0 {
            ((state.elapsed_secs as u64 * 100) / (state.duration_secs as u64)).min(100) as u8
        } else {
            0
        };
        let new_elapsed = Self::format_time(state.elapsed_secs);
        let new_total = Self::format_time(state.duration_secs);

        if new_progress != self.progress_percent
            || new_elapsed != self.time_elapsed_label
            || new_total != self.time_total_label
        {
            self.progress_percent = new_progress;
            self.time_elapsed_label = new_elapsed;
            self.time_total_label = new_total;
            self.dirty_flags |= DIRTY_PROGRESS;
        }

        // 6. Play / Pause dynamic icon
        let mut new_icon: String<16> = String::new();
        if state.state == PlayState::Playing {
            let _ = new_icon.push_str("PAUSE");
        } else {
            let _ = new_icon.push_str("PLAY");
        }

        if new_title != self.title_label
            || new_subtitle != self.subtitle_label
            || new_icon != self.play_pause_icon
        {
            self.title_label = new_title;
            self.subtitle_label = new_subtitle;
            self.dirty_flags |= DIRTY_TRACK;
        }

        if new_icon != self.play_pause_icon {
            self.play_pause_icon = new_icon;
            self.dirty_flags |= DIRTY_TRANSPORT;
        }

        // 7. Volume slider position
        let new_vol = state.vol.min(100);
        if new_vol != self.volume_slider {
            self.volume_slider = new_vol;
            self.dirty_flags |= DIRTY_VOLUME;
        }
    }

    /// Dispatch a button click to the UART link with immediate optimistic visual feedback.
    pub fn click_button(
        &mut self,
        btn: UiButtonId,
        link: &mut CydDisplayLink,
    ) -> Result<Vec<u8, 64>, LinkError> {
        match btn {
            UiButtonId::Prev => {
                link.state_mut().elapsed_secs = 0;
                self.cached_state.elapsed_secs = 0;
                self.progress_percent = 0;
                self.time_elapsed_label.clear();
                let _ = self.time_elapsed_label.push_str("00:00");
                if !link.state().link_active {
                    self.subtitle_label.clear();
                    let _ = self.subtitle_label.push_str("Previous Track (<<)");
                }
                self.dirty_flags |= DIRTY_TRANSPORT | DIRTY_PROGRESS | DIRTY_TRACK;
                link.send_command(CydCommand::Previous)
            }
            UiButtonId::PlayPause => {
                let next_state = if link.state().state == PlayState::Playing {
                    PlayState::Paused
                } else {
                    PlayState::Playing
                };
                link.state_mut().state = next_state;
                self.cached_state.state = next_state;
                self.play_pause_icon.clear();
                let _ = self.play_pause_icon.push_str(if next_state == PlayState::Playing {
                    "PAUSE"
                } else {
                    "PLAY"
                });
                self.dirty_flags |= DIRTY_TRANSPORT | DIRTY_TRACK;
                link.send_command(CydCommand::Toggle)
            }
            UiButtonId::Next => {
                link.state_mut().elapsed_secs = 0;
                self.cached_state.elapsed_secs = 0;
                self.progress_percent = 0;
                self.time_elapsed_label.clear();
                let _ = self.time_elapsed_label.push_str("00:00");
                if !link.state().link_active {
                    self.subtitle_label.clear();
                    let _ = self.subtitle_label.push_str("Next Track (>>)");
                }
                self.dirty_flags |= DIRTY_TRANSPORT | DIRTY_PROGRESS | DIRTY_TRACK;
                link.send_command(CydCommand::Next)
            }
        }
    }

    /// Process a volume slider drag or release event with 10 Hz rate limiting.
    pub fn change_volume_slider(
        &mut self,
        vol: u8,
        released: bool,
        now_ms: u32,
        link: &mut CydDisplayLink,
    ) -> Option<Vec<u8, 64>> {
        let clamped = vol.min(100);
        if self.volume_slider != clamped {
            self.volume_slider = clamped;
            link.state_mut().vol = clamped;
            self.cached_state.vol = clamped;
            self.dirty_flags |= DIRTY_VOLUME;
        }

        if !released
            && self.has_sent_first_vol
            && Self::should_throttle_volume(now_ms, self.last_vol_sent_ms)
        {
            return None;
        }

        match link.send_command(CydCommand::SetVolume(clamped)) {
            Ok(bytes) => {
                self.last_vol_sent_ms = now_ms;
                self.has_sent_first_vol = true;
                Some(bytes)
            }
            Err(_) => None,
        }
    }

    /// Route a touch screen coordinate against UI hit-boxes.
    ///
    /// Supports both standard-Y (`182..=230`) and mirrored-Y (`12..=58`, mapped to `239 - y`)
    /// CYD touch glass panel revisions:
    /// - Prev Button: `x: 12..=56, y: 184..=228`
    /// - Play/Pause Button: `x: 64..=112, y: 182..=230`
    /// - Next Button: `x: 120..=164, y: 184..=228`
    /// - Volume Slider: `x: 195..=310, y: 190..=222`
    pub fn handle_touch_point(
        &mut self,
        x: i16,
        y: i16,
        released: bool,
        now_ms: u32,
        link: &mut CydDisplayLink,
    ) -> Option<Vec<u8, 64>> {
        let effective_y = if (12..=58).contains(&y) { 239 - y } else { y };
        if (12..=56).contains(&x) && (184..=228).contains(&effective_y) {
            if released {
                self.click_button(UiButtonId::Prev, link).ok()
            } else {
                None
            }
        } else if (64..=112).contains(&x) && (182..=230).contains(&effective_y) {
            if released {
                self.click_button(UiButtonId::PlayPause, link).ok()
            } else {
                None
            }
        } else if (120..=164).contains(&x) && (184..=228).contains(&effective_y) {
            if released {
                self.click_button(UiButtonId::Next, link).ok()
            } else {
                None
            }
        } else if (195..=310).contains(&x) && (190..=222).contains(&effective_y) {
            let vol = (((x - 195) as i32 * 100) / 115).clamp(0, 100) as u8;
            self.change_volume_slider(vol, released, now_ms, link)
        } else {
            None
        }
    }

    fn render_status_bar<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        let badge_color = if self.link_badge.as_str() == "ONLINE" {
            UI_COLOR_ONLINE_565
        } else {
            UI_COLOR_OFFLINE_565
        };

        display.fill_rect(0, 0, 319, 27, UI_COLOR_SURFACE_565);
        // Left (x: 6..94, y: 6..21): Mode pill
        draw_text_box(
            display,
            6,
            6,
            94,
            21,
            self.mode_label(),
            UI_COLOR_ACCENT_565,
            UI_COLOR_SURFACE_LIGHT_565,
            1,
            true,
        );
        // Center (x: 98..236, y: 6..21): IP label
        draw_text_box(
            display,
            98,
            6,
            236,
            21,
            self.ip_label(),
            UI_COLOR_TEXT_SECONDARY_565,
            UI_COLOR_SURFACE_565,
            1,
            true,
        );
        // Right (x: 242..314, y: 4..23): Link badge
        draw_text_box(
            display,
            242,
            4,
            314,
            23,
            self.link_badge(),
            UI_COLOR_TEXT_PRIMARY_565,
            badge_color,
            1,
            true,
        );
    }

    fn render_track_panel<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        display.fill_rect(8, 34, 311, 134, UI_COLOR_SURFACE_565);

        // Left Album Art Box (x: 12..88, y: 40..116): accent border + centered 2x "[PCM]" icon
        display.fill_rect(12, 40, 88, 116, UI_COLOR_ACCENT_565);
        draw_text_box(
            display,
            14,
            42,
            86,
            114,
            "[PCM]",
            UI_COLOR_ACCENT_565,
            UI_COLOR_SURFACE_LIGHT_565,
            2,
            true,
        );

        // Track Title (x: 98..312, y: 44..64): 2x if len <= 13, else 1x
        let title_scale = if self.title_label.len() <= 13 { 2 } else { 1 };
        draw_text_box(
            display,
            98,
            44,
            312,
            64,
            self.title_label(),
            UI_COLOR_TEXT_PRIMARY_565,
            UI_COLOR_SURFACE_565,
            title_scale,
            false,
        );

        // Subtitle (x: 98..312, y: 74..90): 1x scale
        draw_text_box(
            display,
            98,
            74,
            312,
            90,
            self.subtitle_label(),
            UI_COLOR_TEXT_SECONDARY_565,
            UI_COLOR_SURFACE_565,
            1,
            false,
        );

        // Audio Status Pill (x: 98..312, y: 98..112)
        let status_text = if self.play_pause_icon.as_str() == "PAUSE" {
            "STATUS: PLAYING"
        } else {
            "STATUS: STANDBY / PAUSED"
        };
        draw_text_box(
            display,
            98,
            98,
            312,
            112,
            status_text,
            UI_COLOR_ACCENT_565,
            UI_COLOR_SURFACE_LIGHT_565,
            1,
            false,
        );
    }

    fn render_progress_section<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        display.fill_rect(8, 138, 311, 172, UI_COLOR_SURFACE_565);

        // Progress Bar (x: 12..308, y: 142..150)
        display.fill_rect(12, 142, 308, 150, UI_COLOR_SURFACE_LIGHT_565);
        let total_w = 308 - 12 + 1; // 297
        let fill_w = ((self.progress_percent.min(100) as i32 * total_w) / 100).clamp(0, total_w);
        if fill_w > 0 {
            display.fill_rect(12, 142, 12 + fill_w - 1, 150, UI_COLOR_ACCENT_565);
        }

        // Elapsed Time (x: 12..80, y: 156..168)
        draw_text_box(
            display,
            12,
            156,
            80,
            168,
            self.time_elapsed_label(),
            UI_COLOR_TEXT_SECONDARY_565,
            UI_COLOR_SURFACE_565,
            1,
            false,
        );

        // Total Time (x: 248..308, y: 156..168)
        draw_text_box(
            display,
            248,
            156,
            308,
            168,
            self.time_total_label(),
            UI_COLOR_TEXT_SECONDARY_565,
            UI_COLOR_SURFACE_565,
            1,
            true,
        );
    }

    fn render_transport_buttons<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        // Prev Button (x: 12..56, y: 184..228)
        draw_text_box(
            display,
            12,
            184,
            56,
            228,
            "|<<",
            UI_COLOR_TEXT_PRIMARY_565,
            UI_COLOR_SURFACE_565,
            1,
            true,
        );

        // Play/Pause Button (x: 64..112, y: 182..230)
        draw_text_box(
            display,
            64,
            182,
            112,
            230,
            self.play_pause_icon(),
            UI_COLOR_BG_565,
            UI_COLOR_ACCENT_565,
            1,
            true,
        );

        // Next Button (x: 120..164, y: 184..228)
        draw_text_box(
            display,
            120,
            184,
            164,
            228,
            ">>|" ,
            UI_COLOR_TEXT_PRIMARY_565,
            UI_COLOR_SURFACE_565,
            1,
            true,
        );
    }

    fn render_volume_section<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        display.fill_rect(180, 182, 314, 230, UI_COLOR_SURFACE_565);

        // Volume readout (x: 195..310, y: 184..196): "VOL: XXX%"
        let mut vol_str: String<16> = String::new();
        let _ = write!(vol_str, "VOL: {:>3}%", self.volume_slider.min(100));
        draw_text_box(
            display,
            195,
            184,
            310,
            196,
            vol_str.as_str(),
            UI_COLOR_TEXT_PRIMARY_565,
            UI_COLOR_SURFACE_565,
            1,
            true,
        );

        // Slider track (x: 195..310, y: 204..212) with accent fill and white thumb knob (y: 198..218)
        display.fill_rect(195, 204, 310, 212, UI_COLOR_SURFACE_LIGHT_565);
        let track_w = 310 - 195 + 1; // 116
        let vol_w = ((self.volume_slider.min(100) as i32 * track_w) / 100).clamp(0, track_w);
        if vol_w > 0 {
            display.fill_rect(195, 204, 195 + vol_w - 1, 212, UI_COLOR_ACCENT_565);
        }
        let knob_x = (195 + ((self.volume_slider.min(100) as i32 * 115) / 100)).clamp(195, 310);
        display.fill_rect(
            (knob_x - 3).max(191),
            198,
            (knob_x + 3).min(313),
            218,
            UI_COLOR_TEXT_PRIMARY_565,
        );
    }

    /// Render UI layout frame to ILI9341 display with full bitmap font rasterization.
    pub fn render_frame<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        self.render_hardware_canvas(display);
    }

    /// Render the complete 320x240 Now Playing interface onto physical ILI9341 GRAM.
    pub fn render_hardware_canvas<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        display.fill_rect(0, 0, 319, 239, UI_COLOR_BG_565);
        self.render_status_bar(display);
        self.render_track_panel(display);
        self.render_progress_section(display);
        self.render_transport_buttons(display);
        self.render_volume_section(display);
    }

    /// Render only dirty UI regions to the display and clear `dirty_flags`.
    /// Returns `true` if any region was flushed, or `false` if no regions were dirty.
    pub fn render_dirty_regions<B: SpiDisplayBus>(
        &mut self,
        display: &mut Ili9341Display<B>,
    ) -> bool {
        if self.dirty_flags == 0 {
            return false;
        }

        if (self.dirty_flags & DIRTY_ALL) == DIRTY_ALL {
            self.render_hardware_canvas(display);
            self.dirty_flags = 0;
            return true;
        }

        if (self.dirty_flags & DIRTY_STATUS) != 0 {
            self.render_status_bar(display);
        }
        if (self.dirty_flags & DIRTY_TRACK) != 0 {
            self.render_track_panel(display);
        }
        if (self.dirty_flags & DIRTY_PROGRESS) != 0 {
            self.render_progress_section(display);
        }
        if (self.dirty_flags & DIRTY_TRANSPORT) != 0 {
            self.render_transport_buttons(display);
        }
        if (self.dirty_flags & DIRTY_VOLUME) != 0 {
            self.render_volume_section(display);
        }

        self.dirty_flags = 0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::MockSpiDisplayBus;

    #[test]
    fn test_ui_format_time_zero() {
        assert_eq!(NowPlayingUi::format_time(0).as_str(), "00:00");
    }

    #[test]
    fn test_ui_format_time_under_one_hour() {
        assert_eq!(NowPlayingUi::format_time(65).as_str(), "01:05");
        assert_eq!(NowPlayingUi::format_time(59).as_str(), "00:59");
        assert_eq!(NowPlayingUi::format_time(599).as_str(), "09:59");
        assert_eq!(NowPlayingUi::format_time(3599).as_str(), "59:59");
    }

    #[test]
    fn test_ui_format_time_one_hour_and_above() {
        assert_eq!(NowPlayingUi::format_time(3600).as_str(), "01:00:00");
        assert_eq!(NowPlayingUi::format_time(3661).as_str(), "01:01:01");
        assert_eq!(NowPlayingUi::format_time(7322).as_str(), "02:02:02");
    }

    #[test]
    fn test_ui_volume_throttling_within_window() {
        assert!(NowPlayingUi::should_throttle_volume(50, 0));
        assert!(NowPlayingUi::should_throttle_volume(99, 0));
        assert!(NowPlayingUi::should_throttle_volume(1050, 1000));
        assert!(NowPlayingUi::should_throttle_volume(1099, 1000));
    }

    #[test]
    fn test_ui_volume_throttling_after_window() {
        assert!(!NowPlayingUi::should_throttle_volume(100, 0));
        assert!(!NowPlayingUi::should_throttle_volume(150, 0));
        assert!(!NowPlayingUi::should_throttle_volume(1100, 1000));
        assert!(!NowPlayingUi::should_throttle_volume(1250, 1000));
    }

    #[test]
    fn test_ui_volume_throttling_timer_wraparound() {
        // 32-bit unsigned wraparound: 0xFFFFFF9C to 0x00000010 is 116 ms -> false
        let last_sent = 0xFFFFFF9Cu32;
        let now = 0x00000010u32;
        assert!(!NowPlayingUi::should_throttle_volume(now, last_sent));

        // Wraparound within 34 ms -> true
        let last_sent = 0xFFFFFFFEu32;
        let now = 0x00000020u32;
        assert!(NowPlayingUi::should_throttle_volume(now, last_sent));
    }

    #[test]
    fn test_ui_state_update_active_playback() {
        let mut ui = NowPlayingUi::new();
        let mut state = CydTelemetryState::default();

        let _ = state.title.push_str("Bohemian Rhapsody");
        let _ = state.artist.push_str("Queen");
        let _ = state.album.push_str("A Night at the Opera");
        state.state = PlayState::Playing;
        let _ = state.mode.push_str("SPOTIFY");
        let _ = state.ip.push_str("192.168.1.42");
        state.elapsed_secs = 120;
        state.duration_secs = 360;
        state.vol = 65;
        state.link_active = true;

        ui.update(&state);

        assert_eq!(ui.cached_state().title.as_str(), "Bohemian Rhapsody");
        assert_eq!(ui.cached_state().artist.as_str(), "Queen");
        assert_eq!(ui.cached_state().album.as_str(), "A Night at the Opera");
        assert_eq!(ui.mode_label.as_str(), "[SPOTIFY]");
        assert_eq!(ui.ip_label.as_str(), "192.168.1.42");
        assert_eq!(ui.title_label.as_str(), "Bohemian Rhapsody");
        assert_eq!(ui.subtitle_label.as_str(), "Queen - A Night at the Opera");
        assert_eq!(ui.progress_percent(), 33);
        assert_eq!(ui.play_pause_icon(), "PAUSE");
        assert!(!ui.is_offline());
        assert_eq!(ui.dirty_flags(), DIRTY_ALL);
    }

    #[test]
    fn test_ui_state_update_paused_and_offline() {
        let mut ui = NowPlayingUi::new();
        let mut state = CydTelemetryState::default();

        let _ = state.title.push_str("Track 1");
        let _ = state.artist.push_str("Artist 1");
        state.state = PlayState::Paused;
        let _ = state.mode.push_str("LMS");
        let _ = state.ip.push_str("192.168.1.50");
        state.elapsed_secs = 0;
        state.duration_secs = 200;
        state.vol = 50;
        state.link_active = false;

        ui.update(&state);

        assert!(!ui.cached_state().link_active);
        assert!(ui.is_offline());
        assert_eq!(ui.play_pause_icon(), "PLAY");
        assert_eq!(ui.progress_percent(), 0);
    }

    #[test]
    fn test_ui_transport_button_actions() {
        let mut ui = NowPlayingUi::new();
        let mut link = CydDisplayLink::new();

        let prev_cmd = ui.click_button(UiButtonId::Prev, &mut link).unwrap();
        assert_eq!(&prev_cmd[..], b"CMD=PREV\n");

        let toggle_cmd = ui.click_button(UiButtonId::PlayPause, &mut link).unwrap();
        assert_eq!(&toggle_cmd[..], b"CMD=TOGGLE\n");

        let next_cmd = ui.click_button(UiButtonId::Next, &mut link).unwrap();
        assert_eq!(&next_cmd[..], b"CMD=NEXT\n");
    }

    #[test]
    fn test_ui_volume_slider_throttled_dispatch() {
        let mut ui = NowPlayingUi::new();
        let mut link = CydDisplayLink::new();

        // First change at t=10ms should send
        let cmd1 = ui.change_volume_slider(40, false, 10, &mut link);
        assert!(cmd1.is_some());
        assert_eq!(&cmd1.unwrap()[..], b"CMD=VOL:40\n");

        // Rapid second change at t=50ms (within 100ms) should be throttled
        let cmd2 = ui.change_volume_slider(45, false, 50, &mut link);
        assert!(cmd2.is_none());

        // Third change at t=120ms (>= 100ms from t=10ms) should send
        let cmd3 = ui.change_volume_slider(50, false, 120, &mut link);
        assert!(cmd3.is_some());
        assert_eq!(&cmd3.unwrap()[..], b"CMD=VOL:50\n");

        // Released at t=130ms should always flush
        let cmd4 = ui.change_volume_slider(50, true, 130, &mut link);
        assert!(cmd4.is_some());
    }

    #[test]
    fn test_ui_touch_point_hit_boxes() {
        let mut ui = NowPlayingUi::new();
        let mut link = CydDisplayLink::new();

        // Prev Button hit-box: x=30, y=200
        assert!(ui.handle_touch_point(30, 200, false, 0, &mut link).is_none());
        let res = ui.handle_touch_point(30, 200, true, 0, &mut link);
        assert_eq!(res.as_deref(), Some(&b"CMD=PREV\n"[..]));

        // Play/Pause Button hit-box: x=80, y=200
        let res = ui.handle_touch_point(80, 200, true, 0, &mut link);
        assert_eq!(res.as_deref(), Some(&b"CMD=TOGGLE\n"[..]));

        // Next Button hit-box: x=140, y=200
        let res = ui.handle_touch_point(140, 200, true, 0, &mut link);
        assert_eq!(res.as_deref(), Some(&b"CMD=NEXT\n"[..]));

        // Volume Slider hit-box: x=250, y=200
        let res = ui.handle_touch_point(250, 200, false, 10, &mut link);
        assert!(res.is_some());
    }

    #[test]
    fn test_ui_font_8x8_glyphs_and_fallback() {
        // Space (' ') has 0 active pixels
        let mut space_lit = 0;
        for gy in 0..8 {
            for gx in 0..8 {
                if glyph_pixel(' ', gx, gy) {
                    space_lit += 1;
                }
            }
        }
        assert_eq!(space_lit, 0);

        // Verify digits '0'..='9', uppercase 'A'..='Z', lowercase 'a'..='z', and UI symbols have non-blank glyphs
        for ch in ('0'..='9')
            .chain('A'..='Z')
            .chain('a'..='z')
            .chain(['[', ']', ':', '.', '/', '-', '%', '|', '<', '>', '?'])
        {
            let mut lit = 0;
            for gy in 0..8 {
                for gx in 0..8 {
                    if glyph_pixel(ch, gx, gy) {
                        lit += 1;
                    }
                }
            }
            assert!(lit > 0, "Glyph '{}' must have active pixels", ch);
        }

        // Verify non-ASCII char ('•') maps to '?'
        for gy in 0..8 {
            for gx in 0..8 {
                assert_eq!(glyph_pixel('•', gx, gy), glyph_pixel('?', gx, gy));
            }
        }
    }

    #[test]
    fn test_ui_render_frame() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        display.init();

        let ui = NowPlayingUi::new();
        let count_before = display.flush_count();
        ui.render_frame(&mut display);

        // Render frame flushes background rects and scanline text boxes
        assert!(display.flush_count() > count_before + 20);
    }

    #[test]
    fn test_ui_dirty_regions_partial_volume_update() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        display.init();

        let mut ui = NowPlayingUi::new();
        let mut link = CydDisplayLink::new();

        // No dirty flags initially -> render_dirty_regions returns false
        assert!(!ui.render_dirty_regions(&mut display));

        // Dragging volume slider marks ONLY DIRTY_VOLUME
        let _ = ui.change_volume_slider(80, false, 100, &mut link);
        assert_eq!(ui.dirty_flags(), DIRTY_VOLUME);

        let bytes_before = display.bus().total_data_bytes;
        assert!(ui.render_dirty_regions(&mut display));
        assert_eq!(ui.dirty_flags(), 0);
        let volume_update_bytes = display.bus().total_data_bytes - bytes_before;

        // Volume region partial update should transmit far fewer bytes than a 320x240 full-screen clear (153,600 bytes)
        assert!(volume_update_bytes > 0);
        assert!(volume_update_bytes < 30_000);
    }

    #[test]
    fn test_ui_optimistic_button_feedback_volume_sync_and_mirrored_y_touch() {
        let mut ui = NowPlayingUi::new();
        let mut link = CydDisplayLink::new();

        // Bypass 2A: PlayPause toggles state and icon immediately and sets DIRTY_TRANSPORT | DIRTY_TRACK
        assert_eq!(ui.play_pause_icon(), "PLAY");
        let _ = ui.click_button(UiButtonId::PlayPause, &mut link).unwrap();
        assert_eq!(link.state().state, PlayState::Playing);
        assert_eq!(ui.play_pause_icon(), "PAUSE");
        assert_eq!(ui.dirty_flags() & (DIRTY_TRANSPORT | DIRTY_TRACK), DIRTY_TRANSPORT | DIRTY_TRACK);
        ui.clear_dirty();
        assert_eq!(ui.dirty_flags(), 0);

        // Prev / Next reset elapsed time and update standalone subtitle
        link.state_mut().elapsed_secs = 90;
        let _ = ui.click_button(UiButtonId::Prev, &mut link).unwrap();
        assert_eq!(link.state().elapsed_secs, 0);
        assert_eq!(ui.time_elapsed_label(), "00:00");
        assert_eq!(ui.subtitle_label(), "Previous Track (<<)");

        let _ = ui.click_button(UiButtonId::Next, &mut link).unwrap();
        assert_eq!(ui.subtitle_label(), "Next Track (>>)");

        // Bypass 2B: Volume slider updates link.state().vol so subsequent ui.update(link.state()) preserves slider
        let _ = ui.change_volume_slider(77, true, 500, &mut link);
        assert_eq!(link.state().vol, 77);
        ui.clear_dirty();
        ui.update(link.state());
        assert_eq!(ui.volume_slider, 77);

        // Bypass 3: Mirrored-Y touch coordinate (y = 39 -> effective_y = 200) routes to transport buttons & volume
        let res = ui.handle_touch_point(80, 39, true, 600, &mut link);
        assert_eq!(res.as_deref(), Some(&b"CMD=TOGGLE\n"[..]));
    }
}

