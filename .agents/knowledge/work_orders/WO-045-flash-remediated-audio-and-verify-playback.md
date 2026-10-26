# Work Order: WO-045 — Flash Remediated Pure-Rust OTA Binary (`1827d291`) to `172.22.1.15` & Verify Live Continuous Audio Playback

**Work Order ID:** WO-045  
**Timestamp:** 2026-10-24T13:41:30-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Endpoint:** `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`) & LMS `http://10.0.0.10:9000/jsonrpc.js`  
**Staged Binary:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`685,136` bytes, SHA-256 `e58c7e9ca8249e68555566f6c37d7ff01d077227a456eb2c3955f45891ba5005`)  
**Task Goal:** Transition `172.22.1.15` cleanly from pure-Rust `ota_0` to Factory `recovery` (`0x10000`) via `POST http://172.22.1.15/recovery.json`, verify `"recovery": 1`, stream the `685,136`-byte remediated binary (`1827d291`) to `ota_0` via `POST http://172.22.1.15/flash.json`, verify single-boot trial mode (`"version": "I2S-4MFlash-Rust"`, `"otadata_prearmed_factory": true`), trigger playback on `"BASEMENT"` via LMS JSON-RPC, and verify via multi-sample `GET http://172.22.1.15/status.json` read-backs that `"frames_played"` advances continuously well past `512` (`~44,100` frames/sec) with zero I2C thrashing.

---

## 1. Context & Constraints
- **User Authorization:** Explicitly granted for OTA flash to `172.22.1.15` (`ota_0`) and live LMS playback verification.
- **Prohibited Actions (Rule 3):** NEVER touch or query `/dev/ttyUSB0`. NEVER call `POST /reboot_ota.json` (keep `otadata` erased to `0xFF` in Single-Boot Trial Mode so Factory `recovery` `0x10000` remains pre-armed!).

---

## 2. Step-by-Step Execution & Verification Instructions
1. **Verify Staged Binary Integrity**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` and confirm SHA-256 `e58c7e9ca8249e68555566f6c37d7ff01d077227a456eb2c3955f45891ba5005` (`685,136` bytes).
2. **Step 1 — Transition `172.22.1.15` to Factory `recovery` (`0x10000`)**:
   - Check `GET http://172.22.1.15/status.json`. If `"recovery": 0`, send `POST http://172.22.1.15/recovery.json` and poll `GET http://172.22.1.15/status.json` until `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).
3. **Step 2 — Flash Remediated Pure-Rust Binary to `ota_0`**:
   - Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via:
     `/usr/bin/curl -i -sS --max-time 60 -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`
   - Poll `GET http://172.22.1.15/status.json` until `172.22.1.15` boots `louder-esp32-app` (`"version": "I2S-4MFlash-Rust"`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"amp_mode": "Play"`).
4. **Step 3 — Trigger Live Audio Playback on LMS (`10.0.0.10:9000`) & Verify Continuous Frame Progression**:
   - Confirm `"BASEMENT"` (`c8:2e:18:be:b8:34`) is connected in LMS (`POST http://10.0.0.10:9000/jsonrpc.js` with `{"id":1,"method":"slim.request","params":["c8:2e:18:be:b8:34",["status","-",1,"tags:uB,a,c,d,e,l,t"]]}`).
   - Set volume to a comfortable audible level (e.g. `["c8:2e:18:be:b8:34", ["mixer", "volume", "65"]]`) and send `["c8:2e:18:be:b8:34", ["play"]]` (or `["playlist", "index", "0"]`).
   - Sample `GET http://172.22.1.15/status.json` at `t = +2s`, `t = +5s`, `t = +8s`, and `t = +12s` and record:
     - `"frames_played"` at each sample (confirming it advances far past `512`!)
     - `"amp_mode"` (`"Play"`)
     - `"i2c_acks"` and `"i2c_nacks"` (confirming `"i2c_nacks": 0` and `"i2c_acks"` is NOT thrashing at thousands of writes/sec!)
   - Also capture the LMS player status JSON (`mode`, `time`, `rate`, `current_title`) during active playback.
5. Complete the Fulfillment Section below and write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_134500_wo045_live_audio_verify.md`.

---

## 3. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T13:46:50-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo045_playback.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/wo045_playback_samples.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_134500_wo045_live_audio_verify.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-045-flash-remediated-audio-and-verify-playback.md` (modified)
- **Test / Verification Command & Results:**
  - Binary verified: `685,136` bytes, SHA-256 `e58c7e9ca8249e68555566f6c37d7ff01d077227a456eb2c3955f45891ba5005`.
  - Transition to Factory recovery confirmed (`version: I2S-4MFlash-1681`, `recovery: 1`).
  - `POST http://172.22.1.15/flash.json` flashed `ota_0`. Pure-Rust booted into `ota_0` at 10.0s (`version: "I2S-4MFlash-Rust"`, `rust_engine: true`, `otadata_prearmed_factory: true`, `amp_mode: "Play"`, `i2c_acks: 15`).
  - LMS player `c8:2e:18:be:b8:34` registered, volume set to 65, play command issued for `"Ibiza Global Radio MP3"`.
  - At `t = +2s`: `frames_played: 0`, `amp_mode: "Play"`, `i2c_acks: 15`, `i2c_nacks: 0`.
  - At `t = +5s`: `frames_played: 0`, `amp_mode: "Play"`, `i2c_acks: 15`, `i2c_nacks: 0`.
  - At `t = +8s`: Connection timed out (device crash / reboot).
  - At `t = +12s`: Device responded from Factory recovery partition (`recovery: 1`, `version: "I2S-4MFlash-1681"`).
- **Observed Outcome vs Predicted Outcome:** I2C thrashing was completely resolved (`i2c_acks` remained at 15), but playback failed: `frames_played` remained 0, and the application crashed upon receiving the LMS MP3 audio stream between `t = +5s` and `t = +8s`. Gate 1 Single-Boot Trial Mode (`otadata_prearmed_factory: true`) cleanly prevented a brick by automatically rebooting the device into Factory recovery partition `0x10000`. `/dev/ttyUSB0` was untouched.
- **Fulfillment Status:** `[BLOCKED / ERROR]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_134500_wo045_live_audio_verify.md`
- **Handoff Path:** [WO-045-flash-remediated-audio-and-verify-playback.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-045-flash-remediated-audio-and-verify-playback.md)
