# Work Order `WO-170`: `/deep-hardware-audit` Phase 1 Auditor 3 (`HW-AUD-03`) — `"BASEMENT"` (`172.22.1.15`) ESP32 Cold-Boot Resiliency, `RTC_WDT`, PSRAM `SPI0`/`SPI1`, Dual-Core `APP_CPU` & `.stack`/`.dram2_uninit` Micro-Auditor

- **ID**: `WO-170`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: `"BASEMENT"` (`172.22.1.15`) ESP32 Cold-Boot, `RTC_WDT`, PSRAM, Dual-Core & Memory Layout Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `"BASEMENT" (172.22.1.15) ESP32 Cold-Boot Resiliency, RTC_WDT, PSRAM SPI0/SPI1, Dual-Core APP_CPU & .stack/.dram2_uninit Micro-Auditor` (`HW-AUD-03`)
2. **Target & Scope**: Embedded Target `"BASEMENT"` (`172.22.1.15`), compiled Xtensa ELF `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`, and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{main.rs,ota_safety.rs}`.
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: Cold-boot `0V -> 5V` POR resiliency, `RTC_WDT` re-arming after `esp_hal::init()` (`rust-pro` Bare-Metal `esp-hal 1.2.2` Cold-Boot Invariant: check if `RTC_CNTL_WDTCONFIG0_REG` `0x3FF4_808C` is re-armed or if `TIMG0` WDT covers early boot), and `GPIO 16` PSRAM CS# / `SPI0`/`SPI1` safety.
   - `L2-Bus-Interconnect`: `4 MB` External Quad-SPI PSRAM (`0x3F80_0000`, `640 KB` used + `192 KB` `FlacDecoder` = `832 KB` used, `3.34 MB` free), `APP_CPU` (`core1_active: true`, `3,584.5 Hz` throttled loop), and zero runtime `SPI1` NOR flash writes during active Wi-Fi/PSRAM.
   - `L3-Driver-Firmware-Config`: ELF memory layout (`xtensa-esp32-elf-readelf -S` and `xtensa-esp32-elf-objdump -d` stack frame of `run_hardware_firmware`): verify `.stack = 36,920` B (`>= 36,000` B Invariant 47), `.dram2_uninit = 98,264` B (`<= 98,768` B), and `run_hardware_firmware` stack frame size.
   - `L4-Service-Logs`: 4-Gate Anti-Brick status (`otadata_prearmed_factory`, `TIMG0` 20s WDT, `DeadManLeaseWatchdog`, Port 80 `/recovery.json`).
4. **Seeds**: Firmware, Bootloader & Platform Resiliency; Embedded / MCU / IoT Hardware (`NIST-800-193:PROTECT-2`, `NIST-800-193:DETECT-1`, `NIST-800-193:RECOVER-1`, `HW-FAULT:PWR-THERMAL`).
5. **Pre-Identified Scout Anomalies**:
   - Check whether `RTC_WDT` (`0x3FF4_808C`) or `TIMG0` WDT is armed during early boot before `Psram` / `WifiController::new` per `rust-pro` Cold-Boot Invariants.
   - Verify `run_hardware_firmware` stack frame size in the current `WO-155` Rev 3 ELF (`/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`).
6. **Shell & Script Safety**: Use `/home/console-admin/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20260121/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf` and `xtensa-esp32-elf-objdump`. Never touch `/dev/ttyUSB0`.
7. **Constraint**: **Read-only telemetry and binary inspection. Do NOT modify any files or flash hardware.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `2a76445d-f976-4401-b9dc-629c236ca20f`
- **Summary**: Verified `.stack = 36,920` B (`>= 36,000` B), `.dram2_uninit = 98,264` B (`<= 98,768` B), `run_hardware_firmware` stack frame = `9,856` B (`27,064` B headroom), `APP_CPU` IRAM loop (`3,584.5 Hz`), and 4-Gate Anti-Brick safety; identified `EMB-001` (`RTC_WDT` `0x3FF4_808C` disarmed by `esp_hal::init()` without cold-boot re-arming).
