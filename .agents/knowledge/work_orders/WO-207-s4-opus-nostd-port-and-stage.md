# Work Order: WO-207 — S4: Opus (`opus-rs 0.1.40`) `no_std + alloc` Vendor Port, In-Memory Ogg Demuxer, Firmware Wiring & Stage-Only Build

- **Work Order ID**: `WO-207`
- **Timestamp**: `2026-10-28T20:50:00-04:00`
- **Assigned Subagent Role**: `Rust no_std Codec Porter (S4 Opus)`
- **Plan**: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/implementation_plan.md` (Step S4)
- **Status**: `[IN PROGRESS]`

## 1. Objective
Vendor `opus-rs 0.1.40` into `rust/vendor/opus-rs`, port it to `no_std + alloc` with `libm` and boxed internal decoder state (`Box<CeltDecoder>`, `Box<SilkDecoder>`), enable `OpusDecoder` under feature `alloc = ["dep:syom", "dep:lewton", "dep:alac", "dep:opus-rs"]` in `squeezelite-decoders`, implement an in-memory streaming Ogg Opus packet demuxer (parsing `OpusHead` and `OpusTags`, then streaming audio packets into `opus_rs::OpusDecoder`), wire LMS `strm` format byte `'u'` (`ops`) to `ActiveDecoder::Opus` in `louder-esp32-app/src/main.rs`, add `,ops` to `BASEMENT_HELO_CAPABILITIES`, pass all host tests and static audits, and build a staged OTA image. **Stage-only: do NOT flash any device.**

## 2. Hypothesis (Rule 6)
- *If this works*: `opus-rs 0.1.40` builds under `no_std + alloc` for `xtensa-esp32-none-elf` with boxed decoder state, `OpusDecoder` demuxes in-memory streaming Ogg Opus packets to PCM without `std::io`, host tests show bit-for-bit PCM consistency across `std` and `alloc` builds, and the staged image stays under `0x290000` (`2,686,976` B) with `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
- *If it fails*: `opus-rs` code size or internal tables exceed partition/memory limits, or `no_std + alloc` port encounters missing core/alloc primitives.

## 3. Hard Constraints
1. **Preserve Verified Baselines**:
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_5444e41b/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s1_aac_530fade4/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s2_vorbis_bd49e6e0/`.
   - Do not modify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s3_alac_80a8cb94/`.
2. **Isolation Guarantee**:
   - Do NOT flash or send HTTP requests to `172.22.1.15`.
   - Do NOT touch `/dev/ttyUSB0`.
   - Do NOT modify LMS `10.0.0.10`.
3. **Stage-Only Output**:
   - Stage binary to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`.
4. **Style & Memory Guardrails**:
   - `google_rust_audit.py --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - Box large structs (`CeltDecoder`, `SilkDecoder`) in PSRAM heap (`esp_alloc::HEAP`). Keep stack footprint small (< 1024 B).

## 4. Execution Steps

1. **Vendor and Port `opus-rs 0.1.40` (`rust/vendor/opus-rs/`)**:
   - Copy `opus-rs 0.1.40` from cargo cache to `rust/vendor/opus-rs`.
   - Configure manifest `Cargo.toml`: `default-features = false`, `libm = "0.2"`, add `alloc` feature if needed.
   - In `rust/vendor/opus-rs/src/lib.rs`: `#![no_std]`, `extern crate alloc;`.
   - In `OpusDecoder`: use `alloc::boxed::Box` for `celt_dec` and `silk_dec` so `OpusDecoder` struct size is < 500 bytes and fits on stack safely.
   - Author `rust/vendor/opus-rs/VENDOR.md`.
   - Add `[patch.crates-io] opus-rs = { path = "vendor/opus-rs" }` to `rust/Cargo.toml`.

2. **Ogg Opus Demuxer & `OpusDecoder` in `squeezelite-decoders`**:
   - In `squeezelite-decoders/Cargo.toml`: update `alloc = ["dep:syom", "dep:lewton", "dep:alac", "dep:opus-rs"]`, add `opus-rs = { version = "0.1.40", default-features = false, features = ["libm"], optional = true }`.
   - Implement `OpusDecoder` in `squeezelite-decoders`:
     - Demux `OggS` pages directly to packets.
     - Parse `OpusHead` (RFC 7845): check `"OpusHead"`, extract channel count (1 or 2), sample rate (48000), instantiate `opus_rs::OpusDecoder::new(48000, channels)`.
     - Skip `OpusTags`.
     - For subsequent audio packets: call `opus_dec.decode(&packet, 5760, &mut float_buf)`. Convert float samples to 16/32-bit PCM in `pending_pcm`.
     - Implement `has_pending_pcm(&self) -> bool` and `flush(&mut self)`.

3. **Bit-for-Bit Host Equivalence Test**:
   - Commit a short Opus test fixture in `crates/squeezelite-decoders/tests/fixtures/test_stereo_05s_48k.opus`.
   - Create `crates/squeezelite-decoders/tests/opus_alloc_equivalence.rs`, verify decoding to PCM across `std` and `alloc` configurations.

