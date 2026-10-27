# Work Order `WO-131`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-04` — TAS5805M Driver, Amp Manager & Board Setup Auditor

- **ID**: `WO-131`
- **Timestamp**: `2026-10-25T20:44:00-04:00`
- **Subagent Role**: TAS5805M Driver, Amp Manager & Board Setup Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: TAS5805M Driver, Amp Manager & Board Setup Auditor (`AUD-04`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (`1,257` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs` (`803` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` (`162` LOC)
3. **Context**: Texas Instruments `TAS5805M` / `TAS5825M` Class-D digital amplifier I2C driver (`0x2D`), typestate transitions (`Standby` -> `HiZ` -> `Play`), `configure_silicon_maximization` (`0x02` `1SPW`/`768 kHz`, `0x53` `ANA_CTRL`, `0x54` `AGAIN`, `0x6B` spread-spectrum), `FS_MON` (`0x37`) / `BCK_MON` (`0x38`) telemetry, Book/Page state tracking (`0x00`/`0x7F`), `AmpManager` volume curves (`percent_to_attenuation`), and board pinout constants.
4. **Static Hotspots**:
   - Check `Tas5805mTelemetry::detected_sample_rate_hz` (`0x09` mapped to `48_000` vs TI datasheet `"44.1 kHz or 48 kHz"`), whether `configure_silicon_maximization()` is called in `Standby` (`0x01`) vs `Hi-Z` (`0x02`) in `AmpManager::initialize()`, Book/Page caching vs redundant `select_page0_book0()` I2C writes, and `amp.rs::percent_to_attenuation` (`0.8 dB`/step) vs `main.rs::lms_gain_to_tas5805m_vol_reg` (`0.5 dB`/step) volume curve divergence.
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Error Handling & Edge Cases`, `Memory, Resource & Concurrency Safety`, `Data Integrity & Persistence`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
