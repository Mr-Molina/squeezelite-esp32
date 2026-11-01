# Work Order `WO-172`: `/deep-hardware-audit` Phase 1 Auditor 5 (`HW-AUD-05`) — `"BASEMENT"` (`172.22.1.15`) Dual-Mode Bluetooth (`BTDM` `CA:2E:18:BE:B8:35`), 2.4 GHz Wi-Fi Coexistence & Local `hci0` RF Micro-Auditor

- **ID**: `WO-172`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: `"BASEMENT"` (`172.22.1.15`) Dual-Mode Bluetooth, 2.4 GHz Wi-Fi Coexistence & Local `hci0` RF Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `"BASEMENT" (172.22.1.15) Dual-Mode Bluetooth (BTDM Classic A2DP + BLE CA:2E:18:BE:B8:35), 2.4 GHz Wi-Fi Coexistence & Local hci0 RF Micro-Auditor` (`HW-AUD-05`)
2. **Target & Scope**: Embedded Target `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`), Local Workstation Bluetooth `hci0` (`04:7F:0E:01:21:B3`), `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs`, and `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/src/ble/{btdm.rs,os_adapter_esp32.rs}`.
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: 2.4 GHz RF link health (`rssi: -48 dBm` Wi-Fi on SSID `"iot"`, Bluetooth `CA:2E:18:BE:B8:35` responsiveness).
   - `L2-Bus-Interconnect`: Run non-destructive L2CAP echo (`sudo /usr/bin/l2ping -c 2 CA:2E:18:BE:B8:35` or `/usr/bin/sdptool browse CA:2E:18:BE:B8:35`) and check `GET http://172.22.1.15/status.json` `bt_rx_pkts` delta to verify live BTDM Classic BR/EDR responsiveness after >7.3 hours of continuous uptime!
   - `L3-Driver-Firmware-Config`: Inspect `rust/vendor/esp-radio/src/ble/{btdm.rs,os_adapter_esp32.rs}` and `bt_a2dp.rs` for any long-uptime resource leaks or coexistence edge cases.
   - `L4-Service-Logs`: Local host `bluetooth.service` / `obexd` journal warning (`org.bluez.Error.NotPermitted, UUID already registered`).
4. **Seeds**: Network Interfaces & RF Hardware; Embedded / MCU / IoT Hardware (`NIST-800-53:CM-6`, `CIS-HW-1.1`, `HW-FAULT:LINK-CRC`).
5. **Pre-Identified Scout Anomalies**:
   - `"BASEMENT"` has been running Dual-Mode BTDM for `7.31 hours` (`uptime_ms: 26,335,695`, `bt_enabled: true`, `bt_rx_pkts: 17`). Verify live L2CAP/SDP responsiveness after 7.3 hours of uptime!
   - Local host `journalctl` logged `obexd[12302]: bluetooth: RequestProfile error: org.bluez.Error.NotPermitted, UUID already registered`.
6. **Shell & Script Safety**: Read-only `l2ping`/`sdptool`/`curl` queries only. Never touch `/dev/ttyUSB0`.
7. **Constraint**: **Read-only telemetry inspection. Do NOT modify any configs, drivers, firmware, or services.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `d4416bc8-659d-4c48-bd52-7b36234e85ea`
- **Summary**: Verified `"BASEMENT"` Dual-Mode Bluetooth (`CA:2E:18:BE:B8:35`) + 2.4 GHz Wi-Fi coexistence is 100% healthy after 7.58 hours uptime (`l2ping` `0% loss`, `sdptool browse` `Audio Sink 0x110b`, `bt_rx_pkts: 17 -> 31`, `i2c_nacks: 0`); identified `LOG-005` (`LOW`, duplicate orphaned `obexd` `PID 12302` on `pc-1`).
