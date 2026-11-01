//! Fixed-Point Polyphase Windowed-Sinc Stereo Resampler
//!
//! Provides a `no_std` polyphase Kaiser windowed-sinc sample rate converter
//! operating on interleaved 32-bit stereo PCM (`[L, R, ...]`). Filter tables
//! and delay history are heap-allocated in PSRAM/DRAM so the stack footprint of
//! `Resampler` is `<= 64` bytes, using exact rational phase accumulation
//! (`from_hz / to_hz`) to eliminate long-term clock drift.

extern crate alloc;

use alloc::boxed::Box;

const NUM_PHASES: usize = 96;
const PHASE_ROWS: usize = NUM_PHASES + 1;
const MAX_TAPS: usize = 32;
const HISTORY_CAP: usize = MAX_TAPS * 2;
const ONE_Q30: i64 = 1_i64 << 30;
const HALF_Q30: i64 = 1_i64 << 29;

/// Filter tap-length and stopband attenuation tier for [`Resampler`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResampleQuality {
    /// Compact 8-tap polyphase filter (`SNR >= 55 dB`).
    Basic,
    /// Intermediate 16-tap polyphase filter (`SNR >= 75 dB`).
    Low,
    /// High-precision 32-tap Kaiser ($\beta = 10.0$) filter (`SNR >= 90 dB`).
    #[default]
    Medium,
}

impl ResampleQuality {
    /// Returns the number of FIR taps per polyphase branch for this quality.
    #[must_use]
    pub const fn taps(self) -> usize {
        match self {
            Self::Basic => 8,
            Self::Low => 16,
            Self::Medium => 32,
        }
    }

    const fn kaiser_beta(self) -> f64 {
        match self {
            Self::Basic => 5.0,
            Self::Low => 7.5,
            Self::Medium => 10.0,
        }
    }

    const fn cutoff_scale(self) -> f64 {
        match self {
            Self::Basic => 0.42,
            Self::Low => 0.435,
            Self::Medium => 0.445,
        }
    }
}

/// Phase table evaluation mode between adjacent polyphase branches.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpolationMode {
    /// Nearest-phase lookup without inter-branch linear interpolation.
    QuantizedPhase,
    /// 16-bit linear interpolation between adjacent polyphase branches.
    #[default]
    LinearPhase,
}

impl From<bool> for InterpolationMode {
    fn from(interpolate: bool) -> Self {
        if interpolate {
            Self::LinearPhase
        } else {
            Self::QuantizedPhase
        }
    }
}

/// Fixed-point polyphase Kaiser windowed-sinc stereo resampler.
#[derive(Clone)]
pub struct Resampler {
    coeffs_q30: Box<[[i32; MAX_TAPS]]>,
    history: Box<[[i32; 2]]>,
    phase_num: u64,
    from_hz: u32,
    to_hz: u32,
    need_input_frames: u32,
    taps: u8,
    hist_pos: u8,
    flush_remaining: u8,
    quality: ResampleQuality,
    interp: InterpolationMode,
}

impl core::fmt::Debug for Resampler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Resampler")
            .field("from_hz", &self.from_hz)
            .field("to_hz", &self.to_hz)
            .field("quality", &self.quality)
            .field("interp", &self.interp)
            .field("taps", &self.taps)
            .field("phase_num", &self.phase_num)
            .field("need_input_frames", &self.need_input_frames)
            .field("flush_remaining", &self.flush_remaining)
            .finish()
    }
}

impl Resampler {
    /// Constructs a new polyphase stereo resampler converting from `from_hz`
    /// to `to_hz` at the specified `quality` and `interp` mode.
    ///
    /// Accepts [`InterpolationMode`] or `bool` via `Into<InterpolationMode>`.
    #[must_use]
    pub fn new(
        from_hz: u32,
        to_hz: u32,
        quality: ResampleQuality,
        interp: impl Into<InterpolationMode>,
    ) -> Self {
        let safe_from = from_hz.max(1);
        let safe_to = to_hz.max(1);
        let interp_mode = interp.into();
        let taps = quality.taps() as u8;
        let coeffs_q30 = build_polyphase_table(safe_from, safe_to, quality);
        let history = alloc::vec![[0_i32; 2]; HISTORY_CAP].into_boxed_slice();

        Self {
            coeffs_q30,
            history,
            phase_num: 0,
            from_hz: safe_from,
            to_hz: safe_to,
            need_input_frames: 1,
            taps,
            hist_pos: 0,
            flush_remaining: 0,
            quality,
            interp: interp_mode,
        }
    }

