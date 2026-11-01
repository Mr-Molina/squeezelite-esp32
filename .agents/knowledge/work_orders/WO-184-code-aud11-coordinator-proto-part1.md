# Work Order `WO-184`: `/deep-code-audit` Phase 1 (`AUD-11`) — SlimProto Coordinator State Machine Part 1 & `HELO`/`STAT` Wire Protocol Auditor

- **ID**: `WO-184`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-11` — SlimProto Coordinator Part 1 & HELO/STAT Protocol Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-11`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` (lines `1..=1100`)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` (lines `1..=900`)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **`PlayerCoordinator` (`coordinator.rs:1..=1100`) & `squeezelite-proto` (`lib.rs:1..=900`)**:
   - Audit `PlayerCoordinator` state transitions (`Disconnected`, `ConnectedIdle`, `Buffering`, `Playing`, `Paused`, `Stopped`), `handle_command`, `tick_audio`, and `AudioFormat::Flac` / `AudioFormat::Mp3` / `AudioFormat::Pcm` routing.
   - Audit `squeezelite-proto` (`lib.rs:1..=900`): `strm` command parsing (format `'p'`, `'m'`, `'f'`, `'a'`, `'o'`, PCM sample size/rate/channels/endianness, autostart `'0'..'3'`, threshold, transition, server port/IP, HTTP request extraction), `audg` gain parsing, `setd`, `cont`, `codc`.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across both file slices (including `GRS-012` line lengths).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `cdb1212f-3ecc-4f07-b199-021778eb8b2a`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `coordinator.rs:1..=1100` and `squeezelite-proto/src/lib.rs:1..=900`. Identified 15 findings (`CONC-001` `CRITICAL`: `evaluate_state` deadlocks in `PlayerState::Buffering` on 0-byte/EOF streams; `LOGIC-001` `CRITICAL`: `strm 'a'` skip-ahead multiplies by 4 instead of 8 bytes/frame, inverting stereo channels on odd frame counts; `LOGIC-002` `HIGH`: `process_audio_step` never syncs `current_sample_rate` from `pipeline.sample_rate`; `LOGIC-003` `HIGH`: `STMd` emitted before `!pipeline.has_pending_pcm()`; `LOGIC-005` `HIGH`: duplicate `DSCO` on HTTP connect error; `LOGIC-007` `MEDIUM`: `parse_strm` numeric `1`/`3` autostart; `LOGIC-015` `GRS-012` line widths).