4. **Firmware Wiring in `louder-esp32-app`**:
   - In `pipeline.rs`: add `ActiveDecoder::Opus(Box<OpusDecoder>)`, wire into `has_pending_pcm()`, `sample_rate()`, and `select_format(b'u')`.
   - In `main.rs`: wire format byte `'u'` (`AudioFormat::Opus`) across both `strm` dispatch sites with PSRAM check `psram_avail_bytes >= 1_048_576`.
   - Add `,ops` to `BASEMENT_HELO_CAPABILITIES` and update test assertions.

5. **Stage-Only Verification Gates**:
   - `cargo test --workspace` -> 0 failures.
   - `python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - Xtensa release build, check `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - Export and verify staged image:
     `espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`
     Run `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.

## 5. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo207_s4_opus.md`.
- Confirm 0 background tasks at handoff.

---

## Fulfillment Section

### Execution & Verification Summary (`[EXECUTED & VERIFIED]`)
- **Execution Date**: 2026-10-28T21:24:00-04:00
- **Assigned Subagent**: `WO-207` S4 Opus Vendor Port & Firmware Staging Engine

#### 1. `opus-rs 0.1.40` Vendored & `no_std + alloc` Port
- Vendored `opus-rs 0.1.40` into `rust/vendor/opus-rs/` with `VENDOR.md`.
- Configured `#![no_std]`, `extern crate alloc;`, and `libm` support.
- Boxed large state fields in `OpusDecoder` (`celt_dec`, `silk_dec`, scratch buffers) and `OpusEncoder`.
- Resolved Xtensa LLVM backend instruction selection friction:
  - Eliminated loop induction variable strength reduction of `c * (DECODE_BUFFER_SIZE + overlap)` in `celt/plc.rs` and `celt.rs` (`Cannot select: i32 = Constant<8192>`).
  - Added `core::hint::black_box` to `silk/vad.rs:207` to prevent constant distribution into `Constant<-32768>`.
  - Moved static `CeltMode` in `modes.rs` to heap-allocated `OnceCell<&'static CeltMode>` via `Box::leak`, recovering ~25 KB of internal DRAM into the stack.
- Configured `[patch.crates-io]` in `rust/Cargo.toml`.

#### 2. `squeezelite-decoders` Integration
- Added `opus-rs = { version = "0.1.40", default-features = false, features = ["libm"], optional = true }` under `alloc` feature.
- Implemented `OpusDecoder` with Ogg packet parsing (`OpusHead`, `OpusTags`, streaming audio packets).
- Measured `core::mem::size_of::<OpusDecoder>()` = 160 bytes (< 1024 B budget).
- Handled pre-skip, float-to-PCM conversion with mono-to-stereo expansion, and stateful sample draining (`has_pending_pcm`).

#### 3. Test Fixture & Host Equivalence Tests
- Added test fixture: `crates/squeezelite-decoders/tests/fixtures/test_stereo_05s_48k.opus`.
- Added test suite: `crates/squeezelite-decoders/tests/opus_alloc_equivalence.rs`.
- Verified 100% bit-for-bit equivalence across std and no_std+alloc:
  - `std=true`: 24,648 frames, FNV-1a64: `0x626fe7b25b0baae2`.
  - `std=false` (`--no-default-features --features alloc`): 24,648 frames, FNV-1a64: `0x626fe7b25b0baae2`.
  - All 5/5 tests passed; all 52 decoder tests passed.

#### 4. `louder-esp32-app` Firmware Wiring
- In `pipeline.rs`: added `ActiveDecoder::Opus(Box<OpusDecoder>)`, format selection for byte `'u'`, sample rate and draining support.
- In `main.rs`: wired format byte `'u'` across both `strm` dispatch sites with 1 MB PSRAM guard, added `,ops` to `BASEMENT_HELO_CAPABILITIES`, and updated telemetry.
- All 123 `louder-esp32-app` tests passed.

#### 5. Section Size Budget & Verification Audit
- Built release binary: `target/xtensa-esp32-none-elf/release/louder-esp32-app`.
- Section sizes:
  - `.stack`: 36,288 B (budget: `>= 36,000` B) -> **PASS**
  - `.dram2_uninit`: 98,456 B (budget: `<= 98,768` B) -> **PASS**
- Staged OTA image:
  - Location: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`
  - Size: 1,603,792 B (59.69% of `ota_0`, budget: `< 2,686,976` B) -> **PASS**
  - SHA-256: `029f64ab10a491330ffeacc90ecc1876ba74637248f56286e4f9241c9d175889`
  - Audit output: `[STAGE-ONLY AUDIT PASS]`
- Google Rust audit: 0 errors, 0 warnings.
- Clippy audit: 0 warnings with `-D warnings`.
- Workspace unit tests: all passed cleanly.
- Strict stage-only constraint maintained: no live hardware flash, no serial port probes.
