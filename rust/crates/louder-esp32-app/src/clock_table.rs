//! Verified I2S clock-divider table for 32-bit-slot stereo (BCK = rate x 64).
//!
//! Pure, host-testable and `no_std`-compatible; the device-side register write
//! lives in the app binary and consumes [`i2s_clock_config`].
//!
//! # Clock model (ESP32, I2S0)
//!
//! * Source `PLL_D2` = 160 MHz.
//! * `I2S0 clock = 160 MHz / (clkm_div_num + clkm_div_b / clkm_div_a)`
//!   (`div_num` 8-bit and >= 2, `div_a`/`div_b` 6-bit, `div_b < div_a`).
//! * `BCK = I2S0 clock / bck_div` (`bck_div` >= 2).
//!
//! # Per-rate result (all on the 160 MHz fractional path)
//!
//! Dividers were found by exhaustive search over `bck_div` and the 6-bit
//! fractional pair, minimising |BCK error|, then preferring the smaller
//! `div_a`. `bck_div` is 4 up to 48 kHz (the value the previous inline table
//! in `main.rs` assumed) and 2 from 88.2 kHz up, where `bck_div` 4 would leave
//! the integer divider too small for good resolution.
//!
//! | rate    | bck_div | div_num | div_b/div_a | BCK error |
//! |---------|---------|---------|-------------|-----------|
//! | 8000    | 4       | 78      | 1/8         | 0 ppm     |
//! | 11025   | 4       | 56      | 20/29       | 5.52 ppm  |
//! | 12000   | 4       | 52      | 1/12        | 0 ppm     |
//! | 16000   | 4       | 39      | 1/16        | 0 ppm     |
//! | 22050   | 4       | 28      | 10/29       | 5.52 ppm  |
//! | 24000   | 4       | 26      | 1/24        | 0 ppm     |
//! | 32000   | 4       | 19      | 17/32       | 0 ppm     |
//! | 44100   | 4       | 14      | 5/29        | 5.52 ppm  |
//! | 48000   | 4       | 13      | 1/48        | 0 ppm     |
//! | 88200   | 2       | 14      | 5/29        | 5.52 ppm  |
//! | 96000   | 2       | 13      | 1/48        | 0 ppm     |
//! | 176400  | 2       | 7       | 5/58        | 5.52 ppm  |
//! | 192000  | 2       | 6       | 25/49       | 32.65 ppm |
//!
//! # APLL: not needed for any listed rate
//!
//! The expectation going in was that 176.4 kHz, 192 kHz and the 11.025 kHz
//! family would need the audio PLL. The search shows otherwise: with a 6-bit
//! `div_a` the 160 MHz path reaches every rate within 33 ppm, so no entry sets
//! `use_apll`. `I2sClockCfg::use_apll` stays in the type so a later device task
//! can add APLL entries (the APLL register math is out of scope here) if
//! hardware measurement shows the fractional path's jitter is unacceptable.
//! 192 kHz alone has a zero-error alternative on the PLL path
//! (`bck_div` 5, `div_num` 2, 29/48) but its integer divider sits at the
//! hardware minimum, so the safer `bck_div` 2 entry above is used.
//!
//! # Corrections to the previous inline table
//!
//! Evaluated at `bck_div` 4, the old `main.rs` dividers were off by
//! 160 ppm (11.025/22.05/44.1 kHz), about 1597 ppm (96 kHz) and about
//! 12326 ppm (88.2 kHz), and every unlisted rate (e.g. 176.4/192 kHz, and
//! anything else) silently fell back to the 44.1 kHz divider.
//! [`i2s_clock_config`] returns `None` for unknown rates instead.
//!
//! # Hardware gate
//!
//! `VERIFIED_RATES` is pending hardware BCK measurement at Task 18 (44.1, 48,
//! 96, 176.4, 192 kHz); unverifiable rates get dropped from it then.

/// I2S0 clock configuration for one sample rate (32-bit slots, stereo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I2sClockCfg {
    /// Integral divider (`I2S_CLKM_DIV_NUM`, >= 2).
    pub clkm_div_num: u8,
    /// Fractional denominator (`I2S_CLKM_DIV_A`, 1..=63).
    pub clkm_div_a: u8,
    /// Fractional numerator (`I2S_CLKM_DIV_B`, < `clkm_div_a`).
    pub clkm_div_b: u8,
    /// Bit-clock divider (`I2S_TX_BCK_DIV_NUM`, >= 2).
    pub bck_div: u8,
    /// Use the audio PLL instead of the 160 MHz `PLL_D2` source.
    pub use_apll: bool,
}

