# Work Order `WO-068`: Fix Port 80 Web UI `http_tx_buf` Truncation (`curl: (18) 353 bytes missing`) & `TimeWait` Socket Stall + Expand `DASHBOARD_HTML` (`--stage-only`)

- **ID**: `WO-068`
- **Timestamp**: `2026-10-25T09:05:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Status**: `[IN PROGRESS]`

## 1. Root Cause & Objective
Live testing of `GET http://172.22.1.15/` with `curl -v http://172.22.1.15/` proved why the Web UI failed in the user's browser:
1. **`http_tx_buf` (`1,024` B) Response Truncation**:
   - `http_tx_buf` is `[u8; 1024]`, while `GET /` returned `125` bytes of HTTP headers + `1,252` bytes of `DASHBOARD_HTML` (`1,377` bytes total).
   - `main.rs:1273-1274` called `let _ = http_sock.send_slice(&resp_buf[..resp_len]); http_sock.close();` once, which only queued `1,024` bytes (`899` bytes of HTML body) and truncated the remaining `353` bytes (`curl: (18) end of response with 353 bytes missing` -> browser `net::ERR_CONTENT_LENGTH_MISMATCH`).
2. **Unread Browser Request Headers & `TimeWait` Socket Stall**:
   - Browser `GET /` requests often exceed `512` bytes of headers; leaving unread bytes in `http_rx_buf` when calling `close()` causes TCP RST, and `smoltcp`'s `http_sock.is_open()` returns `true` during `TimeWait` / `CloseWait` / `LastAck`, blocking follow-up requests (`/favicon.ico`, `/status.json`).

## 2. Required Code Changes (Keeping `.stack == 36,528` B = `0x008eb0`)

### A. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
1. Back up `src/http_control.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_http_control.rs.bak5` and `src/main.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak7` (Rule 35).
2. Add a helper or update `handle_http_control_request` (or add `handle_http_request_streaming` while keeping `handle_http_control_request` working for existing unit tests!) so that:
   - Define `pub fn handle_http_request_streaming(req_bytes: &[u8], snapshot: &HttpStatusSnapshot<'_>, out_buf: &mut [u8]) -> Option<(usize, &'static [u8], HttpControlAction)>`:
     - For `("GET", "/") | ("GET", "/index.html")`:
       - Writes **only the HTTP headers** into `out_buf`:
         `"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"` with `DASHBOARD_HTML.len()`, and returns `Some((header_len, DASHBOARD_HTML.as_bytes(), HttpControlAction::None))`.
     - For `("GET", "/favicon.ico")`:
       - Writes `"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"` into `out_buf` and returns `Some((hdr_len, &[], HttpControlAction::None))`.
     - For `("GET", "/status.json")` and all other JSON endpoints:
       - Include `Access-Control-Allow-Origin: *\r\n` in the JSON response headers, write headers + JSON body into `out_buf`, and return `Some((resp_len, &[], action))`.
   - Keep `handle_http_control_request` calling `handle_http_request_streaming` and, if `static_body` is non-empty and fits in `out_buf`, appending `static_body` into `out_buf` so existing unit tests continue to work seamlessly (update the unit test buffer size in `serves_html_dashboard_on_root_and_index_html` to `[0u8; 4096]` to match the expanded `DASHBOARD_HTML`).
