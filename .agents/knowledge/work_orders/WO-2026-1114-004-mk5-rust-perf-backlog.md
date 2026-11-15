# Work Order: WO-2026-1114-004 — MK5-Rust Performance & Code Hygiene Backlog (T04, T05, T14–T21, T23–T27)

- **Work Order ID**: `WO-2026-1114-004`
- **Timestamp**: `2026-11-14T23:38:00-05:00`
- **Assigned Subagent Role**: `Rust Performance & Firmware Specialist` / `Audit Architect`
- **Target Files**:
  - `/home/console-admin/Github/squeezelite-esp32/docs/mk5-rust-todo.md`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/cdn.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot/src/crypto.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/crypto.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/fade.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/rtp.rs`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope

Execute implementation, verification, and documentation synchronization for the 15 open pure-software MK5-Rust backlog tasks cataloged in `docs/mk5-rust-todo.md`:

### Phase 2: Correctness & Assertions
1. **T04 — Crossfade: restore the "missing frames" debug assert**:
   - In `louder-esp32-app/src/pipeline.rs:940` (`scale_chunk`), remove the redundant clamp or enforce `debug_assert!(frames * OUTPUT_FRAME_BYTES <= self.write_scratch.len())` and assert `got == frames * OUTPUT_FRAME_BYTES`.
2. **T05 — Vorbis/Opus `has_pending_pcm` true on any partial Ogg page**:
   - In `squeezelite-decoders/src/lib.rs`, avoid stalling/spinning on `stream_buf.len() >= 27` by reporting pending only when a complete page is buffered or pending PCM/packets exist.

### Phase 3: Device Performance
3. **T14 — Output path: one fewer copy per sample**:
   - In `louder-esp32-app/src/hal.rs` and `main.rs`, enable the sink to write already swapped L/R frames directly to DMA without intermediate 512-byte scratch copying, hoisting `self.muted` check out of loop while preserving Invariant 46 (whole 8-byte frames).
4. **T15 — mDNS: answer queries only, cache the reply, bound the loop**:
   - In `louder-esp32-app/src/main.rs`, add QR-bit check (`payload[2] & 0x80 == 0`), cache built announcements in `DeviceRuntimeState`, rate-limit multicast replies (RFC 6762), and cap packets processed per lap.
5. **T16 — Bound smoltcp polling and top up DMA mid-lap**:
   - In `louder-esp32-app/src/main.rs`, bound ingress packet processing in `net_poll_phase` (e.g. 8 packets) and top up DMA via `pump_output` after `slim_session_phase` to prevent exceeding 46 ms audio buffer budget.
6. **T17 — Bluetooth SBC synthesis without 64-bit MACs**:
   - In `louder-esp32-app/src/bt_a2dp.rs` (`synthesize_eight`), exploit `SYNMATRIX8` row symmetry and 32-bit accumulation with prescaling to eliminate unnecessary 64-bit multiplications.
7. **T18 — Stream ingest: fewer copies and callbacks**:
   - In `louder-esp32-app/src/coordinator.rs` and `stream_codec.rs`, skip `copy_within` when `r == w`, and hoist `take_stream_side_data` out of the 1 KB inner loop.

### Phase 4: Cleanup & Code Quality
8. **T19 — TAS58xx: use the page/book cache consistently**:
   - In `squeezelite-tas58xx/src/lib.rs`, switch `configure_silicon_maximization`, `set_analog_gain`, and `read_telemetry` to `ensure_page0_book0()`, reducing redundant I2C writes. Update test expectations accordingly.
9. **T20 — Remove duplicate CDN IV function**:
   - In `squeezelite-cspot/src/cdn.rs`, unify `CdnAudioDecryptor::calculate_iv` with `AudioKeyDecryptor::counter_iv`.
10. **T21 — Revert hand-rolled word XOR in AES CTR/CBC**:
    - In `squeezelite-cspot/src/crypto.rs` (`Aes128Ctr::process`) and `squeezelite-raop/src/crypto.rs` (CBC), replace endian word packing with standard byte iterator zip XOR.
11. **T23 — Delete unused `apply_crossfade`**:
    - In `squeezelite-audio-core/src/fade.rs`, delete unused `apply_crossfade` function.
12. **T24 — Delete duplicate `SlimprotoParser::OP_*` constants**:
    - In `squeezelite-proto/src/lib.rs`, remove unused associated `OP_*` constants in favor of module-level `OPCODE_*`.

### Phase 5: Host-Only / Unwired Module Optimization
13. **T25 — Vorbis/Opus/AAC allocation and copy churn**:
    - In `squeezelite-decoders/src/lib.rs`, decode in-page packets directly from `stream_buf`, avoid reallocating `VecDeque<Vec<u8>>`, and optimize AAC MP4 output reservations.
14. **T26 — cspot crypto cleanup**:
    - In `squeezelite-cspot/src/crypto.rs`, `cdn.rs`, and `shannon.rs`, track decrypted watermarks, avoid duplicate decryption on re-feed, and use circular indexing in Shannon cipher.
15. **T27 — `RtpJitterBuffer` per-packet allocation**:
    - In `squeezelite-raop/src/rtp.rs`, eliminate per-packet `to_vec()` allocations by pre-allocating buffer slots.

### Documentation Synchronization
- Update `docs/mk5-rust-todo.md` to reflect task statuses and progress upon completion.
- Keep workspace session ledgers synchronized.

## 2. Constraints & Invariants
- Enforce Rule 5 (One Variable Per Change), Rule 23 (Bash File Modification Ban), Rule 24 (Strict Absolute Paths), Rule 30/31/35 (Pre-Modification Verification & Backup).
- Strictly adhere to Invariant 46 (only whole 8-byte stereo frames may be pushed to I2S DMA).
- Adhere to Google Rust Style Guide and zero-panic / zero-clippy warning guidelines.
- Host and xtensa test/clippy suites must remain clean.

## 3. Required Deliverables
1. Subagent summary at `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/<timestamp>_<role>.md`.
2. Updated work order fulfillment section below.
3. Registration updated to `[EXECUTED & VERIFIED]` in `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`.

## 4. Fulfillment Section
- **Execution Timestamp**: `2026-11-15T00:53:00-05:00`
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Artifact**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-11-15_wo_004_mk5_rust_backlog.md`

