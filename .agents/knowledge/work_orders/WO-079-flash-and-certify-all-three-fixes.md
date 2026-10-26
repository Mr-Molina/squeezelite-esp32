# Work Order: WO-079 — Live Flash of `WO-077` Firmware to `172.22.1.15` & End-to-End Interactive Control + Stream + AirPlay/UPnP Certification

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-079` |
| **Timestamp** | `2026-10-25T10:08:00-04:00` |
| **Assigned Role** | Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`) |
| **Target Scope** | `172.22.1.15` (`"BASEMENT"`) & `10.0.0.10` (`LMS`) |
| **Status** | `[EXECUTED & VERIFIED]` |

## 1. Objective
Flash the remediated `WO-077` firmware (commit `0e2f4732`, `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, `738,240` bytes, SHA-256 `4a69c1d93cb1fc095b063ce187c7ef41777bb4888d4584300849c15bd6970b90`, `.stack == 36,528` B) to `172.22.1.15` (`ota_0` via Factory `recovery: 1` `POST /flash.json`) and execute the strict **Invariant 44** end-to-end verification suite proving all 3 regressions are resolved.

## 2. Mandatory Certification Sequence (Invariant 44)
1. **Gate 5A/5B Safe OTA Flash**:
   - Trigger `POST http://172.22.1.15/recovery.json`, verify `"recovery": 1` on `0x10000`, and stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`738,240` B, SHA-256 `4a69c1d93cb1fc095b063ce187c7ef41777bb4888d4584300849c15bd6970b90`) to `POST http://172.22.1.15/flash.json`.
   - Wait for reboot into `louder-esp32-app` (`"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"otadata_prearmed_factory": true`).
2. **End-to-End Interactive Web UI Control Gate (`/cmd.json?act=...`)**:
   - Verify `GET http://172.22.1.15/` returns full `DASHBOARD_HTML` (`HTTP 200 OK`, `0` missing bytes).
   - Execute `curl -s "http://172.22.1.15/cmd.json?act=vol40"` -> verify `GET http://172.22.1.15/status.json` reports `"vol_pct": 40` and `i2c_acks` incremented (`i2c_nacks: 0`).
   - Execute `curl -s "http://172.22.1.15/cmd.json?act=vol55"` -> verify `GET http://172.22.1.15/status.json` reports `"vol_pct": 55`.
   - Execute `curl -s "http://172.22.1.15/cmd.json?act=preset1"` (*Ibiza Global Radio*) -> verify LMS `10.0.0.10:9000` transitions `"BASEMENT"` to `mode: "play"` and `frames_played` advances at `~44,100` fps.
   - Execute `curl -s "http://172.22.1.15/cmd.json?act=pause"` -> verify LMS `"BASEMENT"` transitions to `mode: "pause"`.
   - Execute `curl -s "http://172.22.1.15/cmd.json?act=play"` -> verify LMS `"BASEMENT"` transitions back to `mode: "play"` and `frames_played` resumes advancing at `~44,100` fps.
3. **Sibling Daemon & Bridge Verification on `10.0.0.10`**:
   - Confirm `shairport_helper-linux-aarch64` (`ShairTunes2W` AirPlay) is running and `_raop._tcp` is advertised.
   - Confirm `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` returns `HTTP/1.1 200 OK`.

## Fulfillment Section
- **Execution Date**: `2026-10-25T10:37:00-04:00`
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Summary of Actions & Verification Evidence**:
  1. **Gate 5A/5B Safe OTA Flash**:
     - Verified binary: commit `0e2f4732`, `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`738,240` bytes, SHA-256 `4a69c1d93cb1fc095b063ce187c7ef41777bb4888d4584300849c15bd6970b90`).
     - Triggered `POST http://172.22.1.15/recovery.json` -> verified clean reboot into Factory recovery partition (`0x10000`, `"recovery": 1`, `version: "I2S-4MFlash-1681"`).
     - Streamed binary to `POST http://172.22.1.15/flash.json` -> returned `HTTP/1.1 200 OK`, `File uploaded. Flashing started.`.
     - Confirmed clean boot into `louder-esp32-app` at `t = 14.5s`: `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"vol_pct": 47`.
  2. **End-to-End Interactive Web UI Control Gate (`/cmd.json?act=...`)**:
     - `GET http://172.22.1.15/`: Returned `HTTP/1.1 200 OK`, `Content-Length: 2959`, `0` missing bytes, ending in `</html>` with embedded interactive buttons and `/cmd.json?act=`.
     - `GET http://172.22.1.15/cmd.json?act=vol40`: Returned `{"ok":true,"act":"vol40"}`; `/status.json` reported `"vol_pct": 40`, and `i2c_acks` increased from 24 to 36 with `i2c_nacks: 0`.
     - `GET http://172.22.1.15/cmd.json?act=vol55`: Returned `{"ok":true,"act":"vol55"}`; `/status.json` reported `"vol_pct": 55`, and `i2c_acks` increased to 40 with `i2c_nacks: 0`.
     - `GET http://172.22.1.15/cmd.json?act=preset1`: Returned `{"ok":true,"act":"preset1"}`; LMS `10.0.0.10:9000` received SlimProto IR code and transitioned `"BASEMENT"` to `mode: "play"` (*Ibiza Global Radio*).
     - `GET http://172.22.1.15/cmd.json?act=pause`: Returned `{"ok":true,"act":"pause"}`; LMS `"BASEMENT"` transitioned to `mode: "pause"`.
     - `GET http://172.22.1.15/cmd.json?act=play`: Returned `{"ok":true,"act":"play"}`; LMS `"BASEMENT"` transitioned back to `mode: "play"`.
     - 15-second soak test (`verify_soak.py`): Measured 662,016 frames advanced across 15.016s -> **44,086.44 fps** (99.969% of nominal 44,100 Hz), 0 I2C NACKs, amp mode `Play`.
  3. **Sibling Daemon & Bridge Verification on `10.0.0.10`**:
     - Verified `logitechmediaserver.service` cleanly handled `"BASEMENT"` disconnect and reconnect.
     - PID 11197 `/bin/avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 38143` actively running; port 38143 listening.
     - `curl -sI "http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34"` returned `HTTP/1.1 200 OK` (3,105 bytes, 12ms).
     - `server.log` confirmed zero errors.
