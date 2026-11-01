# Work Order: WO-199 — S1: AAC (`syom`) `no_std + alloc` Vendor Port, Firmware Wiring & Stage-Only Build

- **Work Order ID**: `WO-199`
- **Timestamp**: `2026-10-28T12:32:00-04:00`
- **Assigned Subagent Role**: `Rust no_std Codec Porter (S1 AAC)`
- **Plan**: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/implementation_plan.md` (user-approved, step S1)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective
Make the existing `AacDecoder` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs:1889-2012`) run in the ESP32 firmware (`louder-esp32-app` builds `squeezelite-decoders` with `default-features = false, features = ["alloc"]`), and dispatch LMS `strm` format byte `a` (AAC/ADTS) to it. **Stage-only. Do NOT flash any device.**

## 2. Hypothesis (Rule 6)
- *If this works*: `syom` builds under `no_std + alloc` for `xtensa-esp32-none-elf`, `AacDecoder` is available with the `alloc` feature, host tests decode an ADTS fixture to PCM matching the `std` build bit-for-bit, and the staged image stays under `0x290000` with `.stack >= 36,000 B` and `.dram2_uninit <= 98,768 B`.
- *If it fails*: `syom` depends on `std`-only behavior that has no `core`/`alloc`/`libm` equivalent → report it, don't work around it with hacks.

## 3. Hard Constraints
1. Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_5444e41b/` (the immutable verified baseline).
2. Do not overwrite `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin`. Stage the new image to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s1-aac.bin`.
3. **NEVER** touch `/dev/ttyUSB0`, call any `172.22.1.15` endpoint, or modify LMS `10.0.0.10`.
4. No `sed`/`awk`/`cat` file edits (Rule 23); absolute paths only (Rule 24); 3-run cap on the same failing build command before escalating (Rule 43); `<= 75` planner steps.
5. Keep existing MP3/FLAC/PCM/Bluetooth behavior unchanged. All existing tests must still pass.
6. Every AAC heap allocation must be able to come from the PSRAM-backed `esp_alloc::HEAP`. Do not add large `static` buffers to internal DRAM.

## 4. Steps
1. Copy `syom 0.7.1` from `/home/console-admin/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/syom-0.7.1` into `/home/console-admin/Github/squeezelite-esp32/rust/vendor/syom`. Add `#![no_std]` + `extern crate alloc;` (behind a `std` default feature if practical), replace `std::` with `core::`/`alloc::`, and route float math (`sin`, `cos`, `exp`, `pow`, `sqrt`, `log`, etc.) through `libm` when `std` is off. Write `/home/console-admin/Github/squeezelite-esp32/rust/vendor/syom/VENDOR.md` with the upstream version and patch list. Point `squeezelite-decoders` at the vendored path (`[patch.crates-io]` or a path dependency).
2. In `squeezelite-decoders`: make `syom` available under the `alloc` feature and change `AacDecoder`'s `#[cfg(feature = "std")]` gates to `alloc` (use `alloc::vec::Vec`).
3. Add host tests: decode an ADTS AAC-LC fixture (generate it with `ffmpeg` if available, and commit the small fixture under `crates/squeezelite-decoders/tests/fixtures/`). Assert identical PCM between the `std` and `no_std`/`alloc` builds, plus correct sample rate and channels.
4. In `louder-esp32-app`: add `ActiveDecoder::Aac` construction on `strm` format byte `a`, in both dispatch sites in `main.rs` (around `:2789-2850` and `:3140-3225`), using the same pattern as FLAC: guard with `psram_avail_bytes >= 1_048_576`, unmute the sink, and keep the mute-on-unsupported fallback. Make sure `/status.json` reports `"codec": "AAC"`.
5. Add an on-device decode-time metric: track the max microseconds per `decode()` call and expose `decode_us_max` in `/status.json` (needed for the CPU real-time gate in S1-live).
6. Gates (each must be shown with fresh output):
   - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` → `0 error(s), 0 warning(s)`
   - `cargo clippy --workspace --all-targets -- -D warnings` → 0 warnings
   - `cargo test --workspace` → 0 failures
   - Xtensa release build of `louder-esp32-app`; `xtensa-esp32-elf-readelf -S` → `.stack >= 36,000`, `.dram2_uninit <= 98,768`
   - `espflash save-image --chip esp32` → `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s1-aac.bin`, then run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py <that path>` → `[STAGE-ONLY AUDIT PASS]`, size `< 2,686,976`.

