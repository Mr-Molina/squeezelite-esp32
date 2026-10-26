# Work Order: WO-086 — Wave 1 Task 2: 10-Band Biquad Equalizer Presets (`squeezelite-audio-core`)

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-086` |
| **Timestamp** | `2026-10-25T12:26:30-04:00` |
| **Assigned Role** | DSP & Equalizer Preset Implementer (`Model: "flash"`, `TypeName: "self"`) |
| **Target Scope** | `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/lib.rs` ONLY |
| **Status** | `[EXECUTED & VERIFIED]` |

## 1. Objective & Task Brief
Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-2-brief.md` first — it is your requirements, with the exact values to use verbatim.

## 2. Mandatory Constraints
1. Modify ONLY `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/lib.rs` (Rule 35 backups already created in `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/`).
2. Note on RBJ Peaking Biquad Q24 Coefficients for `Equalizer<10>`:
   - Every band in `BassBoost`, `Loudness`, `Vocal`, and `Rock` MUST satisfy `coeffs.is_stable() == true` (Schur-Cohn check in `set_filter`).
   - Remember that in `equalizer.rs`, `BiquadCoeffs` uses normalized Direct Form I/II coefficients scaled by `Q24_ONE = 1 << 24 = 16_777_216` where the feedback equation is `y[n] = b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]`. Check existing `mod tests` in `equalizer.rs` to see the exact `a1`/`a2` sign convention so every preset band passes `is_stable()` and `BassBoost` boosts `62.5 Hz` (`peak_out > 125_000` for `100_000` input amplitude).
3. Follow TDD strictly: write the failing unit test first, run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core` to record RED failure, then implement `EqPreset`, `BiquadCoeffs::BYPASS`, `is_enabled`, `set_enabled`, `reset`, `set_band`, and `Equalizer::<10>::apply_preset`, and verify GREEN (`cargo test -p squeezelite-audio-core` and `cargo clippy -p squeezelite-audio-core --all-targets -- -D warnings`).
4. Do NOT run `git add` or `git commit` (to avoid `.git/index.lock` contention with parallel Wave 1 subagents).
5. Do NOT dispatch any subagents and do NOT touch `/dev/ttyUSB0`.
6. Write your detailed report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-2-report.md`, write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo086_task2_audio_core_eq.md`, and update the Fulfillment Section below.

## 3. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]` (DONE)
- **Implementer**: DSP & Equalizer Preset Implementer (`WO-086`)
- **Completion Timestamp**: `2026-10-25T12:36:00-04:00`
- **Modified Files**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/lib.rs`
- **Verification Evidence**:
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core`: 22 passed in `lib.rs`, 4 passed in `stress_test.rs`, 0 failed (exit code `0`).
  - `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core --all-targets -- -D warnings`: 0 warnings, 0 errors (exit code `0`).
  - Schur-Cohn stability asserted across all 40 individual preset biquad bands.
  - `BassBoost` 62.5 Hz amplitude boost verified: `peak_out = 205_235 > 125_000` on 100,000 input sine wave.
  - `Flat` preset verified as bit-perfect 0-cycle bypass (`(123_456, -654_321) == (123_456, -654_321)`).
- **Handoff Artifacts**:
  - Detailed Report: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-2-report.md`
  - Subagent Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo086_task2_audio_core_eq.md` and `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo086_audio_core_eq_implementer.md`
