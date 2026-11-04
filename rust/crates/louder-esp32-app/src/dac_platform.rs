//! Multi-DAC Audio Platform & Software IEC 60958 S/PDIF BMC Encoder (Phase 7).
//!
//! Provides pure-Rust, `no_std`-compatible multi-DAC register tables and
//! volume/mute curves (`TAS5805M`, `TAS57xx`, `TAS5713`, `AC101`, `WM8978`,
//! `ES8388`, `ES8311`, `GenericI2c`, `I2sPassthrough`, `SpdifOut`),
//! C-compatible `dac_config` and `spdif_config` key-value parsers, a software
//! 64-bit biphase-mark (BMC) S/PDIF subframe encoder, and `audo` /
//! headphone-jack mute policy evaluation.
#![forbid(unsafe_code)]

/// Supported external and internal audio DAC/amplifier models (C `adac`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DacModel {
    /// Texas Instruments TAS5805M / TAS5825M Digital Class-D Amplifier.
    Tas5805m,
    /// Texas Instruments TAS57xx family (TAS5754M / TAS5756M / PCM5122).
    Tas57xx,
    /// Texas Instruments TAS5713 Digital Class-D Amplifier (SqueezeAMP).
    Tas5713,
    /// X-Powers AC101 Audio Codec (ESP32-A1S AudioKit).
    Ac101,
    /// Wolfson / Cirrus Logic WM8978 Stereo Codec (T-Watch / Muse).
    Wm8978,
    /// Everest Semiconductor ES8388 Stereo Audio Codec.
    Es8388,
    /// Everest Semiconductor ES8311 Mono Low-Power Audio Codec.
    Es8311,
    /// Generic user-configured I2C DAC (`DAC_CONTROL_SET`).
    GenericI2c,
    /// Pin-only 3-wire/4-wire I2S DAC (PCM5102A, MAX98357A, UDA1334A).
    I2sPassthrough,
    /// Software biphase-mark S/PDIF optical/coaxial output over I2S DOUT.
    SpdifOut,
}

const INIT_TAS5805M: &[(u8, u8)] = &[
    (0x00, 0x00),
    (0x7F, 0x00),
    (0x03, 0x02),
    (0x01, 0x11),
    (0x03, 0x03),
];
const INIT_TAS57XX: &[(u8, u8)] = &[
    (0x00, 0x00),
    (0x02, 0x10),
    (0x03, 0x00),
    (0x28, 0x00),
    (0x02, 0x00),
];
const INIT_TAS5713: &[(u8, u8)] = &[
    (0x1B, 0x00),
    (0x06, 0x00),
    (0x05, 0x00),
    (0x07, 0x30),
];
const INIT_AC101: &[(u8, u8)] = &[
    (0x00, 0x12),
    (0x10, 0x03),
    (0x40, 0x00),
    (0x50, 0x01),
];
const INIT_WM8978: &[(u8, u8)] = &[
    (0x00, 0x00),
    (0x01, 0x1B),
    (0x02, 0xBF),
    (0x03, 0x6F),
    (0x04, 0x10),
];
const INIT_ES8388: &[(u8, u8)] = &[
    (0x00, 0x06),
    (0x01, 0x50),
    (0x02, 0x00),
    (0x17, 0x18),
    (0x19, 0x02),
];
const INIT_ES8311: &[(u8, u8)] = &[
    (0x00, 0x80),
    (0x01, 0x3F),
    (0x09, 0x0C),
    (0x31, 0x00),
    (0x32, 0xBF),
];
const INIT_GENERIC: &[(u8, u8)] = &[];

