# Work Order: WO-089 — Wave 2 Task 4 (`src/amp.rs` & `src/hal.rs` AmpManager + NVS Flash Partition)

## Metadata
- **Work Order ID**: `WO-089`
- **Timestamp**: `2026-10-25T12:35:00-04:00`
- **Assigned Subagent Role**: Wave 2 Task 4 Amp & HAL Implementer (`Model: "flash"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
Implement Wave 2 Task 4 in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs` per `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-4-brief.md`.

## Target Files & Scope
- Strictly restricted to:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
- Do **NOT** modify `src/pipeline.rs`, `src/http_control.rs`, or `src/main.rs`.
- Do **NOT** run `git add` or `git commit`.

## Acceptance Criteria
1. `AmpManager` in `src/amp.rs` supports `with_config(i2c, enable_pin, config)`, `set_analog_gain_code(code)`, `analog_gain_code()`, `config()`, and `poll_telemetry()` returning `Result<Tas5805mTelemetry, AmpError<I2C::Error, P::Error>>`.
2. `NvsFlashBackend<F>` in `src/hal.rs` wraps `FlashPartition` and implements `squeezelite_config::nvs::FlashBackend` (`erase_all` at `0..4096`).
3. Unit tests in `src/amp.rs` and `src/hal.rs` pass under `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib`.

## Fulfillment Section
- **Fulfillment Status**: `DONE` (`[EXECUTED & VERIFIED]`)
- **Completion Timestamp**: `2026-10-25T12:55:00-04:00`
- **Handoff Report**: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-4-report.md`
- **Summary**:
  - `AmpConfig` enhanced with `initial_again_code` (default `0x08` = `-4.0 dB`).
  - `AmpManager` enhanced with `config()`, `analog_gain_code()`, `set_analog_gain()`, `set_analog_gain_code()`, `read_telemetry()`, `poll_telemetry()`, and automatic silicon maximization invocation during `initialize`.
  - `TargetI2cBus` in `src/hal.rs` initialized with simulated TAS5805M telemetry registers (`0x37`, `0x38`, `0x39`, `0x54`).
  - `FlashPartition` and `NvsFlashBackend` in `src/hal.rs` enhanced with `erase_sector`, `erase_all`, and `erase_range`.
  - Verification: 17 `amp` unit tests and 9 `hal` unit tests passed (`cargo test --lib -- amp` and `cargo test --lib -- hal`).
