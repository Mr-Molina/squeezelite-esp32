# Work Order `WO-168`: `/deep-hardware-audit` Phase 1 Auditor 1 (`HW-AUD-01`) — LMS `10.0.0.10` Daemon CPU Spin, Listen Backlog (`4097/4096`), `TranscodingHelper` Log Flood & `disabledformats` Micro-Auditor

- **ID**: `WO-168`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: LMS `10.0.0.10` Daemon CPU Spin, Listen Backlog, `TranscodingHelper` & `disabledformats` Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `LMS 10.0.0.10 Daemon CPU Spin, Listen Backlog (4097/4096), TranscodingHelper Log Flood & disabledformats Micro-Auditor` (`HW-AUD-01`)
2. **Target & Scope**: Remote LMS Appliance `10.0.0.10` (`lyrionmusicserver` PID 9717, TCP Port 9000/3483/9090 socket backlog, `/var/log/squeezeboxserver/server.log`, `/var/lib/squeezeboxserver/prefs/server.prefs`, `/var/lib/squeezeboxserver/prefs/log.conf`).
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: Raspberry Pi aarch64 CPU load/thermal state under 96.4% CPU spin and microSD/disk I/O from log flood.
   - `L2-Bus-Interconnect`: TCP port 9000/3483 socket queues (`ss -lntp`, `ss -ntp | grep -E '9000|3483'`).
   - `L3-Driver-Firmware-Config`: `/var/lib/squeezeboxserver/prefs/server.prefs` (`disabledformats`, player `c8:2e:18:be:b8:34` prefs, `08:d1:f9:e5:06:90` player entry) and `/var/lib/squeezeboxserver/prefs/log.conf` (`player.source` / `TranscodingHelper` debug log level).
   - `L4-Service-Logs`: Exact `/var/log/squeezeboxserver/server.log` lines showing why `PID 9717` is spinning in `Slim::Player::TranscodingHelper::enabledFormat` and saturating Port 9000 (`4097/4096`).
4. **Seeds**: System, Kernel & Hardware Event Logs; Network Interfaces, Switches & Appliance Hardware (`NIST-800-53:SI-2`, `NIST-800-53:AU-6`, `CIS-LOG-4.1`, `HW-FAULT:PWR-THERMAL`).
5. **Pre-Identified Scout Anomalies**:
   - `PID 9717` (`lyrionmusicserver`) is pegged at `96.4%` CPU in `Rs` state, saturating Port `9000` listen backlog (`4097/4096`) and causing JSON-RPC (`http://10.0.0.10:9000/jsonrpc.js`) and UPnP `MediaRenderer.xml` requests to time out.
   - `disabledformats` in `/var/lib/squeezeboxserver/prefs/server.prefs` has `flc-wav-*-c8:2e:18:be:b8:34`, `flc-mp3-*-c8:2e:18:be:b8:34`, `flc-aif-*-c8:2e:18:be:b8:34` disabled while `c8:2e:18:be:b8:34` has `file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac` queued at `mixer_volume: 5`.
6. **Shell & Script Safety**: Use `/usr/bin/ssh -o BatchMode=yes -o ConnectTimeout=5 10.0.0.10 "..."` with bounded read-only commands (`tail`, `grep`, `ss`, `ps`).
7. **Constraint**: **Read-only telemetry inspection. Do NOT modify any configs, drivers, firmware, or services.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md` (`ID`, `Severity`, `Safety Tier`, `Compliance`, `Target & Component`, `Layer`, `Config / Log Source`, `Title`, `Description`, `Live Evidence`, `Remediation`, `Rollback Command`, `Verification Read-Back`, `Blocked By / Related`).

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `dd748961-c6c5-4e07-b086-aabdb2f7321a`
- **Summary**: Proved `PID 9717` (`lyrionmusicserver`) is spinning at `96.4%` CPU because `c8:2e:18:be:b8:34` has `test_stereo_16bit.flac` queued with `playlist repeat: 2` while all `flc-*` transcode targets are disabled in `server.prefs` and `log.conf` has `player.source = DEBUG` (`13.6 GB` `server.log`), saturating Port `9000` (`4097/4096`) and Port `3483` (`3988/4096`) listen backlogs (`CFG-001`, `NET-001`, `STORAGE-001`).
