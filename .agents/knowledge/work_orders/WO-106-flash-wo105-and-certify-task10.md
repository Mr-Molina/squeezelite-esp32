# Work Order: WO-106 — Flash `WO-105` (`91dbd9d4`) & Complete Wave 4 Task 10 Live Hardware Certification

## Metadata
- **Work Order ID**: `WO-106`
- **Timestamp**: `2026-10-25T18:11:00-04:00`
- **Assigned Subagent Role**: Extension Certifier (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[BLOCKED / ERROR]` (Steps 1, 2, & 3.1 Dual-Core/PSRAM/Wi-Fi `[EXECUTED & VERIFIED]`; Step 3.2 `GET /` `[BLOCKED / ERROR]` due to `SmolDevice` `max_transmission_unit = 1500` > `esp-radio` `MTU = 1492` slice bounds panic at `0x4011a75e`)

## Task Goal
Flash the verified `WO-105` OTA binary (`/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, commit `91dbd9d4eaa4ed0580346e3641e641217eac48e5`, `756,496` bytes, SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`) to `172.22.1.15` (`"BASEMENT"`, currently in Factory recovery `"recovery": 1`) and execute the complete Wave 4 Task 10 live hardware certification suite (`GET /status.json`, `GET /`, interactive `/cmd.json` EQ & AGAIN mutations + `settings_seq` persistence, 35-second continuous audio playback soak, and LMS `10.0.0.10` UPnP/AirPlay sibling bridge verification).

## Hard Safety & Role Invariants (MANDATORY — Invariant 47)
- **Source-Edit Ban**: You are strictly **FORBIDDEN** from modifying any `.rs` or `Cargo.toml` file under `/home/console-admin/Github/squeezelite-esp32/rust/crates/**` or running `cargo build`.
- **1-Flash-Attempt Circuit Breaker**: Execute at most **ONE** `POST /flash.json` upload.
- **Anti-Brick Rules**:
  - NEVER call `POST /reboot_ota.json`.
  - NEVER touch `/dev/ttyUSB0`.

## Step-by-Step Execution Procedure

### Step 1: Pre-Flight SHA-256 & Recovery Status Verification
- Verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` has size `756,496` bytes and SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`.
- Verify `curl -sS --max-time 3 http://172.22.1.15/status.json` returns `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).

### Step 2: Single OTA Flash of `91dbd9d4` (`WO-105`)
- Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (`756,496` bytes) via `POST http://172.22.1.15/flash.json`.
- Wait 18–28 seconds and poll `GET http://172.22.1.15/status.json` (timeout 3s, 3s interval, up to 15 polls).

### Step 3: Full Wave 4 Task 10 Live Certification Gates
1. **`GET http://172.22.1.15/status.json` (`~725` B JSON)**:
   - Verify `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
   - Verify 4 MB PSRAM fields (`"psram_free_bytes"`, `"psram_used_bytes": 655360`).
   - Verify Dual-Core fields (`"core1_active": true`, `"core1_loops" > 0`).
   - Verify TI `TAS5805M` live hardware telemetry (`"tas_fs_hz"`, `"tas_bck_ratio"`, `"tas_pll_locked"`, `"again_code"`, `"eq_preset"`, `"settings_seq"`, `"i2c_nacks": 0`).
2. **`GET http://172.22.1.15/` Web UI Dashboard**:
   - Verify `HTTP 200 OK` and full HTML ending in `</html>` (`4,827` bytes, `0` missing bytes).
3. **Interactive `/cmd.json` Mutations (Invariant 44)**:
   - Execute and verify state read-back in `/status.json` after each command (allow `~0.5s` between commands):
     - `GET http://172.22.1.15/cmd.json?act=eq_bass` -> `"eq_preset": "BassBoost"`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=eq_loudness` -> `"eq_preset": "Loudness"`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=again6` -> `"again_code": 6`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=again8` -> `"again_code": 8`, `"settings_seq"` increments
     - `GET http://172.22.1.15/cmd.json?act=eq_flat` -> `"eq_preset": "Flat"`, `"settings_seq"` increments
4. **35-Second Continuous Audio Playback Soak Test**:
   - Ensure `"BASEMENT"` (`c8:2e:18:be:b8:34`) is playing *Ibiza Global Radio* on LMS (`10.0.0.10:9000/jsonrpc.js`).
   - Poll `GET http://172.22.1.15/status.json` every 5 seconds for 35 seconds (8 samples: `t = 0, 5, 10, 15, 20, 25, 30, 35s`).
   - Verify `frames_played` advances monotonically across all 35 seconds at `~44,100 fps`, `i2c_nacks == 0`, `amp_mode == "Play"`, and `core1_loops` increases monotonically.
5. **LMS `10.0.0.10` Sibling Bridge Verification**:
   - Verify UPnP `MediaRenderer.xml` returns `HTTP 200 OK` (`3,105` B) and AirPlay `avahi-publish-service` (`_raop._tcp`) is active on `10.0.0.10`.

## Deliverables
- Append fulfillment section to this work order (`WO-106-flash-wo105-and-certify-task10.md`).
- Append Entry 57 to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo106_extension_certifier.md`.

## Fulfillment Section
- **Subagent**: `Extension Certifier` (`fbf04413-8be7-494e-9e3b-99f44aa00f31`)
- **Timestamp**: `2026-10-25T18:30:00-04:00`
- **Status**:
  - **Step 1 (Pre-Flight SHA-256 & Recovery Status Verification)**: `[EXECUTED & VERIFIED]`
    - Verified `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`: size `756,496` bytes, SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`.
    - Confirmed `172.22.1.15/status.json` in Factory recovery (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -47`).
  - **Step 2 (Single OTA Flash of `91dbd9d4` `WO-105`)**: `[EXECUTED & VERIFIED]`
    - Uploaded `louder-esp32-app-basement-ota.bin` (`91dbd9d4`) once via `POST http://172.22.1.15/flash.json` (`1.68s`, `File uploaded. Flashing started.`).
  - **Step 3.1 (`GET /status.json` Live Verification)**: `[EXECUTED & VERIFIED]` (Dual-Core `APP_CPU`, `4 MB PSRAM`, & Local `-51 dBm` Wi-Fi verified; `tas_pll_locked: false` noted below)
    - `[Poll 2 @ +25.7s]` (`uptime_ms = 12,848`) and `[Gate 3.1 @ +62s]` (`uptime_ms = 46,227`) returned `HTTP 200 OK` (`730` B / `731` B):
      `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-Rust","recovery":0,"rust_engine":true,"otadata_prearmed_factory":true,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-51,"lms_cport":9090,"lms_port":9000,"lms_ip":"10.0.0.10","frames_played":0,"amp_mode":"Play","vol_pct":80,"i2c_acks":191,"i2c_nacks":0,"uptime_ms":46227,"psram_free_bytes":3538944,"psram_used_bytes":655360,"core1_active":true,"core1_loops":493383236,"codec":"PCM","codec_sample_rate":44100,"codec_channels":2,"codec_bit_depth":16,"eq_preset":"Flat","eq_enabled":false,"tas_fs_hz":48000,"tas_bck_ratio":64,"tas_pll_locked":false,"tas_automuted":false,"again_code":8,"settings_seq":0}`
    - Confirmed `WO-105` resolved both `WO-104` bugs:
      1. `"core1_active": true` and `"core1_loops": 132,229,346 -> 493,383,236` (`APP_CPU` running at full speed).
      2. `"rssi": -51` dBm (up `+31 dB` from `-82 dBm` in `WO-104`).
      3. Continuous runtime stability past `46.2 seconds` (`> 45s` boot lease watchdog).
  - **Step 3.2–3.5 (`GET /` Web UI Dashboard & Subsequent Gates)**: `[BLOCKED / ERROR]`
    - At `uptime_ms = 46,727 ms` (`0.5s` after `[Gate 3.1]`), sending `GET http://172.22.1.15/` caused an immediate panic and rollback to Factory recovery (`"recovery": 1`, `"rssi": -50`). Halted per 1-Flash-Attempt Circuit Breaker.
- **Root Causes Proved from ELF Disassembly & Source**:
  1. **Root Cause 1 — `SmolDevice::capabilities()` `max_transmission_unit = 1500` Exceeds `esp-radio` `WIFI_MTU = 1492`, Triggering `slice_index_fail` Panic on `GET /` (`src/main.rs:539` & `esp-radio-1.0.0-beta.1/src/wifi/mod.rs:117, 2019-2020`)**:
     - In `esp-radio-1.0.0-beta.1/esp_config.yml:12-15` and `src/wifi/mod.rs:117, 2019-2020`, `const MTU: usize = esp_config_int!(usize, "ESP_RADIO_CONFIG_WIFI_MTU")` is **`1,492` bytes**, and `WifiTxToken::consume_token(self, len: usize, f: F)` indexes a fixed stack array:
       ```rust
       let mut buffer: [u8; 1492] = [0u8; 1492];
       let buffer = &mut buffer[..len];
       ```
     - In `src/main.rs:539`, `SmolDevice::capabilities()` sets `caps.max_transmission_unit = 1500` (`Medium::Ethernet`), so `smoltcp` calculates `ip_mtu = 1500 - 14 = 1486` and TCP `local_mss = 1486 - 20 - 20 = 1446`.
     - Before `WO-103`, `http_tx_buf` was `[u8; 1024]`, so `http_sock` never emitted an Ethernet frame larger than `14 + 20 + 20 + 1024 = 1,078` B (`<= 1,492`). After `WO-103` increased `STATIC_HTTP_TX_BUF` to `[u8; 1536]`, `GET /status.json` (`879` B -> `933` B Ethernet frame `<= 1,492`) still fits, but `GET /` (`4,827` B `DASHBOARD_HTML`) fills `STATIC_HTTP_TX_BUF` with `1,536` bytes.
     - On the next `smol_iface.poll()`, `smoltcp` dispatches a full-MSS TCP segment (`1,446` B payload + `20` B TCP + `20` B IPv4 + `14` B Ethernet = **`1,500` bytes**) and calls `tx_token.consume(1500, ...)` -> `WifiTxToken::consume_token(1500, ...)`, which immediately panics on `&mut buffer[..1500]` (`range end index 1500 out of range for slice of length 1492`)!
     - Verified directly in the ELF disassembly of `InterfaceInner::dispatch_ip<SmolTxToken>` (`0x4011a2b8`):
       - `0x4011a75e`: `movi a12, 1492 (0x05d4)`
       - `0x4011a764`: `l32r a8, [0x40104e3c]` -> `core::slice::index::slice_index_fail` (`0x40150f20`).
     - **Fix**: Set `caps.max_transmission_unit = 1492;` in `SmolDevice::capabilities()` (`src/main.rs:539`).
  2. **Root Cause 2 — `Esp32HardwareI2sDma::new` Skips Initial Fractional Clock Divider `(14, 11, 63)` for `44,100 Hz` (`src/main.rs:714, 854`)**:
     - `Esp32HardwareI2sDma::new(transfer, 44_100)` sets `self.sample_rate_hz = 44_100` without programming `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`), and `set_clock_rate(44_100)` early-returns when `self.sample_rate_hz == sample_rate_hz`, leaving `esp-hal`'s integer divider `(14, 0, 1)` (`44,642.86 Hz`, `+1.23%` error) active at boot. This causes the `TAS5805M` clock detector to report `tas_fs_hz: 48000` and `tas_pll_locked: false`.
     - **Fix**: In `Esp32HardwareI2sDma::new` (`src/main.rs:714`), initialize `sample_rate_hz: 0` and call `let _ = s.set_clock_rate(sample_rate_hz);` so `I2S0_CLKM_CONF_REG` is programmed to `(14, 11, 63)` (`44,100.29 Hz`, `+6.7 ppm`) immediately on boot.
- **Handoff Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo106_extension_certifier.md`
