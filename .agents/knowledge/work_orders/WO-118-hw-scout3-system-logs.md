# Work Order `WO-118`: `/deep-hardware-audit` Phase 0a Scout 3 — System, Kernel & Hardware Log Scout (`[BASEMENT]`)

- **ID**: `WO-118`
- **Timestamp**: `2026-10-25T20:21:15-04:00`
- **Subagent Role**: System, Kernel & Hardware Log Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 3 (System, Kernel & Hardware Log Scout)** for `/deep-hardware-audit` of `[BASEMENT]` (`172.22.1.15`).
1. **Live `"BASEMENT"` (`172.22.1.15`) Telemetry Counter Delta Sampling**:
   - Sample `curl -sS -m 5 http://172.22.1.15/status.json` 3 times spaced 2 seconds apart (write a small helper script or sequential curl calls) and record the exact deltas for `uptime_ms`, `frames_played`, `core1_loops`, `i2c_acks`, `i2c_nacks`, `tas_pll_locked`, `tas_automuted`, `tas_fs_hz`, `tas_bck_ratio`, `rssi`.
2. **LMS Host (`10.0.0.10`) Log & Socket State Summary**:
   - Via read-only SSH (`ssh -o BatchMode=yes 10.0.0.10`), check:
     - `tail -n 60 /var/log/squeezeboxserver/server.log`
     - `journalctl -u logitechmediaserver -n 40 --no-pager`
     - `dmesg --level=emerg,alert,crit,err,warn | tail -n 30`
     - `ss -tnp | grep 172.22.1.15` (active SlimProto `3483` and HTTP stream `9000` TCP sockets to `"BASEMENT"`).
3. **Strict Constraints**:
   - Bounded read-only queries only (<60s). **NEVER touch `/dev/ttyUSB0`**. Return pure YAML summary.

## 2. Fulfillment Section
*(To be completed by subagent)*
