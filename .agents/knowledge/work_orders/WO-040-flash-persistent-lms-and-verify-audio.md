# Work Order: WO-040 — Stream Updated `louder-esp32-app-basement-ota.bin` (`678,176` bytes) to `ota_0` & Verify Persistent LMS Connection

**Work Order ID:** WO-040  
**Timestamp:** 2026-10-24T11:28:30-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** Live Network Endpoint `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5c.json`  
**Defect IDs:** Task 7 — Flash `WO-039` Staged `.bin` (`d445a736`) to `ota_0` (`0x160000`) & Verify Persistent LMS Registration  
**Task Goal:** While `172.22.1.15` is verified in `"recovery": 1`, upload `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`678,176` bytes, SHA-256 `63b8c46cde45f7cf72de1bb70ab1b725b70d1d2b4210f241c16846005a0b7783`) via `POST http://172.22.1.15/flash.json`, and verify via independent read-back (`GET http://172.22.1.15/status.json` and LMS `10.0.0.10:9000` JSON-RPC) that `172.22.1.15` boots `louder-esp32-app` (`"version":"I2S-4MFlash-Rust"`, `"rust_engine":true`, `"otadata_prearmed_factory":true`) and remains persistently registered as `"BASEMENT"` (`c8:2e:18:be:b8:34`, `"connected": 1`) in LMS `serverstatus` across a 35-second keepalive window.  

---

## 1. Context & Scope
- **Problem Statement:** Deploy the certified `WO-039` (`d445a736`) pure-Rust OTA binary to `ota_0` (`0x160000`) from the Factory `recovery` partition (`0x10000`) and empirically verify that `"BASEMENT"` (`c8:2e:18:be:b8:34`) stays persistently registered in LMS `10.0.0.10:9000` `serverstatus` (`players_loop`).
- **Single Variable Constraint (Rule 5):** Execute ONLY the OTA flash of `d445a736` and read-back verification.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Verify `"recovery": 1` on `http://172.22.1.15/status.json` and verify SHA-256 `63b8c46cde45f7cf72de1bb70ab1b725b70d1d2b4210f241c16846005a0b7783` before uploading.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 4, Rule 21, Rule 52):** Independently verify `GET http://172.22.1.15/status.json` and LMS `http://10.0.0.10:9000/jsonrpc.js` both at boot (`t0`) and after a 35-second soak (`t0 + 35s`).

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** `POST --data-binary @louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json` flashes `678,176` bytes to `ota_0`, boots `louder-esp32-app`, sends `HELO` + `SETD` `"BASEMENT"` + `STAT`, drains coalesced `vers`/`setd`/`audg`/`strm` frames, echoes `strm` `'t'` `replay_gain` timestamps, and keeps `"BASEMENT"` (`c8:2e:18:be:b8:34`) continuously connected in LMS `serverstatus`.
- **Predicted Outcome:** `GET http://172.22.1.15/status.json` returns `"version":"I2S-4MFlash-Rust"`, `"rust_engine":true`, `"otadata_prearmed_factory":true`, and LMS `serverstatus` includes `{"playerid":"c8:2e:18:be:b8:34","name":"BASEMENT","connected":1}` both immediately and after 35 seconds.
- **Falsification Criteria:** If `c8:2e:18:be:b8:34` is missing from `players_loop` at `t0 + 35s`, the LMS keepalive hypothesis is falsified.

---

## 3. Detailed Instructions
1. Verify `"recovery": 1` on `GET http://172.22.1.15/status.json`.
2. Upload `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`678,176` bytes) via `POST http://172.22.1.15/flash.json`.
3. Poll `GET http://172.22.1.15/status.json` until `"version": "I2S-4MFlash-Rust"` boots.
4. Query LMS `http://10.0.0.10:9000/jsonrpc.js` (`serverstatus` and player `status`) at boot and again after 35 seconds (`time.sleep(35)` in a Python verification script `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo040_lms_persistence.py`), saving outputs to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_wo040.json` and `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_post_wo040.json`.
5. Write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_112900_wo040_certifier.md` and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T11:32:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo040_lms_persistence.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_wo040.json` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_post_wo040.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_112900_wo040_certifier.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-040-flash-persistent-lms-and-verify-audio.md` (modified)
- **Backup Verification:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5c.json`
- **Test / Verification Command & Results:**
  - `POST http://172.22.1.15/flash.json` returned HTTP 200 `File uploaded. Flashing started.`
  - Device booted pure-Rust `ota_0` at 10.0s (`version: "I2S-4MFlash-Rust"`, `rust_engine: true`, `otadata_prearmed_factory: true`).
  - At `t0 + 2s`: LMS `serverstatus` recognized `BASEMENT` (`c8:2e:18:be:b8:34`, `connected: 1`, `ip: 172.22.1.15:60832`), LMS player status `connected: 1`.
  - At `t0 + 35s` (after 35-second keepalive soak):
    - `status.json`: `version: "I2S-4MFlash-Rust"`, `recovery: 0`, `uptime_ms: 49752`, `i2c_acks: 364196`, `i2c_nacks: 0`.
    - LMS `serverstatus` (`players_loop`): `{"playerid": "c8:2e:18:be:b8:34", "name": "BASEMENT", "connected": 1, "ip": "172.22.1.15:60832", "power": 1}`.
    - LMS player status: `{"player_name": "BASEMENT", "player_connected": 1, "power": 1, "mode": "stop", "mixer volume": 40}`.
- **Observed Outcome vs Predicted Outcome:** Matched prediction exactly. Pure-Rust `louder-esp32-app` (`d445a736`, `678,176` bytes) maintains active keepalive/replay_gain frame exchanges, preventing LMS slimproto disconnect, and remains persistently connected in LMS `serverstatus` and player `status` across the entire 35-second soak window. `/dev/ttyUSB0` was untouched.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_112900_wo040_certifier.md`
- **Handoff Path:** [WO-040-flash-persistent-lms-and-verify-audio.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-040-flash-persistent-lms-and-verify-audio.md)