## 5. Deliverables
- Fulfillment Summary appended below, with file list, gate outputs, staged size and SHA-256.
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo199_s1_aac.md` (Rule 17).
- Zero background tasks running at handoff (Rule 21).

---

## Fulfillment Section

- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-28T13:12:00-04:00`
- **Subagent Role**: `Rust no_std Codec Porter (S1 AAC)`
- **Summary Document**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo199_s1_aac.md`

### 1. Files Created, Modified, or Patched
1. `rust/vendor/syom/`:
   - Vendored `syom 0.7.1` from crates.io.
   - Patched `Cargo.toml`, `src/lib.rs`, `src/nostd_prelude.rs`, `src/lazy.rs`, `src/fmath.rs`, `src/engine/imdct.rs`.
   - Replaced `std::sync::LazyLock`/`OnceLock` with `LazyBox<T>` and `OnceBoxCell<T>` using `once_cell::race::OnceBox` (allocating in PSRAM heap, zero static DRAM overhead).
   - Routed `f32`/`f64` math via `libm` traits (`F32Math`, `F64Math`).
   - Wrote `rust/vendor/syom/VENDOR.md`.
2. `rust/crates/squeezelite-decoders/Cargo.toml`:
   - Enabled `syom` under `alloc = ["dep:syom"]`.
   - Added `[patch.crates-io] syom = { path = "vendor/syom" }` in root `rust/Cargo.toml`.
3. `rust/crates/squeezelite-decoders/src/lib.rs`:
   - Made `AacDecoder` available under `#[cfg(feature = "alloc")]`.
   - Used `syom::DecodeOptions::audio()` for stereo split channels.
   - Added `has_pending_pcm(&self) -> bool` and inherent `pub fn flush(&mut self)`.
4. `rust/crates/squeezelite-decoders/tests/fixtures/sine_440_1000_stereo_44k1_lc.aac`:
   - Committed test fixture (44.1 kHz, stereo AAC-LC).
5. `rust/crates/squeezelite-decoders/tests/aac_alloc_equivalence.rs`:
   - Verified bit-identical PCM between `std` and `no_std + alloc` (`23,552` frames, FNV-1a64 `0xec34c84f47bbd5ed`).
6. `rust/crates/louder-esp32-app/src/pipeline.rs`:
   - Promoted `ActiveDecoder::Aac` to unconditional (available on Xtensa bare metal).
   - Added microsecond decode metrics (`decode_us_max`, `decode_us_first`) in `StreamPipeline` and `process_decode_step`.
7. `rust/crates/louder-esp32-app/src/http_control.rs`:
   - Added `decode_us_max` and `decode_us_first` to `HttpStatusSnapshot`, `write_status_json`, and `Default`.
8. `rust/crates/louder-esp32-app/src/main.rs`:
   - Added `ActiveDecoder::Aac` dispatch on `strm 'a'` across both format dispatch sites with PSRAM check (`psram_avail_bytes >= 1_048_576`).
   - Exposed `decode_us_max` and `decode_us_first` in `HttpStatusSnapshot` for `/status.json`.
   - Added `,aac` to `BASEMENT_HELO_CAPABILITIES`.
   - Tuned internal DRAM heap allocation to `size: 45 * 1024` preserving `.stack >= 36,000 B` (`36,896 B`).

### 2. Mandatory Verification Gates
1. **Gate 1 (`google_rust_audit.py --strict`)**:
   - `0 error(s), 0 warning(s)` (PASS).
2. **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**:
   - `0` warnings (`exit code 0`, PASS).
3. **Gate 3 (`cargo test --workspace` & `aac_alloc_equivalence`)**:
   - `0` failures across entire workspace (`exit code 0`, PASS).
   - `aac_alloc_equivalence` passes in both `std` and `no_std + alloc` modes.
4. **Gate 4 (Xtensa Release Build & Section Sizes)**:
   - Release build for `xtensa-esp32-none-elf` completed successfully (`exit code 0`).
   - `.stack`: `0x009020` = `36,896` bytes (`>= 36,000` bytes).
   - `.dram2_uninit`: `0x018098` = `98,456` bytes (`<= 98,768` bytes).
5. **Gate 5 (Staged OTA Image Verification)**:
   - Command: `espflash save-image --chip esp32 ... /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s1-aac.bin`
   - Image size: `1,309,856` bytes (`48.75%` of `ota_0` partition capacity `< 2,686,976`).
   - Image SHA-256: `62d64ca4ac3f9ca87342c86d03d91c281f34703676c6952593caca7ca1e5d53f`.
   - Verification script: `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.
6. **Constraint Audits**:
   - Baseline `baseline_5444e41b` untouched (SHA-256: `5444e41ba2a7cdee27efc630598d25782c23a56bc221af4ee847085dbc103d67`).
   - Prior staged `louder-esp32-app-basement-ota.bin` untouched (SHA-256: `5444e41ba2a7cdee27efc630598d25782c23a56bc221af4ee847085dbc103d67`).
   - Stage-only compliance: Zero network requests to `172.22.1.15`, zero serial probes on `/dev/ttyUSB0`, zero changes to LMS `10.0.0.10`.
