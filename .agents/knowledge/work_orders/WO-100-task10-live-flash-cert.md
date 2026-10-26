# Work Order: WO-100 — Wave 4 Task 10 Live Wi-Fi OTA Flash (`cbc35902`) & Invariant 44 + 35s Audio Soak Certification

## Metadata
- **Work Order ID**: `WO-100`
- **Timestamp**: `2026-10-25T16:11:00-04:00`
- **Assigned Subagent Role**: Extension Certifier (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[BLOCKED / ERROR]`

## Task Goal
Deploy the remediated, `--stage-only`-certified pure-Rust firmware binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (commit `cbc35902f1d09db362ba026fc13b57aa3f2d445c`, `757,440` B, SHA-256 `c61a9df6d32dd74224c6f3d335a49a47a025fd426292a4b1a3fc26dcb422c217`) to `172.22.1.15` (`"BASEMENT"`) over Wi-Fi OTA from Factory recovery (`0x10000`) and execute the complete Wave 4 Task 10 + Invariant 44 + 35-second continuous audio playback certification.

## Target Files & Scope
- Staged OTA Binary (Read-Only): `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- Target Device: `http://172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`)
- LMS Server: `http://10.0.0.10:9000/jsonrpc.js`
- Diagnostic Log: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`

## Strict Invariant 47 & Hardware Safety Rules
1. **Extension Certifier Source-Edit Ban (Invariant 47)**: You are strictly **FORBIDDEN** from calling `edit_file` or `write_file` on any Rust source or manifest file (`rust/crates/**`).
2. **1-Flash-Attempt Circuit Breaker**: Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` once via `POST http://172.22.1.15/flash.json`. If the device rolls back to `"recovery": 1` (`"version": "I2S-4MFlash-1681"`), **STOP IMMEDIATELY** and report `[BLOCKED / ERROR]` to the orchestrator. Do not retry flashing.
3. **Never call `POST /reboot_ota.json`** and **never touch `/dev/ttyUSB0`**.

## Certification Steps
1. **Pre-Flight Check**:
   - Verify `sha256sum /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` equals `c61a9df6d32dd74224c6f3d335a49a47a025fd426292a4b1a3fc26dcb422c217`.
   - Query `curl -sS http://172.22.1.15/status.json`. Confirm `172.22.1.15` is online in Factory recovery (`"recovery": 1`). (If `"recovery": 0`, send `curl -sS -X POST http://172.22.1.15/recovery.json` and wait 6s for `"recovery": 1`).
2. **Flash Pure-Rust Firmware (`cbc35902`) to `ota_0`**:
   - Upload via `curl -sS -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`.
   - Wait 16 seconds and poll `curl -sS http://172.22.1.15/status.json` (up to 25 seconds total).
   - Verify `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
3. **Verify Hardware & Silicon Maximization Telemetry (`/status.json`)**:
   - Inspect all fields in `GET http://172.22.1.15/status.json`:
     - `psram_total_bytes` and `psram_free_bytes`
     - `core1_active` and `core1_loops`
     - `tas_fs_hz`, `tas_bck_ratio`, `tas_pll_locked`, `tas_automute`, `again_code`
     - `eq_preset`, `codec`, `settings_seq`, `i2c_acks`, `i2c_nacks: 0`
4. **Verify Interactive Web UI & `/cmd.json` Controls (Invariant 44)**:
   - Verify `GET http://172.22.1.15/` returns the full `DASHBOARD_HTML` ending in `</html>` (`0` missing bytes).
   - Execute `GET http://172.22.1.15/cmd.json?act=eq_bass` -> verify `eq_preset: "BassBoost"` and `settings_seq` increments in `/status.json`.
   - Execute `GET http://172.22.1.15/cmd.json?act=eq_loudness` -> verify `eq_preset: "Loudness"` and `settings_seq` increments in `/status.json`.
   - Execute `GET http://172.22.1.15/cmd.json?act=again6` -> verify `again_code: 6` and `settings_seq` increments in `/status.json`.
   - Execute `GET http://172.22.1.15/cmd.json?act=again8` and `GET http://172.22.1.15/cmd.json?act=eq_flat` -> verify `again_code: 8` (`-4.0 dB`) and `eq_preset: "Flat"`.
5. **Verify 35-Second Continuous Audio Playback Soak (Past the Old ~10–15s Anomaly Window)**:
   - Start playback on `"BASEMENT"` (`c8:2e:18:be:b8:34`) via `/cmd.json?act=preset1` (or LMS JSON-RPC `["c8:2e:18:be:b8:34", ["playlist", "play", "http://listenssl.ibizaglobalradio.com:8024/ibizaglobalradio.mp3"]]`).
   - Sample `GET http://172.22.1.15/status.json` every 5 seconds for **35 seconds** (`t = 0s..35s`).
   - Verify `frames_played` advances monotonically across the entire 35-second window at `~44,100 fps` (`1,500,000+` frames in 35s), `i2c_nacks == 0`, `amp_mode == "Play"`, and `tas_pll_locked == true`.
6. **Verify LMS `10.0.0.10` Sibling Bridges (Invariant 44)**:
   - Verify AirPlay `avahi-publish-service` (`_raop._tcp`) and UPnP `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` (`HTTP 200 OK`) remain active.
7. **Append Findings to `DIAGNOSTIC_LOG.md` & Fulfill Work Order**:
   - Append the certification results to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md` and update the Fulfillment Section of this Work Order.

## Fulfillment Section
- **Status**: `[BLOCKED / ERROR]` (1-Flash-Attempt Circuit Breaker triggered; device safely rolled back to Factory recovery `"recovery": 1`).
- **Pre-Flight Verification (`[EXECUTED & VERIFIED]`)**:
  - Binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`: `757,440` bytes, SHA-256 `c61a9df6d32dd74224c6f3d335a49a47a025fd426292a4b1a3fc26dcb422c217`.
  - Target `172.22.1.15` (`"BASEMENT"`): Confirmed online in Factory recovery (`"version": "I2S-4MFlash-1681"`, `"recovery": 1`, `"rssi": -51`).
- **Single OTA Flash & Automatic Gate 1 Rollback (`[BLOCKED / ERROR]`)**:
  - Uploaded via `POST http://172.22.1.15/flash.json` (`File uploaded. Flashing started.`, `HTTP 200`, `1.128s`).
  - At `t = +29s` (`16:12:53`), `172.22.1.15` rebooted into `ota_0`, executed Gate 1 (`otadata` `0xD000..0xF000` erase to `0xFF`), crashed during early boot, and rolled back cleanly to Factory recovery (`"version": "I2S-4MFlash-1681"`, `"recovery": 1`, `"rssi": -46`) at `t = +42s` (`16:13:06`).
  - Immediately halted per the 1-Flash-Attempt Circuit Breaker and Invariant 47 (zero Rust files modified).
- **Forensic Root Cause Proof (`readelf -S`, `objdump`, & `esp-hal-1.2.2` Audit)**:
  1. **`run_hardware_firmware` Stack Frame Overflow (`33,568` B frame inside `36,336` B `.stack`)**:
     - `readelf -S` on `target/xtensa-esp32-none-elf/release/louder-esp32-app` shows `.stack` (`0x3ffd7210..0x3ffe0000`) is `0x008df0` = **`36,336` bytes**.
     - Disassembly of `run_hardware_firmware` (`0x4011cfa0`) shows `entry a1, 32` + `l32r a8, [0x40105914]` (`a8 = 0x00008300 = 33,536` B) + `sub a1, a1, a8`, allocating a **`33,568`-byte stack frame** on entry and leaving only **`2,768` bytes (`2.70 KB`)** above `_bss_end` (`0x3ffd7210`) for all callees and ISRs!
     - Cause of the `+20 KB` stack frame bloat in Task 7 (`5a837d44`):
       - `NvsFlashBackend` (`hal.rs:466-544`) embeds `scratch: [u8; 4096]` (`4,108` B), its un-inlined `NvsFlashBackend::new()` creates a second `4,108` B temporary on the stack, and its inlined `commit_store()` allocates `let mut b = *self.flash.read_sector(1)?` (`[u8; 4096]`) on the stack (`~12.3 KB` total).
       - `MemConfigStore::<32>` (`squeezelite-config/src/lib.rs:185`) embeds `heapless::Vec<(String<32>, Vec<u8, 128>), 32>` (`32 * 168 = 5,380` B) + `MemConfigStore::new()` temporary (`~10.7 KB` total).
     - Gate 1 (`flash.erase(0xD000, 0xF000)`) succeeded near the top of `run_hardware_firmware`, pre-arming Factory recovery (`0x10000`). Subsequent deeper calls (`NvsFlashBackend::load_store`, `Psram::new`, `WifiController::new`) pushed `sp` below `0x3ffd7210` into `.bss` (`g_cnxMgr` at `0x3ffd4c98`), crashing the MCU and triggering clean Gate 1 rollback.
  2. **`esp_hal::psram::Psram::new` Live `SPI0` XIP Cache Reclocking & `esp_alloc` 64 KB Heap Starvation**:
     - In `esp-hal-1.2.2/src/psram/esp32.rs`, `PsramConfig::default()` selects `PsramCacheSpeed::PsramCacheF80mS40m`. Neither `psram_gpio_config` (`esp32.rs:981`) nor `psram_cache_init` (`esp32.rs:480`) is marked `#[ram]`, and both reconfigure `SPI0` (`_SPI_CACHE_PORT`) to `80 MHz` (`_SPI_80M_CLK_DIV`) with `+2` extra dummy cycles while CPU 0 is executing XIP instructions from a `40 MHz DIO` bootloader image (`0xE9 ... 0x02 0x20`).
     - Additionally, if `psram_avail_bytes < 655_360`, `alloc::vec![0u8; 8192]` x 2 in `main.rs:1167-1168` allocates `16,384` B out of the `64 KB` internal DRAM `esp_alloc` Wi-Fi heap before `WifiController::new`, starving `esp-radio` (which requires `~50 KB` of the `64 KB` heap).
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo100_extension_certifier.md`
