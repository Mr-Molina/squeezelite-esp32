//! Device [`AudioBackend`]: I2S0 clock dividers from the verified clock table, the TAS58xx
//! amplifier (volume, power) and the I2S DMA fill counter.
//!
//! # I2S0 clock registers (ESP32 TRM, chapter "I2S"; cross-checked against the `esp32` PAC and
//! esp-hal's ESP32 `set_clock`)
//!
//! | register | address | fields used |
//! |---|---|---|
//! | `I2S_CLKM_CONF_REG` | `0x3FF4_F0AC` | `I2S_CLKM_DIV_NUM` [7:0], `I2S_CLKM_DIV_B` [13:8], `I2S_CLKM_DIV_A` [19:14], `I2S_CLK_EN` [20], `I2S_CLKA_ENA` [21] (1 = APLL, 0 = `PLL_D2` 160 MHz) |
//! | `I2S_SAMPLE_RATE_CONF_REG` | `0x3FF4_F0B0` | `I2S_TX_BCK_DIV_NUM` [5:0] (`RX_BCK_DIV_NUM` [11:6] and the bit modes are left alone) |
//!
//! `BCK = 160 MHz / (DIV_NUM + DIV_B / DIV_A) / TX_BCK_DIV_NUM`. Both registers are written for
//! every rate: the pre-Task-9 firmware only rewrote `CLKM_CONF` and left `TX_BCK_DIV_NUM` at the
//! value esp-hal chose for 44.1 kHz, which put 88.2 kHz about 12,000 ppm off. The CLKM field
//! order (`DIV_B` below `DIV_A`) is the one the firmware has used since WO-109. A rate missing
//! from [`crate::clock_table`] is [`ClockError::UnsupportedRate`] and writes nothing (the old
//! silent 44.1 kHz fallback is gone).
//!
//! Register access is behind [`I2sClockRegisters`] so this module stays `unsafe`-free and
//! host-testable; the firmware implements it with volatile accesses in `main.rs`.

use core::sync::atomic::{AtomicU32, Ordering};

use embedded_hal::i2c::I2c;

use crate::amp::{AmpManager, AmpMode};
use crate::clock_table::{i2s_clock_config, I2sClockCfg};
use crate::io_traits::{AudioBackend, ClockError};

/// Address of `I2S_CLKM_CONF_REG` (I2S0).
pub const I2S0_CLKM_CONF_ADDR: usize = 0x3FF4_F0AC;
/// Address of `I2S_SAMPLE_RATE_CONF_REG` (I2S0).
pub const I2S0_SAMPLE_RATE_CONF_ADDR: usize = 0x3FF4_F0B0;

const CLKM_DIV_NUM_MASK: u32 = 0xFF;
const CLKM_DIV_B_SHIFT: u32 = 8;
const CLKM_DIV_A_SHIFT: u32 = 14;
const CLKM_FRAC_MASK: u32 = 0x3F;
const CLKM_CLK_EN: u32 = 1 << 20;
const CLKM_CLKA_ENA: u32 = 1 << 21;
const TX_BCK_DIV_MASK: u32 = 0x3F;

/// Read/write access to the two I2S0 clock registers.
pub trait I2sClockRegisters {
    fn read_clkm_conf(&mut self) -> u32;
    fn write_clkm_conf(&mut self, value: u32);
    fn read_sample_rate_conf(&mut self) -> u32;
    fn write_sample_rate_conf(&mut self, value: u32);
}

/// `I2S_CLKM_CONF_REG` value for `cfg`, keeping the bits above `I2S_CLKA_ENA` from `orig`.
pub const fn encode_clkm_conf(orig: u32, cfg: &I2sClockCfg) -> u32 {
    let source = if cfg.use_apll { CLKM_CLKA_ENA } else { 0 };
    (orig & !(CLKM_DIV_NUM_MASK
        | (CLKM_FRAC_MASK << CLKM_DIV_B_SHIFT)
        | (CLKM_FRAC_MASK << CLKM_DIV_A_SHIFT)
        | CLKM_CLK_EN
        | CLKM_CLKA_ENA))
        | (cfg.clkm_div_num as u32 & CLKM_DIV_NUM_MASK)
        | ((cfg.clkm_div_b as u32 & CLKM_FRAC_MASK) << CLKM_DIV_B_SHIFT)
        | ((cfg.clkm_div_a as u32 & CLKM_FRAC_MASK) << CLKM_DIV_A_SHIFT)
        | CLKM_CLK_EN
        | source
}

