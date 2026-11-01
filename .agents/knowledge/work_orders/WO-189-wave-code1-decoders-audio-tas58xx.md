# Work Order `WO-189`: `/deep-code-audit` Phase 3 (`Wave CODE-1`) — Decoders, `nanomp3-core`, Audio Core & TAS5805M Driver Remediation

- **ID**: `WO-189`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Rust Decoders & Audio Core Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Wave CODE-1 Implementer** (`/deep-code-audit` Phase 3 + `rust-pro`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/audit_master_matrix.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Strictly Assigned Files (DO NOT TOUCH ANY OTHER FILES)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/**`

### Required Functional & `rust-pro` Remediations
1. **`squeezelite-decoders/src/lib.rs`**:
   - **`CODE-FLAC-001` (`CRITICAL`)**: Standard CD-DA `44.1 kHz` FLAC files use `4608`-sample blocks (`MAX_FLAC_BLOCK_SAMPLES = 4608`):
     - In `parse_frame_header` (around line 1108): change `if block_size == 0 || block_size > 4096` to `if block_size == 0 || block_size > MAX_FLAC_BLOCK_SAMPLES`.
     - In `FlacDecoder::new()` (around line 1455): change `subframe_scratch: vec![0i64; 8192]` to `subframe_scratch: vec![0i64; MAX_FLAC_BLOCK_SAMPLES * 2]`.
     - In `decode_frame_from_slice` (around lines 1373-1392 and 1709-1710): replace hardcoded `4096` channel-1 offsets (`let start = ch * 4096;`, `scratch[4096 + i]`, `scratch[ch * 4096..]`) with `MAX_FLAC_BLOCK_SAMPLES`.
     - Add a unit test `test_flac_4608_sample_block_decode` verifying a `4608`-sample stereo FLAC frame decodes `9216` interleaved samples without error.
   - **`CODE-FLAC-002` (`HIGH`)**: In `FlacDecoder::decode` (around lines 1555-1562), do NOT scan `self.stream_buf[self.header_len..]` for `b"fLaC"` during active mid-stream frame decoding (which false-matches compressed audio bytes); only check for `b"fLaC"` at `self.stream_buf[..4]` when `!self.header_parsed` or when re-initializing at stream start (`self.header_parsed && self.stream_buf.starts_with(b"fLaC")`).
   - **`CODE-FLAC-003` (`HIGH`)**: In `decode_subframe_lpc` (around lines 1251-1271): reject reserved `qlp_prec == 1` (`raw == 0`), allow `qlp_prec <= 16` (`if qlp_prec == 1 || qlp_prec > 16 { return Err(DecodeError::CorruptFrame); }`), and reject negative `qlp_shift < 0` (`if qlp_shift < 0 { return Err(DecodeError::CorruptFrame); }`). In `decode_residual` (around lines 1334-1346): bound unary quotient `q` (`if q > 65535 { return Err(DecodeError::CorruptFrame); }`) and use `checked_shl`.
   - **`CODE-DEC-001` (`HIGH`)**:
     - In `StaticMp3Decoder::decode` (around lines 651-656): allow decoding a complete terminal frame when `encoded_in.len() == frame_bytes` (only wait for next sync header when `frame_bytes == 0`, or when `encoded_in.len() > frame_bytes && encoded_in.len() < frame_bytes + 4`).
     - In `PcmDecoder::decode` (around lines 118-137, 178-193, 240-244): validate WAV `fmt ` `chunk_size >= 16` and `format_tag == 1`, AIFF `COMM` `chunk_size >= 18`, and `self.channels == 1 || self.channels == 2`.
2. **`nanomp3-core`**:
   - **`GRS-003`**: Move struct/enum/fn definitions out of `nanomp3-core/src/lib.rs` into a new leaf file `nanomp3-core/src/decoder.rs` so `nanomp3-core/src/lib.rs` contains ONLY module declarations (`mod decoder; mod minimp3; #[cfg(test)] mod tests;`) and `pub use decoder::{...};`.
   - **`GRS-002`**: Replace glob imports in `nanomp3-core/src/minimp3.rs:20` and `nanomp3-core/src/tests.rs:1` with explicit imports.
   - **`GRS-008` / `GRS-010` / `GRS-011`**: Add `// JUSTIFICATION:` + `// SAFETY:` on `nanomp3-core/src/minimp3/layer12.rs:230` (or replace with safe code), add a justification comment above `#[rustfmt::skip]` at `layer12.rs:32`, and replace `.unwrap()` in `nanomp3-core/src/minimp3/layer3.rs` with `.unwrap_or(...)` or pattern matching.
3. **`squeezelite-audio-core`**:
   - **`CODE-PIPE-001`**:
     - In `equalizer.rs`: add `pub fn reset_history(&mut self)` to zero `z1`/`z2` filter delay states across `left_bands`/`right_bands`; in `process_interleaved` (line 266), change `if !self.enabled && !self.enabled` to `if !self.enabled && !self.loudness_enabled`.
     - In `ringbuffer.rs`: in `rewind_read_internal` (lines 275-285), store `self.rewindable.store(avail_rewind - actual, Ordering::Release)` BEFORE `self.read_pos.store(new_read, Ordering::Release)`.
   - **`GRS-008`**: Add dual `// JUSTIFICATION:` and `// SAFETY:` comments directly above all 9 `unsafe` blocks in `squeezelite-audio-core/src/ringbuffer.rs`.
4. **`squeezelite-tas58xx/src/lib.rs`**:
   - **`CODE-TAS-001`**: Call `self.select_page0_book0()?` at the beginning of `set_digital_volume`, `check_faults`, `clear_faults`, and `recover_from_fault`.
5. **`GRS-012` Line & Comment Widths**:
   - Wrap all code lines `<= 100` cols and standalone comments `<= 80` cols across all 4 assigned crates (`squeezelite-decoders`, `nanomp3-core`, `squeezelite-audio-core`, `squeezelite-tas58xx`).
6. **Verification Gate**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders /home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx --strict` and verify **0 errors and 0 warnings** (`exit code 0`).
   - Run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core -p squeezelite-audio-core -p squeezelite-tas58xx` and verify 100% pass.
   - Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code1.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section
- **Execution Date**: `2026-10-27T18:55:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary**: [`20261027_wave_code1.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code1.md)

### Verification Summary
1. **Remediated Functional Bugs**:
   - `CODE-TAS-001` (`CRITICAL`): In `squeezelite-tas58xx/src/lib.rs`, prepended `self.select_page0_book0()?` to `set_digital_volume`, `check_faults`, `clear_faults`, and `recover_from_fault` in both `Standby` and `Play` states. Updated `embedded-hal-mock` unit test expectations.
   - `CODE-PIPE-001` (`HIGH`): In `squeezelite-audio-core/src/ringbuffer.rs:rewind_read_internal`, atomic `self.rewindable.fetch_update` is placed BEFORE `self.read_pos.store(new_read, Ordering::Release)`. In `equalizer.rs`, added `pub fn reset_history(&mut self)` and fixed `!self.enabled && !self.loudness_enabled` guard.
   - `CODE-FLAC-001` (`CRITICAL`): In `squeezelite-decoders/src/lib.rs`, defined `MAX_FLAC_BLOCK_SAMPLES = 4608`, updated header bounds check, sized `subframe_scratch` to `MAX_FLAC_BLOCK_SAMPLES * 2`, replaced hardcoded `4096` channel offsets with `MAX_FLAC_BLOCK_SAMPLES`, and added unit test `test_flac_4608_sample_block_decode`.
   - `CODE-FLAC-002` (`HIGH`): Removed mid-stream `b"fLaC"` scanning loop in `FlacDecoder::decode`.
   - `CODE-FLAC-003` (`HIGH`): Rejected reserved `qlp_prec == 1` and `qlp_shift < 0` in `decode_subframe_lpc`; bounded Rice quotient `q <= 65535` with `checked_shl` in `decode_residual`.
   - `CODE-DEC-001` (`HIGH`): Allowed complete terminal frame decoding in `StaticMp3Decoder::decode` when `encoded_in.len() == frame_bytes`. Validated WAV/AIFF header chunk sizes and channels in `PcmDecoder::decode`.

2. **Google Rust Style Guide (`rust-pro`)**:
   - `GRS-003`: Created leaf file `nanomp3-core/src/decoder.rs`. `nanomp3-core/src/lib.rs` strictly restricted to module declarations and re-exports.
   - `GRS-002`: Replaced glob imports in `nanomp3-core/src/minimp3.rs` and `nanomp3-core/src/tests.rs` with explicit symbol imports.
   - `GRS-008`: Added dual `// JUSTIFICATION:` and `// SAFETY:` comments on all 9 `unsafe` blocks in `ringbuffer.rs`, and on `layer12.rs:230` `unsafe` block; added justification above `#[rustfmt::skip]` at `layer12.rs:32`.
   - `GRS-010`: Replaced production `.unwrap()` calls in `minimp3.rs`, `lanes.rs`, and `layer12.rs` with safe `.unwrap_or(...)`.
   - `GRS-012`: Wrapped lines `<= 100` columns and comments `<= 80` columns across all 4 crates.

3. **Independent Verification Commands**:
   - `google_rust_audit.py --strict`:
     - `squeezelite-decoders`: **0 errors, 0 warnings**
     - `nanomp3-core`: **0 errors, 0 warnings**
     - `squeezelite-audio-core`: **0 errors, 0 warnings**
     - `squeezelite-tas58xx`: **0 errors, 0 warnings**
   - `CARGO_TARGET_DIR=/tmp/cargo-wave-code1 cargo test -p squeezelite-decoders -p nanomp3-core -p squeezelite-audio-core -p squeezelite-tas58xx`:
     - **79 passed, 0 failed** (100% pass across all 4 crates).