    /// Convenience constructor enabling [`InterpolationMode::LinearPhase`].
    #[must_use]
    pub fn with_interpolation(from_hz: u32, to_hz: u32, quality: ResampleQuality) -> Self {
        Self::new(from_hz, to_hz, quality, InterpolationMode::LinearPhase)
    }

    /// Returns the configured input sample rate in Hz.
    #[must_use]
    pub const fn from_hz(&self) -> u32 {
        self.from_hz
    }

    /// Returns the configured output sample rate in Hz.
    #[must_use]
    pub const fn to_hz(&self) -> u32 {
        self.to_hz
    }

    /// Returns the configured resampling quality tier.
    #[must_use]
    pub const fn quality(&self) -> ResampleQuality {
        self.quality
    }

    /// Returns the configured phase interpolation mode.
    #[must_use]
    pub const fn interpolation_mode(&self) -> InterpolationMode {
        self.interp
    }

    /// Resets the internal delay history and rational phase accumulator.
    pub fn reset(&mut self) {
        for frame in &mut self.history {
            *frame = [0; 2];
        }
        self.hist_pos = 0;
        self.phase_num = 0;
        self.need_input_frames = 1;
        self.flush_remaining = 0;
    }

    /// Processes interleaved stereo `i32` samples (`[L0, R0, L1, R1, ...]`),
    /// returning `(consumed_samples, produced_samples)` in `i32` elements
    /// (always even multiples of 2).
    pub fn process(&mut self, input: &[i32], output: &mut [i32]) -> (usize, usize) {
        let in_frames = input.len() / 2;
        let out_frames = output.len() / 2;
        let mut in_idx = 0usize;
        let mut out_idx = 0usize;

        while out_idx < out_frames {
            while self.need_input_frames > 0 {
                if in_idx >= in_frames {
                    return (in_idx * 2, out_idx * 2);
                }
                let base = in_idx * 2;
                self.push_frame(input[base], input[base + 1]);
                self.flush_remaining = self.taps / 2;
                in_idx += 1;
                self.need_input_frames -= 1;
            }

            let (out_l, out_r) = self.eval_frame();
            let out_base = out_idx * 2;
            output[out_base] = out_l;
            output[out_base + 1] = out_r;
            out_idx += 1;

            self.advance_phase();
        }

        (in_idx * 2, out_idx * 2)
    }

    /// Drains remaining filter group-delay tail by feeding zero frames into
    /// the delay line and writing interleaved stereo `i32` samples to `output`.
    /// Returns the number of `i32` samples produced.
    pub fn flush(&mut self, output: &mut [i32]) -> usize {
        let out_frames = output.len() / 2;
        let mut out_idx = 0usize;

        while out_idx < out_frames && self.flush_remaining > 0 {
            while self.need_input_frames > 0 {
                if self.flush_remaining == 0 {
                    return out_idx * 2;
                }
                self.push_frame(0, 0);
                self.flush_remaining -= 1;
                self.need_input_frames -= 1;
            }

            let (out_l, out_r) = self.eval_frame();
            let out_base = out_idx * 2;
            output[out_base] = out_l;
            output[out_base + 1] = out_r;
            out_idx += 1;

            self.advance_phase();
        }

        out_idx * 2
    }

    #[inline]
    fn push_frame(&mut self, left: i32, right: i32) {
        self.hist_pos = if self.hist_pos == 0 {
            (MAX_TAPS as u8) - 1
        } else {
            self.hist_pos - 1
        };
        let pos = usize::from(self.hist_pos);
        self.history[pos] = [left, right];
        self.history[pos + MAX_TAPS] = [left, right];
    }

