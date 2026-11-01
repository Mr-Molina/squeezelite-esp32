//! 10-Band Biquad Parametric Equalizer, Runtime RBJ Designer & Loudness Contour
//!
//! Ported and modernized from the Milestone MK-5 optimized C equalizer,
//! providing runtime RBJ Audio-EQ-Cookbook peaking filter synthesis,
//! volume-indexed Fletcher-Munson loudness contour gains, normalized
//! floating-point processing, and Schur-Cohn stability verification.

/// Center frequencies (in Hz) for the 10-band ISO graphic equalizer.
pub const EQ_BANDS_HZ: [f32; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Default quality factor ($Q = \sqrt{2}$) for 1-octave peaking bands.
pub const DEFAULT_EQ_Q: f32 = core::f32::consts::SQRT_2;

/// Fletcher-Munson reference boost (in dB) at maximum loudness compensation.
const LOUDNESS_REFERENCE_CONTOUR_DB: [f32; 10] =
    [9.0, 7.5, 5.5, 3.0, 1.0, 0.0, 0.0, 1.5, 3.0, 4.5];

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
        Self::bypass()
    }
}

impl BiquadCoeffs {
    /// 0-delay identity bypass filter coefficients ($H(z) = 1$).
    pub const BYPASS: Self = Self::bypass();

    /// Unity scale factor in signed Q2.30 fixed-point (`1 << 30`).
    pub const ONE_Q30: i32 = 1 << 30;

    /// Returns identity bypass filter coefficients
    /// ($b_0 = 1, b_1 = 0, b_2 = 0, a_1 = 0, a_2 = 0$).
    #[must_use]
    pub const fn bypass() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Alias for [`Self::bypass`] returning unity pass-through coefficients.
    #[must_use]
    pub const fn unity() -> Self {
        Self::bypass()
    }

    /// Quantizes `b0` into signed Q2.30 fixed-point representation.
    #[must_use]
    pub fn b0_q30(&self) -> i32 {
        quantize_q30(self.b0)
    }

    /// Quantizes `b1` into signed Q2.30 fixed-point representation.
    #[must_use]
    pub fn b1_q30(&self) -> i32 {
        quantize_q30(self.b1)
    }

    /// Quantizes `b2` into signed Q2.30 fixed-point representation.
    #[must_use]
    pub fn b2_q30(&self) -> i32 {
        quantize_q30(self.b2)
    }

    /// Quantizes `a1` into signed Q2.30 fixed-point representation.
    #[must_use]
    pub fn a1_q30(&self) -> i32 {
        quantize_q30(self.a1)
    }

    /// Quantizes `a2` into signed Q2.30 fixed-point representation.
    #[must_use]
    pub fn a2_q30(&self) -> i32 {
        quantize_q30(self.a2)
    }

