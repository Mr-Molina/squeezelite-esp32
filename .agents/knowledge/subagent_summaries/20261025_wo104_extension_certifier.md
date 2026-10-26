# Subagent Summary: WO-104 (`Extension Certifier`)

- **Work Order**: `WO-104` ([WO-104-unwedge-closewait-and-live-cert.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-104-unwedge-closewait-and-live-cert.md))
- **Role**: `Extension Certifier` (`self`, Conversation ID `637970f5-86a7-42a0-8679-ce58fa01862f`)
- **Timestamp**: `2026-10-25T17:53:30-04:00`
- **Status**:
  - Step 1 (Pre-Flight SHA-256 Verification): `[EXECUTED & VERIFIED]`
  - Step 2 (Unwedge `172.22.1.15:80` `CloseWait` & Reboot to Factory Recovery): `[EXECUTED & VERIFIED]`
  - Step 3 (Single OTA Flash of `9612ffe8` `WO-103`): `[EXECUTED & VERIFIED]`
  - Step 4 (Wave 4 Task 10 Live Certification Gates): `[BLOCKED / ERROR]` (`"core1_active": false, "core1_loops": 0` & `GET /` timeout/rollback; halted per 1-Flash-Attempt Circuit Breaker)

## 1. Files Created, Modified, or Deleted (Absolute Paths)
1. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/unwedge_port80.py` (Created/Modified)
   - **Rationale**: Closed-loop raw TCP `SYN` scanner (`10` ports/batch) + RFC 5961 Challenge-ACK/RST unwedger that pinpointed `sport = 44820`, injected in-window `RST` (`seq = 0xc0d86c83`), and transitioned `172.22.1.15` to Factory recovery via `POST /recovery.json`.
2. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/pinpoint_port.py` (Created)
   - **Rationale**: Diagnostic closed-loop raw TCP `SYN` scanner used to isolate `sport = 44820` on `172.22.1.15:80`.
3. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/wo104_flash_and_poll.py` (Created)
   - **Rationale**: Executed the single `POST /flash.json` upload of `9612ffe8` (`WO-103`) and polled `GET /status.json` + `GET /`.
4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md` (Modified, backup at `/home/console-admin/.gemini/antigravity/brain/637970f5-86a7-42a0-8679-ce58fa01862f/scratch/DIAGNOSTIC_LOG.md.bak`)
   - **Rationale**: Appended Entry 56 documenting the closed-loop `CloseWait` unwedging (`sport = 44820`), live `722` B `/status.json` payload on `9612ffe8`, and the root-cause analysis of `core1_active: false` (`esp-hal 1.2.2` `Err(CoreAlreadyRunning)` + `esp-storage` `multicore_auto_park` unparking rogue `APP_CPU`) and `GET /` timeout.
5. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-104-unwedge-closewait-and-live-cert.md` (Modified, backup at `/home/console-admin/.gemini/antigravity/brain/637970f5-86a7-42a0-8679-ce58fa01862f/scratch/WO-104-unwedge-closewait-and-live-cert.md.bak`)
   - **Rationale**: Appended the Rule 15 Fulfillment Section and updated status.
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo104_extension_certifier.md` (Created)
   - **Rationale**: Rule 17 subagent summary.
7. `/home/console-admin/.gemini/antigravity/brain/637970f5-86a7-42a0-8679-ce58fa01862f/handoff.md` (Created)
   - **Rationale**: Rule 17 handoff report.

*(Zero files under `/home/console-admin/Github/squeezelite-esp32/rust/crates/**` were modified, strictly complying with Invariant 47.)*

## 2. Key Findings & Root Cause Summary
1. **Closed-Loop `CloseWait` Unwedging (`sport = 44820`)**:
   - Open-loop raw `ACK` sweeps failed because (a) `esp-radio` Wi-Fi TX/RX rings drop packets when >16–20 `RST` replies are queued in flight, and (b) router `10.0.0.1` (`nf_conntrack`) drops out-of-window Challenge `ACK` replies when preceded by a 4096-window bare `ACK`.
   - Probing 10 ports/batch in closed loop (`unwedge_port80.py`) isolated `sport = 44820`, triggered Challenge `ACK` (`seq=0x29d2f753, ack=0xc0d86c83`), injected `RST` (`seq=0xc0d86c83`), and rebooted `172.22.1.15` to Factory recovery via `POST /recovery.json`.
2. **Live Verification of `WO-103` (`9612ffe8`) on `172.22.1.15`**:
   - `[Poll 2]` returned `HTTP 200 OK` (`722` B) on `GET /status.json`, confirming `STATIC_HTTP_RESP_BUF: [u8; 1536]` fixed the `768`-byte buffer overflow and proving live 4 MB PSRAM (`3,538,944` B free, `655,360` B used) and live `TAS5805M` telemetry (`48000` Hz, BCK `64`, PLL locked, `again_code: 8`, `eq_preset: "Flat"`, `i2c_acks: 79`, `i2c_nacks: 0`).
3. **Root Cause of `"core1_active": false, "core1_loops": 0` & Rogue `APP_CPU` Execution**:
   - `esp-hal 1.2.2` on `esp32` does not park `Cpu::AppCpu` during `esp_hal::init()`. After warm reboot from dual-core ESP-IDF recovery, `is_running(Cpu::AppCpu)` is `true`.
   - Gate 1 `FlashStorage::new(...).multicore_auto_park()` sees `is_running(Cpu::AppCpu) == true`, parks `AppCpu` during `flash.erase`, and then **unparks `AppCpu` in `post_write(true)`** (`esp-storage-0.10.0/src/common.rs:323`).
   - Then `cpu_ctrl.start_app_core(...)` sees `is_running(Cpu::AppCpu) == true` and returns `Err(Error::CoreAlreadyRunning)` (`esp-hal-1.2.2/src/system/multi_core.rs:246`), leaving `_app_core_guard.is_ok() == false` AND leaving `APP_CPU` unparked executing stale code/ROM without a valid stack or cache!
   - **Fix**: Call `unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }` immediately after `esp_hal::init(config)` (before Gate 1 `FlashStorage` and before `start_app_core`), and increase `APP_CORE_STACK` to `Stack<2048>`.
4. **Root Cause of `GET /` Stalled-Socket Wedge on Weak RSSI (`-82 dBm`)**:
   - In `src/main.rs:1503-1536`, when `pending_http_body` is non-empty (`!pending_http_body.is_empty()` during chunked streaming of `4,827`-byte `DASHBOARD_HTML`), there is no deadline or `!http_sock.may_recv()` abort check (line 1530 checks `if pending_http_body.is_empty()`). Adding a `2,500 ms` deadline + `!http_sock.may_recv()` abort when `pending_http_closing || !pending_http_body.is_empty()` prevents `GET /` from ever wedging `http_sock`.

## 3. Known Risks & Untested Assumptions
- `172.22.1.15` is currently online in Factory recovery (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -47`) and ready for the next OTA flash once `park_core(Cpu::AppCpu)` and the `pending_http_body` timeout are compiled in `WO-105`.
