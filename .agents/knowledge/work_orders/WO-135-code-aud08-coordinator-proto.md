# Work Order `WO-135`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-08` — SlimProto Coordinator State Machine & Packet Engine Auditor

- **ID**: `WO-135`
- **Timestamp**: `2026-10-25T20:51:00-04:00`
- **Subagent Role**: SlimProto Coordinator State Machine & Packet Engine Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: SlimProto Coordinator State Machine & Packet Engine Auditor (`AUD-08`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` (lines `1`–`824` ONLY; `824` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs` (lines `1`–`815` ONLY; `815` LOC)
3. **Context**: `PlayerCoordinator` state machine (`PlayerState`, `on_slimproto_packet`, `on_http_response`, `on_stream_eof`, `tick_heartbeat`) and `squeezelite-proto` SlimProto binary wire protocol parser/builder (`SlimprotoParser`, `SlimprotoBuilder`, `HELO`, `STAT`, `RESP`, `DSCO`, `BUTN`, `IR  `, `SETD`, `strm`, `audg`, `setd`, `cont`, `codc`, `vers`, `serv`).
4. **Static Hotspots**:
   - Check binary packet framing bounds, integer endianness/overflows in `SlimprotoParser`, `HELO` capability string formatting (should `flc`/`aac`/`ogg` be advertised when 4 MB PSRAM is active?), `STAT` event code ordering (`STMd`, `STMu`, `STMo`, `STMt`, `STMs`), and `audg` gain parsing.
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Input Boundaries & Parsing`, `Error Handling & Edge Cases`, `Memory, Resource & Concurrency Safety`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