    /// Verifies Bounded-Input Bounded-Output (BIBO) stability under the
    /// Schur-Cohn / Jury stability criterion:
    /// 1. All numerator and denominator coefficients are finite
    ///    (rejecting NaN and Inf).
    /// 2. $|a_2| < 1.0$
    /// 3. $|a_1| < 1.0 + a_2$
    #[must_use]
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

fn quantize_q30(val: f32) -> i32 {
    if !val.is_finite() {
        return 0;
    }
    let scaled = f64::from(val) * f64::from(BiquadCoeffs::ONE_Q30);
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    rounded.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Pure `no_std` IEEE-754 sine approximation on `f64` with range reduction
/// into $[-\pi/2, \pi/2]$ and a 15th-order Taylor polynomial.
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

/// Pure `no_std` IEEE-754 cosine approximation on `f64`.
fn cos_f64(x: f64) -> f64 {
    sin_f64(core::f64::consts::FRAC_PI_2 - x)
}

/// Pure `no_std` base-10 exponentiation $10^x = e^{x \ln(10)}$ using $2^k e^r$
/// range reduction onto $|r| \le \ln(2)/2$.
fn pow10_f64(x: f64) -> f64 {
    const LN_10: f64 = core::f64::consts::LN_10;
    const LN_2: f64 = core::f64::consts::LN_2;

    let y = (x * LN_10).clamp(-80.0, 80.0);
    let k_float = y / LN_2;
    let k = if k_float >= 0.0 {
        (k_float + 0.5) as i32
    } else {
        (k_float - 0.5) as i32
    };
    let r = y - f64::from(k) * LN_2;

    // 12-term Taylor series for exp(r) on |r| <= 0.3466 (error < 1e-16).
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut n = 1;
    while n <= 12 {
        term *= r / f64::from(n);
        sum += term;
        n += 1;
    }

    if k >= 0 {
        let mut i = 0;
        while i < k {
            sum *= 2.0;
            i += 1;
        }
    } else {
        let mut i = 0;
        while i < -k {
            sum *= 0.5;
            i += 1;
        }
    }
    sum
}

/// Synthesizes a second-order RBJ Audio-EQ-Cookbook peaking equalizer biquad
/// for center frequency `f0_hz`, quality factor `q`, gain `gain_db` (dB), and
/// sample rate `fs_hz`.
///
/// Returns [`BiquadCoeffs::unity()`] if inputs are non-finite, out of Nyquist
/// bounds (`f0_hz <= 0.0`, `q <= 0.0`, `fs_hz <= 0.0`, or
/// `f0_hz >= fs_hz * 0.495`), or when `gain_db.abs() < 1e-4`.
#[must_use]
pub fn design_peaking(f0_hz: f32, q: f32, gain_db: f32, fs_hz: f32) -> BiquadCoeffs {
    if !f0_hz.is_finite()
        || !q.is_finite()
        || !gain_db.is_finite()
        || !fs_hz.is_finite()
        || gain_db.abs() < 1e-4
        || f0_hz <= 0.0
        || q <= 0.0
        || fs_hz <= 0.0
        || f0_hz >= fs_hz * 0.495
    {
        return BiquadCoeffs::unity();
    }

    let clamped_db = f64::from(gain_db.clamp(-24.0, 24.0));
    let amp = pow10_f64(clamped_db / 40.0);
    let w0 = core::f64::consts::TAU * f64::from(f0_hz) / f64::from(fs_hz);
    let sin_w0 = sin_f64(w0);
    let cos_w0 = cos_f64(w0);
    let alpha = sin_w0 / (2.0 * f64::from(q));

    let b0 = 1.0 + alpha * amp;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * amp;
    let a0 = 1.0 + alpha / amp;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / amp;

    if a0.abs() < 1e-12 {
        return BiquadCoeffs::unity();
    }

    let coeffs = BiquadCoeffs {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    };

    if coeffs.is_stable() {
        coeffs
    } else {
        BiquadCoeffs::unity()
    }
}

/// Synthesizes all 10 ISO graphic equalizer bands at `EQ_BANDS_HZ` using
/// `DEFAULT_EQ_Q` for sample rate `fs_hz`.
#[must_use]
pub fn design_bands(gains_db: &[f32; 10], fs_hz: f32) -> [BiquadCoeffs; 10] {
    let mut out = [BiquadCoeffs::BYPASS; 10];
    let mut i = 0;
    while i < 10 {
        out[i] = design_peaking(EQ_BANDS_HZ[i], DEFAULT_EQ_Q, gains_db[i], fs_hz);
        i += 1;
    }
    out
}

/// Returns the 10-band gain vector (in dB) for a built-in [`EqPreset`].
#[must_use]
pub const fn preset_gains_db(preset: EqPreset) -> [f32; 10] {
    match preset {
        EqPreset::Flat => [0.0; 10],
        EqPreset::BassBoost => [5.5, 4.5, 3.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        EqPreset::Loudness => [4.5, 3.5, 3.5, 1.5, 0.0, -1.0, 0.0, 1.0, 3.0, 3.0],
        EqPreset::Vocal => [-2.0, -2.0, -1.0, 0.0, 0.0, 3.5, 3.5, 2.5, 0.0, 0.0],
        EqPreset::Rock => [4.0, 4.0, 4.0, 1.0, -1.5, -1.5, 0.0, 3.5, 3.5, 2.0],
    }
}

/// Computes the 10-band Fletcher-Munson loudness contour gain vector (in dB)
/// for `volume_percent` (`0..=100`) and loudness `level` (`0..=10`).
///
/// Boost scales monotonically with `level / 10` and inversely with volume
/// `(1.0 - volume_percent / 100.0)`. At `volume_percent >= 100` or
/// `level == 0`, returns `[0.0; 10]`.
#[must_use]
pub fn loudness_gains_db(volume_percent: u8, level: u8) -> [f32; 10] {
    if volume_percent >= 100 || level == 0 {
        return [0.0; 10];
    }
    let clamped_level = level.min(10);
    let vol_atten = 1.0 - (f32::from(volume_percent) / 100.0);
    let level_scale = f32::from(clamped_level) / 10.0;
    let scale = vol_atten * level_scale;

    let mut gains = [0.0_f32; 10];
    let mut i = 0;
    while i < 10 {
        gains[i] = LOUDNESS_REFERENCE_CONTOUR_DB[i] * scale;
        i += 1;
    }
    gains
}

/// Built-in 10-band ISO graphic equalizer presets
/// (`31.25 Hz` .. `16 kHz` @ `44.1 kHz`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EqPreset {
    /// Bit-perfect 0-cycle bypass (`0.0 dB` across all 10 bands).
    #[default]
    Flat = 0,
    /// Warm low-end shelf (`+5.5 dB` @ 31 Hz, `+4.5 dB` @ 62 Hz,
    /// `+3.0 dB` @ 125 Hz, `+1.5 dB` @ 250 Hz).
    BassBoost = 1,
    /// Fletcher-Munson loudness contour (`+4.5 dB` sub-bass, `+3.5 dB` bass,
    /// `-1.0 dB` 1 kHz, `+3.0 dB` 8-16 kHz air).
    Loudness = 2,
    /// Midrange presence boost for spoken word/radio (`-2.0 dB` sub-bass,
    /// `+3.5 dB` 1-2 kHz, `+2.5 dB` 4 kHz).
    Vocal = 3,
    /// Classic V-curve rock contour (`+4.0 dB` 62-125 Hz,
    /// `-1.5 dB` 500 Hz-1 kHz, `+3.5 dB` 4-8 kHz).
    Rock = 4,
}

impl EqPreset {
    /// Converts a numeric preset index (`0..=4`) into an `EqPreset`
    /// (unknown values map to `Flat`).
    #[must_use]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::BassBoost,
            2 => Self::Loudness,
            3 => Self::Vocal,
            4 => Self::Rock,
            _ => Self::Flat,
        }
    }