    #[inline]
    fn advance_phase(&mut self) {
        self.phase_num += u64::from(self.from_hz);
        self.need_input_frames = (self.phase_num / u64::from(self.to_hz)) as u32;
        self.phase_num %= u64::from(self.to_hz);
    }

    #[inline]
    fn eval_frame(&self) -> (i32, i32) {
        let taps = usize::from(self.taps);
        let pos = usize::from(self.hist_pos);
        let hist_slice = &self.history[pos..pos + taps];

        let scaled_phase =
            ((self.phase_num * (NUM_PHASES as u64)) << 16) / u64::from(self.to_hz);
        let phase_idx = ((scaled_phase >> 16) as usize).min(NUM_PHASES - 1);
        let frac_q16 = (scaled_phase & 0xFFFF) as i64;

        let row0 = &self.coeffs_q30[phase_idx];
        let mut acc_l = 0_i64;
        let mut acc_r = 0_i64;

        match self.interp {
            InterpolationMode::QuantizedPhase => {
                let nearest_idx = if frac_q16 >= 32_768 {
                    phase_idx + 1
                } else {
                    phase_idx
                };
                let row = &self.coeffs_q30[nearest_idx];
                for k in 0..taps {
                    let c = i64::from(row[k]);
                    let frame = hist_slice[k];
                    acc_l = acc_l.saturating_add(i64::from(frame[0]) * c);
                    acc_r = acc_r.saturating_add(i64::from(frame[1]) * c);
                }
            }
            InterpolationMode::LinearPhase => {
                let row1 = &self.coeffs_q30[phase_idx + 1];
                for k in 0..taps {
                    let c0 = i64::from(row0[k]);
                    let c1 = i64::from(row1[k]);
                    let c = c0 + (((c1 - c0) * frac_q16 + 32_768) >> 16);
                    let frame = hist_slice[k];
                    acc_l = acc_l.saturating_add(i64::from(frame[0]) * c);
                    acc_r = acc_r.saturating_add(i64::from(frame[1]) * c);
                }
            }
        }

        let out_l = ((acc_l + HALF_Q30) >> 30).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        let out_r = ((acc_r + HALF_Q30) >> 30).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        (out_l as i32, out_r as i32)
    }
}

fn build_polyphase_table(
    from_hz: u32,
    to_hz: u32,
    quality: ResampleQuality,
) -> Box<[[i32; MAX_TAPS]]> {
    let mut table = alloc::vec![[0_i32; MAX_TAPS]; PHASE_ROWS].into_boxed_slice();
    let taps = quality.taps();
    let beta = quality.kaiser_beta();
    let i0_beta = bessel_i0_f64(beta);

    let ratio = if to_hz < from_hz {
        f64::from(to_hz) / f64::from(from_hz)
    } else {
        1.0
    };
    let fc = quality.cutoff_scale() * ratio;
    let half_taps = (taps as f64) * 0.5;

    let mut phase = 0usize;
    while phase < PHASE_ROWS {
        let mu = (phase as f64) / (NUM_PHASES as f64);
        let mut raw = [0.0_f64; MAX_TAPS];
        let mut sum = 0.0_f64;

        let mut k = 0usize;
        while k < taps {
            let t = (k as f64) + mu - half_taps;
            let u = t / half_taps;
            let win = if u.abs() >= 1.0 {
                0.0
            } else {
                let arg = beta * sqrt_f64((1.0 - u * u).max(0.0));
                bessel_i0_f64(arg) / i0_beta
            };
            let sinc = if t.abs() < 1e-12 {
                2.0 * fc
            } else {
                let x = core::f64::consts::TAU * fc * t;
                sin_f64(x) / (core::f64::consts::PI * t)
            };
            let val = sinc * win;
            raw[k] = val;
            sum += val;
            k += 1;
        }

        let inv_sum = if sum.abs() > 1e-12 { 1.0 / sum } else { 0.0 };
        let mut q_sum = 0_i64;
        let mut j = 0usize;
        while j < taps {
            let scaled = raw[j] * inv_sum * (ONE_Q30 as f64);
            let rounded = if scaled >= 0.0 {
                (scaled + 0.5) as i64
            } else {
                (scaled - 0.5) as i64
            };
            let clamped = rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            table[phase][j] = clamped;
            q_sum += i64::from(clamped);
            j += 1;
        }

        // Enforce exact unity DC sum (`ONE_Q30`) on every phase branch.
        let center = (taps / 2).min(taps.saturating_sub(1));
        let residual = ONE_Q30 - q_sum;
        let adjusted = (i64::from(table[phase][center]) + residual)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        table[phase][center] = adjusted as i32;

        phase += 1;
    }

    table
}

