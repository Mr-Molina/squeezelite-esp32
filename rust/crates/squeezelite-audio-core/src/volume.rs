//! Audio Volume Attenuation & Gain Scaling
//!
//! [`GainStage`] is the LMS output's composite gain (volume x replay gain x fade, polarity,
//! channel select), a port of C squeezelite's `_output_frames` gain composition and
//! `_apply_gain`. [`VolumeScale`] is the plain per-channel volume record, kept for the
//! Bluetooth path and the coordinator's volume bookkeeping.

use crate::fade::{gain, FIXED_ONE};

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

/// Output channel selection from `strm s` (C `output.channels = (flags & 0x0c) >> 2`,
/// `slimproto.c:489`, with `MONO_LEFT = 0x01` and `MONO_RIGHT = 0x02`, `squeezelite.h:663-664`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelMode {
    /// Neither bit: left and right as they are.
    #[default]
    Stereo,
    /// Both bits (`MONO_LEFT | MONO_RIGHT`): the average of the two scaled channels on both
    /// (C `output_pack.c:386-392`).
    MonoMix,
    /// `MONO_LEFT` (0x01): the scaled left sample on both channels (C `output_pack.c:400-405`).
    LeftOnly,
    /// `MONO_RIGHT` (0x02): the scaled right sample on both channels (C `output_pack.c:394-399`).
    RightOnly,
}

impl ChannelMode {
    /// C parity: slimproto.c:489 `output.channels = (strm->flags & 0x0c) >> 2`.
    #[must_use]
    pub const fn from_strm_flags(flags: u8) -> Self {
        match (flags & 0x0c) >> 2 {
            0x01 => Self::LeftOnly,
            0x02 => Self::RightOnly,
            0x03 => Self::MonoMix,
            _ => Self::Stereo,
        }
    }
}

/// The composite output gain: volume x replay gain x fade, polarity and channel select, in the
/// order C composes them in `_output_frames` (`output.c:54-57`, `:226-228`, `:243-245`) and
/// applies them in `_apply_gain` (`output_pack.c:383-410`).
///
/// All gains are Q16.16 (`0x1_0000` = unity). The pipeline owns one and refreshes the
/// per-chunk fields before every output chunk:
/// - `volume_l_q16` / `volume_r_q16`: C `output.gainL` / `gainR` (set by `audg`, kept across
///   chunks). Unity when a hardware amplifier does the volume (C `set_volume` with a
///   `volume_cb`, `output_embedded.c:118-126`).
/// - `replay_gain_q16`: C `output.current_replay_gain`; 0 means none (`output.c:54-55`). During
///   a crossfade it is 0, because the crossfade weights carry each track's own replay gain
///   (C uses plain `output.gainL` there, `output.c:243-245`).
/// - `fade_q16`: the fade-up/down gain of this chunk (`FadeEngine::gain_q16`), unity otherwise.
/// - `invert_polarity`: C `output.invert` (`slimproto.c:488`).
/// - `channel_mode`: C `output.channels` (`slimproto.c:489`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GainStage {
    pub volume_l_q16: u32,
    pub volume_r_q16: u32,
    pub replay_gain_q16: u32,
    pub fade_q16: u32,
    pub invert_polarity: bool,
    pub channel_mode: ChannelMode,
}

impl Default for GainStage {
    fn default() -> Self {
        Self::unity()
    }
}