    /// Returns the canonical short name for JSON telemetry and Web UI display.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::BassBoost => "Bass",
            Self::Loudness => "Loudness",
            Self::Vocal => "Vocal",
            Self::Rock => "Rock",
        }
    }
}

pub struct Equalizer<const BANDS: usize> {
    filters: [BiquadCoeffs; BANDS],
    gains_db: [f32; BANDS],
    hist_left: [(f32, f32); BANDS],
    hist_right: [(f32, f32); BANDS],
    pub enabled: bool,
    pub current_preset: EqPreset,
    pub loudness_enabled: bool,
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
            gains_db: [0.0; BANDS],
            hist_left: [(0.0, 0.0); BANDS],
            hist_right: [(0.0, 0.0); BANDS],
            enabled: false,
            current_preset: EqPreset::Flat,
            loudness_enabled: false,
        }
    }

    /// Returns a reference to the array of filter coefficients for all bands.
    #[must_use]
    pub const fn filters(&self) -> &[BiquadCoeffs; BANDS] {
        &self.filters
    }

    /// Returns the most recently configured gain vector (in dB) for all bands.
    #[must_use]
    pub const fn gains_db(&self) -> &[f32; BANDS] {
        &self.gains_db
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

    /// Safely sets a band's coefficients, returning true on success
    /// or false if out of bounds or unstable.
    pub fn set_band(&mut self, index: usize, coeffs: BiquadCoeffs) -> bool {
        self.set_filter(index, coeffs).is_ok()
    }

    /// Reset internal filter state history without resetting coefficients.
    pub fn reset_history(&mut self) {
        self.hist_left = [(0.0, 0.0); BANDS];
        self.hist_right = [(0.0, 0.0); BANDS];
    }

    /// Reset internal filter state history without resetting coefficients.
    pub fn reset(&mut self) {
        self.reset_history();
    }

    /// Returns true if the equalizer is enabled.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Sets the equalizer enabled state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns the currently active equalizer preset.
    #[must_use]
    pub const fn current_preset(&self) -> EqPreset {
        self.current_preset
    }

    /// Returns true if loudness contour compensation is active.
    #[must_use]
    pub const fn loudness_enabled(&self) -> bool {
        self.loudness_enabled
    }

    /// Enables or disables loudness contour compensation.
    pub fn set_loudness_contour(&mut self, enabled: bool) {
        self.loudness_enabled = enabled;
    }

    /// Process a single stereo frame `(left, right)` in-place and return
    /// the filtered frame.
    #[inline]
    pub fn process_frame(&mut self, left: i32, right: i32) -> (i32, i32) {
        let mut frame = [left, right];
        self.process_interleaved(&mut frame);
        (frame[0], frame[1])
    }

    /// Process interleaved stereo 32-bit audio samples in-place.
    /// Operates in normalized $[-1.0, 1.0]$ float domain with denormal flushing,
    /// NaN state isolation, and accurate 32-bit boundary saturation.
    #[inline]
    #[cfg_attr(target_arch = "xtensa", link_section = ".iram1")]
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
            chunk[0] = if scaled_l.is_nan() {
                0
            } else {
                scaled_l.clamp(MIN_F32_I32, MAX_F32_I32) as i32
            };
            chunk[1] = if scaled_r.is_nan() {
                0
            } else {
                scaled_r.clamp(MIN_F32_I32, MAX_F32_I32) as i32
            };
        }
    }
}

impl<const BANDS: usize> Default for Equalizer<BANDS> {
    fn default() -> Self {
        Self::new()
    }
}

/// Precomputed RBJ peaking biquad coefficients for `EqPreset::BassBoost`
/// at 44.1 kHz ($Q = 1.414$).
#[allow(clippy::excessive_precision)]
pub const BASS_BOOST_COEFFS: [BiquadCoeffs; 10] = [
    // Band 0: 31.25 Hz, +5.5 dB
    BiquadCoeffs {
        b0: 1.0010125_f32,
        b1: -1.9976886_f32,
        b2: 0.99669589_f32,
        a1: -1.9976886_f32,
        a2: 0.99770838_f32,
    },
    // Band 1: 62.5 Hz, +4.5 dB
    BiquadCoeffs {
        b0: 1.0016456_f32,
        b1: -1.9950723_f32,
        b2: 0.99350584_f32,
        a1: -1.9950723_f32,
        a2: 0.99515145_f32,
    },
    // Band 2: 125.0 Hz, +3.0 dB
    BiquadCoeffs {
        b0: 1.0021743_f32,
        b1: -1.9891435_f32,
        b2: 0.98728466_f32,
        a1: -1.9891435_f32,
        a2: 0.98945895_f32,
    },
    // Band 3: 250.0 Hz, +1.5 dB
    BiquadCoeffs {
        b0: 1.0021525_f32,
        b1: -1.9759082_f32,
        b2: 0.97500982_f32,
        a1: -1.9759082_f32,
        a2: 0.9771623_f32,
    },
    // Band 4: 500.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 5: 1000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 6: 2000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 7: 4000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 8: 8000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 9: 16000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
];

