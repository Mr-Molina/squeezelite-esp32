# Work Order `WO-116`: `/deep-hardware-audit` Phase 0a Scout 1 — Hardware, Bus & Storage Topology Scout (`[BASEMENT]`)

- **ID**: `WO-116`
- **Timestamp**: `2026-10-25T20:21:15-04:00`
- **Subagent Role**: Hardware, Bus & Storage Topology Scout (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 1 (Hardware, Bus & Storage Topology Scout)** for `/deep-hardware-audit` of `[BASEMENT]` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, Sonocotta Louder-ESP32 `ESP32-WROVER` + TI `TAS5805M` at `I2C0 0x2D`).
1. **Live Embedded Target `"BASEMENT"` (`172.22.1.15`)**:
   - Execute `ping -c 3 -W 1 172.22.1.15` and `curl -sS -m 5 http://172.22.1.15/status.json`.
   - Record all physical/bus/memory/clock fields reported by `status.json` (`rssi`, `psram_free_bytes`, `psram_used_bytes`, `core1_active`, `core1_loops`, `i2c_acks`, `i2c_nacks`, `tas_fs_hz`, `tas_bck_ratio`, `tas_pll_locked`, `tas_automuted`, `again_code`, `vol_pct`, `amp_mode`, `otadata_prearmed_factory`).
2. **Over-the-Air Wi-Fi RF & L2/L3 Topology**:
   - Check local host Wi-Fi interface (`nmcli -f BSSID,SSID,CHAN,RATE,SIGNAL,BARS,SECURITY dev wifi list ifname wlx98482766488b` or `ip -br link`) for `"iot"` BSSID signal levels.
   - Check LMS host `10.0.0.10` (`ssh -o BatchMode=yes 10.0.0.10 "uptime; free -h; df -h / /var/lib/squeezeboxserver; ip -br addr"`).
3. **Strict Constraints**:
   - Read-only telemetry only (<60s).
   - **NEVER touch `/dev/ttyUSB0`** (`single-target-troubleshooting.md` / Invariant 65).
   - Return pure structured YAML inventory.

## 2. Fulfillment Section
*(To be completed by subagent)*