/// A `u32` gain as the `s32_t` C passes to `gain()`. Values above `i32::MAX` saturate.
/// Declared divergence: C converts `u32_t output.gainL` implicitly, so a gain >= 2^31 would
/// wrap negative (a polarity flip); LMS never sends one.
#[inline]
fn q16(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

impl GainStage {
    /// Unity volume, no replay gain, no fade, no inversion, stereo.
    #[must_use]
    pub const fn unity() -> Self {
        Self {
            volume_l_q16: FIXED_ONE as u32,
            volume_r_q16: FIXED_ONE as u32,
            replay_gain_q16: 0,
            fade_q16: FIXED_ONE as u32,
            invert_polarity: false,
            channel_mode: ChannelMode::Stereo,
        }
    }

    /// One channel's gain, composed exactly as C does.
    #[inline]
    fn compose(&self, volume: u32, invert: bool) -> i32 {
        let volume = q16(volume);
        // C parity: output.c:54-55, replay gain multiplies the volume; 0 means none.
        let mut g = if self.replay_gain_q16 != 0 { gain(volume, q16(self.replay_gain_q16)) } else { volume };
        // C parity: output.c:57, inversion negates the standard gain.
        if invert {
            g = -g;
        }
        // C parity: output.c:226-228, the fade gain multiplies the standard gain. Declared
        // divergence (ruled in Task 13): C negates a second time here (:229), which cancels
        // the inversion for the length of the fade; here it is negated once.
        if self.fade_q16 != FIXED_ONE as u32 {
            g = gain(g, q16(self.fade_q16));
        }
        g
    }

    /// Volume x replay gain x fade per channel (left, right), without the polarity: the
    /// magnitude C's `gainL` / `gainR` have in `_output_frames`. Never negative.
    #[must_use]
    pub fn effective_q16(&self) -> (u64, u64) {
        let l = self.compose(self.volume_l_q16, false);
        let r = self.compose(self.volume_r_q16, false);
        (l.max(0) as u64, r.max(0) as u64)
    }

    /// The signed channel gains handed to `_apply_gain` (C `gainL`, `gainR`).
    #[must_use]
    pub fn channel_gains(&self) -> (i32, i32) {
        (
            self.compose(self.volume_l_q16, self.invert_polarity),
            self.compose(self.volume_r_q16, self.invert_polarity),
        )
    }

    /// Scales interleaved stereo `samples` in place, with polarity and channel select (C
    /// parity: output_pack.c:383-410 `_apply_gain`). A trailing odd sample is left alone.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    pub fn apply(&self, samples: &mut [i32]) {
        let (gl, gr) = self.channel_gains();
        let frames = samples.chunks_exact_mut(2);
        match self.channel_mode {
            ChannelMode::Stereo => {
                // C parity: output_pack.c:384, unity and stereo is a no-op.
                if gl == FIXED_ONE && gr == FIXED_ONE {
                    return;
                }
                for f in frames {
                    f[0] = gain(gl, f[0]);
                    f[1] = gain(gr, f[1]);
                }
            }
            ChannelMode::MonoMix => {
                // C parity: output_pack.c:386-392. Declared divergence: the sum is taken in
                // 64 bits (C adds two s32_t, undefined on overflow); the result is identical
                // whenever C's sum does not overflow. `/` truncates toward zero, as C.
                for f in frames {
                    let m = ((i64::from(gain(gl, f[0])) + i64::from(gain(gr, f[1]))) / 2) as i32;
                    f[0] = m;
                    f[1] = m;
                }
            }
            ChannelMode::RightOnly => {
                // C parity: output_pack.c:394-399.
                for f in frames {
                    let r = gain(gr, f[1]);
                    f[0] = r;
                    f[1] = r;
                }
            }
            ChannelMode::LeftOnly => {
                // C parity: output_pack.c:400-405.
                for f in frames {
                    let l = gain(gl, f[0]);
                    f[0] = l;
                    f[1] = l;
                }
            }
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

    let rem = chunks.into_remainder();
    if let Some(last) = rem.first_mut() {
        let l = ((*last as i64 * volume.gain_left as i64) + 0x8000) >> 16;
        *last = l.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
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
        assert_eq!(samples, [1000, -1000, 2000, -2000, 9999]);
    }

    #[test]
    fn test_volume_attenuation_odd_remainder() {
        let mut samples = [10000, -10000, 20000, -20000, 8000];
        let half = VolumeScale {
            gain_left: 0x00008000, // 0.5x
            gain_right: 0x00008000,
            muted: false,
        };
        scale_samples_inplace(&mut samples, half);
        assert_eq!(samples, [5000, -5000, 10000, -10000, 4000]);
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

    // ---- GainStage (Task 14) ----------------------------------------------------------------

    fn stage(vol: u32, rg: u32, fade: u32) -> GainStage {
        GainStage { volume_l_q16: vol, volume_r_q16: vol, replay_gain_q16: rg, fade_q16: fade, ..GainStage::unity() }
    }

    #[test]
    fn test_composite_gain_exact_fixture() {
        // volume 0.5 x replay gain 2.0 x fade unity: sample 1000 -> exactly 1000.
        let g = stage(0x8000, 0x2_0000, 0x1_0000);
        assert_eq!(g.effective_q16(), (0x1_0000, 0x1_0000));
        let mut s = [1000, -1000, 7, -7];
        g.apply(&mut s);
        assert_eq!(s, [1000, -1000, 7, -7]);
    }

    #[test]
    fn test_composite_gain_composes_like_c() {
        // C output.c:54 then :225: gain(gain(vol, rg), fade), each step truncating (>> 16).
        let g = stage(0x9999, 0x1_6A09, 0xC000);
        let expect = gain(gain(0x9999, 0x1_6A09), 0xC000);
        assert_eq!(g.channel_gains(), (expect, expect));
        assert_eq!(g.effective_q16(), (expect as u64, expect as u64));
        let mut s = [123_456_789, -98_765_432];
        g.apply(&mut s);
        assert_eq!(s, [gain(expect, 123_456_789), gain(expect, -98_765_432)]);
    }

    #[test]
    fn test_replay_gain_zero_means_none() {
        // C output.c:54-55: `current_replay_gain ? gain(gainL, rg) : gainL`.
        let none = stage(0x8000, 0, 0x1_0000);
        assert_eq!(none.effective_q16(), (0x8000, 0x8000));
        let mut s = [1000, 1000];
        none.apply(&mut s);
        assert_eq!(s, [500, 500]);
        // Unity everywhere is C's fast path: samples untouched.
        let mut s = [i32::MAX, i32::MIN];
        GainStage::unity().apply(&mut s);
        assert_eq!(s, [i32::MAX, i32::MIN]);
    }

    #[test]
    fn test_independent_left_right_volume() {
        let g = GainStage { volume_l_q16: 0x1_0000, volume_r_q16: 0x4000, ..GainStage::unity() };
        assert_eq!(g.effective_q16(), (0x1_0000, 0x4000));
        let mut s = [4000, 4000];
        g.apply(&mut s);
        assert_eq!(s, [4000, 1000]);
    }

    #[test]
    fn test_polarity_inversion_negates() {
        // C output.c:57: both channel gains negated.
        let g = GainStage { invert_polarity: true, ..GainStage::unity() };
        assert_eq!(g.channel_gains(), (-0x1_0000, -0x1_0000));
        let mut s = [1000, -2000, i32::MAX, i32::MIN];
        g.apply(&mut s);
        // gain() saturates, so -i32::MIN stays in range (output_pack.c:33-38).
        assert_eq!(s, [-1000, 2000, -i32::MAX, i32::MAX]);
    }

    #[test]
    fn test_polarity_inversion_negates_once_with_fade() {
        // Declared divergence (ruled): C negates a second time under a fade (output.c:57 then
        // :229), which cancels the inversion; here the fade multiplies the inverted gain once.
        let g = GainStage { invert_polarity: true, fade_q16: 0x8000, ..GainStage::unity() };
        assert_eq!(g.channel_gains(), (gain(-0x1_0000, 0x8000), gain(-0x1_0000, 0x8000)));
        let mut s = [1000, -1000];
        g.apply(&mut s);
        assert_eq!(s, [-500, 500]);
    }

    #[test]
    fn test_gain_stage_volume_change_keeps_fade_q16() {
        let mut g = stage(0x1_0000, 0, 0x8000);
        let mut s = [1000, 1000];
        g.apply(&mut s);
        assert_eq!(s, [500, 500]);
        g.volume_l_q16 = 0x8000;
        g.volume_r_q16 = 0x8000;
        assert_eq!(g.fade_q16, 0x8000);
        let mut s = [1000, 1000];
        g.apply(&mut s);
        assert_eq!(s, [250, 250]);
    }

    #[test]
    fn test_channel_mode_from_strm_flags() {
        // C slimproto.c:489 `(flags & 0x0c) >> 2`; squeezelite.h:663-664 MONO_RIGHT 0x02,
        // MONO_LEFT 0x01. The polarity bits (0x03) do not affect it.
        assert_eq!(ChannelMode::from_strm_flags(0x00), ChannelMode::Stereo);
        assert_eq!(ChannelMode::from_strm_flags(0x04), ChannelMode::LeftOnly);
        assert_eq!(ChannelMode::from_strm_flags(0x08), ChannelMode::RightOnly);
        assert_eq!(ChannelMode::from_strm_flags(0x0c), ChannelMode::MonoMix);
        assert_eq!(ChannelMode::from_strm_flags(0xf3), ChannelMode::Stereo);
        assert_eq!(ChannelMode::from_strm_flags(0x07), ChannelMode::LeftOnly);
    }

    #[test]
    fn test_channel_select_left_duplicates_left() {
        // C output_pack.c:400-405: L = R = gain(gainL, L).
        let g = GainStage { volume_l_q16: 0x8000, volume_r_q16: 0x2000, channel_mode: ChannelMode::LeftOnly, ..GainStage::unity() };
        let mut s = [1000, 3000, -64, 99];
        g.apply(&mut s);
        assert_eq!(s, [500, 500, -32, -32]);
        // At unity too (C's fast path excludes MONO flags, output_pack.c:384).
        let g = GainStage { channel_mode: ChannelMode::LeftOnly, ..GainStage::unity() };
        let mut s = [1, 2];
        g.apply(&mut s);
        assert_eq!(s, [1, 1]);
    }

    #[test]
    fn test_channel_select_right_duplicates_right() {
        // C output_pack.c:394-399: L = R = gain(gainR, R).
        let g = GainStage { volume_l_q16: 0x8000, volume_r_q16: 0x4000, channel_mode: ChannelMode::RightOnly, ..GainStage::unity() };
        let mut s = [1000, 3000, -64, 100];
        g.apply(&mut s);
        assert_eq!(s, [750, 750, 25, 25]);
    }

    #[test]
    fn test_channel_select_mono_mix_c_average() {
        // C output_pack.c:386-392: L = R = (gain(gainL, L) + gain(gainR, R)) / 2, C integer
        // division (truncates toward zero).
        let g = GainStage { channel_mode: ChannelMode::MonoMix, ..GainStage::unity() };
        let mut s = [1000, 3001, -1000, -3001, 3, -4, i32::MAX, i32::MAX];
        g.apply(&mut s);
        assert_eq!(s, [2000, 2000, -2000, -2000, 0, 0, i32::MAX, i32::MAX]);
        // Gains apply per channel before the mix.
        let g = GainStage { volume_l_q16: 0x8000, volume_r_q16: 0x1_0000, channel_mode: ChannelMode::MonoMix, ..GainStage::unity() };
        let mut s = [1001, 7];
        g.apply(&mut s);
        // gain(0x8000, 1001) = 500; (500 + 7) / 2 = 253.
        assert_eq!(s, [253, 253]);
    }

    #[test]
    fn test_channel_select_with_invert() {
        let g = GainStage { invert_polarity: true, channel_mode: ChannelMode::RightOnly, ..GainStage::unity() };
        let mut s = [10, 20];
        g.apply(&mut s);
        assert_eq!(s, [-20, -20]);
    }
}