/// `I2S_SAMPLE_RATE_CONF_REG` value with `I2S_TX_BCK_DIV_NUM = bck_div`, other fields from
/// `orig`.
pub const fn encode_tx_bck_div(orig: u32, bck_div: u8) -> u32 {
    (orig & !TX_BCK_DIV_MASK) | (bck_div as u32 & TX_BCK_DIV_MASK)
}

/// Programs I2S0 for `rate_hz` (dividers and bit-clock divider) and returns the configuration
/// used. Unsupported rates write nothing.
pub fn program_i2s_clock<R: I2sClockRegisters>(
    regs: &mut R,
    rate_hz: u32,
) -> Result<I2sClockCfg, ClockError> {
    let cfg = i2s_clock_config(rate_hz).ok_or(ClockError::UnsupportedRate(rate_hz))?;
    let clkm = encode_clkm_conf(regs.read_clkm_conf(), &cfg);
    regs.write_clkm_conf(clkm);
    let rate_conf = encode_tx_bck_div(regs.read_sample_rate_conf(), cfg.bck_div);
    regs.write_sample_rate_conf(rate_conf);
    Ok(cfg)
}

/// The device [`AudioBackend`], built for the duration of one coordinator tick.
///
/// * `set_sample_rate`: [`program_i2s_clock`].
/// * `set_hw_volume`: one TAS58xx digital volume code for both channels (the louder of the
///   two, i.e. the smaller attenuation code), written only when it changes.
/// * `set_mute`: no amplifier action (the I2S sink mute already silences the output; the
///   pre-Task-9 firmware never touched the amplifier on pause), recorded for inspection.
/// * `set_power(true)`: if the amplifier is not playing, clear faults and start it via Hi-Z
///   (what the firmware did on `strm s`); `set_power(false)`: stop playback (DAC disabled).
/// * `dma_outstanding_frames`: the counter the I2S DMA driver keeps updated.
pub struct Esp32AudioBackend<'a, I, R> {
    amp: &'a mut AmpManager<I>,
    regs: R,
    dma_outstanding: &'a AtomicU32,
    muted: bool,
}

impl<'a, I: I2c, R: I2sClockRegisters> Esp32AudioBackend<'a, I, R> {
    pub fn new(amp: &'a mut AmpManager<I>, regs: R, dma_outstanding: &'a AtomicU32) -> Self {
        Self { amp, regs, dma_outstanding, muted: false }
    }

    /// Last mute request (`set_mute`).
    pub fn muted(&self) -> bool {
        self.muted
    }
}

