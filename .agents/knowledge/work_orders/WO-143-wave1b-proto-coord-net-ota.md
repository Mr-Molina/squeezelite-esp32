# Work Order `WO-143`: Phase 3 Wave 1B — SlimProto, Coordinator, Network, HTTP & OTA Safety Implementer

- **ID**: `WO-143`
- **Timestamp**: `2026-10-25T21:38:30-04:00`
- **Subagent Role**: SlimProto, Coordinator, Network, HTTP & OTA Safety Implementer (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Exclusive File Scope & Constraints
- **Exclusive Files Owned by `WO-143` (DO NOT touch any other files in the repository!)**:
  1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
  2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
  4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
  5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
  6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs`
  7. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- **Backups Already Created**: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/`
- **Isolated Cargo Target Dir (MANDATORY)**: Because parallel subagents are compiling concurrently, prefix all `cargo` commands with `CARGO_TARGET_DIR=/tmp/target-wo143`!
- **Backwards-Compatibility Constraint**: Do NOT break existing public signatures called by `louder-esp32-app/src/main.rs` (Wave 2 `WO-146` will update `main.rs`).
- **Hardware Safety**: NEVER touch `/dev/ttyUSB0`. Do not use `sed`, `awk`, or `cat` to edit files (use `edit_file`).

## 2. Assigned Remediations (`CODE-*` & `HW-*`)
1. **`CODE-PROTO-001` (`squeezelite-proto/src/lib.rs:570-573` & `louder-esp32-app/src/coordinator.rs:435-441`)**:
   - In `SlimprotoParser::parse_audg` (`squeezelite-proto/src/lib.rs:570-573`): only zero `gain_left`/`gain_right` when `raw_left == 0 && old_left <= 0` (i.e. `let gain_left = if raw_left != 0 { raw_left } else if old_left < 0 { 0 } else { 0 };` -> `raw_left` is always preserved when non-zero, even if legacy `old_left < 0`!). Check existing unit tests in `squeezelite-proto/src/lib.rs` to ensure any test expecting `old_left < 0` with `raw_left == 0` still passes while non-zero `raw_left` is preserved.
   - In `coordinator.rs:435-441`: when `gain_left == 0 && gain_right == 0 && old_left > 0 && old_right > 0` and `digital_atten == 0`, set `(65536, 65536)` (unity gain) instead of `(0, 0)`.
2. **`CODE-PROTO-002` (`louder-esp32-app/src/coordinator.rs:553-616, 752-796`)**:
   - Add `stream_eof: bool` field to `PlayerCoordinator` (initialized to `false`, reset to `false` on `StreamCommandType::Start` / `reset()`, and exposed via `pub fn set_stream_eof(&mut self, eof: bool)` and `pub fn is_stream_eof(&self) -> bool`).
   - In `poll_audio_stream` (`line 554`), when `self.audio_stream` hits `Ok(0)` with `stream.is_eof()` or `Err(_)`, set `self.stream_eof = true`.
   - In `evaluate_state` (`lines 575-583`), compute `let is_eof = self.stream_eof || ...` on both host and `xtensa` (replacing `#[cfg(target_arch = "xtensa")] let is_eof = false;`).
   - In `evaluate_state` (`lines 597-616`), clear `self.pending_track_started = false` / `self.pending_decode_ready = false` / `self.pending_underrun = false` ONLY after `self.send_stat_full_now(client, ...)?` succeeds!
3. **`CODE-PROTO-003` (`squeezelite-proto/src/lib.rs:80-95, 516-530, 600-614, 931-945` & `coordinator.rs:127-140, 420-425, 452-464`)**:
   - In `SlimprotoParser::parse_setd` (`squeezelite-proto/src/lib.rs:600-614`) and `coordinator.rs:452-464`: `setd` payload byte 0 is always the setting `id` (`id == 0` is player name query/update). Never parse a 4-byte `id == 0` player name (e.g. `[0, b'B', b'o', b'b']`) or a 1-byte `id != 0` query as `current_sample_rate`! Keep explicit sample-rate helper fallback only if needed by existing unit tests (inspect existing `setd` tests in `squeezelite-proto/src/lib.rs` and `coordinator.rs` first so 100% of existing tests pass while fixing `id == 0` 3-char names and 1-byte non-zero `id` queries).
   - In `sample_rate_from_index` / `decode_pcm_sample_rate_hz` (`squeezelite-proto/src/lib.rs:80-95`): add `11 | b';' => Some(176_400)` and `12 | b'<' => Some(192_000)` (or check standard Squeezelite index mapping for `176,400` and `192,000`).
   - In `parse_strm` (`squeezelite-proto/src/lib.rs:516-530`): treat `b'1' | b'3' | 1 | 3` as `autostart: true` (and `b'0' | b'2' | 0 | 2` as `false`).
   - In `coordinator.rs:420-425` (`strm 'a'` skip-ahead): compute byte count using `u64` (`((u64::from(skip_ms) * u64::from(self.current_sample_rate)) / 1000) * 4`) and `usize::try_from(...).unwrap_or(usize::MAX)` to prevent 32-bit overflow.
4. **`CODE-NET-001` (`louder-esp32-app/src/network.rs:373-405, 532-609, 707-717, 938-966`)**:
   - Enforce `MAX_BODY_BUF_LEN = 65_536` and `MAX_CHUNK_SIZE = 1_048_576` in `HttpAudioStream` chunked decoding.
   - In `HttpAudioStream::is_eof()`, return `true` if `self.eof_reached && self.unchunked_buf.is_empty()` even if partial unparseable chunk bytes remain in `self.raw_body_buf` after the underlying socket closed.
   - In `IcyDecoderState::MetaData` (`lines 707-717`), trim trailing `\0` padding bytes (`let trimmed = self.meta_accum.iter().rposition(|&b| b != 0).map_or(&[][..], |i| &self.meta_accum[..=i]);`) before converting to UTF-8 string.
   - In `parse_http_location` (`lines 938-966`), support bracketed IPv6 hosts (`[::1]/stream` and `[::1]:8000/stream`).
5. **`CODE-OTA-001` / `HW-OTA-001` (`louder-esp32-app/src/ota_safety.rs:37, 380, 616-633`)**:
   - Export `pub const SETTINGS_JOURNAL_OFFSET: u32 = 0x003F_E000;` alongside `SETTINGS_PARTITION_OFFSET` (`CODE-MAIN-004`).
   - At line 380, recognize `NVS_PAGE_STATE_FREEING: u32 = 0xFFFF_FFF8` alongside `NVS_PAGE_STATE_ACTIVE` (`0xFFFF_FFFE`) and `NVS_PAGE_STATE_FULL` (`0xFFFF_FFFC`).
   - In `load_nvs_wifi_credentials_split_12kb` (`lines 616-633`): in Pass 2, after reading Pages 2 & 3 into `&mut nvs_buf[NVS_PAGE_SIZE..]`, call `scan_nvs_credentials(&nvs_buf[2 * NVS_PAGE_SIZE..], &mut ns_idx, &mut ssid_raw, &mut pass_raw);` (scanning only Page 3 since Pages 0, 1, 2 were already scanned in Pass 1!) so Page 0 is NEVER re-scanned after Page 1. Add a unit test verifying that newer credentials on Page 1 are NOT overwritten by older credentials on Page 0!
6. **`CODE-HTTP-001` (`louder-esp32-app/src/http_control.rs:183-235, 308-311, 461`)**:
   - Add a `write_json_escaped_str` helper in `http_control.rs` that escapes `"` as `\"`, `\` as `\\`, and control chars `< 0x20` when writing string fields (`project_name`, `platform_name`, `version`, `ip`, `netmask`, `gw`, `ssid`, `lms_ip`, `amp_mode`, `codec`, `eq_preset`) in `write_status_json`.
   - At line 461, map `(HttpMethod::Get | HttpMethod::Post, "/reboot.json")` to `Some(HttpControlAction::PersistOta0AndReboot)` (Check existing unit tests in `http_control.rs` and update any test asserting `/reboot.json` action!).
   - In `parse_http_request` / `route_request` (`lines 308-311`), if `POST /cmd.json` has no `act=` in the URL query string, check the HTTP request body (`\r\n\r\n` payload) for `act=...`.
7. **`CODE-PIPE-002` (`louder-esp32-app/src/pipeline.rs:201-205, 242-277, 313, 404-407`)**:
   - In `process_decode_step`, check the byte count `written` returned by `self.output_buffer.write(pcm_bytes)` and advance `self.pcm_pending_offset` only by `written / 2` samples!
   - Add `user_muted: bool` to `StreamPipeline`: `set_mute(&mut self, mute: bool)` sets `self.user_muted = mute`, and `process_output_step` only calls `self.sink.set_mute(false)` when `!self.user_muted`.
   - In `set_sample_rate`, call `self.sink.set_sample_rate(rate)?` BEFORE updating `self.sample_rate = rate`.
8. **`CODE-CFG-001` (`louder-esp32-app/src/wifi.rs:131-150`)**:
   - In `WifiManager::begin_connect`, reset `self.retry_count = 0`, and preserve existing `static_ip`, `gateway`, `subnet`, and `dns` from `self.store.load_network_config()` instead of overwriting them with `NetworkConfig::default()`.

## 3. Verification & Handoff Requirements (Rule 15, Rule 17, Rule 21)
1. Run `CARGO_TARGET_DIR=/tmp/target-wo143 cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-proto` and `CARGO_TARGET_DIR=/tmp/target-wo143 cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib` and verify exit code `0`.
2. Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo143_proto_coord_net_ota_implementer.md` (Rule 17) and append the Fulfillment Summary below (Rule 15).

## 4. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-25T22:16:30-04:00`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo143_proto_coord_net_ota_implementer.md`
- **Remediations Completed**:
  1. `CODE-PROTO-001`: In `parse_audg`, `raw_left`/`raw_right` are preserved whenever non-zero; in `coordinator.rs`, fallback to unity gain `(65536, 65536)` when `digital_atten == 0` and incoming `gain_left == 0 && gain_right == 0` if previous volume was non-zero.
  2. `CODE-PROTO-002`: Added `stream_eof: bool` to `PlayerCoordinator`, initialized/reset to `false`, set to `true` on stream EOF/error, unified in `evaluate_state`, and cleared pending telemetry flags strictly after `send_stat_full_now` succeeds.
  3. `CODE-PROTO-003`: Extended sample rate index mapping to include `176_400` (`11 | b';'`) and `192_000` (`12 | b'<'`), fixed `parse_strm` flags check (`b'2' | b'3' | 2 | 3`), guarded setting queries from corrupting sample rates, and used `u64` arithmetic for `strm 'a'` skip-ahead byte offset calculation.
  4. `CODE-NET-001`: Enforced `MAX_BODY_BUF_LEN = 65_536` and `MAX_CHUNK_SIZE = 1_048_576` limits, ensured `is_eof()` returns `true` when socket closes and unchunked buffers are empty, trimmed trailing `\0` padding in ICY metadata, and added support for bracketed IPv6 hosts in HTTP Location headers.
  5. `CODE-OTA-001` / `HW-OTA-001`: Exported `SETTINGS_JOURNAL_OFFSET: u32 = 0x003F_E000;`, recognized `NVS_PAGE_STATE_FREEING: u32 = 0xFFFF_FFF8`, and scanned strictly Page 3 in Pass 2 of `load_nvs_wifi_credentials_split_12kb` so Page 0 never overwrites newer credentials on Page 1.
  6. `CODE-HTTP-001`: Implemented `write_json_escaped_str` with `JsonEscaped` formatting wrapper for string escaping in `write_status_json`, mapped `/reboot.json` and `/reboot_ota.json` to `PersistOta0AndReboot`, and parsed `POST /cmd.json` body payloads for `act=...`.
  7. `CODE-PIPE-002`: Advanced `pcm_pending_offset` strictly by committed samples (`written / 8 * 2`), added `user_muted: bool` field and `set_mute`, checked `!self.user_muted` before unmuting in `process_output_step`, and ensured `set_sample_rate` calls sink first.
  8. `CODE-CFG-001`: In `WifiManager::begin_connect`, reset `self.retry_count = 0` and preserved `static_ip`, `gateway`, `subnet`, `dns`, and `autoconnect` from existing saved credentials.
- **Verification Evidence**:
  - `CARGO_TARGET_DIR=/tmp/target-wo143 cargo test --manifest-path rust/Cargo.toml -p squeezelite-proto`: 30/30 passed (20 unit, 10 fuzz), 0 failed.
  - `CARGO_TARGET_DIR=/tmp/target-wo143 cargo clippy --manifest-path rust/Cargo.toml -p squeezelite-proto --all-targets -- -D warnings`: 0 warnings, exit code `0`.
  - `CARGO_TARGET_DIR=/tmp/target-wo143 cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- coordinator network http_control ota_safety wifi pipeline`: 74/74 passed, 0 failed.
  - `CARGO_TARGET_DIR=/tmp/target-wo143 cargo clippy --manifest-path rust/Cargo.toml -p louder-esp32-app --all-targets -- -D warnings`: 0 warnings, exit code `0`.
