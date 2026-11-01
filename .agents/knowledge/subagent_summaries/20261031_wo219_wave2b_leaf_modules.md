# Subagent Summary: WO-219 — Wave 2B: Reconcile `louder-esp32-app` Leaf Submodules & `esp-radio` BTDM Vendor Crate

- **Work Order**: `WO-219` (`/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-219-wave2b-louder-leaf-modules-and-espradio.md`)
- **Timestamp**: `2026-10-31T16:50:45-04:00`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Files Created, Modified, or Checked Out (Absolute Paths)
1. `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/ld/esp32_provides.x` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`) (`BTDM` linker symbol provides).
2. `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`) (`BTDM` controller initialization and `GRS` audit compliance).
3. `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/mod.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`).
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml` — Reconciled `cd139b61` features (`std-codecs`, `esp-rtos-core1`, `runtime-flash-persist`), `embassy-sync`, `embassy-executor`, `embassy-time`, `static_cell`, `critical-section`, `proptest`, and `smoltcp` `socket-dns` with `627f740b` dependencies (`squeezelite-raop`, `squeezelite-cspot`, and `smoltcp` `alloc`).
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs` — Reconciled `WO-192` `GRS-001`–`GRS-012` audit fixes with `cd139b61` `mute(bool)` and `is_muted()` methods and unit tests.
6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/board.rs` — Reconciled `BASEMENT_HELO_CAPABILITIES` (`MaxSampleRate=96000,flc,aac,ogg,alc,ops,pcm,wav,aif,mp3`), Phase 1 `OUTPUT_RING_BYTES` (`2000 * 1024`), `first_boot_defaults` + top-level legacy constant re-exports, and `WO-193` `GRS-001`–`GRS-012` audit fixes.
7. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` — Reconciled `WO-191` `GRS-001`–`GRS-012` audit fixes with `cd139b61` `bt-hci` `WriteClassOfDevice`/`WriteDefaultLinkPolicySettings` fixes.
8. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`).
9. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`).
10. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` — Reconciled `WO-192`/`WO-199` (`decode_us_max`, `decode_us_first`, Spotify `/cmd.json` actions, `GRS-001`–`GRS-012` audit fixes) with `cd139b61` `i2c_nack_by_op`, `DEFAULT_PSRAM_FREE_BYTES`, `DEFAULT_PSRAM_USED_BYTES`, `SetPlayerName`, `SetWifiCredentials`, and tests.
11. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs` — Reconciled `WO-192` `GRS-001`–`GRS-012` audit fixes with `cd139b61` `PLAYER_NAME_MAX_LEN`, `AudioPersistedConfig::{player_name, player_name_len, set_player_name, player_name_str}`, and unit tests.
12. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`).
13. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/wifi.rs` — Checked out from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`).
14. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/backend_device.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
15. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/clock_table.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
16. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/config_handle.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
17. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/io_traits.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
18. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/io_traits/conformance.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
19. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/net_device.rs` — Formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
20. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/stream_codec.rs` — Resolved `GRS-009` (line 366 combinator chain in `for` loop header) and formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`).
21. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs` — Resolved `GRS-010` (10 `.unwrap()` calls replaced with `.expect(...)`), formatted to `<= 80` comment / `<= 100` code columns (`GRS-012`), and updated `HttpStatusSnapshot`/`ActiveDecoder`/`HttpControlAction` arms in `device` for Wave 3 compatibility.

## 2. Verification Evidence
- Executed `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py ... --strict` across all 17 owned `.rs` files in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/`: **`0 error(s), 0 warning(s)`** (exit code `0`).
- Verified `/usr/bin/git status --short` confirms `main.rs`, `lib.rs`, `coordinator.rs`, `pipeline.rs`, and `network.rs` were not modified.

## 3. Known Risks / Notes for Wave 3 (`WO-220`)
- In `amp.rs`, `WO-192` renamed `get_bus()` / `get_bus_mut()` to `bus()` / `bus_mut()` per `GRS-011`. Any call sites in `main.rs` or `lib.rs` from `cd139b61` should use `bus()` / `bus_mut()`.
- In `http_control.rs`, `HttpStatusSnapshot` includes both `i2c_nack_by_op: [u32; 4]` (from `cd139b61`) and `decode_us_max: u32` + `decode_us_first: u32` (from `627f740b`). When `WO-220` reconciles `main.rs`, `HttpStatusSnapshot` initialization in `main.rs` should include `i2c_nack_by_op: Esp32I2cBus::hw_nack_by_op(), decode_us_max: pipeline.decode_us_max, decode_us_first: pipeline.decode_us_first`.
