# Subagent Summary: WO-100 — Wave 4 Task 10 Live Wi-Fi OTA Flash (`cbc35902`) & Boot Rollback Forensics

## 1. Files Created, Modified, or Deleted
- **Modified**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`
  - **Rationale**: Appended Entry 54 documenting the single OTA flash attempt of commit `cbc35902`, confirmation of automatic Gate 1 recovery rollback to Factory (`0x10000`, `"recovery": 1`), and the ELF/disassembly forensic root cause analysis (`33,568` B `run_hardware_firmware` stack frame inside `36,336` B `.stack` + `esp_hal::psram::Psram::new` `SPI0` XIP reclocking & `esp_alloc` 64 KB heap starvation).
- **Modified**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-100-task10-live-flash-cert.md`
  - **Rationale**: Updated status to `[BLOCKED / ERROR]` per the 1-Flash-Attempt Circuit Breaker and appended the Fulfillment Summary.
- **Created**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo100_extension_certifier.md`
  - **Rationale**: Rule 17 subagent summary report.

*(Note: Zero Rust source or manifest files under `rust/crates/**` were modified, in strict compliance with Invariant 47.)*

## 2. Root Cause Analysis of `cbc35902` Rollback on `172.22.1.15`
1. **`run_hardware_firmware` Stack Frame Overflow (`33,568` B frame inside `36,336` B `.stack`)**:
   - `readelf -S` on `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` shows `.stack` (`0x3ffd7210..0x3ffe0000`) is `0x008df0` = **`36,336` bytes**.
   - Disassembly of `run_hardware_firmware` (`0x4011cfa0`) shows `entry a1, 32` + `l32r a8, [0x40105914]` (`a8 = 0x00008300 = 33,536` bytes) + `sub a1, a1, a8`, allocating **`33,568` bytes (`0x8320`)** on entry and leaving only **`2,768` bytes (`2.70 KB`)** above `_bss_end` (`0x3ffd7210`) for all callees and ISRs.
   - Cause of the `+20 KB` stack frame growth in Task 7 (`5a837d44`):
     - `NvsFlashBackend` (`hal.rs:466-544`) embeds `scratch: [u8; 4096]` (`4,108` B), its un-inlined `NvsFlashBackend::new()` creates a second `4,108` B temporary on the stack, and its inlined `commit_store()` allocates `let mut b = *self.flash.read_sector(1)?` (`[u8; 4096]`) on the stack (`~12.3 KB` total).
     - `MemConfigStore::<32>` (`squeezelite-config/src/lib.rs:185`) embeds `heapless::Vec<(String<32>, Vec<u8, 128>), 32>` (`32 * 168 = 5,380` B) + `MemConfigStore::new()` temporary (`~10.7 KB` total).
   - Gate 1 (`flash.erase(0xD000, 0xF000)`) succeeded near the top of `run_hardware_firmware`, pre-arming Factory recovery (`0x10000`). Subsequent deeper calls (`NvsFlashBackend::load_store`, `Psram::new`, `WifiController::new`) pushed `sp` below `0x3ffd7210` into `.bss` (`g_cnxMgr` at `0x3ffd4c98`), crashing the MCU and triggering clean Gate 1 rollback to Factory recovery (`"recovery": 1`).
2. **`esp_hal::psram::Psram::new` Live `SPI0` XIP Cache Reclocking & `esp_alloc` 64 KB Heap Starvation**:
   - In `esp-hal-1.2.2/src/psram/esp32.rs`, `PsramConfig::default()` selects `PsramCacheSpeed::PsramCacheF80mS40m`. Neither `psram_gpio_config` (`esp32.rs:981`) nor `psram_cache_init` (`esp32.rs:480`) is marked `#[ram]`, and both reconfigure `SPI0` (`_SPI_CACHE_PORT`) to `80 MHz` (`_SPI_80M_CLK_DIV`) with `+2` extra dummy cycles while CPU 0 is executing XIP instructions from a `40 MHz DIO` bootloader image (`0xE9 ... 0x02 0x20`).
   - Additionally, if `psram_avail_bytes < 655_360`, `alloc::vec![0u8; 8192]` x 2 in `main.rs:1167-1168` allocates `16,384` B out of the `64 KB` internal DRAM `esp_alloc` Wi-Fi heap before `WifiController::new`, starving `esp-radio` (which requires `~50 KB` of the `64 KB` heap).

## 3. Known Risks or Untested Assumptions
- Target device `172.22.1.15` (`"BASEMENT"`) is verified online and healthy in Factory recovery (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -46`). No further flash attempts were made.