impl DacModel {
    /// Parses a DAC model identifier from a `dac_config` `"model=..."` value.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("TAS5805M")
            || trimmed.eq_ignore_ascii_case("TAS5825M")
            || trimmed.eq_ignore_ascii_case("TAS58XX")
        {
            Some(Self::Tas5805m)
        } else if trimmed.eq_ignore_ascii_case("TAS57XX")
            || trimmed.eq_ignore_ascii_case("TAS5754")
            || trimmed.eq_ignore_ascii_case("PCM5122")
        {
            Some(Self::Tas57xx)
        } else if trimmed.eq_ignore_ascii_case("TAS5713")
            || trimmed.eq_ignore_ascii_case("SQUEEZEAMP")
        {
            Some(Self::Tas5713)
        } else if trimmed.eq_ignore_ascii_case("AC101") {
            Some(Self::Ac101)
        } else if trimmed.eq_ignore_ascii_case("WM8978") {
            Some(Self::Wm8978)
        } else if trimmed.eq_ignore_ascii_case("ES8388") {
            Some(Self::Es8388)
        } else if trimmed.eq_ignore_ascii_case("ES8311") {
            Some(Self::Es8311)
        } else if trimmed.eq_ignore_ascii_case("GENERIC") {
            Some(Self::GenericI2c)
        } else if trimmed.eq_ignore_ascii_case("I2S")
            || trimmed.eq_ignore_ascii_case("PCM5102")
        {
            Some(Self::I2sPassthrough)
        } else if trimmed.eq_ignore_ascii_case("SPDIF") {
            Some(Self::SpdifOut)
        } else {
            None
        }
    }

    /// Canonical `dac_config` model name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tas5805m => "TAS5805M",
            Self::Tas57xx => "TAS57XX",
            Self::Tas5713 => "TAS5713",
            Self::Ac101 => "AC101",
            Self::Wm8978 => "WM8978",
            Self::Es8388 => "ES8388",
            Self::Es8311 => "ES8311",
            Self::GenericI2c => "GENERIC",
            Self::I2sPassthrough => "I2S",
            Self::SpdifOut => "SPDIF",
        }
    }

    /// Default 7-bit I2C slave address (`None` for non-I2C models).
    #[must_use]
    pub const fn default_i2c_addr(self) -> Option<u8> {
        match self {
            Self::Tas5805m => Some(0x2D),
            Self::Tas57xx => Some(0x4C),
            Self::Tas5713 => Some(0x1B),
            Self::Ac101 | Self::Wm8978 => Some(0x1A),
            Self::Es8388 => Some(0x10),
            Self::Es8311 => Some(0x18),
            Self::GenericI2c => Some(0x20),
            Self::I2sPassthrough | Self::SpdifOut => None,
        }
    }

    /// Static I2C register `(reg, value)` initialization sequence.
    #[must_use]
    pub const fn init_sequence(self) -> &'static [(u8, u8)] {
        match self {
            Self::Tas5805m => INIT_TAS5805M,
            Self::Tas57xx => INIT_TAS57XX,
            Self::Tas5713 => INIT_TAS5713,
            Self::Ac101 => INIT_AC101,
            Self::Wm8978 => INIT_WM8978,
            Self::Es8388 => INIT_ES8388,
            Self::Es8311 => INIT_ES8311,
            Self::GenericI2c | Self::I2sPassthrough | Self::SpdifOut => INIT_GENERIC,
        }
    }

    /// Computes the `(reg, value)` I2C write for setting digital volume
    /// (`0..=100%`) on this DAC.
    #[must_use]
    pub const fn volume_register_pair(self, volume_pct: u8) -> Option<(u8, u8)> {
        let pct = if volume_pct > 100 { 100 } else { volume_pct };
        match self {
            Self::Tas5805m => {
                let code = if pct == 0 { 0xFF } else { 0x30 + (100 - pct) };
                Some((0x4C, code))
            }
            Self::Tas57xx => {
                let code = if pct == 0 { 0xFF } else { 0x30 + (100 - pct) };
                Some((0x3D, code))
            }
            Self::Tas5713 => {
                let code = if pct == 0 { 0xFF } else { 0x30 + (100 - pct) };
                Some((0x07, code))
            }
            Self::Ac101 => {
                let code = ((pct as u16) * 0xBF / 100) as u8;
                Some((0x43, code))
            }
            Self::Wm8978 => {
                let code = ((pct as u16) * 63 / 100) as u8;
                Some((0x34, code))
            }
            Self::Es8388 => {
                let atten = ((100 - pct) as u16 * 96 / 100) as u8;
                Some((0x1A, atten))
            }
            Self::Es8311 => {
                let code = ((pct as u16) * 0xBF / 100) as u8;
                Some((0x32, code))
            }
            Self::GenericI2c | Self::I2sPassthrough | Self::SpdifOut => None,
        }
    }

    /// Computes the `(reg, value)` I2C write for muting or unmuting this DAC.
    #[must_use]
    pub const fn mute_register_pair(self, muted: bool) -> Option<(u8, u8)> {
        match self {
            Self::Tas5805m => Some((0x03, if muted { 0x0B } else { 0x03 })),
            Self::Tas57xx => Some((0x03, if muted { 0x11 } else { 0x00 })),
            Self::Tas5713 => Some((0x06, if muted { 0x07 } else { 0x00 })),
            Self::Ac101 => Some((0x40, if muted { 0x01 } else { 0x00 })),
            Self::Wm8978 => Some((0x0A, if muted { 0x40 } else { 0x00 })),
            Self::Es8388 => Some((0x19, if muted { 0x06 } else { 0x02 })),
            Self::Es8311 => Some((0x31, if muted { 0x60 } else { 0x00 })),
            Self::GenericI2c | Self::I2sPassthrough | Self::SpdifOut => None,
        }
    }
}

