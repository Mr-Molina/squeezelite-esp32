# Work Order `WO-164`: `/deep-hardware-audit` Phase 0a Scout 3 — System, Kernel & Hardware Log Scout

- **ID**: `WO-164`
- **Timestamp**: `2026-10-27T17:03:00-04:00`
- **Subagent Role**: System, Kernel & Hardware Log Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
Execute `/deep-hardware-audit` Phase 0a Scout 3 (Bounded System, Kernel & Hardware Log Summary):
1. **`"BASEMENT"` (`172.22.1.15`) Telemetry Counter Deltas**:
   - Poll `GET http://172.22.1.15/status.json` twice separated by 3 seconds to capture counter deltas (`uptime_ms`, `frames_played`, `core1_loops`, `i2c_acks`, `i2c_nacks`, `bt_rx_pkts`, `bt_tx_pkts`, `tas_pll_locked`, `tas_automuted`, `rssi`).
2. **LMS Host (`10.0.0.10`) Bounded Logs**:
   - Run bounded log summary on `10.0.0.10`: `ssh -o BatchMode=yes -o ConnectTimeout=5 10.0.0.10 "tail -n 50 /var/log/squeezeboxserver/server.log && dmesg --level=emerg,alert,crit,err,warn | tail -n 30"`.
3. **Local Workstation Bounded Kernel/Bluetooth Logs**:
   - Run `journalctl -p 0..3 -b --no-pager | tail -n 40` (or `dmesg --level=emerg,alert,crit,err,warn | tail -n 40` if accessible) to check for any USB, Bluetooth (`hci0`), or network errors.
4. **Zero Phase Leakage Constraint**: Strictly bounded (`tail`) read-only log queries (<60s). Append your Fulfillment Summary (in YAML format) to §2 of this Work Order file.

## 2. Fulfillment Section
*(To be completed by subagent)*
