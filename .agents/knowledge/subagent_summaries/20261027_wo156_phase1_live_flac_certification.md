# Subagent Fulfillment Summary: `WO-156` (Revision 1)

- **Work Order ID**: `WO-156`
- **Role**: Extension Certifier — Live OTA & Hardware Certification
- **Subagent Conversation ID**: `d7a23fc0-f199-4dd7-88f8-b021753f98d4`
- **Timestamp**: `2026-10-27T09:07:00-04:00`
- **Status**: `[BLOCKED / ERROR]`

---

## 1. Files Created, Modified, or Deleted

| File Path (Absolute) | Operation | Rationale |
| --- | --- | --- |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-156-phase1-live-flac-ota-certification.md` | Modified | Updated Status to `[BLOCKED / ERROR]` and documented empirical execution telemetry and HELO codec root cause analysis. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` | Modified | Updated central work order ledger status for `WO-156` to `[BLOCKED / ERROR]`. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo156_phase1_live_flac_certification.md` | Created | Rule 17 subagent summary and certification handoff record. |
| `/home/console-admin/Github/squeezelite-esp32/handoff.md` | Modified | Working directory handoff report. |

---

## 2. Empirical Execution Telemetry (Revision 1)

### Step 1: Pre-Flash Verification & Baseline Status (`[EXECUTED & VERIFIED]`)
- Verified staged OTA image `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`:
  - Size: `955,072` bytes (exact match).
  - SHA-256: `0127e0f9ded2db4ecdf6e6869e542eca12ce0b48d28b6c25445676c194cf52cc` (exact match).
- Recorded baseline pre-flash status from `GET http://172.22.1.15/status.json`:
  - `uptime_ms = 31,732,299` (>8.8 hours continuous uptime since `WO-153` certification).
  - `"recovery": 0`, `"rust_engine": true`, `"psram_used_bytes": 655,360`.

### Step 2: Recovery Transition & Single-Attempt OTA Upload (`[EXECUTED & VERIFIED]`)
- Dispatched Factory recovery transition:
  - `POST http://172.22.1.15/recovery.json` -> `{"status":"rebooting_to_recovery","target_partition":"factory_0x10000"}`.
  - Confirmed recovery mode: `GET http://172.22.1.15/status.json` responded with `"recovery": 1`, `"version": "I2S-4MFlash-1681"`.
- Uploaded staged binary via single HTTP/1.0 request:
  - `curl -s -S --http1.0 -X POST -H "Content-Type: application/octet-stream" --data-binary @.../louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json` -> `File uploaded. Flashing started.`
- Target flashed `ota_0` and rebooted into `louder-esp32-app`.
- Verified Gate 3 boot lease survival past 45 seconds (`uptime_ms = 58,756` and `748,453`):
  - `"recovery": 0` (PASS)
  - `"rust_engine": true` (PASS)
  - `"bt_enabled": true` (PASS — all 17 HCI initialization commands completed)
  - `"bt_rx_pkts": 17` (PASS)
  - `"i2c_nacks": 0` (PASS)
  - `"tas_pll_locked": true` (PASS)
  - `"amp_mode": "Play"` (PASS)
  - Stability: > 748 seconds continuous uptime without crashes, panics, or resets.

### Step 3: Live Lossless `.flac` Streaming Certification (`[BLOCKED / ERROR]`)
- Deterministic FLAC test track generation:
  - Generated 30-second 44.1 kHz 16-bit stereo FLAC track `/tmp/test_stereo_16bit.flac` via `ffmpeg`.
  - Allowed LMS (`10.0.0.10`) access via UFW and served over temporary Python HTTP server on port 8088.
- Stream Execution via LMS:
  - Instructed LMS via JSON-RPC to play `http://10.0.0.90:8088/test_stereo_16bit.flac` on player `c8:2e:18:be:b8:34`.
  - LMS fetched the track (`HTTP 200 OK`).
  - Playback commenced smoothly on `172.22.1.15` (`frames_played` advanced monotonically at 44.1 kHz, `amp_mode = Play`, `tas_pll_locked = true`, `tas_automuted = false`, `i2c_nacks = 0`).
- **Defect Encountered**:
  - `status.json` during active playback reported `"codec": "PCM"`, and `psram_used_bytes` remained at `655,360` B.
  - LMS streamed server-transcoded PCM (`format = 'p'`), rather than native FLAC (`format = 'f'`).
- **Root Cause Analysis**:
  - In `rust/crates/louder-esp32-app/src/main.rs:1995`:
    ```rust
    SlimprotoBuilder::build_helo(
        &BASEMENT_PLAYER_MAC,
        12,
        "Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3",
        &mut helo_buf,
    )
    ```
    The capability format list sent in the HELO frame is `pcm,wav,aif,mp3`. It does NOT include `,flc`!
  - Under the SlimProto protocol and LMS server implementation (`Slim/Player/SqueezePlay.pm:163-175`), LMS parses the codec tokens advertised by the player in HELO:
    ```perl
    for my $cap (split(/,/, $capabilities)) {
        if ($cap =~ /^[a-z][a-z0-9]{1,4}$/) {
            push(@formats, $cap);
        }
    ```
    Because `flc` was not declared, LMS marks the player as unable to decode FLAC and automatically transcodes all `.flac` library and HTTP tracks to PCM via `convert.conf`.
  - Consequently, LMS never sends `format = 'f'` (`AudioFormat::Flac`), and `ActiveDecoder::Flac` in `main.rs:2217` is never triggered.

### Step 4: Dual-Mode Bluetooth Coexistence (`[EXECUTED & VERIFIED]`)
- Executed `sudo /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` from local `hci0`.
- Result: `3 sent, 3 received, 0% loss` (RTT 39.87ms - 58.74ms).
- Dual-Mode Bluetooth Classic BR/EDR and BLE remain 100% responsive and coexisting with Wi-Fi audio.

### Step 5: Hardware Safety & Cleanup
- `/dev/ttyUSB0` was strictly preserved and never opened or accessed.
- Temporary UFW rule on port 8088 deleted.
- Temporary HTTP server process cleanly terminated (`manage_task(list)` = 0).
- Temporary `/tmp/test_stereo_16bit.flac` file deleted.

---

## 3. Known Risks & Remediation Plan (Revision 2)

### Required Code Change
In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs:1995`:
Change:
```rust
"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3"
```
To:
```rust
"Model=SqueezeESP32,ModelName=SqueezeESP32,AccuratePlayPoints=1,HasDigitalOut=1,MaxSampleRate=48000,pcm,wav,aif,mp3,flc"
```
Once `build_helo` advertises `flc`, LMS will pass FLAC bitstreams natively (`format = 'f'`) without server-side transcoding, activating `ActiveDecoder::Flac` and allocating `FlacDecoder`'s three 64 KB buffers in External PSRAM (`851,968` bytes used).