/// Parsed `dac_config` NVS specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DacConfigSpec {
    /// Selected DAC hardware model.
    pub model: DacModel,
    /// I2S bit clock GPIO (`bck`).
    pub bck_pin: i8,
    /// I2S word select / LRCLK GPIO (`ws`).
    pub ws_pin: i8,
    /// I2S serial data out GPIO (`do`).
    pub do_pin: i8,
    /// Optional master clock GPIO (`mck`).
    pub mclk_pin: Option<i8>,
    /// Optional I2C SDA GPIO (`sda`).
    pub sda_pin: Option<i8>,
    /// Optional I2C SCL GPIO (`scl`).
    pub scl_pin: Option<i8>,
    /// Optional 7-bit I2C slave address (`i2c`).
    pub i2c_addr: Option<u8>,
    /// Optional amplifier hardware mute/enable GPIO (`mute`).
    pub mute_pin: Option<u8>,
    /// Polarity of `mute_pin` (`true` if active-low `:0`, `false` if `:1`).
    pub mute_active_low: bool,
}

impl DacConfigSpec {
    /// Parses a C-style `dac_config` key-value string such as:
    /// `"model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=33:0,i2c=45"`.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut model = DacModel::I2sPassthrough;
        let mut bck_pin: Option<i8> = None;
        let mut ws_pin: Option<i8> = None;
        let mut do_pin: Option<i8> = None;
        let mut mclk_pin: Option<i8> = None;
        let mut sda_pin: Option<i8> = None;
        let mut scl_pin: Option<i8> = None;
        let mut i2c_addr: Option<u8> = None;
        let mut mute_pin: Option<u8> = None;
        let mut mute_active_low = false;

