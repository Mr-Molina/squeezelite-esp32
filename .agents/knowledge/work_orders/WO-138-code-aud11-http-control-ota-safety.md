# Work Order `WO-138`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-11` — HTTP Web Control & 4-Gate OTA Safety Auditor

- **ID**: `WO-138`
- **Timestamp**: `2026-10-25T20:51:00-04:00`
- **Subagent Role**: HTTP Web Control & 4-Gate OTA Safety Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: HTTP Web Control & 4-Gate OTA Safety Auditor (`AUD-11`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` (`977` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs` (`951` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` (`42` LOC)
3. **Context**: Port 80 HTTP control server (`handle_http_request_streaming`, `/status.json`, `/cmd.json`, `/recovery.json`, `/reboot.json`, `/reboot_ota.json`, `DASHBOARD_HTML`), `DeadManLeaseWatchdog`, and 4-Gate Anti-Brick OTA safety (`load_nvs_wifi_credentials_split_12kb`, `derive_wifi_nvs_key`, `verify_ota_binary_header`, `SETTINGS_PARTITION_OFFSET`).
4. **Static Hotspots**:
   - Check JSON string escaping in `write_status_json` (`ssid`, `eq_preset`, `amp_mode`), `/cmd.json` query parameter parsing, CSRF / unauthenticated destructive endpoints (`/recovery.json`, `/reboot.json`), `SETTINGS_PARTITION_OFFSET` (`0x3F0000` in `ota_safety.rs` vs `0x3FE000` in `main.rs`), and NVS multi-page string/blob edge cases in `load_nvs_wifi_credentials_split_12kb`.
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Auth, Session & Access Control`, `Input Boundaries & Parsing`, `Data Integrity & Persistence`, `Error Handling & Edge Cases`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
