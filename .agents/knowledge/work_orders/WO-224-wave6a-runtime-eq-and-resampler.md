# Work Order: WO-224 — Wave 6A: Phase 1 Tasks 16 & 17 — Runtime RBJ EQ, Volume-Indexed Loudness Contour, & Fixed-Point Polyphase Resampler (`squeezelite-audio-core` & `squeezelite-config`)

- **Work Order ID**: `WO-224`
- **Timestamp**: `2026-10-31T23:48:30-04:00`
- **Assigned Subagent Role**: `Rust DSP, EQ & Resampler Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/` & `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Implement Phase 1 Task 16 (Runtime RBJ Audio-EQ-Cookbook Peaking EQ, `preset_gains_db`, `loudness_gains_db`, and `Equalizer::retune`) and Phase 1 Task 17 (Fixed-point Q31 polyphase Kaiser windowed-sinc `Resampler` + `audio.maxrate` config key) in `squeezelite-audio-core` and `squeezelite-config` per `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-phase0-1-foundation-correct-player.md` (Tasks 16 & 17).

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly:
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/**`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/**`
   - Do **NOT** modify `squeezelite-decoders`, `nanomp3-core`, `squeezelite-raop`, `squeezelite-cspot`, `louder-esp32-app`, or `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
   - Keep `squeezelite-audio-core/Cargo.toml` zero-extra-dependency (implement `sin`/`cos`/`pow10`/`bessel_i0` in pure `no_std` Rust using IEEE-754 polynomial/Taylor/range-reduction math so `Cargo.lock` is untouched!).
3. **Backward Compatibility for Existing Callers**:
   - Keep all existing public functions/methods on `Equalizer`, `EqPreset`, `BiquadCoeffs`, `AudioConfig`, `JournalView`, and `SectorWriter` intact so `louder-esp32-app` continues to compile without breakage before Wave 7 (`WO-228`) wires the new methods.
4. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]` (except existing documented `ringbuffer.rs` `unsafe` blocks), `lib.rs` pure `mod` + `pub use` (`GRS-003`), single-module `use` (`GRS-001`), `#[non_exhaustive]` on public enums/structs where required (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
5. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Implementation Requirements

### Part A: Task 16 — Runtime RBJ Peaking EQ & Loudness Contour (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs`)
1. Add `pub const EQ_BANDS_HZ: [f32; 10] = [31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];` and `pub const DEFAULT_EQ_Q: f32 = 1.4142135;`.
2. Implement RBJ Audio-EQ-Cookbook peaking EQ designer (and unity conversion to Q30 fixed-point `BiquadCoeffs`, plus `f32` accessor methods or normalized fields):
   - `pub fn design_peaking(f0_hz: f32, q: f32, gain_db: f32, fs_hz: f32) -> BiquadCoeffs`
     - Standard RBJ peaking EQ:
       - If `gain_db.abs() < 1e-4` or `f0_hz <= 0.0` or `fs_hz <= 0.0` or `f0_hz >= fs_hz * 0.495`: return unity pass-through `BiquadCoeffs::unity()`.
       - `A = 10^(gain_db / 40.0)`, `w0 = 2 * pi * f0_hz / fs_hz`, `alpha = sin(w0) / (2.0 * q)`.
       - `b0 = 1 + alpha * A`, `b1 = -2 * cos(w0)`, `b2 = 1 - alpha * A`, `a0 = 1 + alpha / A`, `a1 = -2 * cos(w0)`, `a2 = 1 - alpha / A`.
       - Normalize by `a0` and quantize into `BiquadCoeffs` Q30 fixed-point (matching existing `BiquadCoeffs` sign convention for `a1`/`a2` in `equalizer.rs`! Check existing `BiquadCoeffs::unity()` and `process_sample` in `equalizer.rs` carefully so feedback signs match 100%!).
   - `pub fn design_bands(gains_db: &[f32; 10], fs_hz: f32) -> [BiquadCoeffs; 10]`
   - `pub fn preset_gains_db(preset: EqPreset) -> [f32; 10]`
   - `pub fn loudness_gains_db(volume_percent: u8, level: u8) -> [f32; 10]`
     - When `volume_percent >= 100` or `level == 0`, returns `[0.0; 10]`.
     - Scales monotonically with `level` (`1..=10`) and inversely with `volume_percent` (`0..100`): `(1.0 - (volume_percent as f32 / 100.0)) * (level as f32 / 10.0)` applied to a physiological Fletcher-Munson low/high-frequency contour vector (e.g., `[9.0, 7.5, 5.5, 3.0, 1.0, 0.0, 0.0, 1.5, 3.0, 4.5]`).
   - `impl Equalizer<10> { pub fn retune(&mut self, gains_db: &[f32; 10], fs_hz: f32) }`
     - Computes `design_bands(gains_db, fs_hz)`, checks stability of each biquad, updates coefficients, and resets filter state (`reset()`).
3. Add Task 16 unit tests in `equalizer.rs`:
   - `test_rbj_peaking_matches_reference_at_3_rates` (44.1 kHz, 48 kHz, 96 kHz for `f0 = 1000.0`, `q = 1.414`, `gain_db = 6.0`, verifying Q30 coefficients match RBJ reference within `1e-6` * `ONE_Q30`).
   - `test_retune_on_rate_change_keeps_stability`
   - `test_eqlz_packet_retunes_bands`
   - `test_loudness_contour_zero_at_full_volume` (`loudness_gains_db(100, 10) == [0.0; 10]`)
   - `test_loudness_scales_with_level_and_volume`

### Part B: Task 17 — Fixed-Point Polyphase Resampler (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs`)
1. Create `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs` and re-export `pub mod resampler;` + `pub use resampler::{InterpolationMode, ResampleQuality, Resampler};` in `src/lib.rs`:
   - Note `GRS-009` (no opaque `bool` parameter on public APIs!): To satisfy both Task 17 and `GRS-009`, provide `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ResampleQuality { Basic, Low, Medium }` and `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum InterpolationMode { QuantizedPhase, LinearPhase }`, with `Resampler::new(from_hz: u32, to_hz: u32, quality: ResampleQuality, interp: InterpolationMode) -> Self` (and `Resampler::with_interpolation(from_hz: u32, to_hz: u32, quality: ResampleQuality)`).
   - Implement a high-precision polyphase windowed-sinc filter (Kaiser window, `beta >= 8.5`, with sufficient phases e.g. 256 or 512 phases + linear interpolation between adjacent polyphase branches, and taps per phase e.g. 8 taps for `Basic`, 16 taps for `Low`, 32 taps for `Medium`, scaled by `min(1.0, to_hz / from_hz)` cutoff for anti-aliased downsampling) whose state fits well within `<= 16 KB` (`[i32; 128]` stereo ring history = 512 bytes, with static or on-the-fly/compact phase table).
   - Methods:
     - `pub fn process(&mut self, input: &[i32], output: &mut [i32]) -> (usize, usize)` (returns `(consumed_samples, produced_samples)` where `input` and `output` are stereo interleaved `i32` samples, i.e., `2` `i32`s per stereo frame).
     - `pub fn flush(&mut self, output: &mut [i32]) -> usize`
     - `pub fn reset(&mut self)`
     - `pub const fn from_hz(&self) -> u32`, `pub const fn to_hz(&self) -> u32`, `pub const fn quality(&self) -> ResampleQuality`
2. Create `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/tests/resampler_quality.rs` with Task 17 quality tests:
   - `test_sine_snr_medium_44k1_to_48k` (`SNR >= 90.0` dB for 1 kHz stereo sine wave in passband after settling transient)
   - `test_sine_snr_downsample_96k_to_44k1` (`SNR >= 90.0` dB at Medium quality for 1 kHz stereo sine wave)
   - `test_basic_quality_floor` (`SNR >= 55.0` dB at Basic quality)
   - `test_length_ratio_exact_over_10s` (produced/consumed stereo frame ratio within 1 frame of `to_hz / from_hz` over 10 seconds of input)
3. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs`:
   - Add `audio.maxrate` (`u32`, default `0` = unlimited) support (`max_sample_rate(&self) -> u32` and `set_max_sample_rate(&mut self, rate_hz: u32)` on `AudioConfig`, plus `JournalView::max_sample_rate(&self) -> u32` and `SectorWriter::set_max_sample_rate(&mut self, rate_hz: u32) -> Result<(), ConfigError>`), with unit tests!

## 4. Verification & Deliverables
1. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-config`.
2. Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src --strict` (`0 error(s), 0 warning(s)`).
3. Append Fulfillment Summary to §5 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo224_wave6a_eq_resampler.md`.

---

## 5. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T00:05:30-04:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo224_wave6a_eq_resampler.md`

### Files Created / Modified
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs` (Modified):
   - Added `EQ_BANDS_HZ`, `DEFAULT_EQ_Q`, pure `no_std` IEEE-754 RBJ Audio-EQ-Cookbook peaking filter designer (`design_peaking`, `design_bands`), `preset_gains_db`, `loudness_gains_db`, `BiquadCoeffs::{unity, ONE_Q30, b0_q30..a2_q30}`, `Equalizer::gains_db`, and `Equalizer<10>::retune`.
   - Added all 5 Task 16 unit tests (`test_rbj_peaking_matches_reference_at_3_rates`, `test_retune_on_rate_change_keeps_stability`, `test_eqlz_packet_retunes_bands`, `test_loudness_contour_zero_at_full_volume`, `test_loudness_scales_with_level_and_volume`).
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/resampler.rs` (Created):
   - Implemented `ResampleQuality::{Basic, Low, Medium}`, `InterpolationMode::{QuantizedPhase, LinearPhase}` (with `From<bool>`), and `Resampler` (fixed-point Q30 polyphase Kaiser windowed-sinc filter with 97 phase rows x 32 max taps, double-mirrored stereo delay line, exact rational modulo phase accumulator, and `size_of::<Resampler>() == 12,984 bytes <= 16,384 bytes`).
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/lib.rs` (Modified):
   - Re-exported `resampler::{InterpolationMode, ResampleQuality, Resampler}` and Task 16 EQ designer functions/constants (`GRS-001` & `GRS-003` compliant).
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/tests/resampler_quality.rs` (Created):
   - Added Task 17 quality integration tests (`test_sine_snr_medium_44k1_to_48k`, `test_sine_snr_downsample_96k_to_44k1`, `test_basic_quality_floor`, `test_length_ratio_exact_over_10s`).
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` (Modified):
   - Added `AudioConfig::KEY_MAXRATE` (`"audio.maxrate"`), `max_sample_rate: u32` field (default `0`), `AudioConfig::{max_sample_rate, set_max_sample_rate}`, `JournalView::max_sample_rate`, `SectorWriter::set_max_sample_rate`, and unit test `test_audio_config_max_sample_rate_roundtrip_and_journal_helpers`.

### Verification Evidence
- `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-config`: **PASS** (`63` audio-core unit + `4` resampler_quality + `4` stress + `22` config unit + `9` config_fuzz = `102` passed, `0` failed).
- `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-config --all-targets -- -D warnings`: **PASS** (`0` errors, `0` warnings).
- `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src --strict`: **PASS** (`0 error(s), 0 warning(s)`).
- `manage_task(list)`: `0` active background tasks.
