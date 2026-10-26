# Subagent Summary: WO-105 — Fix Core 1 (`APP_CPU`) Warm-Reboot Park/MMU Startup, Multi-AP `ScanMethod::AllChannels` Selection, & `pending_http_body` Timeout (`--stage-only`)

- **Work Order**: `WO-105`
- **Subagent Role**: Rust App Dual-Core & Wi-Fi Implementer (`self`)
- **Timestamp**: `2026-10-25T22:09:40Z`
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `91dbd9d4eaa4ed0580346e3641e641217eac48e5` (`91dbd9d4`)

## 1. Files Created, Modified, or Backed Up (Absolute Paths)
1. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.pre_wo105.bak` — Created Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` — Modified:
   - **Core 1 (`APP_CPU`) Warm-Reboot Park + MMU/Cache Sync**:
     - Increased `APP_CORE_STACK` from `Stack<512>` to `Stack<2048>` (`src/main.rs:951`).
     - Immediately after `let peripherals = esp_hal::init(config);` (before Gate 1 `FlashStorage::new(peripherals.FLASH).multicore_auto_park()`), instantiated `let mut cpu_ctrl = CpuControl::new(peripherals.CPU_CTRL);` and called `unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }` so `is_running(Cpu::AppCpu)` is `false` across Gate 1 and `start_app_core`.
     - Immediately before `cpu_ctrl.start_app_core(...)`, synchronized `DPORT_APP_FLASH_MMU_TABLE` (`0x3FF1_2000..0x3FF1_2400`) and `DPORT_APP_CACHE_CTRL1_REG` (`0x3FF0_005C` bits `0..5`) from `PRO_CPU` (`0x3FF1_0000` and `0x3FF0_0044`) while `DPORT_APP_CACHE_CTRL_REG` (`0x3FF0_0058` bit 3) is disabled.
   - **Multi-AP Wi-Fi `ScanMethod::AllChannels` & `5,000 ms` Reconnect Cadence**:
     - Configured `StationConfig::default()` (`src/main.rs:1366-1373`) with `.with_scan_method(esp_radio::wifi::sta::ScanMethod::AllChannels)`, `.with_beacon_timeout(30)`, and `.with_failure_retry_cnt(3)`.
     - Set `wifi_controller.set_max_tx_power(52)` (`13 dBm`) and `PowerSaveMode::None`.
     - Changed the main loop `!wifi_controller.is_connected()` poll cadence (`src/main.rs:1478-1482`) from `1,000 ms` to `5,000 ms` (`last_connect_attempt_ms == 0 || now_ms.saturating_sub(last_connect_attempt_ms) >= 5_000`, setting `last_connect_attempt_ms = now_ms.max(1)`) so `ScanMethod::AllChannels` (`~1.3s`) + 3 retries are never interrupted mid-scan.
   - **`pending_http_body` Stalled-Socket Deadline**:
     - Added `pending_http_started_ms: u64 = 0`, set `pending_http_started_ms = now_ms` whenever `pending_http_body` is populated (`!static_body.is_empty()`), and added a `2_500 ms` deadline + closing-state check inside `if !pending_http_body.is_empty()` (`src/main.rs:1547-1563`) to clear `pending_http_body = &[]`, call `http_sock.abort()`, and set `pending_http_closing = false`.
3. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` — Packaged `--stage-only` OTA binary (`756,496` bytes, SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`).
4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-105-fix-core1-boot-and-wifi-ap-selection.md` — Appended Fulfillment Section.

## 2. Verification Metrics (`--stage-only`)
- `cargo test --workspace`: `0` failures (`219` unit tests passed across workspace).
- `cargo clippy --workspace --all-targets -- -D warnings`: `exit code 0`.
- `xtensa-esp32-elf-readelf -S`:
  - `.stack`: `0x008bd0` = `35,792` bytes (`>= 35,000` B gate PASS)
  - `.data`: `0x003dd0` = `15,824` bytes (`<= 20,000` B gate PASS)
  - `.bss`: `0x023440` = `144,448` bytes
  - `.dram2_uninit`: `0x004990` = `18,832` bytes
- `xtensa-esp32-elf-objdump -d`:
  - `run_hardware_firmware`: `entry a1, 0x2120` = `8,480` bytes (`<= 9,000` B gate PASS; leaves `27,312` B / `26.67 KB` free stack headroom above `_bss_end` `0x3ffd7430`)
- `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, `756,496` bytes (`28.15%` of `ota_0`), SHA-256 `770d08507bc94332bf194351e148794e56989cf9f2de83580a7dac9a24556f04`.

## 3. Risks & Untested Assumptions
- Strictly `--stage-only`: `172.22.1.15` was not flashed or modified, and `/dev/ttyUSB0` was untouched. Live OTA certification on `172.22.1.15` will verify `core1_active: true`, `core1_loops > 0`, local `-47 dBm` AP association, and full `GET /` dashboard delivery.