const fn pll(clkm_div_num: u8, clkm_div_b: u8, clkm_div_a: u8, bck_div: u8) -> I2sClockCfg {
    I2sClockCfg {
        clkm_div_num,
        clkm_div_a,
        clkm_div_b,
        bck_div,
        use_apll: false,
    }
}

/// Sample rates with a divider entry. Pending hardware BCK measurement at Task 18.
pub const VERIFIED_RATES: &[u32] = &[
    8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400,
    192_000,
];

/// Highest rate in [`VERIFIED_RATES`].
pub fn max_verified_rate() -> u32 {
    let mut max = 0;
    for &r in VERIFIED_RATES {
        if r > max {
            max = r;
        }
    }
    max
}

/// Divider configuration for `rate_hz`, or `None` for a rate not in the table.
pub fn i2s_clock_config(rate_hz: u32) -> Option<I2sClockCfg> {
    Some(match rate_hz {
        8_000 => pll(78, 1, 8, 4),
        11_025 => pll(56, 20, 29, 4),
        12_000 => pll(52, 1, 12, 4),
        16_000 => pll(39, 1, 16, 4),
        22_050 => pll(28, 10, 29, 4),
        24_000 => pll(26, 1, 24, 4),
        32_000 => pll(19, 17, 32, 4),
        44_100 => pll(14, 5, 29, 4),
        48_000 => pll(13, 1, 48, 4),
        88_200 => pll(14, 5, 29, 2),
        96_000 => pll(13, 1, 48, 2),
        176_400 => pll(7, 5, 58, 2),
        192_000 => pll(6, 25, 49, 2),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_RATES: [u32; 13] = [
        8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000,
        176_400, 192_000,
    ];

    fn bck_hz(cfg: &I2sClockCfg) -> f64 {
        let div =
            f64::from(cfg.clkm_div_num) + f64::from(cfg.clkm_div_b) / f64::from(cfg.clkm_div_a);
        160_000_000.0 / div / f64::from(cfg.bck_div)
    }

    /// Best achievable |BCK error| in ppm on the 160 MHz path over all dividers.
    fn best_pll_d2_ppm(rate: u32) -> f64 {
        let target = f64::from(rate) * 64.0;
        let mut best = f64::MAX;
        for bck_div in 2..=63u32 {
            for num in 2..=255u32 {
                for a in 1..=63u32 {
                    for b in 0..a {
                        let bck = 160_000_000.0
                            / (f64::from(num) + f64::from(b) / f64::from(a))
                            / f64::from(bck_div);
                        best = best.min((bck - target).abs() / target * 1e6);
                    }
                }
            }
        }
        best
    }

    #[test]
    fn test_clock_config_exact_rates() {
        for &r in &ALL_RATES {
            let cfg = i2s_clock_config(r).unwrap_or_else(|| panic!("no config for {r}"));
            let target = f64::from(r) * 64.0;
            if cfg.use_apll {
                // APLL entries exist only because no PLL_D2 divider reaches 100 ppm.
                assert!(
                    best_pll_d2_ppm(r) > 100.0,
                    "{r}: PLL_D2 suffices, APLL unneeded"
                );
                continue;
            }
            assert!(cfg.clkm_div_num >= 2, "{r}: div_num must be >= 2");
            assert!(
                cfg.clkm_div_a >= 1 && cfg.clkm_div_a <= 63,
                "{r}: div_a 6-bit"
            );
            assert!(
                cfg.clkm_div_b < cfg.clkm_div_a || cfg.clkm_div_b == 0,
                "{r}: div_b < div_a"
            );
            assert!(cfg.bck_div >= 2, "{r}: bck_div must be >= 2");
            let ppm = (bck_hz(&cfg) - target).abs() / target * 1e6;
            assert!(ppm <= 100.0, "{r}: {ppm} ppm");
        }
    }

    #[test]
    fn test_unknown_rate_is_none() {
        assert_eq!(i2s_clock_config(44_101), None);
        assert_eq!(i2s_clock_config(0), None);
        assert_eq!(i2s_clock_config(384_000), None);
    }

    #[test]
    fn test_verified_rates_consistent() {
        for &r in VERIFIED_RATES {
            assert!(
                i2s_clock_config(r).is_some(),
                "{r} verified but has no config"
            );
        }
        assert_eq!(max_verified_rate(), *VERIFIED_RATES.iter().max().unwrap());
        assert_eq!(max_verified_rate(), 192_000);
    }
}
