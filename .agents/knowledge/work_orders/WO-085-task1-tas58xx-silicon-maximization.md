# Work Order: WO-085 — Wave 1 Task 1: TI TAS5805M Silicon Maximization, Register Fixes & Live Telemetry (`squeezelite-tas58xx`)

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-085` |
| **Timestamp** | `2026-10-25T12:26:30-04:00` |
| **Assigned Role** | TAS5805M Silicon Driver Implementer (`Model: "flash"`, `TypeName: "self"`) |
| **Target Scope** | `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` ONLY |
| **Status** | `[EXECUTED & VERIFIED]` |

## 1. Objective & Task Brief
Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-1-brief.md` first — it is your requirements, with the exact values to use verbatim.

## 2. Mandatory Constraints
1. Modify ONLY `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (Rule 35 backup already created at `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/squeezelite_tas58xx_lib.rs.bak`).
2. Follow TDD strictly: write the failing unit tests in `mod tests`, run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx` to record RED failure, then implement the register constants, fault bitmasks, `Tas5805mTelemetry`, `configure_silicon_maximization(again_code: u8)`, `set_analog_gain(again_code: u8)`, and `read_telemetry()`, and verify GREEN (`cargo test -p squeezelite-tas58xx` and `cargo clippy -p squeezelite-tas58xx --all-targets -- -D warnings`).
3. Do NOT run `git add` or `git commit` (to avoid `.git/index.lock` contention with parallel Wave 1 subagents; the orchestrator will commit each crate atomically).
4. Do NOT dispatch any subagents and do NOT touch `/dev/ttyUSB0`.
5. Write your detailed report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-1-report.md`, write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo085_task1_tas58xx.md`, and update the Fulfillment Section below.

## 3. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]` (DONE)
- **Implementer**: TAS5805M Silicon Driver Implementer (`WO-085`)
- **Completion Timestamp**: `2026-10-25T12:38:00-04:00`
- **Modified Files**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
- **Verification Evidence**:
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx`: 13 passed, 0 failed (exit code `0`).
  - `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx --all-targets -- -D warnings`: 0 warnings, 0 errors (exit code `0`).
  - Register definitions and bitmasks verified via `test_datasheet_register_addresses_and_fault_bitmasks`.
  - Silicon maximization configuration and live telemetry decoding verified via `test_configure_silicon_maximization_and_telemetry`.
- **Handoff Artifacts**:
  - Detailed Report: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-1-report.md`
  - Subagent Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo085_tas58xx_implementer.md` and `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo085_task1_tas58xx.md`