### Wave Execution & Commit Mapping
- **Wave 1 (`c186c659`)**: Completed `T04`, `T05` (decoder `has_pending_pcm` complete-page guard), `T19` (TAS58xx `ensure_page0_book0()` consistency), `T23` (remove unused `apply_crossfade`), `T24` (remove duplicate `SlimprotoParser::OP_*`), and `T25` (Vorbis/Opus/AAC buffer/allocation churn). Verified `845` host tests pass.
- **Wave 2 (`f7b58cf0`)**: Completed `T05` (coordinator pump test), `T17` (SBC `synthesize_eight` symmetry + 32-bit MACs), `T18` (stream ingest `r == w` skip + hoisted side-data callback), `T20` (cspot CDN IV reuse), `T21` (`squeezelite-cspot` `Aes128Ctr::process` byte-wise XOR), `T26` (cspot crypto/watermark/Shannon circular index, amended by `579ffcde`), and `T27` (`RtpJitterBuffer` pre-allocated slots). Verified `851` host tests pass.
- **Wave 3 (`579ffcde`)**: Completed `T14` (`SWAP_LR` / `write_frames_i32` single-copy DMA output + hoisted `muted`), `T15` (mDNS QR-bit query filter, cached announcements, 1/s rate limit, bounded lap), `T16` (bounded smoltcp ingress + mid-lap `pump_output`), pre-existing Xtensa clippy cleanup, plus amended `T26` (`const TE0` in flash `.rodata` instead of `.dram1` and `#![forbid(unsafe_code)]` restored in `squeezelite-cspot`).

### Verification Results at `579ffcde`
- **Host Workspace Tests**: `859 passed, 0 failed` (`0` ignored).
- **Static Audit (`google_rust_audit.py --strict`)**: `0` errors, `0` warnings.
- **Xtensa 10-Crate Clippy**: `PASS` (`0` warnings).
- **Core 1 Check (`esp-rtos-core1`)**: `PASS`.
- **Release ELF Sizes (`target/xtensa-esp32-none-elf/release/`)**:
  - `louder-esp32-app`: IRAM `129636` B, `.dram2_uninit` `98520` B, `.data.wifi` `3400` B, `.stack` `28528` B (pre-existing `e6488153` Rule 64 DRAM0 baseline; 0-byte Wave 1–3 DRAM regression).
  - `cyd-controller-app`: IRAM `18236` B, `.stack` `188856` B.
- **Documentation & Ledger Updates**: Updated `docs/mk5-rust-todo.md` (`T01`–`T27` marked `[x]`; `HW1` and `D1` explicitly left `[ ]` open), `.agents/knowledge/work_orders/INDEX.md`, `.agents/knowledge/DIAGNOSTIC_LOG.md`, and `.agents/knowledge/SYSTEM_STATE.md`.
