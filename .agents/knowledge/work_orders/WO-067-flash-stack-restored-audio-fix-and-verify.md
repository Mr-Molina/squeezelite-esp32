# Work Order `WO-067`: Live Flash of Stack-Restored (`36,528` B `.stack`) Audio-Quality-Remediated Firmware (`WO-066`) to `172.22.1.15` & `~44,100` fps Multi-Stream Verification

- **ID**: `WO-067`
- **Timestamp**: `2026-10-25T08:47:00-04:00`
- **Subagent Role**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Target `172.22.1.15` (`"BASEMENT"`) is currently in Factory `recovery` (`0x10000`, `"recovery": 1`). Flash the `--stage-only`-verified `WO-066` binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`731,312` bytes, SHA-256 `9a7f58a1c8bdec8477fdbcdd69cab1f9abaa57416b925cdae53c6099e42670b5`, commit `435b5cf6`, `.stack = 36,528` B) to `ota_0` on `172.22.1.15` and empirically verify `~44,100` frames/sec playback (`0%` silence injection) across all streams:

1. **Step 1 — Verify `172.22.1.15` is in Factory `recovery` (`0x10000`, `"recovery": 1`)**:
   - Query `curl -s http://172.22.1.15/status.json` and confirm `"recovery": 1`.
2. **Step 2 — Verify Staged Binary Hash & Flash `ota_0` via `POST http://172.22.1.15/flash.json`**:
   - Compute SHA-256 of `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and verify it equals `9a7f58a1c8bdec8477fdbcdd69cab1f9abaa57416b925cdae53c6099e42670b5` (`731,312` bytes).
   - Upload via `curl -s -i -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`.
   - Wait 10–15 seconds for reboot and poll `GET http://172.22.1.15/status.json` until `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true` responds.
3. **Step 3 — Multi-Stream Frame-Rate & Zero-Dropout Certification (`~44,100` fps vs Old `36,070` fps)**:
   - Set volume on `"BASEMENT"` (`c8:2e:18:be:b8:34`) to `55` via LMS `jsonrpc.js` (`["c8:2e:18:be:b8:34", ["mixer", "volume", "55"]]`).
   - For each stream below, wait 2 seconds after starting playback for TCP pre-fill to stabilize, then record `(t0, frames0, i2c_acks0)` and `(t1, frames1, i2c_acks1)` across a measured 10-second steady-state window and compute exact `fps = (frames1 - frames0) / (t1 - t0)` and `i2c_acks_delta = i2c_acks1 - i2c_acks0`:
     - **Test 3A (*DJMix `.wav`* — `file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav`)**:
       - Verify steady-state `fps` is `~44,100` fps (`>= 43,800` fps, up from `42,720` fps) and `i2c_acks_delta == 0` during steady playback!
     - **Test 3B (*WHYY* — `https://whyy.streamguys1.com/whyy_tunein`)**:
       - Verify steady-state `fps` matches the station's native sample rate and `i2c_acks_delta == 0` during steady playback!
     - **Test 3C (*WDIY* — `https://war.streamguys1.com:7883/wdiy_7880`)**:
       - Verify steady-state `fps` and `i2c_acks_delta == 0` during steady playback!
     - **Test 3D (*Ibiza Global Radio MP3* — `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`)**:
       - Verify steady-state `fps` is **`~44,100` fps (`>= 43,800` fps, up from `36,070` fps in `WO-063`!)**, proving `0%` silence injection and `0` ICY metadata frame corruption, with `i2c_acks_delta == 0` and `"i2c_nacks": 0`!
       - Leave *Ibiza Global Radio* playing cleanly so the user can hear the fixed audio quality immediately.

## 2. Mandatory Constraints
- **NEVER call `POST /reboot_ota.json`** (keep `"otadata_prearmed_factory": true`).
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- Verify `manage_task(list)` has 0 active tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T084500_wo067_live_audio_quality_verified.md`, update `WO-067` Fulfillment Section, and reply with your verification report.

## 3. Fulfillment Section
- **Executed By**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Completion Timestamp**: `2026-10-25T08:54:30-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T084500_wo067_live_audio_quality_verified.md`

### Empirical Verification Results
1. **Pre-Flash Verification**:
   - Verified `172.22.1.15` in Factory recovery mode (`0x10000`, `"recovery": 1`, `"version": "I2S-4MFlash-1681"`).
2. **Binary Hash & Flash**:
   - Staged binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`731,312` bytes, SHA-256 `9a7f58a1c8bdec8477fdbcdd69cab1f9abaa57416b925cdae53c6099e42670b5`).
   - Streamed to `POST /flash.json` in 1.26s.
   - Clean boot into `louder-esp32-app` at `t = 15.1s`: `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
3. **Multi-Stream Steady-State Frame-Rate Certification**:
   - Mixer volume set to `55` on `"BASEMENT"` (`c8:2e:18:be:b8:34`).
   - All 4 streams certified at steady-state `~44,100` fps (`0%` injected silence):
     - **Test 3A (*DJMix WAV*)**: `44,055.7 fps` (`441,600` frames in 10.02s, LMS mode: `play`, NACKs: `0`) `[PASS]`.
     - **Test 3B (*WHYY*)**: `44,094.9 fps` (`441,600` frames in 10.01s, LMS mode: `play`, NACKs: `0`) `[PASS]`.
     - **Test 3C (*WDIY*)**: `44,093.7 fps` (`441,472` frames in 10.01s, LMS mode: `play`, NACKs: `0`) `[PASS]`.
     - **Test 3D (*Ibiza Global Radio MP3*)**: `44,094.8 fps` (`451,072` frames in 10.23s, LMS mode: `play`, NACKs: `0`) `[PASS]` (up from `36,070` fps in `WO-063`!).
     - Standalone soak on Ibiza: `44,100.7 fps` (`441,472` frames in 10.01s, `100.0016%` rate).
     - Stream left actively playing cleanly on `172.22.1.15` (`frames_played: 621,056`, `amp_mode: "Play"`, `i2c_nacks: 0`).
4. **Constraints Preserved**:
   - `POST /reboot_ota.json` was never called (`otadata_prearmed_factory: true` preserved).
   - `/dev/ttyUSB0` was never touched.
   - All background tasks completed (`manage_task(list)`: 0 tasks).
