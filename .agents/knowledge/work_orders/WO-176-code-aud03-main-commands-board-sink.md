# Work Order `WO-176`: `/deep-code-audit` Phase 1 (`AUD-03`) — Bare-Metal Server Commands, Audio Sink & Board Setup Auditor

- **ID**: `WO-176`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-03` — Bare-Metal Server Commands, Audio Sink & Board Setup Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-03`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines `2101..=3131`)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` (all 162 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs` (all 198 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Audio Stream Ingestion, Bluetooth A2DP Handover, Board Pinouts & Tests**:
   - Audit `main.rs:2101..=3131` (SlimProto `strm` command handling, HTTP audio socket chunked/ICY reading, `FlacDecoder` / `StaticMp3Decoder` / `PcmDecoder` feeding, `STMd`/`STMu`/`STMo`/`STMs`/`STMt` reporting, Bluetooth A2DP `bt_sink` polling and PCM injection, and unit tests).
   - Check `board.rs` (`BASEMENT_HELO_CAPABILITIES`, pin assignments, sample rate bounds) and `sink.rs` (`NullAudioSink`, `SimulatedSink`).
   - Check whether `BASEMENT_HELO_CAPABILITIES` (`MaxSampleRate=96000,flc,pcm,wav,aif,mp3`) aligns with `Esp32HardwareI2sDma::set_clock_rate` and `StreamPipeline` buffer sizes when 88.2/96 kHz or 24-bit FLAC streams are played.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across `main.rs:2101..=3131`, `board.rs`, and `sink.rs` (including `GRS-011` getter prefixes and `GRS-012` line lengths).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `7616cb49-7d9d-4c42-8f21-0fa30f8d69fb`
- **Summary**: Audited `louder-esp32-app/src/main.rs:2101..=3131`, `board.rs`, and `sink.rs`. Identified 11 findings (`CONC-001` `bt_audio_active = false` reset on SlimProto preemption, `LOGIC-001` PSRAM guard for `FlacDecoder`, `LOGIC-002` ICY `max_chunk` clamping to `available_write()`, `LOGIC-003`..`005`, `INFRA-001`, `QA-001`, `STYLE-001`..`003`).
