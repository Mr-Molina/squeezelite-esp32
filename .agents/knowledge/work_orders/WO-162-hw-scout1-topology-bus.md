# Work Order `WO-162`: `/deep-hardware-audit` Phase 0a Scout 1 — Hardware, Bus & Storage Topology Scout

- **ID**: `WO-162`
- **Timestamp**: `2026-10-27T17:03:00-04:00`
- **Subagent Role**: Hardware, Bus & Storage Topology Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
Execute `/deep-hardware-audit` Phase 0a Scout 1 (Hardware, Bus & Storage Topology) across the active project hardware targets:
1. **Live Embedded Target `"BASEMENT"` (`172.22.1.15`)**:
   - Query `curl -sS -m 5 http://172.22.1.15/status.json` to capture live SoC, PSRAM, Core 1, I2C0 (`0x2D` TAS5805M), I2S0 clock/PLL, Bluetooth (`bt_enabled`, `bt_rx_pkts`), Wi-Fi RSSI, and partition state.
2. **Local Linux Host & Passive Peripherals**:
   - Run read-only topology checks: `lscpu`, `free -h`, `lsusb`, `ip -br link`, `ls -l /dev/serial/by-id/` (PASSIVE ONLY — per `single-target-troubleshooting.md`, NEVER run `esptool` or `espflash` or open `/dev/ttyUSB0`!).
   - Check local Bluetooth adapter `hci0` status via `hciconfig -a` or `btmgmt info`.
3. **Lyrion Music Server Host (`10.0.0.10`)**:
   - Query `ssh -o BatchMode=yes -o ConnectTimeout=5 10.0.0.10 "uname -a && free -h && df -h /"` and LMS JSON-RPC `serverstatus` on `http://10.0.0.10:9000/jsonrpc.js`.
4. **Zero Phase Leakage Constraint**: Fast read-only inventory only (<60s). Append your Fulfillment Summary (in YAML format) to §2 of this Work Order file.

## 2. Fulfillment Section
*(To be completed by subagent)*
