# Work Order `WO-069`: Deep Technical & Memory-Budget Audit of Bare-Metal ESP32 VHCI Bluetooth Classic (`BR/EDR` A2DP Sink + SBC) vs. C Recovery Partition (`0x10000`)

- **ID**: `WO-069`
- **Timestamp**: `2026-10-25T09:05:00-04:00`
- **Subagent Role**: Bluetooth Classic VHCI & A2DP Sink Architecture Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Perform a read-only symbol-level, protocol-level, and DRAM-budget audit of implementing Bluetooth Classic Audio (`A2DP Sink` + `SBC`) on `172.22.1.15` (`"BASEMENT"`, ESP32-WROVER):
1. **Inspect `esp-radio 1.0.0-beta.1` & `esp-wifi-sys` Bluetooth Controller Symbols in Cargo Cache (`/home/console-admin/.cargo/`)**:
   - Check the exact `btdm` symbols and structs in `esp-radio` / `esp-wifi-sys`:
     - `btdm_controller_init`, `btdm_controller_enable`, `API_vhci_host_check_send_available`, `API_vhci_host_send_packet`, `API_vhci_host_register_callback`, `esp_bt_controller_config_t`, `esp_bt_mode_t`.
     - Look at why `esp-radio-1.0.0-beta.1/src/ble/btdm.rs` and `os_adapter_esp32.rs` pass `ESP_BT_MODE_BLE` (`1`) and `bt_max_acl_conn: 0`, and check whether `libbtdm_app.a` in `esp-wifi-sys` includes the Classic BR/EDR link manager (`LM`/`LC`) symbols or if `esp-radio`'s `BleConnector` only exposes raw HCI packet read/write over VHCI.
2. **Audit DRAM (`dram_seg`) & Heap Budget with Wi-Fi + Bluetooth Coexistence on ESP32**:
   - Currently, `louder-esp32-app` on `172.22.1.15` has:
     - `64 KB` `esp_alloc` heap in `dram_seg` (used by `esp-radio` Wi-Fi STA)
     - `.stack = 36,528` bytes (`0x008eb0`), and we proved empirically in `WO-050` (`.stack = 27,824` B) and `WO-065` (`.stack = 28,288` B) that reducing `.stack` by `8 KB` causes an immediate boot stack overflow during `esp-radio` initialization!
   - Measure how much additional internal `DRAM` (`dram_seg` `.bss` + `esp_alloc` heap) enabling `btdm_controller_init` + `btdm_controller_enable` requires on ESP32 alongside Wi-Fi (`coex_init`), and whether `4 MB` external PSRAM (`esp_hal::psram`) can hold the HCI/L2CAP/AVDTP/SBC state while keeping internal `dram_seg` `.stack >= 36,528` B.
3. **Audit Minimal Bare-Metal HCI Classic A2DP Sink State Machine vs. C `recovery` Partition (`0x10000`)**:
   - Detail the exact HCI/L2CAP/SDP/AVDTP/SBC packet layer required for a phone (iOS/Android) to discover, pair (SSP `HCI_IO_Capability_Request_Reply` `NoInputNoOutput`), connect SDP (`PSM 0x0001`, UUID `0x110B` AudioSink), negotiate AVDTP (`PSM 0x0019`, `Discover`, `GetCapabilities`, `SetConfiguration`, `Open`, `Start`), and stream RTP-SBC frames (`Media Packet` -> SBC decoder -> `StreamPipeline`).
   - Also check whether the C `recovery` partition (`0x10000`, `I2S-4MFlash-1681`) on `172.22.1.15` has Bluetooth A2DP Sink compiled in (`components/driver_bt/` / NVS `bt_name` / `enable_bt_sink` in `0x9000` NVS).
4. Write your complete findings to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T090500_wo069_bt_vhci_a2dp_audit.md`, update `WO-069` Fulfillment Section, and reply with a concise synthesis.

## 2. Mandatory Constraints
- Read-only research subagent. Do NOT modify source files or touch `/dev/ttyUSB0`.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Bluetooth Classic VHCI & A2DP Sink Architecture Auditor (`d83a2c5c-eb51-45af-bf18-7e06ffd5d9d4`, `Model: "pro"`)
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T090500_wo069_bt_vhci_a2dp_audit.md`
- **Key Findings**:
  1. `libbtdm_app.a` in `esp-wifi-sys-esp32-0.3.0` contains full Classic BR/EDR Link Controller (`lc.o`, `ld_acl.o`, `ld_pscan.o`) and Link Manager (`lm.o`) code, exposed via standard VHCI functions (`API_vhci_host_send_packet`, `API_vhci_host_register_callback`).
  2. `esp-radio 1.0.0-beta.1` hardcodes `ESP_BT_MODE_BLE` (`mode: 1`, `bt_max_acl_conn: 0`) and enforces `MAX_HCI_PACKET_LEN = 259` in `HciOutCollector`, which drops Classic ACL frames. Furthermore, no `no_std` Bluetooth Classic host stack (`L2CAP`/`SDP`/`AVDTP`/`AVRCP`) exists in Rust.
  3. `dram_seg` in `louder-esp32-app` is currently 100.00% utilized (`196,608` B / `192 KB`). Enabling BTDM Classic requires reserving `0xDB5C` (`54.84 KB`) for hardware Exchange Memory, shrinking `dram_seg` to `136 KB`. Concurrent Wi-Fi + BT Classic is physically impossible in internal DRAM without moving audio ringbuffers and `STATIC_MP3_DEC` (`~55 KB`) to external `4 MB` PSRAM (`esp_hal::psram`).
  4. The C `recovery` partition (`0x10000`, `I2S-4MFlash-1681`) has Bluetooth Classic A2DP Sink compiled in, reporting `"bt_status": 0`, and activates when NVS key `"enable_bt_sink"` is set to `"1"` in partition `0x9000`.
