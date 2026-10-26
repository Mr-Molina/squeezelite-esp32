# Subagent Summary: WO-080 — Live Host Bluetooth & Wi-Fi Adapter Probing + Full Connectivity Verification

- **Timestamp**: `2026-10-25T10:47:00-04:00`
- **Role**: Extension Certifier (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified, Created, or Deleted (Absolute Paths)
1. **Modified in Local Repository**:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-080-host-bt-wifi-and-full-connectivity-verification.md`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`
2. **Backups Created in Local Repository**:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-080-host-bt-wifi-and-full-connectivity-verification.md.bak`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md.bak`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md.bak`

---

## 2. Rationale for Each Action
- **Physical Layer RF Inventory**: Identified attached host USB wireless devices (TP-Link Archer T2U PLUS RTL8821AU Wi-Fi dongle `wlx98482766488b` and UGREEN BT6.0 Adapter `hci0`) and activated `hci0` / `bluetoothctl power on` without disrupting host wired routing.
- **Over-the-Air Wi-Fi Scan**: Used host's Wi-Fi interface to scan 2.4/5GHz environment, confirming presence of SSID `"iot"` across multiple BSSIDs and verifying zero rogue Squeezelite softAPs.
- **Over-the-Air Bluetooth Scan**: Queried local RF environment for Bluetooth Classic BR/EDR and BLE devices. Observed Squeezebox C-firmware sibling devices (`KITCHEN`, `DININGROOM`, `LIVINGROOM`) actively broadcasting BT Classic names, while target `"BASEMENT"` (`c8:2e:18:be:b8:36`) reported `Host is down`, empirically confirming the architectural finding of `WO-069` (BT Classic uninitialized in `louder-esp32-app` `ota_0` to preserve internal DRAM and eliminate RF coexistence audio stalls).
- **L3/L7 & Multi-Protocol Verification**: Verified end-to-end ping, Web UI Port 80 `/` and `/cmd.json?act=vol55`, SlimProto streaming at 44,091.82 fps, Apple AirPlay RTSP `OPTIONS *` (HTTP 200), UPnP MediaRenderer XML (HTTP 200), and CastBridge daemon status.

---

## 3. Empirical Verification Evidence
1. **Wi-Fi RF**:
   - `nmcli dev wifi list ifname wlx98482766488b`: SSID `"iot"` detected on `02:EC:DA:B7:C8:B4` (Ch 1, 80% / -58 dBm), `BA:FB:E4:D7:70:10` (Ch 11, 67%), `02:EC:DA:BD:BB:44` (Ch 6, 50%).
2. **Bluetooth Classic**:
   - `btmgmt find`: Squeezebox siblings `KITCHEN` (`C8:2E:18:BE:B8:3A`, -65 dBm), `DININGROOM` (`C8:2E:18:BE:B8:1E`, -70 dBm), `LIVINGROOM` (`F8:B3:B7:64:6A:BE`, -73 dBm) discoverable.
   - `sdptool browse C8:2E:18:BE:B8:36`: `Failed to connect to SDP server: Host is down` (`louder-esp32-app` pure-Rust mode).
3. **L3/L7 Target Health**:
   - `ping -c 3 172.22.1.15`: 0% loss, avg 3.967 ms.
   - `GET http://172.22.1.15/`: `HTTP/1.1 200 OK` (2,959 bytes, 0 missing bytes).
   - `GET http://172.22.1.15/cmd.json?act=vol55`: `{"ok":true,"act":"vol55"}`; `/status.json` confirms `vol_pct: 55`, `i2c_acks: 349`, `i2c_nacks: 0`.
4. **Audio Streaming**:
   - LMS JSON-RPC: `mode: "play"`, *Ibiza Global Radio*, continuous streaming past 696s.
   - Frame rate: 220,928 frames in 5.011s = **44,091.82 fps** (99.98% rate).
5. **AirPlay & UPnP**:
   - `OPTIONS * RTSP/1.0` to `10.0.0.10:38143`: `RTSP/1.0 200 OK` with full AirPlay headers.
   - `GET http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`: `HTTP/1.1 200 OK` (3,105 bytes).

---

## 4. Known Risks or Untested Assumptions
- None. Hardware isolation strictly preserved: `/dev/ttyUSB0` was never touched.
- All background tasks are terminated (`manage_task(list)` is empty).