3. **Expand `DASHBOARD_HTML` (`pub const DASHBOARD_HTML: &str`, stored in Flash `.rodata` — `0` bytes of DRAM `.bss` or `.stack`!)**:
   - Build a clean, dark-mode responsive HTML5/JS control panel (`~2,800..3,400` bytes in `.rodata`) that displays:
     1. **Live Hardware & Audio Telemetry Table/Cards** (polled every `1500ms` from `/status.json`):
        - Device IP, SSID, Wi-Fi RSSI (`dBm`), Uptime (`m:ss`), Firmware Version (`I2S-4MFlash-Rust`), Anti-Brick Status (`Pre-Armed Factory 0x10000`).
        - TAS5805M Amp Mode (`Play` / `Standby`), I2C ACKs / NACKs, Total Frames Played, and **Live Audio Frame Rate (`fps`)** computed in JS from `(d.frames_played - prevFrames) * 1000 / (now - prevTime)`.
     2. **Lyrion (LMS `10.0.0.10:9000`) Player Transport & Volume Controls for `"BASEMENT"` (`c8:2e:18:be:b8:34`)**:
        - Use a JS helper `function lms(cmd){fetch('http://'+lmsHost+'/jsonrpc.js',{method:'POST',mode:'no-cors',headers:{'Content-Type':'text/plain'},body:JSON.stringify({id:1,method:'slim.request',params:['c8:2e:18:be:b8:34',cmd]})});}` (using `mode:'no-cors'` and `text/plain` so the browser sends the JSON-RPC POST directly to `http://10.0.0.10:9000/jsonrpc.js` without CORS preflight blocking!).
        - Transport buttons: `Play` (`['play']`), `Pause` (`['pause','1']`), `Stop` (`['stop']`), `Prev` (`['playlist','index','-1']`), `Next` (`['playlist','index','+1']`).
        - Volume controls: `-5%` (`['mixer','volume','-5']`), `25%`, `40%`, `55%`, `70%`, `85%`, `+5%` (`['mixer','volume','+5']`).
        - 1-Click Stream Presets:
          - *Ibiza Global Radio* (`['playlist','play','https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3']`)
          - *WHYY 90.9 FM* (`['playlist','play','https://whyy.streamguys1.com/whyy_tunein']`)
          - *WDIY 88.1 FM* (`['playlist','play','https://war.streamguys1.com:7883/wdiy_7880']`)
          - *Basement Deep Groove WAV* (`['playlist','play','file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav']`)
        - Direct link `<a href="http://10.0.0.10:9000/" target="_blank">Open Full Lyrion Server UI (10.0.0.10:9000)</a>`.
     3. **System / Partition Controls** (with `confirm(...)` guard so accidental clicks don't reboot):
        - `Reboot to Factory Recovery (0x10000)` (`POST /recovery.json`)
        - `Commit OTA & Reboot` (`POST /reboot_ota.json`)

### B. In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
1. Import `handle_http_request_streaming` from `louder_esp32_app::http_control`.
2. In `run_hardware_firmware()`, add two small state variables:
   - `let mut pending_http_body: &'static [u8] = &[];`
   - `let mut pending_http_closing = false;`
3. Replace the Port 80 HTTP server block (`lines 1239..1284`) with a non-blocking multi-chunk `.rodata` streamer & immediate `TimeWait` recycler:
   - First, if `!pending_http_body.is_empty()`:
     - If `http_sock.may_send()`:
       - `if let Ok(sent) = http_sock.send_slice(pending_http_body) { pending_http_body = &pending_http_body[sent..]; }`
       - `if pending_http_body.is_empty() { http_sock.close(); pending_http_closing = true; }`
     - Else:
       - `pending_http_body = &[]; http_sock.abort(); pending_http_closing = false;`
   - Second, if `pending_http_closing`:
     - If `http_sock.send_queue() == 0 && (!http_sock.may_send() || matches!(http_sock.state(), tcp::State::TimeWait | tcp::State::CloseWait | tcp::State::Closing | tcp::State::LastAck | tcp::State::FinWait2 | tcp::State::Closed))`:
       - `http_sock.abort(); pending_http_closing = false;`
   - Third, if `!http_sock.is_open()`:
     - `pending_http_body = &[]; pending_http_closing = false; let _ = http_sock.listen(80);`
   - Fourth, if `pending_http_body.is_empty() && !pending_http_closing && http_sock.can_recv() && http_sock.may_send()`:
     - Read `let mut req_buf = [0u8; 256]; let n = http_sock.recv_slice(&mut req_buf).unwrap_or(0);`
     - Drain any remaining HTTP request headers in `http_rx_buf`:
       `while http_sock.can_recv() { let _ = http_sock.recv(|buf| (buf.len(), ())); }`
     - Use `let mut resp_buf = [0u8; 640];` (saving `1,152` bytes of stack compared to the old `512 + 1536` buffers!) and call `handle_http_request_streaming(&req_buf[..n], &snap, &mut resp_buf)`:
       - Send `&resp_buf[..hdr_or_resp_len]` via `http_sock.send_slice(...)`.
       - If `!static_body.is_empty()`:
         - Set `pending_http_body = static_body;`
         - Immediately attempt `if let Ok(s) = http_sock.send_slice(pending_http_body) { pending_http_body = &pending_http_body[s..]; }`
         - If `pending_http_body.is_empty()`, call `http_sock.close(); pending_http_closing = true;`.
       - Else:
         - Call `http_sock.close(); pending_http_closing = true;`.
       - Handle `action != HttpControlAction::None` as before.
   - Fifth, if `pending_http_body.is_empty() && http_sock.may_recv() && !http_sock.may_send()`:
     - `http_sock.abort(); pending_http_closing = false;`

## 3. 5-Gate `--stage-only` Verification
1. Gate 1: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`).
2. Gate 2: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`).
3. Gate 3: `rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`).
4. Gate 4: Verify via `readelf -S` that `.stack` is **`0x008eb0` (`36,528` bytes)** — exact match to `435b5cf6`!
5. Gate 5: Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32` and verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` prints `[STAGE-ONLY AUDIT PASS]`.
6. Commit to `MK5-Rust`, verify `manage_task(list)` has 0 running tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T090500_wo068_webui_streaming_fix.md`, update `WO-068` Fulfillment Section, and reply with your report.

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Commit Hash**: `c37c3d84` on branch `MK5-Rust`
- **Verification Gates**:
  - Gate 1: `cargo test --workspace` PASSED (41 cyd, 78 louder-esp32-app, 23 decoders, 18 proto, 10 proto_fuzz, 11 tas58xx tests passed, 0 failures)
  - Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` PASSED (0 warnings)
  - Gate 3: Xtensa release build (`cargo build --release --target xtensa-esp32-none-elf -p louder-esp32-app`) PASSED (`louder-esp32-app` ELF built in 4.49s)
  - Gate 4: `readelf -S` stack verification: `.stack` size `0x008eb0` = `36,528` bytes (EXACT byte-for-byte match with `435b5cf6` and `41eb03a4`)
  - Gate 5: `espflash save-image` produced `735,184` B OTA binary; `verify_staged_ota_bin.py` PASSED (`[STAGE-ONLY AUDIT PASS]`, sha256 `f1b3c048d3cb92261eeab305fd0ee9a9b432a909079dac13191cc58785904488`)
- **Remediated State**:
  - `http_control.rs`: Implemented `handle_http_request_streaming` returning header slice and static body slice for multi-chunk `.rodata` streaming.
  - Added `GET /favicon.ico` returning `204 No Content`.
  - Added `Access-Control-Allow-Origin: *` to JSON endpoints.
  - Expanded `DASHBOARD_HTML` in Flash `.rodata` with live telemetry (live FPS, RSSI, LMS, Amp mode, I2C ACKs/NACKs, Anti-brick status), full LMS transport and volume controls, 4 stream presets (Ibiza Global Radio, WHYY 90.9, WDIY 88.1, Basement WAV), direct LMS link, and guarded system recovery/OTA buttons.
  - `main.rs`: Integrated streaming loop for `pending_http_body`, drained remaining request headers on recv, and added immediate recycling of closing sockets (`TimeWait`, `CloseWait`, `Closing`, `LastAck`, `FinWait2`, `Closed`).
  - Reduced Port 80 stack frame allocation from 2,048 B to 896 B (`req_buf: [u8; 256]`, `resp_buf: [u8; 640]`), preserving the exact `36,528` B `.stack` budget.
