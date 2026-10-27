# Work Order `WO-125`: `/deep-hardware-audit` Phase 1 Micro-Auditor 4 — Dual-Core LX6, PSRAM SPI0 / Flash SPI1 Interconnect & Stack Safety

- **ID**: `WO-125`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: Dual-Core LX6, PSRAM SPI0 / Flash SPI1 Interconnect & Stack Safety Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: Dual-Core LX6, PSRAM SPI0 / Flash SPI1 Interconnect & Stack Safety Micro-Auditor
2. **Target & Scope**: `[BASEMENT]` (`172.22.1.15`, Sonocotta Louder-ESP32 `ESP32-WROVER`). Subsystems: Dual-Core Xtensa LX6 (`PRO_CPU` + `APP_CPU`), External 4 MB Quad-SPI PSRAM (`SPI0` `0x3F800000`), External 4 MB NOR Flash (`SPI1`), and internal DRAM (`.stack`, `.bss`, `.data`, `.dram2_uninit`).
3. **Layer Mandate**: Audit bottom-up: L1 (`SPI0` Cache XIP/PSRAM vs `SPI1` NOR Flash physical bus sharing) -> L2 (`APP_CPU` parking at warm boot, `DPORT_APP_FLASH_MMU_TABLE` + `DPORT_APP_CACHE_CTRL1_REG` sync, `app_core_worker_loop` in `#[esp_hal::ram]`, `11.64M` loops/sec spinning without `waiti` or delay) -> L3 (ELF memory layout: `.stack = 35,792` B, `run_hardware_firmware` frame = `8,480` B, `.bss = 144,448` B, `.dram2_uninit = 18,832` B, `esp_alloc` `64 KB` internal heap) -> L4 (Volatile RAM `settings_scratch` persistence in `persist_audio_config`: settings changes like EQ/AGAIN are lost on reboot because runtime `SPI1` flash erase/write deadlocks when PSRAM/Core1/Wi-Fi are active; check whether safe pre-reboot persistence on `/reboot.json` or deferred boot/shutdown flush is possible or if any other runtime code path touches `flash`).
4. **Seeds**: `Firmware, BIOS/UEFI, Bootloader & Platform Resiliency`, `Embedded / MCU / IoT Hardware`, `Power, Thermal & Clocking Configurations`. Compliance: `NIST-800-53:CM-6`, `NIST-800-193:PROTECT-2`, `HW-FAULT:PWR-THERMAL`.
5. **Pre-Identified Scout Anomalies**:
   - Anomaly A: `settings_seq: 7` is stored only in RAM (`settings_scratch`) during runtime (`WO-109`), so EQ preset and AGAIN settings set via `/cmd.json` do not survive a power cycle or reboot.
   - Anomaly B: `core1_loops` advances at `11.64M` loops/sec (`tight spin loop` with `core::hint::spin_loop()` in `app_core_worker_loop`), consuming 100% of `APP_CPU` cycles and continuous dynamic power/heat even when idle.
   - Anomaly C: Verify that ZERO runtime functions after `Psram::new` and `start_app_core` invoke `flash.read` / `flash.erase` / `flash.write`.
6. **Shell & Script Safety**: Read-only Linux commands (`xtensa-esp32-elf-readelf`, `xtensa-esp32-elf-objdump`, `xtensa-esp32-elf-nm`) on `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` and read `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`. **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry and binary/source inspection. Do NOT modify any files or configs.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
