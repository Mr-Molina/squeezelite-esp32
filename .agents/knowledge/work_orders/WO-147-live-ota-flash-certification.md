# Work Order `WO-147`: Authorized Tier-3 Live Wi-Fi OTA Flash & End-to-End Certification on `[BASEMENT]` (`172.22.1.15`)

- **ID**: `WO-147`
- **Timestamp**: `2026-10-26T06:43:00-04:00`
- **Subagent Role**: Extension Certifier (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Safety Invariants
- **Target Device**: `"BASEMENT"` (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`) & LMS Bridge (`10.0.0.10:9000`)
- **Staged OTA Binary**: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
  - Expected Size: `759,760` bytes
  - Expected SHA-256: `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`
- **Hardware Isolation**: **NEVER touch `/dev/ttyUSB0`**.
- **Strict 1-Flash-Attempt Circuit Breaker**: You may call `POST http://172.22.1.15/flash.json` **at most ONCE**. If `172.22.1.15` rolls back to Factory recovery (`"recovery": 1`) after flashing, HALT immediately and report the exact failure telemetry without re-flashing.

## 2. Step-by-Step Certification Protocol (`verify-autonomous-work.md`)
1. **Step 1 — Pre-Flight Verification & Transition to Factory Recovery (`0x10000`)**:
   - Verify `sha256sum` (`b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`) and size (`759,760` B) of `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`.
   - Send `curl -sS -X POST http://172.22.1.15/recovery.json` to reboot `172.22.1.15` into the Factory recovery partition (`0x10000`).
   - Poll `curl -sS --max-time 3 http://172.22.1.15/status.json` until `"recovery": 1` and `"version": "I2S-4MFlash-1681"` are confirmed.
2. **Step 2 — Single-Attempt OTA Flash to `ota_0` (`0x160000`) & 45s Gate 3 Boot Lease**:
   - Upload the binary once via:
     `curl -sS -X POST --data-binary @/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`
   - Poll `GET http://172.22.1.15/status.json` every 5 seconds for 55 seconds until `uptime_ms > 46,000` (past the 45s Gate 3 boot lease) and confirm `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
3. **Step 3 — Verify `HW-PWR-001` (`CODE-MAIN-005`) Core 1 `nop` Loop Throttling & `GET /` Web UI**:
   - Record two `GET http://172.22.1.15/status.json` snapshots separated by `~5.0` seconds and compute $\Delta\text{core1\_loops} / \Delta\text{uptime\_s}$. Verify `"core1_active": true` and that the loop rate is throttled (`<< 11,650,000` loops/sec from the old un-throttled loop).
   - Verify `GET http://172.22.1.15/` returns `HTTP 200 OK` with full `DASHBOARD_HTML` (`3,928` bytes ending in `</html>`).
4. **Step 4 — Verify `HW-CFG-002` (`CODE-MAIN-002` / `CODE-TAS-001`) LMS Volume Synchronization**:
   - Set LMS mixer volume for `c8:2e:18:be:b8:34` to `35` via `http://10.0.0.10:9000/jsonrpc.js` (`["c8:2e:18:be:b8:34", ["mixer", "volume", "35"]]`), wait 1 second, and verify `GET http://172.22.1.15/status.json` updates `"vol_pct"` (no longer stuck at `80`!). Then set LMS mixer volume to `50` (`["c8:2e:18:be:b8:34", ["mixer", "volume", "50"]]`) and verify `"vol_pct"` updates accordingly.
5. **Step 5 — Verify `HW-EMB-001` (`CODE-MAIN-001`), `HW-DRV-002` (`CODE-TAS-002`), & `HW-NET-001` (`CODE-MAIN-008`) Live Audio Playback & Clock Rate**:
   - Trigger *Ibiza Global Radio* playback on `c8:2e:18:be:b8:34` via LMS JSON-RPC (`["c8:2e:18:be:b8:34", ["button", "playPreset_1"]]` or `["c8:2e:18:be:b8:34", ["playlist", "play", "http://cdn-peer012.streaming-pro.com:8024/ibizaglobalradio.mp3"]]`).
   - Sample `GET http://172.22.1.15/status.json` over a 15-second window (every 3 seconds) after playback starts and verify:
     1. $\Delta\text{frames\_played} / \Delta\text{uptime\_s} \approx 44,100\text{ fps}$ (`[44,050, 44,150]` fps, up from `41,667` fps before the `I2S0_CLKM_CONF_REG` `div_b`/`div_a` fix!).
     2. `"tas_fs_hz": 44100` (proving `detected_sample_rate_hz_with_hint(44100)` resolves `FS_MON = 0x09` to `44100`!).
     3. `"tas_pll_locked": true`, `"tas_automuted": false`, `"i2c_nacks": 0`.
   - On `10.0.0.10`, check `ss -tni dst 172.22.1.15` to observe the TCP audio stream socket metrics.
6. **Step 6 — Verify `HW-STORAGE-001` (`CODE-MAIN-003`) & `HW-EMB-002` (`CODE-HTTP-001`) Settings Persistence Across Controlled `POST /reboot.json`**:
   - Send `GET http://172.22.1.15/cmd.json?act=eq_bass` and verify `GET http://172.22.1.15/status.json` shows `"eq_preset": "Bass"`, `"eq_enabled": true`, and incremented `"settings_seq"`.
   - Send `POST http://172.22.1.15/reboot.json` (`{"status":"persisting_ota0_and_rebooting","target_partition":"ota_0_0x160000"}`).
   - Wait ~18 seconds for `172.22.1.15` to reboot and reconnect to Wi-Fi, then query `GET http://172.22.1.15/status.json` and verify:
     1. `"recovery": 0` (confirming `/reboot.json` persisted `ota_0` and rebooted into `ota_0` rather than Factory recovery!).
     2. `"eq_preset": "Bass"` and `"eq_enabled": true` (confirming `settings_scratch` was committed to NOR Flash `0x3FE000` before `software_reset()` and reloaded on boot!).
   - Restore `"eq_preset": "Flat"` via `GET http://172.22.1.15/cmd.json?act=eq_flat` and verify `"eq_preset": "Flat"` and active playback.
7. **Step 7 — Verify LMS `10.0.0.10` Sibling Bridges**:
   - Confirm `UPnP/DLNA` (`http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`), `ShairTunes2W` AirPlay (`avahi-publish-service`), and `CastBridge` remain active.

## 3. Handoff Requirements (Rule 15, Rule 17, Rule 21)
1. Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261026_wo147_extension_certifier.md` (Rule 17) and append the Fulfillment Summary below (Rule 15).
2. Update `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` to mark `WO-147` as `[EXECUTED & VERIFIED]`.

## 4. Fulfillment Section

### 4.1 Execution Summary (`[EXECUTED & VERIFIED]`)
- **Execution Date**: `2026-10-26T06:53:00-04:00`
- **Subagent**: Extension Certifier (`Model: "pro"`, `TypeName: "self"`)
- **Staged Binary**: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` (`759,760` B, SHA-256 `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`)
- **Hardware Isolation**: STRICT ZERO INTERACTION with `/dev/ttyUSB0`.
- **Circuit Breaker**: Exactly 1 flash attempt executed via `POST http://172.22.1.15/flash.json` after verifying Factory recovery mode (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`).

### 4.2 Gate Verification Checklist
1. **Step 1 — Pre-Flight & Recovery Transition**: `[EXECUTED & VERIFIED]`
   - Binary checksum and size confirmed: `759,760` bytes, `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`.
   - `POST /recovery.json` executed; polled until `"recovery": 1`, `"version": "I2S-4MFlash-1681"`.
2. **Step 2 — Single-Attempt OTA Flash & 45s Gate 3 Boot Lease**: `[EXECUTED & VERIFIED]`
   - `POST /flash.json` uploaded binary; device flashed and rebooted into `ota_0`.
   - Polled past 45s boot lease (`uptime_ms`: `46,104` ms); confirmed `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
3. **Step 3 — Core 1 `nop` Loop Throttling & `GET /` Web UI**: `[EXECUTED & VERIFIED]`
   - Measured loop rate: `18,006` loops over `5.056` seconds = **3,561.3 loops/sec** (`<< 11,650,000` un-throttled rate).
   - `GET /` returned `HTTP 200 OK`, `3,928` bytes, ending with `</html>`.
4. **Step 4 — LMS Volume Synchronization**: `[EXECUTED & VERIFIED]`
   - LMS mixer volume set to 35: `172.22.1.15` updated to `"vol_pct": 53`, `settings_seq: 8` (TAS5805M calibrated 0.5 dB curve; no longer stuck at 80!).
   - LMS mixer volume set to 50: `172.22.1.15` updated to `"vol_pct": 65`, `settings_seq: 10`.
5. **Step 5 — Live Audio Playback & Clock Rate Verification**: `[EXECUTED & VERIFIED]`
   - *Ibiza Global Radio* streaming active.
   - 12.068-second sample: `531,968` frames played $\implies \mathbf{44,080.9\text{ frames/sec}}$ (in exact range `[44,050, 44,150]`, up from `41,667` fps before `I2S0_CLKM_CONF_REG` `div_b`/`div_a` bitfield fix).
   - `"tas_fs_hz": 44100`, `"tas_pll_locked": true`, `"tas_automuted": false`, `"i2c_nacks": 0`.
   - `10.0.0.10:9000 -> 172.22.1.15:62933` TCP socket active (`bytes_sent: 787,830`, `bbr` pacing).
6. **Step 6 — Settings Persistence Across Controlled `POST /reboot.json`**: `[EXECUTED & VERIFIED]`
   - Set `"eq_preset": "Bass"` (`settings_seq: 14`).
   - `POST /reboot.json` committed `settings_scratch` to NOR Flash `0x3FE000` and rebooted into `ota_0` (`recovery: 0`).
   - Restored state: `"eq_preset": "Bass"`, `"eq_enabled": true`, `settings_seq: 17`.
   - Restored `"eq_preset": "Flat"` (`settings_seq: 18`).
7. **Step 7 — LMS `10.0.0.10` Sibling Bridges**: `[EXECUTED & VERIFIED]`
   - `UPnP/DLNA`: `HTTP 200 OK` (`3,105` bytes).
   - `CastBridge`: PID 772 active.
   - `ShairTunes2W` AirPlay: PID 2071 active (`_raop._tcp 34027`).

### 4.3 Summary & Handoff Paths
- Subagent Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261026_wo147_extension_certifier.md`
- Work Order Ledger: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
