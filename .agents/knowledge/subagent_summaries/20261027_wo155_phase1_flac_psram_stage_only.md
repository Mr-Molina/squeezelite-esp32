# Subagent Summary: `WO-155` Phase 1 Native On-Chip Lossless FLAC Decoding in 4 MB PSRAM (`--stage-only`)

- **Subagent Role**: `louder-esp32-app` Implementer & `--stage-only` Packager
- **Subagent ID**: `6b5668dc-cfae-4206-9564-b607ab122f29`
- **Parent Orchestrator**: `55587c74-4c6d-4688-90db-d3185fdddae1`
- **Work Order**: `WO-155` (`.agents/knowledge/work_orders/WO-155-phase1-native-flac-psram-stage-only.md`)
- **Status**: `[EXECUTED & VERIFIED]`
- **Timestamp**: `2026-10-27T13:15:00Z`

---

## 1. Files Modified and Created

### Files Modified:
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/Cargo.toml`:
   - Moved `claxon` from dependencies to `[dev-dependencies]` for test verification.
   - Removed `dep:claxon` from runtime `std` feature.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`:
   - Implemented pure-Rust `#![no_std]` + `alloc` (`#![forbid(unsafe_code)]`) RFC 9639 lossless FLAC decoder gated on `#[cfg(any(feature = "alloc", feature = "std"))]`.
   - Pre-allocated internal buffers to $\ge 64\text{ KB}$ heap allocations in `new()`: `stream_buf` (65,536 B), `pending_pcm` (16,384 `i32` = 64 KB), and `subframe_scratch` (8,192 `i64` = 64 KB), ensuring deterministic placement in Region 2 External 4 MB PSRAM (`0x3F80_0000`).
   - Implemented table-driven CRC-8 (`0x07`) and CRC-16 (`0x8005`) matching `libflac`.
   - Implemented `BitReader` with unary zeros fast-path and bit-level residual readers.
   - Implemented subframe decoding for Constant, Verbatim, Fixed orders 0..=4, and LPC orders 1..=32 with 64-bit wide accumulators and chronological coefficient ordering $c_j \cdot s_{i-1-j}$.
   - Implemented Rice and Rice2 residual partition decoders with escape fallbacks.
   - Implemented channel decorrelation: `Independent` (mono/stereo), `LeftSide`, `RightSide`, and `MidSide`.
   - Implemented `has_pending_pcm(&self) -> bool` and pending PCM fast drain returning `Ok((0, to_copy))`.
   - **Rev 2 Real-Time Chunk Ingestion Fix**: Set `self.need_more_input = true;` on all partial-frame / metadata-incomplete paths (`meta_skip_remaining > 0`, `stream_buf.len() < 42`, `stream_buf.len() < header_len + 4`, `avail < total_block_bytes`, `avail_bytes < 4`, `sync_pos == None`, `frame_buf.len() < 4`, `parse_frame_header` `Ok(None)`, `subframe_incomplete`, `frame_buf.len() < frame_body_len + 2`). This ensures `has_pending_pcm()` returns `false` during multi-chunk streaming ingestion so `run_firmware_tick` (`main.rs`) does not break out early after 1 chunk and instead ingests chunks until a complete frame is decoded.
   - Added unit tests `test_flac_cross_validation_all_fixtures`, `test_flac_block_draining_across_constrained_pcm_buffer`, and `test_flac_partial_chunk_ingestion_need_more_input`.
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`:
   - Enabled `features = ["alloc"]` on `squeezelite-decoders`.
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`:
   - Enabled `ActiveDecoder::Flac(Box<FlacDecoder>)` on all targets.
   - Routed `ActiveDecoder::has_pending_pcm(&self)` to `FlacDecoder::has_pending_pcm()`.
   - Updated `StreamPipeline::has_pending_pcm` and `process_decode_step` to drain decoder pending PCM through EQ + volume scaling into `output_buffer`.
   - Enabled `select_format(b'f' | b'F')`.
   - Added unit test `test_pipeline_flac_playback_flow` ingesting `stereo_16bit.flac`.
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - **Rev 3 HELO Capability Fix**: Defined `pub const BASEMENT_HELO_CAPABILITIES: &str = "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=96000,flc,pcm,wav,aif,mp3";` and passed it into `SlimprotoBuilder::build_helo` at line 1995. This advertises native FLAC (`,flc,`) and `MaxSampleRate=96000` to LMS server (`10.0.0.10:9000`), preventing LMS from transcoding FLAC streams to PCM.
   - Added unit test `test_basement_helo_capabilities_flc_and_sample_rate`.
   - Set `max_decode_steps = 16` for `ActiveDecoder::Flac(_)`.
   - Added `ActiveDecoder::Flac(_) => "FLAC"` in `codec_name`.
   - Wired dynamic format transitions for `AudioFormat::Flac` in `StreamCommandType::Start` and `opcode == b"codc"` with `dec.flush()` reuse or clean allocation.
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-155-phase1-native-flac-psram-stage-only.md`:
   - Populated Fulfillment Section with all verification results and closed status as `[EXECUTED & VERIFIED]`.
7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`:
   - Updated `WO-155` row to `[EXECUTED & VERIFIED]`.

