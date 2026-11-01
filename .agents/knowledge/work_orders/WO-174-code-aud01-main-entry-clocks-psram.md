# Work Order `WO-174`: `/deep-code-audit` Phase 1 (`AUD-01`) — Bare-Metal Hardware Entry, Clocks, Bootloader Gate 1 & PSRAM Dual-Core Auditor

- **ID**: `WO-174`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-01` — Bare-Metal Hardware Entry, Clocks & PSRAM Dual-Core Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-01`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`, `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md`, and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines `1..=1050`)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` (all 44 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Bare-Metal Cold-Boot & MMIO Invariants (`rust-pro` Section 4B)**:
   - Check whether `RTC_WDT` (`0x3FF4_808C` bits 14 & 11) is re-armed immediately after `esp_hal::init()` (`esp-hal 1.2.2` disables `RTC_WDT` inside `init()`).
   - Check `SPI0`/`SPI1` PSRAM initialization, `Esp32HardwareI2sDma::set_clock_rate` (`I2S0_CLKM_CONF_REG` `0x3FF4_F0AC` `div_b` `[13:8]` and `div_a` `[19:14]` bitfields and higher sample rates `88200`/`96000` Hz since `BASEMENT_HELO_CAPABILITIES` advertises `MaxSampleRate=96000`), and `app_core_loop` Core 1 `APP_CPU` synchronization.
   - Check `persist_audio_config` / `flush_pending_audio_config_to_flash` for `SPI0`/`SPI1` flash/PSRAM cache safety.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-001`: Nested `::` inside `use { ... }` braces at `main.rs:437, 450`.
   - `GRS-008`: Every `unsafe` block/fn/impl in `main.rs:1..=1050` (including lines `489, 888, 929, 1034`) MUST have dual `// JUSTIFICATION:` and `// SAFETY:` comments directly preceding it.
   - `GRS-012`: Lines exceeding 100 columns (code) or 80 columns (comments).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `4fbfa4aa-3f4c-4ba7-924d-0c6ffe0764dc`
- **Summary**: Audited `louder-esp32-app/src/main.rs:1..=1050` and `src/lib.rs`. Identified 8 findings (`LOGIC-001`..`005`, `INFRA-001`, `CONC-001`, `SEC-001`), including `88.2/96 kHz` fractional clock dividers (`(7, 5, 58)` and `(6, 25, 49)`), `RTC_WDT` re-arming, `GRS-001` nested imports, `GRS-008` dual `unsafe` comments, and `GRS-012` line widths.
