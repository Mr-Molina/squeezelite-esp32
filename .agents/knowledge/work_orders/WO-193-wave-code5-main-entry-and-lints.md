# Work Order `WO-193`: `/deep-code-audit` Phase 3 (`Wave CODE-5`) — Bare-Metal Entry (`main.rs`), Board/Sink & Workspace Lint Remediation

- **ID**: `WO-193`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Rust Bare-Metal Entry & Workspace Lint Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Wave CODE-5 Implementer** (`/deep-code-audit` Phase 3 + `rust-pro`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/audit_master_matrix.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Strictly Assigned Files (DO NOT TOUCH ANY OTHER FILES)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
- `/home/console-admin/Github/squeezelite-esp32/rust/rustfmt.toml`

### Required Functional, Hardware & `rust-pro` Remediations
1. **`louder-esp32-app/src/main.rs`**:
   - **`CODE-MAIN-BT-001` (`CRITICAL`, lines 1576, 2167-2252, 2385-2420)**:
     - Whenever SlimProto starts (`b's'`), stops (`b'q'`), or flushes (`b'f'`) a stream, reset `bt_audio_active = false;` so that if Bluetooth A2DP later resumes after a SlimProto MP3 or FLAC stream, `if !bt_audio_active` triggers and reconfigures `pipeline.set_decoder(ActiveDecoder::Pcm(PcmDecoder::with_config(16, false, 2)))`!
   - **`CODE-MAIN-OTA-001` (`CRITICAL`, lines 1912-1930)**:
     - In `if pending_http_action != HttpControlAction::None && !http_sock.is_open() && pending_http_body.is_empty()`: call `unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }` unconditionally BEFORE `if settings_dirty` AND BEFORE `HttpControlAction::PersistOta0AndReboot` so `APP_CPU` is always parked during any `SPI1` NOR flash erase/write!
   - **`CODE-MAIN-CLK-001` / `HW-ESP-CLK-001` (`HIGH`, lines 900-901)**:
     - Update `88_200 => (7, 11, 63)` to `88_200 => (7, 5, 58)` (`88,199.5 Hz`, `-5.5 ppm`) and `96_000 => (6, 25, 48)` to `96_000 => (6, 25, 49)` (`96,003.1 Hz`, `+0.003%`).
     - **CRITICAL**: Do NOT change `(div_b << 8) | (div_a << 14)` in `encode_i2s0_clkm_conf` at lines 420-425!
   - **`CODE-MAIN-NET-001` (`HIGH`)**:
     - On `dhcpv4::Event::Deconfigured` (lines 1661-1671): also reset `slim_acc_len = 0; pending_resp_len = 0; pending_resp_sent = 0;`.
     - On `http_sock` `CloseWait` check (lines 1684, 1727): only abort immediately on `CloseWait` when `pending_http_body.is_empty()` (`!pending_http_closing && pending_http_body.is_empty() && matches!(http_sock.state(), tcp::State::CloseWait)`).
     - On `HttpControlAction::PlayerIrCommand` (line 1882): check `slim_connected && slim_sock.can_send() && pending_resp_sent >= pending_resp_len` before sending `ir_buf`.
     - At lines 1797 and 1969: use `let lms_ip_octets = audio_cfg.lms_server_ip.unwrap_or(DEFAULT_LMS_SERVER_IP);` instead of hardcoded `DEFAULT_LMS_SERVER_IP`.
     - At lines 2230 and 2548 (`ActiveDecoder::Flac(FlacDecoder::new())`): guard with `if psram_avail_bytes >= 1_048_576` so `FlacDecoder::new()` (`192 KB` `alloc::vec!`) is never called if PSRAM is absent.
     - In `audio_sock.recv_slice` (lines 2741-2761): clamp `max_chunk` to `pipeline.stream_buffer.available_write().min(audio_rx_chunk.len())` BEFORE calling `audio_sock.recv_slice` so every byte read from `audio_sock` is written into `stream_buffer` and `icy_meta_next` never desynchronizes!
   - **`GRS-001` (`main.rs:437, 450`)**: Flatten `use esp_hal::{ ... }` and `use smoltcp::{ ... }` so zero `::` appear inside `{ ... }` braces.
   - **`GRS-008` (`main.rs`)**: Add dual `// JUSTIFICATION:` and `// SAFETY:` comments directly above all `unsafe` blocks/fns in `main.rs`.
   - **`GRS-010` (`main.rs:1232, 1253`)**: Replace `.unwrap()` on `Rate::from_khz(400)` and `I2c::new(...)` with `.expect("valid I2C0 400 kHz config")`.
   - **`GRS-012` (`main.rs`, `lib.rs`, `board.rs`, `sink.rs`)**: Wrap all code lines `<= 100` cols and standalone comments `<= 80` cols.
2. **`rust/rustfmt.toml` & `rust/Cargo.toml`**:
   - Create `/home/console-admin/Github/squeezelite-esp32/rust/rustfmt.toml` with `edition = "2021"`, `max_width = 100`, `tab_spaces = 4`, `hard_tabs = false`, `newline_style = "Unix"`, `use_small_heuristics = "Default"`.
   - Add `[workspace.lints.rust]` (`unsafe_op_in_unsafe_fn = "warn"`) and `[workspace.lints.clippy]` in `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
3. **Verification Gate**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs --strict` and verify **0 errors and 0 warnings** (`exit code 0`).
   - Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code5.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `aa95f139-e8a9-4cb5-bfa7-13eb8feef35e` (Wave CODE-5 Implementer)
- **Handoff Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code5.md`
- **Verification Evidence**:
  - `google_rust_audit.py --strict` on `main.rs`, `lib.rs`, `board.rs`, `sink.rs`: **0 error(s), 0 warning(s)** (exit code 0).
  - All assigned functional fixes (`CODE-MAIN-BT-001`, `CODE-MAIN-OTA-001`, `CODE-MAIN-CLK-001`, `CODE-MAIN-NET-001`) and Google Rust Style rules (`GRS-001`, `GRS-008`, `GRS-010`, `GRS-012`) implemented cleanly without any external regressions.
