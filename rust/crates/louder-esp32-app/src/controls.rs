//! Physical Controls, IR Receiver, GPIO Expanders, LEDs & Battery (Phase 7).
//!
//! Provides pure-Rust, `no_std`-compatible state machines for debounced
//! buttons (short/long/repeat + shifter), quadrature rotary encoders
//! (standard, knob-only, volume-lock), NEC/RC5 infrared decoding with
//! SlimProto `IR  `/`BUTN` frame builders, I2C/SPI GPIO expanders,
//! `actrls_config` JSON action mapping, WS2812 GRB VU/spectrum visualizers,
//! and ADC battery/jack/fault monitoring.
#![forbid(unsafe_code)]

/// Logical player control actions triggered by buttons, encoders, or IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControlAction {
    /// Toggle play / pause (`["pause"]`).
    PlayPause,
    /// Stop playback (`["stop"]`).
    Stop,
    /// Step volume up (`["mixer", "volume", "+5"]`).
    VolumeUp,
    /// Step volume down (`["mixer", "volume", "-5"]`).
    VolumeDown,
    /// Toggle hardware/mixer mute (`["mixer", "muting"]`).
    MuteToggle,
    /// Skip to next track (`["playlist", "index", "+1"]`).
    NextTrack,
    /// Return to previous track (`["playlist", "index", "-1"]`).
    PrevTrack,
    /// Toggle player power / standby (`["power"]`).
    PowerToggle,
    /// Trigger numbered LMS preset (`1..=9`).
    Preset(u8),
}

impl ControlAction {
    /// Parses a C `actrls` action token (e.g. `"BTN_PLAY"`, `"KNOB_LEFT"`).
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        let t = token.trim();
        if t.eq_ignore_ascii_case("BTN_PLAY") || t.eq_ignore_ascii_case("PLAY_PAUSE") {
            Some(Self::PlayPause)
        } else if t.eq_ignore_ascii_case("BTN_STOP") {
            Some(Self::Stop)
        } else if t.eq_ignore_ascii_case("BTN_VOLUP") || t.eq_ignore_ascii_case("KNOB_RIGHT") {
            Some(Self::VolumeUp)
        } else if t.eq_ignore_ascii_case("BTN_VOLDOWN") || t.eq_ignore_ascii_case("KNOB_LEFT") {
            Some(Self::VolumeDown)
        } else if t.eq_ignore_ascii_case("BTN_MUTE") {
            Some(Self::MuteToggle)
        } else if t.eq_ignore_ascii_case("BTN_NEXT") {
            Some(Self::NextTrack)
        } else if t.eq_ignore_ascii_case("BTN_PREV") {
            Some(Self::PrevTrack)
        } else if t.eq_ignore_ascii_case("BTN_POWER") {
            Some(Self::PowerToggle)
        } else if t.eq_ignore_ascii_case("BTN_PRESET1") {
            Some(Self::Preset(1))
        } else if t.eq_ignore_ascii_case("BTN_PRESET2") {
            Some(Self::Preset(2))
        } else {
            None
        }
    }

    /// Returns the equivalent LMS CLI command string for this action.
    #[must_use]
    pub const fn lms_cli_command(self) -> &'static str {
        match self {
            Self::PlayPause => "pause",
            Self::Stop => "stop",
            Self::VolumeUp => "mixer volume +5",
            Self::VolumeDown => "mixer volume -5",
            Self::MuteToggle => "mixer muting toggle",
            Self::NextTrack => "button jump_fwd",
            Self::PrevTrack => "button jump_rew",
            Self::PowerToggle => "power",
            Self::Preset(_) => "button preset_1.single",
        }
    }
}

/// Button gesture emitted by [`ButtonDebouncer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonGesture {
    /// Initial debounced press edge.
    Pressed,
    /// Released before the long-press threshold.
    ShortRelease,
    /// Held past the long-press threshold.
    LongPress,
    /// Held continuously past repeat intervals after a long press.
    LongRepeat,
    /// Released before long-press threshold while shifter button was held.
    ShiftedShortRelease,
    /// Held past long-press threshold while shifter button was held.
    ShiftedLongPress,
}

/// Single-pin button debouncer with short, long, repeat, and shifter support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ButtonDebouncer {
    gpio: u8,
    active_low: bool,
    debounce_ms: u32,
    long_press_ms: u32,
    repeat_ms: u32,
    stable_pressed: bool,
    raw_pressed: bool,
    last_edge_ms: u32,
    press_start_ms: u32,
    last_repeat_ms: u32,
    long_fired: bool,
}

impl ButtonDebouncer {
    /// Creates a new [`ButtonDebouncer`] for `gpio`.
    #[must_use]
    pub const fn new(
        gpio: u8,
        active_low: bool,
        debounce_ms: u32,
        long_press_ms: u32,
        repeat_ms: u32,
    ) -> Self {
        Self {
            gpio,
            active_low,
            debounce_ms,
            long_press_ms,
            repeat_ms,
            stable_pressed: false,
            raw_pressed: false,
            last_edge_ms: 0,
            press_start_ms: 0,
            last_repeat_ms: 0,
            long_fired: false,
        }
    }

    /// Returns the assigned GPIO pin number.
    #[must_use]
    pub const fn gpio(&self) -> u8 {
        self.gpio
    }

    /// Returns whether the button is currently in the debounced pressed state.
    #[must_use]
    pub const fn is_pressed(&self) -> bool {
        self.stable_pressed
    }

