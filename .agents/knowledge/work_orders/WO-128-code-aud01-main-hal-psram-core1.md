# Work Order `WO-128`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-01` — Bare-Metal HAL, Clock, PSRAM & Dual-Core Auditor

- **ID**: `WO-128`
- **Timestamp**: `2026-10-25T20:44:00-04:00`
- **Subagent Role**: Bare-Metal HAL, Clock, PSRAM & Dual-Core Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: Bare-Metal HAL, Clock, PSRAM & Dual-Core Auditor (`AUD-01`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines `1`–`1450` ONLY; `1,450` LOC)
3. **Context**: Bare-metal `xtensa-esp32-none-elf` hardware entry point, `SmolDevice` Wi-Fi MTU (`1492`), `Esp32HardwareI2sDma` (`I2S0_CLKM_CONF_REG` `0x3FF4_F0AC` fractional clock divider, circular DMA `tx_buffer` `8,192` B), `APP_CPU` Core 1 worker loop & MMU/cache sync, `4 MB` PSRAM (`SPI0` `0x3F80_0000`) initialization, Gate 1 `otadata` (`0xD000..0xF000`) erase, NVS Wi-Fi credentials load, and `persist_audio_config` (`settings_scratch`).
4. **Static Hotspots (`WO-115`)**:
   - `unsafe` blocks in `src/main.rs` at lines `489`, `888`, `929`, `1034`, `1051`, `1054`, `1057`, `1060`, `1063`, `1109`, `1206`, `1207`, `1210`, `1239`, `1263`.
   - Check `I2S0_CLKM_CONF_REG` (`div_a` vs `div_b` bitfields `[13:8]` vs `[19:14]` at line `891`), `app_core_worker_loop` tight CAS spin (`11.65M` loops/sec without delay), `SETTINGS_PARTITION_OFFSET` shadowing (`0x3FE000` at line `980` vs `0x3F0000` in `ota_safety.rs`), and stack frame usage (`8,480` B in `run_hardware_firmware`).
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Memory, Resource & Concurrency Safety`, `Error Handling & Edge Cases`, `Configuration, Build & Supply Chain`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
