# Work Order `WO-149`: `esp-radio` Dual-Mode (`BTDM`) VHCI Patch & Pure-Rust Bluetooth Classic A2DP Sink Feasibility Audit

- **ID**: `WO-149`
- **Timestamp**: `2026-10-26T20:21:00-04:00`
- **Subagent Role**: ESP32 BTDM Controller & A2DP Sink Architecture Researcher (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Objectives (Read-Only Deep Dive)
Inspect `/home/console-admin/.cargo/registry/src/*/esp-radio-1.0.0-beta.1/src/ble/{mod.rs,btdm.rs,os_adapter_esp32.rs}` and `/home/console-admin/.cargo/registry/src/*/esp-wifi-sys-esp32-0.3.0/` (`include.rs` & `libbtdm_app.a`) to determine the exact requirements for enabling **Dual-Mode Bluetooth (`ESP_BT_MODE_BTDM`: Classic BR/EDR A2DP Sink + BLE)** on `louder-esp32-app` (`172.22.1.15`, `C8:2E:18:BE:B8:36`):

1. **`esp_bt_controller_config_t` & `btdm_controller_enable` in `esp-radio-1.0.0-beta.1`**:
   - Read `os_adapter_esp32.rs` (`BT_CONTROLLER_CFG` / `create_ble_config`) and `btdm.rs` (`ble_init`).
   - Verify the exact fields of `esp_bt_controller_config_t` in `os_adapter_esp32.rs` (`mode`, `ble_max_conn`, `bt_max_acl_conn`, `bt_sco_datapath`, `auto_latency`, `bt_legacy_auth_vs_evt`, `bt_max_sync_conn`, `pcm_role`, etc.) and compare them with ESP-IDF's `BT_CONTROLLER_INIT_CONFIG_DEFAULT()` in C `squeezelite-esp32` (or `esp-idf` headers in the repo).
   - Verify what happens in `btdm_controller_mem_init()` in `btdm.rs` (`_bss_start_btdm.._bss_end_btdm`, `_data_start_btdm.._data_end_btdm`, and exchange memory `0x3ffb0000..0x3ffbdb5c`). Does `btdm_controller_mem_init()` in `btdm.rs` already initialize the full `0x3ffb0000..0x3ffbdb5c` BTDM memory range?
2. **`dram_seg` & `.dram2_uninit` Linker Memory Budget with `"ble"` + `"coex"` Enabled**:
   - Check `/home/console-admin/.cargo/registry/src/*/esp-hal-1.2.2/` linker scripts (`esp32.x` / `memory.x`) to see where `dram_seg` starts when `esp-radio` `"ble"` is enabled (`0x3ffb0000` vs `0x3ffbdb60`).
   - In `louder-esp32-app/src/main.rs`, calculate the exact byte sizes of all static variables currently in `.bss` (`HEAP: 64 KB`, `BSS_BUFFERS: 25 KB`, `STATIC_MP3_DEC: 22.8 KB`) that can be moved to `.dram2_uninit` (`dram2_seg` `0x3FFE_7E40..0x4000_0000` = `98,768` B) or allocated from the `4 MB` external PSRAM (`0x3F80_0000`) so that `dram_seg` has `>= 96 KB` for `esp_alloc` `Internal` heap (Wi-Fi + BTDM) AND `>= 36,000` B for `.stack`!
3. **Pure-Rust HCI Classic BR/EDR + A2DP Sink over `BleConnector` (VHCI)**:
   - Verify how `BleConnector::read` and `BleConnector::write` exchange raw H4 HCI packets (`0x01` Command, `0x02` ACL Data, `0x04` Event) with `API_vhci_host_send_packet` and `VHCI_HOST_CALLBACK`.
   - Confirm that VHCI carries both BLE and Classic BR/EDR HCI Commands, Events (`Connection_Request`, `IO_Capability_Request`, `User_Confirmation_Request`), and ACL Data (`0x02`, L2CAP PSM `0x0001` SDP and PSM `0x0019` AVDTP/A2DP Sink SBC audio stream) when `btdm` is initialized in `ESP_BT_MODE_BTDM` (`3`)!
4. **Hardware Safety**:
   - **NEVER touch `/dev/ttyUSB0`**. Read-only investigation.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**:
  1. Verified `os_adapter_esp32.rs:431-446` (`btdm_controller_mem_init`) already initializes the entire `0x3ffb0000..0x3ffbdb5c` range (including `BREDR` exchange memory `0x3ffb2730..0x3ffb6388`) and wires `RWBT()`, `RWBLE()`, and `BT_BB()` interrupt vectors (`os_adapter_esp32.rs:551-561, 670-686`).
  2. Identified the exact 5-line patch in `os_adapter_esp32.rs:377-383` (`mode: esp_bt_mode_t_ESP_BT_MODE_BTDM as _`, `bt_max_acl_conn: 2`, `bt_max_sync_conn: 0`, `bt_legacy_auth_vs_evt: true`) and `btdm.rs:375` (`btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BTDM)`).
  3. Verified `dram_seg` (`128 KB` at `0x3FFC0000..0x3FFE0000` with `RESERVE_DRAM = 0x10000`) and `dram2_seg` (`98,768` B at `0x3FFE_7E30..0x4000_0000`) memory layout: relocating `BSS_BUFFERS` (`29,184` B) and `STATIC_MP3_DEC` (`22,792` B) to `.dram2_uninit` plus a `32 KB` `STATIC_BT_HEAP` in `.dram2_uninit` yields `96 KB` (`64 KB + 32 KB`) of `MemoryCapability::Internal` heap while keeping `.stack >= 36,000` B (`~40,960` B) in `dram_seg`.
  4. Verified `BleConnector` H4 VHCI (`API_vhci_host_send_packet` / `receive_packet`) is transport-agnostic and carries Classic `BR/EDR` HCI Commands (`0x01`), ACL Data (`0x02`), and Events (`0x04`).
