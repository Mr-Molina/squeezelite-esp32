# Work Order `WO-175`: `/deep-code-audit` Phase 1 (`AUD-02`) — Bare-Metal Event Loop, 4-Socket Smoltcp & Network I/O Auditor

- **ID**: `WO-175`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-02` — Bare-Metal Event Loop, 4-Socket Smoltcp & Network I/O Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-02`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (lines `1051..=2100`)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Bare-Metal Event Loop, Boot Sequence & Socket Safety**:
   - Audit `run_hardware_firmware` startup order (`CpuControl::park_core(Cpu::AppCpu)`, Gate 1 `otadata` `0xD000..0xF000`, `load_nvs_wifi_credentials_split_12kb`, PSRAM init, `APP_CPU` start, `WifiController`, `BleConnector`, `SmolDevice` `MTU=1492`, 4-socket `smoltcp` loop).
   - Check whether boot-time flash reads (`load_nvs_wifi_credentials_split_12kb` or `NvsFlashBackend`) or PSRAM init have any edge cases across warm/cold boots.
   - Check Port 80 HTTP socket (`CloseWait` / `TimeWait` / `pending_http_body` timeout), SlimProto socket, and HTTP audio socket state transitions, buffer bounds, and FLAC (`AudioFormat::Flac`) / `96 kHz` stream handling.
   - Check `ServerCommand::AudioGain` (`audg`) and `HttpControlAction` volume/mute/preset handling.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-008`: All `unsafe` blocks in `main.rs:1051..=2100` (lines `1051, 1054, 1057, 1060, 1063, 1218, 1281, 1300, 1330, 1365, 1375`) lacking dual `// JUSTIFICATION:` + `// SAFETY:` comments.
   - `GRS-010`: Production `.unwrap()` calls at `main.rs:1232, 1253`.
   - `GRS-012`: Code lines >100 cols / comments >80 cols.
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `ea861ec3-058c-4fe3-a660-b5500fcfb3e4`
- **Summary**: Audited `louder-esp32-app/src/main.rs:1051..=2100`. Identified 9 findings (`CONC-001` `park_core(AppCpu)` before `PersistOta0AndReboot` flash write, `LOGIC-001`, `NET-001` DHCP deconfig buffer reset, `NET-002` `CloseWait` body drain, `PROTO-001` `PlayerIrCommand` `pending_resp_sent` check, `CONFIG-001` `audio_cfg.lms_server_ip`, `STYLE-001` `GRS-008`, `STYLE-002` `GRS-010`, `STYLE-003` `GRS-012`).
