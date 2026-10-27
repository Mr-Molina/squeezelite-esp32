# Work Order `WO-122`: `/deep-hardware-audit` Phase 1 Micro-Auditor 1 — RF Physical Layer, Wi-Fi 802.11 b/g/n & Power Rail Stability

- **ID**: `WO-122`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: RF Physical Layer, Wi-Fi 802.11 b/g/n & Power Rail Stability Micro-Auditor (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: RF Physical Layer, Wi-Fi 802.11 b/g/n & Power Rail Stability Micro-Auditor
2. **Target & Scope**: `[BASEMENT]` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, Sonocotta Louder-ESP32 `ESP32-WROVER`). Subsystems: 2.4 GHz RF Front-End, PCB Trace Antenna, ESP32 3.3V Power Rail, and multi-AP `"iot"` BSSID selection (`src/main.rs` Wi-Fi config & live RF telemetry).
3. **Layer Mandate**: Audit bottom-up: L1 (3.3V power rail/brownout stability during TX bursts) -> L2 (802.11 BSSID link stability, `-48 dBm` RSSI, Ch 1 vs Ch 11 co-channel/roaming behavior across the 4 `"iot"` BSSIDs) -> L3 (`esp-radio` `ScanMethod::AllChannels`, `PowerSaveMode::None`, `beacon_timeout(30)`, `WIFI_MAX_FRAME_LEN = 1492` MTU) -> L4 (continuous uptime `> 60 min`, ICMP ping jitter/loss).
4. **Seeds**: `Embedded / MCU / IoT Hardware`, `Power, Thermal & Clocking Configurations`, `Network Interfaces, Switches & VoIP/Appliance Hardware`. Compliance: `NIST-800-53:PE-13/14`, `NIST-800-53:CM-6`, `HW-FAULT:PWR-THERMAL`, `HW-FAULT:LINK-CRC`.
5. **Pre-Identified Scout Anomalies**: 4 BSSIDs visible on `wlx98482766488b` for SSID `"iot"` (Ch 1 `02:EC:DA:B7:C8:B4` 70%, Ch 11 `BA:FB:E4:D7:70:10` 70%, Ch 11 `86:2A:A8:87:C5:B8` 50%, Ch 6 `02:EC:DA:BD:BB:44` 39%); check whether `ScanMethod::AllChannels` without `SignalStrength` sorting in `esp-radio` `ClientConfig` could still pick a weaker BSSID on reboot if channel 6/11 responds first or if `esp-radio` sorts by RSSI.
6. **Shell & Script Safety**: Read-only Linux commands (`ping -c 5 172.22.1.15`, `curl -sS http://172.22.1.15/status.json`, `nmcli dev wifi list ifname wlx98482766488b`) and read `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` around Wi-Fi initialization. **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry and source inspection. Do NOT modify any files or configs.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
