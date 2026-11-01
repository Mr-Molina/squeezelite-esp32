# Work Order `WO-165`: `/deep-hardware-audit` Phase 0a Scout 4 — Configuration Surface & Diagnostic Tool Prober

- **ID**: `WO-165`
- **Timestamp**: `2026-10-27T17:03:00-04:00`
- **Subagent Role**: Configuration Surface & Diagnostic Tool Prober (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
Execute `/deep-hardware-audit` Phase 0a Scout 4 (Configuration Surface & Diagnostic Tool Prober):
1. **LMS `10.0.0.10` & Player `c8:2e:18:be:b8:34` Configuration Surface**:
   - Query LMS `http://10.0.0.10:9000/jsonrpc.js` for player `c8:2e:18:be:b8:34` status (`mode`, `mixer volume`, current playlist track, `can_seek`), UPnP `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` HTTP status, and active helper processes on `10.0.0.10` (`pgrep -a -f "shairport_helper|avahi-publish|squeeze2cast"`).
2. **Host `udev` Rules & Serial Port Safety Audit (`single-target-troubleshooting.md`)**:
   - Inspect `/etc/udev/rules.d/` to verify zero active `RUN+=` reset hooks exist on serial devices.
3. **Diagnostic CLI Tool Availability Probe**:
   - Verify exact absolute paths (`command -v`) on local host for: `python3`, `cargo`, `espflash`, `esptool.py`, `xtensa-esp32-elf-objdump`, `xtensa-esp32-elf-readelf`, `btmgmt`, `l2ping`, `sdptool`, `ffmpeg`, `flac`, `curl`, `ssh`.
4. **Zero Phase Leakage Constraint**: Read-only queries only (<60s). Append your Fulfillment Summary (in YAML format) to §2 of this Work Order file.

## 2. Fulfillment Section
*(To be completed by subagent)*
