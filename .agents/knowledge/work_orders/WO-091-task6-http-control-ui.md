# Work Order: WO-091 — Wave 2 Task 6 (`src/http_control.rs` Extended Telemetry, EQ/AGAIN Controls & Web UI)

## Metadata
- **Work Order ID**: `WO-091`
- **Timestamp**: `2026-10-25T12:35:00-04:00`
- **Assigned Subagent Role**: Wave 2 Task 6 HTTP Control & Web UI Implementer (`Model: "flash"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[DONE]`

## Task Goal
Implement Wave 2 Task 6 in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` per `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-6-brief.md`.

## Target Files & Scope
- Strictly restricted to:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
- Do **NOT** modify `src/amp.rs`, `src/hal.rs`, `src/pipeline.rs`, or `src/main.rs`.
- Do **NOT** run `git add` or `git commit`.

## Acceptance Criteria
1. Extend `RuntimeStatusSnapshot` with the 16 new PSRAM, Dual-Core, Codec, EQ, TAS5805M Silicon, and NVS Settings fields (`Default` values included) and serialize them in `format_status_json` (`[u8; 2048]` buffer).
2. Add `HttpControlAction::SetEqPreset(u8)` (`act=eq_flat`, `eq_bass`, `eq_voice`, `eq_treble`, `eq_loudness`) and `HttpControlAction::SetAnalogGain(u8)` (`act=again0`, `again4`, `again6`, `again10`).
3. Update `INDEX_HTML` dashboard with interactive EQ Preset and Analog Gain (`AGAIN`) buttons + live PSRAM/Core1/TAS5805M status badges, and verify all unit tests pass.

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-25T13:00:00-04:00`
- **Report**: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-6-report.md`
- **Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo091_http_control_implementer.md`
- **Fulfillment Details**:
  1. Extended `HttpStatusSnapshot<'a>` / `RuntimeStatusSnapshot<'a>` with the 16 new fields: `psram_free_bytes`, `psram_used_bytes`, `core1_active`, `core1_loops`, `codec`, `codec_sample_rate`, `codec_channels`, `codec_bit_depth`, `eq_preset`, `eq_enabled`, `tas_fs_hz`, `tas_bck_ratio`, `tas_pll_locked`, `tas_automuted`, `again_code`, and `settings_seq`. Implemented `Default` for all fields and zero-allocation `BufferWriter`, `write_status_json`, and `format_status_json`.
  2. Extended `HttpControlAction` with `SetEqPreset(u8)` and `SetAnalogGain(u8)`. Added `/cmd.json` handling for `eq_flat`, `eq_bass`, `eq_loudness`, `eq_vocal`/`eq_voice`, `eq_rock`/`eq_treble`, and `again0`, `again4`, `again6`, `again8`, `again10`.
  3. Updated `DASHBOARD_HTML` and aliased `INDEX_HTML` with live telemetry table rows for PSRAM, Core 1, TAS5805M, and EQ/NVS, interactive 10-band EQ preset buttons, and AGAIN buttons.
  4. Executed `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib -- http_control`: all 10 unit tests pass (0 failures).
  5. Strictly complied with concurrency guardrail: modified only `src/http_control.rs`. No changes to `src/amp.rs`, `src/hal.rs`, `src/pipeline.rs`, or `src/main.rs`. No `git add` or `git commit`.
