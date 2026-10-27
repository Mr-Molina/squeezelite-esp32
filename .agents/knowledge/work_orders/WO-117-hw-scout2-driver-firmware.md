# Work Order `WO-117`: `/deep-hardware-audit` Phase 0a Scout 2 — Driver, Kernel & Firmware Revision Scout (`[BASEMENT]`)

- **ID**: `WO-117`
- **Timestamp**: `2026-10-25T20:21:15-04:00`
- **Subagent Role**: Driver, Kernel & Firmware Revision Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 2 (Driver, Kernel & Firmware Revision Scout)** for `/deep-hardware-audit` of `[BASEMENT]` (`172.22.1.15`).
1. **Live Firmware & ELF/OTA Binary Revisions**:
   - Query `curl -sS -m 5 http://172.22.1.15/status.json` for `project_name`, `platform_name`, `version`, `recovery`, `rust_engine`.
   - Inspect the compiled Xtensa release ELF `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` using `/home/console-admin/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20241119/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf -S` (record `.stack`, `.bss`, `.data`, `.dram2_uninit`, `.text`, `.rodata` sizes and addresses) and check `run_hardware_firmware` stack frame size via `xtensa-esp32-elf-objdump -d` (first 15 lines of `run_hardware_firmware`).
   - Check the latest staged OTA binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (if present): file size and `sha256sum`.
2. **LMS Host (`10.0.0.10`) Kernel & Bridge Revisions**:
   - Via read-only SSH (`ssh -o BatchMode=yes 10.0.0.10 "uname -a; dpkg -l logitechmediaserver avahi-daemon avahi-utils | cat; ps aux | grep -E 'squeeze|shairport|avahi|spotty' | grep -v grep"`), inventory active daemon and helper binary versions.
3. **Strict Constraints**:
   - Read-only queries only (<60s). **NEVER touch `/dev/ttyUSB0`**. Return pure YAML inventory.

## 2. Fulfillment Section
*(To be completed by subagent)*
