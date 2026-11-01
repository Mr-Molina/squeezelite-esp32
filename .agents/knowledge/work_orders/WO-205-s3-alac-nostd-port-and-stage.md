# Work Order: WO-205 — S3: ALAC (`alac 0.5.0`) `no_std + alloc` Vendor Port, In-Memory MP4 Demuxer, Firmware Wiring & Stage-Only Build

- **Work Order ID**: `WO-205`
- **Timestamp**: `2026-10-28T20:30:00-04:00`
- **Assigned Subagent Role**: `Rust no_std Codec Porter (S3 ALAC)`
- **Plan**: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/implementation_plan.md` (Step S3)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective
Vendor `alac 0.5.0` into `rust/vendor/alac`, port it to `no_std + alloc`, enable `AlacDecoder` under feature `alloc = ["dep:syom", "dep:lewton", "dep:alac"]` in `squeezelite-decoders`, implement an in-memory streaming MP4 atom demuxer that parses `ftyp`, `moov`, `stsd`, `alac`, `stsz`, and extracts `mdat` packets into `alac::Decoder`, wire LMS `strm` format byte `'l'` (`alc`) to `ActiveDecoder::Alac` in `louder-esp32-app/src/main.rs`, add `,alc` to `BASEMENT_HELO_CAPABILITIES`, pass all host tests and static audits, and build a staged OTA image. **Stage-only: do NOT flash any device.**

## 2. Hypothesis (Rule 6)
- *If this works*: `alac 0.5.0` builds under `no_std + alloc` for `xtensa-esp32-none-elf`, `AlacDecoder` decodes ALAC audio in-memory on host with 100% bit-for-bit equivalence between `std` and `alloc` builds, and the staged image stays under `0x290000` (`2,686,976` B) with `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
- *If it fails*: The in-memory MP4 atom parser encounters unsupported MP4 container variants or `alac 0.5.0` requires unexpected standard library features.

## 3. Hard Constraints
1. **Preserve Verified Baselines**:
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_5444e41b/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s1_aac_530fade4/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s2_vorbis_bd49e6e0/`.
2. **Isolation Guarantee**:
   - Do NOT flash or send HTTP requests to `172.22.1.15`.
   - Do NOT touch `/dev/ttyUSB0`.
   - Do NOT modify LMS `10.0.0.10`.
3. **Stage-Only Output**:
   - Stage binary to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s3-alac.bin`.
4. **Style & Memory Guardrails**:
   - `google_rust_audit.py --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - All dynamic decoder allocations must come from `esp_alloc::HEAP` (PSRAM). Keep decoder struct small on the stack (< 1024 B).

## 4. Execution Steps

1. **Vendor and Port `alac 0.5.0` (`rust/vendor/alac/`)**:
   - Copy `alac 0.5.0` from cargo registry to `rust/vendor/alac`.
   - Remove unused modules `caf.rs`, `mp4.rs`, `reader.rs`. Keep `bitcursor.rs`, `dec.rs`, `lib.rs`.
   - Configure `Cargo.toml`: `default-features = false`.
   - In `rust/vendor/alac/src/lib.rs`: `#![no_std]`, `extern crate alloc;`.
   - In `rust/vendor/alac/src/bitcursor.rs`: replace `use std::cmp` with `use core::cmp`.
   - In `rust/vendor/alac/src/dec.rs`: replace `use std::cmp::min` with `use core::cmp::min`, use `alloc::boxed::Box`, `alloc::vec`.
   - Author `rust/vendor/alac/VENDOR.md`.
   - Add `[patch.crates-io] alac = { path = "vendor/alac" }` to `rust/Cargo.toml`.

2. **MP4 Demuxer & `AlacDecoder` in `squeezelite-decoders`**:
   - In `squeezelite-decoders/Cargo.toml`: update `alloc = ["dep:syom", "dep:lewton", "dep:alac"]`, add `alac = { version = "0.5.0", default-features = false, optional = true }`.
   - Implement `AlacDecoder` in `squeezelite-decoders`:
     - Maintain parsing state: `SearchingMoov`, `ParsingMoov`, `ReadingMdat`.
     - In `SearchingMoov` / `ParsingMoov`: parse MP4 atoms. Descend into container atoms `moov`, `trak`, `mdia`, `minf`, `stbl`. At `stsd`, skip 16 bytes. At `alac`, extract the `ALACSpecificConfig` (at offset 36) and initialize `alac::Decoder::new(StreamInfo::from_cookie(&cookie)?)`.
     - At `stsz`, read the packet size table (`Vec<usize>`).
     - At `mdat`, stream packets into `decoder.decode_packet()` and buffer interleaved 16-bit / 32-bit PCM in `pending_pcm`.
     - Implement `has_pending_pcm(&self) -> bool` and `flush(&mut self)`.

3. **Bit-for-Bit Host Equivalence Test**:
   - Commit a short ALAC test fixture in `crates/squeezelite-decoders/tests/fixtures/test_stereo_05s_44k1.m4a`.
   - Create `crates/squeezelite-decoders/tests/alac_alloc_equivalence.rs`, verify successful decoding and bit-for-bit PCM consistency across `std` and `alloc` configurations.

4. **Firmware Wiring in `louder-esp32-app`**:
   - In `pipeline.rs`: add `ActiveDecoder::Alac(AlacDecoder)` under `#[cfg(feature = "alloc")]` (or unconditional), wire into `has_pending_pcm()`, and instrument decode step timing.
   - In `main.rs`: wire format byte `'l'` (`alc`) to `ActiveDecoder::Alac` across both `strm` dispatch sites with PSRAM check `psram_avail_bytes >= 1_048_576`.
   - Add `,alc` to `BASEMENT_HELO_CAPABILITIES` and update test assertions.

