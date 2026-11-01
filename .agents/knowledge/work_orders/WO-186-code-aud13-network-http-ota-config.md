# Work Order `WO-186`: `/deep-code-audit` Phase 1 (`AUD-13`) — Network Audio Streaming, Web Control & 4-Gate Anti-Brick OTA Safety Auditor

- **ID**: `WO-186`
- **Timestamp**: `2026-10-27T17:26:00-04:00`
- **Subagent Role**: `AUD-13` — Network Audio Streaming, Web Control & 4-Gate OTA Safety Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-13`** (`/deep-code-audit` Phase 1 Cohort 2 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` (1,573 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` (1,087 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs` (1,032 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs` (394 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` (1,157 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/tests/config_fuzz.rs` (320 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Network, HTTP Web Control, 4-Gate Anti-Brick OTA & Config Journal**:
   - Audit `network.rs` (`SlimprotoClient`, `HttpAudioStream`, chunked transfer encoding, ICY metadata stripping, `LmsDiscovery`).
   - Audit `http_control.rs` (`parse_http_control_request`, `write_status_json`, `handle_http_request_streaming`, `DASHBOARD_HTML`, `DeadManLeaseWatchdog`).
   - Audit `ota_safety.rs` (`load_nvs_wifi_credentials_split_12kb`, `decrypt_nvs_secret`, `OtaPartitionGuard`, `otadata` `0xD000..0xF000`).
   - Audit `wifi.rs` and `squeezelite-config/src/lib.rs` (`FlashJournal`, `DualSectorFlashStore`, `AudioConfig`, `WifiCredentials`).
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-002`: Disallowed glob imports at `crates/squeezelite-config/tests/config_fuzz.rs:6, 7`.
   - `GRS-012`: Line lengths >100 cols / comments >80 cols across all 6 files.
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Subagent ID**: `2745fbea-bc7e-4b94-ad83-bea32315bf88`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**: Audited `network.rs`, `http_control.rs`, `ota_safety.rs`, `wifi.rs`, `squeezelite-config/src/lib.rs`, and `config_fuzz.rs`. Identified 9 findings (`LOGIC-001` `volup` `snapshot.vol_pct.saturating_add(5).min(100)`; `LOGIC-002` `vol<digits>` `parse::<u32>()` clamping; `SEC-001` `LmsDiscovery::discover_server` wall-clock timeout deadline; `SEC-002` `ota_safety.rs` NVS entry `span` check only when `is_nvs_entry_written(bitmap, idx)`; `STYLE-001` `GRS-002` glob imports in `config_fuzz.rs:6-7`; `STYLE-002` `GRS-012` line widths).
