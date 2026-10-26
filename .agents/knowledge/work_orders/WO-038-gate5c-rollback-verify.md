# Work Order: WO-038 — Execute Gate 5C (Live Rust-to-Factory-Recovery Rollback Verification via `POST /recovery.json`)

**Work Order ID:** WO-038  
**Timestamp:** 2026-10-24T10:58:15-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** Live Network Endpoint `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5b.json`  
**Defect IDs:** Task 5 Gate 5C — Verify Live Anti-Brick Rollback from Pure-Rust (`ota_0` at `0x160000`) to Factory Recovery (`0x10000`, `"recovery": 1`)  
**Task Goal:** While `172.22.1.15` is running pure-Rust `louder-esp32-app` (`"version":"I2S-4MFlash-Rust"`, `"recovery":0`, `"otadata_prearmed_factory":true`), send `POST http://172.22.1.15/recovery.json` and verify via independent read-back (`GET http://172.22.1.15/status.json`) that `172.22.1.15` reboots cleanly into the untouched Factory `recovery` partition (`0x10000`) with `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).  

---

## 1. Context & Scope
- **Problem Statement:** Empirically verify Anti-Brick Gate 1 (`otadata` pre-erased to `0xFF`) + Gate 4 (Port 80 HTTP `/recovery.json` handler) by triggering a live rollback from pure-Rust `louder-esp32-app` (`ota_0` at `0x160000`) back into the Factory `recovery` partition (`0x10000`).
- **Single Variable Constraint (Rule 5):** Execute ONLY Gate 5C (`POST /recovery.json` + read-back verification of `"recovery": 1`). Do not flash or reboot back to `ota_0` unless subsequently instructed.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Capture pre-rollback `/status.json` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5c.json` and confirm `"version": "I2S-4MFlash-Rust"`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 4, Rule 21, Rule 52):** Poll `GET http://172.22.1.15/status.json` until `"recovery": 1` is returned, save post-rollback state to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5c.json`, and run an independent `curl` read-back.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Sending `POST http://172.22.1.15/recovery.json` to `louder-esp32-app` invokes `handle_http_control_request()`, returns HTTP 200 JSON (`{"status":"rebooting_to_recovery","target_partition":"factory_0x10000"}`), schedules `HttpControlAction::RebootToRecovery` (`+250 ms`), and executes `esp_hal::system::software_reset()`. Because Gate 1 erased `otadata` (`0xD000..0xF000`) to `0xFF` at boot, the ESP-IDF second-stage bootloader selects the Factory `recovery` partition (`0x10000`).
- **Predicted Outcome:** Within ~5–15 seconds, `GET http://172.22.1.15/status.json` returns `"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"ip": "172.22.1.15"`, `"ssid": "iot"`.
- **Falsification Criteria:** If `GET http://172.22.1.15/status.json` still returns `"version": "I2S-4MFlash-Rust"` after 20 seconds, then `POST /recovery.json` did not trigger a reset or `otadata` was not erased to `0xFF`.

---

## 3. Detailed Instructions
1. Capture pre-rollback state via `/usr/bin/curl -sS --max-time 5 http://172.22.1.15/status.json` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5c.json`.
2. Send `/usr/bin/curl -sS -i --max-time 10 -X POST http://172.22.1.15/recovery.json` and log the HTTP response.
3. Run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5c_rollback.py` to poll `GET http://172.22.1.15/status.json` every 2 seconds (up to 35 seconds) until `"recovery": 1` is returned, saving the JSON to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5c.json`.
4. Write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_105830_gate5c_certifier.md` and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T11:00:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5c.json` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5c_rollback.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5c.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_105830_gate5c_certifier.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-038-gate5c-rollback-verify.md` (modified)
- **Backup Verification:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5b.json`
- **Test / Verification Command & Results:**
  - Pre-rollback status confirmed `version: "I2S-4MFlash-Rust"`, `recovery: 0`, `otadata_prearmed_factory: true`, `uptime_ms: 6994382`.
  - `POST http://172.22.1.15/recovery.json` returned HTTP 200 OK:
    `{"status":"rebooting_to_recovery","target_partition":"factory_0x10000"}`
  - Polling verification script `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5c_rollback.py` verified recovery partition boot:
    `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-1681","recovery":1,"Jack":false,"Voltage":0,"disconnect_count":0,"avg_conn_time":0,"bt_status":0,"bt_sub_status":0,"is_i2c_locked":false,"urc":0,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-50}`
  - Independent read-back `/usr/bin/curl -sS --max-time 5 http://172.22.1.15/status.json` confirmed `recovery: 1`, `version: "I2S-4MFlash-1681"`.
- **Observed Outcome vs Predicted Outcome:** Matched prediction exactly. Pure-Rust `louder-esp32-app` received `POST /recovery.json`, issued HTTP 200 confirmation, reset the CPU, and the bootloader selected factory recovery partition `0x10000` because `otadata` was pre-erased to `0xFF` (Gate 1). The device reconnected to SSID `"iot"`, re-acquired `172.22.1.15`, and is serving `recovery: 1`. `/dev/ttyUSB0` was untouched.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_105830_gate5c_certifier.md`
- **Handoff Path:** [WO-038-gate5c-rollback-verify.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-038-gate5c-rollback-verify.md)