    /// Samples the physical pin level at `now_ms` with optional `shifter_held`
    /// modifier, returning a [`ButtonGesture`] when a transition occurs.
    pub fn update(
        &mut self,
        pin_high: bool,
        now_ms: u32,
        shifter_held: bool,
    ) -> Option<ButtonGesture> {
        let logical_pressed = if self.active_low { !pin_high } else { pin_high };
        if logical_pressed != self.raw_pressed {
            self.raw_pressed = logical_pressed;
            self.last_edge_ms = now_ms;
        }
        if now_ms.wrapping_sub(self.last_edge_ms) < self.debounce_ms {
            return None;
        }
        if self.raw_pressed != self.stable_pressed {
            self.stable_pressed = self.raw_pressed;
            if self.stable_pressed {
                self.press_start_ms = now_ms;
                self.last_repeat_ms = now_ms;
                self.long_fired = false;
                return Some(ButtonGesture::Pressed);
            }
            if !self.long_fired {
                return Some(if shifter_held {
                    ButtonGesture::ShiftedShortRelease
                } else {
                    ButtonGesture::ShortRelease
                });
            }
            return None;
        }
        if self.stable_pressed {
            let held_ms = now_ms.wrapping_sub(self.press_start_ms);
            if !self.long_fired && held_ms >= self.long_press_ms {
                self.long_fired = true;
                self.last_repeat_ms = now_ms;
                return Some(if shifter_held {
                    ButtonGesture::ShiftedLongPress
                } else {
                    ButtonGesture::LongPress
                });
            }
            if self.long_fired
                && self.repeat_ms > 0
                && now_ms.wrapping_sub(self.last_repeat_ms) >= self.repeat_ms
            {
                self.last_repeat_ms = now_ms;
                return Some(ButtonGesture::LongRepeat);
            }
        }
        None
    }
}

/// Builds a SlimProto `BUTN` client-to-server raw button frame (16 bytes).
///
/// Wire layout: `"BUTN"` (4B) + `len = 8` (4B BE) + `timestamp_ms` (4B BE) +
/// `button_code` (4B BE).
#[must_use]
pub fn build_butn_frame(timestamp_ms: u32, button_code: u32, out: &mut [u8]) -> Option<usize> {
    if out.len() < 16 {
        return None;
    }
    out[0..4].copy_from_slice(b"BUTN");
    out[4..8].copy_from_slice(&8u32.to_be_bytes());
    out[8..12].copy_from_slice(&timestamp_ms.to_be_bytes());
    out[12..16].copy_from_slice(&button_code.to_be_bytes());
    Some(16)
}

/// Operational mode of a [`RotaryEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncoderMode {
    /// Emits `Clockwise` / `CounterClockwise` navigation events.
    Standard,
    /// Knob-only mode: rapid direction reversal within `reversal_window_ms`
    /// synthesizes a virtual button press (`VirtualPress`).
    KnobOnly {
        /// Maximum time window in ms for direction-reversal click detection.
        reversal_window_ms: u32,
    },
    /// Volume-lock mode: rotation always emits `VolumeUp` / `VolumeDown`.
    VolumeLock,
}

/// Event emitted by [`RotaryEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RotaryEvent {
    /// One detent clockwise.
    Clockwise,
    /// One detent counter-clockwise.
    CounterClockwise,
    /// Direct volume-up step (`VolumeLock` mode).
    VolumeUp,
    /// Direct volume-down step (`VolumeLock` mode).
    VolumeDown,
    /// Synthesized click from rapid reversal in `KnobOnly` mode.
    VirtualPress,
}

/// Quadrature Gray-code rotary encoder state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RotaryEncoder {
    mode: EncoderMode,
    prev_ab: u8,
    accum: i8,
    last_step_dir: i8,
    last_step_ms: u32,
}

/// Quadrature state transition table indexed by `(prev_ab << 2) | curr_ab`.
const QUADRATURE_TABLE: [i8; 16] = [0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1, -1, 0];

impl RotaryEncoder {
    /// Creates a new [`RotaryEncoder`] in `mode`.
    #[must_use]
    pub const fn new(mode: EncoderMode) -> Self {
        Self {
            mode,
            prev_ab: 0x03,
            accum: 0,
            last_step_dir: 0,
            last_step_ms: 0,
        }
    }

    /// Returns the configured [`EncoderMode`].
    #[must_use]
    pub const fn mode(&self) -> EncoderMode {
        self.mode
    }

    /// Feeds quadrature pin states `(pin_a, pin_b)` at `now_ms` and returns a
    /// [`RotaryEvent`] on each completed 4-edge detent.
    pub fn sample(&mut self, pin_a: bool, pin_b: bool, now_ms: u32) -> Option<RotaryEvent> {
        let curr_ab = ((pin_a as u8) << 1) | (pin_b as u8);
        let idx = ((self.prev_ab & 0x03) << 2) | curr_ab;
        self.prev_ab = curr_ab;
        let delta = QUADRATURE_TABLE[idx as usize];
        if delta == 0 {
            return None;
        }
        self.accum = self.accum.saturating_add(delta);
        if self.accum >= 4 {
            self.accum = 0;
            return Some(self.resolve_detent(1, now_ms));
        }
        if self.accum <= -4 {
            self.accum = 0;
            return Some(self.resolve_detent(-1, now_ms));
        }
        None
    }

