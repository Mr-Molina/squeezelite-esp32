# Subagent Summary: `WO-150` `esp-radio` BTDM Vendor Patch & `dram2_seg` Memory Rebalancing

- **Timestamp**: `2026-10-26T21:33:00-04:00`
- **Role**: `wo150_btdm_vendor_implementer`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Modified, Created, and Deleted Files

### Created Files
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/**`
  - Cloned from `/home/console-admin/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-radio-1.0.0-beta.1`
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261026_wo150_btdm_vendor_implementer.md`

### Modified Files
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/os_adapter_esp32.rs`
  - Lines 377-383: Patched controller configuration fields:
    - `mode: esp_bt_mode_t_ESP_BT_MODE_BTDM as _` (Dual-Mode Classic BR/EDR + BLE)
    - `bt_max_acl_conn: 2` (Allow 2 Classic ACL connections for audio + controls)
    - `bt_max_sync_conn: 0` (No SCO voice channels needed for A2DP)
    - `bt_legacy_auth_vs_evt: true` (Enable vendor-specific legacy auth events for pairing)
- `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/btdm.rs`
  - Line 375: Updated `btdm_controller_enable` invocation from `esp_bt_mode_t_ESP_BT_MODE_BLE` to `esp_bt_mode_t_ESP_BT_MODE_BTDM`.
- `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
  - Added `esp-radio = { path = "vendor/esp-radio" }` to `[patch.crates-io]`.
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
  - Enabled `"ble"` and `"coex"` features on `esp-radio`: `features = ["esp32", "wifi", "ble", "coex", "unstable"]`.
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - Relocated `STATIC_MP3_DEC` (22,792 B) to `#[unsafe(link_section = ".dram2_uninit")]`.
  - Relocated `BSS_BUFFERS` (29,184 B) to `#[unsafe(link_section = ".dram2_uninit")]`.
  - Defined `STATIC_BT_HEAP: core::mem::MaybeUninit<[u8; 19 * 1024]>` in `#[unsafe(link_section = ".dram2_uninit")]`.
  - Registered `STATIC_BT_HEAP` into `esp_alloc::HEAP` via `add_region` with `MemoryCapability::Internal.into()`.
  - Sized primary internal heap in `dram_seg` via `esp_alloc::heap_allocator!(size: 48 * 1024)` to provide total `67 KB` internal heap while keeping `.stack` at `40,232` bytes (`>= 36,000` bytes).
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-150-vendor-esp-radio-btdm-dram2.md`
  - Marked status `[EXECUTED & VERIFIED]` and added fulfillment details.

### Deleted Files
- Temporary backup files (`.bak`) created during edits were removed.

---

## 2. Rationale for Each Change
1. **Vendoring `esp-radio`**: Upstream `esp-radio` hardcodes `esp_bt_mode_t_ESP_BT_MODE_BLE` in `os_adapter_esp32.rs` and `btdm.rs`, locking the controller into BLE-only mode. Dual-mode Classic BR/EDR A2DP Sink requires `ESP_BT_MODE_BTDM` (`3`), 2 ACL connections, and legacy auth events.
2. **`dram_seg` vs `dram2_seg` Rebalancing**: When `ble` is enabled, `esp-hal` reserves 64 KB (`0x10000`) at `0x3FFB0000` for the BTDM hardware controller, reducing `dram_seg` length from 192 KB to 128 KB. Without rebalancing, `.stack` is crushed below 24 KB. Relocating `STATIC_MP3_DEC` (22.8 KB) and `BSS_BUFFERS` (29.2 KB) into `.dram2_uninit` frees 52 KB in `dram_seg`.
3. **Internal Heap & Stack Allocation**: Sizing `STATIC_BT_HEAP` at `19 KB` fully utilizes `dram2_seg` (leaving 312 bytes margin without overflow). Sizing `heap_allocator!(size: 48 * 1024)` in `dram_seg` yields `67 KB` of total internal heap (`48 KB + 19 KB`) and leaves `40,232` bytes for `.stack` (`>= 36,000` bytes).

---

## 3. Linker Memory Section Verification Evidence
Fresh `readelf -S` on `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`:
- `.data` + `.data.wifi`: `3ffc0000..3ffc4320` (`17,184` B)
- `.bss`: `3ffc4320..3ffd62d8` (`0x11FB8` = `73,656` B, includes 48 KB `HEAP` and 8 KB DMA `BUFFER`)
- `.stack`: `3ffd62d8..3ffe0000` (`0x9D28` = `40,232` B) -> **PASS** (`>= 36,000` B / `>= 0x8CA0`)
- `.dram2_uninit`: `3ffe7e40..3ffffed8` (`0x18098` = `98,456` B <= `98,768` B) -> **PASS** (`dram2_seg` fits cleanly, 312 B free)

---

## 4. Known Risks and Untested Assumptions
- Note on build environment: `/home/console-admin/export-esp.sh` exports `CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS="-C link-arg=-nostartfiles -C link-arg=-Tlinkall.x"`. Because `rust/.cargo/config.toml` already specifies these flags, sourcing `export-esp.sh` without unsetting `CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS` causes GNU `ld` to receive `-Tlinkall.x` twice and exit with `linker script file appears multiple times`. Building requires `unset CARGO_TARGET_XTENSA_ESP32_NONE_ELF_RUSTFLAGS` after sourcing `export-esp.sh` or removing the redundant export from `export-esp.sh`.
- Untested runtime: While static memory layout and compilation are strictly verified, runtime testing of dual-mode BTDM controller initialization depends on `WO-151` completing the pure-Rust VHCI A2DP Sink implementation.
