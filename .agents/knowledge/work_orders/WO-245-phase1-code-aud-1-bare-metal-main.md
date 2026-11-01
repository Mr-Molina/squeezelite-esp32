# Work Order: WO-245 — Phase 1 `CODE-AUD-1`: Bare-Metal Runtime, `unsafe` MMIO/PSRAM/Core1, & `init_static_pipeline` Micro-Auditor

- **Work Order ID**: `WO-245`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `Bare-Metal Runtime, unsafe MMIO/PSRAM/Core1, & init_static_pipeline Micro-Auditor` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`4,126` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` (`227` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/clock_table.rs` (`205` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs` (`1,195` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/io_traits.rs` (`1,004` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/io_traits/conformance.rs` (`263` LOC)
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Micro-Auditor Contract
1. **Role**: `Bare-Metal Runtime, unsafe MMIO/PSRAM/Core1, & init_static_pipeline Micro-Auditor` (`CODE-AUD-1`)
2. **Scope**: Strictly the 6 files listed above (`main.rs`, `board.rs`, `clock_table.rs`, `hal.rs`, `io_traits.rs`, `io_traits/conformance.rs`).
3. **Focus Areas & Pre-Identified Hotspots**:
   - Audit all 35 `unsafe` blocks/functions/attributes in `main.rs` (`GRS-008` `// JUSTIFICATION:` + `// SAFETY:` comments, pointer validity, MMIO register addresses, PSRAM cache mask/restore around flash erase/write, Core 1 stack/launch, `STATIC_PIPELINE` initialization in `init_static_pipeline()`).
   - Check whether all Phase 3/4/7 endpoints and modules (`/config.json`, `/scan.json`, `/messages.json`, `/commands.json`, `POST /config.json`, `console.rs` CLI/Telnet, `ota_safety.rs`, `controls.rs`, `display_gds.rs`) are completely wired in `main.rs` or if any Phase 3–7 route/poll handler has an integration gap.
   - Check `I2S0` clock divider reconfiguration (`set_clock_rate`) across all supported sample rates (`44.1k`, `48k`, `88.2k`, `96k`, `176.4k`, `192k`, `32k`, `24k`, `22.05k`, `16k`, `12k`, `11.025k`).
4. **Constraint**: **Read-only code inspection. Do NOT modify any files.**
5. **Output Schema**: Use the standardized 11-field code finding schema (`/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`) with OWASP, CWE, SOC 2, and `GRS-*` compliance tags. If no defects are found in a module, explicitly certify it `INNOCENT / VERIFIED` with line-range citations.

## 2. Deliverables
Return your complete structured findings in your final message.

## 3. Fulfillment Section
- **Status**: `[AUDIT COMPLETE]`
- **Subagent ID**: `5a4d0e00-a6f1-4043-a9c4-983d4812244d`
- **Summary**:
  - Certified `board.rs`, `clock_table.rs`, `hal.rs`, `io_traits.rs`, and `io_traits/conformance.rs` as `INNOCENT / VERIFIED` (`0` `unsafe` blocks).
  - Audited all 35 `unsafe` constructs in `main.rs` and reported 6 findings (`1 CRITICAL`, `2 HIGH`, `1 MEDIUM`, `2 LOW`): `CONC-101` (`main.rs:2755-2766, 3261-3283` unparked Core 1 flash write when `pending_image()` is `None`), `LOGIC-101` (`main.rs:1601-1659` missing `AtomicBool` singleton guard in `init_static_pipeline`), `LOGIC-102` (`main.rs:3171-3251` Phase 3 HTTP actions swallowed by `_ => {}`), `LOGIC-103` (`main.rs:2480-2495` hardcoded ESP32 GPIO pins in `bring_up`), `SEC-101` (missing `GRS-008` comments on 10 `#[unsafe(...)]` attributes), and `PERF-101` (`main.rs:72-75` dead `encode_i2s0_clkm_conf`).