/// Precomputed RBJ peaking biquad coefficients for `EqPreset::Loudness`
/// at 44.1 kHz ($Q = 1.414$).
#[allow(clippy::excessive_precision)]
pub const LOUDNESS_COEFFS: [BiquadCoeffs; 10] = [
    // Band 0: 31.25 Hz, +4.5 dB
    BiquadCoeffs {
        b0: 1.0008238_f32,
        b1: -1.997553_f32,
        b2: 0.99674895_f32,
        a1: -1.997553_f32,
        a2: 0.99757276_f32,
    },
    // Band 1: 62.5 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.0012741_f32,
        b1: -1.9947858_f32,
        b2: 0.99359078_f32,
        a1: -1.9947858_f32,
        a2: 0.99486489_f32,
    },
    // Band 2: 125.0 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.0025416_f32,
        b1: -1.9894409_f32,
        b2: 0.98721489_f32,
        a1: -1.9894409_f32,
        a2: 0.98975649_f32,
    },
    // Band 3: 250.0 Hz, +1.5 dB
    BiquadCoeffs {
        b0: 1.0021525_f32,
        b1: -1.9759082_f32,
        b2: 0.97500982_f32,
        a1: -1.9759082_f32,
        a2: 0.9771623_f32,
    },
    // Band 4: 500.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 5: 1000.0 Hz, -1.0 dB
    BiquadCoeffs {
        b0: 0.99450823_f32,
        b1: -1.8797594_f32,
        b2: 0.90449284_f32,
        a1: -1.8797594_f32,
        a2: 0.89900107_f32,
    },
    // Band 6: 2000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 7: 4000.0 Hz, +1.0 dB
    BiquadCoeffs {
        b0: 1.0186232_f32,
        b1: -1.4268986_f32,
        b2: 0.67612513_f32,
        a1: -1.4268986_f32,
        a2: 0.6947483_f32,
    },
    // Band 8: 8000.0 Hz, +3.0 dB
    BiquadCoeffs {
        b0: 1.0877855_f32,
        b1: -0.65774215_f32,
        b2: 0.48662668_f32,
        a1: -0.65774215_f32,
        a2: 0.57441216_f32,
    },
    // Band 9: 16000.0 Hz, +3.0 dB
    BiquadCoeffs {
        b0: 1.0760083_f32,
        b1: 1.0620084_f32,
        b2: 0.55550031_f32,
        a1: 1.0620084_f32,
        a2: 0.63150858_f32,
    },
];

/// Precomputed RBJ peaking biquad coefficients for `EqPreset::Vocal`
/// at 44.1 kHz ($Q = 1.414$).
#[allow(clippy::excessive_precision)]
pub const VOCAL_COEFFS: [BiquadCoeffs; 10] = [
    // Band 0: 31.25 Hz, -2.0 dB
    BiquadCoeffs {
        b0: 0.99963732_f32,
        b1: -1.9964535_f32,
        b2: 0.99683593_f32,
        a1: -1.9964535_f32,
        a2: 0.99647326_f32,
    },
    // Band 1: 62.5 Hz, -2.0 dB
    BiquadCoeffs {
        b0: 0.99927593_f32,
        b1: -1.99288_f32,
        b2: 0.99368306_f32,
        a1: -1.99288_f32,
        a2: 0.992959_f32,
    },
    // Band 2: 125.0 Hz, -1.0 dB
    BiquadCoeffs {
        b0: 0.99927941_f32,
        b1: -1.9864326_f32,
        b2: 0.98746828_f32,
        a1: -1.9864326_f32,
        a2: 0.98674769_f32,
    },
    // Band 3: 250.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 4: 500.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 5: 1000.0 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.0195663_f32,
        b1: -1.9016751_f32,
        b2: 0.90157478_f32,
        a1: -1.9016751_f32,
        a2: 0.92114108_f32,
    },
    // Band 6: 2000.0 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.0372954_f32,
        b1: -1.7750987_f32,
        b2: 0.81239154_f32,
        a1: -1.7750987_f32,
        a2: 0.8496869_f32,
    },
    // Band 7: 4000.0 Hz, +2.5 dB
    BiquadCoeffs {
        b0: 1.0472901_f32,
        b1: -1.4451447_f32,
        b2: 0.66912944_f32,
        a1: -1.4451447_f32,
        a2: 0.71641952_f32,
    },
    // Band 8: 8000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 9: 16000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
];

