//! 10-Band Biquad Parametric Equalizer & Horner Loudness Evaluator
//!
//! Ported and modernized from the Milestone MK-5 optimized C equalizer,
//! replacing transcendentals with fast $O(n)$ Horner polynomial evaluations,
//! normalized floating-point processing, and Schur-Cohn stability verification.


/// Filter coefficients for a second-order IIR biquad filter section:
/// $H(z) = \frac{b_0 + b_1 z^{-1} + b_2 z^{-2}}{1 + a_1 z^{-1} + a_2 z^{-2}}$
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for BiquadCoeffs {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl BiquadCoeffs {
    /// Verifies Bounded-Input Bounded-Output (BIBO) stability under the
    /// Schur-Cohn / Jury stability criterion:
    /// 1. All numerator and denominator coefficients are finite (rejecting NaN and Inf).
    /// 2. $|a_2| < 1.0$
    /// 3. $|a_1| < 1.0 + a_2$
    pub fn is_stable(&self) -> bool {
        if !self.b0.is_finite()
            || !self.b1.is_finite()
            || !self.b2.is_finite()
            || !self.a1.is_finite()
            || !self.a2.is_finite()
        {
            return false;
        }
        self.a2.abs() < 1.0 && self.a1.abs() < (1.0 + self.a2)
    }
}

pub struct Equalizer<const BANDS: usize> {
    filters: [BiquadCoeffs; BANDS],
    hist_left: [(f32, f32); BANDS],
    hist_right: [(f32, f32); BANDS],
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqualizerError {
    BandOutOfBounds,
    UnstableCoefficients,
}

impl<const BANDS: usize> Equalizer<BANDS> {
    pub const fn new() -> Self {
        Self {
            filters: [BiquadCoeffs {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            }; BANDS],
            hist_left: [(0.0, 0.0); BANDS],
            hist_right: [(0.0, 0.0); BANDS],
            enabled: false,
        }
    }

    /// Returns a reference to the array of filter coefficients for all bands.
    pub fn filters(&self) -> &[BiquadCoeffs; BANDS] {
        &self.filters
    }

    /// Safely updates a filter band's coefficients after verifying stability.
    pub fn set_filter(&mut self, band: usize, coeffs: BiquadCoeffs) -> Result<(), EqualizerError> {
        if band >= BANDS {
            return Err(EqualizerError::BandOutOfBounds);
        }
        if !coeffs.is_stable() {
            return Err(EqualizerError::UnstableCoefficients);
        }
        self.filters[band] = coeffs;
        Ok(())
    }

    /// Reset internal filter state history without resetting coefficients.
    pub fn reset_history(&mut self) {
        self.hist_left = [(0.0, 0.0); BANDS];
        self.hist_right = [(0.0, 0.0); BANDS];
    }

    /// Process interleaved stereo 32-bit audio samples in-place.
    /// Operates in normalized $[-1.0, 1.0]$ float domain with denormal flushing,
    /// NaN state isolation, and accurate 32-bit boundary saturation.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn process_interleaved(&mut self, samples: &mut [i32]) {
        if !self.enabled {
            return;
        }

        const NORM: f32 = 1.0 / 2147483648.0;
        const DENORM: f32 = 2147483648.0;
        const MAX_F32_I32: f32 = 2147483520.0; // Largest representable f32 <= i32::MAX
        const MIN_F32_I32: f32 = i32::MIN as f32;

        let mut chunks = samples.chunks_exact_mut(2);
        for chunk in &mut chunks {
            let mut l = chunk[0] as f32 * NORM;
            let mut r = chunk[1] as f32 * NORM;

            for i in 0..BANDS {
                let f = &self.filters[i];

                // Left channel biquad (Direct Form II Transposed)
                let (z1_l, z2_l) = &mut self.hist_left[i];
                let mut out_l = f.b0 * l + *z1_l;
                if !out_l.is_finite() {
                    *z1_l = 0.0;
                    *z2_l = 0.0;
                    out_l = 0.0;
                } else {
                    *z1_l = f.b1 * l - f.a1 * out_l + *z2_l;
                    *z2_l = f.b2 * l - f.a2 * out_l;
                    // Flush subnormal floats to prevent CPU traps (DSP-006)
                    if z1_l.abs() < 1e-15 { *z1_l = 0.0; }
                    if z2_l.abs() < 1e-15 { *z2_l = 0.0; }
                }
                l = out_l;

                // Right channel biquad (Direct Form II Transposed)
                let (z1_r, z2_r) = &mut self.hist_right[i];
                let mut out_r = f.b0 * r + *z1_r;
                if !out_r.is_finite() {
                    *z1_r = 0.0;
                    *z2_r = 0.0;
                    out_r = 0.0;
                } else {
                    *z1_r = f.b1 * r - f.a1 * out_r + *z2_r;
                    *z2_r = f.b2 * r - f.a2 * out_r;
                    if z1_r.abs() < 1e-15 { *z1_r = 0.0; }
                    if z2_r.abs() < 1e-15 { *z2_r = 0.0; }
                }
                r = out_r;
            }

            let scaled_l = l * DENORM;
            let scaled_r = r * DENORM;
            chunk[0] = if scaled_l.is_nan() { 0 } else { scaled_l.clamp(MIN_F32_I32, MAX_F32_I32) as i32 };
            chunk[1] = if scaled_r.is_nan() { 0 } else { scaled_r.clamp(MIN_F32_I32, MAX_F32_I32) as i32 };
        }

        // Remainder guard: zero out trailing odd sample to prevent channel phase swap (DSP-007)
        let rem = chunks.into_remainder();
        if let Some(last) = rem.first_mut() {
            *last = 0;
        }
    }
}

impl<const BANDS: usize> Default for Equalizer<BANDS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Evaluates loudness compensation curve using Horner's polynomial method:
/// $P(x) = c_0 + x(c_1 + x(c_2 + x \cdot c_3))$
pub fn horner_loudness(volume_level: f32) -> f32 {
    if volume_level.is_nan() {
        return 1.0;
    }
    const C0: f32 = 1.0;
    const C1: f32 = -0.45;
    const C2: f32 = 0.12;
    const C3: f32 = -0.02;

    let x = volume_level.clamp(0.0, 1.0);
    C0 + x * (C1 + x * (C2 + x * C3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equalizer_passthrough_when_disabled() {
        let mut eq = Equalizer::<10>::new();
        eq.enabled = false;
        let mut samples = [1000i32, -2000i32, 3000i32, -4000i32];
        let original = samples;
        eq.process_interleaved(&mut samples);
        assert_eq!(samples, original);
    }

    #[test]
    fn test_equalizer_identity_filtering() {
        let mut eq = Equalizer::<1>::new();
        eq.enabled = true;
        // Identity biquad: b0=1, all other coeffs 0
        eq.set_filter(
            0,
            BiquadCoeffs {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            },
        )
        .unwrap();
        assert_eq!(eq.filters()[0].b0, 1.0);
        let mut samples = [50000i32, -50000i32];
        eq.process_interleaved(&mut samples);
        assert_eq!(samples, [50000i32, -50000i32]);
    }

    #[test]
    fn test_biquad_stability_check() {
        let stable = BiquadCoeffs {
            b0: 0.5,
            b1: 0.0,
            b2: 0.0,
            a1: -0.5,
            a2: 0.2,
        };
        assert!(stable.is_stable());

        let unstable = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: -2.05,
            a2: 1.05,
        };
        assert!(!unstable.is_stable());
    }

    #[test]
    fn test_biquad_stability_nan_and_infinity_rejection() {
        let nan_coeff = BiquadCoeffs {
            b0: f32::NAN,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        };
        assert!(!nan_coeff.is_stable());

        let nan_a1 = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: f32::NAN,
            a2: 0.0,
        };
        assert!(!nan_a1.is_stable());

        let inf_a2 = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: f32::INFINITY,
        };
        assert!(!inf_a2.is_stable());

        let neg_inf_b1 = BiquadCoeffs {
            b0: 1.0,
            b1: f32::NEG_INFINITY,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        };
        assert!(!neg_inf_b1.is_stable());
    }

    #[test]
    fn test_biquad_stability_unit_circle_boundary_rejection() {
        // Boundary case: a2 = 1.0 (|a2| < 1.0 fails)
        let boundary_a2 = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.5,
            a2: 1.0,
        };
        assert!(!boundary_a2.is_stable());

        // Boundary case: a2 = -1.0 (|a2| < 1.0 fails)
        let boundary_neg_a2 = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: -1.0,
        };
        assert!(!boundary_neg_a2.is_stable());

        // Boundary case: a1 = 1.5, a2 = 0.5 (|a1| < 1.0 + a2 fails: 1.5 < 1.5 is false)
        let boundary_triangle = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 1.5,
            a2: 0.5,
        };
        assert!(!boundary_triangle.is_stable());
    }

    #[test]
    fn test_nan_resilience_in_equalizer() {
        let mut eq = Equalizer::<1>::new();
        eq.enabled = true;
        let mut samples = [i32::MAX, i32::MIN];
        eq.process_interleaved(&mut samples);
        // Ensure no panic or NaN explosion
        assert!(samples[0] != 0 && samples[1] != 0);
    }

    #[test]
    fn test_horner_loudness_range() {
        assert!((horner_loudness(0.0) - 1.0).abs() < 1e-4);
        let max_vol = horner_loudness(1.0);
        assert!(max_vol > 0.6 && max_vol < 0.7);
        assert_eq!(horner_loudness(f32::NAN), 1.0);
    }
}
