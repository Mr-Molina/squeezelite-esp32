# Work Order `WO-156`: Phase 1 — Live Wi-Fi OTA Flash & Lossless `.flac` Streaming Certification on `172.22.1.15`

- **Work Order ID**: `WO-156`
- **Timestamp**: `2026-10-27T08:52:00-04:00`
- **Assigned Subagent Role**: Extension Certifier — Live OTA & Hardware Certification (`d7a23fc0-f199-4dd7-88f8-b021753f98d4`)
- **Status**: `[BLOCKED / ERROR]`
- **Safety Tier**: `TIER-3-AUTHORIZED-LIVE-OTA` (Authorized by user `"Phase 1 begin"`; 1-Flash-Attempt Circuit Breaker enforced; NEVER touch `/dev/ttyUSB0`)

---

## 1. Objective & Scope
Execute **Menu Phase 1 Wave 2**: Live Wi-Fi OTA deployment of the staged `louder-esp32-app` firmware (`955,072` B, SHA-256 `0127e0f9ded2db4ecdf6e6869e542eca12ce0b48d28b6c25445676c194cf52cc`) to `[BASEMENT]` (`172.22.1.15`) and end-to-end hardware certification of native on-chip lossless `.flac` decoding in `4 MB` PSRAM.

---

## 2. Step-by-Step Certification Protocol

### Step 1: Pre-Flash Ground-Truth & SHA-256 Verification
1. Verify staged binary `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`:
   - Size must be `955,072` bytes.
   - SHA-256 must equal `0127e0f9ded2db4ecdf6e6869e542eca12ce0b48d28b6c25445676c194cf52cc`.
2. Query `GET http://172.22.1.15/status.json` to record baseline pre-flash state.

### Step 2: 2-Step Recovery Transition & Single-Attempt OTA Upload
1. Trigger Factory Recovery transition: `curl -s -m 5 -X POST http://172.22.1.15/recovery.json`.
2. Poll `GET http://172.22.1.15/status.json` until `"recovery": 1` (Factory C firmware online).
3. Upload `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` via `POST http://172.22.1.15/flash.json` (**1-Flash-Attempt Circuit Breaker**: never retry if it rolls back).
4. Wait for reboot into `ota_0` (`"recovery": 0`, `"rust_engine": true`) and verify survival past the 45-second Gate 3 boot lease (`uptime_ms >= 46000`, `"otadata_prearmed_factory": false`, `"bt_enabled": true`).

### Step 3: Live Lossless `.flac` Streaming Certification (`16-bit` & `24-bit`)
1. Query LMS (`http://10.0.0.10:9000/jsonrpc.js`) for any `.flac` tracks in the LMS library, OR generate deterministic lossless `.flac` test tracks on `/tmp` (e.g. a 30-second 44.1 kHz 16-bit stereo `.flac` track and/or 24-bit stereo `.flac` track via `ffmpeg`/`flac`) and serve them over a temporary local HTTP server (or from `10.0.0.10`) to LMS player `c8:2e:18:be:b8:34` via `["playlist", "play", "http://.../test_stereo_16bit.flac"]`.
   - **Important**: Ensure LMS streams native FLAC (`format = 'f'`) to `c8:2e:18:be:b8:34` without server-side transcoding.
2. During active `.flac` playback on `172.22.1.15`, capture two `GET http://172.22.1.15/status.json` snapshots separated by `10.0` seconds and verify:
   - `"codec": "FLAC"`
   - `"state": "Playing"`
   - `"amp_mode": "Play"`
   - `"tas_pll_locked": true`
   - `"tas_automuted": false`
   - `"i2c_nacks": 0`
   - `frames_played` advancing monotonically at `~44,100` frames/sec (`43,500..44,600` fps)
   - `psram_used_bytes` increased by `~196,608` bytes (`851,968` B used, `3,342,336` B free in External `4 MB` PSRAM), proving `FlacDecoder`'s three `64 KB` buffers were deterministically allocated in External PSRAM with `0` bytes of Internal DRAM heap exhaustion!

### Step 4: Dual-Mode Bluetooth Coexistence Regression Check
1. Run `sudo l2ping -c 3 CA:2E:18:BE:B8:35` from local `hci0` to confirm Dual-Mode Bluetooth (`BTDM`) remains 100% responsive (`0% loss`) while `FlacDecoder` is resident in PSRAM.
2. Stop the test `.flac` stream cleanly (or restore the previous LMS playlist state) and record final `GET http://172.22.1.15/status.json`.

---

