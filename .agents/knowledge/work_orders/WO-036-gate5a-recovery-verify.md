# Work Order: WO-036 — Execute Gate 5A Only (`POST http://172.22.1.15/recovery.json` & Verify `recovery: 1`)

**Work Order ID:** WO-036  
**Timestamp:** 2026-10-24T08:43:30-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** Live Network Endpoint `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5a.json`  
**Defect IDs:** Task 5 Gate 5A — Pre-Upload Factory Recovery Partition (`0x10000`) Boot & Wi-Fi Verification  
**Task Goal:** Verify live pre-state of `172.22.1.15` (`GET /status.json`), trigger transition into the untouched Factory `recovery` partition (`0x10000`) via `POST http://172.22.1.15/recovery.json`, and verify via independent read-back (`GET http://172.22.1.15/status.json`) that `172.22.1.15` reboots, reconnects to SSID `"iot"`, and reports `"recovery": 1` (WITHOUT uploading any firmware binary).  

---

## 1. Context & Scope
- **Problem Statement:** The user authorized **Gate 5A Only First**: transition `172.22.1.15` (`"BASEMENT"`) from its running `ota_0` C app into its factory `recovery` partition (`0x10000`) and empirically confirm that `recovery` (`0x10000`) boots, associates to SSID `"iot"`, acquires `172.22.1.15`, and serves `"recovery": 1` on `GET /status.json` before a single byte of `ota_0` is flashed.
- **Single Variable Constraint (Rule 5):** Execute ONLY Gate 5A (`GET /status.json` -> `POST /recovery.json` -> poll `GET /status.json` until `"recovery": 1`). Do **NOT** execute Gate 5B (`POST /flash.json`).
- **Pre-Modification Integrity (Rule 31, Rule 35):** Save pre-transition `GET http://172.22.1.15/status.json` output to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5a.json`.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never call `/flash.json` in this work order. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 4, Rule 21, Rule 52):** Confirm via independent read-back (`GET http://172.22.1.15/status.json`) that `"recovery": 1` is active on `172.22.1.15`.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Sending `POST http://172.22.1.15/recovery.json` sets the ESP32 boot partition to `factory` (`0x10000`, `recovery`) and reboots `172.22.1.15` into the factory recovery application, which connects to `"iot"` and serves `GET /status.json` with `"recovery": 1`.
- **Predicted Outcome:** Within 8–20 seconds of `POST http://172.22.1.15/recovery.json`, `GET http://172.22.1.15/status.json` returns HTTP 200 JSON containing `"recovery":1`, `"project_name":"Squeezelite-ESP32"`, `"ip":"172.22.1.15"`, `"ssid":"iot"`.
- **Falsification Criteria:** `GET http://172.22.1.15/status.json` continues to show `"recovery":0` or fails to respond after the reboot window.

---

## 3. Detailed Instructions
1. Query live pre-state:
   `/usr/bin/curl -sS --max-time 5 http://172.22.1.15/status.json`
   Write the response to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5a.json` using `write_file`.
2. Trigger reboot to factory recovery (`0x10000`):
   `/usr/bin/curl -sS -i --max-time 10 -X POST http://172.22.1.15/recovery.json`
3. Wait and verify read-back (`GET http://172.22.1.15/status.json`) using a short Python verification script `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5a_recovery.py` that polls `http://172.22.1.15/status.json` every 2 seconds for up to 30 seconds until `"recovery": 1` is observed, and saves the post-state JSON to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5a.json`.
4. Write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_084330_gate5a_certifier.md` and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T08:45:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5a.json` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5a_recovery.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_post_gate5a.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_084330_gate5a_certifier.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-036-gate5a-recovery-verify.md` (modified)
- **Backup Verification:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/status_pre_gate5a.json` (captured pre-transition `recovery: 0`)
- **Test / Verification Command & Results:**
  - `POST http://172.22.1.15/recovery.json` returned HTTP 200 `{}`.
  - Verification script `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_gate5a_recovery.py` confirmed `recovery == 1` after 5.0 seconds.
  - Direct read-back `/usr/bin/curl -sS --max-time 5 http://172.22.1.15/status.json` returned:
    `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-1681","recovery":1,"Jack":false,"Voltage":0,"disconnect_count":0,"avg_conn_time":0,"bt_status":0,"bt_sub_status":0,"is_i2c_locked":false,"urc":0,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-69}`
- **Observed Outcome vs Predicted Outcome:** Matched prediction exactly. `172.22.1.15` rebooted into factory recovery partition `0x10000`, re-associated to SSID `"iot"`, re-acquired `172.22.1.15`, and is serving `recovery: 1`. Gate 5B (`POST /flash.json`) was NOT executed. `/dev/ttyUSB0` was untouched.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_084330_gate5a_certifier.md`
- **Handoff Path:** [WO-036-gate5a-recovery-verify.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-036-gate5a-recovery-verify.md)
