# Work Order: WO-250 — Phase 1 `CODE-AUD-6`: Phase 4 Bluetooth (`bt_a2dp`) & Phase 7 Peripherals (`display_gds`, `controls`, `dac_platform`, `power_boards`, `amp`, `tas58xx`, `cyd`) Micro-Auditor

- **Work Order ID**: `WO-250`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `Phase 4 Bluetooth & Phase 7 Peripherals Micro-Auditor` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/bt_a2dp.rs` (`5,684` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs` (`939` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/controls.rs` (`1,034` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs` (`1,128` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/dac_platform.rs` (`656` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/power_boards.rs` (`440` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs` (`267` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (`1,411` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/src/lib.rs` (`821` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/{lib,main,comms,drivers,ui}.rs` (`3,622` LOC)
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Micro-Auditor Contract
1. **Role**: `Phase 4 Bluetooth & Phase 7 Peripherals Micro-Auditor` (`CODE-AUD-6`)
2. **Scope**: Strictly the files listed above.
3. **Focus Areas & Pre-Identified Hotspots**:
   - Audit Phase 4 (`bt_a2dp.rs`, `WO-230`):
     - HCI/L2CAP/SDP/AVDTP/AVCTP packet bounds, AVRCP 1.6 Passthrough (`0x7C`), `GetElementAttributes` (`0x20`), `SetAbsoluteVolume` (`0x50` `0x00..=0x7F` <-> `0..=100%`), `RegisterNotification` (`0x31`), 4-slot `BondingStore` LRU & hex serialization, `SbcEncoder` 8-subband polyphase analysis filterbank & `BtA2dpSourceState`.
   - Audit Phase 7 (`WO-231`):
     - `display_gds.rs`: `SSD1306`, `SH1106`, `ST7789` init sequences, dirty-page framebuffer bounds, `SpectrumVuMeter` integer math, scrolling text offset bounds.
     - `controls.rs`: `ButtonDebouncer`, `QuadratureEncoder` Gray-code transitions, `NecIrDecoder` pulse width tolerance, `AdcButtonLadder`, `BatteryMonitor`.
     - `dac_platform.rs` & `power_boards.rs`: `ExternalDacPlatform` (`ES8388`, `WM8978`, `AC101`, `TAS57xx`/`TAS5713`, `PCM5102A`, `S/PDIF` BMC encoder), `LedVuStrip`, `AmplifierPowerController`.
     - `amp.rs` & `squeezelite-tas58xx/src/lib.rs`: TAS5805M fault clear & telemetry polling frequency (`ANOM-01-CODE`).
     - `squeezelite-cyd-link` & `cyd-controller-app`: UART framing and UI state handling.
4. **Constraint**: **Read-only code inspection. Do NOT modify any files.**
5. **Output Schema**: Use the standardized 11-field code finding schema (`/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`) with OWASP, CWE, SOC 2, and `GRS-*` compliance tags.

## 2. Deliverables
Return your complete structured findings in your final message.

## 3. Fulfillment Section
- **Status**: `[AUDIT COMPLETE]`
- **Subagent ID**: `a0d779ad-062f-49f6-a3f6-08e0275e75cf`
- **Summary**:
  - Audited Phase 4 Bluetooth and Phase 7 Peripherals (`16,002` LOC across 14 files) and reported 7 findings (`2 HIGH`, `3 MEDIUM`, `2 LOW`): `LOGIC-602` (`bt_a2dp.rs:3277-3288` missing `HCI_User_Confirmation_Request_Negative_Reply` `0x042D` in `BondedOnly` mode), `LOGIC-603` (`squeezelite-tas58xx/src/lib.rs:56-61, 708-723` `FAULT_CLEAR` `0x80 -> 0x81` digital fault bit & retry throttling), `PERF-601` (`squeezelite-tas58xx/src/lib.rs:814-839` burst read `0x37..=0x39` & combined `check_faults_and_telemetry` reducing I2C polling from 11 to 3 ACKs per 2s), `LOGIC-604` (`bt_a2dp.rs:4395-4405` AVRCP unsupported vendor PDU `AVRCP_RSP_NOT_IMPLEMENTED`), `QA-601` (`controls.rs` missing `AdcButtonLadder`), `LOGIC-605` (`display_gds.rs:870-874` `TextScroller::visible_slice` UTF-8 char boundary alignment), and `PERF-602` (`cyd-controller-app/src/ui.rs:491-498` `DIRTY_TRACK` condition).
