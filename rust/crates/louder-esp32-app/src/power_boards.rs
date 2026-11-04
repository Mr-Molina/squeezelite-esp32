//! Power / Sleep Management & Multi-Board Hardware Profiles (Phase 7).
//!
//! Provides pure-Rust, `no_std`-compatible power management (`SleepManager`
//! with inactivity timeouts, GPIO wake pin masks, low-battery cutoffs,
//! spurious-wake filtering, SlimProto `aude` amplifier power GPIO control, and
//! `deep_sleep`/`light_sleep` console command parsing) alongside hardware
//! board profiles (`LouderEsp32`, `LouderEsp32S3`, `SqueezeAmp`, `MuseProto`,
//! `TWatch2020`, `GenericI2s`).
#![forbid(unsafe_code)]

/// System power state managed by [`SleepManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SleepState {
    /// Normal active operation.
    Active,
    /// ESP32 light sleep (RAM preserved, fast wake on GPIO/timer).
    LightSleep,
    /// ESP32 deep sleep (RTC wake only via EXT0/EXT1 GPIO mask or timer).
    DeepSleep,
}

/// Configuration and runtime state for [`SleepManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SleepManager {
    state: SleepState,
    inactivity_timeout_ms: u32,
    spurious_wake_window_ms: u32,
    low_battery_cutoff_mv: u16,
    wake_pin_mask: u64,
    power_gpio: Option<u8>,
    aude_spdif_enabled: bool,
    aude_dac_enabled: bool,
    last_activity_ms: u32,
    last_wake_ms: u32,
    spurious_wake_count: u8,
    pre_sleep_soft_mute: bool,
}

impl SleepManager {
    /// Creates a new [`SleepManager`] in [`SleepState::Active`].
    #[must_use]
    pub const fn new(
        inactivity_timeout_ms: u32,
        spurious_wake_window_ms: u32,
        low_battery_cutoff_mv: u16,
        wake_pin_mask: u64,
        power_gpio: Option<u8>,
    ) -> Self {
        Self {
            state: SleepState::Active,
            inactivity_timeout_ms,
            spurious_wake_window_ms,
            low_battery_cutoff_mv,
            wake_pin_mask,
            power_gpio,
            aude_spdif_enabled: true,
            aude_dac_enabled: true,
            last_activity_ms: 0,
            last_wake_ms: 0,
            spurious_wake_count: 0,
            pre_sleep_soft_mute: false,
        }
    }

    /// Returns the current [`SleepState`].
    #[must_use]
    pub const fn state(&self) -> SleepState {
        self.state
    }

    /// Returns the RTC EXT1 GPIO wake pin bitmask (`1 << gpio`).
    #[must_use]
    pub const fn wake_pin_mask(&self) -> u64 {
        self.wake_pin_mask
    }

    /// Adds a GPIO pin (`0..=63`) to the RTC wake bitmask.
    pub fn add_wake_gpio(&mut self, gpio: u8) {
        if gpio < 64 {
            self.wake_pin_mask |= 1u64 << gpio;
        }
    }

    /// Returns the optional amplifier power rail GPIO controlled by `aude`.
    #[must_use]
    pub const fn power_gpio(&self) -> Option<u8> {
        self.power_gpio
    }

    /// Returns whether the external amplifier power rail should be energized
    /// (`true` when `Active` and `aude_dac_enabled` is set).
    #[must_use]
    pub const fn is_amp_power_enabled(&self) -> bool {
        matches!(self.state, SleepState::Active) && self.aude_dac_enabled
    }

    /// Returns whether the amplifier power rail should be energized.
    #[must_use]
    pub const fn should_enable_amp_power(&self) -> bool {
        self.is_amp_power_enabled()
    }

    /// Returns whether soft-mute is requested prior to entering deep sleep
    /// or power cutoff, preventing speaker pop.
    #[must_use]
    pub const fn soft_mute_requested(&self) -> bool {
        self.pre_sleep_soft_mute
            || matches!(self.state, SleepState::DeepSleep | SleepState::LightSleep)
    }

    /// Sets or clears the pre-sleep soft-mute hold flag.
    pub fn set_pre_sleep_soft_mute(&mut self, soft_mute: bool) {
        self.pre_sleep_soft_mute = soft_mute;
    }

    /// Returns the count of filtered spurious wakes.
    #[must_use]
    pub const fn spurious_wake_count(&self) -> u8 {
        self.spurious_wake_count
    }

