# Work Order: WO-219 — Wave 2B: Reconcile `louder-esp32-app` Leaf Submodules & `esp-radio` BTDM Vendor Crate onto `MK5-Rust` (`cd139b61`)

- **Work Order ID**: `WO-219`
- **Timestamp**: `2026-10-31T16:27:00-04:00`
- **Assigned Subagent Role**: `Rust Leaf Modules & BTDM Reconciler` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/` (Leaf Submodules) & `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Reconcile the `louder-esp32-app` leaf submodules (`amp.rs`, `board.rs`, `bt_a2dp.rs`, `cyd.rs`, `hal.rs`, `http_control.rs`, `ota_safety.rs`, `sink.rs`, `wifi.rs`), `louder-esp32-app/Cargo.toml`, and `rust/vendor/esp-radio` between `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`) and `MK5-Rust` (`cd139b61`), and audit all Phase 0+1 leaf modules (`backend_device.rs`, `clock_table.rs`, `config_handle.rs`, `io_traits.rs`, `io_traits/conformance.rs`, `net_device.rs`, `stream_codec.rs`, `tasks.rs`) for `GRS-001`–`GRS-012` compliance.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN**:
     - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/**`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{amp.rs,board.rs,bt_a2dp.rs,cyd.rs,hal.rs,http_control.rs,ota_safety.rs,sink.rs,wifi.rs}`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{backend_device.rs,clock_table.rs,config_handle.rs,io_traits.rs,net_device.rs,stream_codec.rs,tasks.rs}` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/io_traits/**`
   - Do **NOT** modify `main.rs`, `lib.rs`, `coordinator.rs`, `pipeline.rs`, or `network.rs` in `louder-esp32-app/src/` (reserved for Wave 3 `WO-220`), and do **NOT** modify `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml` or run `cargo` commands (since `WO-218` is concurrently building shared crates in `target/`).
3. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. **Checkout Non-Overlapping Owned Files from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`)**:
   - Execute:
     ```bash
     /usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 checkout local/wo157-wo214-codecs-airplay-spotify -- \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs
     ```
2. **Reconcile the 6 Overlapping Leaf Files (`b49854f3..cd139b61` vs `b49854f3..627f740b`)**:
   - Inspect both sides via `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..cd139b61 -- <path>` and `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..local/wo157-wo214-codecs-airplay-spotify -- <path>`:
     1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`: Combine Phase 0+1 dependencies/features (`embassy-executor`, `embassy-time`, `core1-audio`, `bt-hci` resolution fix from `cd139b61`) with `squeezelite-raop = { path = "../squeezelite-raop", default-features = false, features = ["alloc"] }` and `squeezelite-cspot = { path = "../squeezelite-cspot", default-features = false, features = ["alloc"] }` from `627f740b`.
     2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`: Merge `cd139b61` changes with `WO-192` audit hardening (`GRS-001`–`GRS-012`).
     3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`: Preserve both `BASEMENT_HELO_CAPABILITIES` (`MaxSampleRate=96000,flc,aac,ogg,alc,ops,pcm,wav,aif,mp3`), Phase 1 `OUTPUT_RING_BYTES` (`2000 * 1024` / `2000 KiB` from `b46df60c`), and `WO-193` `GRS-001`–`GRS-012` audit fixes.
     4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`: Merge `cd139b61` `bt-hci` fix with `WO-191` audit hardening (`GRS-001`–`GRS-012`).
     5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`: Merge `cd139b61` changes with `WO-192`/`WO-199` (`decode_us_max` in `status.json`) and `GRS-001`–`GRS-012` audit fixes.
     6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`: Merge `cd139b61` changes with `WO-192` `GRS-001`–`GRS-012` audit fixes.
3. **Audit & Harden Phase 0+1 Leaf Modules (`GRS-001`–`GRS-012`)**:
   - Inspect `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{backend_device.rs,clock_table.rs,config_handle.rs,io_traits.rs,net_device.rs,stream_codec.rs,tasks.rs}` and `io_traits/conformance.rs` using `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src --strict` and resolve all `GRS-001`–`GRS-012` violations in your owned files (do not touch `main.rs`, `lib.rs`, `coordinator.rs`, `pipeline.rs`, `network.rs`).
4. **Deliverables**:
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-219-wave2b-louder-leaf-modules-and-espradio.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo219_wave2b_leaf_modules.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-31T16:50:45-04:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo219_wave2b_leaf_modules.md`

### Summary of Reconciled & Audited Files
1. **Non-Overlapping Owned Files Checked Out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`)**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs` (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs` (`[EXECUTED & VERIFIED]`)
2. **Overlapping Leaf Files Reconciled (`b49854f3..cd139b61` + `b49854f3..627f740b`)**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`: Combined `cd139b61` features (`std-codecs`, `esp-rtos-core1`, `runtime-flash-persist`), `embassy-sync`, `embassy-executor`, `embassy-time`, `static_cell`, `critical-section`, `proptest`, and `smoltcp` `socket-dns` with `627f740b` dependencies (`squeezelite-raop`, `squeezelite-cspot`, and `smoltcp` `alloc`). (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`: Merged `WO-192` `GRS-001`–`GRS-012` audit hardening (`bus` / `bus_mut` getters, `#[non_exhaustive]` on `AmpMode`/`AmpError`) with `cd139b61`'s `mute(bool)`, `is_muted()`, and unit tests. (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs`: Preserved `BASEMENT_HELO_CAPABILITIES` (`MaxSampleRate=96000,flc,aac,ogg,alc,ops,pcm,wav,aif,mp3`), Phase 1 `OUTPUT_RING_BYTES` (`2000 * 1024`), `pub mod first_boot_defaults` + legacy aliases (`DEFAULT_LMS_SERVER_IP`, `DEFAULT_LMS_SERVER_PORT`, `DEFAULT_PLAYER_NAME`, `DEFAULT_WIFI_SSID`, `BASEMENT_PLAYER_MAC`), and `WO-193` `GRS-001`–`GRS-012` audit fixes. (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`: Combined `WO-191` `GRS-001`–`GRS-012` audit hardening (`ConnectionState`, `StreamingState`, `write_avdtp_media_sbc`) with `cd139b61`'s `bt-hci` `WriteClassOfDevice`/`WriteDefaultLinkPolicySettings` fixes. (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`: Combined `WO-192`/`WO-199` (`decode_us_max`, `decode_us_first`, Spotify `/cmd.json` actions, `GRS-001`–`GRS-012` audit fixes) with `cd139b61`'s `i2c_nack_by_op`, `DEFAULT_PSRAM_FREE_BYTES`, `DEFAULT_PSRAM_USED_BYTES`, `SetPlayerName`, `SetWifiCredentials`, and tests. (`[EXECUTED & VERIFIED]`)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`: Combined `WO-192` `GRS-001`–`GRS-012` audit fixes (`FlashTargetPolicy`, `select_flash_target`) with `cd139b61`'s `PLAYER_NAME_MAX_LEN`, `AudioPersistedConfig::{player_name, player_name_len, set_player_name, player_name_str}`, and unit tests. (`[EXECUTED & VERIFIED]`)
3. **Phase 0+1 Leaf Modules Audited & Hardened (`GRS-001`–`GRS-012`)**:
   - Resolved `GRS-009` in `stream_codec.rs`, `GRS-010` in `tasks.rs`, and `GRS-012` (`<= 100` code / `<= 80` comments) across `backend_device.rs`, `clock_table.rs`, `config_handle.rs`, `io_traits.rs`, `io_traits/conformance.rs`, `net_device.rs`, `stream_codec.rs`, and `tasks.rs`.
   - Updated `tasks.rs` `http_serve` and `decode_burst` to include `decode_us_max`/`decode_us_first` in `HttpStatusSnapshot`, `Alac`/`Opus` in `ActiveDecoder`, and wildcard arm `_ => {}` on `HttpControlAction` for seamless Wave 3 integration.
   - Verified `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py ... --strict` across all 17 owned `.rs` files in `louder-esp32-app/src/`: **`0 error(s), 0 warning(s)`** (exit code `0`).
