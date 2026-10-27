# Subagent Summary: `WO-143` Phase 3 Wave 1B Implementation

- **Work Order**: `WO-143`
- **Subagent Role**: SlimProto, Coordinator, Network, HTTP & OTA Safety Implementer (`Model: "pro"`, `TypeName: "self"`)
- **Timestamp**: `2026-10-25T22:16:30-04:00`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified (Strict Scope Compliance)
All modifications were strictly constrained to the 7 designated files. No changes made to `main.rs`, `hal.rs`, `amp.rs`, `cyd.rs`, or any other crates.

1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs`
7. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`

---

## 2. Rationale & Implementation Details for Each Remediation

### 1. `CODE-PROTO-001` (`squeezelite-proto/src/lib.rs` & `louder-esp32-app/src/coordinator.rs`)
- **`squeezelite-proto/src/lib.rs`**: In `parse_audg`, `raw_left` and `raw_right` (16.16 linear gain) are preserved whenever non-zero, regardless of legacy `old_left`/`old_right` negative dB fields. Eliminates premature muting when LMS sends non-zero fixed-point volume alongside negative dB legacy fields.
- **`louder-esp32-app/src/coordinator.rs`**: In `ServerCommand::AudioGain`, when `digital_atten == 0` and incoming `gain_left == 0 && gain_right == 0` but previous volume was non-zero (`self.volume.gain_left > 0 && self.volume.gain_right > 0`), sets unity gain `(65536, 65536)` instead of zeroing out output volume.

### 2. `CODE-PROTO-002` (`louder-esp32-app/src/coordinator.rs`)
- Added `pub stream_eof: bool` to `PlayerCoordinator`, initialized to `false` in `new()` and reset to `false` on `StreamCommandType::Start` and `reset()`. Exposed via `pub fn set_stream_eof(&mut self, eof: bool)` and `pub fn is_stream_eof(&self) -> bool`.
- In `poll_http_stream`, sets `self.stream_eof = true` on `Ok(0)` when `stream.is_eof()` or `Err(_)`.
- In `evaluate_state`, unified `is_eof` check to include `self.stream_eof` on both `xtensa` and host targets.
- In `sync_telemetry`, cleared pending telemetry flags (`pending_track_started`, `pending_decode_ready`, `pending_dsco`, `pending_underrun`) strictly after `send_stat_full_now` succeeds.

### 3. `CODE-PROTO-003` (`squeezelite-proto/src/lib.rs` & `louder-esp32-app/src/coordinator.rs`)
- **`squeezelite-proto/src/lib.rs`**:
  - In `sample_rate_from_index`: added mappings `11 | b';' => Some(176_400)` and `12 | b'<' => Some(192_000)`.
  - In `parse_strm`: flags check supports `b'2' | b'3' | 2 | 3` for continuous wait without breaking proptest fuzz assertions on `autostart`.
  - In `parse_setd_setting`: accurately maps setting IDs (`id == 0` for player name query/update) and prevents 4-byte names or 1-byte non-zero queries from corrupting sample rates.
- **`louder-esp32-app/src/coordinator.rs`**:
  - In `decode_pcm_sample_rate_hz`: added `176_400` and `192_000`.
  - In `strm 'a'` skip-ahead: computes byte offset using `u64` arithmetic (`((u64::from(skip_ms) * u64::from(self.current_sample_rate)) / 1000) * 4`) and clamps via `usize::try_from(...).unwrap_or(usize::MAX)` to prevent 32-bit arithmetic overflow.
  - In `ServerCommand::SetDevice`: preserves `current_sample_rate` and rejects non-rate setting queries.

### 4. `CODE-NET-001` (`louder-esp32-app/src/network.rs`)
- In `HttpAudioStream`: enforced chunked decoding limits `MAX_BODY_BUF_LEN = 65_536` and `MAX_CHUNK_SIZE = 1_048_576`.
- In `HttpAudioStream::is_eof()`: returns `true` if `self.eof_reached && self.unchunked_buf.is_empty() && self.audio_buf.is_empty()`, even if unparseable chunk fragments remain in `raw_body_buf` after socket closure.
- In `IcyDecoderState::MetaData`: trimmed trailing `\0` padding bytes before converting to UTF-8.
- In `parse_http_location`: added full parsing support for bracketed IPv6 addresses (`[::1]/stream` and `[::1]:8000/stream`).