impl<I: I2c, R: I2sClockRegisters> AudioBackend for Esp32AudioBackend<'_, I, R> {
    fn set_sample_rate(&mut self, hz: u32) -> Result<(), ClockError> {
        program_i2s_clock(&mut self.regs, hz).map(|_| ())
    }

    fn set_hw_volume(&mut self, left: u8, right: u8) {
        // TAS58xx `DIG_VOL_CTRL`: a larger code attenuates more; one code drives both channels.
        let code = left.min(right);
        if self.amp.current_volume() != code {
            let _ = self.amp.set_volume(code);
        }
    }

    fn set_mute(&mut self, mute: bool) {
        self.muted = mute;
    }

    fn set_power(&mut self, on: bool) {
        if on {
            if self.amp.mode() != AmpMode::Play {
                let _ = self.amp.clear_faults();
                let _ = self.amp.start_playback_via_hiz();
            }
        } else {
            let _ = self.amp.stop_playback();
        }
    }

    fn dma_outstanding_frames(&self) -> u32 {
        self.dma_outstanding.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock_table::VERIFIED_RATES;
    use crate::hal::TargetI2cBus;

    /// Register file with the reset values of the two registers plus a write log.
    struct FakeRegs {
        clkm: u32,
        rate_conf: u32,
        writes: usize,
    }

    impl FakeRegs {
        fn new() -> Self {
            // Upper CLKM bits set to check they survive; RX BCK 6, bit modes 16 (reset values).
            Self { clkm: 0xFFC0_0000 | CLKM_CLKA_ENA, rate_conf: (16 << 18) | (16 << 12) | (6 << 6) | 6, writes: 0 }
        }
    }

    impl I2sClockRegisters for FakeRegs {
        fn read_clkm_conf(&mut self) -> u32 {
            self.clkm
        }
        fn write_clkm_conf(&mut self, value: u32) {
            self.clkm = value;
            self.writes += 1;
        }
        fn read_sample_rate_conf(&mut self) -> u32 {
            self.rate_conf
        }
        fn write_sample_rate_conf(&mut self, value: u32) {
            self.rate_conf = value;
            self.writes += 1;
        }
    }

    #[test]
    fn test_every_verified_rate_programs_clkm_and_bck_divider() {
        for &rate in VERIFIED_RATES {
            let mut regs = FakeRegs::new();
            let cfg = program_i2s_clock(&mut regs, rate).unwrap();
            assert_eq!(cfg, i2s_clock_config(rate).unwrap());
            assert_eq!(regs.clkm & 0xFF, u32::from(cfg.clkm_div_num), "{rate}: DIV_NUM [7:0]");
            assert_eq!((regs.clkm >> 8) & 0x3F, u32::from(cfg.clkm_div_b), "{rate}: DIV_B [13:8]");
            assert_eq!((regs.clkm >> 14) & 0x3F, u32::from(cfg.clkm_div_a), "{rate}: DIV_A [19:14]");
            assert_ne!(regs.clkm & (1 << 20), 0, "{rate}: CLK_EN");
            assert_eq!(regs.clkm & (1 << 21), 0, "{rate}: PLL_D2 source (CLKA_ENA clear)");
            assert_eq!(regs.clkm & 0xFFC0_0000, 0xFFC0_0000, "{rate}: upper bits kept");
            assert_eq!(regs.rate_conf & 0x3F, u32::from(cfg.bck_div), "{rate}: TX_BCK_DIV_NUM [5:0]");
            assert_eq!(regs.rate_conf >> 6, (16 << 12) | (16 << 6) | 6, "{rate}: other fields kept");
            let expected_bck = if rate <= 48_000 { 4 } else { 2 };
            assert_eq!(cfg.bck_div, expected_bck, "{rate}");
        }
    }

    #[test]
    fn test_88k2_writes_bck_div_2_not_the_44k1_divider() {
        let mut regs = FakeRegs::new();
        program_i2s_clock(&mut regs, 44_100).unwrap();
        assert_eq!(regs.rate_conf & 0x3F, 4);
        program_i2s_clock(&mut regs, 88_200).unwrap();
        assert_eq!(regs.rate_conf & 0x3F, 2, "88.2 kHz needs BCK/2 with the 44.1 kHz CLKM dividers");
        assert_eq!(regs.clkm & 0xFF, 14);
    }

    #[test]
    fn test_unsupported_rate_is_an_error_and_writes_nothing() {
        let mut regs = FakeRegs::new();
        let before = (regs.clkm, regs.rate_conf);
        for rate in [0, 7_999, 44_000, 64_000, 384_000] {
            assert_eq!(program_i2s_clock(&mut regs, rate), Err(ClockError::UnsupportedRate(rate)));
        }
        assert_eq!((regs.clkm, regs.rate_conf, regs.writes), (before.0, before.1, 0));
    }

    #[test]
    fn test_encoders_match_the_legacy_clkm_layout() {
        let cfg = I2sClockCfg { clkm_div_num: 6, clkm_div_a: 2, clkm_div_b: 1, bck_div: 2, use_apll: false };
        let v = encode_clkm_conf(0x1230_0000, &cfg);
        assert_eq!(v & 0xFF, 6);
        assert_eq!((v >> 8) & 0x3F, 1);
        assert_eq!((v >> 14) & 0x3F, 2);
        assert_eq!(v & 0xFFC0_0000, 0x1200_0000, "bits 22+ kept");
        assert_eq!(encode_tx_bck_div(0xFFFF_FFFF, 4), 0xFFFF_FFC4);
    }

    fn amp() -> AmpManager<TargetI2cBus> {
        let mut amp = AmpManager::for_active_board(TargetI2cBus::new());
        amp.initialize(32).unwrap();
        amp
    }

    #[test]
    fn test_backend_volume_power_mute_and_dma_counter() {
        let mut amp = amp();
        let dma = AtomicU32::new(0);
        {
            let mut b = Esp32AudioBackend::new(&mut amp, FakeRegs::new(), &dma);
            assert_eq!(b.set_sample_rate(96_000), Ok(()));
            assert_eq!(b.set_sample_rate(12_345), Err(ClockError::UnsupportedRate(12_345)));
            b.set_hw_volume(0x40, 0x39);
            b.set_power(true);
            b.set_mute(true);
            assert!(b.muted());
            dma.store(321, Ordering::Relaxed);
            assert_eq!(b.dma_outstanding_frames(), 321);
        }
        assert_eq!(amp.current_volume(), 0x39, "louder channel wins");
        assert_eq!(amp.mode(), AmpMode::Play);

        Esp32AudioBackend::new(&mut amp, FakeRegs::new(), &dma).set_power(false);
        assert_ne!(amp.mode(), AmpMode::Play);
    }
}
