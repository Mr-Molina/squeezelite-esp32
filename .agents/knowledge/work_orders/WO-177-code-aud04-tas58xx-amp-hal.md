# Work Order `WO-177`: `/deep-code-audit` Phase 1 (`AUD-04`) — TAS5805M Driver, Amp Lifecycle & System Bus HAL Auditor

- **ID**: `WO-177`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-04` — TAS5805M Driver, Amp Lifecycle & System Bus HAL Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-04`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (all 1,344 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs` (all 805 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs` (all 1,156 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **TI `TAS5805M` Driver (`squeezelite-tas58xx`), `AmpManager` (`amp.rs`) & HAL (`hal.rs`)**:
   - Audit `Tas58xx` register sequences (`0x00` Page, `0x7F` Book, `0x02` `DEVICE_CTRL_1`, `0x03` `DEVICE_CTRL_2`, `0x4C` `DIG_VOL_CTRL`, `0x53` `ANA_CTRL`, `0x54` `AGAIN`, `0x6B` `SS_CTRL0`, `0x37` `FS_MON`, `0x38` `BCK_MON`, `0x39` `CLKDET_STATUS`, `0x40` `AUTOMUTE_STATE`, `0x70..0x73` fault registers, `0x78` `FAULT_CLEAR`).
   - Audit `AmpManager` volume attenuation curves (`set_volume_percent`, `lms_gain_to_tas5805m_vol_reg`) and check why `vol_pct: 5` on live hardware causes `tas_automuted: true`.
   - Audit `hal.rs` (`EspI2sSink`, `CydUartBridge`, `NvsFlashBackend`) for sample-rate validation (`8000..=192000`), 32-bit stereo frame packing, and dual-sector NVS flash recovery.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across all 3 files (including `GRS-011` `get_` prefixes and `GRS-012` line lengths >100 cols / comments >80 cols).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `78fe5e5c-5e58-4fdd-bbdf-f923fb5a4dac`
- **Summary**: Audited `squeezelite-tas58xx/src/lib.rs`, `louder-esp32-app/src/amp.rs`, and `hal.rs`. Identified 13 findings (`HW-001` `NvsFlashBackend::commit_store` sector erase before write, `HW-002`..`004`, `LOGIC-001`..`004`, `STYLE-001`..`005`).
