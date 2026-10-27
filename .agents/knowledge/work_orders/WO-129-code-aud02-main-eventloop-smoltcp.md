# Work Order `WO-129`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-02` — Bare-Metal Event Loop, 4-Socket Smoltcp & Network I/O Auditor

- **ID**: `WO-129`
- **Timestamp**: `2026-10-25T20:44:00-04:00`
- **Subagent Role**: Bare-Metal Event Loop, 4-Socket Smoltcp & Network I/O Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: Bare-Metal Event Loop, 4-Socket Smoltcp & Network I/O Auditor (`AUD-02`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines `1451`–`2865` ONLY; `1,415` LOC)
3. **Context**: Main runtime event loop (`smol_iface.poll`), 4 `smoltcp` sockets (`dhcp`, `http`, `slim`, `audio`), HTTP Port 80 streaming/control dispatch (`/status.json`, `/cmd.json`, `/recovery.json`, `/reboot.json`), SlimProto frame accumulator (`slim_acc_buf`), `audg` volume handling (`lms_gain_to_tas5805m_vol_reg` vs `current_vol_pct`), `audio_sock` HTTP stream header parsing & `512`-byte chunk ingestion, `TAS5805M` telemetry polling (`tas_fs_hz`), and host unit tests.
4. **Static Hotspots**:
   - Check `ServerCommand::AudioGain` (`audg` around lines `1937`–`1949`) not updating `current_vol_pct` / `audio_cfg.volume`, `/reboot.json` & `/reboot_ota.json` (`1758`–`1770`) resetting without flushing `settings_scratch`, `audio_sock` `512`-byte read chunk sizing vs `16,384` B RX window, and `tas_fs_hz` mapping at line `1615`.
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Input Boundaries & Parsing`, `Error Handling & Edge Cases`, `Memory, Resource & Concurrency Safety`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