5. **Stage-Only Verification Gates**:
   - `cargo test --workspace` -> 0 failures.
   - `python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - Xtensa release build, check `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - Export and verify staged image:
     `espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s3-alac.bin`
     Run `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.

## 5. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo205_s3_alac.md`.
- Confirm 0 background tasks at handoff.

---

## Fulfillment Section

### 1. Vendored Crate & Patches (`rust/vendor/alac`)
- Vendored `alac 0.5.0` into `rust/vendor/alac/`.
- Pruned unused modules `caf.rs`, `mp4.rs`, `reader.rs`. Kept `bitcursor.rs`, `dec.rs`, `lib.rs`.
- Configured manifest `Cargo.toml`: `default-features = false`, zero external dependencies.
- Added `#![no_std]` unconditionally to `src/lib.rs` and `extern crate alloc;`.
- Switched to `core::cmp` and `core::str::FromStr` in `src/bitcursor.rs` and `src/lib.rs`.
- Switched to `alloc::{boxed::Box, vec}` in `src/dec.rs`.
- Authored `rust/vendor/alac/VENDOR.md` and registered `[patch.crates-io] alac = { path = "vendor/alac" }` in `rust/Cargo.toml`.

### 2. Squeezelite-Decoders In-Memory MP4 Demuxer & `AlacDecoder`
- Updated `rust/crates/squeezelite-decoders/Cargo.toml`: `alloc = ["dep:syom", "dep:lewton", "dep:alac"]`, `alac = { version = "0.5.0", default-features = false, optional = true }`.
- Implemented `AlacDecoder` under `#[cfg(feature = "alloc")]` (stack footprint: 200 bytes, well under 1024 B budget).
- Streaming in-memory MP4 atom demuxer traverses `ftyp`, `moov`, `trak`, `mdia`, `minf`, `stbl`, skips FullBox header (16 B) in `stsd`, parses `ALACSpecificConfig` (at offset 36) in `alac` AudioSampleEntry into `alac::StreamInfo::from_cookie()`, parses packet size table from `stsz` (`sample_sizes: Vec<usize>`), parses first chunk offset from `stco` / `co64`, and transitions to `ReadingMdat` at `mdat`.
- Decodes packet by packet using `alac::Decoder::decode_packet::<i32>` with interleaved stereo sample output and mono duplication.
- Implemented `has_pending_pcm(&self) -> bool` and `flush(&mut self)`.

### 3. Bit-for-Bit Host Equivalence Test
- Committed 0.5s stereo sine fixture `rust/crates/squeezelite-decoders/tests/fixtures/test_stereo_05s_44k1.m4a`.
- Implemented `rust/crates/squeezelite-decoders/tests/alac_alloc_equivalence.rs`:
  - `cargo test -p squeezelite-decoders --test alac_alloc_equivalence` -> PASS (22,050 frames, FNV-1a64 `0x4d97_2ad9_5e5d_0401`).
  - `cargo test -p squeezelite-decoders --no-default-features --features alloc --test alac_alloc_equivalence` -> PASS (22,050 frames, FNV-1a64 `0x4d97_2ad9_5e5d_0401`).
  - Proved 100% bit-for-bit PCM identity between `std` and `alloc` builds.

### 4. Firmware Wiring in `louder-esp32-app`
- Wired `ActiveDecoder::Alac` unconditionally in `pipeline.rs`, added to `has_pending_pcm()`, `sample_rate()`, and `select_format(b'l')`.
- Wired format byte `'l'` in `main.rs` across both `strm` dispatch sites with PSRAM guard `psram_avail_bytes >= 1_048_576`.
- Added `ActiveDecoder::Alac(_) => 16` to `max_decode_steps` in `main.rs`.
- Exposed `"ALAC"` in telemetry `codec_name`.
- Appended `,alc` to `BASEMENT_HELO_CAPABILITIES` and updated unit test assertion.

### 5. Verification Gates & Stage-Only Artifact
1. `cargo test --workspace` -> 0 failures across all crates and integration tests (`exit code 0`, PASS).
2. `google_rust_audit.py --strict` -> 0 errors, 0 warnings (`exit code 0`, PASS).
3. `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings (`exit code 0`, PASS).
4. Xtensa release build (`cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`):
   - `.stack`: `0x008f68` = 36,712 bytes (`>= 36,000` bytes budget, PASS).
   - `.dram2_uninit`: `0x018098` = 98,456 bytes (`<= 98,768` bytes budget, PASS).
5. Staged OTA binary:
   - Path: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s3-alac.bin`
   - Size: 1,389,952 bytes (51.73% of `ota_0` limit 2,686,976 bytes).
   - SHA-256: `80a8cb94fed1ff057c80199fc8c9b7f6a080992e5b2177f3b1f0f7e1e5858416`.
   - `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.
6. Baseline & Isolation:
   - `baseline_5444e41b` SHA-256 verified untouched.
   - `baseline_s1_aac_530fade4` SHA-256 verified untouched.
   - `baseline_s2_vorbis_bd49e6e0` SHA-256 verified untouched.
   - Zero network packets to `172.22.1.15`, zero serial probes to `/dev/ttyUSB0`, zero LMS changes.
