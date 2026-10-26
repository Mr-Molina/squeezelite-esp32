# Work Order: WO-105 — Fix Core 1 (`APP_CPU`) Warm-Reboot Park/MMU Startup, Multi-AP `ScanMethod::AllChannels` Selection, & `pending_http_body` Timeout (`--stage-only`)

## Metadata
- **Work Order ID**: `WO-105`
- **Timestamp**: `2026-10-25T18:02:00-04:00`
- **Assigned Subagent Role**: Rust App Dual-Core & Wi-Fi Implementer (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
Remediate the 3 root causes isolated in `WO-104` (`DIAGNOSTIC_LOG.md` Entry 56) while preserving the proven `WO-101`/`WO-103` stack-frame (`<= 9 KB`) and `1,536`-byte HTTP buffer architecture:
1. **Fix `"core1_active": false, "core1_loops": 0` on Warm Reboot from Dual-Core ESP-IDF Recovery**:
   - In `esp-hal 1.2.2` on `esp32`, `esp_hal::init()` does not park `Cpu::AppCpu`, so after warm reboot from dual-core ESP-IDF (`I2S-4MFlash-1681`), `is_running(Cpu::AppCpu)` returns `true`, causing Gate 1 `FlashStorage::multicore_auto_park()` to unpark a rogue `APP_CPU` and causing `cpu_ctrl.start_app_core(...)` to return `Err(Error::CoreAlreadyRunning)`.
   - Immediately after `let peripherals = esp_hal::init(config);` (BEFORE Gate 1 `FlashStorage`), create `let mut cpu_ctrl = CpuControl::new(peripherals.CPU_CTRL);` and call `unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }`.
   - Increase `APP_CORE_STACK` from `Stack<512>` to `Stack<2048>` (`src/main.rs:951`).
   - Immediately before `cpu_ctrl.start_app_core(...)`, sync `DPORT_APP_FLASH_MMU_TABLE` (`0x3FF1_2000..0x3FF1_2400`) and `DPORT_APP_CACHE_CTRL1_REG` (`0x3FF0_005C`) from `PRO_CPU` (`0x3FF1_0000` and `0x3FF0_0044`) while `APP_CPU` cache (`0x3FF0_0058` bit 3) is disabled, so `APP_CPU` can fetch `start_core1_init` / `start_core1_run` from XIP Flash `.text` without a cache-mask fault before entering `#[esp_hal::ram] fn app_core_loop()`.
2. **Fix Distant Multi-AP Association (`"rssi": -82` vs `-47 dBm`) & `1,000 ms` Scan Interruption**:
   - In `src/main.rs:1331-1335`, configure `StationConfig::default()` with:
     - `.with_scan_method(esp_radio::wifi::sta::ScanMethod::AllChannels)` (so `WIFI_CONNECT_AP_BY_SIGNAL` scans all 11 channels and picks the local `-47 dBm` Basement AP `02:EC:DA:B7:C8:B4` instead of stopping on the first `-82 dBm` channel)
     - `.with_beacon_timeout(30)` (`3.0s` instead of `600ms`)
     - `.with_failure_retry_cnt(3)`
   - Set `wifi_controller.set_max_tx_power(52)` (`13 dBm`).
   - In `src/main.rs:1437`, change the `!wifi_controller.is_connected()` poll cadence from `1_000 ms` to `5_000 ms` (`last_connect_attempt_ms == 0 || now_ms.saturating_sub(last_connect_attempt_ms) >= 5_000`, setting `last_connect_attempt_ms = now_ms.max(1)`) so `ScanMethod::AllChannels` (`~1.3s`) + retries are never interrupted mid-scan.
3. **Fix `pending_http_body` Stalled-Socket Deadline (`src/main.rs:1503-1545`)**:
   - Add `let mut pending_http_started_ms: u64 = 0;`. Set `pending_http_started_ms = now_ms;` whenever `pending_http_body` is populated (`!static_body.is_empty()`).
   - Inside `if !pending_http_body.is_empty()`, if `now_ms.saturating_sub(pending_http_started_ms) >= 2_500` or `matches!(http_sock.state(), tcp::State::CloseWait | tcp::State::TimeWait | tcp::State::Closing | tcp::State::LastAck | tcp::State::FinWait1 | tcp::State::FinWait2 | tcp::State::Closed)`, clear `pending_http_body = &[]`, call `http_sock.abort()`, and set `pending_http_closing = false`.

## Target Files & Scope
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

## Required Verification Gates (`--stage-only`)
1. Back up `src/main.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.pre_wo105.bak` (Rule 35).
2. `cargo test --workspace` (`exit code 0`, 219 tests pass).
3. `cargo clippy --workspace --all-targets -- -D warnings` (`exit code 0`).
4. `source /home/console-admin/export-esp.sh && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`).
5. `xtensa-esp32-elf-readelf -S`: verify `.stack >= 35,000` B and `.data <= 20,000` B.
6. `xtensa-esp32-elf-objdump -d`: verify `run_hardware_firmware` stack frame (`entry a1, ...`) is `<= 9,000` B.
7. Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and verify `verify_staged_ota_bin.py` (`[STAGE-ONLY AUDIT PASS]`).
8. Commit to `MK5-Rust`, append Fulfillment Section, and write summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo105_core1_wifi_implementer.md`.

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `91dbd9d4eaa4ed0580346e3641e641217eac48e5` (`91dbd9d4`)
- **Rule 35 Pre-Modification Backup**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.pre_wo105.bak`
- **Summary of Changes**:
  1. **Core 1 (`APP_CPU`) Warm-Reboot Park + MMU/Cache Sync (`src/main.rs:951, 1126-1131, 1257-1284`)**:
     - Increased `APP_CORE_STACK` from `Stack<512>` to `Stack<2048>`.
     - Immediately after `let peripherals = esp_hal::init(config);` (before Gate 1 `FlashStorage::new(peripherals.FLASH).multicore_auto_park()`), instantiated `let mut cpu_ctrl = CpuControl::new(peripherals.CPU_CTRL);` and called `unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }`.
     - Immediately before `cpu_ctrl.start_app_core(...)`, synchronized `DPORT_APP_FLASH_MMU_TABLE` (`0x3FF1_2000..0x3FF1_2400`) and `DPORT_APP_CACHE_CTRL1_REG` (`0x3FF0_005C` bits `0..5`) from `PRO_CPU` (`0x3FF1_0000` and `0x3FF0_0044`) while `DPORT_APP_CACHE_CTRL_REG` (`0x3FF0_0058` bit 3) is disabled.
  2. **Multi-AP Wi-Fi `ScanMethod::AllChannels` & `5,000 ms` Reconnect Cadence (`src/main.rs:1366-1382, 1478-1488`)**:
     - Configured `StationConfig::default()` with `.with_scan_method(esp_radio::wifi::sta::ScanMethod::AllChannels)`, `.with_beacon_timeout(30)`, and `.with_failure_retry_cnt(3)`.
     - Set `wifi_controller.set_max_tx_power(52)` (`13 dBm`) and `PowerSaveMode::None`.
     - Updated main loop reconnect poll condition to `last_connect_attempt_ms == 0 || now_ms.saturating_sub(last_connect_attempt_ms) >= 5_000` (`last_connect_attempt_ms = now_ms.max(1)`).
  3. **`pending_http_body` Stalled-Socket Deadline (`src/main.rs:1440, 1547-1577, 1692`)**:
     - Added `pending_http_started_ms: u64 = 0`, set on `!static_body.is_empty()`, and added a `2_500 ms` deadline + closing-state check inside `if !pending_http_body.is_empty()` to clear `pending_http_body = &[]`, call `http_sock.abort()`, and set `pending_http_closing = false`.
- **Stage-Only Verification Gates**:
  - `cargo test --workspace`: `exit code 0` (`219` tests passed across workspace).
  - `cargo clippy --workspace --all-targets -- -D warnings`: `exit code 0`.
  - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`: `exit code 0`.
  - `xtensa-esp32-elf-readelf -S`: `.stack = 0x008bd0` (`35,792` B `>= 35,000` B), `.data = 0x003dd0` (`15,824` B `<= 20,000` B), `.bss = 0x023440` (`144,448` B), `.dram2_uninit = 0x004990` (`18,832` B).
  - `xtensa-esp32-elf-objdump -d`: `run_hardware_firmware` stack frame `entry a1, 0x2120` = `8,480` B (`<= 9,000` B, leaving `27,312` B / `26.67 KB` free stack headroom above `_bss_end`).
  - `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, size `756,496` B (`28.15%` of `ota_0`), SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`.
- **Handoff Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo105_core1_wifi_implementer.md`
