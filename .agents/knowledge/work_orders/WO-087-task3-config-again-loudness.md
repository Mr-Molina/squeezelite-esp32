# Work Order: WO-087 — Wave 1 Task 3: Persistent Analog Gain & Loudness Schema in `FlashJournal` (`squeezelite-config`)

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-087` |
| **Timestamp** | `2026-10-25T12:26:30-04:00` |
| **Assigned Role** | NVS FlashJournal Schema Implementer (`Model: "flash"`, `TypeName: "self"`) |
| **Target Scope** | `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` ONLY |
| **Status** | `[EXECUTED & VERIFIED]` |

## 1. Objective & Task Brief
Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-3-brief.md` first — it is your requirements, with the exact values to use verbatim.

## 2. Mandatory Constraints
1. Modify ONLY `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` (Rule 35 backup already created at `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/squeezelite_config_lib.rs.bak`).
2. Follow TDD strictly: write the failing unit test first, run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config` to record RED failure, then implement `DEFAULT_ANALOG_GAIN_CODE = 0x08`, `MAX_ANALOG_GAIN_CODE = 0x1F`, `analog_gain_code: u8`, and `loudness_enabled: bool` in `AudioConfig` (including backwards-compatible `load_from` fallback when `audio.again` or `audio.loud` is missing from older flash stores), and verify GREEN (`cargo test -p squeezelite-config` and `cargo clippy -p squeezelite-config --all-targets -- -D warnings`).
3. Do NOT run `git add` or `git commit` (to avoid `.git/index.lock` contention with parallel Wave 1 subagents).
4. Do NOT dispatch any subagents and do NOT touch `/dev/ttyUSB0`.
5. Write your detailed report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-3-report.md`, write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo087_task3_config_schema.md`, and update the Fulfillment Section below.

## 3. Fulfillment Section
- **Execution Status**: `[EXECUTED & VERIFIED]` (DONE)
- **Implementer**: NVS FlashJournal Schema Implementer (`WO-087`)
- **Completion Timestamp**: `2026-10-25T12:34:00-04:00`
- **Files Modified**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs`
- **Report**: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-3-report.md`
- **Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T122630_wo087_task3_config_schema.md`
- **Verification Evidence**:
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config`: 15 unit tests passed, 9 fuzz tests passed, 0 failures (exit code `0`).
  - `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config --all-targets -- -D warnings`: 0 warnings, passed (exit code `0`).