    fn resolve_detent(&mut self, dir: i8, now_ms: u32) -> RotaryEvent {
        if let EncoderMode::KnobOnly { reversal_window_ms } = self.mode {
            if self.last_step_dir != 0
                && self.last_step_dir != dir
                && now_ms.wrapping_sub(self.last_step_ms) <= reversal_window_ms
            {
                self.last_step_dir = 0;
                return RotaryEvent::VirtualPress;
            }
        }
        self.last_step_dir = dir;
        self.last_step_ms = now_ms;
        match self.mode {
            EncoderMode::VolumeLock => {
                if dir > 0 {
                    RotaryEvent::VolumeUp
                } else {
                    RotaryEvent::VolumeDown
                }
            }
            EncoderMode::Standard | EncoderMode::KnobOnly { .. } => {
                if dir > 0 {
                    RotaryEvent::Clockwise
                } else {
                    RotaryEvent::CounterClockwise
                }
            }
        }
    }
}

/// Infrared protocol family decoded by [`IrDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IrProtocol {
    /// NEC 32-bit pulse-distance protocol (`0x01`).
    Nec,
    /// NEC repeat pulse (`0x02`).
    NecRepeat,
    /// Philips RC5 14-bit Manchester protocol (`0x03`).
    Rc5,
}

/// Decoded infrared frame ready for local action mapping or SlimProto `IR  `.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IrFrame {
    /// Decoded IR protocol family.
    pub protocol: IrProtocol,
    /// Bit width of `code` (`32` for NEC, `14` for RC5).
    pub bits: u8,
    /// Decoded 32-bit or 14-bit command word.
    pub code: u32,
}

/// Stateless NEC & RC5 pulse/space decoder and local action mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IrDecoder {
    last_nec_code: Option<u32>,
}

impl Default for IrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl IrDecoder {
    /// Creates a new [`IrDecoder`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_nec_code: None,
        }
    }

    /// Decodes a sequence of `(mark_us, space_us)` RMT pulse pairs as NEC.
    ///
    /// - Leader mark: `8000..=10000 us`
    /// - Standard space: `4000..=5000 us` followed by 32 `(mark, space)` pairs
    ///   where `space >= 1200 us` is a `1` bit and `< 1200 us` is a `0` bit.
    /// - Repeat space: `1800..=2600 us` re-emits the last NEC code.
    pub fn decode_nec_pulses(&mut self, pulses: &[(u16, u16)]) -> Option<IrFrame> {
        let &(leader_mark, leader_space) = pulses.first()?;
        if !(8000..=10000).contains(&leader_mark) {
            return None;
        }
        if (1800..=2600).contains(&leader_space) {
            let code = self.last_nec_code?;
            return Some(IrFrame {
                protocol: IrProtocol::NecRepeat,
                bits: 32,
                code,
            });
        }
        if !(4000..=5000).contains(&leader_space) || pulses.len() < 33 {
            return None;
        }
        let mut code = 0u32;
        let mut bit = 0usize;
        while bit < 32 {
            let (mark, space) = pulses[1 + bit];
            if !(350..=800).contains(&mark) {
                return None;
            }
            code <<= 1;
            if (1200..=2000).contains(&space) {
                code |= 1;
            } else if !(350..=900).contains(&space) {
                return None;
            }
            bit += 1;
        }
        self.last_nec_code = Some(code);
        Some(IrFrame {
            protocol: IrProtocol::Nec,
            bits: 32,
            code,
        })
    }

    /// Decodes a 14-bit Philips RC5 word from 14 sampled Manchester bit values
    /// (`S1 = 1`, `S2`, `toggle`, 5-bit address, 6-bit command).
    #[must_use]
    pub fn decode_rc5_bits(&self, bits: &[bool; 14]) -> Option<IrFrame> {
        if !bits[0] {
            return None;
        }
        let mut word = 0u32;
        let mut idx = 0usize;
        while idx < 14 {
            word = (word << 1) | (bits[idx] as u32);
            idx += 1;
        }
        Some(IrFrame {
            protocol: IrProtocol::Rc5,
            bits: 14,
            code: word,
        })
    }

    /// Maps standard Squeezebox / NEC IR codes to a [`ControlAction`].
    #[must_use]
    pub const fn map_to_action(code: u32) -> Option<ControlAction> {
        match code {
            0x7689_10EF | 0x00FF_02FD => Some(ControlAction::PlayPause),
            0x7689_807F | 0x00FF_629D => Some(ControlAction::VolumeUp),
            0x7689_00FF | 0x00FF_A857 => Some(ControlAction::VolumeDown),
            0x7689_A05F | 0x00FF_C23D => Some(ControlAction::NextTrack),
            0x7689_C03F | 0x00FF_22DD => Some(ControlAction::PrevTrack),
            0x7689_C43B | 0x00FF_E21D => Some(ControlAction::MuteToggle),
            0x7689_40BF | 0x00FF_A25D => Some(ControlAction::PowerToggle),
            _ => None,
        }
    }
}

/// Builds a SlimProto `IR  ` client-to-server packet (18 bytes).
///
/// Wire layout: `"IR  "` (4B) + `len = 10` (4B BE) + `timestamp_ms` (4B BE) +
/// `format` (1B) + `bits` (1B) + `code` (4B BE).
#[must_use]
pub fn build_ir_frame(
    timestamp_ms: u32,
    format: u8,
    bits: u8,
    code: u32,
    out: &mut [u8],
) -> Option<usize> {
    if out.len() < 18 {
        return None;
    }
    out[0..4].copy_from_slice(b"IR  ");
    out[4..8].copy_from_slice(&10u32.to_be_bytes());
    out[8..12].copy_from_slice(&timestamp_ms.to_be_bytes());
    out[12] = format;
    out[13] = bits;
    out[14..18].copy_from_slice(&code.to_be_bytes());
    Some(18)
}

