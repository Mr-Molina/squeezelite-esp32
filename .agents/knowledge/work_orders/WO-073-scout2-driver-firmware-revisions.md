# Work Order `WO-073`: `/deep-hardware-audit` Phase 0a Scout 2 — Driver, Kernel & Firmware Revision Scout

- **ID**: `WO-073`
- **Timestamp**: `2026-10-25T09:40:00-04:00`
- **Subagent Role**: Driver, Kernel & Firmware Revision Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 2 (Driver, Kernel & Firmware Revision Scout)** for `/deep-hardware-audit`.
Inventory all firmware, bootloader, crate, and daemon versions across `172.22.1.15` and `10.0.0.10`:
1. **Target `172.22.1.15` (`"BASEMENT"`) Firmware & Partition Baseline**:
   - Inspect ELF sections (`readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`), staged binary `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` size + SHA-256, and git commit hash (`git rev-parse --short HEAD`).
   - Record crate versions in `rust/crates/louder-esp32-app/Cargo.toml` (`esp-hal`, `esp-radio`, `esp-rtos`, `esp-alloc`, `esp-storage`, `esp-bootloader-esp-idf`, `smoltcp`, `squeezelite-tas58xx`).
2. **LMS / Bridge Server `10.0.0.10` Kernel & Daemon Revisions**:
   - Via read-only SSH (`ssh -o BatchMode=yes 10.0.0.10 "uname -a; dpkg -l | grep -E 'logitechmediaserver|lyrion|avahi|librespot|ffmpeg|sox|flac|lame'"`), inventory OS kernel, LMS version, and bridge helper versions (`shairport_helper`, `squeeze2cast`, `spotty`).
3. **Strict Constraints**:
   - Read-only inventory only (<60s).
   - **NEVER touch `/dev/ttyUSB0`** (Rule 3).
   - Return pure structured YAML inventory.

## 2. Fulfillment Section
*(To be completed)*
