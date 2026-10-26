# Work Order: WO-047 — Flash PCM-Remediated Pure-Rust OTA Binary (`7248a3c5`) to `172.22.1.15` & Verify Live Continuous Audio Playback

**Work Order ID:** WO-047  
**Timestamp:** 2026-10-24T14:06:00-04:00  
**Assigned Subagent Role:** Extension Certifier (`Model: "flash"`, `TypeName: "self"`)  
**Target Endpoint:** `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`) & LMS `http://10.0.0.10:9000/jsonrpc.js`  
**Staged Binary:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`683,712` bytes, SHA-256 `6dc2e22504ceeb4a39eec5847967bb31890abd840c73ce63bceeffeafa290b38`)  
**Task Goal:** Confirm `172.22.1.15` is in Factory `recovery` (`0x10000`, `"recovery": 1`), stream the `683,712`-byte binary (`7248a3c5`) to `ota_0` via `POST http://172.22.1.15/flash.json`, verify single-boot trial mode (`"version": "I2S-4MFlash-Rust"`, `"otadata_prearmed_factory": true`), trigger playback on `"BASEMENT"` via LMS JSON-RPC, and sample `GET http://172.22.1.15/status.json` across `t = +2s, +5s, +8s, +12s, +16s, +20s` to verify `"frames_played"` advances continuously well past `512` (`~44,100` frames/sec) with zero I2C thrashing.

---

## 1. Context & Constraints
- **User Authorization:** Explicitly granted for OTA flash to `172.22.1.15` (`ota_0`) and live LMS playback verification.
- **Prohibited Actions (Rule 3):** NEVER touch or query `/dev/ttyUSB0`. NEVER call `POST /reboot_ota.json` (keep `otadata` erased to `0xFF` in Single-Boot Trial Mode so Factory `recovery` `0x10000` remains pre-armed!).

---

## 2. Step-by-Step Execution & Verification Instructions
1. **Verify Staged Binary Integrity**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` and confirm SHA-256 `6dc2e22504ceeb4a39eec5847967bb31890abd840c73ce63bceeffeafa290b38` (`683,712` bytes).
2. **Step 1 — Confirm `172.22.1.15` is in Factory `recovery` (`0x10000`)**:
   - Check `GET http://172.22.1.15/status.json` and confirm `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).
3. **Step 2 — Flash Remediated Pure-Rust Binary to `ota_0`**:
   - Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via:
     `/usr/bin/curl -i -sS --max-time 60 -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`
   - Poll `GET http://172.22.1.15/status.json` until `172.22.1.15` boots `louder-esp32-app` (`"version": "I2S-4MFlash-Rust"`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"amp_mode": "Play"`).
4. **Step 3 — Trigger Live Audio Playback on LMS (`10.0.0.10:9000`) & Verify Continuous Frame Progression**:
   - Confirm `"BASEMENT"` (`c8:2e:18:be:b8:34`) is connected in LMS (`POST http://10.0.0.10:9000/jsonrpc.js` with `{"id":1,"method":"slim.request","params":["c8:2e:18:be:b8:34",["status","-",1,"tags:uB,a,c,d,e,l,t"]]}`).
   - Note: Also check if there are any local library tracks on LMS (`{"id":1,"method":"slim.request","params":["",["titles",0,5,"tags:u,t,l"]]}`); whether playing a local track or `"Ibiza Global Radio"`, send `["c8:2e:18:be:b8:34", ["mixer", "volume", "65"]]` and `["c8:2e:18:be:b8:34", ["play"]]`.
   - Sample `GET http://172.22.1.15/status.json` at `t = +2s, +5s, +8s, +12s, +16s, +20s` and record:
     - `"frames_played"` at each sample (confirming it advances continuously past `512`!)
     - `"amp_mode"` (`"Play"`)
     - `"i2c_acks"` and `"i2c_nacks"`
   - Also capture the LMS player status JSON (`mode`, `time`, `rate`, `current_title`) at `t = +20s`.
5. Complete the Fulfillment Section below and write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_140800_wo047_live_audio_verify.md`.

---

## 3. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-24T14:14:50-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo047_playback.py` (created)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/wo047_playback_samples.json` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_140800_wo047_live_audio_verify.md` (created)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-047-flash-pcm-remediated-and-verify-playback.md` (modified)
- **Test / Verification Command & Results:**
  - Binary verified: `683,712` bytes, SHA-256 `6dc2e22504ceeb4a39eec5847967bb31890abd840c73ce63bceeffeafa290b38`.
  - Device confirmed in Factory recovery partition (`version: I2S-4MFlash-1681`, `recovery: 1`).
  - `POST http://172.22.1.15/flash.json` flashed `ota_0`. Pure-Rust booted into `ota_0` at 12.2s (`version: "I2S-4MFlash-Rust"`, `rust_engine: true`, `otadata_prearmed_factory: true`, `amp_mode: "Play"`, `i2c_acks: 15`).
  - LMS player `c8:2e:18:be:b8:34` registered, volume set to 65, play command issued.
  - Samples across `t = +2s, +5s, +8s, +12s, +16s, +20s`:
    - `frames_played: 0` at all 6 samples.
    - `amp_mode: "Play"` at all 6 samples.
    - `i2c_acks: 15` at all 6 samples (zero I2C thrashing).
    - `recovery: 0` at all 6 samples (no crash / no panic).
    - LMS status returned `mode: "stop"` at `t = +20s`.
- **Observed Outcome vs Predicted Outcome:** Significant progress: The panic/watchdog crash observed in WO-045 was completely eliminated by removing `Mp3Decoder`, and I2C thrashing remained at 0 writes. The device survived the entire test and remains online in pure-Rust `ota_0` (`uptime_ms > 105s`). However, `frames_played` did not advance past 0 because LMS immediately reverted to `mode: "stop"` upon receiving the play command. Single-boot trial mode (`otadata_prearmed_factory: true`) remains safely armed. `/dev/ttyUSB0` was untouched.
- **Fulfillment Status:** `[BLOCKED / ERROR]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024_140800_wo047_live_audio_verify.md`
- **Handoff Path:** [WO-047-flash-pcm-remediated-and-verify-playback.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-047-flash-pcm-remediated-and-verify-playback.md)