/// Precomputed RBJ peaking biquad coefficients for `EqPreset::Rock`
/// at 44.1 kHz ($Q = 1.414$).
#[allow(clippy::excessive_precision)]
pub const ROCK_COEFFS: [BiquadCoeffs; 10] = [
    // Band 0: 31.25 Hz, +4.0 dB
    BiquadCoeffs {
        b0: 1.0007305_f32,
        b1: -1.9974822_f32,
        b2: 0.99677143_f32,
        a1: -1.9974822_f32,
        a2: 0.99750197_f32,
    },
    // Band 1: 62.5 Hz, +4.0 dB
    BiquadCoeffs {
        b0: 1.0014592_f32,
        b1: -1.9949311_f32,
        b2: 0.99355098_f32,
        a1: -1.9949311_f32,
        a2: 0.99501022_f32,
    },
    // Band 2: 125.0 Hz, +4.0 dB
    BiquadCoeffs {
        b0: 1.0029111_f32,
        b1: -1.9897301_f32,
        b2: 0.98713456_f32,
        a1: -1.9897301_f32,
        a2: 0.99004567_f32,
    },
    // Band 3: 250.0 Hz, +1.0 dB
    BiquadCoeffs {
        b0: 1.0014335_f32,
        b1: -1.9752496_f32,
        b2: 0.97506974_f32,
        a1: -1.9752496_f32,
        a2: 0.97650326_f32,
    },
    // Band 4: 500.0 Hz, -1.5 dB
    BiquadCoeffs {
        b0: 0.9957643_f32,
        b1: -1.9416508_f32,
        b2: 0.95082374_f32,
        a1: -1.9416508_f32,
        a2: 0.94658804_f32,
    },
    // Band 5: 1000.0 Hz, -1.5 dB
    BiquadCoeffs {
        b0: 0.9917688_f32,
        b1: -1.8769916_f32,
        b2: 0.90443611_f32,
        a1: -1.8769916_f32,
        a2: 0.89620491_f32,
    },
    // Band 6: 2000.0 Hz, +0.0 dB (Bypass)
    BiquadCoeffs::BYPASS,
    // Band 7: 4000.0 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.0669565_f32,
        b1: -1.4566987_f32,
        b2: 0.66318586_f32,
        a1: -1.4566987_f32,
        a2: 0.73014235_f32,
    },
    // Band 8: 8000.0 Hz, +3.5 dB
    BiquadCoeffs {
        b0: 1.1032232_f32,
        b1: -0.66173733_f32,
        b2: 0.48075209_f32,
        a1: -0.66173733_f32,
        a2: 0.58397525_f32,
    },
    // Band 9: 16000.0 Hz, +2.0 dB
    BiquadCoeffs {
        b0: 1.0499869_f32,
        b1: 1.0505394_f32,
        b2: 0.56390243_f32,
        a1: 1.0505394_f32,
        a2: 0.61388936_f32,
    },
];

impl Equalizer<10> {
    /// Retunes all 10 equalizer bands for `gains_db` at sample rate `fs_hz`
    /// using the RBJ peaking EQ designer, verifies Schur-Cohn BIBO stability
    /// for each band, and conditionally resets internal filter delay history
    /// only if transitioning to disabled or if history contains non-finite values.
    pub fn retune(&mut self, gains_db: &[f32; 10], fs_hz: f32) {
        self.gains_db = *gains_db;
        let bands = design_bands(gains_db, fs_hz);
        let mut any_active = false;
        for (i, &coeffs) in bands.iter().enumerate() {
            if coeffs.is_stable() {
                self.filters[i] = coeffs;
            } else {
                self.filters[i] = BiquadCoeffs::BYPASS;
            }
            if gains_db[i].is_finite() && gains_db[i].abs() >= 1e-4 {
                any_active = true;
            }
        }
        self.enabled = any_active && fs_hz.is_finite() && fs_hz > 0.0;
        let has_non_finite_history = self
            .hist_left
            .iter()
            .chain(self.hist_right.iter())
            .any(|&(z1, z2)| !z1.is_finite() || !z2.is_finite());
        if !self.enabled || has_non_finite_history {
            self.reset();
        }
    }