/// Supported external GPIO & touch expander chips (Phase 7 parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GpioExpanderKind {
    /// NXP PCA9535 16-bit I2C GPIO expander.
    Pca9535,
    /// NXP PCA8574 8-bit quasi-bidirectional I2C expander.
    Pca8574,
    /// Microchip MCP23017 16-bit I2C GPIO expander.
    Mcp23017,
    /// Microchip MCP23S17 16-bit SPI GPIO expander.
    Mcp23s17,
    /// NXP MPR121 12-channel capacitive touch sensor controller.
    Mpr121,
    /// Awinic AW9523 16-channel I2C GPIO & LED driver.
    Aw9523,
}

const INIT_PCA9535: &[(u8, u8)] = &[(0x06, 0xFF), (0x07, 0xFF)];
const INIT_PCA8574: &[(u8, u8)] = &[(0xFF, 0xFF)];
const INIT_MCP23017: &[(u8, u8)] = &[(0x00, 0xFF), (0x01, 0xFF), (0x0C, 0xFF), (0x0D, 0xFF)];
const INIT_MPR121: &[(u8, u8)] = &[(0x5E, 0x00), (0x5B, 0x00), (0x5E, 0x8F)];
const INIT_AW9523: &[(u8, u8)] = &[(0x7F, 0x00), (0x04, 0xFF), (0x05, 0xFF)];

impl GpioExpanderKind {
    /// Default 7-bit I2C slave address (or `0x20` hardware address for MCP23S17).
    #[must_use]
    pub const fn default_i2c_addr(self) -> u8 {
        match self {
            Self::Pca9535 | Self::Pca8574 | Self::Mcp23017 | Self::Mcp23s17 => 0x20,
            Self::Mpr121 => 0x5A,
            Self::Aw9523 => 0x58,
        }
    }

    /// Number of input channels provided by this expander (`8`, `12`, or `16`).
    #[must_use]
    pub const fn port_bits(self) -> u8 {
        match self {
            Self::Pca8574 => 8,
            Self::Mpr121 => 12,
            Self::Pca9535 | Self::Mcp23017 | Self::Mcp23s17 | Self::Aw9523 => 16,
        }
    }

    /// Register `(reg, value)` initialization pairs to configure input mode.
    #[must_use]
    pub const fn init_register_pairs(self) -> &'static [(u8, u8)] {
        match self {
            Self::Pca9535 => INIT_PCA9535,
            Self::Pca8574 => INIT_PCA8574,
            Self::Mcp23017 | Self::Mcp23s17 => INIT_MCP23017,
            Self::Mpr121 => INIT_MPR121,
            Self::Aw9523 => INIT_AW9523,
        }
    }

    /// Starting register address to read input port status.
    #[must_use]
    pub const fn input_read_register(self) -> u8 {
        match self {
            Self::Pca9535 | Self::Pca8574 | Self::Mpr121 | Self::Aw9523 => 0x00,
            Self::Mcp23017 | Self::Mcp23s17 => 0x12,
        }
    }

    /// Parses 1 or 2 raw bytes read from [`Self::input_read_register`] into a
    /// 16-bit channel bitmask.
    #[must_use]
    pub fn parse_port_state(self, raw: &[u8]) -> u16 {
        match self {
            Self::Pca8574 => raw.first().copied().unwrap_or(0xFF) as u16,
            Self::Mpr121 => {
                let lo = raw.first().copied().unwrap_or(0) as u16;
                let hi = (raw.get(1).copied().unwrap_or(0) & 0x0F) as u16;
                (hi << 8) | lo
            }
            Self::Pca9535 | Self::Mcp23017 | Self::Mcp23s17 | Self::Aw9523 => {
                let lo = raw.first().copied().unwrap_or(0xFF) as u16;
                let hi = raw.get(1).copied().unwrap_or(0xFF) as u16;
                (hi << 8) | lo
            }
        }
    }
}

/// Single GPIO-to-action binding parsed from `actrls_config` JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ActrlsEntry {
    /// Physical or virtual GPIO pin number.
    pub gpio: u8,
    /// Action triggered on a normal short release.
    pub normal: Option<ControlAction>,
    /// Action triggered on a long press.
    pub long_press: Option<ControlAction>,
    /// Action triggered on a shifted short release.
    pub shifted: Option<ControlAction>,
}

/// Fixed-capacity `actrls_config` JSON action map (up to 16 entries).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ActrlsMap {
    entries: [Option<ActrlsEntry>; 16],
    len: usize,
}

