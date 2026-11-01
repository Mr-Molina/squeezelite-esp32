# Work Order: WO-203 — S2: Ogg Vorbis (`lewton`) `no_std + alloc` Vendor Port, Firmware Wiring & Stage-Only Build

- **Work Order ID**: `WO-203`
- **Timestamp**: `2026-10-28T19:51:00-04:00`
- **Assigned Subagent Role**: `Rust no_std Codec Porter (S2 Vorbis)`
- **Plan**: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/implementation_plan.md` (Step S2)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective
Vendor `lewton 0.10.2` into `rust/vendor/lewton`, port it to `no_std + alloc` with `libm` float functions (`sin`, `cos`), enable `VorbisDecoder` under feature `alloc = ["dep:syom", "dep:lewton"]` in `squeezelite-decoders`, implement an in-memory Ogg page packet demuxer (demuxing `OggS` pages directly to Vorbis header/audio packets without `std::io::Read`/`Seek`), wire LMS `strm` format byte `'o'` to `ActiveDecoder::Vorbis` in `louder-esp32-app/src/main.rs`, add `,ogg` to `BASEMENT_HELO_CAPABILITIES`, pass all host tests and static audits, and build a staged OTA image. **Stage-only: do NOT flash any device.**

## 2. Hypothesis (Rule 6)
- *If this works*: `lewton` builds under `no_std + alloc` for `xtensa-esp32-none-elf`, `VorbisDecoder` decodes Ogg Vorbis audio on host with 100% bit-for-bit equivalence between `std` and `alloc` builds, and the staged image stays under `0x290000` with `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
- *If it fails*: `lewton`'s setup headers or codebooks exceed available memory or require missing standard library features.

## 3. Hard Constraints
1. **Preserve Verified Baselines**:
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_5444e41b/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s1_aac_530fade4/`.
2. **Isolation Guarantee**:
   - Do NOT flash or send HTTP requests to `172.22.1.15`.
   - Do NOT touch `/dev/ttyUSB0`.
   - Do NOT modify LMS `10.0.0.10`.
3. **Stage-Only Output**:
   - Stage binary to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s2-vorbis.bin`.
4. **Style & Memory Guardrails**:
   - `google_rust_audit.py --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - All Vorbis allocations must come from `esp_alloc::HEAP` (PSRAM). No multi-kilobyte structs on the stack (remember `Filterbank` lesson from WO-201/202).

## 4. Execution Steps

1. **Vendor and Port `lewton 0.10.2` (`rust/vendor/lewton/`)**:
   - Copy `lewton 0.10.2` from cargo registry to `rust/vendor/lewton`.
   - Configure `Cargo.toml`: `default-features = false`, add `libm = "0.2"`, `byteorder = { version = "1.0", default-features = false }`, `tinyvec = { version = "1.0", default-features = false, features = ["alloc"] }`.
   - In `rust/vendor/lewton/src/lib.rs`: `#![cfg_attr(not(feature = "std"), no_std)]`, `extern crate alloc;`.
   - Route float math in `header_cached.rs` and `audio.rs` through `libm` extension traits.
   - Provide `alloc` replacements for `Vec`, `Box`, `String` across `header.rs`, `audio.rs`, `imdct.rs`, etc.
   - Author `rust/vendor/lewton/VENDOR.md`.
   - Configure root `rust/Cargo.toml` with `[patch.crates-io] lewton = { path = "vendor/lewton" }`.

2. **Ogg Page Demuxer & `VorbisDecoder` in `squeezelite-decoders`**:
   - In `squeezelite-decoders/Cargo.toml`: update `alloc = ["dep:syom", "dep:lewton"]`.
   - In `squeezelite-decoders/src/lib.rs`:
     - Implement in-memory Ogg page parsing (`OggS` magic, capture segments, reassemble packet bytes).
     - Promote `VorbisDecoder` to `#[cfg(feature = "alloc")]` (replacing `#[cfg(feature = "std")]`).
     - Pass headers 1, 2, 3 to `lewton::header::read_header_ident`, `read_header_comment`, and `read_header_setup`.
     - Pass subsequent audio packets to `lewton::audio::read_audio_packet`.
     - Implement `has_pending_pcm(&self) -> bool` and `flush(&mut self)`.

3. **Bit-for-Bit Host Equivalence Test**:
   - Commit an Ogg Vorbis test fixture in `crates/squeezelite-decoders/tests/fixtures/sine_440_1000_stereo_44k1.ogg`.
   - In `crates/squeezelite-decoders/tests/vorbis_alloc_equivalence.rs`, verify identical PCM output between `std` and `alloc` builds.

4. **Firmware Wiring in `louder-esp32-app`**:
   - In `pipeline.rs`: promote `ActiveDecoder::Vorbis` to unconditional, wire `has_pending_pcm()`, and instrument timing in `process_decode_step()`.
   - In `main.rs`: wire `ActiveDecoder::Vorbis` on format byte `'o'` in both dispatch sites (with PSRAM check `psram_avail_bytes >= 1_048_576`), add `,ogg` to `BASEMENT_HELO_CAPABILITIES`.

