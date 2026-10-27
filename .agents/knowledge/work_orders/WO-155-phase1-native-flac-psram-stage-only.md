# Work Order `WO-155`: Phase 1 — Native On-Chip Lossless FLAC Decoding in `4 MB` PSRAM (`--stage-only`)

- **Work Order ID**: `WO-155`
- **Timestamp**: `2026-10-27T08:01:00-04:00`
- **Assigned Subagent Role**: `louder-esp32-app` Implementer & `--stage-only` Packager (`6b5668dc-cfae-4206-9564-b607ab122f29`)
- **Status**: `[IN PROGRESS]`
- **Safety Tier**: `TIER-3-STAGE-ONLY` (Zero network flashes to `172.22.1.15`; zero serial probes on `/dev/ttyUSB*`)

---

## 1. Objective & Scope
Implement **Menu Phase 1: Native On-Chip Lossless FLAC Decoding in `4 MB` PSRAM**:
1. Replace the `#[cfg(feature = "std")]`-only `claxon` dependency inside `FlacDecoder` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` with a 100% pure-Rust `#![no_std]` + `alloc` (`#![forbid(unsafe_code)]`) lossless FLAC decoder (`#[cfg(any(feature = "alloc", feature = "std"))]`) that runs identically on both host (`cargo test --workspace`) and bare-metal `xtensa-esp32-none-elf`.
2. Guarantee **zero Internal DRAM heap consumption**, **zero `.bss` growth**, **zero `.dram2_uninit` growth**, and **zero CPU `.stack` growth** on `xtensa-esp32-none-elf` by pre-allocating `FlacDecoder`'s three buffers (`stream_buf`, `pending_pcm`, and `subframe_scratch`) as `65,536`-byte (`64 KB`) heap allocations in `FlacDecoder::new()`:
   - Because `esp-alloc` on `louder-esp32-app` has Region 0 (`46 KB` `Internal`) and Region 1 (`18 KB` `Internal`), any allocation `>= 65,536` bytes (`64 KB`) **cannot fit** in `Internal` DRAM and is **100% deterministically** placed in Region 2 (`3.53 MB` free External PSRAM at `0x3F80_0000`)!
   - Specifically, in `FlacDecoder::new()`:
     - `stream_buf: Vec::with_capacity(MAX_STREAM_BUF)` (`65,536` bytes = `64 KB`)
     - `pending_pcm: Vec::with_capacity(16384)` (`16,384 * 4 = 65,536` bytes = `64 KB`, holding up to `8,192` stereo frames)
     - `subframe_scratch: vec![0i64; 8192].into_boxed_slice()` (`8,192 * 8 = 65,536` bytes = `64 KB`, split into `ch0: [0..4096]` and `ch1: [4096..8192]`)
