# Work Order: WO-236 — Wave 10D: Live `"BASEMENT"` (`172.22.1.15`) OTA Deployment of Remediated Image (`e830ed90...`) & 6-Gate Full Multi-Protocol Certification

- **Work Order ID**: `WO-236`
- **Timestamp**: `2026-11-01T01:54:00-04:00`
- **Assigned Subagent Role**: `Extension Certifier` (`92e605c6-2351-418f-8416-f0324ebabd6f`)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`), Lyrion Music Server (`10.0.0.10:9000` / `3483`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Deploy the remediated full-parity staged OTA image `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` (`1,794,896` bytes, SHA-256 `e830ed90119d536f8c6eadb817000e371ee3afcf7476a9dcbb0797db96b5ef74`, `.stack = 37,416` B, `.dram2_uninit = 95,256` B, 4 MB PSRAM as `esp_alloc::HEAP` Region 0, `STATIC_PIPELINE` zeroed & all `WO-228` fields initialized) to `"BASEMENT"` (`172.22.1.15`, currently waiting in Factory recovery `"recovery": 1`) strictly **ONCE** via `POST http://172.22.1.15/flash.json`, and certify all 6 live hardware gates across Phase 0+1 and Phases 2–7.

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash to `172.22.1.15` at most **ONCE** via Factory recovery `POST http://172.22.1.15/flash.json`.
4. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
5. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution & 6-Gate Live Certification Steps
1. **Pre-Flight Check**:
   - Verify `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` has size `1,794,896` B and SHA-256 `e830ed90119d536f8c6eadb817000e371ee3afcf7476a9dcbb0797db96b5ef74`.
   - Query `GET http://172.22.1.15/status.json` to confirm `"BASEMENT"` is in Factory recovery (`"recovery": 1`).
2. **1-Flash OTA Deployment to `"BASEMENT"` (`172.22.1.15`)**:
   - Upload `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` strictly **ONCE** via `POST http://172.22.1.15/flash.json` (`--data-binary @/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`).
3. **Gate 1 — Boot Lease & Hardware Health (`> 45s`)**:
   - Wait for `"BASEMENT"` to boot into `ota_0` (`0x1D0000`) and run past the 45-second boot lease (`uptime_ms > 46,000`).
   - Verify `/status.json` reports `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"psram_free_bytes"` + `"psram_used_bytes"` = `4,194,304` B (`4 MB` PSRAM intact), `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, and `"bt_enabled": true`.
4. **Gate 2 — Web UI, Phase 3 REST APIs, & TAS5805M Control**:
   - Verify `GET http://172.22.1.15/` returns `HTTP 200 OK` (`3,928` B).
   - Verify `GET http://172.22.1.15/cmd.json?act=vol55` sets `"vol_pct": 55` and `"tas_automuted": false`.
   - Verify Phase 3 REST endpoints (`GET /config.json`, `GET /scan.json`, `GET /messages.json`, `GET /commands.json`).
5. **Gate 3 — Live SlimProto Multi-Codec Playback (`FLAC`, `MP3`, etc.) on LMS `10.0.0.10`**:
   - Trigger playback on player `c8:2e:18:be:b8:34` via LMS JSON-RPC (`http://10.0.0.10:9000/jsonrpc.js`) and `cmd.json?act=preset1`:
     - Verify native `FLAC` (`file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac`) and `MP3` (*Ibiza Global Radio* `cmd.json?act=preset1`), confirming `"codec"` switches accurately (`"FLAC"` <-> `"MP3"`), `"frames_played"` advances monotonically, PSRAM is reclaimed cleanly on track changes, `"tas_pll_locked": true`, and `"i2c_nacks": 0`.
6. **Gate 4 — Live AirPlay 1 (RAOP) & Multi-Source `SourceArbiter` Preemption**:
   - Verify RTSP port `5000` (`OPTIONS * RTSP/1.0`) and live AirPlay ALAC streaming preempts LMS (`"codec": "AirPlay"`, `frames_played` advancing) and `TEARDOWN` cleanly restores LMS playback (`"codec": "MP3"`).
7. **Gate 5 — Live Spotify Connect (`cspot`) ZeroConf Discovery & Playback**:
   - Verify `GET http://172.22.1.15/?action=getInfo` returns HTTP 200 JSON with `status: 101`, `remoteName: "BASEMENT"`, and `deviceID: "142137fd329622137a149016c82e18beb834"`, and verify `cmd.json?act=spotify_play` -> `"codec": "Spotify"` and `cmd.json?act=spotify_stop` -> restores `"MP3"`.
8. **Gate 6 — Bluetooth Classic/BLE Coexistence & LMS Sibling Bridges**:
   - Run `/usr/bin/sudo -n /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` (`0% loss`) and `/usr/bin/sdptool browse CA:2E:18:BE:B8:35` (`Audio Sink 0x110b`) while Wi-Fi audio is streaming.
   - Verify LMS `10.0.0.10` (`:9000` & `:3483`) and UPnP/Cast/AirPlay bridges remain active and healthy.