5. **Stage-Only Verification Gates**:
   - `cargo test --workspace` -> 0 failures.
   - `python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - Xtensa release build, check `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - Export and verify staged image:
     `espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s2-vorbis.bin`
     Run `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.

## 5. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo203_s2_vorbis.md`.
- Confirm 0 background tasks at handoff.

---

## Fulfillment Section

### 1. Vendored `lewton 0.10.2` Port (`rust/vendor/lewton/`)
- Vendored `lewton 0.10.2` from crates.io with `edition = "2015"`.
- Configured manifest `Cargo.toml`: `default-features = false`, added `libm = "0.2"`, `byteorder = { version = "1.0", default-features = false }`, `tinyvec = { version = "1.0", default-features = false, features = ["alloc"] }`.
- Added `#![no_std]` unconditionally to `src/lib.rs` and scoped `std` under `#[cfg(feature = "std")]`.
- Implemented `fmath.rs` with `libm` wrapper functions (`sin`, `cos`, `atan`, `floor`, `sqrt`, `exp`, `exp2`, `abs`) ensuring bit-for-bit identical float math across both `std` and `no_std + alloc` environments.
- Implemented `SliceReader` in `src/header.rs` to decode Vorbis comment headers without `std::io::Cursor` or `ReadBytesExt`.
- Authored `rust/vendor/lewton/VENDOR.md` and registered `[patch.crates-io] lewton = { path = "vendor/lewton" }` in `rust/Cargo.toml`.

### 2. Squeezelite-Decoders In-Memory Ogg Demuxer & `VorbisDecoder`
- Updated `rust/crates/squeezelite-decoders/Cargo.toml`: `alloc = ["dep:syom", "dep:lewton"]`.
- Promoted `VorbisDecoder` to `#[cfg(feature = "alloc")]` (stack footprint: 112 bytes).
- Implemented streaming in-memory Ogg page parser and multi-page packet reassembler in `VorbisDecoder::decode()`.
- Routed header packets (`\x01vorbis`, `\x03vorbis`, `\x05vorbis`) to `read_header_ident`, `read_header_comment`, and `read_header_setup`.
- Implemented `has_pending_pcm(&self) -> bool` and `flush(&mut self)`.

### 3. Bit-for-Bit Host Equivalence Test
- Committed 0.5s stereo sine fixture `rust/crates/squeezelite-decoders/tests/fixtures/sine_440_1000_stereo_44k1.ogg`.
- Implemented `rust/crates/squeezelite-decoders/tests/vorbis_alloc_equivalence.rs`:
  - `cargo test -p squeezelite-decoders --test vorbis_alloc_equivalence` -> PASS (22,080 frames, FNV-1a64 `0xfe93_46f1_654b_ca6d`).
  - `cargo test -p squeezelite-decoders --no-default-features --features alloc --test vorbis_alloc_equivalence` -> PASS (22,080 frames, FNV-1a64 `0xfe93_46f1_654b_ca6d`).
  - Proved 100% bit-for-bit PCM identity between `std` and `alloc` builds.

### 4. Firmware Wiring in `louder-esp32-app`
- Promoted `ActiveDecoder::Vorbis` to unconditional in `pipeline.rs`, wired into `has_pending_pcm()`, and instrumented decode loops.
- Wired format byte `'o'` in `main.rs` across both `strm` dispatch sites with PSRAM guard `psram_avail_bytes >= 1_048_576`.
- Appended `,ogg` to `BASEMENT_HELO_CAPABILITIES`.

### 5. Verification Gates & Stage-Only Artifact
1. `cargo test --workspace` -> 0 failures across all crates and integration tests (`exit code 0`, PASS).
2. `google_rust_audit.py --strict` -> 0 errors, 0 warnings (`exit code 0`, PASS).
3. `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings (`exit code 0`, PASS).
4. Xtensa release build (`cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`):
   - `.stack`: `0x008f78` = 36,728 bytes (`>= 36,000` bytes budget, PASS).
   - `.dram2_uninit`: `0x018098` = 98,456 bytes (`<= 98,768` bytes budget, PASS).
5. Staged OTA binary:
   - Path: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s2-vorbis.bin`
   - Size: 1,377,600 bytes (51.27% of `ota_0` limit 2,686,976 bytes).
   - SHA-256: `bd49e6e007f0347f19fad00b13b03265156c81914feca4f39a5df61cb77b2edf`.
   - `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.
6. Baseline & Isolation:
   - `baseline_5444e41b` SHA-256 verified untouched.
   - `baseline_s1_aac_530fade4` SHA-256 verified untouched.
   - Zero network packets to `172.22.1.15`, zero serial probes to `/dev/ttyUSB0`, zero LMS changes.
