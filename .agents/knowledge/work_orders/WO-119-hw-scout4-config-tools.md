# Work Order `WO-119`: `/deep-hardware-audit` Phase 0a Scout 4 — Configuration Surface & Diagnostic Tool Prober (`[BASEMENT]`)

- **ID**: `WO-119`
- **Timestamp**: `2026-10-25T20:21:15-04:00`
- **Subagent Role**: Configuration Surface & Diagnostic Tool Prober (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are Phase 0a **Scout 4 (Configuration Surface & Diagnostic Tool Prober)** for `/deep-hardware-audit` of `[BASEMENT]` (`172.22.1.15`).
1. **Live `"BASEMENT"` (`172.22.1.15`) & LMS (`10.0.0.10`) Configuration Surfaces**:
   - Query `curl -sS -D - -o /dev/null -m 5 http://172.22.1.15/` and `curl -sS -m 5 http://172.22.1.15/status.json`.
   - Query LMS JSON-RPC (`http://10.0.0.10:9000/jsonrpc.js`) for player `c8:2e:18:be:b8:34` status (`["status", "-", 1, "tags:uB"]`) and preferences (`plugin.squeezedsp:enabled`, `volume`, `power`, `presets`).
   - Check UPnP (`curl -sS -m 5 "http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34" | head -n 25`) and mDNS AirPlay advertisement (`avahi-browse -rt _raop._tcp` or on `10.0.0.10`).
2. **Diagnostic CLI Tool Prober**:
   - Verify availability and versions of local diagnostic tools: `espflash`, `esptool.py`, `xtensa-esp32-elf-objdump`, `xtensa-esp32-elf-readelf`, `avahi-browse`, `nmcli`, `btmgmt`.
3. **Strict Constraints**:
   - Read-only queries only (<60s). **NEVER touch `/dev/ttyUSB0`**. Return pure YAML inventory.

## 2. Fulfillment Section
*(To be completed by subagent)*