    /// Processes a SlimProto `aude` packet (`[spdif_enable, dac_enable]`).
    pub fn handle_aude_packet(&mut self, payload: &[u8]) {
        if let Some(&spdif) = payload.first() {
            self.aude_spdif_enabled = spdif != 0;
        }
        if let Some(&dac) = payload.get(1) {
            self.aude_dac_enabled = dac != 0;
        }
    }

    /// Records user or playback activity at `now_ms`.
    pub fn record_activity(&mut self, now_ms: u32) {
        self.last_activity_ms = now_ms;
        self.state = SleepState::Active;
        self.pre_sleep_soft_mute = false;
    }

    /// Evaluates inactivity and battery thresholds at `now_ms`, transitioning
    /// into [`SleepState::LightSleep`] or [`SleepState::DeepSleep`] when idle.
    pub fn evaluate(
        &mut self,
        now_ms: u32,
        is_playing: bool,
        battery_mv: Option<u16>,
    ) -> SleepState {
        if let Some(mv) = battery_mv {
            if self.low_battery_cutoff_mv > 0 && mv > 0 && mv <= self.low_battery_cutoff_mv {
                self.pre_sleep_soft_mute = true;
                self.state = SleepState::DeepSleep;
                return self.state;
            }
        }
        if is_playing {
            self.last_activity_ms = now_ms;
            self.state = SleepState::Active;
            self.pre_sleep_soft_mute = false;
            return self.state;
        }
        if self.inactivity_timeout_ms > 0 {
            let idle_ms = now_ms.wrapping_sub(self.last_activity_ms);
            if idle_ms >= self.inactivity_timeout_ms.saturating_mul(2) {
                self.pre_sleep_soft_mute = true;
                self.state = SleepState::DeepSleep;
            } else if idle_ms >= self.inactivity_timeout_ms {
                self.state = SleepState::LightSleep;
            }
        }
        self.state
    }

    /// Handles an RTC wake event at `now_ms` and applies the spurious-wake
    /// filter: if no real activity occurs within `spurious_wake_window_ms`,
    /// returns the unit directly to [`SleepState::DeepSleep`].
    pub fn evaluate_wake_window(
        &mut self,
        wake_ms: u32,
        now_ms: u32,
        activity_detected: bool,
    ) -> SleepState {
        self.last_wake_ms = wake_ms;
        if activity_detected {
            self.last_activity_ms = now_ms;
            self.state = SleepState::Active;
            return self.state;
        }
        if now_ms.wrapping_sub(wake_ms) >= self.spurious_wake_window_ms {
            self.spurious_wake_count = self.spurious_wake_count.saturating_add(1);
            self.state = SleepState::DeepSleep;
        } else {
            self.state = SleepState::LightSleep;
        }
        self.state
    }

    /// Parses a console sleep command (`"deep_sleep [sec]"` or
    /// `"light_sleep [sec]"`), returning `(SleepState, Option<duration_sec>)`.
    #[must_use]
    pub fn parse_console_command(line: &str) -> Option<(SleepState, Option<u32>)> {
        let mut parts = line.split_whitespace();
        let cmd = parts.next()?;
        let state = if cmd.eq_ignore_ascii_case("deep_sleep") {
            SleepState::DeepSleep
        } else if cmd.eq_ignore_ascii_case("light_sleep") {
            SleepState::LightSleep
        } else {
            return None;
        };
        let duration_sec = parts.next().and_then(parse_u32);
        Some((state, duration_sec))
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.trim().as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// Supported hardware board targets (Phase 7 multi-board parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoardProfile {
    /// Sonocotta Louder-ESP32 (ESP32-WROVER-N8R8 + TAS5805M/TAS5825M).
    LouderEsp32,
    /// Sonocotta Louder-ESP32-Plus (ESP32-S3-WROOM-N8R8 + TAS5805M/TAS5825M).
    LouderEsp32S3,
    /// SqueezeAMP (ESP32-WROVER + TAS5713 + battery/jack sense).
    SqueezeAmp,
    /// RASPIAUDIO ESP-MUSE Proto / Luxe (ESP32-WROVER + WM8978 / ES8388).
    MuseProto,
    /// LilyGO T-WATCH-2020 (ESP32 + MAX98357A I2S + ST7789 240x240 TFT).
    TWatch2020,
    /// Generic ESP32 3-wire I2S DAC target (PCM5102A / UDA1334A).
    GenericI2s,
}

impl BoardProfile {
    /// Resolves a board profile by target name or C `target` NVS identifier.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let t = name.trim();
        if t.eq_ignore_ascii_case("LOUDER-ESP32")
            || t.eq_ignore_ascii_case("LOUDER_ESP32")
        {
            Some(Self::LouderEsp32)
        } else if t.eq_ignore_ascii_case("LOUDER-ESP32-S3")
            || t.eq_ignore_ascii_case("LOUDER_ESP32_S3")
            || t.eq_ignore_ascii_case("LOUDER-ESP32-PLUS")
        {
            Some(Self::LouderEsp32S3)
        } else if t.eq_ignore_ascii_case("SQUEEZEAMP") {
            Some(Self::SqueezeAmp)
        } else if t.eq_ignore_ascii_case("MUSE") || t.eq_ignore_ascii_case("ESP-MUSE") {
            Some(Self::MuseProto)
        } else if t.eq_ignore_ascii_case("TWATCH2020")
            || t.eq_ignore_ascii_case("T-WATCH2020")
        {
            Some(Self::TWatch2020)
        } else if t.eq_ignore_ascii_case("GENERIC") || t.eq_ignore_ascii_case("GENERIC-I2S") {
            Some(Self::GenericI2s)
        } else {
            None
        }
    }