        for token in spec.split(',') {
            let Some((k, v)) = token.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("model") {
                model = DacModel::from_name(val)?;
            } else if key.eq_ignore_ascii_case("bck") {
                bck_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("ws") {
                ws_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("do") {
                do_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("mck") || key.eq_ignore_ascii_case("mclk") {
                mclk_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("sda") {
                sda_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("scl") {
                scl_pin = parse_i8(val);
            } else if key.eq_ignore_ascii_case("i2c") {
                i2c_addr = parse_i8(val).map(|n| n as u8);
            } else if key.eq_ignore_ascii_case("mute") {
                if let Some((pin_str, pol_str)) = val.split_once(':') {
                    mute_pin = parse_i8(pin_str).map(|n| n as u8);
                    mute_active_low = pol_str.trim() == "0";
                } else {
                    mute_pin = parse_i8(val).map(|n| n as u8);
                }
            }
        }

        Some(Self {
            model,
            bck_pin: bck_pin?,
            ws_pin: ws_pin?,
            do_pin: do_pin?,
            mclk_pin,
            sda_pin,
            scl_pin,
            i2c_addr: i2c_addr.or_else(|| model.default_i2c_addr()),
            mute_pin,
            mute_active_low,
        })
    }
}

/// Parsed `spdif_config` NVS specification (`bck`, `ws`, `do`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpdifConfigSpec {
    /// I2S bit clock GPIO (`bck`).
    pub bck_pin: i8,
    /// I2S word select GPIO (`ws`).
    pub ws_pin: i8,
    /// S/PDIF data output GPIO (`do`).
    pub do_pin: i8,
}

impl SpdifConfigSpec {
    /// Parses a C-style `spdif_config` string (`"bck=26,ws=25,do=22"`).
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut bck: Option<i8> = None;
        let mut ws: Option<i8> = None;
        let mut dout: Option<i8> = None;
        for token in spec.split(',') {
            let Some((k, v)) = token.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim();
            if key.eq_ignore_ascii_case("bck") {
                bck = parse_i8(val);
            } else if key.eq_ignore_ascii_case("ws") {
                ws = parse_i8(val);
            } else if key.eq_ignore_ascii_case("do") {
                dout = parse_i8(val);
            }
        }
        Some(Self {
            bck_pin: bck?,
            ws_pin: ws?,
            do_pin: dout?,
        })
    }
}

fn parse_i8(s: &str) -> Option<i8> {
    let bytes = s.trim().as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (neg, start) = if bytes[0] == b'-' {
        (true, 1usize)
    } else {
        (false, 0usize)
    };
    if start >= bytes.len() {
        return None;
    }
    if bytes.len() >= start + 2
        && bytes[start] == b'0'
        && (bytes[start + 1] == b'x' || bytes[start + 1] == b'X')
    {
        let mut idx = start + 2;
        if idx == bytes.len() {
            return None;
        }
        let mut acc = 0i16;
        while idx < bytes.len() {
            let b = bytes[idx];
            let digit = match b {
                b'0'..=b'9' => (b - b'0') as i16,
                b'a'..=b'f' => (b - b'a' + 10) as i16,
                b'A'..=b'F' => (b - b'A' + 10) as i16,
                _ => return None,
            };
            acc = acc * 16 + digit;
            if acc > 127 {
                return None;
            }
            idx += 1;
        }
        return Some(if neg { -(acc as i8) } else { acc as i8 });
    }
    let mut acc = 0i16;
    let mut idx = start;
    while idx < bytes.len() {
        let b = bytes[idx];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc * 10 + ((b - b'0') as i16);
        if acc > 127 {
            return None;
        }
        idx += 1;
    }
    Some(if neg { -(acc as i8) } else { acc as i8 })
}

/// IEC 60958 / S/PDIF sync preamble type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpdifPreamble {
    /// `B` preamble: start of 192-frame block (frame 0, channel A / Left).
    BlockStartB,
    /// `M` preamble: channel A / Left (frames 1..=191).
    LeftM,
    /// `W` preamble: channel B / Right (all frames 0..=191).
    RightW,
}

impl SpdifPreamble {
    /// Returns the 8-cell biphase-mark sync pattern for the given starting
    /// bus polarity (`prev_high == false` means previous cell ended at `0`).
    #[must_use]
    pub const fn bmc_byte(self, prev_high: bool) -> u8 {
        let pos = match self {
            Self::BlockStartB => 0b1110_1000u8,
            Self::LeftM => 0b1110_0010u8,
            Self::RightW => 0b1110_0100u8,
        };
        if prev_high {
            !pos
        } else {
            pos
        }
    }
}

/// Software IEC 60958 / S/PDIF biphase-mark (BMC) encoder.
///
/// Converts 24-bit signed audio samples into 64-bit BMC subframes ready for
/// 2x-oversampled I2S DMA output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpdifBmcEncoder {
    frame_index: u8,
    last_cell_high: bool,
    channel_status_copy_permitted: bool,
}

