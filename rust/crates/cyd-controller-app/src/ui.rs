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

    /// Retrieve current progress percentage (0..=100).
    pub fn progress_percent(&self) -> u8 {
        self.progress_percent
    }

    /// Retrieve current symbol / text on Play/Pause button ("PLAY" or "PAUSE").
    pub fn play_pause_icon(&self) -> &str {
        &self.play_pause_icon
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

    /// Update UI widget labels, progress, and controls from received telemetry state.
    pub fn update(&mut self, state: &CydTelemetryState) {
        self.cached_state = state.clone();

        // 1. Mode and IP
        self.mode_label.clear();
        if state.mode.is_empty() {
            let _ = self.mode_label.push_str("[STANDBY]");
        } else {
            let _ = self.mode_label.push('[');
            let _ = self.mode_label.push_str(&state.mode);
            let _ = self.mode_label.push(']');
        }

        self.ip_label.clear();
        if state.ip.is_empty() {
            let _ = self.ip_label.push_str("---.---.---.---");
        } else {
            let _ = self.ip_label.push_str(&state.ip);
        }

        // 2. Link Badge
        self.link_badge.clear();
        if state.link_active {
            let _ = self.link_badge.push_str("ONLINE");
        } else {
            let _ = self.link_badge.push_str("OFFLINE");
        }

        // 3. Track Title
        self.title_label.clear();
        if state.title.is_empty() {
            let _ = self.title_label.push_str("No Track Playing");
        } else {
            let _ = self.title_label.push_str(&state.title);
        }

        // 4. Subtitle (Artist • Album)
        self.subtitle_label.clear();
        if !state.artist.is_empty() && !state.album.is_empty() {
            let _ = self.subtitle_label.push_str(&state.artist);
            let _ = self.subtitle_label.push_str(" • ");
            let _ = self.subtitle_label.push_str(&state.album);
        } else if !state.artist.is_empty() {
            let _ = self.subtitle_label.push_str(&state.artist);
        } else if !state.album.is_empty() {
            let _ = self.subtitle_label.push_str(&state.album);
        } else {
            let _ = self.subtitle_label.push_str("Ready to stream");
        }

        // 5. Progress percent and time strings
        if state.duration_secs > 0 {
            let pct =
                ((state.elapsed_secs as u64 * 100) / (state.duration_secs as u64)).min(100) as u8;
            self.progress_percent = pct;
        } else {
            self.progress_percent = 0;
        }

        self.time_elapsed_label = Self::format_time(state.elapsed_secs);
        self.time_total_label = Self::format_time(state.duration_secs);

        // 6. Play / Pause dynamic icon
        self.play_pause_icon.clear();
        if state.state == PlayState::Playing {
            let _ = self.play_pause_icon.push_str("PAUSE");
        } else {
            let _ = self.play_pause_icon.push_str("PLAY");
        }

        // 7. Volume slider position
        self.volume_slider = state.vol.min(100);
    }

    /// Dispatch a button click to the UART link.
    pub fn click_button(
        &mut self,
        btn: UiButtonId,
        link: &mut CydDisplayLink,
    ) -> Result<Vec<u8, 64>, LinkError> {
        match btn {
            UiButtonId::Prev => link.send_command(CydCommand::Previous),
            UiButtonId::PlayPause => link.send_command(CydCommand::Toggle),
            UiButtonId::Next => link.send_command(CydCommand::Next),
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
        self.volume_slider = clamped;

        if released {
            self.last_vol_sent_ms = now_ms;
            self.has_sent_first_vol = true;
            link.send_command(CydCommand::SetVolume(clamped)).ok()
        } else if self.has_sent_first_vol
            && Self::should_throttle_volume(now_ms, self.last_vol_sent_ms)
        {
            None
        } else {
            self.last_vol_sent_ms = now_ms;
            self.has_sent_first_vol = true;
            link.send_command(CydCommand::SetVolume(clamped)).ok()
        }
    }

    /// Route a touch screen coordinate against UI hit-boxes.
    ///
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
        if (12..=56).contains(&x) && (184..=228).contains(&y) {
            if released {
                self.click_button(UiButtonId::Prev, link).ok()
            } else {
                None
            }
        } else if (64..=112).contains(&x) && (182..=230).contains(&y) {
            if released {
                self.click_button(UiButtonId::PlayPause, link).ok()
            } else {
                None
            }
        } else if (120..=164).contains(&x) && (184..=228).contains(&y) {
            if released {
                self.click_button(UiButtonId::Next, link).ok()
            } else {
                None
            }
        } else if (195..=310).contains(&x) && (190..=222).contains(&y) {
            let vol = (((x - 195) as i32 * 100) / 115).clamp(0, 100) as u8;
            self.change_volume_slider(vol, released, now_ms, link)
        } else {
            None
        }
    }

    /// Render UI layout frame to ILI9341 display.
    pub fn render_frame<B: SpiDisplayBus>(&self, display: &mut Ili9341Display<B>) {
        let bg_color = rgb888_to_rgb565(UI_COLOR_HEX_BG);
        let surface_color = rgb888_to_rgb565(UI_COLOR_HEX_SURFACE);
        let accent_color = rgb888_to_rgb565(UI_COLOR_HEX_ACCENT);
        let surface_light = rgb888_to_rgb565(UI_COLOR_HEX_SURFACE_LIGHT);

        let mut buf = [0u16; 320];

        // 1. Top Status Bar: x = 0..=319, y = 0..=27
        buf.fill(surface_color);
        display.flush(0, 0, 319, 27, &buf);

        // 2. Center Panel: x = 8..=311, y = 34..=167
        buf[..304].fill(surface_color);
        display.flush(8, 34, 311, 167, &buf[..304]);

        // 3. Progress Bar: x = 20..=299, y = 114..=121
        let fill_width = ((self.progress_percent as usize * 280) / 100).min(280);
        buf[..fill_width].fill(accent_color);
        buf[fill_width..280].fill(surface_light);
        display.flush(20, 114, 299, 121, &buf[..280]);

        // 4. Bottom Transport Bar: x = 0..=319, y = 174..=237
        buf.fill(bg_color);
        display.flush(0, 174, 319, 237, &buf);
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
        assert_eq!(ui.subtitle_label.as_str(), "Queen • A Night at the Opera");
        assert_eq!(ui.progress_percent(), 33);
        assert_eq!(ui.play_pause_icon(), "PAUSE");
        assert!(!ui.is_offline());
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
    fn test_ui_render_frame() {
        let bus = MockSpiDisplayBus::default();
        let mut display = Ili9341Display::new(bus);
        display.init();

        let ui = NowPlayingUi::new();
        let count_before = display.flush_count();
        ui.render_frame(&mut display);

        // Render frame flushes 4 UI regions: status bar, center panel, progress bar, transport bar
        assert_eq!(display.flush_count(), count_before + 4);
    }
}
