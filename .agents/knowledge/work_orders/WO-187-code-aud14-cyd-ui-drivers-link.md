# Work Order `WO-187`: `/deep-code-audit` Phase 1 (`AUD-14`) — CYD Controller Touch UI, Display Drivers & Inter-Board UART Link Auditor

- **ID**: `WO-187`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-14` — CYD Controller Touch UI, Display Drivers & UART Link Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-14`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/ui.rs` (1,241 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/drivers.rs` (912 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/comms.rs` (752 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/main.rs` (584 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/lib.rs` (37 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/src/lib.rs` (727 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/tests/cyd_fuzz.rs` (190 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs` (257 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **CYD Display, Touch, Bitmap UI & Inter-Board UART2 Link**:
   - Audit `Ili9341Display` and `Xpt2046Touch` (`drivers.rs`) for SPI coordinate clamping, scanline chunking, and ADC outlier filtering.
   - Audit `NowPlayingUi` (`ui.rs`) and `CydDisplayLink` (`comms.rs`) for dirty-region redrawing, touch throttling, and UART line framing.
   - Audit `cyd-controller-app/src/main.rs` for `RTC_WDT` re-arming and bare-metal `esp-hal` safety.
   - Audit `squeezelite-cyd-link/src/lib.rs` and `louder-esp32-app/src/cyd.rs`.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-001`: Nested `::` inside `use { ... }` braces at `crates/cyd-controller-app/src/main.rs:30`.
   - `GRS-002`: Disallowed glob imports at `crates/squeezelite-cyd-link/tests/cyd_fuzz.rs:7, 8`.
   - `GRS-012`: Line lengths >100 cols / comments >80 cols across all 8 files.
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `d6ca3d6d-3f72-4cff-b3c8-08001ea7d10a`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `cyd-controller-app/**`, `squeezelite-cyd-link/**`, and `louder-esp32-app/src/cyd.rs`. Identified 9 findings (`HW-001` `HIGH`: re-arm and feed `timg0.wdt` in `cyd-controller-app/src/main.rs`; `LOGIC-001` `MEDIUM`: `Xpt2046Touch::calibrate_raw` axis range calibration when `swap_xy` is enabled; `HW-002` `MEDIUM`: remove redundant `is_irq_low()` guard inside `read_raw_12bit`; `STYLE-001` `GRS-001` nested `use` in `cyd-controller-app/src/main.rs:30`; `STYLE-002` `GRS-002` glob imports in `cyd_fuzz.rs:7-8`; `STYLE-003` `GRS-012` line widths).
