//! 10-Band Biquad Parametric Equalizer & Horner Loudness Evaluator
//!
//! Ported and modernized from the Milestone MK-5 optimized C equalizer,
//! replacing transcendentals with fast $O(n)$ Horner polynomial evaluations.

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

pub struct Equalizer<const BANDS: usize> {
    pub filters: [BiquadCoeffs; BANDS],
    hist_left: [(f32, f32); BANDS],
    hist_right: [(f32, f32); BANDS],
    pub enabled: bool,
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

    /// Reset internal filter state history without resetting coefficients
    pub fn reset_history(&mut self) {
        self.hist_left = [(0.0, 0.0); BANDS];
        self.hist_right = [(0.0, 0.0); BANDS];
    }

    /// Process interleaved stereo 32-bit audio samples in-place
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn process_interleaved(&mut self, samples: &mut [i32]) {
        if !self.enabled {
            return;
        }

        for chunk in samples.chunks_exact_mut(2) {
            let mut l = chunk[0] as f32;
            let mut r = chunk[1] as f32;

            for i in 0..BANDS {
                let f = &self.filters[i];
                let (z1_l, z2_l) = &mut self.hist_left[i];
                let out_l = f.b0 * l + *z1_l;
                *z1_l = f.b1 * l - f.a1 * out_l + *z2_l;
                *z2_l = f.b2 * l - f.a2 * out_l;
                l = out_l;

                let (z1_r, z2_r) = &mut self.hist_right[i];
                let out_r = f.b0 * r + *z1_r;
                *z1_r = f.b1 * r - f.a1 * out_r + *z2_r;
                *z2_r = f.b2 * r - f.a2 * out_r;
                r = out_r;
            }

            chunk[0] = l.clamp(i32::MIN as f32, i32::MAX as f32) as i32;
            chunk[1] = r.clamp(i32::MIN as f32, i32::MAX as f32) as i32;
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
        eq.filters[0] = BiquadCoeffs {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        };
        let mut samples = [50000i32, -50000i32];
        eq.process_interleaved(&mut samples);
        assert_eq!(samples, [50000i32, -50000i32]);
    }

    #[test]
    fn test_horner_loudness_range() {
        assert!((horner_loudness(0.0) - 1.0).abs() < 1e-4);
        let max_vol = horner_loudness(1.0);
        assert!(max_vol > 0.6 && max_vol < 0.7);
    }
}
