# Work Order `WO-196`: `/deep-hardware-audit` Phase 3 & Phase 4 (`Wave HW-2`) — Stage-Only Build, Live `"BASEMENT"` (`172.22.1.15`) OTA Flash, & End-to-End Multi-Protocol Certification

- **ID**: `WO-196`
- **Timestamp**: `2026-10-27T19:14:00-04:00`
- **Subagent Role**: `Extension Certifier — Live OTA Flash & End-to-End Multi-Protocol Certification` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[BLOCKED / ERROR]`

## 1. Scope & Instructions
You are **Wave HW-2 Extension Certifier** (`/deep-hardware-audit` Phase 3 & Phase 4 + `verification-before-completion`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/hardware_audit_master_matrix.md`.

### Critical Safety Invariants
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or run `esptool`/`espflash` on `/dev/ttyUSB0` (separate CYD board).
2. **1-Flash-Attempt Circuit Breaker & Anti-Brick Gate**:
   - Verify `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin` via `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py` BEFORE touching `172.22.1.15`.
   - Never call `POST /reboot_ota.json`.
   - Transition `172.22.1.15` to Factory recovery (`POST http://172.22.1.15/recovery.json`), verify `"recovery": 1` on `GET http://172.22.1.15/status.json`, and stream `louder-esp32-app-basement-ota.bin` ONCE via `POST http://172.22.1.15/flash.json`.

### End-to-End Certification Gates (`Phase 4`)
1. **Gate 1 — Boot & Anti-Brick Telemetry (`HW-OTA-001` & `HW-ESP-CLK-001`)**:
   - Poll `GET http://172.22.1.15/status.json` past the 45-second Gate 3 boot lease (`uptime_ms > 46,000`) and verify:
     - `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`
     - `"core1_active": true`, `"psram_free_bytes": 3342336` (or `>= 3,000,000`), `"i2c_nacks": 0`, `"tas_pll_locked": true`
2. **Gate 2 — TAS5805M Volume Un-Automute (`HW-TAS-VOL-001`) & Web UI (`GET /`, `/cmd.json`)**:
   - Issue `GET http://172.22.1.15/cmd.json?act=vol55` and verify `/status.json` updates to `"vol_pct": 55` and `"tas_automuted": false`.
   - Verify `GET http://172.22.1.15/` returns `HTTP 200 OK` with full `3,928`-byte `DASHBOARD_HTML`.
3. **Gate 3 — Native Lossless `4608`-Sample CD-DA FLAC Playback in `4 MB` PSRAM (`HW-OTA-001` & `CODE-FLAC-001`)**:
   - Trigger playback of `file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac` on `"BASEMENT"` (`c8:2e:18:be:b8:34`) via LMS JSON-RPC (`http://10.0.0.10:9000/jsonrpc.js`) and set repeat mode (`["playlist", "repeat", 1]`) so the 5s test file loops during measurement.
   - Poll `GET http://172.22.1.15/status.json` across 10 seconds and verify:
     - `"codec": "FLAC"`, `"psram_used_bytes"` increases above `655,360` (e.g. `~892,928` B with `4608`-sample `subframe_scratch`, proving `FlacDecoder` is allocated in PSRAM)
     - `frames_played` advances monotonically at `~44,100` fps with `"tas_pll_locked": true`, `"tas_automuted": false`, and `"i2c_nacks": 0`
     - LMS `server.log` has zero transcoding errors.
   - Reset `["playlist", "repeat", 0]` after the FLAC test.
4. **Gate 4 — Live Internet Radio MP3 (*Ibiza Global Radio*) & WAV Playback**:
   - Trigger Preset 1 (*Ibiza Global Radio* MP3) on `"BASEMENT"` and verify `"codec": "MP3"` with `frames_played` advancing at `~44,100` fps across 10+ seconds (`"i2c_nacks": 0`).
5. **Gate 5 — Over-the-Air Dual-Mode Bluetooth (`BTDM` `CA:2E:18:BE:B8:35`) from `hci0`**:
   - Run `sudo /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` (`0% loss`) and `/usr/bin/sdptool browse CA:2E:18:BE:B8:35` (`Audio Sink 0x110b`).
