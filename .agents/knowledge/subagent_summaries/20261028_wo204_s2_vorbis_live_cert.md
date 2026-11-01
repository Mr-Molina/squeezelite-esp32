# Subagent Summary: WO-204 — S2 Ogg Vorbis Live Hardware Certification on "BASEMENT"

- **Date**: `2026-10-28T20:26:00-04:00`
- **Role**: `Extension Certifier (WO-204 S2 Live Vorbis Certification)`
- **Work Order**: `WO-204` ([WO-204-s2-vorbis-live-certification.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-204-s2-vorbis-live-certification.md))
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Created, Modified, or Backed Up

### Workstation Files (`pc-1`)
- `created`: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/poll_gate1.py` (Gate 1 boot lease telemetry monitor)
- `created`: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/poll_gate3.py` (Gate 3 10-second Vorbis telemetry monitor)
- `created`: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/poll_gate4.py` (Gate 4 MP3 reversion and heap reclamation monitor)
- `created`: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s2_vorbis_bd49e6e0/` (S2 certified baseline archive)
  - `louder-esp32-app-basement-ota-s2-vorbis.bin` (`1,377,600` B, SHA-256 `bd49e6e007f0347f19fad00b13b03265156c81914feca4f39a5df61cb77b2edf`)
  - `rust/` (clean source snapshot excluding `target/`)
- `modified`: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-204-s2-vorbis-live-certification.md` (appended fulfillment summary, updated status)
- `modified`: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` (registered WO-204 status)

### LMS Server Files (`10.0.0.10`)
- `created`: `/var/lib/squeezeboxserver/cache/test_stereo_15s_44k1.ogg` (88,335 B, test audio fixture)
- `modified`: `/etc/squeezeboxserver/custom-convert.conf` (added native ogg direct streaming rule; backup at `/etc/squeezeboxserver/custom-convert.conf.bak_wo204`)
- `modified`: `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` (preserved native ogg commands; backup at `.bak_wo204`)
- `modified`: `/var/lib/squeezeboxserver/prefs/log.conf` (enabled DEBUG logging on `network.protocol.slimproto`; backup at `.bak_wo204`)

---

## 2. Rationale for Changes
- Enabled native Ogg Vorbis streaming from LMS `10.0.0.10` to ESP32 player `c8:2e:18:be:b8:34` without intermediate transcoding to FLAC or MP3.
- Uploaded and certified the `WO-203` pure-Rust `no_std + alloc` `lewton` Vorbis decoder on live hardware (`172.22.1.15`), confirming PSRAM memory stability, I2S TAS5805M synchronization, and dynamic MP3/Vorbis decoder switching.

---

## 3. Ground-Truth Verification Evidence

1. **Staged OTA Audit**:
   - `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]` (`size=1377600`, SHA-256 `bd49e6e007f0347f19fad00b13b03265156c81914feca4f39a5df61cb77b2edf`).
2. **Factory Recovery Transition & Flash**:
   - `POST /recovery.json` -> rebooted to Factory recovery `0x10000` (`recovery: 1`).
   - Single-flash upload: `POST /flash.json` -> `File uploaded. Flashing started.`.
3. **Gate 1 (Boot Lease past 45s)**:
   - `uptime_ms = 46,495` (> 46,000 ms), `recovery = 0`, `rust_engine = true`, `otadata_prearmed_factory = true`, `core1_active = true`, `tas_pll_locked = true`, `i2c_nacks = 0`, `bt_enabled = true`.
4. **Gate 2 (TAS5805M Vol 55% & Web UI)**:
   - `cmd.json?act=vol55` -> `vol_pct: 55`, `tas_automuted: false`.
   - Web UI `GET /` -> `HTTP_CODE:200 SIZE:3928` B.
5. **Gate 3 (Native Vorbis Streaming)**:
   - LMS `server.log`: `stream_s called: format: ogg url: file:///var/lib/squeezeboxserver/cache/test_stereo_15s_44k1.ogg`.
   - ESP32 telemetry: `"codec": "Vorbis"`, `frames_played` monotonically advanced `+260,160` frames across 11.5s, `"tas_automuted": false`, `"tas_pll_locked": true`, `"i2c_nacks": 0`.
   - CPU metrics: `"decode_us_first": 32070`, `"decode_us_max": 150026` (setup codebooks/tables).
6. **Gate 4 (Dynamic MP3 Reversion)**:
   - `cmd.json?act=preset1` -> `"codec": "MP3"`, `frames_played` advancing at `39,109` fps.
   - PSRAM reclaimed: `psram_free_bytes: 3534064`, `psram_used_bytes: 660240` (exact byte parity with Gate 1 boot baseline, 0 B leaked).
7. **Gate 5 (Bluetooth A2DP/BLE)**:
   - `sudo l2ping -c 3 CA:2E:18:BE:B8:35` -> 3 packets sent/received, 0% loss (min 39.9ms, max 95.9ms).
   - `sdptool browse CA:2E:18:BE:B8:35` -> Service Class ID `"Audio Sink" (0x110b)`.
8. **Gate 6 (LMS Sibling Services)**:
   - UPnP `MediaRenderer.xml`: `HTTP 200 OK`, `Content-Length: 3105`.
   - CastBridge (`squeeze2cast-linux-aarch64-static` PID 35201) and ShairTunes2W (`avahi-publish-service` PID 35286) active.

---

## 4. Known Risks or Untested Assumptions
- None. Serial port `/dev/ttyUSB0` was strictly untouched (Invariant 47). No calls were made to `/reboot_ota.json` (Invariant 42). Exactly 1 flash cycle was executed under the circuit breaker constraint.
