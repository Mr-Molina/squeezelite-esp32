# Work Order `WO-065`: Live Flash of Audio-Quality-Remediated Firmware (`WO-064`) to `172.22.1.15` & Bit-Accurate `~44,100` fps Multi-Stream Verification

- **ID**: `WO-065`
- **Timestamp**: `2026-10-25T08:32:30-04:00`
- **Subagent Role**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[BLOCKED / ERROR]`

## 1. Objective & Scope
Flash the `--stage-only`-verified `WO-064` binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`731,328` bytes, SHA-256 `424de9fb7c9498dbb8d288e1a2a45ef3da87c17445d72ab32a7864a828f5a011`, commit `92e10556`) to `ota_0` on `172.22.1.15` (`"BASEMENT"`) and empirically verify that eliminating mid-waveform silence injection (`write_dma` / `pump_silence_if_needed`), DMA-first scheduling (`12 KB` DMA + `12 KB` `output_storage` + `1,024`-B writes), ICY `cont` phase preservation, and `0.5 dB`/step `0x4C` hardware volume restores crystal-clear `~44,100` frames/sec playback (`0%` silence injection) across all streams:

1. **Step 1 — Transition `172.22.1.15` to Factory `recovery` (`0x10000`)**:
   - Send `curl -s -i -X POST http://172.22.1.15/recovery.json` and poll `GET http://172.22.1.15/status.json` until `"recovery": 1` (`"version": "I2S-4MFlash-1681"`) responds.
2. **Step 2 — Verify Staged Binary Hash & Flash `ota_0` via `POST http://172.22.1.15/flash.json`**:
   - Compute SHA-256 of `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and verify it equals `424de9fb7c9498dbb8d288e1a2a45ef3da87c17445d72ab32a7864a828f5a011` (`731,328` bytes).
   - Upload via `curl -s -i -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`.
   - Wait 10–15 seconds for reboot and poll `GET http://172.22.1.15/status.json` until `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true` responds.
3. **Step 3 — Multi-Stream Frame-Rate & Zero-Dropout Certification (`~44,100` fps vs Old `36,070` fps)**:
   - Set volume on `"BASEMENT"` (`c8:2e:18:be:b8:34`) to `55` via LMS `jsonrpc.js` (`["c8:2e:18:be:b8:34", ["mixer", "volume", "55"]]`).
   - For each stream below, wait 2 seconds after starting playback for TCP pre-fill to stabilize, then record `(t0, frames0, i2c_acks0)` and `(t1, frames1, i2c_acks1)` across a measured 10-second steady-state window and compute exact `fps = (frames1 - frames0) / (t1 - t0)` and `i2c_acks_delta = i2c_acks1 - i2c_acks0`:
     - **Test 3A (*DJMix `.wav`* — `file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav`)**:
       - Verify steady-state `fps` is `~44,100` fps (`>= 43,800` fps, up from `42,720` fps) and `i2c_acks_delta == 0` during steady playback!
     - **Test 3B (*WHYY* — `https://whyy.streamguys1.com/whyy_tunein`)**:
       - Verify steady-state `fps` matches the station's native sample rate (`~44,100` or `~24,000`/`48,000` fps with `0` silence injection) and `i2c_acks_delta == 0` during steady playback!
     - **Test 3C (*WDIY* — `https://war.streamguys1.com:7883/wdiy_7880`)**:
       - Verify steady-state `fps` and `i2c_acks_delta == 0` during steady playback!
     - **Test 3D (*Ibiza Global Radio MP3* — `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`)**:
       - Verify steady-state `fps` is **`~44,100` fps (`>= 43,800` fps, up from `36,070` fps in `WO-063`!)**, proving `0%` silence injection and `0` ICY metadata frame corruption, with `i2c_acks_delta == 0` and `"i2c_nacks": 0`!
       - Leave *Ibiza Global Radio* playing cleanly so the user can hear the fixed audio quality immediately.

## 2. Mandatory Constraints
- **NEVER call `POST /reboot_ota.json`** (keep `"otadata_prearmed_factory": true`).
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- Verify `manage_task(list)` has 0 active tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T082500_wo065_live_audio_quality_verification.md`, update `WO-065` Fulfillment Section, and reply with your verification report.

## 3. Fulfillment Section
- **Executed By**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Completion Timestamp**: `2026-10-25T08:37:30-04:00`
- **Status**: `[BLOCKED / ERROR]`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T082500_wo065_live_audio_quality_verification.md`

### Empirical Verification Results
1. **Transition to Factory Recovery**:
   - Sent `POST http://172.22.1.15/recovery.json` -> rebooted in 4.7s into Factory recovery (`recovery: 1`, `version: "I2S-4MFlash-1681"`).
2. **Flash & Boot Failure Root Cause**:
   - Verified binary SHA-256 (`424de9fb7c9498dbb8d288e1a2a45ef3da87c17445d72ab32a7864a828f5a011`, `731,328` bytes).
   - Uploaded via `POST http://172.22.1.15/flash.json` in 1.40s.
   - Upon reboot into `ota_0`, `louder-esp32-app` crashed during early boot due to DRAM `.stack` overflow into `.bss`.
   - Gate 1 Anti-Brick Protection (`otadata_prearmed_factory: true`) caught the failure and safely rolled back to Factory recovery `0x10000` (`recovery: 1`).
3. **Memory Map Root Cause**:
   - `readelf -S` confirms available `.stack` shrunk to `28,288` bytes (`0x3FFE0000 - 0x3FFD9180`) because `ring_storage` expanded to `20,480` B (+4,096 B in `.bss`).
   - Simultaneously, `dma_tx_stream_buffer!(12288, 1024)` expanded local stack consumption in `main()` by +4,096 bytes. Combined stack deficit: 8,192 bytes.
4. **Prescription**:
   - Revert `ring_storage` to `16,384` B and `tx_buffer` to `dma_tx_stream_buffer!(8192, 1024)` in `main.rs`, recovering +8,192 B of stack headroom.
5. **Constraints**:
   - Target is safe in Factory recovery mode (`recovery: 1`).
   - `POST /reboot_ota.json` was never called; `/dev/ttyUSB0` was never touched; `manage_task(list)` confirmed 0 active child tasks.
