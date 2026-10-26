# Work Order `WO-054`: `/deep-code-audit` Micro-Auditor 2 — Bluetooth (A2DP Sink, AVRCP, BLE & ESP32 Radio Coexistence) Feature-Parity Audit

- **ID**: `WO-054`
- **Timestamp**: `2026-10-24T16:11:30-04:00`
- **Subagent Role**: Bluetooth A2DP & Radio Coexistence Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of Bluetooth support across the repository (`components/`, `main/`, and `rust/crates/`):
1. Inspect the C `squeezelite-esp32` codebase (`components/`, `main/`) to document every Bluetooth feature implemented in C:
   - Bluetooth Classic A2DP Sink (`bt_sink`, SBC audio decoding, sample rate / I2S sink arbitration with Squeezelite/AirPlay).
   - AVRCP controller/target (volume sync, play/pause/next/prev, track metadata).
   - Bluetooth Classic / BLE coexistence with Wi-Fi in ESP-IDF (`esp_bt`, `btdm`, DRAM memory footprint).
2. Inspect `rust/crates/*` and `Cargo.lock` (`esp-radio 1.0.0-beta.1`, `esp-hal 1.2.2`, `bt-hci`, `trouble-host`) to verify:
   - Whether any Bluetooth crate or module exists in `rust/crates/*`.
   - What `esp-radio 1.0.0-beta.1` on ESP32 (`xtensa-esp32-none-elf`) actually supports: does `esp-radio` support **BLE HCI** (`esp_radio::ble::controller::BleConnector`) vs **Bluetooth Classic BR/EDR A2DP** (which requires the proprietary ESP-IDF BTDM Classic Bluedroid/VHCI stack)?
3. Formulate structured findings (`FA-BT-*`) with exact file/line citations from `components/` and `esp-radio` crate source in `/home/console-admin/.cargo/` proving what is in C, what is missing in `rust/`, and the exact technical capability of `esp-radio` on ESP32 (BLE vs BT Classic BR/EDR A2DP).

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify any repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `4d883d79-dd54-42a7-8dd4-78d316fa614f` (`Bluetooth A2DP & Radio Auditor`)
- **Key Findings (`FA-BT-01`..`FA-BT-07`)**:
  - `FA-BT-01`: C `components/driver_bt/bt_app_core.c` & `bt_app_sink.c` implement Bluetooth Classic A2DP Sink using ESP-IDF's Bluedroid BTDM Classic stack (`esp_bt_controller_enable(ESP_BT_MODE_CLASSIC_BT)`, `esp_bluedroid_enable()`, `esp_a2d_sink_init()`, internal OI_Codec SBC decoder).
  - `FA-BT-02`: C `bt_app_sink.c` implements dual-role AVRCP Controller + Target (`esp_avrc_ct_init()`, `esp_avrc_tg_init()`) with bidirectional absolute volume sync and track metadata.
  - `FA-BT-03`: C `components/squeezelite/decode_external.c` arbitrates `DECODE_BT`, `DECODE_RAOP`, and `DECODE_CSPOT` against LMS SlimProto.
  - `FA-BT-04`: C `sdkconfig.defaults:443` reserves `0xdb5c` (`~56 KB`) of internal DRAM exclusively for the BTDM controller blob (`CONFIG_BT_RESERVE_DRAM=0xdb5c`) and routes Bluedroid host allocations to external PSRAM (`CONFIG_BT_ALLOCATION_FROM_SPIRAM_FIRST=y`).
  - `FA-BT-05` & `FA-BT-06`: `rust/crates/*` has 0 lines of Bluetooth code, and `esp-radio 1.0.0-beta.1` (`src/ble/btdm.rs:375`, `os_adapter_esp32.rs:377-380`) hardcodes `btdm_controller_enable(ESP_BT_MODE_BLE)` with `bt_max_acl_conn: 0` (BLE HCI only; Classic BR/EDR disabled).
  - `FA-BT-07`: Because original ESP32 (Bluetooth 4.2) lacks BLE 5.2 Isochronous Channels (LE Audio) and `esp-radio` + embedded Rust crates (`trouble-host`, `bt-hci`) are BLE-only (no Classic L2CAP/SDP/AVDTP/AVRCP host stack), Bluetooth Classic A2DP requires either a custom BR/EDR L2CAP+AVDTP+SDP+SBC host stack + `esp-radio` BTDM mode patch + PSRAM, or an ESP-IDF Bluedroid shim.
