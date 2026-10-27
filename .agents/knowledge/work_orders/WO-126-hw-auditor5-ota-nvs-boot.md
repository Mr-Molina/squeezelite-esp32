# Work Order `WO-126`: `/deep-hardware-audit` Phase 1 Micro-Auditor 5 — Dual-Bank OTA Boot Resiliency, NVS Partition & Cryptographic Staging

- **ID**: `WO-126`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: Dual-Bank OTA Boot Resiliency, NVS Partition & Cryptographic Staging Micro-Auditor (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: Dual-Bank OTA Boot Resiliency, NVS Partition & Cryptographic Staging Micro-Auditor
2. **Target & Scope**: `[BASEMENT]` (`172.22.1.15`, Sonocotta Louder-ESP32 `ESP32-WROVER`). Subsystems: ESP32 Partition Table (`partitions.csv`), `otadata` (`0xD000..0xF000`), `nvs` (`0x9000..0xD000`), `recovery` (`0x10000`), `ota_0` (`0x160000`), `settings` (`0x3F0000`), and staged OTA binary verification.
3. **Layer Mandate**: Audit bottom-up: L1 (4 MB SPI NOR Flash sector geometry & partition boundaries in `partitions.csv` vs `src/ota_safety.rs` constants) -> L2 (Dual-bank `recovery` + `ota_0` layout & `otadata` `0xD000..0xF000` erase-to-`0xFF` behavior) -> L3 (4-Gate Anti-Brick Architecture & Invariant 47 compliance: Gate 1 order, split 12 KB NVS Page-0 namespace pinning in `load_nvs_wifi_credentials_split_12kb`, Gate 2 20s `TIMG0` WDT, Gate 3 `DeadManLeaseWatchdog`, Gate 4 `/recovery.json` & `/reboot.json`) -> L4 (Staged OTA binary availability and `esp_app_desc_t` cryptographic/SHA-256 verification).
4. **Seeds**: `Firmware, BIOS/UEFI, Bootloader & Platform Resiliency`, `Embedded / MCU / IoT Hardware`. Compliance: `NIST-800-193:PROTECT-1`, `NIST-800-193:RECOVER-1`, `NIST-800-53:SI-2`.
5. **Pre-Identified Scout Anomalies**:
   - Anomaly A: `WO-117` noted that `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` is `ABSENT` (cleaned up by `goodbye_preflight.py` at the end of session `31deac42` per Invariant 13), while the compiled ELF `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` is present.
   - Anomaly B: Check whether `otadata_prearmed_factory: true` (Gate 1 erasing `0xD000..0xF000` on every boot) means a normal power outage or clean `/reboot.json` also boots back into `recovery` (`0x10000`) instead of `ota_0` (`0x160000`), and how `recovery` vs `ota_0` boot selection works across power cycles!
6. **Shell & Script Safety**: Read-only Linux commands and file reads (`partitions.csv`, `rust/crates/louder-esp32-app/src/{ota_safety.rs,main.rs,http_control.rs}`). **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry and source inspection. Do NOT modify any files or configs.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
