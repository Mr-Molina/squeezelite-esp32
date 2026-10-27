# Subagent Summary: `WO-147` Authorized Tier-3 Live Wi-Fi OTA Flash & End-to-End Certification

- **ID**: `WO-147`
- **Timestamp**: `2026-10-26T06:53:00-04:00`
- **Subagent Role**: Extension Certifier (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`
- **Target Device**: `BASEMENT` (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`)
- **LMS Server**: `10.0.0.10:9000` (CLI `9090`)

---

## 1. Execution Overview & Evidence

### Step 1: Pre-Flight Verification & Transition to Factory Recovery (`0x10000`) `[EXECUTED & VERIFIED]`
- **Binary Image Verified**:
  - Path: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
  - Exact Size: `759,760` bytes
  - SHA-256: `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`
- **Recovery Reboot**:
  - Sent `POST http://172.22.1.15/recovery.json` (`{"status":"rebooting_to_recovery","target_partition":"factory_0x10000"}`).
  - Polled `http://172.22.1.15/status.json` and verified Factory recovery mode:
    ```json
    {
      "project_name": "Squeezelite-ESP32",
      "platform_name": "ESP32",
      "version": "I2S-4MFlash-1681",
      "recovery": 1,
      "ip": "172.22.1.15",
      "ssid": "iot",
      "rssi": -53
    }
    ```

### Step 2: Single-Attempt OTA Flash & 45s Gate 3 Boot Lease `[EXECUTED & VERIFIED]`
- **OTA Flash Upload**:
  - Enforced 1-Flash Circuit Breaker via `curl -sS -X POST --data-binary @... http://172.22.1.15/flash.json`.
  - Recovery response: `"File uploaded. Flashing started."`.
  - Zero interaction with `/dev/ttyUSB0`.
- **Gate 3 Boot Lease Verification**:
  - Polled `http://172.22.1.15/status.json` past 45s boot lease (`uptime_ms`: `46,104` ms):
  - `"recovery": 0`
  - `"rust_engine": true`
  - `"otadata_prearmed_factory": true`
  - `"version": "I2S-4MFlash-Rust"`
  - `"core1_active": true` (`162,537` loops)

### Step 3: Core 1 `nop` Loop Throttling & `GET /` Web UI `[EXECUTED & VERIFIED]`
- **`HW-PWR-001` / `CODE-MAIN-005` Throttling Evidence**:
  - Snapshot 1: `uptime_ms = 89,652`, `core1_loops = 317,604`
  - Snapshot 2 (5.056s later): `uptime_ms = 94,708`, `core1_loops = 335,610`
  - $\Delta\text{loops} = 18,006$ over $5.056\text{ s} \implies \mathbf{3,561.3\text{ loops/sec}}$.
  - Massive power/thermal reduction compared to un-throttled spin (`~11,650,000 loops/sec`).
- **Dashboard Web UI Evidence**:
  - `GET http://172.22.1.15/` returned `HTTP 200 OK`, exact length `3,928` bytes, terminating cleanly in `</html>`.

### Step 4: LMS Volume Synchronization `[EXECUTED & VERIFIED]`
- **`HW-CFG-002` / `CODE-MAIN-002` / `CODE-TAS-001` Evidence**:
  - Set LMS mixer volume to 35: `["c8:2e:18:be:b8:34", ["mixer", "volume", "35"]]`.
    - `172.22.1.15` updated to `"vol_pct": 53`, `settings_seq: 8` (TAS5805M calibrated 0.5 dB attenuation curve, no longer stuck at `80`!).
  - Set LMS mixer volume to 50: `["c8:2e:18:be:b8:34", ["mixer", "volume", "50"]]`.
    - `172.22.1.15` updated to `"vol_pct": 65`, `settings_seq: 10`.

### Step 5: Live Audio Playback & Clock Rate Verification `[EXECUTED & VERIFIED]`
- **`HW-EMB-001`, `HW-DRV-002`, `HW-NET-001` Evidence**:
  - Playback triggered for *Ibiza Global Radio* (`http://cdn-peer012.streaming-pro.com:8024/ibizaglobalradio.mp3`).
  - Sampled `status.json` over 12.068 seconds across 5 consecutive samples:
    - Delta frames: `531,968` frames
    - Effective playback rate: $\mathbf{44,080.9\text{ frames/sec}}$ (target `[44,050, 44,150]`, verified up from `41,667` fps prior to the `I2S0_CLKM_CONF_REG` `div_b`/`div_a` bitfield correction).
    - `"tas_fs_hz": 44100` (`detected_sample_rate_hz_with_hint(44100)` verified resolving `FS_MON = 0x09` to `44100`).
    - `"tas_pll_locked": true`
    - `"tas_automuted": false`
    - `"i2c_nacks": 0`
  - LMS TCP stream socket metrics on `10.0.0.10`:
    - `ESTAB 10.0.0.10:9000 -> 172.22.1.15:62933` actively streaming audio bytes (`bytes_sent: 787,830`, `bbr` pacing).

### Step 6: Settings Persistence Across Controlled `POST /reboot.json` `[EXECUTED & VERIFIED]`
- **`HW-STORAGE-001` / `HW-EMB-002` Evidence**:
  - Set Bass preset: `GET http://172.22.1.15/cmd.json?act=eq_bass` -> verified `"eq_preset": "Bass"`, `"eq_enabled": true`, `settings_seq: 14`.
  - Sent `POST http://172.22.1.15/reboot.json` -> response `{"status":"rebooting_to_ota_0","target_partition":"ota_0_0x160000"}`.
  - Device rebooted and came online at `uptime_ms: 16,229`:
    - `"recovery": 0` (Confirmed `ota_0` persisted and selected by bootloader).
    - `"eq_preset": "Bass"`, `"eq_enabled": true`, `settings_seq: 17` (Confirmed `settings_scratch` committed to NOR Flash `0x3FE000` and restored).
  - Restored to Flat: `GET http://172.22.1.15/cmd.json?act=eq_flat` -> verified `"eq_preset": "Flat"`, `"eq_enabled": false`, `settings_seq: 18`.

### Step 7: LMS `10.0.0.10` Sibling Bridges `[EXECUTED & VERIFIED]`
- **UPnP/DLNA**: `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` returned `HTTP/1.1 200 OK` (`3,105` bytes).
- **CastBridge**: PID 772 `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/CastBridge/Bin/squeeze2cast-linux-aarch64-static` running.
- **ShairTunes2W AirPlay**: PID 2071 `/bin/avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 34027 34027 ... am=shairtunes2` running.

---

## 2. Files Modified / Created (Rule 17)

1. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261026_wo147_extension_certifier.md` (Created: Subagent summary report).
2. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-147-live-ota-flash-certification.md` (Modified: Appended Fulfillment Summary).
3. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` (Modified: Updated status to `[EXECUTED & VERIFIED]`).

## 3. Known Risks & Untested Assumptions
- None identified. Target `172.22.1.15` is running live pure-Rust firmware on `ota_0` with verified 44.1 kHz playback rate, PLL lock, zero I2C errors, full LMS volume sync, and dual-core loop throttling.
