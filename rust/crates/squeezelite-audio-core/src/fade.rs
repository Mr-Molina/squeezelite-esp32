//! Fade engine: C squeezelite's fade-in, fade-out, fade-in-out and crossfade.
//!
//! This is a port of the fade half of `_output_frames` (`output.c:174-252`) and of
//! `_checkfade` (`output.c:288-345`), with the sample arithmetic of `output_pack.c`.
//!
//! **Distance model.** C keeps `fade_start` / `fade_end` as pointers into the output ring and
//! recovers positions with wrap arithmetic. Here every position is a distance in frames from
//! the ring's read position, as for the pipeline's track boundary (`TrackMarker::bytes_ahead`):
//! - `Due`: the fade starts `start_ahead` frames ahead of the read position (C `fade_start`);
//! - `Active`: playback is `cur` frames into a fade of `dur` frames (C `cur_f`, `dur_f`), so
//!   `fade_end` is `dur - cur` frames ahead.
//!
//! The owner reports every frame it consumes from the ring through [`FadeEngine::advance`]
//! (and a rewind through [`FadeEngine::retreat`]); [`FadeEngine::prepare`] is called before
//! each output chunk and says how to scale it.
//!
//! **Ramp.** Linear in amplitude, as C: `to_gain((float)cur_f / (float)dur_f)` with
//! `to_gain(f) = f * 65536` (`output.c:224`, `output_pack.c:41-43`). The gain is computed once
//! per output chunk from the position at the chunk's start, as C does.

/// Q16.16 unity gain (C `FIXED_ONE`).
pub const FIXED_ONE: i32 = 0x1_0000;

/// C `gain()` (`output_pack.c:31-37`): `gain * sample` in 64 bits, clamped to
/// ±`0x7fff_ffff_ffff`, then shifted right 16 (so the result saturates at the `i32` range).
#[inline]
#[must_use]
pub fn gain(gain: i32, sample: i32) -> i32 {
    const MAX_SCALESAMPLE: i64 = 0x7fff_ffff_ffff;
    let res = (i64::from(gain) * i64::from(sample)).clamp(-MAX_SCALESAMPLE, MAX_SCALESAMPLE);
    (res >> 16) as i32
}

/// C `to_gain()` (`output_pack.c:41-43`): `(s32_t)(f * 65536.0F)`, in `f32` as in C.
#[inline]
#[must_use]
pub fn to_gain(f: f32) -> i32 {
    (f * 65536.0) as i32
}

/// C `to_gain((float)num / (float)den)` (`output.c:224`, `:232`).
#[inline]
fn ratio_gain(num: u64, den: u64) -> i32 {
    to_gain(num as f32 / den as f32)
}

/// Scales `samples` in place by a Q16.16 gain (C `_apply_gain`, `output_pack.c:383-410`, both
/// channels at the same gain). Unity is a no-op, as in C.
pub fn apply_gain(samples: &mut [i32], g: i32) {
    if g == FIXED_ONE {
        return;
    }
    for s in samples {
        *s = gain(g, *s);
    }
}

/// One crossfaded sample (C `_apply_cross`, `output_pack.c:373`): the old track's sample
/// weighted by `gain_out` plus the new track's weighted by `gain_in`.
///
/// Deliberate divergence: the sum saturates. C adds two `s32_t` (overflow is undefined
/// behavior there); it can only overflow when a replay gain above unity is in a weight.
#[inline]
#[must_use]
pub fn cross_sample(old: i32, new: i32, gain_out: i32, gain_in: i32) -> i32 {
    gain(gain_out, old).saturating_add(gain(gain_in, new))
}

/// Crossfades `new_head` into `old_tail` in place (C `_apply_cross`, `output_pack.c:366-376`).
/// Mixes `min(old_tail.len(), new_head.len())` samples.
pub fn mix_crossfade(old_tail: &mut [i32], new_head: &[i32], gain_out: i32, gain_in: i32) {
    for (old, &new) in old_tail.iter_mut().zip(new_head) {
        *old = cross_sample(*old, new, gain_out, gain_in);
    }
}

