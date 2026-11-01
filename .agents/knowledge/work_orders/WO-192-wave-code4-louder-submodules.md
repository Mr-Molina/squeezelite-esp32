# Work Order `WO-192`: `/deep-code-audit` Phase 3 (`Wave CODE-4`) — `louder-esp32-app` Submodules Remediation

- **ID**: `WO-192`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Rust Louder-ESP32 Submodules Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Wave CODE-4 Implementer** (`/deep-code-audit` Phase 3 + `rust-pro`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/audit_master_matrix.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Strictly Assigned Files (DO NOT TOUCH `main.rs`, `bt_a2dp.rs`, `lib.rs`, `board.rs`, `sink.rs`, OR ANY OTHER CRATE)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs`

### Required Functional & `rust-pro` Remediations
1. **`hal.rs`**:
   - **`CODE-HAL-FLASH-001` (`CRITICAL`, lines 563-584)**: In `NvsFlashBackend::commit_store`, call `self.flash.erase_sector(target_sector)?;` immediately before `self.flash.write_sector(target_sector, &self.scratch)?;`.
2. **`coordinator.rs`**:
   - **`CODE-COORD-EOF-001` (`CRITICAL`, lines 658-666)**: Allow transition to `PlayerState::Stopped` + `self.pending_underrun = true` when `self.decode_complete_sent && (self.state == PlayerState::Playing || self.state == PlayerState::Buffering) && pipeline.output_buffer.available_read() == 0 && !pipeline.has_pending_pcm()`.
   - **`CODE-COORD-SKIP-001` (`CRITICAL`, lines 446-452)**: In `StreamCommandType::Unknown(b'a')` skip-ahead, multiply `frames` by `8` (`frames.saturating_mul(8)`) instead of `4` (`[i32; 2]` stereo frames are 8 bytes/frame) and align to 8 bytes (`bytes & !7`). Update any unit test in `coordinator.rs` that tests `b'a'` skip-ahead to expect `8` bytes/frame.
   - **`CODE-COORD-SR-001` (`HIGH`, lines 683-691)**: In `process_audio_step`, after `pipeline.process_decode_step()`, synchronize `if pipeline.sample_rate > 0 && pipeline.sample_rate != self.current_sample_rate { self.current_sample_rate = pipeline.sample_rate; }`.
   - **`CODE-COORD-STMD-001` (`HIGH`, lines 284-296, 648-656)**:
     - In `evaluate_state` (line 650): require `!pipeline.has_pending_pcm()` before setting `self.decode_complete_sent = true`.
     - In `handle_command` HTTP connect failure (line 286): do not set `self.pending_dsco = Some(DisconnectReason::Unreachable)` when `client.send_dsco(DisconnectReason::Unreachable)?` is already sent immediately (`self.pending_dsco = None;`).
     - In `audg` handling (lines 460-476): when `digital_atten == 0`, if `gain_left == 0 && gain_right == 0` and volume was not explicitly muted (`gain_left == 0` is only mute when `digital_atten != 0` or both gains are 0 with `self.volume.gain_left == 0`), ensure `digital_atten == 0 && (gain_left > 0 || gain_right > 0)` uses `(65536, 65536)` as intended by existing tests.
   - **`CODE-COORD-TEST-001` (`MEDIUM`, lines 1247-1276)**: Fix `test_flush_resets_buffer_threshold_and_exits_buffering` so it invokes `coordinator.handle_command(flush_cmd, &mut pipeline, &mut client, 100)` over a loopback `SlimprotoClient` instead of mutating fields manually.
3. **`pipeline.rs`**:
   - **`CODE-PIPE-001` (`HIGH`, line 28 & `set_sample_rate`)**:
     - Increase `READ_SCRATCH_BYTES` on `#[cfg(target_arch = "xtensa")]` from `1536` to `1728` (matching max MP3 frame size `1728` B).
     - In `StreamPipeline::set_sample_rate`, call `self.equalizer.reset_history();` when the sample rate changes so stale biquad filter state from a prior sample rate cannot cause a transient pop.
4. **`http_control.rs`, `network.rs`, `ota_safety.rs`**:
   - **`CODE-HTTP-VOL-001` (`http_control.rs:7, 428, 485`)**: Use `snapshot.vol_pct.saturating_add(5).min(100)` in `"volup"`, use `other[3..].parse::<u32>().map(|v| v.min(100) as u8).unwrap_or(snapshot.vol_pct)` in `vol<digits>`, and update line 7 doc comment for `POST /reboot.json`.
   - **`CODE-NET-DISC-001` (`network.rs:30-38`)**: Add a wall-clock `Instant::now()` deadline check inside `LmsDiscovery::discover_server` loop.
   - **`CODE-OTA-NVS-001` (`ota_safety.rs:547-555`)**: In `scan_nvs_credentials`, `scan_nvs_namespaces`, and `parse_nvs_wifi_credentials`, check `let written = is_nvs_entry_written(bitmap, idx);` first and compute `let span = if written { usize::from(entry[2]).clamp(1, NVS_ENTRIES_PER_PAGE - idx) } else { 1 };` so unwritten `0xFF` entries advance by `1` instead of jumping `255` entries past the page end.
5. **`GRS-012` Line & Comment Widths**:
   - Wrap all code lines `<= 100` cols and standalone comments `<= 80` cols across all 9 assigned files (`coordinator.rs`, `pipeline.rs`, `hal.rs`, `amp.rs`, `network.rs`, `http_control.rs`, `ota_safety.rs`, `wifi.rs`, `cyd.rs`).
6. **Verification Gate**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs --strict` and verify **0 errors and 0 warnings** (`exit code 0`).
   - Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code4.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section

- **Execution Date**: `2026-10-27T19:17:00-04:00`
- **Result Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code4.md`

### Completed Action Items
1. **`hal.rs`**: Implemented `self.flash.erase_sector(target_sector)?;` before `write_sector` in `NvsFlashBackend::commit_store` (`CODE-HAL-FLASH-001`). [VERIFIED]
2. **`coordinator.rs`**:
   - Implemented `CODE-COORD-EOF-001`: allowed `PlayerState::Buffering` in EOF output drain transition to `Stopped` + `pending_underrun = true`. [VERIFIED]
   - Implemented `CODE-COORD-SKIP-001`: `frames.saturating_mul(8)` and 8-byte alignment `bytes & !7` for stereo skip-ahead. [VERIFIED]
   - Implemented `CODE-COORD-SR-001`: sample rate synchronization in `process_audio_step`. [VERIFIED]
   - Implemented `CODE-COORD-STMD-001`: verified `!pipeline.has_pending_pcm()` before `decode_complete_sent = true`, cleared `pending_dsco = None` on immediate HTTP connect failure, preserved unity gain when `digital_atten == 0`. [VERIFIED]
   - Implemented `CODE-COORD-TEST-001`: rewrote `test_flush_resets_buffer_threshold_and_exits_buffering` with real `handle_command` and loopback client. [VERIFIED]
3. **`pipeline.rs`**: Set `READ_SCRATCH_BYTES = 1728` on `xtensa`, added `self.equalizer.reset_history()` on sample rate changes (`CODE-PIPE-001`). [VERIFIED]
4. **`amp.rs`**: Updated mock expectations to account for `select_page0_book0` preamble writes in `squeezelite-tas58xx`. [VERIFIED]
5. **`http_control.rs`**: Fixed volume arithmetic with saturating add / clamp, updated reboot endpoints doc comment (`CODE-HTTP-VOL-001`). [VERIFIED]
6. **`network.rs`**: Added wall-clock deadline check in `discover_server` loop (`CODE-NET-DISC-001`). [VERIFIED]
7. **`ota_safety.rs`**: In NVS scan loops, bounded unwritten `0xFF` entry advance to 1 (`CODE-OTA-NVS-001`). [VERIFIED]
8. **`GRS-012` Compliance**:
   - `google_rust_audit.py --strict` executed on all 9 assigned files (`coordinator.rs`, `pipeline.rs`, `hal.rs`, `amp.rs`, `network.rs`, `http_control.rs`, `ota_safety.rs`, `wifi.rs`, `cyd.rs`).
   - Result: **0 errors, 0 warnings across all 9 modules**.
9. **Crate Unit Tests**:
   - `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- --skip bt_a2dp`: **105 passed, 0 failed**.
