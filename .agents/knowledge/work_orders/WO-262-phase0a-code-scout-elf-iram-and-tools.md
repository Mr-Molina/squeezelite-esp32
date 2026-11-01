# Work Order: WO-262 — Phase 0a Code Scout 2: `MK5-Rust` (`84feda56`) ELF Symbol Placement, IRAM vs Flash Hotspots & Static Scanner Prober

- **Work Order ID**: `WO-262`
- **Timestamp**: `2026-11-01T13:28:30-05:00`
- **Assigned Subagent Role**: `ELF Symbol, IRAM Placement & Static Tool Prober` (`Model: "flash"`, `TypeName: "research"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Strict Phase 0a Anti-Leakage Guardrail
Execute a fast (<90s), strictly **read-only** Phase 0a ELF section/symbol placement and static tool inventory on `/home/console-admin/Github/squeezelite-esp32/rust`.
- **DO NOT** modify any files.

## 2. Inventory Tasks
1. **Release ELF Section Table & Memory Map**:
   - Run `/home/console-admin/.rustup/toolchains/esp/xtensa-esp-elf/esp-14.2.0_20240906/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` to report exact sizes and addresses of `.iram0.vectors`, `.iram0.text`, `.dram0.data`, `.dram0.bss`, `.dram2_uninit`, `.stack`, `.flash.text`, and `.flash.rodata`.
2. **IRAM (`0x4008_0000..0x400A_0000`) vs Flash (`0x400D_0000..`) Symbol Placement**:
   - Run `/home/console-admin/.rustup/toolchains/esp/xtensa-esp-elf/esp-14.2.0_20240906/xtensa-esp-elf/bin/xtensa-esp32-elf-nm -S -n /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` (filtering for functions in `0x40080000..0x400a0000` vs top 40 largest functions in `0x400d0000..`) to inventory which functions reside in IRAM vs SPI Flash cache.
3. **Static Analysis & Style Tool Probe**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` and check available CLI tools (`cargo clippy --version`, `semgrep --version`, `trivy --version`).

## 3. Deliverable
Return the structured ELF Section, IRAM/Flash Symbol Placement, and Static Tool Probe report.