/// Transition type from `strm s` (C `fade_mode`, `squeezelite.h:661`, set by
/// `slimproto.c:486`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FadeMode {
    /// C `FADE_NONE`.
    #[default]
    None,
    /// C `FADE_CROSSFADE`.
    Cross,
    /// C `FADE_IN`.
    In,
    /// C `FADE_OUT`.
    Out,
    /// C `FADE_INOUT`.
    InOut,
    /// Any other `transition_type - '0'`: C stores it and no `_checkfade` branch matches, so
    /// it arms nothing.
    Other,
}

/// C `fade_state` (`squeezelite.h:659`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeState {
    Inactive,
    Due,
    Active,
}

/// C `fade_dir` (`squeezelite.h:660`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeDir {
    Up,
    Down,
    Cross,
}

/// How one output chunk is scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkGain {
    /// No fade gain.
    Unity,
    /// Fade up/down: Q16.16 fade gain that multiplies the standard gain (C `output.c:222-226`).
    Fade(i32),
    /// Crossfade (C `output.c:231-246`): every old sample at the read position is mixed with
    /// the new track's sample `new_ahead` frames further on (C `cross_ptr = fade_end + cur_f`).
    /// Each weight already includes its track's replay gain.
    Cross { gain_in: i32, gain_out: i32, new_ahead: u64 },
}

/// What [`FadeEngine::prepare`] decided for the next output chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeStep {
    /// Frames to drop unplayed before this chunk: a crossfade completed and the new track's
    /// first `dur` frames were already played inside the mix (C `output.c:196-200`).
    pub skip_frames: u64,
    /// A crossfade completed: the next replay gain becomes current (C `output.c:203`).
    pub crossfade_done: bool,
    /// The chunk may not be longer than this, so playback lands exactly on the fade start or
    /// end (C `output.c:178-180`, `:212-214`).
    pub max_frames: Option<u64>,
    /// How to scale the chunk.
    pub gain: ChunkGain,
}

/// The fade state of the output (C `output.fade`, `fade_dir`, `fade_mode`, `fade_secs`,
/// `fade_start`, `fade_end`), with positions as distances (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FadeEngine {
    mode: FadeMode,
    secs: u32,
    state: FadeState,
    dir: FadeDir,
    start_ahead: u64,
    dur: u64,
    cur: u64,
    gain: u32,
}

