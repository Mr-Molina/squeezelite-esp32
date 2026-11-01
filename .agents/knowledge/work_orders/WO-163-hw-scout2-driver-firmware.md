# Work Order `WO-163`: `/deep-hardware-audit` Phase 0a Scout 2 — Driver, Kernel & Firmware Revision Scout

- **ID**: `WO-163`
- **Timestamp**: `2026-10-27T17:03:00-04:00`
- **Subagent Role**: Driver, Kernel & Firmware Revision Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
Execute `/deep-hardware-audit` Phase 0a Scout 2 (Driver, Kernel & Firmware Revisions):
1. **Staged & Active ESP32 Firmware Revisions**:
   - Check the currently running firmware on `172.22.1.15` (`GET http://172.22.1.15/status.json`) vs. the staged OTA binary at `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` (if present) and the compiled Xtensa release ELF at `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`.
   - Compute SHA-256 and file size of the staged `.bin` and ELF, and inspect ELF sections (`xtensa-esp32-elf-readelf -S`) for `.stack`, `.data`, `.bss`, and `.dram2_uninit` sizes.
   - Check what `BASEMENT_HELO_CAPABILITIES` string is compiled into the ELF (`strings` or `grep` in `main.rs`) vs what `172.22.1.15` is currently advertising to LMS `10.0.0.10` (`WO-156` Rev 2 check).
2. **Host & LMS `10.0.0.10` Kernel/Daemon Revisions**:
   - Check local Linux kernel (`uname -r`), Rust/Espressif toolchain versions (`rustc --version`, `espflash --version`), and LMS `10.0.0.10` service/bridge binary versions (`logitechmediaserver`, `shairport_helper`, `squeeze2cast`).
3. **Zero Phase Leakage Constraint**: Read-only inventory only (<60s). Do NOT flash or modify anything. Append your Fulfillment Summary (in YAML format) to §2 of this Work Order file.

## 2. Fulfillment Section
*(To be completed by subagent)*