impl Default for ActrlsMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ActrlsMap {
    /// Creates an empty [`ActrlsMap`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; 16],
            len: 0,
        }
    }

    /// Parses a C-compatible `actrls_config` JSON array string into an
    /// [`ActrlsMap`].
    #[must_use]
    pub fn parse_json(json: &str) -> Self {
        let mut map = Self::new();
        for obj in json.split('{').skip(1) {
            let Some(end) = obj.find('}') else {
                continue;
            };
            let body = &obj[..end];
            if let Some(entry) = Self::parse_object_fields(body) {
                let _ = map.insert(entry);
            }
        }
        map
    }

    fn parse_object_fields(body: &str) -> Option<ActrlsEntry> {
        let mut gpio: Option<u8> = None;
        let mut normal: Option<ControlAction> = None;
        let mut long_press: Option<ControlAction> = None;
        let mut shifted: Option<ControlAction> = None;

        for pair in body.split(',') {
            let Some((k, v)) = pair.split_once(':') else {
                continue;
            };
            let key = k.trim().trim_matches('"');
            let val = v.trim().trim_matches('"');
            if key.eq_ignore_ascii_case("gpio") {
                gpio = Self::parse_u8(val);
            } else if key.eq_ignore_ascii_case("normal") {
                normal = ControlAction::from_token(val);
            } else if key.eq_ignore_ascii_case("long") {
                long_press = ControlAction::from_token(val);
            } else if key.eq_ignore_ascii_case("shifted") {
                shifted = ControlAction::from_token(val);
            }
        }
        Some(ActrlsEntry {
            gpio: gpio?,
            normal,
            long_press,
            shifted,
        })
    }

    fn parse_u8(s: &str) -> Option<u8> {
        let bytes = s.trim().as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let mut acc = 0u16;
        let mut idx = 0usize;
        while idx < bytes.len() {
            let b = bytes[idx];
            if !b.is_ascii_digit() {
                return None;
            }
            acc = acc * 10 + ((b - b'0') as u16);
            if acc > 255 {
                return None;
            }
            idx += 1;
        }
        Some(acc as u8)
    }

    /// Inserts or updates an [`ActrlsEntry`].
    pub fn insert(&mut self, entry: ActrlsEntry) -> bool {
        if self.len >= self.entries.len() {
            return false;
        }
        self.entries[self.len] = Some(entry);
        self.len += 1;
        true
    }

    /// Returns the number of configured bindings.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the action map is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Resolves `(gpio, gesture)` to a mapped [`ControlAction`].
    #[must_use]
    pub fn resolve(&self, gpio: u8, gesture: ButtonGesture) -> Option<ControlAction> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(entry) = self.entries[i] {
                if entry.gpio == gpio {
                    return match gesture {
                        ButtonGesture::ShortRelease => entry.normal,
                        ButtonGesture::LongPress | ButtonGesture::LongRepeat => {
                            entry.long_press.or(entry.normal)
                        }
                        ButtonGesture::ShiftedShortRelease | ButtonGesture::ShiftedLongPress => {
                            entry.shifted.or(entry.normal)
                        }
                        ButtonGesture::Pressed => None,
                    };
                }
            }
            i += 1;
        }
        None
    }
}

/// System status LED state for green/red PWM or WS2812 indicator LEDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusLedState {
    /// Indicator off.
    Off,
    /// Booting / connecting to Wi-Fi (amber).
    Booting,
    /// Connected to LMS, idle (dim green).
    Connected,
    /// Actively streaming audio (bright green).
    Playing,
    /// Hardware or network fault (red).
    Fault,
}

/// WS2812 GRB LED strip formatter for VU meter (`ledv`) and spectrum (`ledd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LedStripVisualizer {
    num_leds: usize,
    brightness: u8,
}

impl LedStripVisualizer {
    /// Creates a new [`LedStripVisualizer`] for `num_leds` pixels.
    #[must_use]
    pub const fn new(num_leds: usize, brightness: u8) -> Self {
        Self {
            num_leds,
            brightness,
        }
    }

    /// Formats a stereo VU bar (`left_pct`, `right_pct` in `0..=100`) into
    /// WS2812 `[G, R, B]` byte triples in `out`.
    pub fn format_vu_grb(&self, left_pct: u8, right_pct: u8, out: &mut [u8]) -> usize {
        let required = self.num_leds * 3;
        if out.len() < required || self.num_leds == 0 {
            return 0;
        }
        let avg_pct = ((left_pct.min(100) as usize) + (right_pct.min(100) as usize)) / 2;
        let lit_leds = (avg_pct * self.num_leds + 50) / 100;
        let scale = self.brightness as u16;
        let mut idx = 0usize;
        while idx < self.num_leds {
            let base = idx * 3;
            if idx < lit_leds {
                let (r, g, b) = if idx * 10 >= self.num_leds * 8 {
                    (255u16, 0u16, 0u16)
                } else if idx * 10 >= self.num_leds * 6 {
                    (200u16, 180u16, 0u16)
                } else {
                    (0u16, 220u16, 0u16)
                };
                out[base] = ((g * scale) >> 8) as u8;
                out[base + 1] = ((r * scale) >> 8) as u8;
                out[base + 2] = ((b * scale) >> 8) as u8;
            } else {
                out[base] = 0;
                out[base + 1] = 0;
                out[base + 2] = 0;
            }
            idx += 1;
        }
        required
    }

    /// Formats a single status pixel in `[G, R, B]` order for `state`.
    #[must_use]
    pub const fn status_pixel_grb(state: StatusLedState) -> [u8; 3] {
        match state {
            StatusLedState::Off => [0, 0, 0],
            StatusLedState::Booting => [0x40, 0x80, 0x00],
            StatusLedState::Connected => [0x30, 0x00, 0x00],
            StatusLedState::Playing => [0xFF, 0x00, 0x00],
            StatusLedState::Fault => [0x00, 0xFF, 0x00],
        }
    }
}

/// Gestures emitted by [`AdcButtonLadder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdcButtonGesture {
    /// Button was pressed and released before `long_press_ms`.
    ShortPress(usize),
    /// Button was held continuously for at least `long_press_ms`.
    LongPress(usize),
    /// Button was released after a `LongPress`.
    Released(usize),
}