impl Default for SpdifBmcEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SpdifBmcEncoder {
    /// Creates a new [`SpdifBmcEncoder`] at frame 0 of the 192-frame block.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frame_index: 0,
            last_cell_high: false,
            channel_status_copy_permitted: true,
        }
    }

    /// Returns the current frame index within the 192-frame IEC 60958 block.
    #[must_use]
    pub const fn frame_index(&self) -> u8 {
        self.frame_index
    }

    /// Encodes a single 32-time-slot S/PDIF subframe into a 64-bit BMC word.
    ///
    /// - Time-slots `0..=3`: 8-cell sync preamble (`B`, `M`, or `W`).
    /// - Time-slots `4..=27`: 24-bit signed audio sample (LSB first).
    /// - Time-slot `28`: Validity bit (`0` = valid linear PCM).
    /// - Time-slot `29`: User bit (`0`).
    /// - Time-slot `30`: Channel status bit (`C`).
    /// - Time-slot `31`: Even parity bit (`P`) ensuring time-slots `4..=31`
    ///   have an even number of logic `1` bits.
    pub fn encode_subframe(
        &mut self,
        preamble: SpdifPreamble,
        sample_24bit: i32,
        c_bit: bool,
    ) -> u64 {
        let sample_u24 = (sample_24bit as u32) & 0x00FF_FFFF;
        let mut payload_28 = sample_u24;
        if c_bit {
            payload_28 |= 1u32 << 26;
        }
        let ones = payload_28.count_ones();
        if (ones & 1) != 0 {
            payload_28 |= 1u32 << 27;
        }

        let preamble_bmc = preamble.bmc_byte(self.last_cell_high);
        let mut out_bmc = (preamble_bmc as u64) << 56;
        self.last_cell_high = (preamble_bmc & 1) != 0;

        let mut bit_idx = 0u32;
        while bit_idx < 28 {
            let bit_one = ((payload_28 >> bit_idx) & 1) != 0;
            let first_half = !self.last_cell_high;
            let second_half = if bit_one { !first_half } else { first_half };
            self.last_cell_high = second_half;

            let shift = 54 - (bit_idx * 2);
            let pair = ((first_half as u64) << 1) | (second_half as u64);
            out_bmc |= pair << shift;
            bit_idx += 1;
        }
        out_bmc
    }

    /// Encodes a stereo `(left, right)` 24-bit PCM frame into two 64-bit BMC
    /// subframes and advances the `0..192` IEC 60958 frame counter.
    pub fn encode_stereo_frame(&mut self, left_pcm24: i32, right_pcm24: i32) -> (u64, u64) {
        let left_preamble = if self.frame_index == 0 {
            SpdifPreamble::BlockStartB
        } else {
            SpdifPreamble::LeftM
        };
        let c_bit = self.frame_index == 2 && self.channel_status_copy_permitted;
        let left_bmc = self.encode_subframe(left_preamble, left_pcm24, c_bit);
        let right_bmc = self.encode_subframe(SpdifPreamble::RightW, right_pcm24, c_bit);
        self.frame_index = (self.frame_index + 1) % 192;
        (left_bmc, right_bmc)
    }
}

/// Speaker amplifier mute & `audo` output routing policy evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AudioOutputPolicy {
    audo_enabled: bool,
    jack_mutes_amp: bool,
    mute_active_low: bool,
}

impl AudioOutputPolicy {
    /// Creates a new [`AudioOutputPolicy`].
    #[must_use]
    pub const fn new(jack_mutes_amp: bool, mute_active_low: bool) -> Self {
        Self {
            audo_enabled: true,
            jack_mutes_amp,
            mute_active_low,
        }
    }

    /// Applies an LMS `audo` packet payload (`0` = disabled, non-zero = enabled).
    pub fn handle_audo_packet(&mut self, payload: &[u8]) {
        if let Some(&flag) = payload.first() {
            self.audo_enabled = flag != 0;
        }
    }

    /// Returns whether speaker output should be muted given current `audo`,
    /// headphone jack, and speaker fault states.
    #[must_use]
    pub const fn should_mute_speaker(&self, jack_inserted: bool, speaker_fault: bool) -> bool {
        if !self.audo_enabled || speaker_fault {
            return true;
        }
        if self.jack_mutes_amp && jack_inserted {
            return true;
        }
        false
    }