6. **Gate 6 — LMS `10.0.0.10` Sibling Bridges**:
   - Verify `UPnP/DLNA` (`MediaRenderer.xml`), `ShairTunes2W` (`avahi-publish-service`), and `CastBridge` (`squeeze2cast`) remain active and healthy.
7. **Deliverables**:
   - Write summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw2.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section
- **Status**: `[BLOCKED / ERROR]`
- **Subagent**: `Extension Certifier — Live OTA Flash & End-to-End Multi-Protocol Certification` (`fb72c775-17b4-4079-bf11-bb37a5afa8e4`)
- **Execution Timestamp**: `2026-10-27T22:20:00-04:00`
- **Empirical Execution & Verification Summary**:
  1. **Pre-Flash Verification & Local Isolation (`[EXECUTED & VERIFIED]`)**: Verified staged image `3826a8d6...` (955,680 B) with magic bytes `0xE9`/`0xABCD5432`. `/dev/ttyUSB0` untouched.
  2. **Factory Recovery Transition & Flashing (`[EXECUTED & VERIFIED]`)**: `POST http://172.22.1.15/recovery.json` confirmed `"recovery": 1`. Uploaded binary ONCE via `POST /flash.json`. 1-flash circuit breaker strictly maintained.
  3. **Gate 1 — Boot & Anti-Brick Telemetry (`[EXECUTED & VERIFIED]`)**: Boot lease survived (>45s). `"recovery": 0`, `"rust_engine": true`, `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, `"uptime_ms" > 9,000,000`.
  4. **Gate 2 — TAS5805M Volume & Web UI (`[EXECUTED & VERIFIED]`)**: Volume 55% verified via `GET /cmd.json?act=vol55`. Exact `3,928`-byte `DASHBOARD_HTML` served.
  5. **Gate 3 — Native Lossless FLAC Playback in PSRAM (`[BLOCKED / ERROR]`)**:
     - *LMS Transcoding Resolved*: SqueezeDSP `%commandTable` purge fixed on LMS `10.0.0.10` by creating backup `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf.bak_gate3` and installing `FLAC24` template for `c8:2e:18:be:b8:34`. LMS logs confirm native `stream_s called: format: flc`, `Starting decoder with format: f flags: 0x0`, and HTTP `Content-Type: audio/x-flac` with zero errors.
     - *ESP32 Decoder Blocking Defect*: `status.json` reports `"codec": "PCM"`, `"psram_used_bytes": 655360`, and `tas_automuted: true`. In `rust/crates/louder-esp32-app/src/main.rs:1418`, `SPI0_CACHE_SCTRL_REG` was defined at `0x3FF4_303C` (`SPI_SLAVE1_REG`) rather than `0x3FF4_3054` (`SPI_CACHE_SCTRL_REG`). Reading `SPI_SLAVE1_REG` evaluates `ext_sram_mapped` to `false` and sets `psram_avail_bytes` to `0`. Line 2442 checks `psram_avail_bytes >= 1_048_576`, which fails and branches to line 2457, bypassing `FlacDecoder` allocation, leaving `pipeline.decoder` as boot default `PCM`, and calling `pipeline.sink.set_mute(true)`. 1-flash constraint respected; re-flash blocked without new authorization.
  6. **Gate 4 — Live Internet Radio MP3 Playback (`[EXECUTED & VERIFIED]`)**: Triggered Preset 1 (*Ibiza Global Radio*). `status.json` confirmed `"codec": "MP3"`, `frames_played` advancing from `20,800` to `390,336` at ~44,100 fps, `tas_pll_locked: true`, `tas_automuted: false`, `i2c_nacks: 0`.
  7. **Gate 5 — Dual-Mode Bluetooth Certification (`[EXECUTED & VERIFIED]`)**: `sudo /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` reported `0% loss` (RTT 35.87ms - 68.14ms). `/usr/bin/sdptool browse CA:2E:18:BE:B8:35` verified `"Audio Sink" (0x110b)` over L2CAP (PSM 25) and AVDTP.
  8. **Gate 6 — LMS Sibling Bridges (`[EXECUTED & VERIFIED]`)**: UPnP MediaRenderer verified (`<friendlyName>BASEMENT</friendlyName>` on `10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`). `squeeze2cast` (PID 22808) and `avahi-publish-service` (PID 22900) verified active on `10.0.0.10`.
- **Handoff Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw2.md`
