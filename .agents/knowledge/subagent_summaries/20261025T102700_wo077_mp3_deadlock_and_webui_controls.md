# Subagent Summary: WO-077 MP3 Deadlock Remediation & Native Web UI Control API

- **Timestamp**: `2026-10-25T10:27:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`
- **Target Mode**: Strictly `--stage-only` (no hardware flashing or physical interface modification)
- **Commit Hash**: `0e2f4732`
- **Branch**: `MK5-Rust`

---

## 1. Files Modified & Created

### Modified Files:
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
   - **Rationale**: Eliminated MP3 false-sync and reservoir decode deadlocks in `StaticMp3Decoder::decode`. When `frame_bytes == 0 || frame_bytes + 4 > 1536` or `next_h` comparison fails, returns `Ok((1, 0))` to advance past false sync headers. On all `self.decoder.decode` error branches, returns `Ok((consumed, 0))` if `consumed > 0`, or `Ok((frame_bytes.max(1), 0))` if `consumed == 0`, ensuring the ring buffer never wedges on corrupt frames or missing reservoirs. Added `test_static_mp3_decoder_never_deadlocks_on_false_sync_or_reservoir`.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
   - **Rationale**: Added `pub type BuildError = ParseError;` and `SlimprotoBuilder::build_ir` encoding 18-byte NEC IR remote control packets (`IR  ` opcode, length 10, timestamp, 0xFF code type, 16-bit code length, 4-byte IR code) for native transport/preset control via LMS. Added `test_build_ir_encodes_nec_slimproto_frame`.
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
   - **Rationale**: Added unit test `test_mp3_corrupt_frame_skips_and_recovers` verifying corrupt/garbage bytes are consumed without blocking the audio pipeline.
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
   - **Rationale**:
     - Added `vol_pct: u8` to `HttpStatusSnapshot` (default 55) and exposed `"vol_pct":{}` in `GET /status.json`.
     - Added `HttpControlAction::PlayerIrCommand { ir_code: u32, vol_pct: Option<u8>, mute: Option<bool> }`.
     - Added `("GET", "/cmd.json") | ("POST", "/cmd.json")` route parsing `?act=...`:
       - `play` (`ir_code: 0x7689_10EF, vol_pct: None, mute: Some(false)`)
       - `pause` (`ir_code: 0x7689_20DF, vol_pct: None, mute: None`)
       - `stop` (`ir_code: 0x7689_20DF, vol_pct: None, mute: Some(true)`)
       - `prev` (`ir_code: 0x7689_C03F, vol_pct: None, mute: None`)
       - `next` (`ir_code: 0x7689_A05F, vol_pct: None, mute: None`)
       - `volup` (`ir_code: 0x7689_807F, vol_pct: Some((snapshot.vol_pct + 5).min(100)), mute: Some(false)`)
       - `voldown` (`ir_code: 0x7689_00FF, vol_pct: Some(snapshot.vol_pct.saturating_sub(5)), mute: None`)
       - `vol25`..`vol85` (`ir_code: 0, vol_pct: Some(25..85), mute: Some(false)`)
       - `preset1`..`preset4` (`ir_code: 0x7689_F00F`, `0x7689_08F7`, `0x7689_8877`, `0x7689_48B7`)
       - Returns `{"ok":true,"act":"..."}` with CORS header `Access-Control-Allow-Origin: *`.
     - Updated `DASHBOARD_HTML` so all buttons dispatch to `/cmd.json?act=...` directly (same origin) and update the UI immediately via `.then(u)`, eliminating Chrome Private Network Access (PNA) and cross-origin blocking of `http://10.0.0.10:9000/jsonrpc.js`.
     - Added unit tests for `/cmd.json`.
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
   - **Rationale**:
     - Upgraded `run_firmware_tick` to allow up to 16 MP3 decode steps during stream resync/skipping, breaking immediately when PCM is produced (`pipeline.has_pending_pcm() || output_buffer.available_read() > prev_out`).
     - Populated `vol_pct` in `snap` from `amp.current_volume()`.
     - Handled `PlayerIrCommand` immediately: applied volume scaling to amplifier (`amp.set_volume`) and software DSP (`pipeline.set_volume`), applied mute (`pipeline.sink.set_mute`), and dispatched `SlimprotoBuilder::build_ir` over `slim_sock`.
     - Re-calibrated `audio_header_buf` from 2048 to 2000 bytes in `FirmwareBssBuffers` to perfectly counteract the 48-byte `.data` expansion from control string literals, preserving the exact `0x008eb0` (`36,528` bytes) `.stack` size budget byte-for-byte.
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-077-fix-mp3-deadlock-and-native-webui-controls.md`
   - **Rationale**: Appended Fulfillment Section.
7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
   - **Rationale**: Updated WO-077 status to `[EXECUTED & VERIFIED]`.

---

## 2. Verification Gates (All 5 Gates Passed)

1. **Gate 1: Unit Test Suite**
   - Command: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
   - Result: `184` tests passed across all crates, 0 failures.
2. **Gate 2: Clippy Lints**
   - Command: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
   - Result: 0 warnings with `-D warnings`.
3. **Gate 3: Xtensa Native Build**
   - Command: `source /home/console-admin/export-esp.sh && unset CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS && cd /home/console-admin/Github/squeezelite-esp32/rust && rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
   - Result: Compiled successfully with exit code `0`.
4. **Gate 4: Stack Budget Invariant Verification**
   - Command: `/home/console-admin/.rustup/toolchains/esp/xtensa-esp-elf/esp-16.2.0_20260914/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app | grep stack`
   - Result: `[23] .stack NOBITS 3ffd7150 023f58 008eb0 00 WA 0 0 4`
   - Exact size: `0x008eb0` = `36,528` bytes (EXACT byte-for-byte match with commit `435b5cf6` and `41eb03a4`).
5. **Gate 5: Staged Image Packaging & Offline Audit**
   - Command: `espflash save-image --chip esp32 ... && python3 verify_staged_ota_bin.py`
   - Result: `[STAGE-ONLY AUDIT PASS]`
   - Image Path: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
   - Size: `738,240` bytes (`27.47%` of `ota_0`)
   - SHA-256: `4a69c1d93cb1fc095b063ce187c7ef41777bb4888d4584300849c15bd6970b90`
   - Header: `img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0'`

---

## 3. Untested Assumptions & Risks
- **Untested Live Web UI**: Under `--stage-only`, the binary has not yet been flashed to `172.22.1.15`. The subsequent deployment work order will flash `louder-esp32-app-basement-ota.bin` and verify live HTTP control responses and MP3 stream resynchronization on the physical speaker.