    /// Canonical board target identifier.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LouderEsp32 => "LOUDER-ESP32",
            Self::LouderEsp32S3 => "LOUDER-ESP32-S3",
            Self::SqueezeAmp => "SQUEEZEAMP",
            Self::MuseProto => "MUSE",
            Self::TWatch2020 => "TWATCH2020",
            Self::GenericI2s => "GENERIC-I2S",
        }
    }

    /// Returns whether this board uses an ESP32-S3 MCU.
    #[must_use]
    pub const fn is_esp32_s3(self) -> bool {
        matches!(self, Self::LouderEsp32S3)
    }

    /// Default `dac_config` string for this board target.
    #[must_use]
    pub const fn default_dac_config(self) -> &'static str {
        match self {
            Self::LouderEsp32 => {
                "model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=33:0,i2c=45"
            }
            Self::LouderEsp32S3 => {
                "model=TAS5805M,bck=14,ws=15,do=16,sda=8,scl=9,mute=17:0,i2c=76"
            }
            Self::SqueezeAmp => {
                "model=TAS5713,bck=26,ws=25,do=22,sda=21,scl=27,mute=19:0,i2c=27"
            }
            Self::MuseProto => {
                "model=ES8388,bck=5,ws=25,do=26,mck=0,sda=18,scl=23,mute=21:1,i2c=16"
            }
            Self::TWatch2020 => "model=I2S,bck=26,ws=25,do=33",
            Self::GenericI2s => "model=I2S,bck=26,ws=25,do=22",
        }
    }

    /// Returns default I2S pin assignments `(bck, ws, do, mclk)`.
    #[must_use]
    pub const fn i2s_pins(self) -> (i8, i8, i8, Option<i8>) {
        match self {
            Self::LouderEsp32 | Self::SqueezeAmp | Self::GenericI2s => (26, 25, 22, None),
            Self::LouderEsp32S3 => (14, 15, 16, None),
            Self::MuseProto => (5, 25, 26, Some(0)),
            Self::TWatch2020 => (26, 25, 33, None),
        }
    }

    /// Returns default I2C control bus pins `(sda, scl)`, if present.
    #[must_use]
    pub const fn i2c_pins(self) -> Option<(i8, i8)> {
        match self {
            Self::LouderEsp32 | Self::SqueezeAmp => Some((21, 27)),
            Self::LouderEsp32S3 => Some((8, 9)),
            Self::MuseProto => Some((18, 23)),
            Self::TWatch2020 => Some((21, 22)),
            Self::GenericI2s => None,
        }
    }

    /// Returns default SPI display/peripheral pins `(mosi, sclk, cs)`.
    #[must_use]
    pub const fn spi_pins(self) -> Option<(i8, i8, i8)> {
        match self {
            Self::LouderEsp32 | Self::SqueezeAmp => Some((23, 18, 5)),
            Self::LouderEsp32S3 => Some((11, 12, 10)),
            Self::MuseProto => Some((15, 14, 13)),
            Self::TWatch2020 => Some((19, 18, 5)),
            Self::GenericI2s => None,
        }
    }

    /// Returns the amplifier power/enable GPIO pin, if equipped.
    #[must_use]
    pub const fn amp_power_gpio(self) -> Option<u8> {
        match self {
            Self::LouderEsp32 => Some(33),
            Self::LouderEsp32S3 => Some(17),
            Self::SqueezeAmp => Some(33),
            Self::MuseProto => Some(21),
            Self::TWatch2020 | Self::GenericI2s => None,
        }
    }

    /// Returns the battery ADC1 channel number (`0..=7`), if equipped.
    #[must_use]
    pub const fn battery_adc_channel(self) -> Option<u8> {
        match self {
            Self::SqueezeAmp => Some(7),
            Self::MuseProto => Some(5),
            Self::TWatch2020 => Some(7),
            Self::LouderEsp32 | Self::LouderEsp32S3 | Self::GenericI2s => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_manager_inactivity_battery_and_spurious_wake() {
        let mut mgr = SleepManager::new(10_000, 3_000, 3_300, 0, Some(33));
        mgr.add_wake_gpio(0);
        mgr.add_wake_gpio(34);
        assert_eq!(mgr.wake_pin_mask(), (1u64 << 0) | (1u64 << 34));
        assert!(mgr.is_amp_power_enabled());
        assert!(mgr.should_enable_amp_power());
        assert!(!mgr.soft_mute_requested());

        assert_eq!(mgr.evaluate(5_000, false, Some(3_800)), SleepState::Active);
        assert_eq!(
            mgr.evaluate(12_000, false, Some(3_800)),
            SleepState::LightSleep
        );
        assert!(!mgr.is_amp_power_enabled());
        assert!(mgr.soft_mute_requested());
        assert_eq!(
            mgr.evaluate(25_000, false, Some(3_800)),
            SleepState::DeepSleep
        );
        assert!(mgr.soft_mute_requested());

        assert_eq!(
            mgr.evaluate_wake_window(30_000, 31_000, false),
            SleepState::LightSleep
        );
        assert_eq!(
            mgr.evaluate_wake_window(30_000, 34_000, false),
            SleepState::DeepSleep
        );
        assert_eq!(mgr.spurious_wake_count(), 1);

        assert_eq!(
            mgr.evaluate_wake_window(40_000, 40_500, true),
            SleepState::Active
        );
        assert_eq!(
            mgr.evaluate(41_000, true, Some(3_200)),
            SleepState::DeepSleep
        );
    }

    #[test]
    fn test_aude_packet_and_console_sleep_commands() {
        let mut mgr = SleepManager::new(60_000, 5_000, 3_300, 1, Some(17));
        assert!(mgr.is_amp_power_enabled());
        mgr.handle_aude_packet(&[1, 0]);
        assert!(!mgr.is_amp_power_enabled());
        mgr.handle_aude_packet(&[1, 1]);
        assert!(mgr.is_amp_power_enabled());

        assert_eq!(
            SleepManager::parse_console_command("deep_sleep 3600"),
            Some((SleepState::DeepSleep, Some(3600)))
        );
        assert_eq!(
            SleepManager::parse_console_command("light_sleep"),
            Some((SleepState::LightSleep, None))
        );
        assert!(SleepManager::parse_console_command("reboot").is_none());
    }

    #[test]
    fn test_all_board_profiles() {
        let boards = [
            ("LOUDER-ESP32", BoardProfile::LouderEsp32, false),
            ("LOUDER-ESP32-S3", BoardProfile::LouderEsp32S3, true),
            ("SQUEEZEAMP", BoardProfile::SqueezeAmp, false),
            ("MUSE", BoardProfile::MuseProto, false),
            ("TWATCH2020", BoardProfile::TWatch2020, false),
            ("GENERIC-I2S", BoardProfile::GenericI2s, false),
        ];
        for (name, expected, is_s3) in boards {
            let p = BoardProfile::from_name(name).unwrap();
            assert_eq!(p, expected);
            assert_eq!(p.is_esp32_s3(), is_s3);
            assert!(p.default_dac_config().starts_with("model="));
            let (bck, ws, dout, _) = p.i2s_pins();
            assert!(bck >= 0 && ws >= 0 && dout >= 0);
        }
        assert_eq!(BoardProfile::SqueezeAmp.battery_adc_channel(), Some(7));
        assert_eq!(BoardProfile::SqueezeAmp.amp_power_gpio(), Some(33));
        assert_eq!(BoardProfile::LouderEsp32.amp_power_gpio(), Some(33));
        assert_eq!(BoardProfile::LouderEsp32S3.amp_power_gpio(), Some(17));
    }
}