/// Zeroth-order modified Bessel function of the first kind $I_0(x)$ via its
/// convergent power series $\sum_{m=0}^{28} \frac{(x^2 / 4)^m}{(m!)^2}$.
fn bessel_i0_f64(x: f64) -> f64 {
    let half_x_sq = 0.25 * x * x;
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    let mut m = 1usize;
    while m <= 28 {
        let mf = m as f64;
        term *= half_x_sq / (mf * mf);
        sum += term;
        if term < sum * 1e-16 {
            break;
        }
        m += 1;
    }
    sum
}

/// Pure `no_std` IEEE-754 square root on `f64` using Newton-Raphson iterations.
fn sqrt_f64(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut guess = f64::from_bits((x.to_bits() >> 1) + (0x3ff0_0000_0000_0000_u64 >> 1));
    let mut i = 0;
    while i < 8 {
        guess = 0.5 * (guess + x / guess);
        i += 1;
    }
    guess
}

/// Pure `no_std` IEEE-754 sine on `f64` reduced into $[-\pi/2, \pi/2]$.
fn sin_f64(x: f64) -> f64 {
    const PI: f64 = core::f64::consts::PI;
    const TAU: f64 = core::f64::consts::TAU;
    const HALF_PI: f64 = core::f64::consts::FRAC_PI_2;

    let mut angle = x % TAU;
    if angle > PI {
        angle -= TAU;
    } else if angle < -PI {
        angle += TAU;
    }
    if angle > HALF_PI {
        angle = PI - angle;
    } else if angle < -HALF_PI {
        angle = -PI - angle;
    }

    let u2 = angle * angle;
    let c3 = -1.0 / 6.0;
    let c5 = 1.0 / 120.0;
    let c7 = -1.0 / 5_040.0;
    let c9 = 1.0 / 362_880.0;
    let c11 = -1.0 / 39_916_800.0;
    let c13 = 1.0 / 6_227_020_800.0;
    let c15 = -1.0 / 1_307_674_368_000.0;

    let poly = 1.0
        + u2 * (c3 + u2 * (c5 + u2 * (c7 + u2 * (c9 + u2 * (c11 + u2 * (c13 + u2 * c15))))));
    angle * poly
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_state_fits_psram_budget() {
        let size = core::mem::size_of::<Resampler>();
        assert!(
            size <= 64,
            "Resampler inline struct size ({size} bytes) must be <= 64 bytes"
        );
    }

    #[test]
    fn test_resampler_unity_dc_preservation_and_reset() {
        let mut r = Resampler::with_interpolation(44_100, 48_000, ResampleQuality::Medium);
        assert_eq!(r.from_hz(), 44_100);
        assert_eq!(r.to_hz(), 48_000);
        assert_eq!(r.quality(), ResampleQuality::Medium);
        assert_eq!(r.interpolation_mode(), InterpolationMode::LinearPhase);

        let dc_in = [100_000_i32; 256];
        let mut dc_out = [0_i32; 512];
        let (consumed, produced) = r.process(&dc_in, &mut dc_out);
        assert_eq!(consumed, 256);
        assert!(produced > 256);

        // After settling (first 32 stereo frames), DC output must equal 100_000.
        let prod_frames = produced / 2;
        for f in 40..prod_frames {
            assert_eq!(dc_out[f * 2], 100_000, "left DC mismatch at frame {f}");
            assert_eq!(dc_out[f * 2 + 1], 100_000, "right DC mismatch at frame {f}");
        }

        let mut tail = [0_i32; 64];
        let tail_samples = r.flush(&mut tail);
        assert!(tail_samples > 0);
        r.reset();
        assert_eq!(r.flush(&mut tail), 0);
    }
}
