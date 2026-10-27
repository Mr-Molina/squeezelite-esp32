# Subagent Fulfillment Summary: `WO-153` (Rev 2)

- **Work Order ID**: `WO-153`
- **Role**: Extension Certifier — Live Bluetooth A2DP & Wi-Fi Coexistence Certification
- **Subagent Conversation ID**: `d7a23fc0-f199-4dd7-88f8-b021753f98d4`
- **Timestamp**: `2026-10-26T23:32:00-04:00`
- **Status**: `[BLOCKED / ERROR]`

---

## 1. Files Created, Modified, or Deleted

| File Path (Absolute) | Operation | Rationale |
| --- | --- | --- |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-153-live-bt-a2dp-ota-certification.md` | Modified | Updated Status to `[BLOCKED / ERROR]` and completed Fulfillment Section with live telemetry and opcode root cause analysis. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` | Modified | Updated central work order ledger status for `WO-153` to `[BLOCKED / ERROR]`. |
| `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261026_wo153_bt_a2dp_live_certifier.md` | Overwritten | Updated Rule 17 subagent summary and fulfillment record. |

---

## 2. Empirical Execution Telemetry

### Step 1: Pre-Flight Verification & Recovery Check (`[EXECUTED & VERIFIED]`)
- Verified staged OTA image `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`:
  - Size: `944,208` bytes
  - SHA-256: `b38e8987819dfde719910ce8bf02205f949ef8fec0336254e2fd5629883e1c0f`
- Confirmed recovery mode active on `172.22.1.15`:
  - `GET http://172.22.1.15/status.json` responded:
    `{"project_name":"Squeezelite-ESP32","platform_name":"ESP32","version":"I2S-4MFlash-1681","recovery":1,"Jack":false,"Voltage":0,"disconnect_count":0,"avg_conn_time":0,"bt_status":0,"bt_sub_status":0,"is_i2c_locked":false,"urc":0,"if":"wifi","ip":"172.22.1.15","netmask":"255.255.255.0","gw":"172.22.1.1","ssid":"iot","rssi":-56}`

### Step 2: Single OTA Flash Upload & Stable `ota_0` Boot (`[BLOCKED / ERROR]`)
- Dispatched single OTA upload via HTTP/1.0:
  - Command: `curl -s -S --http1.0 -X POST -H "Content-Type: application/octet-stream" --data-binary @/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin http://172.22.1.15/flash.json`
  - Result: `File uploaded. Flashing started.`
- Target flashed `ota_0` (`0x160000`) and rebooted at `22:27:16`.
- **Boot Stability & Anti-Brick Verification**:
  - The board booted `ota_0` without crashing or resetting!
  - `config_bt_funcs_reset()` successfully linked the Classic BT controller stack in `libbtdm_app.a`, eliminating the previous boot crash.
  - Polled status at `GET http://172.22.1.15/status.json` past the 45-second Gate 3 lease (`uptime_ms = 48854`, `uptime_ms = 99544`, `uptime_ms = 221518`):
    - `"recovery": 0` (PASS)
    - `"rust_engine": true` (PASS)
    - `"otadata_prearmed_factory": true` (PASS)
    - `"amp_mode": "Play"` (PASS)
    - `"tas_pll_locked": true` (PASS)
    - `"i2c_nacks": 0` (PASS)
    - Over 220s continuous uptime without dropouts or panics.
- **Defect Encountered**:
  - `"bt_rx_pkts": 8` (< 17 required).
  - `"bt_enabled": false` (expected `true`).
  - HCI initialization sequence stopped advancing after Step 8.

### Step 3: Over-the-Air Bluetooth Testing on `hci0` (`[BLOCKED / ERROR]`)
- Command: `sudo btmgmt --index 0 --timeout 5 find`
  - Discovered local BLE devices, but did not discover `C8:2E:18:BE:B8:36`.
- Command: `sudo /usr/bin/l2ping -c 2 C8:2E:18:BE:B8:36`
  - Output: `Can't connect: Host is down` (exit code 1).
- Root Cause: Because Step 8 did not complete, Step 13 (`HCI_Write_Scan_Enable = 0x03`) was never sent. The radio is not in discoverable/connectable scan mode.

### Step 4: LMS Sibling Integration (`[EXECUTED & VERIFIED]`)
- LMS `10.0.0.10:9000` `serverstatus`:
  - `BASEMENT` (`c8:2e:18:be:b8:34`) is connected to `10.0.0.10:3483` at `172.22.1.15:54352` (`"connected": 1`, `"name": "BASEMENT"`, `"modelname": "SqueezeESP32"`).
  - Wi-Fi and SlimProto connection are 100% stable and operational.

### Hardware Isolation
- `/dev/ttyUSB0` was strictly preserved and never opened or accessed.

---

## 3. Root Cause Analysis of HCI Initialization Stall

Code inspection of `rust/crates/louder-esp32-app/src/bt_a2dp.rs` identified the exact issue:
1. At line 57:
   ```rust
   pub const OPCODE_WRITE_PAGE_SCAN_ACTIVITY: u16 = 0x0C18;
   ```
2. In the Bluetooth Core Specification:
   - `HCI_Write_Page_Scan_Activity` has OGF `0x03`, OCF `0x001C` -> Opcode **`0x0C1C`**.
   - `0x0C18` is `HCI_Write_Page_Timeout` (which takes 2 octets).
3. At line 1105, `step_init` sends 4 octets (`[0x00, 0x08, 0x12, 0x00]`) to opcode `0x0C18`.
4. The BT controller returns an error/status rejecting the 4-byte parameter length on `Write_Page_Timeout`, preventing the state machine from advancing past Step 8 (`WritePageScanActivity`).
5. Changing line 57 to `pub const OPCODE_WRITE_PAGE_SCAN_ACTIVITY: u16 = 0x0C1C;` will allow all 17 init commands to complete, reaching `InitStep::Completed` and enabling `"bt_enabled": true`.
