# Work Order `WO-052`: Flash Pure-Rust OTA Binary (`96c7e60b`) to `172.22.1.15` (`ota_0`) & Verify Live Internet Radio (`"Ibiza Global Radio MP3"`) + Spotify Playback

- **ID**: `WO-052`
- **Timestamp**: `2026-10-24T16:01:00-04:00`
- **Subagent Role**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Execute `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo050_internet_radio.py` to:
1. Confirm `172.22.1.15` (`"BASEMENT"`) is in Factory `recovery` (`0x10000`, `"recovery": 1`).
2. Stream the staged pure-Rust binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`727,312` bytes, SHA-256 `b1973a9d8eea38c627c57116094a7d85f5d2b61a1d0cd68af19e1b0fd0c98ac3`, commit `96c7e60b`) to `ota_0` (`0x160000`) via `POST http://172.22.1.15/flash.json`.
3. Verify `172.22.1.15` boots `louder-esp32-app` (`"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"otadata_prearmed_factory": true`) and registers `"BASEMENT"` (`c8:2e:18:be:b8:34`) with LMS `10.0.0.10:9000`.
4. Play `"Ibiza Global Radio MP3"` (`https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`) on `"BASEMENT"` and verify `"frames_played"` in `GET http://172.22.1.15/status.json` advances continuously past `512` (`> 150,000` frames over 20s) with `"amp_mode": "Play"`, `"i2c_nacks": 0`, and zero OOM/stack reset.

## 2. Mandatory Constraints
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- **NEVER call `POST /reboot_ota.json`** (keep `otadata_prearmed_factory: true` single-boot trial mode).
- Follow Rule 15, Rule 17, Rule 21 (wait for all background tasks to finish and verify exit code `0` before handoff).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Extension Certifier
- **Completion Timestamp**: 2026-10-24T16:05:00-04:00
- **Target Device**: `172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)
- **Staged OTA Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Binary Size**: 727,312 bytes (27.07% of `ota_0`)
- **SHA-256**: `b1973a9d8eea38c627c57116094a7d85f5d2b61a1d0cd68af19e1b0fd0c98ac3`
- **Git Commit**: `96c7e60b240ffb60bc571b058a9831969a5a3a78`
- **Verification Scripts**:
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo052_internet_radio.py`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_wo052_switch_resume.py`

### Empirical Verification Telemetry

1. **Pre-Check & OTA Flash**:
   - `http://172.22.1.15/status.json` pre-check confirmed Factory recovery partition `0x10000` (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`).
   - Streamed `727,312` bytes via `POST /flash.json` (`HTTP/1.1 200 OK: File uploaded. Flashing started.`).
   - Device erased and flashed `ota_0` (`0x160000`), then rebooted at `t = 18s`.

2. **Pure-Rust Boot on `ota_0`**:
   - Polling at `t = 22s` confirmed pure-Rust firmware boot:
     ```json
     {"project_name": "Squeezelite-ESP32", "platform_name": "ESP32", "version": "I2S-4MFlash-Rust", "recovery": 0, "rust_engine": true, "otadata_prearmed_factory": true, "if": "wifi", "ip": "172.22.1.15", "netmask": "255.255.255.0", "gw": "172.22.1.1", "ssid": "iot", "rssi": -83, "frames_played": 0, "amp_mode": "Play", "i2c_acks": 15, "i2c_nacks": 0, "uptime_ms": 2384}
     ```
   - Confirmed `.stack` expansion (`38,576` B) and `16 KB` `nvs_buf` stack frame elimination completely resolved the early boot stack collision.

3. **LMS Registration**:
   - Registered with LMS `10.0.0.10:9000` within 1.0 second (`player_ip: "172.22.1.15:50965"`, `mode: "stop"`).

4. **Phase 1: Live Internet Radio ("Ibiza Global Radio MP3") Playback**:
   - URL: `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3` (MP3 stream with ICY metadata).
   - Telemetry progression:
     - `t = +2s`: `frames_played = 0`, `uptime_ms = 6570`
     - `t = +5s`: `frames_played = 640`, `uptime_ms = 9682`
     - `t = +8s`: `frames_played = 137,600`, `uptime_ms = 12785`
     - `t = +12s`: `frames_played = 274,176`, `uptime_ms = 15884`
     - `t = +16s`: `frames_played = 413,440`, `uptime_ms = 19040`
     - `t = +20s`: `frames_played = 549,376`, `i2c_acks = 26`, `i2c_nacks = 0`, `uptime_ms = 22124`
   - Zero crashes, zero panics, zero I2C NACKs.

5. **Phase 2: Seamless Codec Switch -> Spotify (PCM)**:
   - Track: `spotify:track:5w7cKwRYjlNJCZgcrscvjg` (*"14 Romances, Op. 34: No. 14, Vocalise"*).
   - Telemetry progression:
     - `t = +2s`: `frames_played = 20,661`, `uptime_ms = 24124`
     - `t = +5s`: `frames_played = 150,174`, `uptime_ms = 27124`
     - `t = +8s`: `frames_played = 272,538`, `uptime_ms = 30124`
     - `t = +12s`: `frames_played = 409,370`, `uptime_ms = 33124`
     - `t = +16s`: `frames_played = 534,474`, `uptime_ms = 36124`
     - `t = +50s`: `frames_played = 2,200,051`, `i2c_acks = 96`, `i2c_nacks = 0`, `uptime_ms = 75649`
   - Confirmed `codc` dynamic codec switching from MP3 to PCM handled cleanly.

6. **Phase 3: Seamless Codec Switch Back -> Internet Radio ("Ibiza Global Radio MP3")**:
   - Switched playlist back to Ibiza MP3.
   - Stream reset and resumed immediately:
     - `t = +5s`: `frames_played = 0`, `uptime_ms = 80965`
     - `t = +8s`: `frames_played = 122,560`, `uptime_ms = 84426`
     - `t = +12s`: `frames_played = 258,240`, `uptime_ms = 87502`
     - `t = +16s`: `frames_played = 394,688`, `uptime_ms = 90597`
     - `t = +20s`: `frames_played = 530,496`, `i2c_acks = 107`, `i2c_nacks = 0`, `uptime_ms = 93677`
     - Current soak: `frames_played = 1,524,544`, `uptime_ms = 116,217`, `i2c_acks = 118`, `i2c_nacks = 0`.

7. **Safety & Hardware Boundaries**:
   - Single-boot trial mode (`otadata_prearmed_factory: true`) remained strictly pre-armed throughout the test.
   - `/dev/ttyUSB0` was strictly never accessed (Rule 3/Rule 44 compliant).
