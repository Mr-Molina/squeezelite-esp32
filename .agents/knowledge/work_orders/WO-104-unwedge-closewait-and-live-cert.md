# Work Order: WO-104 — Unwedge `172.22.1.15:80` `CloseWait`, Flash `WO-103` (`9612ffe8`), & Complete Wave 4 Task 10 Live Certification

## Metadata
- **Work Order ID**: `WO-104`
- **Timestamp**: `2026-10-25T17:22:00-04:00`
- **Assigned Subagent Role**: Extension Certifier (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[BLOCKED / ERROR]` (Steps 1–3 `[EXECUTED & VERIFIED]`; Step 4 `[BLOCKED / ERROR]` on `"core1_active": false` & `GET /` reset/rollback)

## Task Goal
1. Unwedge the single `smoltcp` port-80 listener (`State::CloseWait`) on `172.22.1.15` (`"BASEMENT"`, currently running `7717733f`) using the RFC 5961 / `smoltcp` Challenge-ACK -> Kernel-RST mechanism, then transition `172.22.1.15` to Factory recovery (`0x10000`, `"recovery": 1`) via `POST http://172.22.1.15/recovery.json`.
2. Flash the verified `WO-103` OTA binary (`/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, commit `9612ffe8345c54b0d9937be7e409c9f1472b2285`, `756,096` bytes, SHA-256 `500e9260bb7a4c6b5ca8911698b1a7f1cf1441f394c17331bbfef2065ed7596c`) to `172.22.1.15` (`ota_0`).
3. Execute the complete Wave 4 Task 10 live hardware certification suite (`GET /status.json`, `GET /`, interactive `/cmd.json` EQ & AGAIN mutations + `settings_seq` persistence, 35-second continuous audio playback soak, and LMS `10.0.0.10` UPnP/AirPlay sibling bridge verification).

## Hard Safety & Role Invariants (MANDATORY — Invariant 47)
- **Source-Edit Ban**: You are strictly **FORBIDDEN** from modifying any `.rs` or `Cargo.toml` file under `/home/console-admin/Github/squeezelite-esp32/rust/crates/**` or running `cargo build`.
- **1-Flash-Attempt Circuit Breaker**: Execute at most **ONE** `POST /flash.json` upload.
- **Anti-Brick Rules**:
  - NEVER call `POST /reboot_ota.json`.
  - NEVER touch `/dev/ttyUSB0`.

## Step-by-Step Execution Procedure

### Step 1: Pre-Flight SHA-256 Verification
- Verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` has size `756,096` bytes and SHA-256 `500e9260bb7a4c6b5ca8911698b1a7f1cf1441f394c17331bbfef2065ed7596c`.

### Step 2: Unwedge `172.22.1.15:80` (`State::CloseWait`) & Reboot into Factory Recovery (`0x10000`)
- First test `curl -sS -X POST --max-time 2 http://172.22.1.15/recovery.json`.
- If `172.22.1.15:80` returns `Connection refused` (because `http_sock` on `7717733f` is in `State::CloseWait` bound to `(<our_host_ip>, <poll_3_ephemeral_port>)` from `WO-102` `[Poll 3]`):
  - Write and execute a Python script `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/unwedge_port80.py` (using `sudo -n python3`):
    1. Determine local source IP toward `172.22.1.15` via a UDP dummy connect (`s.connect(("172.22.1.15", 80)); src_ip = s.getsockname()[0]`).
    2. Open a raw TCP socket `socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_TCP)`.
    3. For each candidate source port `sport` in `range(32768, 61000)`:
       - Construct and send two bare TCP `ACK` segments (`flags = 0x10`, `seq = 1`, `window = 4096`) from `(src_ip, sport)` to `("172.22.1.15", 80)` with valid TCP pseudo-header checksums:
         - Packet A: `ack_seq = 0x00000001`
         - Packet B: `ack_seq = 0x80000001`
       - Why this works deterministically (`smoltcp-0.12.0/src/socket/tcp.rs:1586, 1746`):
         - Because `TcpSeqNumber` uses RFC 1982 signed 32-bit comparison, for the `sport` that matches `http_sock.tuple.remote.port`, one of `0x00000001` or `0x80000001` is guaranteed to satisfy `ack_number > ack_max` (`tcp.rs:1586`).
         - `smoltcp` immediately replies with a Challenge `ACK (ack = window_start)` to `(src_ip, sport)`.
         - The Linux kernel receives the unsolicited Challenge `ACK` on `sport` (which has no open socket in the kernel) and automatically sends `RST (seq = window_start)` back to `172.22.1.15:80`.
         - `smoltcp` (`tcp.rs:1746`) accepts the in-window `RST`, sets `http_sock` to `State::Closed`, and `main.rs:1515` immediately re-opens `http_sock.listen(80)`!
       - Pace the sweep gently (e.g., `time.sleep(0.001)` every 25 ports = `~1.1s` total so ESP32 Wi-Fi RX buffers aren't overrun), or check `POST http://172.22.1.15/recovery.json` every `4,000` ports so it stops as soon as `http_sock` resets!
  - Note: What is the response size of `POST http://172.22.1.15/recovery.json` on `7717733f`? Only **`211` bytes** (`<= 768` bytes!), so `POST http://172.22.1.15/recovery.json` succeeds immediately on `7717733f` and triggers `HttpControlAction::RebootToRecovery` (`250ms` drain -> `software_reset()` into Factory `recovery` `0x10000` because Gate 1 `otadata` is already `0xFF`)!
  - Wait 6–10 seconds after `POST /recovery.json` returns `{"status":"rebooting_to_factory_recovery"}` and verify `GET http://172.22.1.15/status.json` returns `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).

### Step 3: Single OTA Flash of `9612ffe8` (`WO-103`)
- Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`756,096` bytes) via `POST http://172.22.1.15/flash.json`.
- Wait 18–28 seconds and poll `GET http://172.22.1.15/status.json` (timeout 3s, 3s interval, up to 15 polls).

### Step 4: Full Wave 4 Task 10 Live Certification Gates
1. **`GET http://172.22.1.15/status.json` (`~833` B JSON)**:
   - Verify `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
   - Verify 4 MB PSRAM fields (`"psram_free_bytes"`, `"psram_used_bytes": 655360`).
   - Verify Dual-Core fields (`"core1_active": true`, `"core1_loops" > 0`).
   - Verify TI `TAS5805M` live hardware telemetry (`"tas_fs_hz"`, `"tas_bck_ratio"`, `"tas_pll_locked"`, `"again_code"`, `"eq_preset"`, `"settings_seq"`, `"i2c_nacks": 0`).
2. **`GET http://172.22.1.15/` Web UI Dashboard**:
   - Verify `HTTP 200 OK` and full HTML ending in `</html>` (`0` missing bytes).
3. **Interactive `/cmd.json` Mutations (Invariant 44)**:
   - Execute and verify state read-back in `/status.json` after each command:
     - `GET http://172.22.1.15/cmd.json?act=eq_bass` -> `"eq_preset": "BassBoost"`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=eq_loudness` -> `"eq_preset": "Loudness"`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=again6` -> `"again_code": 6`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=again8` -> `"again_code": 8`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=eq_flat` -> `"eq_preset": "Flat"`, `"settings_seq"` increments
4. **35-Second Continuous Audio Playback Soak Test**:
   - Ensure `"BASEMENT"` (`c8:2e:18:be:b8:34`) is playing on LMS (`10.0.0.10:9000/jsonrpc.js`).
   - Poll `GET http://172.22.1.15/status.json` every 5 seconds for 35 seconds (8 samples: `t = 0, 5, 10, 15, 20, 25, 30, 35s`).
   - Verify `frames_played` advances monotonically across all 35 seconds at `~44,100 fps`, `i2c_nacks == 0`, `amp_mode == "Play"`, and `core1_loops` increases monotonically.
5. **LMS `10.0.0.10` Sibling Bridge Verification**:
   - Verify UPnP `MediaRenderer.xml` returns `HTTP 200 OK` (`3,105` B) and AirPlay `avahi-publish-service` (`_raop._tcp`) is active on `10.0.0.10`.

## Deliverables
- Append fulfillment section to this work order (`WO-104-unwedge-closewait-and-live-cert.md`).
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo104_extension_certifier.md`.

## Fulfillment Section

### Summary of Execution & Status (Rule 52)
- **Step 1 — Pre-Flight SHA-256 Verification (`[EXECUTED & VERIFIED]`)**:
  - Verified `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`: size `756,096` bytes, SHA-256 `500e9260bb7a4c6b5ca8911698b1a7f1cf1441f394c17331bbfef2065ed7596c`.
- **Step 2 — Unwedge `172.22.1.15:80` (`State::CloseWait`) & Reboot into Factory Recovery (`[EXECUTED & VERIFIED]`)**:
  - Verified `curl -sS -X POST --max-time 2 http://172.22.1.15/recovery.json` initially failed with `Connection refused` (`exit 7`).
  - Diagnosed why open-loop raw `ACK` sweeps (`40,000` pps / `2,000` pps) failed to unwedge `smoltcp`:
    1. `esp-radio` Wi-Fi TX/RX rings (`~16–20` packet buffers) drop incoming packets when flooded faster than the round-trip Wi-Fi `RST` transmit rate.
    2. Router `10.0.0.1` (`nf_conntrack`) drops out-of-window Challenge `ACK` replies when preceded by a bare `ACK` with `window = 4096`.
  - Implemented closed-loop raw TCP `SYN` probing (`10` ports/batch, awaiting all `10` `RST|ACK` `flags=0x14` replies before advancing) in `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/unwedge_port80.py` (and `pinpoint_port.py`).
  - Pinpointed the exact wedged ephemeral port from `WO-102` `[Poll 3]`: **`sport = 44820`** (the single port in `32768..60998` where `smoltcp` `State::CloseWait` accepted the `SYN` and returned `None` instead of `RST`).
  - Sent bare `ACK` (`ack_seq = 0x00000001`, `flags = 0x10`, `window = 65535`) from `10.0.0.90:44820 -> 172.22.1.15:80`, which elicited the `smoltcp` Challenge `ACK`:
    `Reply on wedged port 44820: flags=0x10 seq=0x29d2f753 ack=0xc0d86c83`
  - Immediately injected the exact in-window `RST` (`seq = 0xc0d86c83`, `flags = 0x04`) from `10.0.0.90:44820 -> 172.22.1.15:80`, transitioning `http_sock` to `State::Closed` and reopening `http_sock.listen(80)`.
  - Executed `POST http://172.22.1.15/recovery.json`, which returned `{"status":"rebooting_to_recovery","target_partition":"factory_0x10000"}`, and verified 8s later that `GET http://172.22.1.15/status.json` returned `"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -47`.
- **Step 3 — Single OTA Flash of `9612ffe8` (`WO-103`) (`[EXECUTED & VERIFIED]`)**:
  - Streamed `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`9612ffe8`) once via `POST http://172.22.1.15/flash.json` (`HTTP 200`, `1.25s`, `File uploaded. Flashing started.`).
  - At `[Poll 2]` (`17:35:32.25`, `uptime_ms = 12,630`), `GET http://172.22.1.15/status.json` returned `HTTP 200 OK` (`722` bytes):
    ```json
    {"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-Rust","recovery":0,"rust_engine":true,"otadata_prearmed_factory":true,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-82,"lms_cport":9090,"lms_port":9000,"lms_ip":"10.0.0.10","frames_played":0,"amp_mode":"Play","vol_pct":80,"i2c_acks":79,"i2c_nacks":0,"uptime_ms":12630,"psram_free_bytes":3538944,"psram_used_bytes":655360,"core1_active":false,"core1_loops":0,"codec":"PCM","codec_sample_rate":44100,"codec_channels":2,"codec_bit_depth":16,"eq_preset":"Flat","eq_enabled":false,"tas_fs_hz":48000,"tas_bck_ratio":64,"tas_pll_locked":true,"tas_automuted":false,"again_code":8,"settings_seq":0}
    ```
- **Step 4 — Wave 4 Task 10 Live Certification Gates (`[BLOCKED / ERROR]`)**:
  - **Step 4.1 (`GET /status.json`)**:
    - Verified `WO-103`'s `STATIC_HTTP_RESP_BUF: [u8; 1536]` fix resolved the `768`-byte overflow (`HTTP 200 OK`, `722` B).
    - Verified `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
    - Verified 4 MB PSRAM fields (`"psram_free_bytes": 3538944`, `"psram_used_bytes": 655360`).
    - Verified TI `TAS5805M` live hardware telemetry (`"tas_fs_hz": 48000`, `"tas_bck_ratio": 64`, `"tas_pll_locked": true`, `"tas_automuted": false`, `"again_code": 8`, `"eq_preset": "Flat"`, `"settings_seq": 0`, `"i2c_acks": 79`, `"i2c_nacks": 0`).
    - **Failed Dual-Core Gate**: `"core1_active": false, "core1_loops": 0` (expected `"core1_active": true, "core1_loops" > 0`).
  - **Step 4.2 (`GET /` Web UI Dashboard) & Rollback**:
    - `0.5s` after `[Poll 2]` (`17:35:32.75`), `GET http://172.22.1.15/` timed out (`TimeoutError: timed out`), and `172.22.1.15` reset and rolled back to Factory recovery (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -47`).
    - Halted immediately per the 1-Flash-Attempt Circuit Breaker. `172.22.1.15` is verified online and healthy in Factory recovery (`"recovery": 1`, `"rssi": -47`).

### Root Cause Analysis for `WO-105` Remediation
1. **Root Cause 1 — `"core1_active": false, "core1_loops": 0` AND Unparked Rogue `APP_CPU` Crash (`esp-hal-1.2.2` + `esp-storage-0.10.0`)**:
   - In `esp-hal-1.2.2/src/soc/esp32/mod.rs:27` (`pub(crate) fn pre_init() {}`), `esp-hal` on `esp32` does **not** park `Cpu::AppCpu` during `esp_hal::init()`. After a warm reboot from dual-core ESP-IDF FreeRTOS recovery (`I2S-4MFlash-1681`), `dport.appcpu_ctrl_b().appcpu_clkgate_en()` is still `1` and `LPWR.sw_cpu_stall` is not `0x86`, so `CpuControl::is_running(Cpu::AppCpu)` (`esp-hal-1.2.2/src/soc/esp32/cpu_control.rs:41-82`) returns **`true`**.
   - At Gate 1 (`src/main.rs:1137`), `FlashStorage::new(peripherals.FLASH).multicore_auto_park()` sees `is_running(Cpu::AppCpu) == true` before `flash.erase(0xD000, 0xF000)`, stalls `AppCpu` during the flash write, and then calls **`self.park(false)` (`unpark_core(Cpu::AppCpu)`) in `post_write(true)`** (`esp-storage-0.10.0/src/common.rs:323`)!
   - Then at `src/main.rs:1246`, `cpu_ctrl.start_app_core(...)` checks `if !debugger_connected() && is_running(Cpu::AppCpu)` (`esp-hal-1.2.2/src/system/multi_core.rs:246`) and immediately returns **`Err(Error::CoreAlreadyRunning)`** without setting `appcpu_boot_addr` or resetting `APP_CPU`!
   - Consequently:
     1. `_app_core_guard.is_ok()` is `false`, producing `"core1_active": false, "core1_loops": 0`.
     2. Worse, `APP_CPU` is left **unparked and executing stale code/ROM with an uninitialized stack and cache** while `PRO_CPU` runs!
   - **Exact Fix (`src/main.rs:1130`)**:
     - Immediately after `let peripherals = esp_hal::init(config);` (BEFORE Gate 1 `FlashStorage::new(peripherals.FLASH).multicore_auto_park()` and BEFORE `start_app_core`), instantiate `let mut cpu_ctrl = esp_hal::system::CpuControl::new(peripherals.CPU_CTRL);` and call:
       ```rust
       unsafe {
           cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu);
       }
       ```
     - With `AppCpu` explicitly parked at boot, `is_running(Cpu::AppCpu)` is `false`:
       - Gate 1 `FlashStorage::multicore_auto_park()` sees `need_auto_park == false` and does NOT unpark `AppCpu` after `flash.erase`.
       - `cpu_ctrl.start_app_core(...)` sees `is_running(Cpu::AppCpu) == false`, succeeds (`Ok(AppCoreGuard)` -> `"core1_active": true`), sets `appcpu_boot_addr`, resets `AppCpu`, and boots Core 1 cleanly into the closure (`"core1_loops" > 0`).
     - Also increase `APP_CORE_STACK` from `Stack<512>` to `Stack<2048>` (`src/main.rs:943`) for adequate Core 1 stack headroom.
2. **Root Cause 2 — `pending_http_body` Stalled-Socket Timeout & Multi-AP RSSI (`-82 dBm`)**:
   - In `[Poll 2]`, `172.22.1.15` associated to a distant `"iot"` BSSID at `-82 dBm` (vs `-47 dBm` in recovery).
   - In `src/main.rs:1503-1536`, when `GET /` (`4,827` B `DASHBOARD_HTML`) streams in chunks across `http_tx_buf` (`1,536` B), `pending_http_body` is non-empty (`!pending_http_body.is_empty()`). If TCP ACKs stall or the client times out (`3.0s`) and closes (`State::CloseWait`) while `http_tx_buf` is full (`send_slice` returns `Ok(0)`), `http_sock.may_send()` remains `true` in `CloseWait` and line 1530 (`if pending_http_body.is_empty() && ...`) never aborts `http_sock` because `pending_http_body` is non-empty!
   - **Exact Fix (`src/main.rs:1503-1536`)**:
     - Track `pending_http_started_ms: u64` when a request sets `pending_http_body` or `pending_http_closing = true`. If `(pending_http_closing || !pending_http_body.is_empty()) && (!http_sock.may_recv() || now_ms.saturating_sub(pending_http_started_ms) >= 2_500)`, abort `http_sock`, clear `pending_http_body = &[]`, and set `pending_http_closing = false` so `http_sock.listen(80)` immediately re-arms.
