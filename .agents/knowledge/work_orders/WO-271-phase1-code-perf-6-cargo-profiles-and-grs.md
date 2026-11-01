# Work Order: WO-271 — Phase 1 `CODE-PERF-6`: Build System, Cargo Profiles (`opt-level`, `lto`), Linker/IRAM Placement & `GRS-002` Auditor

- **Work Order ID**: `WO-271`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Build System, Cargo Profiles, Linker Map & Code Quality Auditor (CODE-PERF-6)` (`Model: "flash"`, `TypeName: "research"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
  - `/home/console-admin/Github/squeezelite-esp32/rust/.cargo/config.toml`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/tests/marker_fuzz.rs`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Read-Only Constraint
Audit the workspace Cargo profiles, per-crate optimization overrides, cross-crate inlining, and `GRS-002` test compliance (`BUILD-601`, `BUILD-602`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **Release Profile Optimization (`opt-level = 2, lto = false` in `rust/Cargo.toml`)**:
   - Currently `rust/Cargo.toml` has `opt-level = 2` and `lto = false` across the entire workspace!
   - With `lto = false`, cross-crate calls between `louder-esp32-app`, `squeezelite-audio-core` (`AudioRingBuffer`, `Equalizer`, `Resampler`, `Volume`), `squeezelite-decoders`, `nanomp3-core`, and `squeezelite-tas58xx` are NEVER link-time inlined, and DSP/codec crates (`nanomp3-core`, `squeezelite-audio-core`, `squeezelite-decoders`, `alac`, `lewton`, `opus-rs`, `syom`) are compiled at `opt-level = 2` instead of `opt-level = 3`!
   - Inspect whether setting `[profile.release.package."nanomp3-core"] opt-level = 3`, `[profile.release.package."squeezelite-audio-core"] opt-level = 3`, `[profile.release.package."squeezelite-decoders"] opt-level = 3`, and `#[inline]` on hot cross-crate functions (or `lto = "thin"`) is supported cleanly by the Xtensa backend without increasing stack frames.
2. **`GRS-002` & `GRS-010`/`GRS-012` Test Suite Hygiene (`ANOM-PERF-05`)**:
   - Inspect `rust/crates/louder-esp32-app/tests/marker_fuzz.rs:13` (`use proptest::prelude::*;`) and list the exact replacement imports so `google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` passes with `0` errors across both `src/` and `tests/`.

## 3. Deliverable
Return a structured list of findings (`BUILD-601`, `BUILD-602`, ...) with exact `file:line` citations and concrete configuration/code remediations.