    /// Applies a pre-tuned 10-band graphic equalizer preset
    /// (`Flat`, `BassBoost`, `Loudness`, `Vocal`, `Rock`).
    ///
    /// When `EqPreset::Flat`, resets all 10 bands to `BiquadCoeffs::BYPASS`
    /// and sets `enabled = false` for 0-cycle passthrough.
    /// For all other presets, loads the Schur-Cohn verified coefficient tables
    /// and sets `enabled = true` without clearing biquad history to ensure
    /// smooth, click-free transitions.
    pub fn apply_preset(&mut self, preset: EqPreset) {
        self.current_preset = preset;
        self.gains_db = preset_gains_db(preset);
        match preset {
            EqPreset::Flat => {
                for i in 0..10 {
                    let _ = self.set_band(i, BiquadCoeffs::BYPASS);
                }
                self.set_enabled(false);
            }
            EqPreset::BassBoost => {
                for (i, &coeffs) in BASS_BOOST_COEFFS.iter().enumerate() {
                    let ok = self.set_band(i, coeffs);
                    debug_assert!(ok, "BassBoost band {} must be stable", i);
                }
                self.set_enabled(true);
            }
            EqPreset::Loudness => {
                for (i, &coeffs) in LOUDNESS_COEFFS.iter().enumerate() {
                    let ok = self.set_band(i, coeffs);
                    debug_assert!(ok, "Loudness band {} must be stable", i);
                }
                self.set_enabled(true);
            }
            EqPreset::Vocal => {
                for (i, &coeffs) in VOCAL_COEFFS.iter().enumerate() {
                    let ok = self.set_band(i, coeffs);
                    debug_assert!(ok, "Vocal band {} must be stable", i);
                }
                self.set_enabled(true);
            }
            EqPreset::Rock => {
                for (i, &coeffs) in ROCK_COEFFS.iter().enumerate() {
                    let ok = self.set_band(i, coeffs);
                    debug_assert!(ok, "Rock band {} must be stable", i);
                }
                self.set_enabled(true);
            }
        }
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

    #[test]
    fn test_loudness_contour_processing_and_odd_remainder() {
        let mut eq: Equalizer<10> = Equalizer::new();
        eq.set_loudness_contour(true);
        assert!(eq.loudness_enabled());

        // Enable EQ with BassBoost
        eq.apply_preset(EqPreset::BassBoost);
        assert!(eq.is_enabled());

        let mut samples = [100_000, -100_000, 999_999];
        eq.process_interleaved(&mut samples);
        // Odd remainder trailing sample (999_999) must remain unmodified
        assert_eq!(samples[2], 999_999);
        // First stereo pair should be processed by biquad filter
        assert_ne!(samples[0], 0);

        // When Flat preset is applied, EQ is disabled and samples pass
        // through unmodified.
        eq.apply_preset(EqPreset::Flat);
        assert!(!eq.is_enabled());
        let mut samples_flat = [500_000, -500_000];
        eq.process_interleaved(&mut samples_flat);
        assert_eq!(samples_flat[0], 500_000);
        assert_eq!(samples_flat[1], -500_000);
    }

    #[test]
    fn test_retune_preserves_biquad_delay_history_during_playback() {
        let mut eq = Equalizer::<10>::new();
        let gains1 = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        eq.retune(&gains1, 48_000.0);
        assert!(eq.is_enabled());

        // Process a frame to populate delay history
        let _ = eq.process_frame(500_000, -500_000);
        let has_active_history = eq
            .hist_left
            .iter()
            .chain(eq.hist_right.iter())
            .any(|&(z1, z2)| z1 != 0.0 || z2 != 0.0);
        assert!(has_active_history, "history should be populated after frame");

        // Retune with new volume/loudness gains at the same sample rate
        let gains2 = [2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0];
        eq.retune(&gains2, 48_000.0);
        assert!(eq.is_enabled());

        // History must be preserved to prevent audible clicks/pops (LOGIC-204)
        let history_preserved = eq
            .hist_left
            .iter()
            .chain(eq.hist_right.iter())
            .any(|&(z1, z2)| z1 != 0.0 || z2 != 0.0);
        assert!(history_preserved, "history must NOT be zeroed when retuning active EQ");

        // Transitioning to disabled (all zeros) must reset history
        let gains_zero = [0.0; 10];
        eq.retune(&gains_zero, 48_000.0);
        assert!(!eq.is_enabled());
        let history_zeroed = eq
            .hist_left
            .iter()
            .chain(eq.hist_right.iter())
            .all(|&(z1, z2)| z1 == 0.0 && z2 == 0.0);
        assert!(history_zeroed, "history must be zeroed when transitioning to disabled");
    }

    #[test]
    fn test_eq_presets_are_schur_cohn_stable_and_modify_spectrum() {
        let mut eq: Equalizer<10> = Equalizer::new();
        assert!(!eq.is_enabled());

        for code in 1..=4u8 {
            let preset = EqPreset::from_u8(code);
            eq.apply_preset(preset);
            assert!(eq.is_enabled(), "preset {:?} must enable EQ", preset);
        }

        // Verify BassBoost amplifies a low-frequency 62.5 Hz sine wave
        eq.apply_preset(EqPreset::BassBoost);
        let mut peak_out = 0i32;
        for n in 0..2048 {
            let theta = (n as f32) * core::f32::consts::TAU * 62.5 / 44_100.0;
            // Taylor-series / Bhaskara sine approximation for no_std test compatibility
            let x = theta % core::f32::consts::TAU;
            let x_pi = if x > core::f32::consts::PI { x - core::f32::consts::TAU } else { x };
            let sin_val = (16.0 * x_pi * (core::f32::consts::PI - x_pi.abs()))
                / (5.0 * core::f32::consts::PI * core::f32::consts::PI
                    - 4.0 * x_pi.abs() * (core::f32::consts::PI - x_pi.abs()));
            let sample = (sin_val * 100_000.0) as i32;
            let (l, _) = eq.process_frame(sample, sample);
            if n > 512 && l.abs() > peak_out {
                peak_out = l.abs();
            }
        }
        assert!(peak_out > 125_000, "BassBoost should boost 62.5 Hz (peak={peak_out})");

        // Verify Flat bypasses EQ (0-cycle passthrough)
        eq.apply_preset(EqPreset::Flat);
        assert!(!eq.is_enabled());
        assert_eq!(eq.process_frame(123_456, -654_321), (123_456, -654_321));
    }

    #[test]
    fn test_all_preset_coefficients_individually_stable() {
        assert!(BiquadCoeffs::BYPASS.is_stable());

        for (i, c) in BASS_BOOST_COEFFS.iter().enumerate() {
            assert!(c.is_stable(), "BASS_BOOST band {i} unstable: {c:?}");
        }
        for (i, c) in LOUDNESS_COEFFS.iter().enumerate() {
            assert!(c.is_stable(), "LOUDNESS band {i} unstable: {c:?}");
        }
        for (i, c) in VOCAL_COEFFS.iter().enumerate() {
            assert!(c.is_stable(), "VOCAL band {i} unstable: {c:?}");
        }
        for (i, c) in ROCK_COEFFS.iter().enumerate() {
            assert!(c.is_stable(), "ROCK band {i} unstable: {c:?}");
        }
    }

    #[test]
    fn test_eq_preset_properties_and_equalizer_helpers() {
        assert_eq!(EqPreset::from_u8(0), EqPreset::Flat);
        assert_eq!(EqPreset::from_u8(1), EqPreset::BassBoost);
        assert_eq!(EqPreset::from_u8(2), EqPreset::Loudness);
        assert_eq!(EqPreset::from_u8(3), EqPreset::Vocal);
        assert_eq!(EqPreset::from_u8(4), EqPreset::Rock);
        assert_eq!(EqPreset::from_u8(99), EqPreset::Flat);

        assert_eq!(EqPreset::Flat.as_str(), "Flat");
        assert_eq!(EqPreset::BassBoost.as_str(), "Bass");
        assert_eq!(EqPreset::Loudness.as_str(), "Loudness");
        assert_eq!(EqPreset::Vocal.as_str(), "Vocal");
        assert_eq!(EqPreset::Rock.as_str(), "Rock");

        let mut eq = Equalizer::<10>::new();
        assert_eq!(eq.current_preset(), EqPreset::Flat);
        assert!(!eq.loudness_enabled());

        eq.set_loudness_contour(true);
        assert!(eq.loudness_enabled());
        eq.set_loudness_contour(false);
        assert!(!eq.loudness_enabled());

        eq.apply_preset(EqPreset::Vocal);
        assert_eq!(eq.current_preset(), EqPreset::Vocal);
        assert!(eq.is_enabled());

        eq.set_enabled(false);
        assert!(!eq.is_enabled());

        assert!(eq.set_band(0, BiquadCoeffs::BYPASS));
        assert!(!eq.set_band(10, BiquadCoeffs::BYPASS)); // out of bounds
        eq.reset();
    }

    #[test]
    fn test_rbj_peaking_matches_reference_at_3_rates() {
        let cases: [(f32, BiquadCoeffs); 3] = [
            (
                44_100.0,
                BiquadCoeffs {
                    b0: 1.0341633_f32,
                    b1: -1.9117787_f32,
                    b2: 0.89718485_f32,
                    a1: -1.9117787_f32,
                    a2: 0.93134815_f32,
                },
            ),
            (
                48_000.0,
                BiquadCoeffs {
                    b0: 1.0314914_f32,
                    b1: -1.9201485_f32,
                    b2: 0.90522593_f32,
                    a1: -1.9201485_f32,
                    a2: 0.9367174_f32,
                },
            ),
            (
                96_000.0,
                BiquadCoeffs {
                    b0: 1.0160326_f32,
                    b1: -1.963569_f32,
                    b2: 0.9517496_f32,
                    a1: -1.963569_f32,
                    a2: 0.9677822_f32,
                },
            ),
        ];

        let q30_tol = (1e-6_f64 * f64::from(BiquadCoeffs::ONE_Q30)) as i32 + 1;
        for (fs, expected) in cases {
            let actual = design_peaking(1000.0, 1.414, 6.0, fs);
            assert!(actual.is_stable(), "coeffs at fs={fs} must be stable: {actual:?}");
            assert!(
                (actual.b0 - expected.b0).abs() < 1e-6,
                "b0 mismatch at fs={fs}: got {}, expected {}",
                actual.b0,
                expected.b0
            );
            assert!(
                (actual.b1 - expected.b1).abs() < 1e-6,
                "b1 mismatch at fs={fs}: got {}, expected {}",
                actual.b1,
                expected.b1
            );
            assert!(
                (actual.b2 - expected.b2).abs() < 1e-6,
                "b2 mismatch at fs={fs}: got {}, expected {}",
                actual.b2,
                expected.b2
            );
            assert!(
                (actual.a1 - expected.a1).abs() < 1e-6,
                "a1 mismatch at fs={fs}: got {}, expected {}",
                actual.a1,
                expected.a1
            );
            assert!(
                (actual.a2 - expected.a2).abs() < 1e-6,
                "a2 mismatch at fs={fs}: got {}, expected {}",
                actual.a2,
                expected.a2
            );
            assert!(
                (actual.b0_q30() - expected.b0_q30()).abs() <= q30_tol,
                "b0_q30 mismatch at fs={fs}"
            );
            assert!(
                (actual.a2_q30() - expected.a2_q30()).abs() <= q30_tol,
                "a2_q30 mismatch at fs={fs}"
            );
        }
    }

    #[test]
    fn test_retune_on_rate_change_keeps_stability() {
        let mut eq = Equalizer::<10>::new();
        let gains = preset_gains_db(EqPreset::Rock);
        let rates = [22_050.0_f32, 44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0];

        let mut prev_b1 = 0.0_f32;
        for &fs in &rates {
            let _ = eq.process_frame(200_000, -200_000);
            eq.retune(&gains, fs);
            assert!(eq.is_enabled(), "retune with active gains at {fs} must enable EQ");
            for (band_idx, c) in eq.filters().iter().enumerate() {
                assert!(
                    c.is_stable(),
                    "band {band_idx} unstable at fs={fs}: {c:?}"
                );
            }
            // At 22.05 kHz, band 9 (16 kHz >= 0.495 * 22.05 kHz) bypasses.
            if fs == 22_050.0 {
                assert_eq!(eq.filters()[9], BiquadCoeffs::BYPASS);
            } else {
                assert_ne!(eq.filters()[9], BiquadCoeffs::BYPASS);
            }
            let b1_band0 = eq.filters()[0].b1;
            assert!(
                (b1_band0 - prev_b1).abs() > 1e-7,
                "retuning to a new sample rate ({fs}) must update biquad pole/zero angle"
            );
            prev_b1 = b1_band0;
        }
    }

    #[test]
    fn test_eqlz_packet_retunes_bands() {
        // Simulate an LMS `eqlz` 10-band signed i8 payload (with out-of-range
        // values clamped to [-12.0, 12.0] dB).
        let raw_eqlz_i8: [i8; 10] = [6, 4, 2, 0, -3, -6, 0, 3, 6, 15];
        let mut gains = [0.0_f32; 10];
        for (i, &val) in raw_eqlz_i8.iter().enumerate() {
            gains[i] = f32::from(val).clamp(-12.0, 12.0);
        }

        let mut eq = Equalizer::<10>::new();
        eq.retune(&gains, 48_000.0);
        assert!(eq.is_enabled());
        assert_eq!(eq.gains_db(), &gains);
        // Band 0 (+6 dB) boosts: b0 > 1.0
        assert!(eq.filters()[0].b0 > 1.0);
        // Band 3 (0 dB) is unity bypass
        assert_eq!(eq.filters()[3], BiquadCoeffs::BYPASS);
        // Band 5 (-6 dB) cuts: b0 < 1.0
        assert!(eq.filters()[5].b0 < 1.0);
        // Band 9 (+12 dB clamped) boosts: b0 > 1.0
        assert!(eq.filters()[9].b0 > 1.0);
    }

    #[test]
    fn test_loudness_contour_zero_at_full_volume() {
        assert_eq!(loudness_gains_db(100, 10), [0.0; 10]);
        assert_eq!(loudness_gains_db(120, 10), [0.0; 10]);
        assert_eq!(loudness_gains_db(50, 0), [0.0; 10]);
    }

    #[test]
    fn test_loudness_scales_with_level_and_volume() {
        // Monotonic increase with `level` (1..=10) at fixed volume (40%)
        let mut prev_level_gains = loudness_gains_db(40, 0);
        for level in 1..=10u8 {
            let cur = loudness_gains_db(40, level);
            for i in 0..10 {
                assert!(
                    cur[i] >= prev_level_gains[i],
                    "band {i} must not decrease when level increases to {level}"
                );
            }
            assert!(
                cur[0] > prev_level_gains[0],
                "sub-bass boost must strictly increase with level={level}"
            );
            assert!(
                cur[9] > prev_level_gains[9],
                "treble air boost must strictly increase with level={level}"
            );
            prev_level_gains = cur;
        }

        // Monotonic increase as `volume_percent` drops from 100 to 0 at level 8
        let mut prev_vol_gains = loudness_gains_db(100, 8);
        for step in (0..10u8).rev() {
            let vol = step * 10;
            let cur = loudness_gains_db(vol, 8);
            for i in 0..10 {
                assert!(
                    cur[i] >= prev_vol_gains[i],
                    "band {i} must not decrease when volume drops to {vol}%"
                );
            }
            assert!(
                cur[0] > prev_vol_gains[0],
                "sub-bass boost must strictly increase when volume drops to {vol}%"
            );
            prev_vol_gains = cur;
        }
    }
}
