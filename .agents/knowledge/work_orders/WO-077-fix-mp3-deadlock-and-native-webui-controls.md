# Work Order: WO-077 — Remediate MP3 Stream Resync Deadlock (`PM-2026-1025-001`) & Same-Origin ESP32 Web UI Control API (`PM-2026-1025-002`) (`--stage-only`)

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-077` |
| **Timestamp** | `2026-10-25T10:06:00-04:00` |
| **Assigned Role** | Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`) |
| **Target Scope** | `rust/crates/squeezelite-decoders/src/lib.rs`, `rust/crates/squeezelite-proto/src/lib.rs`, `rust/crates/louder-esp32-app/src/http_control.rs`, `rust/crates/louder-esp32-app/src/main.rs`, `rust/crates/louder-esp32-app/src/pipeline.rs` |
| **Status** | `[IN PROGRESS]` |

## 1. Objective
Fix the two firmware root causes identified in `/deep-hardware-audit` and `/improvement`, and package/verify the `--stage-only` OTA binary while preserving the exact `.stack = 36,528` B (`0x008eb0`) budget:

### Fix 1: Eliminate `StaticMp3Decoder` & `run_firmware_tick` Stream Deadlock on Corrupt / Reservoir / False-Sync MP3 Frames (`PM-2026-1025-001`)
1. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (`StaticMp3Decoder::decode`, lines 695-780):
   - In the `Some((0, h))` branch (line 717):
     - Because `READ_SCRATCH_BYTES` on `xtensa` is `1536` bytes, any false-positive 4-byte sync header where `frame_bytes == 0` OR `frame_bytes + 4 > 1536` (or `h.layer() != 3`) can NEVER satisfy `encoded_in.len() >= frame_bytes + 4`, causing `StaticMp3Decoder::decode` to return `Err(DecoderError::NeedMoreData)` forever!
     - Fix: If `frame_bytes == 0 || frame_bytes + 4 > 1536`, immediately return `Ok((1, 0))` to skip 1 byte past the false sync header!
   - After `let (consumed, result) = self.decoder.decode(slice_to_decode, pcm_out);` (line 735):
     - Because `slice_to_decode` already contains a complete `frame_bytes + 4` frame verified by `h.compare(next_h)`, if `result` is `Err(_)` (including `DecodeError::NoFrame` or `DecodeError::ReservoirUnavailable(_)`) and `consumed == 0`, returning `Err(DecoderError::NeedMoreData)` will re-feed the exact same `slice_to_decode` on every subsequent tick forever!
     - Fix: On ANY `Err(_)` from `self.decoder.decode(slice_to_decode, pcm_out)`, if `consumed > 0`, return `Ok((consumed, 0))`; if `consumed == 0`, return `Ok((frame_bytes.max(1), 0))` so the ringbuffer immediately advances to `next_h` (the verified next MP3 frame header) without stalling!
2. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`run_firmware_tick`, lines 77-92):
   - When `pipeline.decoder` is `ActiveDecoder::Mp3(_)`, `process_decode_step()` may return `Ok(consumed)` with `0` PCM frames produced when skipping ID3 tags, false sync bytes (`Ok((1, 0))`), or reservoir-unavailable frames.
   - Instead of running `process_decode_step()` at most once even when `0` PCM samples were produced, allow up to `16` iterations while `!pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 1024 && pipeline.stream_buffer.available_read() > 0`, recording `let prev_out = pipeline.output_buffer.available_read();` before `pipeline.process_decode_step()` and breaking as soon as:
     - `pipeline.has_pending_pcm() || pipeline.output_buffer.available_read() > prev_out` (for `ActiveDecoder::Mp3(_)`, meaning 1 full MP3 frame was synthesized into PCM!), OR
     - `!matches!(res, Ok(c) if c > 0)`.
3. Add unit test `test_static_mp3_decoder_never_deadlocks_on_false_sync_or_reservoir` in `squeezelite-decoders/src/lib.rs` and `test_mp3_corrupt_frame_skips_and_recovers` in `louder-esp32-app/src/pipeline.rs`.

### Fix 2: Same-Origin ESP32 Web UI Control Endpoint (`GET/POST /cmd.json?act=...`) + SlimProto `IR  ` Dispatch + Local TAS5805M Volume/Mute (`PM-2026-1025-002`)
1. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` (`SlimprotoBuilder`):
   - Add `pub fn build_ir(time_ms: u32, ir_code: u32, out: &mut [u8]) -> Result<usize, BuildError>` which builds a 18-byte SlimProto `IR  ` packet:
     - `b"IR  "` (4 bytes)
     - `10u32.to_be_bytes()` (4 bytes payload length = 10)
     - `time_ms.to_be_bytes()` (4 bytes timestamp)
     - `0xFFu8` (1 byte code format), `16u8` (1 byte code bits = 16)
     - `ir_code.to_be_bytes()` (4 bytes 32-bit Slim Devices IR code)
   - Add a unit test verifying `build_ir`.
2. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`:
   - Add `vol_pct: u8` to `HttpStatusSnapshot` (default `55`) and include `"vol_pct":{}` in `GET /status.json`.
   - Extend `HttpControlAction` with:
     - `PlayerIrCommand { ir_code: u32, vol_pct: Option<u8>, mute: Option<bool> }`
   - Support `GET` or `POST` to `/cmd.json` with query parameters (`?act=...` parsed from `raw_path`):
     - `act=play` -> `ir_code: 0x7689_10EF`, `vol_pct: None`, `mute: Some(false)`
     - `act=pause` -> `ir_code: 0x7689_20DF`, `vol_pct: None`, `mute: None`
     - `act=stop` -> `ir_code: 0x7689_20DF`, `vol_pct: None`, `mute: Some(true)`
     - `act=prev` -> `ir_code: 0x7689_C03F`, `vol_pct: None`, `mute: None`
     - `act=next` -> `ir_code: 0x7689_A05F`, `vol_pct: None`, `mute: None`
     - `act=volup` -> `ir_code: 0x7689_807F`, `vol_pct: Some((snapshot.vol_pct + 5).min(100))`, `mute: Some(false)`
     - `act=voldown` -> `ir_code: 0x7689_00FF`, `vol_pct: Some(snapshot.vol_pct.saturating_sub(5))`, `mute: None`
     - `act=vol25` / `vol40` / `vol55` / `vol70` / `vol85` -> `ir_code: 0`, `vol_pct: Some(25 / 40 / 55 / 70 / 85)`, `mute: Some(false)`
     - `act=preset1`..`preset4` -> `ir_code: 0x7689_F00F` (`preset1`), `0x7689_08F7` (`preset2`), `0x7689_8877` (`preset3`), `0x7689_48B7` (`preset4`), `vol_pct: None`, `mute: Some(false)`
     - Return `HTTP/1.1 200 OK` JSON `{"ok":true,"act":"..."}` with `Access-Control-Allow-Origin: *`.
   - Update `DASHBOARD_HTML` so every button calls same-origin `fetch('/cmd.json?act=...')` FIRST (guaranteed to work from any browser with zero CORS/Private-Network-Access errors!) and displays `vol_pct` in the status table.
3. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - Handle `HttpControlAction::PlayerIrCommand { ir_code, vol_pct, mute }` immediately when `handle_http_request_streaming` returns it (do not wait 250ms — only `RebootToRecovery` / `PersistOta0AndReboot` wait 250ms):
     - If `let Some(pct) = vol_pct`, compute TAS5805M register `let reg = if pct == 0 { 0xFF } else { 0x30 + (100 - pct.min(100)) }; let _ = amp.set_volume(reg); pipeline.set_volume(if pct > 0 { 65536 } else { 0 }, if pct > 0 { 65536 } else { 0 });`.
     - If `let Some(m) = mute`, call `pipeline.sink.set_mute(m);`.
     - If `ir_code != 0`, send `SlimprotoBuilder::build_ir(now_ms as u32, ir_code, &mut ir_buf)` on `slim_sock` if `slim_sock.may_send()`.
   - Populate `vol_pct` in `HttpStatusSnapshot` from `amp.current_volume()` (`if v >= 0xFE { 0 } else { 100u8.saturating_sub(v.saturating_sub(0x30)) }`).

## 2. Mandatory `--stage-only` Verification Gates
1. `cargo test --workspace` (0 failures).
2. `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings).
3. `cargo +esp build --release --target xtensa-esp32-none-elf -p louder-esp32-app --bin louder-esp32-app --no-default-features --features board-louder-esp32`
4. Verify `.stack == 36,528` B (`0x008eb0`) via `readelf -S`.
5. Package OTA binary (`espflash save-image`) and run `python3 /home/console-admin/Github/squeezelite-esp32/scripts/verify_staged_ota_bin.py`.
6. Commit cleanly to `MK5-Rust`. Do NOT flash `172.22.1.15` in this work order (`--stage-only`).

## Fulfillment Section
- Status: [EXECUTED & VERIFIED]
- Commit: `0e2f4732`
- Branch: `MK5-Rust`
- Verification Gates:
  - Gate 1: `cargo test --workspace` -> 184 passed, 0 failed.
  - Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
  - Gate 3: `xtensa-esp32-none-elf` release build -> exit code 0.
  - Gate 4: `readelf -S .stack` -> `0x008eb0` (`36,528` bytes, exact match).
  - Gate 5: `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]` (738,240 B, SHA256 `4a69c1d93cb1fc095b063ce187c7ef41777bb4888d4584300849c15bd6970b90`).
- Scope items completed:
  1. `squeezelite-decoders/src/lib.rs`: Fixed false-sync (`frame_bytes == 0 || frame_bytes + 4 > 1536`) and reservoir/corrupt decode error deadlocks by ensuring bytes are consumed on all error branches. Added `test_static_mp3_decoder_never_deadlocks_on_false_sync_or_reservoir`.
  2. `squeezelite-proto/src/lib.rs`: Added `pub type BuildError = ParseError;` and `SlimprotoBuilder::build_ir` for outbound 18-byte NEC IR remote control frames. Added `test_build_ir_encodes_nec_slimproto_frame`.
  3. `louder-esp32-app/src/pipeline.rs`: Added `test_mp3_corrupt_frame_skips_and_recovers`.
  4. `louder-esp32-app/src/http_control.rs`: Added `vol_pct` to `HttpStatusSnapshot` and `/status.json`. Implemented `/cmd.json?act=...` same-origin control API supporting play, pause, stop, prev, next, volup, voldown, vol25..vol85, and presets 1..4. Updated `DASHBOARD_HTML` to invoke `fetch('/cmd.json?act=...')` directly without cross-origin JSON-RPC or PNA restrictions. Added unit tests for `/cmd.json`.
  5. `louder-esp32-app/src/main.rs`: Enabled up to 16-step MP3 resync skipping per tick in `run_firmware_tick`, breaking immediately when PCM is produced. Populated `vol_pct` from `amp.current_volume()`. Handled `PlayerIrCommand` with immediate volume/mute scaling and `SlimprotoBuilder::build_ir` dispatch. Re-calibrated `audio_header_buf` to 2000 B to preserve exact `0x008eb0` (36,528 B) `.stack` budget.