    /// Computes the physical GPIO level (`true` = High, `false` = Low) to drive
    /// on the amplifier mute/enable pin.
    #[must_use]
    pub const fn mute_gpio_level(&self, jack_inserted: bool, speaker_fault: bool) -> bool {
        let muted = self.should_mute_speaker(jack_inserted, speaker_fault);
        if self.mute_active_low {
            !muted
        } else {
            muted
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_dac_models_and_register_sequences() {
        let models = [
            ("TAS5805M", DacModel::Tas5805m, Some(0x2D)),
            ("TAS57XX", DacModel::Tas57xx, Some(0x4C)),
            ("TAS5713", DacModel::Tas5713, Some(0x1B)),
            ("AC101", DacModel::Ac101, Some(0x1A)),
            ("WM8978", DacModel::Wm8978, Some(0x1A)),
            ("ES8388", DacModel::Es8388, Some(0x10)),
            ("ES8311", DacModel::Es8311, Some(0x18)),
            ("GENERIC", DacModel::GenericI2c, Some(0x20)),
            ("I2S", DacModel::I2sPassthrough, None),
            ("SPDIF", DacModel::SpdifOut, None),
        ];
        for (name, expected, addr) in models {
            let m = DacModel::from_name(name).unwrap();
            assert_eq!(m, expected);
            assert_eq!(m.default_i2c_addr(), addr);
            if addr.is_some() && m != DacModel::GenericI2c {
                assert!(!m.init_sequence().is_empty());
                assert!(m.volume_register_pair(75).is_some());
                assert!(m.mute_register_pair(true).is_some());
            }
        }
        assert_eq!(
            DacModel::Tas5805m.volume_register_pair(0),
            Some((0x4C, 0xFF))
        );
        assert_eq!(
            DacModel::Tas5805m.volume_register_pair(100),
            Some((0x4C, 0x30))
        );
    }

    #[test]
    fn test_dac_config_and_spdif_config_parsers() {
        let spec = DacConfigSpec::parse(
            "model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=33:0,i2c=45",
        )
        .unwrap();
        assert_eq!(spec.model, DacModel::Tas5805m);
        assert_eq!(spec.bck_pin, 26);
        assert_eq!(spec.ws_pin, 25);
        assert_eq!(spec.do_pin, 22);
        assert_eq!(spec.sda_pin, Some(21));
        assert_eq!(spec.scl_pin, Some(27));
        assert_eq!(spec.mute_pin, Some(33));
        assert!(spec.mute_active_low);
        assert_eq!(spec.i2c_addr, Some(45));

        let spdif = SpdifConfigSpec::parse("bck=26,ws=25,do=22").unwrap();
        assert_eq!(spdif.bck_pin, 26);
        assert_eq!(spdif.ws_pin, 25);
        assert_eq!(spdif.do_pin, 22);
    }

    #[test]
    fn test_spdif_bmc_encoder_preambles_and_block_cycle() {
        let mut enc = SpdifBmcEncoder::new();
        assert_eq!(enc.frame_index(), 0);
        let (l0, r0) = enc.encode_stereo_frame(0x12_3456, -0x12_3456);
        assert_eq!((l0 >> 56) as u8, 0b1110_1000);
        assert_ne!(l0, r0);
        assert_eq!(enc.frame_index(), 1);

        let (l1, _r1) = enc.encode_stereo_frame(0, 0);
        let l1_preamble = (l1 >> 56) as u8;
        assert!(l1_preamble == 0b1110_0010 || l1_preamble == !0b1110_0010);

        for _ in 2..192 {
            let _ = enc.encode_stereo_frame(100, 100);
        }
        assert_eq!(enc.frame_index(), 0);
    }

    #[test]
    fn test_audio_output_policy_audo_and_jack_mute() {
        let mut policy = AudioOutputPolicy::new(true, true);
        assert!(!policy.should_mute_speaker(false, false));
        assert!(policy.mute_gpio_level(false, false));

        assert!(policy.should_mute_speaker(true, false));
        assert!(!policy.mute_gpio_level(true, false));

        policy.handle_audo_packet(&[0]);
        assert!(policy.should_mute_speaker(false, false));
        assert!(!policy.mute_gpio_level(false, false));
    }
}