## 3. Fulfillment Section
- **Status**: `[BLOCKED / ERROR]`
- **Fulfilled By**: Extension Certifier (`Model: "pro"`, `TypeName: "self"`, Conversation ID: `d7a23fc0-f199-4dd7-88f8-b021753f98d4`)
- **Fulfillment Timestamp**: `2026-10-27T09:06:00-04:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo156_phase1_live_flac_certification.md`
- **Handoff Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo156_phase1_live_flac_certification.md`

### 3.1 Step-by-Step Execution & Live Telemetry

1. **Step 1 — Pre-Flight SHA-256 Verification & Baseline Status (`[EXECUTED & VERIFIED]`)**:
   - Target image: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
   - Size: `955,072` bytes (exact match).
   - SHA-256: `0127e0f9ded2db4ecdf6e6869e542eca12ce0b48d28b6c25445676c194cf52cc` (exact match).
   - Baseline pre-flash status recorded: `uptime_ms = 31,732,299` (>8.8 hours continuous uptime), `recovery: 0`, `rust_engine: true`, `psram_used_bytes: 655,360`.

2. **Step 2 — Recovery Transition & Single-Attempt OTA Upload (`[EXECUTED & VERIFIED]`)**:
   - `POST http://172.22.1.15/recovery.json` -> rebooted into recovery partition (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`).
   - Uploaded staged binary via `POST http://172.22.1.15/flash.json` (`File uploaded. Flashing started.`).
   - Target rebooted into `ota_0` (`louder-esp32-app`).
   - Gate 3 boot lease verification past 45 seconds (`uptime_ms = 58,756` and `748,453`):
     - `"recovery": 0` (PASS)
     - `"rust_engine": true` (PASS)
     - `"bt_enabled": true` (PASS)
     - `"bt_rx_pkts": 17` (PASS)
     - `"i2c_nacks": 0` (PASS)
     - `"tas_pll_locked": true` (PASS)
     - `"amp_mode": "Play"` (PASS)
     - Stability: > 748 seconds continuous uptime with zero crashes or resets.

3. **Step 3 — Live Lossless `.flac` Streaming Certification (`[BLOCKED / ERROR]`)**:
   - Generated 30-second 44.1 kHz 16-bit stereo FLAC track `/tmp/test_stereo_16bit.flac`.
   - Served on port 8088.
   - Instructed LMS (`10.0.0.10:9000`) to play `http://10.0.0.90:8088/test_stereo_16bit.flac` on player `c8:2e:18:be:b8:34`.
   - LMS fetched `/test_stereo_16bit.flac` (HTTP 200).
   - Audio played smoothly on `172.22.1.15` (`frames_played` advancing at 44.1 kHz, `tas_pll_locked = true`, `tas_automuted = false`, `i2c_nacks = 0`).
   - **Blocking Defect**: LMS streamed server-transcoded PCM (`format = 'p'`), NOT native FLAC (`format = 'f'`). The ESP32's `status.json` reported `"codec": "PCM"` and `psram_used_bytes` remained at `655,360` B.
   - **Root Cause Analysis**: In `rust/crates/louder-esp32-app/src/main.rs:1995`:
     ```rust
     SlimprotoBuilder::build_helo(
         &BASEMENT_PLAYER_MAC,
         12,
         "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3",
         &mut helo_buf,
     )
     ```
     The format list does NOT include `,flc`!
     According to the SlimProto protocol and LMS `Slim::Player::SqueezePlay::updateCapabilities` (`/usr/share/perl5/Slim/Player/SqueezePlay.pm:163-175`), LMS only streams native FLAC if `flc` is declared as a supported format token in the HELO capabilities. Because `flc` was omitted by `WO-155`, LMS believes the player does not support FLAC and automatically transcodes all `.flac` streams to PCM on the server.

4. **Step 4 — Dual-Mode Bluetooth Coexistence (`[EXECUTED & VERIFIED]`)**:
   - `sudo /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` passed with `3 sent, 3 received, 0% loss` (39.87ms - 58.74ms RTT).
   - Dual-Mode Bluetooth Classic BR/EDR and BLE remain 100% responsive and coexisting with Wi-Fi audio playback.

5. **Hardware Safety**:
   - `/dev/ttyUSB0` was strictly preserved and never opened or touched.

### 3.2 Required Remediation for Revision 2
In `rust/crates/louder-esp32-app/src/main.rs:1995`:
Change:
```rust
"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3"
```
to:
```rust
"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3,flc"
```
Rebuild and stage OTA image via `WO-155` Rev 2, then re-flash and certify.
