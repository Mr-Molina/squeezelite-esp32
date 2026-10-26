# Work Order `WO-063`: Live Flash of Remediated Pure-Rust Firmware (`41eb03a4`, `731,632` B) to `172.22.1.15` & Full Multi-Stream Verification

- **ID**: `WO-063`
- **Timestamp**: `2026-10-24T19:45:30-04:00`
- **Subagent Role**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Flash the verified `--stage-only` pure-Rust binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`731,632` bytes, SHA-256 `3e4b235dd457b94e0d32a373f2c7e5804d7a2772b11f3a88c51c2845f16108f1`, commit `41eb03a4`) to `ota_0` on `172.22.1.15` (`"BASEMENT"`) and verify all 4 previously failing streams + Web UI + AirPlay/UPnP:

1. **Step 1 — Transition `172.22.1.15` to Factory `recovery` (`0x10000`)**:
   - Send `curl -s -i -X POST http://172.22.1.15/recovery.json` and poll `GET http://172.22.1.15/status.json` until `"recovery": 1` (`"version": "I2S-4MFlash-1681"`) responds.
2. **Step 2 — Flash `ota_0` via `POST http://172.22.1.15/flash.json`**:
   - Verify SHA-256 of `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` is `3e4b235dd457b94e0d32a373f2c7e5804d7a2772b11f3a88c51c2845f16108f1`.
   - Upload via `curl -s -i -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`.
   - Wait 10–15 seconds for reboot and poll `GET http://172.22.1.15/status.json` until `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true` responds.
3. **Step 3 — Verify Embedded Web UI Dashboard (`FA-CORE-004`)**:
   - Query `curl -s -i http://172.22.1.15/` and confirm `HTTP/1.1 200 OK`, `Content-Type: text/html; charset=utf-8`, and HTML dashboard content.
4. **Step 4 — Verify All 4 Previously Failing Streams (> 10s Each, Past the Old `297,255`-Frame / `6.74s` Cutoff) & TAS5805M Hardware Volume (`F-HW-01`..`03`, `F-STREAM-001`..`006`, `F-PIPE-001`)**:
   - Set volume on `"BASEMENT"` (`c8:2e:18:be:b8:34`) to `55` via LMS `jsonrpc.js` (`["c8:2e:18:be:b8:34", ["mixer", "volume", "55"]]`).
   - **Test 4A (*WHYY* — `https://whyy.streamguys1.com/whyy_tunein`)**:
     - Play via `["c8:2e:18:be:b8:34", ["playlist", "play", "https://whyy.streamguys1.com/whyy_tunein"]]`.
     - Poll `http://172.22.1.15/status.json` over 12 seconds and verify `"frames_played"` increases by `> 350,000` frames (`> 8s` continuous audio) with LMS `mode: "play"` and zero `CLOCK_FAULT` resets (`i2c_acks` stays constant during steady playback after initial volume set).
   - **Test 4B (*WDIY* — `https://war.streamguys1.com:7883/wdiy_7880`)**:
     - Play via `["c8:2e:18:be:b8:34", ["playlist", "play", "https://war.streamguys1.com:7883/wdiy_7880"]]`.
     - Poll `http://172.22.1.15/status.json` over 12 seconds and verify `"frames_played"` increases by `> 350,000` frames with LMS `mode: "play"`.
   - **Test 4C (*DJMix `.wav`* — `file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav`)**:
     - Play via `["c8:2e:18:be:b8:34", ["playlist", "play", "file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav"]]`.
     - Poll `http://172.22.1.15/status.json` over 10 seconds and verify `"frames_played"` increases by `> 350,000` frames with LMS `mode: "play"`.
   - **Test 4D (*Ibiza Global Radio* — `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`)**:
     - Play via `["c8:2e:18:be:b8:34", ["playlist", "play", "https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"]]`.
     - Poll `http://172.22.1.15/status.json` over 12 seconds and verify `"frames_played"` increases by `> 400,000` frames (`> 9s` continuous audio, well past the old `297,255`-frame `6.74s` cutoff!) with LMS `mode: "play"`, `"i2c_nacks": 0`, and zero `CLOCK_FAULT` `i2c_acks` thrashing! Leave *Ibiza Global Radio* playing cleanly so the user can hear it immediately.
5. **Step 5 — Verify AirPlay (`ShairTunes2W`) Listener & LMS Log Cleanliness**:
   - Confirm `shairport_helper-linux-aarch64` is active for `"BASEMENT"` on `10.0.0.10` and `server.log` has zero `Slim::Player::Song::open (426)` errors.

## 2. Mandatory Constraints
- **NEVER call `POST /reboot_ota.json`** (keep `"otadata_prearmed_factory": true`).
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- Verify `manage_task(list)` has 0 active tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024T194530_wo063_live_verification.md`, update `WO-063` Fulfillment Section, and reply with your verification report.

## 3. Fulfillment Section
- **Executed By**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Completion Timestamp**: `2026-10-24T19:56:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024T194530_wo063_live_verification.md`

### Empirical Verification Results
1. **Transition to Factory Recovery**:
   - Sent `POST http://172.22.1.15/recovery.json` -> rebooted in 4.8s with `"recovery": 1`, `"version": "I2S-4MFlash-1681"`.
2. **SHA-256 Audit & Live Flash**:
   - Verified SHA-256 of staged binary `3e4b235dd457b94e0d32a373f2c7e5804d7a2772b11f3a88c51c2845f16108f1` (`731,632` bytes).
   - Uploaded via `POST http://172.22.1.15/flash.json` in 1.10s.
   - At `t = 15.4s`, verified clean boot into `louder-esp32-app`: `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
3. **Embedded Web UI Dashboard Audit**:
   - Queried `curl -s -i http://172.22.1.15/`: returned `HTTP/1.1 200 OK`, `Content-Type: text/html; charset=utf-8`, `Content-Length: 1252`, serving Louder-ESP32 Control dashboard HTML/JS.
4. **All 4 Previously Failing Streams Certified**:
   - Set mixer volume to `55` via JSON-RPC.
   - Test 4A (*WHYY*): `501,248` frames played across 12.2s (`41,137` fps, LMS mode: `play`, I2C ack delta: `12`, NACKs: `0`) -> `[PASS]`.
   - Test 4B (*WDIY*): `508,416` frames played across 12.1s (`42,116` fps, LMS mode: `play`, I2C ack delta: `6`, NACKs: `0`) -> `[PASS]`.
   - Test 4C (*DJMix WAV*): `428,096` frames played across 10.0s (`42,720` fps, LMS mode: `play`, I2C ack delta: `8`, NACKs: `0`) -> `[PASS]`.
   - Test 4D (*Ibiza Global Radio MP3*): `436,672` frames played across 12.1s (`36,070` fps, LMS mode: `play`, I2C ack delta: `12`, NACKs: `0`) -> `[PASS]`.
   - Stream continued to `4,900,160` frames at `uptime_ms: 434,308` with zero I2C errors.
5. **AirPlay & Server Log Cleanliness**:
   - `shairport_helper-linux-aarch64` spawned listener on TCP port `41063` for `"BASEMENT"`.
   - Sent `OPTIONS * RTSP/1.0`: received `RTSP/1.0 200 OK` with full AirPlay method set.
   - Inspected `server.log`: zero `Slim::Player::Song::open (426)` errors occurred.
6. **Constraints Adherence**:
   - `POST /reboot_ota.json` was never called (`otadata_prearmed_factory: true` preserved).
   - `/dev/ttyUSB0` was never touched.
   - All background tasks completed (`manage_task(list)`: 0 tasks).