### 5. `CODE-OTA-001` / `HW-OTA-001` (`louder-esp32-app/src/ota_safety.rs`)
- Exported `pub const SETTINGS_JOURNAL_OFFSET: u32 = 0x003F_E000;`.
- Defined `NVS_PAGE_STATE_FREEING: u32 = 0xFFFF_FFF8` and recognized it in NVS page header validity checks.
- In `load_nvs_wifi_credentials_split_12kb`: Pass 2 scans strictly Page 3 (`&nvs_buf[2 * NVS_PAGE_SIZE..]`), preventing Page 0 from re-scanning and overwriting newer credentials on Page 1. Added unit test `test_split_12kb_nvs_read_preserves_page1_newer_credentials_over_page0`.

### 6. `CODE-HTTP-001` (`louder-esp32-app/src/http_control.rs`)
- Implemented `write_json_escaped_str` and `JsonEscaped` formatting wrapper escaping `"`, `\`, and control characters `< 0x20`.
- Applied escaping to all dynamic string fields in `write_status_json`.
- Mapped `/reboot.json` and `/reboot_ota.json` to `Some(HttpControlAction::PersistOta0AndReboot)`.
- In `handle_http_request_streaming`: parses `POST /cmd.json` body payload after headers (`\r\n\r\n` or `\n\n`) for `act=...`.

### 7. `CODE-PIPE-002` (`louder-esp32-app/src/pipeline.rs`)
- In `process_decode_step`: inspects `written` bytes from `self.output_buffer.write(pcm_bytes)` and advances `self.pcm_pending_offset` and `self.pcm_pending_samples` strictly by committed samples (`written / 8 * 2`).
- Added `pub user_muted: bool` field, initialized to `false` in `new()`, and `pub fn set_mute(&mut self, mute: bool)`.
- In `process_output_step`: checks `if !self.user_muted { self.sink.set_mute(false); }` before flushing frames to sink.
- In `set_sample_rate`: calls `self.sink.set_sample_rate(rate)?` before updating `self.sample_rate = rate`.

### 8. `CODE-CFG-001` (`louder-esp32-app/src/wifi.rs`)
- In `WifiManager::begin_connect`: resets `self.retry_count = 0` (clearing accumulated exponential backoff delay).
- Preserves existing `static_ip`, `gateway`, `subnet`, `dns`, and `autoconnect` from previously saved `WifiCredentials` instead of wiping them on credential updates.

---

## 3. Verification Evidence
All tests compiled with `CARGO_TARGET_DIR=/tmp/target-wo143`.

1. **`squeezelite-proto`**:
   - `cargo test --manifest-path rust/Cargo.toml -p squeezelite-proto`:
     - 20 unit tests: PASS
     - 10 proptest fuzz tests: PASS
     - Total: 30 passed; 0 failed.
   - `cargo clippy --manifest-path rust/Cargo.toml -p squeezelite-proto --all-targets -- -D warnings`: PASS (0 warnings).

2. **`louder-esp32-app` (WO-143 Modules)**:
   - `coordinator`: 16 passed; 0 failed.
   - `network`: 13 passed; 0 failed.
   - `http_control`: 12 passed; 0 failed.
   - `ota_safety`: 9 passed; 0 failed.
   - `wifi`: 9 passed; 0 failed.
   - `pipeline`: 15 passed; 0 failed.
   - Total WO-143 tests: 74 passed; 0 failed.
   - Total combined tests: 104 passed; 0 failed.
   - `cargo clippy --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- -D warnings`: PASS (0 warnings).
   - `cargo clippy --manifest-path rust/Cargo.toml -p louder-esp32-app --all-targets -- -D warnings`: PASS (0 warnings).

---

## 4. Known Risks and Untested Assumptions
- None within the assigned 7 files.
- Upstream note for Wave 2 (`WO-146`): `amp::tests` in `louder-esp32-app/src/amp.rs` fail because `WO-142` (Wave 1A) updated `squeezelite-tas58xx` to write `PAGE_SELECT (0x00)` before `BOOK_SELECT (0x7F)`. Wave 2 owns `amp.rs` and will update the mock I2C expectations accordingly.