3. Enable `features = ["alloc"]` on `squeezelite-decoders` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml` and wire `ActiveDecoder::Flac` across `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
4. Verify all 5 `--stage-only` gates (`cargo test --workspace` including cross-validated `claxon` vs native `FlacDecoder` tests on real `.flac` bitstreams, `cargo clippy --workspace --all-targets -- -D warnings`, `xtensa-esp32-none-elf` release build, `readelf -S` `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B, and `verify_staged_ota_bin.py`).

---

## 2. Technical Specifications

### A. Rule 35 Pre-Modification Backups
Before modifying any file, copy backups to `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/`:
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/Cargo.toml` -> `squeezelite_decoders_Cargo.toml.wo155.bak`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` -> `squeezelite_decoders_lib.rs.wo155.bak`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml` -> `louder_esp32_app_Cargo.toml.wo155.bak`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` -> `louder_esp32_app_pipeline.rs.wo155.bak`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` -> `louder_esp32_app_main.rs.wo155.bak`

### B. Pure-Rust `#![no_std]` + `alloc` `FlacDecoder` (`squeezelite-decoders/src/lib.rs`)
1. Gate `FlacDecoder` on `#[cfg(any(feature = "alloc", feature = "std"))]` (using `vec::Vec` and `Box` imported at the top of `squeezelite-decoders/src/lib.rs`).
2. Keep existing public/crate fields (`pub sample_rate: u32`, `pub bits_per_sample: u32`, `pub channels: u32`, `header_len: usize`, `stream_buf: vec::Vec<u8>`, `pub pending_pcm: vec::Vec<i32>`, `pub pending_offset: usize`) plus:
   - `subframe_scratch: Box<[i64]>` (`vec![0i64; 8192].into_boxed_slice()`)
   - `meta_skip_remaining: usize` (so oversized non-`STREAMINFO` metadata blocks such as embedded `PICTURE` album art or large `PADDING` blocks are skipped in `O(1)` memory without overflowing `MAX_STREAM_BUF`)
   - `in_metadata: bool` (tracks whether metadata blocks after `"fLaC"` are still being parsed)
   - `min_block_size: u16` / `max_block_size: u16`
3. Add `pub fn has_pending_pcm(&self) -> bool { self.pending_offset < self.pending_pcm.len() }`.
4. **Pending PCM Fast Drain**:
   - At the start of `FlacDecoder::decode`:
     If `self.pending_offset < self.pending_pcm.len()`, copy `to_copy = core::cmp::min(pending_frames, max_frames)` stereo frames (`to_copy * 2` samples) into `pcm_out`, advance `self.pending_offset`, clear `self.pending_pcm` when `self.pending_offset >= self.pending_pcm.len()`, and **immediately return `Ok((0, to_copy))`**!
     This guarantees that draining `pending_pcm` in `StreamPipeline::process_decode_step()` is a deterministic `O(1)` `memcpy` and never consumes new input or triggers LPC decoding while `pending_pcm` is non-empty.
5. **Metadata & Stream Header Parsing (`"fLaC"` / `ID3`)**:
   - Support skipping leading `ID3`v2 tags or prefix bytes before `b"fLaC"`.
   - When `b"fLaC"` is encountered, parse metadata blocks:
     - 4-byte metadata block header: `is_last = (b0 & 0x80) != 0`, `block_type = b0 & 0x7F`, `len = u24 BE`.
     - `block_type == 0` (`STREAMINFO`, `34` bytes): parse `min_block_size`, `max_block_size`, 20-bit `sample_rate`, 3-bit `channels - 1`, 5-bit `bits_per_sample - 1`. Record `self.header_len` (`42` bytes for `"fLaC" + STREAMINFO` so `test_decoder_flushing` compatibility is preserved if `header_len` is set manually in tests, and in normal streaming consume processed metadata bytes or keep only the compact 42-byte `fLaC` + `STREAMINFO` prefix if `header_len <= 42`).
     - Wait: look at `test_decoder_flushing` (`squeezelite-decoders/src/lib.rs:1578-1588`):
       ```rust
       let mut flac = FlacDecoder::new();
       flac.stream_buf.extend_from_slice(b"fLaC00001111");
       flac.header_len = 8;
       flac.pending_pcm.extend_from_slice(&[50, -50, 60, -60]);
       let mut flac_out = [0i32; 2];
       assert_eq!(flac.decode(&[], &mut flac_out), Ok((0, 1)));
       assert_eq!(flac_out, [50, -50]);
       flac.flush();
       assert!(flac.pending_pcm.is_empty());
       assert_eq!(flac.stream_buf.len(), 8);
       ```
       Notice how `test_decoder_flushing` sets `flac.header_len = 8` and asserts `assert_eq!(flac.stream_buf.len(), 8)` after `flac.flush()`!
       And in `flush(&mut self)`:
       ```rust
       self.pending_pcm.clear();
       self.pending_offset = 0;
       self.meta_skip_remaining = 0;
       self.in_metadata = false;
       if self.header_len > 0 && self.stream_buf.len() >= self.header_len {
           self.stream_buf.truncate(self.header_len);
       } else {
           self.stream_buf.clear();
           self.header_len = 0;
       }
       ```
       And if a new `"fLaC"` stream header arrives after `flush()`, detect `b"fLaC"` at the start of the new data and reset `header_len = 0` so switching between tracks works seamlessly!
6. **RFC 9639 FLAC Frame Decoder**:
   - **Frame Sync & Header CRC-8 (`0x07`)**:
     - Scan for `0xFF` followed by `(b1 & 0xFC) == 0xF8`.
     - Parse `block_size_code` (4 bits), `sample_rate_code` (4 bits), `channel_assignment` (4 bits), `sample_size_code` (3 bits), reserved bit `0` (`== 0`), UTF-8 coded frame/sample number (`1..=7` bytes), optional 8/16-bit block size (`code 6/7`), optional 8/16-bit sample rate (`code 12/13/14`), and 1-byte **CRC-8** (`poly = 0x07`, init `0x00`).
     - Validate `block_size` (`1..=4096`), `channels` (`1` or `2`), `bits_per_sample` (`8`, `12`, `16`, `20`, `24`, `32`), and **CRC-8**. If header CRC-8 fails, it is a false sync word — advance 1 byte and continue scanning.
   - **Subframe Decoding (1 or 2 channels into `self.subframe_scratch`)**:
     - Account for side-channel extra bit (`bps + 1`) on `LeftSide` (`channel_assignment == 8`, ch 1), `RightSide` (`channel_assignment == 9`, ch 0), and `MidSide` (`channel_assignment == 10`, ch 1).
     - Parse subframe header: bit 7 `== 0`, 6-bit `subframe_type`, bit 0 `wasted_bits_flag` (if `1`, read unary zeros + `1` for `wasted_bits`; effective bps `eff_bps = subframe_bps - wasted_bits`).
     - Support:
       - `Constant` (`subframe_type == 0`): 1 signed `eff_bps`-bit value repeated `block_size` times.
       - `Verbatim` (`subframe_type == 1`): `block_size` signed `eff_bps`-bit values.
       - `Fixed` (`subframe_type == 8..=12`, `order = (subframe_type & 7) as usize`, `order <= 4`): `order` warm-up samples (`eff_bps` signed bits), Rice/Rice2 residual for `block_size - order` samples, and exact integer polynomial reconstruction (`order 0..=4`).
       - `LPC` (`subframe_type == 32..=63`, `order = ((subframe_type & 0x1F) + 1) as usize`, `1..=32`): `order` warm-up samples (`eff_bps` signed bits), 4-bit `qlp_prec` (`+ 1`, `15` invalid), 5-bit signed `qlp_shift` (`>= 0`), `order` signed `qlp_prec`-bit coefficients, Rice/Rice2 residual for `block_size - order` samples, and 64-bit accumulator reconstruction `s[i] = res[i] + (sum >> qlp_shift)`.
     - Shift samples left by `wasted_bits` if `wasted_bits > 0`.
   - **Footer Alignment & CRC-16 (`0x8005`)**:
     - Align bit reader to next byte boundary and read 16-bit big-endian CRC-16 (`poly = 0x8005`, init `0x0000`).
     - Verify CRC-16 over the frame bytes (`frame_start .. frame_end - 2`).
     - **Incomplete vs Corrupted Frame Distinction**:
       - If the bit reader runs out of bytes in `self.stream_buf` before completing the frame + CRC-16, the frame is simply **incomplete** (`NeedMoreBytes`): stop the loop, keep the unconsumed frame bytes in `self.stream_buf`, and return `Ok((input_taken, frames_out))` (or `Err(DecoderError::NeedMoreData)` if `input_taken == 0 && frames_out == 0`).
       - If all bytes of the frame were present in `self.stream_buf` and syntax or CRC-16 failed, advance past the frame sync byte and flag `corrupted_block = true`.
   - **Channel Decorrelation & 32-bit MSB-Aligned Output (`sample_shift`)**:
     - `Independent` (`0` = mono duplicated to L & R; `1` = stereo L & R).
     - `LeftSide` (`8`): `L = ch0`, `R = ch0 - ch1`.
     - `RightSide` (`9`): `L = ch1 + ch0`, `R = ch1`.
     - `MidSide` (`10`): `mid = (ch0 << 1) | (ch1 & 1); L = (mid + ch1) >> 1; R = (mid - ch1) >> 1`.
     - Shift each sample left by `Self::sample_shift(self.bits_per_sample)` (`8 -> 24`, `12 -> 20`, `16 -> 16`, `20 -> 12`, `24 -> 8`, `32 -> 0`) into `i32`.
     - Write up to `available_space` stereo frames into `pcm_out` and push any remaining stereo frames of the block into `self.pending_pcm`.

### C. Wiring `ActiveDecoder::Flac` on Xtensa (`Cargo.toml`, `pipeline.rs`, `main.rs`)
1. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`:
   - Change line 18 from:
     `squeezelite-decoders = { path = "../squeezelite-decoders", default-features = false }`
     to:
     `squeezelite-decoders = { path = "../squeezelite-decoders", default-features = false, features = ["alloc"] }`
2. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`:
   - Import `alloc::boxed::Box` when `#[cfg(target_arch = "xtensa")]` (`extern crate alloc; use alloc::boxed::Box;`).
   - Enable `ActiveDecoder::Flac(Box<squeezelite_decoders::FlacDecoder>)` on ALL targets (remove `#[cfg(not(target_arch = "xtensa"))]` from `Flac` in `ActiveDecoder`, `AudioDecoder for ActiveDecoder`, `ActiveDecoder::sample_rate`, and `StreamPipeline::select_format`).
   - Add `pub fn has_pending_pcm(&self) -> bool` on `ActiveDecoder`:
     - `Self::Flac(d) => d.has_pending_pcm(),`
     - `#[cfg(not(target_arch = "xtensa"))] Self::Aac(d) => d.pending_offset < d.pending_pcm.len(),`
     - `#[cfg(not(target_arch = "xtensa"))] Self::Vorbis(d) => d.pending_offset < d.pending_samples.len(),`
     - `_ => false,`
   - Update `StreamPipeline::has_pending_pcm(&self) -> bool`:
     `self.pcm_pending_samples > 0 || self.decoder.has_pending_pcm()`
   - Update `StreamPipeline::process_decode_step(&mut self)`:
     When `self.pcm_pending_samples == 0` and `self.decoder.has_pending_pcm()` is `true`:
     - Check `if self.output_buffer.available_write() < 8 { return Ok(0); }`
     - Call `self.decoder.decode(&[], &mut self.pcm_scratch)` directly (without requiring `self.stream_buffer.available_read() > 0` or peeking `self.stream_buffer`), then run EQ + volume scaling and push into `self.output_buffer`!
3. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - Line 78 (`run_firmware_tick` `max_decode_steps`):
     `ActiveDecoder::Mp3(_) | ActiveDecoder::Flac(_) => 16,`
   - Line 1756 (`codec_name`):
     Add `ActiveDecoder::Flac(_) => "FLAC",`
   - Lines 2202–2224 (`StreamCommandType::Start`) and Lines 2507–2544 (`opcode == [b'c', b'o', b'd', b'c']`):
     - In `AudioFormat::Mp3` arm: change `ActiveDecoder::Pcm(_) =>` to `_ =>` so switching from `Flac` or `Pcm` to `Mp3` works cleanly.
     - Add `else if format == squeezelite_proto::AudioFormat::Flac` (and `else if codc_fmt == squeezelite_proto::AudioFormat::Flac`):
       Release `MP3_DEC_TAKEN` if `ActiveDecoder::Mp3(_)`, reuse existing `ActiveDecoder::Flac(ref mut dec)` via `dec.flush()` or allocate `ActiveDecoder::Flac(alloc::boxed::Box::new(squeezelite_decoders::FlacDecoder::new()))`, and call `pipeline.sink.set_mute(false)`.

---

## 3. Mandatory 5-Gate `--stage-only` Verification Protocol
1. **Gate 1 (`cargo test --workspace`)**:
   - Include unit tests in `squeezelite-decoders` testing real FLAC frames (`Constant`, `Verbatim`, `Fixed 0..=4`, `LPC`, `Independent`/`LeftSide`/`RightSide`/`MidSide`, `16-bit` & `24-bit`, `4,096`-frame block draining across `2,304`-sample `pcm_out` buffer, and `claxon` sample-for-sample cross-validation) + `louder-esp32-app` pipeline FLAC streaming test.
2. **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**:
   - Must exit `0` with `0` warnings.
3. **Gate 3 (`xtensa-esp32-none-elf` Release Build)**:
   - `cd /home/console-admin/Github/squeezelite-esp32/rust && source /home/console-admin/export-esp.sh && unset CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
4. **Gate 4 (`readelf -S` Stack & DRAM2 Budget Gate)**:
   - Verify `.stack >= 36,000` bytes (`36,992` B expected) and `.dram2_uninit <= 98,768` bytes (`98,264` B expected).
5. **Gate 5 (`espflash save-image` & `verify_staged_ota_bin.py`)**:
   - Stage `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` and verify `[STAGE-ONLY AUDIT PASS]`.

---

## 4. Fulfillment Section (To be completed by Subagent)
- **Status**: `[EXECUTED & VERIFIED]`
- **Timestamp**: `2026-10-27T13:15:00Z`
- **Subagent**: `6b5668dc` (`louder-esp32-app Implementer & --stage-only Packager`)
- **Key Deliverables & Implementations**:
  1. `squeezelite-decoders/Cargo.toml`: Moved `claxon` from dependencies to `[dev-dependencies]` for host-side verification tests; removed `claxon` from runtime `std` feature.
  2. `squeezelite-decoders/src/lib.rs`: Implemented pure-Rust `#![no_std]` + `alloc` (`#![forbid(unsafe_code)]`) RFC 9639 lossless FLAC decoder gated on `#[cfg(any(feature = "alloc", feature = "std"))]`.
     - Deterministic 4 MB PSRAM buffer placement: `stream_buf: Vec::with_capacity(65536)` (64 KB), `pending_pcm: Vec::with_capacity(16384)` (64 KB), and `subframe_scratch: vec![0i64; 8192].into_boxed_slice()` (64 KB).
     - Full RFC 9639 feature set: Constant, Verbatim, Fixed 0..=4, LPC 1..=32 (with 64-bit wide math and chronological coefficient ordering $c_j \cdot s_{i-1-j}$), Rice & Rice2 residual partitions with escape fallbacks, channel decorrelations (`Independent`, `LeftSide`, `RightSide`, `MidSide`), 8/12/16/20/24/32-bit sample depths, and streaming $O(1)$ metadata skipping.
     - Fast non-blocking PCM drainage via `has_pending_pcm(&self) -> bool` and pending drain loop.
     - **Rev 2 Real-Time Chunk Ingestion Fix**: Set `self.need_more_input = true;` on all partial-frame / metadata-incomplete paths (`meta_skip_remaining > 0`, `stream_buf.len() < 42`, `stream_buf.len() < header_len + 4`, `avail < total_block_bytes`, `avail_bytes < 4`, `sync_pos == None`, `frame_buf.len() < 4`, `parse_frame_header` `Ok(None)`, `subframe_incomplete`, `frame_buf.len() < frame_body_len + 2`). This ensures `has_pending_pcm()` returns `false` during multi-chunk streaming ingestion so `run_firmware_tick` (`main.rs`) does not break out early after 1 chunk and instead ingests chunks until a complete frame is decoded.
     - Added unit test `test_flac_partial_chunk_ingestion_need_more_input` asserting `Ok((500, 0))` and `!decoder.has_pending_pcm()` on 500-byte and 1,536-byte partial prefixes.
     - 100% sample-for-sample bit-exact parity against `claxon` across all fixtures (`mono_16bit.flac`, `stereo_16bit.flac`, `stereo_24bit.flac`, `fixed.flac`, `lpc.flac`).
  3. `louder-esp32-app/Cargo.toml`: Enabled `features = ["alloc"]` on `squeezelite-decoders`.
  4. `louder-esp32-app/src/pipeline.rs`:
     - Activated `ActiveDecoder::Flac(Box<FlacDecoder>)` across all architectures (`xtensa` and host).
     - Routed `ActiveDecoder::has_pending_pcm(&self)` to `FlacDecoder::has_pending_pcm()`.
     - Integrated `FlacDecoder` pending PCM draining in `process_decode_step` through DSP equalizer and volume scaling into `output_buffer`.
     - Enabled format selector `select_format(b'f' | b'F')` returning `ActiveDecoder::Flac(Box::new(FlacDecoder::new()))`.
     - Added unit test `test_pipeline_flac_playback_flow` ingesting 11,539-byte stereo 16-bit FLAC audio stream, confirming full decode and output of 8,820 stereo frames.
  5. `louder-esp32-app/src/main.rs`:
     - Defined `pub const BASEMENT_HELO_CAPABILITIES: &str = "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=96000,flc,pcm,wav,aif,mp3";` (Rev 3).
     - Passed `BASEMENT_HELO_CAPABILITIES` into `SlimprotoBuilder::build_helo` at line 1995, advertising native FLAC (`,flc,`) and `MaxSampleRate=96000` to LMS server (`10.0.0.10:9000`).
     - Added unit test `test_basement_helo_capabilities_flc_and_sample_rate` in `main.rs`.
     - Set `max_decode_steps = 16` for `ActiveDecoder::Flac(_)`.
     - Added `ActiveDecoder::Flac(_) => "FLAC"` in `codec_name`.
     - Wired dynamic format transitions for `AudioFormat::Flac` in `StreamCommandType::Start` and `opcode == b"codc"` with `dec.flush()` reuse or clean allocation.
  6. Verified 5-Gate `--stage-only` Protocol:
     - **Gate 1 (`cargo test --workspace`)**: PASS (100% across all crates, 0 failures, 27 decoder tests + 121 louder-esp32-app tests).
     - **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**: PASS (0 warnings, exit code 0).
     - **Gate 3 (`xtensa-esp32-none-elf` Release Build)**: PASS (cargo +esp release build finished in 4.86s, exit code 0).
     - **Gate 4 (`readelf -S` Budget Gate)**:
       - `.stack`: `36,920` bytes (`0x009038 >= 36,000` bytes threshold, +920 B margin).
       - `.dram2_uninit`: `98,264` bytes (`0x017fd8 <= 98,768` bytes limit, 504 B headroom).
       - `.bss`: `75,544` bytes (0 byte growth from baseline).
     - **Gate 5 (`verify_staged_ota_bin.py`)**: `[STAGE-ONLY AUDIT PASS]`
       - Path: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
       - Size: `955,072` bytes (`<= 2,686,976` bytes ceiling).
       - SHA-256: `aa55b7cc9af807799ae8f747830d7d2d5a02ae92fdb531a6c2b4ef5f742763d9`.
       - Headers: `img_magic: 0xE9`, `app_desc_magic: 0xABCD5432`, `project_name: 'Squeezelite-ESP32'`.
- **Handoff Path**: `.agents/knowledge/subagent_summaries/20261027_wo155_phase1_flac_psram_stage_only.md`
