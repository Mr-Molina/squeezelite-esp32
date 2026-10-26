# Work Order `WO-074`: `/deep-hardware-audit` Phase 0a Scout 3 — System, Kernel & Hardware Log Scout

- **ID**: `WO-074`
- **Timestamp**: `2026-10-25T09:40:00-04:00`
- **Subagent Role**: System, Kernel & Hardware Log Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 3 (System, Kernel & Hardware Log Scout)** for `/deep-hardware-audit`.
Query fast bounded summaries (`tail -n 80`) of hardware/stream/bridge logs across `172.22.1.15` and `10.0.0.10`:
1. **Target `172.22.1.15` Live Hardware Telemetry & Error Counters**:
   - Sample `curl -s http://172.22.1.15/status.json` twice 3 seconds apart and record `frames_played` delta, `i2c_acks` delta, `i2c_nacks`, `rssi`, `uptime_ms`, `amp_mode`, `otadata_prearmed_factory`.
2. **LMS / Bridge Server `10.0.0.10` Logs**:
   - Via read-only SSH to `10.0.0.10`, inspect bounded tails (`tail -n 60`) of:
     - `/var/log/squeezeboxserver/server.log`
     - `/var/log/squeezeboxserver/castbridge.log` (if present)
     - `journalctl -u logitechmediaserver -u avahi-daemon -n 50 --no-pager`
     - `dmesg --level=emerg,alert,crit,err,warn | tail -n 40`
3. **Strict Constraints**:
   - Read-only bounded queries only (<60s).
   - **NEVER touch `/dev/ttyUSB0`** (Rule 3).
   - Return pure structured YAML summary of log events and counters.

## 2. Fulfillment Section
*(To be completed)*