impl Default for FadeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FadeEngine {
    /// No transition configured, no fade.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: FadeMode::None,
            secs: 0,
            state: FadeState::Inactive,
            dir: FadeDir::Up,
            start_ahead: 0,
            dur: 0,
            cur: 0,
            gain: FIXED_ONE as u32,
        }
    }

    /// `strm s` transition (C `slimproto.c:486-487`: `fade_mode`, `fade_secs`). A fade that
    /// is already armed keeps running, as in C.
    pub fn configure(&mut self, mode: FadeMode, secs: u32) {
        self.mode = mode;
        self.secs = secs;
    }

    pub const fn mode(&self) -> FadeMode {
        self.mode
    }

    pub const fn state(&self) -> FadeState {
        self.state
    }

    pub const fn dir(&self) -> FadeDir {
        self.dir
    }

    /// Length of the armed fade in frames (C `fade_end - fade_start`).
    pub const fn duration_frames(&self) -> u64 {
        self.dur
    }

    /// Frames from the read position to the fade start while it is due.
    pub const fn start_ahead(&self) -> u64 {
        self.start_ahead
    }

    /// The fade gain of the current chunk, Q16.16 (unity when no fade up/down runs; a
    /// crossfade's weights are in [`ChunkGain::Cross`]). The pipeline copies it into its
    /// [`crate::GainStage`] once per chunk, which composes it with volume and replay gain, as C
    /// does at `output.c:226-228`.
    pub const fn gain_q16(&self) -> u32 {
        self.gain
    }

    /// A crossfade is armed or running: the playing track keeps its replay gain at the track
    /// start (C `output.c:157`: `fade == FADE_INACTIVE || fade_mode != FADE_CROSSFADE`).
    pub fn holds_replay_gain(&self) -> bool {
        self.state != FadeState::Inactive && self.mode == FadeMode::Cross
    }

    /// Arms a fade `start_ahead` frames ahead, `dur` frames long (C `fade = FADE_DUE`).
    fn arm(&mut self, dir: FadeDir, start_ahead: u64, dur: u64) {
        self.state = FadeState::Due;
        self.dir = dir;
        self.start_ahead = start_ahead;
        self.dur = dur;
        self.cur = 0;
        self.gain = FIXED_ONE as u32;
    }

    /// C `_checkfade(start)` (`output.c:288-345`). `start`: the first frames of a new stream
    /// are about to be written; otherwise decoding of the stream ended. `used_frames` is the
    /// unread output (the write position's distance), `size_frames` the ring size.
    ///
    /// Returns the new distance of the track start when a crossfade moved it to the fade start
    /// (C `output.track_start = output.fade_start`, `output.c:339`).
    pub fn check_fade(
        &mut self,
        start: bool,
        used_frames: u64,
        size_frames: u64,
        next_rate: u32,
        current_rate: u32,
    ) -> Option<u64> {
        // C output.c:292: bytes = next_sample_rate * BYTES_PER_FRAME * fade_secs (as frames).
        let mut frames = u64::from(next_rate) * u64::from(self.secs);
        if self.mode == FadeMode::InOut {
            // C output.c:293-296: half each way, on a frame boundary.
            frames /= 2;
        }

        // C output.c:298-309: fade in (or in-out from an empty buffer) from the write position.
        if start && (self.mode == FadeMode::In || (self.mode == FadeMode::InOut && used_frames == 0)) {
            // C output.c:299: shorter than the full buffer, otherwise start and end align.
            frames = frames.min(size_frames.saturating_sub(1));
            self.arm(FadeDir::Up, used_frames, frames);
        }

        // C output.c:311-321: fade out over the last frames before the write position.
        if !start && (self.mode == FadeMode::Out || self.mode == FadeMode::InOut) {
            frames = frames.min(used_frames);
            self.arm(FadeDir::Down, used_frames - frames, frames);
        }

        // C output.c:323-339: crossfade over the old track's last frames.
        if start && self.mode == FadeMode::Cross && used_frames != 0 {
            if next_rate != current_rate {
                // C output.c:325-327: "crossfade disabled as sample rates differ".
                return None;
            }
            // C output.c:329-330: at most what is left of the old track, and 90 % of the ring
            // (the new track's frames must fit behind it during the fade).
            // C bug not reproduced: C floors `size * 0.9` in BYTES, which need not be frame
            // aligned (a 131,072 B ring gives 117,964 B = 14,745.5 frames). When that cap
            // binds, C's fade_start and moved track_start never equal readp, so neither the
            // crossfade nor STMs fires. Rust floors to whole frames. At the firmware's 2000 KiB
            // output ring the C value is aligned (1,843,200 B = 230,400 frames): exact parity.
            frames = frames.min(used_frames).min(size_frames * 9 / 10);
            let start_ahead = used_frames - frames;
            self.arm(FadeDir::Cross, start_ahead, frames);
            // C output.c:339: output.track_start = output.fade_start.
            return Some(start_ahead);
        }
        // Deliberate divergence: with an empty buffer C resizes outputbuf to
        // OUTPUTBUF_SIZE_CROSSFADE (output.c:340-345). The Rust rings are fixed-size statics.
        None
    }

    /// The fade part of one `_output_frames` iteration (`output.c:174-252`), before a chunk is
    /// written. `used_frames`: unread output; `size_frames`: frames this step wants to write
    /// (C `size`); the replay gains are C `current_replay_gain` / `next_replay_gain`.
    pub fn prepare(&mut self, used_frames: u64, size_frames: u64, current_rg: u32, next_rg: u32) -> FadeStep {
        let mut step = FadeStep { skip_frames: 0, crossfade_done: false, max_frames: None, gain: ChunkGain::Unity };
        self.gain = FIXED_ONE as u32;
        if self.state == FadeState::Due {
            if self.start_ahead == 0 {
                // C output.c:176-177: "fade start reached".
                self.state = FadeState::Active;
                self.cur = 0;
            } else {
                // C output.c:178-180: the chunk ends exactly on the fade start.
                step.max_frames = Some(self.start_ahead);
            }
        }
        if self.state != FadeState::Active {
            return step;
        }

        let dur = self.dur;
        if self.cur >= dur {
            if self.mode == FadeMode::InOut && self.dir == FadeDir::Down {
                // C output.c:186-193: "fade down complete, starting fade up" from the read
                // position, over the same duration.
                self.dir = FadeDir::Up;
                self.cur = 0;
                if dur == 0 {
                    // Deliberate divergence: C goes on to compute to_gain(0.0 / 0.0) (NaN,
                    // undefined in the cast) for one chunk. A 0-frame fade has nothing to do.
                    self.state = FadeState::Inactive;
                    return step;
                }
            } else if self.mode == FadeMode::Cross {
                // C output.c:194-204: "crossfade complete". The new track's first dur_f frames
                // were played inside the mix: skip them, then its replay gain applies.
                if used_frames >= dur {
                    step.skip_frames = dur;
                }
                self.state = FadeState::Inactive;
                step.crossfade_done = true;
                return step;
            } else {
                // C output.c:205-207: "fade complete".
                self.state = FadeState::Inactive;
                return step;
            }
        }

        let cur = self.cur;
        // C output.c:212-214: the chunk ends exactly on the fade end.
        step.max_frames = Some(dur - cur);
        match self.dir {
            FadeDir::Up | FadeDir::Down => {
                // C output.c:216-226: linear in amplitude; down uses dur_f - cur_f.
                let num = if self.dir == FadeDir::Down { dur - cur } else { cur };
                let g = ratio_gain(num, dur);
                self.gain = g as u32;
                step.gain = ChunkGain::Fade(g);
            }
            FadeDir::Cross => {
                // C output.c:228-250: the new track's frames must follow the whole fade plus
                // this chunk, else the crossfade ends here.
                if used_frames > dur + size_frames {
                    let mut gain_in = ratio_gain(cur, dur);
                    let mut gain_out = FIXED_ONE - gain_in;
                    // Each track keeps its own replay gain (C output.c:234-239; 0 = none).
                    if current_rg != 0 {
                        gain_out = gain(gain_out, current_rg as i32);
                    }
                    if next_rg != 0 {
                        gain_in = gain(gain_in, next_rg as i32);
                    }
                    // C output.c:245: cross_ptr = fade_end + cur_f = read position + dur_f.
                    step.gain = ChunkGain::Cross { gain_in, gain_out, new_ahead: dur };
                } else {
                    // C output.c:247-249: "unable to continue crossfade - too few samples". C
                    // leaves current_replay_gain alone here (only completion switches it).
                    self.state = FadeState::Inactive;
                }
            }
        }
        step
    }

    /// `frames` were consumed from the output ring (played or skipped).
    pub fn advance(&mut self, frames: u64) {
        match self.state {
            FadeState::Inactive => {}
            FadeState::Due if frames <= self.start_ahead => self.start_ahead -= frames,
            FadeState::Due => {
                // Only a skip (`strm a`) jumps over the fade start: the fade goes on from where
                // playback landed. Deliberate divergence: C moves readp past fade_start and then
                // never sees fade_start == readp until the ring wraps (output.c:72-79, 176).
                self.state = FadeState::Active;
                self.cur = frames - self.start_ahead;
                self.start_ahead = 0;
            }
            FadeState::Active => self.cur += frames,
        }
    }

    /// The read position moved back by `frames` (a rewind).
    pub fn retreat(&mut self, frames: u64) {
        match self.state {
            FadeState::Inactive => {}
            FadeState::Due => self.start_ahead += frames,
            FadeState::Active if frames <= self.cur => self.cur -= frames,
            FadeState::Active => {
                self.state = FadeState::Due;
                self.start_ahead = frames - self.cur;
                self.cur = 0;
            }
        }
    }

    /// Everything from `kept_frames` ahead on was dropped from the ring (`strm f`).
    ///
    /// Deliberate divergence: C's `output_flush_streaming` (output.c:461-470) leaves the fade
    /// alone, so a fade due in the dropped region later fires on unrelated audio (a crossfade
    /// would mix the replacement track with its own later frames). Such a fade is dropped
    /// here; the replacement track's first frames arm a fresh one (C `_checkfade(true)`).
    pub fn discard_from(&mut self, kept_frames: u64) {
        if self.state == FadeState::Due && self.start_ahead >= kept_frames {
            self.state = FadeState::Inactive;
        }
    }

    /// No fade (C `output_flush`, `output.c:448`).
    pub fn reset(&mut self) {
        self.state = FadeState::Inactive;
        self.gain = FIXED_ONE as u32;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    /// Drives a whole fade the way the pipeline does: chunks of at most `chunk` frames, each
    /// clipped by the step's limit, with `used` unread frames before the first one. Returns
    /// (chunk start position, chunk length, gain) for every chunk until the fade ends.
    fn drive(engine: &mut FadeEngine, mut used: u64, chunk: u64) -> std::vec::Vec<(u64, u64, ChunkGain)> {
        let mut out = std::vec::Vec::new();
        let mut pos = 0;
        for _ in 0..10_000 {
            if used == 0 || engine.state() == FadeState::Inactive {
                break;
            }
            let step = engine.prepare(used, chunk.min(used), 0, 0);
            if engine.state() == FadeState::Inactive {
                break;
            }
            used -= step.skip_frames;
            let n = step.max_frames.map_or(chunk, |m| chunk.min(m)).min(used);
            out.push((pos, n, step.gain));
            engine.advance(n);
            used -= n;
            pos += n;
        }
        out
    }

    fn up(dur_frames: u64, ahead: u64) -> FadeEngine {
        let mut e = FadeEngine::new();
        // 1 Hz "rate", so secs == frames.
        e.configure(FadeMode::In, dur_frames as u32);
        assert_eq!(e.check_fade(true, ahead, 1 << 20, 1, 1), None);
        e
    }

    #[test]
    fn test_gain_and_to_gain_match_c() {
        assert_eq!(to_gain(0.5), 0x8000);
        assert_eq!(to_gain(1.0), FIXED_ONE);
        assert_eq!(gain(FIXED_ONE, 12345), 12345);
        assert_eq!(gain(0x8000, -4), -2);
        // Arithmetic shift: rounds toward minus infinity, as C's >> on s64.
        assert_eq!(gain(0x8000, -3), -2);
        // Saturates at the 0x7fffffffffff clamp (output_pack.c:27-28).
        assert_eq!(gain(-FIXED_ONE, i32::MIN), i32::MAX);
        assert_eq!(gain(2 * FIXED_ONE, i32::MAX), i32::MAX);
    }

    #[test]
    fn test_fade_in_ramp_monotonic_and_reaches_unity() {
        // Re-pinned to C (output.c:224, output_pack.c:42): LINEAR IN AMPLITUDE, not dB.
        let mut e = up(100, 0);
        assert_eq!((e.state(), e.dir()), (FadeState::Due, FadeDir::Up));
        let mut gains = std::vec::Vec::new();
        for _ in 0..100 {
            let step = e.prepare(1_000, 1, 0, 0);
            assert_eq!(step.max_frames, Some(100 - gains.len() as u64));
            let ChunkGain::Fade(g) = step.gain else { panic!("{step:?}") };
            assert_eq!(e.gain_q16(), g as u32);
            gains.push(g);
            e.advance(1);
        }
        // to_gain(cur_f / dur_f) at every frame.
        for (cur, g) in gains.iter().enumerate() {
            assert_eq!(*g, to_gain(cur as f32 / 100.0));
        }
        assert_eq!(gains[0], 0);
        assert_eq!(gains[50], 0x8000, "50 % of the duration = 0.5 x unity");
        assert!(gains.windows(2).all(|w| w[0] < w[1]));
        // The fade completes at fade_end: unity from there on (output.c:205-207).
        let step = e.prepare(1_000, 1, 0, 0);
        assert_eq!(step.gain, ChunkGain::Unity);
        assert_eq!(e.state(), FadeState::Inactive);
        assert_eq!(e.gain_q16(), FIXED_ONE as u32);
    }

    #[test]
    fn test_fade_out_reaches_silence() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Out, 100);
        // Decode complete with 300 unread frames: DOWN over the last 100 (output.c:311-321).
        assert_eq!(e.check_fade(false, 300, 1 << 20, 1, 1), None);
        assert_eq!((e.state(), e.dir(), e.start_ahead(), e.duration_frames()), (FadeState::Due, FadeDir::Down, 200, 100));
        let step = e.prepare(300, 64, 0, 0);
        assert_eq!((step.max_frames, step.gain), (Some(200), ChunkGain::Unity));
        e.advance(200);
        let mut gains = std::vec::Vec::new();
        for _ in 0..100 {
            let ChunkGain::Fade(g) = e.prepare(100, 1, 0, 0).gain else { panic!() };
            gains.push(g);
            e.advance(1);
        }
        // C output.c:220-224: DOWN uses dur_f - cur_f.
        for (cur, g) in gains.iter().enumerate() {
            assert_eq!(*g, to_gain((100 - cur) as f32 / 100.0));
        }
        assert_eq!(gains[0], FIXED_ONE);
        assert_eq!(gains[99], to_gain(0.01), "last frame of the track at 1 % amplitude");
        assert!(gains.windows(2).all(|w| w[0] > w[1]));
        // fade_end is the end of the track's data: nothing of it plays after the ramp.
        assert_eq!(e.prepare(0, 0, 0, 0).gain, ChunkGain::Unity);
        assert_eq!(e.state(), FadeState::Inactive);
    }

    #[test]
    fn test_crossfade_mix_complementary() {
        let old = [1_000_000 << 8, -(1_000_000 << 8), 7 << 16, i32::MAX];
        let new = [-(3 << 20), 3 << 20, 5 << 16, i32::MAX];
        for cur in [0u64, 1, 25, 50, 99, 100] {
            let gin = ratio_gain(cur, 100);
            let gout = FIXED_ONE - gin;
            // The two weights always sum to unity (C output.c:233).
            assert_eq!(gin + gout, FIXED_ONE);
            let mut mixed = old;
            mix_crossfade(&mut mixed, &new, gout, gin);
            for i in 0..old.len() {
                assert_eq!(mixed[i], gain(gout, old[i]).saturating_add(gain(gin, new[i])));
            }
            // Endpoints are exact.
            if cur == 0 {
                assert_eq!(mixed, old);
            }
            if cur == 100 {
                assert_eq!(mixed, new);
            }
        }
    }

    #[test]
    fn test_crossfade_arms_moves_the_track_start_and_weights_by_replay_gain() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Cross, 100);
        // 236 unread frames of the old track: the fade covers its last 100 (output.c:328-339).
        assert_eq!(e.check_fade(true, 236, 2048, 1, 1), Some(136));
        assert!(e.holds_replay_gain());
        assert_eq!((e.state(), e.dir(), e.start_ahead(), e.duration_frames()), (FadeState::Due, FadeDir::Cross, 136, 100));
        e.advance(136);
        let step = e.prepare(500, 64, 0x2_0000, 0x8000);
        assert_eq!(step.max_frames, Some(100));
        assert_eq!(e.state(), FadeState::Active);
        // cur 0: in = 0, out = unity x current replay gain (output.c:232-239).
        assert_eq!(step.gain, ChunkGain::Cross { gain_in: 0, gain_out: 0x2_0000, new_ahead: 100 });
        assert_eq!(e.gain_q16(), FIXED_ONE as u32);
        e.advance(64);
        let step = e.prepare(436, 36, 0x2_0000, 0x8000);
        let gin = to_gain(0.64);
        assert_eq!(
            step.gain,
            ChunkGain::Cross { gain_in: gain(gin, 0x8000), gain_out: gain(FIXED_ONE - gin, 0x2_0000), new_ahead: 100 }
        );
        assert_eq!(step.max_frames, Some(36));
        e.advance(36);
        // Complete: skip the dur_f new frames already played in the mix (output.c:195-204).
        let step = e.prepare(400, 64, 0x2_0000, 0x8000);
        assert_eq!((step.skip_frames, step.crossfade_done, step.gain), (100, true, ChunkGain::Unity));
        assert!(!e.holds_replay_gain());
    }

    #[test]
    fn test_crossfade_length_capped_by_used_and_ninety_percent_of_the_ring() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Cross, 10_000);
        assert_eq!(e.check_fade(true, 1_000, 1_024, 1, 1), Some(79));
        assert_eq!(e.duration_frames(), 921, "0.9 x 1024 frames, floored (output.c:330)");
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Cross, 10_000);
        assert_eq!(e.check_fade(true, 300, 1_024, 1, 1), Some(0));
        assert_eq!(e.duration_frames(), 300);
    }

    #[test]
    fn test_crossfade_not_armed_on_rate_change_or_empty_buffer() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Cross, 2);
        // C output.c:325-327: sample rates differ -> no fade at all.
        assert_eq!(e.check_fade(true, 1_000, 1 << 20, 48_000, 44_100), None);
        assert_eq!(e.state(), FadeState::Inactive);
        // Nothing to fade from (output.c:340: C would resize the buffer, Rust's is fixed).
        assert_eq!(e.check_fade(true, 0, 1 << 20, 44_100, 44_100), None);
        assert_eq!(e.state(), FadeState::Inactive);
    }

    #[test]
    fn test_crossfade_aborts_when_too_few_frames_follow() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Cross, 100);
        assert_eq!(e.check_fade(true, 100, 1 << 20, 1, 1), Some(0));
        // used must exceed dur_f + size (output.c:231): 100 + 64 = 164 is not enough.
        let step = e.prepare(164, 64, 0, 0);
        assert_eq!(step.gain, ChunkGain::Unity);
        assert_eq!(e.state(), FadeState::Inactive);
        assert!(!step.crossfade_done, "an aborted crossfade keeps the current replay gain (C)");
    }

    #[test]
    fn test_inout_halves_the_duration_and_fades_up_only_on_an_empty_buffer() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::InOut, 3);
        // 44.1 kHz x 3 s / 2 = 66150 frames (output.c:292-296).
        assert_eq!(e.check_fade(true, 10, 1 << 20, 44_100, 44_100), None);
        assert_eq!(e.state(), FadeState::Inactive, "INOUT fades up only from an empty buffer");
        assert_eq!(e.check_fade(true, 0, 1 << 20, 44_100, 44_100), None);
        assert_eq!((e.state(), e.dir(), e.duration_frames()), (FadeState::Due, FadeDir::Up, 66_150));
        // A fade up is at most one frame shorter than the ring (output.c:299).
        let mut e = FadeEngine::new();
        e.configure(FadeMode::In, 10);
        e.check_fade(true, 0, 1_000, 44_100, 44_100);
        assert_eq!(e.duration_frames(), 999);
    }

    #[test]
    fn test_inout_down_complete_switches_to_up_over_the_same_duration() {
        let mut e = FadeEngine::new();
        e.configure(FadeMode::InOut, 200); // 100 frames at 1 Hz
        e.check_fade(false, 100, 1 << 20, 1, 1);
        let chunks = drive(&mut e, 400, 64);
        let gains: std::vec::Vec<_> = chunks.iter().map(|c| (c.0, c.1, c.2)).collect();
        assert_eq!(
            gains,
            [
                (0, 64, ChunkGain::Fade(FIXED_ONE)),
                (64, 36, ChunkGain::Fade(to_gain(0.36))),
                // output.c:186-193: down complete -> up from the read position.
                (100, 64, ChunkGain::Fade(0)),
                (164, 36, ChunkGain::Fade(to_gain(0.64))),
            ]
        );
    }

    #[test]
    fn test_other_transition_values_arm_nothing() {
        // C keeps any `transition_type - '0'`; no _checkfade branch matches it.
        let mut e = FadeEngine::new();
        e.configure(FadeMode::Other, 5);
        assert_eq!(e.check_fade(true, 0, 1 << 20, 44_100, 44_100), None);
        assert_eq!(e.check_fade(true, 100, 1 << 20, 44_100, 44_100), None);
        assert_eq!(e.check_fade(false, 100, 1 << 20, 44_100, 44_100), None);
        assert_eq!(e.state(), FadeState::Inactive);
    }

    #[test]
    fn test_advance_retreat_and_discard_keep_the_distances() {
        let mut e = up(50, 100);
        e.advance(30);
        assert_eq!(e.start_ahead(), 70);
        e.retreat(10);
        assert_eq!(e.start_ahead(), 80);
        // A skip over the fade start (strm a) lands inside the fade, not before it.
        e.advance(90);
        assert_eq!(e.state(), FadeState::Active);
        assert_eq!(e.prepare(1_000, 1, 0, 0).gain, ChunkGain::Fade(to_gain(10.0 / 50.0)));
        e.retreat(15);
        assert_eq!((e.state(), e.start_ahead()), (FadeState::Due, 5));
        // strm f dropping the fade's region drops the fade.
        e.discard_from(20);
        assert_eq!(e.state(), FadeState::Due);
        e.discard_from(5);
        assert_eq!(e.state(), FadeState::Inactive);
        let mut e = up(50, 0);
        e.reset();
        assert_eq!(e.state(), FadeState::Inactive);
    }
}
