# Vendored Crate: `esp-radio` (1.0.0-beta.1)

- **Upstream Crate**: `esp-radio` (version 1.0.0-beta.1, crates.io)
- **License**: MIT OR Apache-2.0
- **Repository**: https://github.com/esp-rs/esp-hal
- **Vendored In**: Work Orders `WO-150` (`b95d352c`) and `WO-154` (`1a7b9912`); see `.agents/knowledge/DIAGNOSTIC_LOG.md`
- **Wired In By**: `[patch.crates-io]` in `rust/Cargo.toml`

## Purpose
Upstream `esp-radio` runs the ESP32 Bluetooth controller in BLE-only mode. The
firmware needs Bluetooth Classic (BR/EDR) for A2DP, so this copy runs the
controller in dual mode (BTDM) and fills in the OS-adapter callbacks that the
dual-mode controller calls and upstream leaves as `todo!()`.

## Applied Patches
This list was produced by diffing this directory against the unmodified
crates.io 1.0.0-beta.1 sources (2026-11-16). Five files differ; everything else
is identical to upstream.

1. **Controller mode (`src/ble/btdm.rs`, `src/ble/os_adapter_esp32.rs`)**:
   - `btdm_controller_enable` and the controller config `mode` use
     `ESP_BT_MODE_BTDM` instead of `ESP_BT_MODE_BLE`.
   - Controller config: `bt_max_acl_conn` 0 -> 2, `bt_max_sync_conn` 1 -> 0,
     `bt_legacy_auth_vs_evt` false -> true.
   - `max_connections` and `scan_duplicate_list_count` are fixed at 1 and 20
     instead of the `CONFIG_BTDM_*` values.
   - `patch_apply` also calls `config_bt_funcs_reset()` (between the BTDM and
     BLE resets), which upstream omits.
2. **OS-adapter callbacks (`src/ble/btdm.rs`)**:
   - `mutex_create/delete/lock/unlock` forward to `crate::compat::mutex`
     (upstream: `todo!()`).
   - `custom_queue_create` forwards to `common_adapter::queue_create`
     (upstream: `todo!()`).
   - `btdm_lpcycles_2_hus` returns `cycles * 2`; `btdm_sleep_check_duration`
     returns 0; the five `btdm_sleep_*_phase*` callbacks are no-ops (upstream:
     `todo!()` in all of them). Controller sleep is therefore not implemented.
   - `interrupt_l3_disable/restore` call `interrupt_disable/enable` (upstream:
     empty).
   - `task_create` skips writing the task handle when the out-pointer is null.
3. **HCI transport (`src/ble/btdm.rs`, `src/ble/mod.rs`)**:
   - `MAX_HCI_PACKET_LEN` 259 -> 1048, for ACL data packets.
   - The receive queue is capped at 32 packets; packets arriving beyond that
     are dropped.
   - `send_hci` yields while waiting and gives up after 10,000 spins, returning
     0, instead of busy-waiting forever.
   - `read_next` clamps the copy to the caller's buffer length.
4. **Linker script (`ld/esp32_provides.x`)**:
   - Adds ROM addresses for `ld_iscan_evt_start_cbk`, `ld_page_evt_start_cbk`,
     `ld_pscan_evt_start_cbk` and `lm_get_total_pkt_type` (inquiry scan, page
     and page scan).
   - Removes the `sleep`/`usleep` `EXTERN`/`PROVIDE` entries and a duplicated
     `EXTERN( __esp_radio_putchar )`.
5. **Comments only (`src/lib.rs`)**:
   - `JUSTIFICATION`/`SAFETY` comments on existing `unsafe` blocks. No code
     change.

## Upgrading
Re-apply the patches above on top of the new upstream release, then re-run the
firmware gates and a Bluetooth A2DP hardware check. The ROM addresses in item 4
are specific to the ESP32 ROM and do not apply to other chips.