### Files Created:
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/fixtures/mono_16bit.flac`
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/fixtures/stereo_16bit.flac`
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/fixtures/stereo_24bit.flac`
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/fixtures/fixed.flac`
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/fixtures/lpc.flac`
6. Staged OTA binary: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
7. Subagent summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo155_phase1_flac_psram_stage_only.md`

---

## 2. Rationale for Each Change

- **Pure-Rust `#![no_std]` RFC 9639 Decoder**: Replaces the std-only `claxon` crate with a pure `#![no_std]` + `alloc` implementation (`#![forbid(unsafe_code)]`) that compiles cleanly on `xtensa-esp32-none-elf` bare-metal runtime while preserving 100% sample-for-sample bit-exact parity.
- **$\ge 64\text{ KB}$ PSRAM Buffer Placement**: On ESP32 bare-metal, the small heap regions in internal SRAM are 46 KB (Region 0) and 18 KB (Region 1). By allocating buffers with sizes $\ge 64\text{ KB}$ (`stream_buf`: 65,536 B, `pending_pcm`: 65,536 B, `subframe_scratch`: 65,536 B), the allocator deterministically places all decoder buffers in Region 2 (4 MB external PSRAM at `0x3F80_0000`). This ensures zero internal DRAM heap consumption, zero `.bss`, zero `.dram2_uninit`, and zero CPU `.stack` growth.
- **Rev 2 Real-Time Ingestion Fix (`need_more_input`)**: When streaming audio chunks (1,536 B on Xtensa) into `FlacDecoder::decode`, incomplete frames previously left `self.need_more_input = false`, causing `has_pending_pcm()` to falsely return `true` on partial stream bytes. By setting `self.need_more_input = true` on every partial-frame and incomplete-metadata exit, `has_pending_pcm()` correctly returns `false` until a frame has completely decoded into PCM samples.
- **Rev 3 LMS HELO Capability Advertisement**: By including `,flc,` in `BASEMENT_HELO_CAPABILITIES` sent to LMS during SlimProto handshake, LMS streams native `.flac` files directly without server-side transcoding, preserving true 16/24-bit lossless fidelity on the local network.
- **Strict `--stage-only` Isolation**: The OTA binary was built and packaged to disk without modifying live hardware (`/dev/ttyUSB0` untouched, `172.22.1.15` not flashed), adhering to Stage-Before-Restart requirements and Invariant 44.

---

## 3. Verification Evidence

1. **Gate 1 (`cargo test --workspace`)**:
   - `squeezelite-decoders`: 27/27 tests passed (including full `claxon` cross-validation on all fixtures and `test_flac_partial_chunk_ingestion_need_more_input`).
   - `louder-esp32-app`: 113/113 library tests passed (including `test_pipeline_flac_playback_flow`) + 8/8 main tests passed (including `test_basement_helo_capabilities_flc_and_sample_rate`).
   - All workspace crates (audio core, proto, tas58xx, cyd) passed 100%.
2. **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**:
   - Exit code: 0, 0 warnings.
3. **Gate 3 (`xtensa-esp32-none-elf` Release Build)**:
   - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`: Exit code 0.
4. **Gate 4 (`readelf -S` Budget Gate)**:
   - `.stack`: `36,920` bytes (`0x009038 >= 36,000` bytes threshold, +920 B margin).
   - `.dram2_uninit`: `98,264` bytes (`0x017fd8 <= 98,768` bytes limit, 504 B headroom).
   - `.bss`: `75,544` bytes (0 bytes growth).
5. **Gate 5 (`verify_staged_ota_bin.py`)**:
   - Status: `[STAGE-ONLY AUDIT PASS]`
   - Path: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
   - Size: `955,072` bytes (`<= 2,686,976` bytes partition limit).
   - SHA-256: `aa55b7cc9af807799ae8f747830d7d2d5a02ae92fdb531a6c2b4ef5f742763d9`.
   - Headers: `img_magic: 0xE9`, `app_desc_magic: 0xABCD5432`, `project_name: 'Squeezelite-ESP32'`.

---

## 4. Known Risks & Untested Assumptions

- **LMS Server FLAC Stream Configuration**: The on-chip decoder supports FLAC block sizes up to 4,096 samples (the standard maximum for standard FLAC streams). Extremely rare non-standard streams with block size 4,608 (e.g. non-compliant encoders) will return `InvalidHeader`. Standard LMS transcoders and standard FLAC streams use 4,096 block size or lower.
- **Physical Hardware Verification**: Live Wi-Fi OTA deployment and hardware playback testing on `BASEMENT` (`172.22.1.15`) is deferred to Phase 2 per `--stage-only` authorization protocol.