/// Multi-button resistor ladder connected to an analog-to-digital converter
/// (ADC) channel.
///
/// Decodes up to `N` pushbuttons mapped to distinct nominal millivolt levels,
/// providing configurable tolerance/hysteresis, debounce filtering, and
/// short/long/release gesture detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdcButtonLadder<const N: usize> {
    targets_mv: [u16; N],
    tolerance_mv: u16,
    debounce_ms: u32,
    long_press_ms: u32,
    raw_button: Option<usize>,
    stable_button: Option<usize>,
    last_edge_ms: u32,
    press_start_ms: u32,
    long_fired: bool,
}

impl<const N: usize> AdcButtonLadder<N> {
    /// Creates a new [`AdcButtonLadder`] with target voltages in millivolts.
    #[must_use]
    pub const fn new(
        targets_mv: [u16; N],
        tolerance_mv: u16,
        debounce_ms: u32,
        long_press_ms: u32,
    ) -> Self {
        Self {
            targets_mv,
            tolerance_mv,
            debounce_ms,
            long_press_ms,
            raw_button: None,
            stable_button: None,
            last_edge_ms: 0,
            press_start_ms: 0,
            long_fired: false,
        }
    }

    /// Returns the target millivolt thresholds.
    #[must_use]
    pub const fn targets_mv(&self) -> &[u16; N] {
        &self.targets_mv
    }

    /// Returns the current tolerance window in millivolts.
    #[must_use]
    pub const fn tolerance_mv(&self) -> u16 {
        self.tolerance_mv
    }

    /// Sets the tolerance window in millivolts.
    pub fn set_tolerance_mv(&mut self, tolerance_mv: u16) {
        self.tolerance_mv = tolerance_mv;
    }

    /// Returns the debounce duration in milliseconds.
    #[must_use]
    pub const fn debounce_ms(&self) -> u32 {
        self.debounce_ms
    }

    /// Sets the debounce duration in milliseconds.
    pub fn set_debounce_ms(&mut self, debounce_ms: u32) {
        self.debounce_ms = debounce_ms;
    }

    /// Returns the long-press threshold in milliseconds.
    #[must_use]
    pub const fn long_press_ms(&self) -> u32 {
        self.long_press_ms
    }

    /// Sets the long-press threshold in milliseconds.
    pub fn set_long_press_ms(&mut self, long_press_ms: u32) {
        self.long_press_ms = long_press_ms;
    }

    /// Returns the currently debounced active button, if any.
    #[must_use]
    pub const fn stable_button(&self) -> Option<usize> {
        self.stable_button
    }

    /// Returns `true` if any button on the ladder is currently debounced
    /// and held.
    #[must_use]
    pub const fn is_pressed(&self) -> bool {
        self.stable_button.is_some()
    }

    /// Finds the closest matching button index within `tolerance_mv` for
    /// `sample_mv`.
    #[must_use]
    pub fn match_target(&self, sample_mv: u16) -> Option<usize> {
        let mut best = None;
        let mut min_diff = u16::MAX;
        let mut i = 0usize;
        while i < N {
            let diff = sample_mv.abs_diff(self.targets_mv[i]);
            if diff <= self.tolerance_mv && diff < min_diff {
                min_diff = diff;
                best = Some(i);
            }
            i += 1;
        }
        best
    }

    /// Updates the button ladder state with a new ADC millivolt sample at `now_ms`.
    ///
    /// Emits:
    /// - [`AdcButtonGesture::LongPress`] once held for `long_press_ms`.
    /// - [`AdcButtonGesture::Released`] when released following a `LongPress`.
    /// - [`AdcButtonGesture::ShortPress`] when released before `long_press_ms`.
    pub fn update(&mut self, sample_mv: u16, now_ms: u32) -> Option<AdcButtonGesture> {
        let raw = self.match_target(sample_mv);
        if raw != self.raw_button {
            self.raw_button = raw;
            self.last_edge_ms = now_ms;
        }

        if now_ms.wrapping_sub(self.last_edge_ms) < self.debounce_ms {
            return None;
        }

        if self.raw_button != self.stable_button {
            let prev = self.stable_button;
            let was_long_fired = self.long_fired;
            self.stable_button = self.raw_button;

            if self.stable_button.is_some() {
                self.press_start_ms = now_ms;
                self.long_fired = false;
            }

            if let Some(prev_idx) = prev {
                if was_long_fired {
                    self.long_fired = false;
                    return Some(AdcButtonGesture::Released(prev_idx));
                } else {
                    return Some(AdcButtonGesture::ShortPress(prev_idx));
                }
            }
            return None;
        }

        if let Some(btn_idx) = self.stable_button {
            let held_ms = now_ms.wrapping_sub(self.press_start_ms);
            if !self.long_fired && held_ms >= self.long_press_ms {
                self.long_fired = true;
                return Some(AdcButtonGesture::LongPress(btn_idx));
            }
        }

        None
    }
}

/// Battery ADC filter, headphone jack detector, and speaker fault monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatteryMonitor {
    empty_mv: u16,
    full_mv: u16,
    filtered_mv: u16,
    initialized: bool,
    jack_inserted: bool,
    speaker_fault: bool,
}

