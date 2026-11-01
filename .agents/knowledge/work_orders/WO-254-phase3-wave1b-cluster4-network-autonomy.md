# Work Order: WO-254 — Phase 3 Wave 1B Cluster 4 Remediation (`http_control`, `ota_safety`, `wifi`, `console`, `eth_w5500`, `config_handle`)

- **Work Order ID**: `WO-254`
- **Timestamp**: `2026-11-01T09:18:00-05:00`
- **Assigned Subagent Role**: `Cluster 4 Phase 3 Network Autonomy Remediation Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/console.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/eth_w5500.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/config_handle.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Disjoint Scope
Remediate all **9 findings** (`SEC-501`, `SEC-502`, `SEC-503`, `NET-501`, `LOGIC-502`, `NET-502`, `NET-503`, `LOGIC-505`, `SEC-504`) in the 6 files listed above.

## 2. Findings to Remediate
1. **`SEC-501` (`CRITICAL`, `louder-esp32-app/src/ota_safety.rs:1049-1091`)**:
   - In `OtaStreamAssembler::ingest_chunk`: when `self.state == OtaStreamState::AwaitingHeader`, buffer incoming bytes into a `[u8; 288]` `header_buf` (`header_len: usize`) inside `OtaStreamAssembler`. Do NOT emit any `SectorErase` or `FlashWrite` operations while `self.state == OtaStreamState::AwaitingHeader` (when `header_len < 288`, return `Ok(OtaChunkPlan::empty())`). Once `header_len == 288`, validate the 288-byte header via `validate_ota_image_header(&self.header_buf)`: if invalid, transition to `OtaStreamState::Rejected` and return `Err(...)` with zero flash operations emitted; if valid, transition to `OtaStreamState::Streaming` and emit the initial sector 0 erase/write operations for the buffered header bytes plus any remaining bytes in `chunk`!
2. **`SEC-502` (`HIGH`, `louder-esp32-app/src/http_control.rs:461-477`)**:
   - In `extract_param_or_json_field` (and/or add a `percent_decode_into` helper):
     - Support optional whitespace around `:` in JSON fields (e.g. `"key": "value"` as well as `"key":"value"`).
     - Percent-decode `%XX` hex sequences and `+` (`-> ' '`) when extracting URL query/form parameters so Wi-Fi SSIDs/passwords with spaces or special characters decode accurately.
3. **`SEC-503` (`HIGH`, `louder-esp32-app/src/http_control.rs:844-846`)**:
   - Replace the constant `[0x42u8; 96]` Diffie-Hellman private key in `handle_http_request_streaming` with a deterministic non-constant 96-byte secret derived via SHA-1/HMAC mixing of `status.ip`, `status.uptime_ms`, and device identity (or a helper function `derive_zeroconf_dh_private_key(status: &HttpStatusSnapshot) -> [u8; 96]`).
4. **`NET-501` (`HIGH`, `louder-esp32-app/src/eth_w5500.rs:350-374, 387-400`)**:
   - In `W5500Driver::read_frame` and `write_frame`, mask the 16-bit socket ring buffer offset with `& 0x3FFF` (`const S0_BUF_SIZE: u16 = 0x4000; const S0_BUF_MASK: u16 = 0x3FFF;`). If `offset + len > S0_BUF_SIZE`, split the read/write into two SPI transactions (`first_len = (S0_BUF_SIZE - offset) as usize` at `offset`, followed by `len - first_len` at `0x0000`).
5. **`LOGIC-502` (`MEDIUM`, `louder-esp32-app/src/http_control.rs:1194-1200, 1255-1260`)**:
   - Ensure `write_scan_json` and `write_messages_json` format valid JSON arrays and expose helpers `write_scan_table_json(table: &WifiScanTable, out_buf: &mut [u8]) -> Option<usize>` and `write_syslog_ring_json<const N: usize>(ring: &crate::console::SyslogRingBuffer<N>, out_buf: &mut [u8]) -> Option<usize>` while keeping `handle_http_request_streaming` working cleanly.
6. **`NET-502` (`MEDIUM`, `louder-esp32-app/src/wifi.rs:182-205`)**:
   - In `CaptivePortalDns::build_response`, verify that the DNS query header has standard query opcode `(flags & 0x7800) == 0`, and in the QNAME label loop allow a 2-byte compression pointer (`(len & 0xC0) == 0xC0`) to advance `pos += 2` and terminate the QNAME loop cleanly (followed by reading the 4-byte QTYPE/QCLASS).
7. **`NET-503` (`MEDIUM`, `louder-esp32-app/src/console.rs:981-1016`)**:
   - In `TelnetSession::ingest_bytes`:
     - In `TelnetIacState::SawIac`, if `b == TELNET_IAC` (`0xFF`), append literal `0xFF` to `self.line_buf` (if `line_len < line_buf.len()`) and transition to `TelnetIacState::Normal`.
     - In `TelnetIacState::SawSubIac`, if `b == TELNET_SE` (`240`), transition to `TelnetIacState::Normal`; otherwise (including `b == TELNET_IAC` escaped `0xFF` inside subnegotiation), transition back to `TelnetIacState::InSubnegotiation`.
8. **`LOGIC-505` (`MEDIUM`, `louder-esp32-app/src/config_handle.rs:473-482`)**:
   - Add a `resolve_boot_wifi_with_journal` method (or update `resolve_boot_wifi` / `ConfigHandle`) that checks `FlashJournal` (`journal.wifi_ssid()`) ahead of NVS and compile-time defaults when a non-empty SSID is present in the journal, while preserving existing `resolve_boot_wifi` signature for callers/tests.
9. **`SEC-504` (`LOW`, `louder-esp32-app/src/http_control.rs:784-798`)**:
   - In `write_json_http_response`, check `if HEADER_ROOM.saturating_add(body_len) > out_buf.len() { return None; }` *before* calling `out_buf.copy_within(0..body_len, HEADER_ROOM)`.

## 3. Verification & Rules
- Verify tests, Clippy, and `google_rust_audit.py --strict` before returning.
- Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo254_cluster4_implementer.md` and update the Fulfillment Section of this Work Order.

