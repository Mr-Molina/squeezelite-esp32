# Work Order: WO-103 — Fix `/status.json` `768`-Byte `resp_buf` Overflow, `http_sock` `CloseWait` Recycling, & `.bss` `BSS_BUFFERS` Placement (`--stage-only`)

## Metadata
- **Work Order ID**: `WO-103`
- **Timestamp**: `2026-10-25T17:13:30-04:00`
- **Assigned Subagent Role**: Rust App HTTP & Memory Implementer (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
Remediate the 3 issues isolated by `WO-102` (`DIAGNOSTIC_LOG.md` Entry 55) while preserving the proven `WO-101` stack-frame (`<= 9 KB`) and PSRAM architecture (`7717733f`):
1. Fix the `768`-byte `resp_buf` truncation on `GET /status.json` (`~833` bytes) in `src/main.rs` and `src/http_control.rs`, and expand `http_tx_buf` so the full response is transmitted cleanly.
2. Fix `http_sock` lifecycle handling in `src/main.rs` so `http_sock` is immediately closed/aborted if `handle_http_request_streaming` returns `None` or if `http_sock` enters `CloseWait` / `FinWait1` / `FinWait2` / `Closing` / `LastAck` / `TimeWait` when idle.
3. Change `settings_scratch: [0xFFu8; 512]` to `settings_scratch: [0u8; 512]` in `FirmwareBssBuffers::new()` (`src/main.rs`) so the `30,720`-byte `BSS_BUFFERS` static moves back from `.data` (`PROGBITS`) to `.bss` (`NOBITS`), reducing the `.bin` image size by `-30 KB`.

## Target Files & Scope
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

## Required Implementation Steps
1. **Rule 35 Backups**:
   - Back up `src/http_control.rs` and `src/main.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/` before editing.
2. **Fix `/status.json` Buffer Size & Stack Usage (`src/http_control.rs` & `src/main.rs`)**:
   - In `src/main.rs`:
     - Inside `init_static_pipeline` (`.dram2_uninit`), allocate `STATIC_HTTP_RESP_BUF: core::mem::MaybeUninit<[u8; 1536]>` (`1,536` bytes in `.dram2_uninit`, taking `0` stack bytes and `0` `.bss` bytes!) and return it alongside `pipeline` and `pending_resp_buf`, OR reuse `http_resp_buf: &mut [u8; 1536]` from `.dram2_uninit` instead of `let mut resp_buf = [0u8; 768];` on the stack at `src/main.rs:1596`!
     - Also place `http_tx_buf: [u8; 1536]` in `.dram2_uninit` (or expand `http_tx_buf` in `FirmwareBssBuffers` from `1024` to `1536` — placing `STATIC_HTTP_TX_BUF: [u8; 1536]` in `.dram2_uninit` even saves `+1,024` bytes of `.bss` and increases `.stack` by `+1,024` bytes!).
   - In `src/http_control.rs`:
     - Add unit test `test_status_json_fits_in_1024_byte_buffer` that constructs a realistic `HttpStatusSnapshot` (with PSRAM, Core 1, TAS5805M, EQ, and `settings_seq` fields populated) and passes `let mut out_buf = [0u8; 1024];` to `handle_http_request_streaming(b"GET /status.json HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n", &snap, &mut out_buf)`, asserting `Some((len, static_body, HttpControlAction::None))` is returned with `len > 600 && len <= 1024` and `static_body.is_empty()`.
3. **Fix `http_sock` `CloseWait` & `None` Response Handling (`src/main.rs:1505-1676`)**:
   - In `src/main.rs`, right after `if pending_http_closing { ... }`:
     - Add a check: if `pending_http_body.is_empty() && !pending_http_closing && http_sock.is_open() && matches!(http_sock.state(), tcp::State::CloseWait | tcp::State::TimeWait | tcp::State::Closing | tcp::State::LastAck | tcp::State::FinWait1 | tcp::State::FinWait2)`:
       call `http_sock.abort();`!
   - And in the `if let Some((hdr_or_resp_len, static_body, action)) = handle_http_request_streaming(...)` block at `src/main.rs:1597`:
     - Add an `else` branch:
       ```rust
       } else {
           http_sock.close();
           pending_http_closing = true;
           pending_close_deadline_ms = now_ms + 250;
       }
       ```
       so even an invalid/unrecognized HTTP request closes `http_sock` cleanly and recycles it within 250ms!
4. **Move `BSS_BUFFERS` (`30,720` B) Back from `.data` to `.bss` (`src/main.rs:938`)**:
   - In `FirmwareBssBuffers::new()`, change `settings_scratch: [0xFFu8; 512]` to `settings_scratch: [0u8; 512]` (`persist_audio_config` already calls `scratch.fill(0xFF)` before reading/writing).
   - Verify via `xtensa-esp32-elf-readelf -S` that `.data` shrinks by `~30 KB` (back to `~15.6 KB`) and `.bin` image shrinks back to `~756 KB`!
5. **Verify All `--stage-only` Gates & Commit**:
   - `cargo test --workspace` (`exit code 0`, 218+ tests pass)
   - `cargo clippy --workspace --all-targets -- -D warnings` (`exit code 0`)
   - `source /home/console-admin/export-esp.sh && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`)
   - `xtensa-esp32-elf-readelf -S`: verify `.stack >= 36,000` B AND `.data <= 20,000` B.
   - `xtensa-esp32-elf-objdump -d`: verify `run_hardware_firmware` stack frame (`entry a1, ...`) is `<= 9,000` B.
   - Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and verify `verify_staged_ota_bin.py` (`[STAGE-ONLY AUDIT PASS]`).
   - Commit to `MK5-Rust` and report full `git rev-parse HEAD` SHA, `.stack` size, `run_hardware_firmware` frame size, `.data` size, and `.bin` SHA-256.

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `9612ffe8345c54b0d9937be7e409c9f1472b2285`
- **Rule 35 Backups**:
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.pre_wo103.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/http_control.rs.pre_wo103.bak`
- **Summary of Changes**:
  1. Allocated `STATIC_HTTP_RESP_BUF: core::mem::MaybeUninit<[u8; 1536]>` and `STATIC_HTTP_TX_BUF: core::mem::MaybeUninit<[u8; 1536]>` in `.dram2_uninit` inside `init_static_pipeline` (`src/main.rs`), replacing the `768`-byte stack-allocated `resp_buf` at `src/main.rs:1596` and moving `http_tx_buf` (`1,536` B) out of `FirmwareBssBuffers`.
  2. Added unit test `test_status_json_fits_in_1024_byte_buffer` in `src/http_control.rs` verifying that `handle_http_request_streaming` formats `GET /status.json` cleanly into a `1,024`-byte buffer (`600 < len <= 1024`).
  3. Added idle `CloseWait | TimeWait | Closing | LastAck | FinWait1 | FinWait2` socket recycling (`http_sock.abort()`) and an `else` branch on `handle_http_request_streaming(...) == None` (`http_sock.close(); pending_http_closing = true; pending_close_deadline_ms = now_ms + 250;`) in `src/main.rs`.
  4. Changed `settings_scratch: [0xFFu8; 512]` to `settings_scratch: [0u8; 512]` in `FirmwareBssBuffers::new()`, moving `BSS_BUFFERS` back from `.data` (`PROGBITS`) to `.bss` (`NOBITS`).
- **Verification Gates (`--stage-only`)**:
  - `cargo test --workspace`: PASS (`219` tests passed, `0` failed)
  - `cargo clippy --workspace --all-targets -- -D warnings`: PASS (`exit code 0`)
  - `xtensa-esp32-elf-readelf -S`:
    - `.stack`: `0x0091d0` = `37,328` B (`>= 36,000` B PASS)
    - `.data`: `0x003dd0` = `15,824` B (`<= 20,000` B PASS; `-30,720` B vs `ef630833`)
    - `.bss`: `0x022e40` = `142,912` B
    - `.dram2_uninit`: `0x004990` = `18,832` B
  - `xtensa-esp32-elf-objdump -d`:
    - `run_hardware_firmware` stack frame: `entry a1, 0x2120` = `8,480` B (`<= 9,000` B PASS)
  - Staged OTA Binary (`/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`):
    - `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`
    - Size: `756,096` bytes (`28.14%` of `ota_0`)
    - SHA-256: `500e9260bb7a4c6b5ca8911698b1a7f1cf1441f394c17331bbfef2065ed7596c`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo103_http_status_buffer_implementer.md`
