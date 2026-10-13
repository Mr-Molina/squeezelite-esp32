//! Audio Volume Attenuation & Gain Scaling

/// Represents independent Left and Right 32-bit fixed-point volume gains.
///
/// In SlimProto, `0x00010000` (65536) represents 0 dB (unity gain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeScale {
    pub gain_left: u32,
    pub gain_right: u32,
    pub muted: bool,
}

impl Default for VolumeScale {
    fn default() -> Self {
        Self {
            gain_left: 0x00010000,
            gain_right: 0x00010000,
            muted: false,
        }
    }
}

impl VolumeScale {
    pub const fn unity() -> Self {
        Self {
            gain_left: 0x00010000,
            gain_right: 0x00010000,
            muted: false,
        }
    }

    pub const fn mute() -> Self {
        Self {
            gain_left: 0,
            gain_right: 0,
            muted: true,
        }
    }
}

/// Applies volume scaling to interleaved stereo 32-bit audio samples in-place.
#[allow(clippy::chunks_exact_to_as_chunks)]
pub fn scale_samples_inplace(samples: &mut [i32], volume: VolumeScale) {
    if volume.muted {
        samples.fill(0);
        return;
    }

    // Remainder guard: zero out trailing odd sample to prevent channel phase swap (DSP-007)
    if !samples.len().is_multiple_of(2) {
        if let Some(last) = samples.last_mut() {
            *last = 0;
        }
    }

    if volume.gain_left == 0x00010000 && volume.gain_right == 0x00010000 {
        return; // Unity gain fast path
    }

    let mut chunks = samples.chunks_exact_mut(2);
    for chunk in &mut chunks {
        let l = ((chunk[0] as i64 * volume.gain_left as i64) + 0x8000) >> 16;
        let r = ((chunk[1] as i64 * volume.gain_right as i64) + 0x8000) >> 16;
        chunk[0] = l.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        chunk[1] = r.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_unity_gain() {
        let mut samples = [1000, -1000, 2000, -2000];
        let original = samples;
        scale_samples_inplace(&mut samples, VolumeScale::unity());
        assert_eq!(samples, original);
    }

    #[test]
    fn test_volume_unity_gain_odd_remainder() {
        let mut samples = [1000, -1000, 2000, -2000, 9999];
        scale_samples_inplace(&mut samples, VolumeScale::unity());
        assert_eq!(samples, [1000, -1000, 2000, -2000, 0]);
    }

    #[test]
    fn test_volume_mute() {
        let mut samples = [1000, -1000, 2000, -2000];
        scale_samples_inplace(&mut samples, VolumeScale::mute());
        assert_eq!(samples, [0, 0, 0, 0]);
    }

    #[test]
    fn test_volume_half_attenuation() {
        let mut samples = [10000, 20000];
        let half = VolumeScale {
            gain_left: 0x00008000, // 0.5x
            gain_right: 0x00008000,
            muted: false,
        };
        scale_samples_inplace(&mut samples, half);
        assert_eq!(samples, [5000, 10000]);
    }
}
