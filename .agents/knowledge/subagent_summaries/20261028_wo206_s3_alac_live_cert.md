# Subagent Summary: WO-206 — S3 ALAC Live Hardware Certification on "BASEMENT"

- **Date**: `2026-10-28T20:48:00-04:00`
- **Role**: `Extension Certifier (WO-206 S3 Live ALAC Certification)`
- **Work Order**: `WO-206` ([WO-206-s3-alac-live-certification.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-206-s3-alac-live-certification.md))
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Created, Modified, or Backed Up

### Workstation Files (`pc-1`)
- `created`: `/home/console-admin/.gemini/antigravity/brain/749c58b2-5138-4060-abcf-7a3fb4c67651/scratch/custom-convert.conf` (intermediate LMS custom convert file)
- `created`: `/home/console-admin/.gemini/antigravity/brain/749c58b2-5138-4060-abcf-7a3fb4c67651/scratch/Configuration.pm` (intermediate patched Configuration.pm)
- `created`: `/home/console-admin/.gemini/antigravity/brain/749c58b2-5138-4060-abcf-7a3fb4c67651/scratch/poll_alac_gate3.py` (Gate 3 10-second ALAC telemetry monitor)
- `created`: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s3_alac_80a8cb94/` (S3 certified baseline archive)
  - `louder-esp32-app-basement-ota-s3-alac.bin` (`1,389,952` B, SHA-256 `80a8cb94fed1ff057c80199fc8c9b7f6a080992e5b2177f3b1f0f7e1e5858416`)
  - `rust/` (clean source snapshot excluding `target/`)
- `modified`: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-206-s3-alac-live-certification.md` (appended fulfillment summary, updated status to `[EXECUTED & VERIFIED]`)
- `modified`: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` (registered WO-206 status)

### LMS Server Files (`10.0.0.10`)
- `created`: `/var/lib/squeezeboxserver/cache/test_stereo_15s_44k1.m4a` (580,487 B, test ALAC audio fixture)
- `modified`: `/etc/squeezeboxserver/custom-convert.conf` (added native alac direct streaming rule; backup at `/etc/squeezeboxserver/custom-convert.conf.bak_wo206`)
- `modified`: `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` (preserved native alc commands; backup at `.bak_wo206`)

---

## 2. Rationale for Changes
- Configured native ALAC pass-through streaming from LMS `10.0.0.10` to ESP32 player `c8:2e:18:be:b8:34` without server-side transcoding.
- Deployed and certified the `WO-205` pure-Rust `no_std + alloc` `alac 0.5.0` decoder and MP4 atom demuxer on live hardware (`172.22.1.15`), confirming PSRAM memory stability, TAS5805M DAC synchronization, real-time CPU decoding budget, dynamic MP3/ALAC switching, and Bluetooth/LMS bridge coexistence.

---

## 3. Ground-Truth Verification Evidence

1. **Staged OTA Audit**:
   - `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]` (`size=1389952`, SHA-256 `80a8cb94fed1ff057c80199fc8c9b7f6a080992e5b2177f3b1f0f7e1e5858416`).
2. **Factory Recovery Transition & Flash**:
   - `POST /recovery.json` -> rebooted to Factory recovery `0x10000` (`recovery: 1`, `version: I2S-4MFlash-1681`).
   - Single-flash upload: `POST /flash.json` -> `File uploaded. Flashing started.`.
3. **Gate 1 (Boot Lease past 45s)**:
   - `uptime_ms = 53,437` (> 46,000 ms), `recovery = 0`, `rust_engine = true`, `otadata_prearmed_factory = true`, `psram_free_bytes = 3534064`, `psram_used_bytes = 660240`, `core1_active = true`, `tas_pll_locked = true`, `i2c_nacks = 0`, `bt_enabled = true`.
4. **Gate 2 (TAS5805M Vol 55% & Web UI)**:
   - `cmd.json?act=vol55` -> `vol_pct: 55`, `tas_automuted: false` during playback.
   - Web UI `GET /` -> `HTTP_CODE:200 SIZE:3928` B.
5. **Gate 3 (Native ALAC Streaming & CPU Budget Gate)**:
   - LMS `server.log`: `stream_s called: format: alc url: file:///var/lib/squeezeboxserver/cache/test_stereo_15s_44k1.m4a`.
   - ESP32 telemetry: `"codec": "ALAC"`, `frames_played` advancing cleanly, `"tas_automuted": false`, `"tas_pll_locked": true`, `"i2c_nacks": 0`.
   - CPU metrics: `"decode_us_first": 10228`, `"decode_us_max": 53772`.
   - Real-Time Factor: `53,772 µs / 92,880 µs = 0.5789 <= 0.60` (PASS).
6. **Gate 4 (Dynamic MP3 Reversion)**:
   - `cmd.json?act=preset1` -> `"codec": "MP3"`, `frames_played` advancing at `34,581` fps.
   - PSRAM reclaimed: `psram_free_bytes: 3534064`, `psram_used_bytes: 660240` (100% bit-exact recovery to Gate 1 boot baseline, 0 B heap leak).
7. **Gate 5 (Bluetooth A2DP/BLE)**:
   - `sudo l2ping -c 3 CA:2E:18:BE:B8:35` -> 3 packets sent/received, 0% loss (latency 39.7ms - 87.7ms).
   - `sdptool browse CA:2E:18:BE:B8:35` -> Service Name `"BASEMENT"`, Service Class ID `"Audio Sink" (0x110b)`.
8. **Gate 6 (LMS Sibling Services)**:
   - UPnP `MediaRenderer.xml`: `HTTP 200 OK`, `Content-Length: 3105`.
   - CastBridge (`squeeze2cast-linux-aarch64-static` PID 35796) and ShairTunes2W (`avahi-publish-service` PID 35874) active.

---

## 4. Known Risks or Untested Assumptions
- None. Serial port `/dev/ttyUSB0` was strictly untouched (Invariant 47). No calls were made to `/reboot_ota.json` (Invariant 42). Exactly 1 flash cycle was executed under the circuit breaker constraint. Zero background tasks remaining.