9. **Deliverables**:
   - Append a concise synthesis to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`.
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-236-wave10d-live-basement-full-parity-certification.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo236_wave10d_live_basement_cert.md`.

---

## 4. Fulfillment Section
- **Overall Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo236_wave10d_live_basement_cert.md`
- **6-Gate Live Hardware Certification Summary**:
  1. **Pre-Flight & 1-Flash OTA Deployment (`[EXECUTED & VERIFIED]`)**:
     - Verified `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` size = `1,794,896` B, SHA-256 = `e830ed90119d536f8c6eadb817000e371ee3afcf7476a9dcbb0797db96b5ef74`.
     - Confirmed `"BASEMENT"` (`172.22.1.15`) in Factory recovery (`"recovery": 1`) and uploaded strictly once via `POST http://172.22.1.15/flash.json` (`File uploaded. Flashing started.`).
  2. **Gate 1 — Boot Lease & Hardware Health (`[EXECUTED & VERIFIED]`)**:
     - After 60s wait, `GET http://172.22.1.15/status.json` reported `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"uptime_ms": 47481` (`> 46,000` ms), `psram_free_bytes` (`1,521,992`) + `psram_used_bytes` (`2,672,312`) = `4,194,304` B (`4 MB` PSRAM intact), `"core1_active": true` (`"core1_loops": 184123`), `"tas_pll_locked": true`, `"i2c_nacks": 0`, `"bt_enabled": true`.
  3. **Gate 2 — Web UI, Phase 3 REST APIs, & TAS5805M Control (`[EXECUTED & VERIFIED]`)**:
     - `GET http://172.22.1.15/` -> `HTTP_CODE:200 SIZE:3928`.
     - `GET http://172.22.1.15/cmd.json?act=vol55` -> `"vol_pct": 55`, `"tas_automuted": false`.
     - Phase 3 REST endpoints (`/config.json`, `/scan.json`, `/messages.json`, `/commands.json`) all returned valid JSON (`HTTP 200`).
  4. **Gate 3 — Live SlimProto Multi-Codec Playback (`[EXECUTED & VERIFIED]`)**:
     - Native `FLAC` (`file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac`): `"codec": "FLAC"`, `frames_played` advanced monotonically (`0` -> `26,560` -> `467,136`), `"tas_automuted": false`, `"tas_pll_locked": true`, `"i2c_nacks": 0`.
     - Native `MP3` (`cmd.json?act=preset1` *Ibiza Global Radio*): `"codec": "MP3"`, reclaimed `+204,888` B of FLAC PSRAM (`psram_free_bytes`: `1,317,088` -> `1,521,976`), `frames_played` advanced monotonically (`12,288` -> `103,168`), `"tas_pll_locked": true`, `"i2c_nacks": 0`.
  5. **Gate 4 — Live AirPlay 1 (RAOP) & Multi-Source `SourceArbiter` Preemption (`[EXECUTED & VERIFIED]`)**:
     - RTSP port `5000` (`OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`) returned `RTSP/1.0 200 OK`; UDP port `6000` ALAC stream preempted LMS (`"codec": "AirPlay"`, `frames_played`: `6,050,176` -> `6,104,448` -> `6,289,792`, `"tas_pll_locked": true`, `"i2c_nacks": 0`); `TEARDOWN` (`RTSP/1.0 200 OK`) restored LMS `"codec": "MP3"` (`frames_played: 6,978,659`).
  6. **Gate 5 — Live Spotify Connect (`cspot`) ZeroConf Discovery & Playback (`[EXECUTED & VERIFIED]`)**:
     - `GET http://172.22.1.15/?action=getInfo` returned HTTP 200 JSON (`"status": 101`, `"remoteName": "BASEMENT"`, `"deviceID": "142137fd329622137a149016c82e18beb834"`).
     - `cmd.json?act=spotify_play` switched `"codec": "Spotify"`; `cmd.json?act=spotify_stop` restored `"codec": "MP3"` (`frames_played`: `7,058,659` -> `7,152,739`).
  7. **Gate 6 — Bluetooth Classic/BLE Coexistence & LMS Sibling Bridges (`[EXECUTED & VERIFIED]`)**:
     - `/usr/bin/sudo -n /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` -> `3 sent, 3 received, 0% loss`.
     - `/usr/bin/sdptool browse CA:2E:18:BE:B8:35` -> `Service Name: BASEMENT`, `"Audio Sink" (0x110b)`, `L2CAP PSM: 25`, `AVDTP 0x0103`.
     - LMS `10.0.0.10` (`:9000` & `:3483`, 15 players including CastBridge and sibling ESP32 units) verified active and healthy; final `/status.json` reported `"uptime_ms": 284522`, `"frames_played": 7970147`, `"i2c_nacks": 0`, `"tas_pll_locked": true`.