impl BatteryMonitor {
    /// Creates a new [`BatteryMonitor`] with `empty_mv` (0%) and `full_mv`
    /// (100%) thresholds (e.g. `3300 mV` to `4200 mV` for 1S Li-Ion).
    #[must_use]
    pub const fn new(empty_mv: u16, full_mv: u16) -> Self {
        Self {
            empty_mv,
            full_mv,
            filtered_mv: 0,
            initialized: false,
            jack_inserted: false,
            speaker_fault: false,
        }
    }

    /// Feeds a new battery sample in millivolts and updates the EMA filter.
    pub fn push_sample_mv(&mut self, sample_mv: u16) {
        if !self.initialized {
            self.filtered_mv = sample_mv;
            self.initialized = true;
        } else {
            let acc = (self.filtered_mv as u32) * 3 + (sample_mv as u32);
            self.filtered_mv = (acc >> 2) as u16;
        }
    }

    /// Updates headphone jack insertion and speaker amplifier fault states.
    pub fn update_gpio_sensors(&mut self, jack_inserted: bool, speaker_fault: bool) {
        self.jack_inserted = jack_inserted;
        self.speaker_fault = speaker_fault;
    }

    /// Returns the filtered battery voltage in millivolts (`mV`).
    #[must_use]
    pub const fn voltage_mv(&self) -> u16 {
        self.filtered_mv
    }

    /// Computes the battery state-of-charge percentage (`0..=100%`) for
    /// SlimProto `STAT` reporting.
    #[must_use]
    pub const fn state_of_charge_pct(&self) -> u8 {
        if !self.initialized || self.filtered_mv <= self.empty_mv {
            return 0;
        }
        if self.filtered_mv >= self.full_mv || self.full_mv <= self.empty_mv {
            return 100;
        }
        let span = (self.full_mv - self.empty_mv) as u32;
        let above = (self.filtered_mv - self.empty_mv) as u32;
        ((above * 100) / span) as u8
    }

    /// Returns whether headphones are currently inserted in the 3.5mm jack.
    #[must_use]
    pub const fn is_jack_inserted(&self) -> bool {
        self.jack_inserted
    }

    /// Returns whether the speaker amplifier fault pin is asserted.
    #[must_use]
    pub const fn is_speaker_fault(&self) -> bool {
        self.speaker_fault
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_debouncer_short_long_repeat_and_shifter() {
        let mut btn = ButtonDebouncer::new(32, true, 20, 500, 200);
        assert_eq!(btn.gpio(), 32);
        assert_eq!(btn.update(false, 0, false), None);
        assert_eq!(btn.update(false, 25, false), Some(ButtonGesture::Pressed));
        assert_eq!(btn.update(true, 100, false), None);
        assert_eq!(
            btn.update(true, 125, false),
            Some(ButtonGesture::ShortRelease)
        );

        assert_eq!(btn.update(false, 200, true), None);
        assert_eq!(btn.update(false, 225, true), Some(ButtonGesture::Pressed));
        assert_eq!(
            btn.update(false, 750, true),
            Some(ButtonGesture::ShiftedLongPress)
        );
        assert_eq!(
            btn.update(false, 960, true),
            Some(ButtonGesture::LongRepeat)
        );
        assert_eq!(btn.update(true, 1000, true), None);
        assert_eq!(btn.update(true, 1025, true), None);
    }

    #[test]
    fn test_rotary_encoder_modes() {
        let mut enc = RotaryEncoder::new(EncoderMode::VolumeLock);
        let cw_seq = [(false, true), (false, false), (true, false), (true, true)];
        let mut last = None;
        for (idx, (a, b)) in cw_seq.into_iter().enumerate() {
            last = enc.sample(a, b, (idx as u32) * 5);
        }
        assert_eq!(last, Some(RotaryEvent::VolumeUp));

        let mut knob = RotaryEncoder::new(EncoderMode::KnobOnly {
            reversal_window_ms: 150,
        });
        for (a, b) in cw_seq {
            let _ = knob.sample(a, b, 10);
        }
        let ccw_seq = [(true, false), (false, false), (false, true), (true, true)];
        let mut rev = None;
        for (a, b) in ccw_seq {
            rev = knob.sample(a, b, 50);
        }
        assert_eq!(rev, Some(RotaryEvent::VirtualPress));
    }

    #[test]
    fn test_ir_decoder_nec_and_rc5_and_slimproto_frames() {
        let mut ir = IrDecoder::new();
        let target_code: u32 = 0x7689_10EF;
        let mut pulses = [(0u16, 0u16); 33];
        pulses[0] = (9000, 4500);
        for i in 0..32 {
            let bit = (target_code >> (31 - i)) & 1;
            pulses[1 + i] = (560, if bit == 1 { 1690 } else { 560 });
        }
        let decoded = ir.decode_nec_pulses(&pulses).unwrap();
        assert_eq!(decoded.protocol, IrProtocol::Nec);
        assert_eq!(decoded.code, target_code);
        assert_eq!(
            IrDecoder::map_to_action(decoded.code),
            Some(ControlAction::PlayPause)
        );

        let repeat = ir.decode_nec_pulses(&[(9000, 2250)]).unwrap();
        assert_eq!(repeat.protocol, IrProtocol::NecRepeat);
        assert_eq!(repeat.code, target_code);

        let mut ir_pkt = [0u8; 20];
        assert_eq!(
            build_ir_frame(1234, 1, 32, target_code, &mut ir_pkt),
            Some(18)
        );
        assert_eq!(&ir_pkt[0..4], b"IR  ");

        let mut butn_pkt = [0u8; 20];
        assert_eq!(build_butn_frame(1234, 0x01, &mut butn_pkt), Some(16));
        assert_eq!(&butn_pkt[0..4], b"BUTN");
    }

    #[test]
    fn test_gpio_expanders_and_actrls_json_map() {
        for kind in [
            GpioExpanderKind::Pca9535,
            GpioExpanderKind::Pca8574,
            GpioExpanderKind::Mcp23017,
            GpioExpanderKind::Mcp23s17,
            GpioExpanderKind::Mpr121,
            GpioExpanderKind::Aw9523,
        ] {
            assert!(!kind.init_register_pairs().is_empty());
            assert!(kind.port_bits() >= 8);
            let mask = kind.parse_port_state(&[0xAA, 0x05]);
            assert_eq!(mask & 0xFF, 0xAA);
        }

        let json = r#"[{"gpio":32,"normal":"BTN_PLAY","long":"BTN_POWER","shifted":"BTN_NEXT"}]"#;
        let map = ActrlsMap::parse_json(json);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.resolve(32, ButtonGesture::ShortRelease),
            Some(ControlAction::PlayPause)
        );
        assert_eq!(
            map.resolve(32, ButtonGesture::LongPress),
            Some(ControlAction::PowerToggle)
        );
        assert_eq!(
            map.resolve(32, ButtonGesture::ShiftedShortRelease),
            Some(ControlAction::NextTrack)
        );
    }