## 4. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T10:05:00-05:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo254_cluster4_implementer.md`
- **Verification Evidence**:
  - `CARGO_TARGET_DIR=/tmp/target-wo254 cargo test -p louder-esp32-app --lib`: **314 passed; 0 failed**
  - Integration tests (`config_persistence`, `icy_metadata_passthrough`, `marker_fuzz`, `no_cfg_invariant`, `session_replay`, `trait_conformance`): **52 passed; 0 failed**
  - `google_rust_audit.py --strict`: **0 errors, 0 warnings** across all 6 modified files
  - `manage_task(Action: "list")`: 0 background tasks running
- **Remediations Summary**:
  1. `SEC-501`: `OtaStreamAssembler` buffers incoming bytes into `header_buf: [u8; 288]` during `AwaitingHeader`. Emits 0 flash write/erase operations until full 288-byte header is received and verified with `verify_ota_binary_header`. Rejects invalid headers without touching flash.
  2. `SEC-502`: Added `percent_decode_into` and `percent_decode_str` for URL query parameter decoding (`%XX` and `+`). Updated `extract_param_or_json_field` to tolerate optional whitespace around `:` in JSON fields.
  3. `SEC-503`: Replaced hardcoded `[0x42u8; 96]` DH key in Spotify ZeroConf request handling with deterministic non-constant key via `derive_zeroconf_dh_private_key(&status)` using HMAC-SHA1 over IP, uptime, and telemetry. Fixed `ZeroConfServer::new().handle_request(...)` call.
  4. `NET-501`: Added `S0_BUF_SIZE = 0x4000` and `S0_BUF_MASK = 0x3FFF` in `W5500Driver`. Added `read_socket0_ring_block` and `write_socket0_ring_block` to split SPI transactions crossing the 16 KiB ring buffer boundary into two operations (offset..0x4000 and 0x0000..rest).
  5. `LOGIC-502`: Implemented `write_scan_table_json(&WifiScanTable, out_buf)` and `write_syslog_ring_json(&SyslogRingBuffer, out_buf)` formatting valid JSON arrays.
  6. `NET-502`: In `CaptivePortalDns::build_response`, validated standard query opcode `(flags & 0x7800) == 0` and enabled 2-byte DNS compression pointers (`(query[pos] & 0xC0) == 0xC0`) in the QNAME loop.
  7. `NET-503`: In `TelnetSession::ingest_bytes`, escaped IAC (`0xFF 0xFF`) appends literal `0xFF` in `SawIac`, and subnegotiation transitions cleanly back to `InSubnegotiation` on embedded escaped IAC and `Normal` on `TELNET_SE`.
  8. `LOGIC-505`: Added `resolve_boot_wifi_with_journal` taking `FlashJournal` ahead of NVS when a non-empty SSID is present, while preserving existing `resolve_boot_wifi` signature.
  9. `SEC-504`: Added `if HEADER_ROOM.saturating_add(body_len) > out_buf.len() { return None; }` before `out_buf.copy_within` in `write_json_http_response`.
