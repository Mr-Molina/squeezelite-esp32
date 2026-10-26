# Work Order: WO-037 — Execute Gate 5B (`POST /flash.json` Staged Rust OTA Binary & Verify Single-Boot Trial Mode)

**Work Order ID:** WO-037  
**Timestamp:** 2026-10-24T08:44:45-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** Live Network Endpoint `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5a.json`  
**Defect IDs:** Task 5 Gate 5B — Stream Staged `.bin` to `ota_0` (`0x160000`) & Verify Rust `/status.json` + LMS Registration  
**Task Goal:** While `172.22.1.15` is verified in `"recovery": 1`, upload `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`659,776` bytes, SHA-256 `856697537255742d58d5b2a7dfaebf0fc718207255cad3e458e44f72fff4cae6`) via `POST http://172.22.1.15/flash.json`, and verify via independent read-back (`GET http://172.22.1.15/status.json` and LMS `10.0.0.10:9000` JSON-RPC) that `172.22.1.15` boots `louder-esp32-app` with `"version":"I2S-4MFlash-Rust"`, `"rust_engine":true`, and `"otadata_prearmed_factory":true`.  

---

## 1. Context & Scope
- **Problem Statement:** Stream the certified `--stage-only` pure-Rust OTA binary into `ota_0` (`0x160000`) from the verified factory `recovery` partition (`0x10000`) and verify single-boot trial mode (`otadata_prearmed_factory: true`), Port 80 HTTP status, and LMS (`10.0.0.10:3483`) player connection.
- **Single Variable Constraint (Rule 5):** Execute Gate 5B only after Gate 5A (`"recovery": 1`) is verified and authorized by the user.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Verify `"recovery": 1` on `http://172.22.1.15/status.json` and verify SHA-256 `856697537255742d58d5b2a7dfaebf0fc718207255cad3e458e44f72fff4cae6` on `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` before uploading.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 4, Rule 21, Rule 52):** Independently verify both `GET http://172.22.1.15/status.json` and `POST http://10.0.0.10:9000/jsonrpc.js`.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** `POST --data-binary @louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json` passes `ota_header_check()`, writes `659,776` bytes to `ota_0` (`0x160000`), sets `otadata` (`0xD000`) to `ota_0`, and boots `louder-esp32-app`, which immediately erases `otadata` back to `0xFF` (`Factory`), decrypts `"iot"` credentials from `0x9000` NVS, acquires `172.22.1.15`, serves `/status.json`, and registers `"BASEMENT"` on LMS `10.0.0.10:3483`.
- **Predicted Outcome:** `GET http://172.22.1.15/status.json` returns `"version":"I2S-4MFlash-Rust"`, `"rust_engine":true`, `"otadata_prearmed_factory":true`, and LMS `10.0.0.10:9000` reports `"BASEMENT"` connected.
- **Falsification Criteria:** If `louder-esp32-app` fails to connect within 45s (Gate 3) or panics (Gate 2), Gate 1 (`otadata` erased to `0xFF`) causes `172.22.1.15` to automatically reboot back into `recovery` (`0x10000`, `"recovery": 1`).

---

## 3. Detailed Instructions
1. Verify `"recovery": 1` on `GET http://172.22.1.15/status.json`.
2. Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` to `http://172.22.1.15/flash.json`.
3. Poll `GET http://172.22.1.15/status.json` and query LMS `http://10.0.0.10:9000/jsonrpc.js` to verify post-boot state.
4. Write a Rule 17 subagent summary and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T09:03:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5b_ota.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5b.json` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_post_gate5b.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_090100_gate5b_certifier.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-037-gate5b-flash-ota0.md` (modified)
- **Backup Verification:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5a.json` (verified `"recovery": 1`)
- **Test / Verification Command & Results:**
  - `POST http://172.22.1.15/flash.json` returned HTTP 200 `File uploaded. Flashing started.`
  - Polling script confirmed device rebooted and booted `ota_0` at 21.3s with:
    `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-Rust","recovery":0,"rust_engine":true,"otadata_prearmed_factory":true,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-83,"lms_cport":9090,"lms_port":9000,"lms_ip":"10.0.0.10","frames_played":0,"amp_mode":"Play","i2c_acks":92435,"i2c_nacks":0,"uptime_ms":11741}`
  - LMS `10.0.0.10:9000` JSON-RPC query returned:
    `{"player_ip":"172.22.1.15:60846","player_name":"BASEMENT","player_connected":1,"power":1,"mode":"stop","mixer volume":40}`
- **Observed Outcome vs Predicted Outcome:** Matched prediction exactly. `172.22.1.15` successfully streamed pure-Rust binary into `ota_0` (`0x160000`), rebooted, executed pre-arm erase of `otadata` to `0xFF` (`otadata_prearmed_factory: true`), reconnected to Wi-Fi SSID `"iot"`, served `/status.json` with `rust_engine: true`, and registered on LMS `10.0.0.10:3483` as `"BASEMENT"`.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_090100_gate5b_certifier.md`
- **Handoff Path:** [WO-037-gate5b-flash-ota0.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-037-gate5b-flash-ota0.md)
