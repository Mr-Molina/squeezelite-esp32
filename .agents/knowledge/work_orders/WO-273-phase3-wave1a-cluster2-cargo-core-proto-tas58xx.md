# Work Order: WO-273 — Perf Phase 3 Wave 1A Cluster 2: `Cargo.toml`, `squeezelite-audio-core`, `squeezelite-proto`, `squeezelite-tas58xx`, & `marker_fuzz.rs` Remediation

- **Work Order ID**: `WO-273`
- **Timestamp**: `2026-11-01T14:32:00-05:00`
- **Assigned Subagent Role**: `Cluster 2 Core, Proto, TAS58xx & Build Performance Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/{ringbuffer.rs,equalizer.rs,resampler.rs,volume.rs,fade.rs}`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/tests/marker_fuzz.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict File-Scope Boundary
Remediate `BUILD-601`, `BUILD-602`, `BUILD-603`, `BUILD-604`, `CPU-103a`, `CPU-104a`, and `HW-PERF-204`/`PIPE-206`.
- **STRICT SCOPE**: Only edit the 9 files listed above. Do NOT edit `squeezelite-decoders`, `nanomp3-core`, or any file in `louder-esp32-app/src/` (sibling subagents own those).
- Use `edit_file` (never `sed`/`awk`/`cat`).

## 2. Mandatory Remediations
1. **`BUILD-601` (`/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`)**:
   - Under `[profile.release]`, add per-package `opt-level = 3` overrides for the hot audio/DSP/codec crates:
     ```toml
     [profile.release.package.nanomp3-core]
     opt-level = 3

     [profile.release.package.squeezelite-audio-core]
     opt-level = 3

     [profile.release.package.squeezelite-decoders]
     opt-level = 3

     [profile.release.package.alac]
     opt-level = 3

     [profile.release.package.lewton]
     opt-level = 3

     [profile.release.package.opus-rs]
     opt-level = 3

     [profile.release.package.syom]
     opt-level = 3

     [profile.release.package.squeezelite-raop]
     opt-level = 3

     [profile.release.package.squeezelite-cspot]
     opt-level = 3
     ```
2. **`BUILD-602`, `BUILD-603`, & `CPU-103a` (`squeezelite-audio-core/src/{ringbuffer.rs,equalizer.rs,resampler.rs,volume.rs,fade.rs}`)**:
   - In `ringbuffer.rs`:
     - Add `#[inline]` to `AudioRingBuffer::available_read`, `AudioRingBuffer::available_write`, `AudioRingBuffer::capacity`, `AudioRingBuffer::clear`, `AudioRingBuffer::peek_at`, `AudioRingBuffer::consume`, `AudioRingBuffer::write_internal`, `AudioRingBuffer::read_internal`, `Producer::write`, `Producer::available_write`, `Consumer::read`, `Consumer::available_read`, `Consumer::peek`, and `Consumer::consume`.
   - In `equalizer.rs`:
     - Add `#[inline]` and `#[cfg_attr(target_arch = "xtensa", link_section = ".iram1")]` to `Equalizer::process_interleaved` (and `#[inline(always)]` to `BiquadState::process_sample` or internal biquad helper if separate).
   - In `resampler.rs`:
     - Add `#[inline]` to `Resampler::process`.
   - In `volume.rs`:
     - Add `#[inline]` to `GainStage::apply` and `GainStage::set_gains`.
   - In `fade.rs`:
     - Add `#[inline]` to `FadeEngine::process_samples` / hot sample methods.
3. **`CPU-104a` (`squeezelite-tas58xx/src/lib.rs:705-716`)**:
   - Inspect `TasFaultStatus` in `squeezelite-tas58xx/src/lib.rs`.
   - Keep `is_clean(&self) -> bool` for backward compatibility, and add a new method `pub const fn has_critical_amp_fault(&self) -> bool`:
     ```rust
     /// Returns `true` if a non-clock amplifier fault (over-current, DC fault, over-temp,
     /// PVDD/BQ/OTP/CBC fault) requires a `Hi-Z -> Clear -> Play` recovery cycle.
     /// Transient `FAULT1_CLOCK_FAULT` (`0x04`) events self-recover via the TAS5805M auto-mute
     /// state machine and must NOT force the power stage into `Hi-Z` every 2 seconds.
     #[inline]
     #[must_use]
     pub const fn has_critical_amp_fault(&self) -> bool {
         self.chan_fault != 0
             || (self.global_fault1 & !FAULT1_CLOCK_FAULT) != 0
             || self.global_fault2 != 0
     }
     ```
   - Add a unit test in `squeezelite-tas58xx/src/lib.rs` verifying that `TasFaultStatus { chan_fault: 0, global_fault1: FAULT1_CLOCK_FAULT, global_fault2: 0, ot_warning: 0 }` returns `has_critical_amp_fault() == false` while `chan_fault != 0` or `global_fault1: FAULT1_PVDD_UV` returns `true`.
4. **`HW-PERF-204` / `PIPE-206` (`squeezelite-proto/src/lib.rs:806, 845, 850`)**:
   - Inspect `build_stat` and `build_stat_with_metrics` in `squeezelite-proto/src/lib.rs`.
   - In `build_stat_with_metrics` (lines 840–860), instead of hardcoding `stream_buffer_size: 65536` and `output_buffer_size: 65536` when `fullness > 65536`, compute:
     `stream_buffer_size: fullness.max(262_144)` (or `262_144` if `fullness > 65536` else `65536`, wait: check if existing unit tests in `squeezelite-proto` assert exact `65536` bytes in `build_stat_with_metrics`!).
     - **CRITICAL**: Check existing tests in `squeezelite-proto/src/lib.rs` first! If unit tests assert `65536` when `fullness <= 65536`, use `let stream_buffer_size = if fullness > 65536 { 262_144 } else { 65536 };` and `let output_buffer_size = if output_fullness > 65536 { 2_048_000 } else { 65536 };` so 100% of existing unit tests pass AND live PSRAM fullness values (`> 65536`) never report `> 100%` capacity!
5. **`BUILD-604` (`louder-esp32-app/tests/marker_fuzz.rs:13`)**:
   - Replace `use proptest::prelude::*;` with explicit single-module imports (zero `::` inside `{ ... }` per `GRS-001` and no `*` per `GRS-002`).

## 3. Verification
- Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/tests --strict` (`0` errors).
- Run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-tas58xx` (`0` failures).
- Write subagent summary and complete the Fulfillment Section below.

## 4. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completion Timestamp**: `2026-11-01T14:45:00-05:00`
- **Subagent Summary Artifact**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo273_cluster2_core_proto_tas58xx.md`
- **Pre-Modification Backups**:
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/Cargo.toml.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/ringbuffer.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/equalizer.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/resampler.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/volume.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/fade.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/squeezelite_proto_lib.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/squeezelite_tas58xx_lib.rs.bak`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/backups/wo273/marker_fuzz.rs.bak`

### Verification Results
1. **Google Rust Style Audit**:
   `0 error(s)` across all targeted crate sources and tests.
2. **Cargo Unit & Integration Tests**:
   - `squeezelite-audio-core`: 64 unit tests, 4 resampler quality tests, 4 stress tests passed (`0` failed).
   - `squeezelite-proto`: 25 unit tests, 10 proto fuzz tests passed (`0` failed).
   - `squeezelite-tas58xx`: 18 unit tests passed (`0` failed).
   - `louder-esp32-app` (`marker_fuzz`): 1 passed (`0` failed).
3. **Running Tasks Check**:
   `manage_task(Action: "list")` confirmed 0 running tasks.