    #[test]
    fn test_led_strip_and_battery_monitor() {
        let strip = LedStripVisualizer::new(4, 255);
        let mut grb = [0u8; 12];
        assert_eq!(strip.format_vu_grb(50, 50, &mut grb), 12);
        assert!(grb[0] > 0);
        assert_eq!(grb[9], 0);
        assert_eq!(
            LedStripVisualizer::status_pixel_grb(StatusLedState::Fault),
            [0x00, 0xFF, 0x00]
        );

        let mut batt = BatteryMonitor::new(3300, 4200);
        batt.push_sample_mv(3750);
        assert_eq!(batt.voltage_mv(), 3750);
        assert_eq!(batt.state_of_charge_pct(), 50);
        batt.update_gpio_sensors(true, false);
        assert!(batt.is_jack_inserted());
        assert!(!batt.is_speaker_fault());
    }

    #[test]
    fn test_adc_button_ladder_gestures_and_hysteresis() {
        // Ladder with 3 buttons: 500 mV (btn 0), 1500 mV (btn 1), 2500 mV (btn 2)
        let mut ladder = AdcButtonLadder::new([500, 1500, 2500], 100, 30, 300);
        assert_eq!(ladder.targets_mv(), &[500, 1500, 2500]);
        assert_eq!(ladder.tolerance_mv(), 100);
        assert_eq!(ladder.debounce_ms(), 30);
        assert_eq!(ladder.long_press_ms(), 300);
        assert!(!ladder.is_pressed());

        // 1. Initial idle at 3300 mV (no button)
        assert_eq!(ladder.update(3300, 0), None);
        assert!(!ladder.is_pressed());

        // 2. Button 0 ShortPress: press at t=10, debounced at t=50,
        // release at t=100, debounced release at t=140
        assert_eq!(ladder.update(520, 10), None);
        assert_eq!(ladder.update(510, 30), None); // still debouncing (20ms < 30ms)
        assert_eq!(ladder.update(495, 50), None); // debounced -> stable_button = Some(0)
        assert!(ladder.is_pressed());
        assert_eq!(ladder.stable_button(), Some(0));

        assert_eq!(ladder.update(3300, 100), None); // raw release edge
        assert_eq!(ladder.update(3300, 120), None); // debouncing release
        assert_eq!(
            ladder.update(3300, 140),
            Some(AdcButtonGesture::ShortPress(0))
        );
        assert!(!ladder.is_pressed());
        assert_eq!(ladder.stable_button(), None);

        // 3. Button 1 LongPress & Released: press at t=200, hold past 300ms,
        // release at t=600
        assert_eq!(ladder.update(1480, 200), None);
        assert_eq!(ladder.update(1510, 240), None); // debounced Some(1) at t=240
        assert!(ladder.is_pressed());
        assert_eq!(ladder.stable_button(), Some(1));

        // Advance to t=550 (held for 310ms > 300ms)
        assert_eq!(
            ladder.update(1500, 550),
            Some(AdcButtonGesture::LongPress(1))
        );
        // Subsequent poll while still held -> None
        assert_eq!(ladder.update(1500, 560), None);

        // Release at t=600, debounced at t=640 -> Released(1)
        assert_eq!(ladder.update(3300, 600), None);
        assert_eq!(
            ladder.update(3300, 640),
            Some(AdcButtonGesture::Released(1))
        );
        assert!(!ladder.is_pressed());

        // 4. Out-of-tolerance reject: 700 mV is not within 100 mV of 500, 1500, or 2500
        assert_eq!(ladder.match_target(700), None);
        assert_eq!(ladder.match_target(580), Some(0));
        assert_eq!(ladder.match_target(1590), Some(1));
        assert_eq!(ladder.match_target(2420), Some(2));

        // 5. Mutators
        ladder.set_tolerance_mv(150);
        assert_eq!(ladder.tolerance_mv(), 150);
        ladder.set_debounce_ms(50);
        assert_eq!(ladder.debounce_ms(), 50);
        ladder.set_long_press_ms(500);
        assert_eq!(ladder.long_press_ms(), 500);
    }
}
