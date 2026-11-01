# Work Order `WO-173`: `/deep-hardware-audit` Phase 1 Auditor 6 (`HW-AUD-06`) — Local Workstation (`pc-1`) Toolchain, Staged OTA Binary Packaging (`--stage-only`), & Passive Serial (`/dev/ttyUSB0`) Safety Micro-Auditor

- **ID**: `WO-173`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: Local Workstation Toolchain, Staged OTA Binary Packaging, & Passive Serial Safety Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `Local Workstation (pc-1) Toolchain, Staged OTA Binary Packaging (--stage-only), & Passive Serial (/dev/ttyUSB0) Safety Micro-Auditor` (`HW-AUD-06`)
2. **Target & Scope**: Local Workstation `pc-1` (`/etc/udev/rules.d/99-esp32-wrover.rules`, `/dev/serial/by-id/usb-1a86_USB_Serial-if00-port0`, `/home/console-admin/Github/squeezelite-esp32/partitions.csv`, `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`, and `/home/console-admin/.cargo/bin/espflash`).
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: Passive USB sysfs inspection (`/sys/bus/usb/devices/.../power/control`) on CH340 `1a86:7523` (`/dev/ttyUSB0` CYD board) to confirm zero active probes or USB selective suspend drops.
   - `L2-Bus-Interconnect`: `/dev/serial/by-id/` symlink integrity and `/etc/udev/rules.d/99-esp32-wrover.rules` compliance with `single-target-troubleshooting.md`.
   - `L3-Driver-Firmware-Config`: Audit `partitions.csv` (`ota_0` offset `0x160000`, size `0x290000` = `2,686,976` bytes) vs `louder-esp32-app` release ELF (`1,542,848` B ELF -> `~955 KB` `.bin` = `35.5%` of `ota_0`), and verify `esp_app_desc_t` fields (`0xABCD5432` magic, `"Squeezelite-ESP32"` project name, `"I2S-4MFlash-Rust"` version).
   - `L4-Service-Logs`: Audit why the staged OTA `.bin` image is currently absent (cleaned up by `/goodbye` at the end of the last session per Invariant 13) and specify the exact `TIER-3-STAGE-ONLY` packaging & verification procedure required before live deployment to `172.22.1.15`.
4. **Seeds**: Firmware, Bootloader & Platform Resiliency; PCIe, USB & Peripheral Interconnects (`NIST-800-193:PROTECT-1`, `NIST-800-193:RECOVER-1`, `NIST-800-53:SI-2`, `CIS-DRV-2.1`).
5. **Pre-Identified Scout Anomalies**:
   - Staged OTA `.bin` (`/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`) is absent and must be regenerated into `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin` after code audit remediations are complete.
6. **Shell & Script Safety**: Passive sysfs/udev/ELF reads only. **NEVER open or touch `/dev/ttyUSB0`.**
7. **Constraint**: **Read-only telemetry inspection. Do NOT modify any files or generate staged binaries during Phase 1.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `8e758242-5147-4532-8858-038449bdcd9a`
- **Summary**: Verified passive `/dev/ttyUSB0` isolation (`power/control: on`, `ID_MM_DEVICE_IGNORE=1`, `0` active processes) and `partitions.csv` (`ota_0` `2,686,976` B @ `0x160000`) + `esp_app_desc_t` headers (`0xABCD5432`, `"Squeezelite-ESP32"`, `"I2S-4MFlash-Rust"`), and documented `FW-001` (`TIER-3-STAGE-ONLY` `.bin` regeneration requirement).
