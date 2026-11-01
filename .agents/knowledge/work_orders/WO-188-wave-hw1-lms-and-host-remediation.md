# Work Order `WO-188`: `/deep-hardware-audit` Phase 3 (`Wave HW-1`) — LMS `10.0.0.10` `TIER-1`/`TIER-2` Remediation & Workstation `obexd` Cleanup

- **ID**: `WO-188`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Infrastructure Administrator — LMS & Host Hardware Remediator` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are the **Wave HW-1 Hardware & Infrastructure Remediator** (`/deep-hardware-audit` Phase 3).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/hardware_audit_master_matrix.md`.

### Strict Operational Rules
- **NEVER** use `sed`, `awk`, or `cat` to modify files (Rule 23). For remote files on `10.0.0.10`, write a clean Python script `/tmp/remediate_lms_hw1.py` or copy files via `scp` and edit with `edit_file` / Python file I/O.
- **NEVER** use relative paths (Rule 24).
- **NEVER** touch `/dev/ttyUSB0` on `pc-1`.
- **Mandatory Pre-Flight Snapshot & Sibling Bridge Gate (`verify-autonomous-work.md`)**:
  1. Before stopping `lyrionmusicserver` on `10.0.0.10`, create `/var/backups/lms-pre-audit-20261027/` on `10.0.0.10` and back up:
     - `/var/lib/squeezeboxserver/prefs/server.prefs`
     - `/var/lib/squeezeboxserver/prefs/log.conf`
     - `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs`
     - Record pre-restart active sibling processes (`pgrep -a -f "shairport|squeeze2cast|avahi-publish|squeezebox"`), `ss -lntp`, and `df -h /` into `/var/backups/lms-pre-audit-20261027/pre_state.txt`.
  2. Stop `lyrionmusicserver.service` (`sudo systemctl stop lyrionmusicserver`).
  3. Remediate `HW-LMS-CPU-001` & `HW-LMS-NET-001` on `10.0.0.10` while `lyrionmusicserver` is stopped (using a Python script `/tmp/fix_lms_prefs.py` on `10.0.0.10`):
     - In `/var/lib/squeezeboxserver/prefs/log.conf`: change `log4perl.logger.player.source = DEBUG` to `log4perl.logger.player.source = ERROR`.
     - In `/var/lib/squeezeboxserver/prefs/server.prefs`:
       - Remove `flc-wav-*-*`, `flc-pcm-*-*`, `flc-aif-*-*`, `flc-mp3-*-*` from the global `disabledformats` list.
       - Under `_client:c8:2e:18:be:b8:34`: remove `flc-wav-*-c8:2e:18:be:b8:34`, `flc-aif-*-c8:2e:18:be:b8:34`, and `flc-mp3-*-c8:2e:18:be:b8:34` from `disabledformats`, and set `repeat: 0`.
  4. Remediate `HW-LMS-DISK-001` on `10.0.0.10`:
     - Truncate `/var/log/squeezeboxserver/server.log` to `0` bytes (`/usr/bin/sudo /usr/bin/truncate -s 0 /var/log/squeezeboxserver/server.log`).
     - Update `/etc/logrotate.d/squeezeboxserver` (or `/etc/logrotate.d/lyrionmusicserver`) to include `maxsize 50M` and `su squeezeboxserver nogroup` so `server.log` can never grow to `13.6 GB` again.
  5. Remediate `HW-LMS-MEM-001` on `10.0.0.10`:
     - Set `vm.swappiness=15` in `/etc/sysctl.d/99-lms-swappiness.conf` and apply via `/usr/sbin/sysctl -w vm.swappiness=15`.
  6. Start `lyrionmusicserver.service` (`/usr/bin/sudo /usr/bin/systemctl start lyrionmusicserver`), wait 15s, and verify:
     - `lyrionmusicserver` CPU usage `< 10%` (`ps -o pid,pcpu,pmem,stat,comm -C perl,lyrionmusicserver`).
     - `ss -lnt '( sport = :9000 or sport = :3483 )'` shows `Recv-Q == 0`.
     - `df -h /` shows `/var/log/squeezeboxserver/server.log` `< 5 MB` and root filesystem usage `< 30%`.
     - `curl -sS -m 5 http://10.0.0.10:9000/jsonrpc.js` responds in `< 100 ms`.
     - Sibling bridges (`avahi-publish-service` for `A1C6A30629A8@BASEMENT`, `squeeze2cast`, `UPnP/MediaRenderer.xml`, and `SqueezeDSP bypass: 1`) are all active!
  7. Remediate `HW-HOST-BT-001` on local workstation `pc-1`:
     - Check `pgrep -a obexd`. If orphaned `PID 12302` (`PPID 1`) is still running alongside `PID 1845`, terminate `12302` (`/bin/kill 12302`) and verify only the `systemd --user` managed `obexd` remains.
  8. Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw1.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section

- **Execution Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-27T18:45:00-04:00`
- **Subagent Summary Artifact**: [/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw1.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw1.md)

### Verification Summary
1. **Pre-Flight Snapshot**: Captured in `/var/backups/lms-pre-audit-20261027/` on `10.0.0.10` containing `server.prefs`, `log.conf`, `squeezedsp.prefs`, `clientplaylist_c82e18beb834.m3u`, `server.log.tail1000` (1,000 lines), and `pre_state.txt`.
2. **HW-LMS-CPU-001 & HW-LMS-NET-001**:
   - `log.conf`: `log4perl.logger.player.source = ERROR` applied (was `DEBUG`).
   - `server.prefs`: Disabled formats `flc-wav-*-c8:2e:18:be:b8:34`, `flc-mp3-*-c8:2e:18:be:b8:34`, `flc-pcm-*-*`, `flc-aif-*-*`, and `flc-aif-*-c8:2e:18:be:b8:34` removed. Under `_client:c8:2e:18:be:b8:34:`, `repeat: 0` set (purged typo `repear: '2'`).
   - Post-start LMS CPU: `0.0%` (STAT `S` idle; down from `96.4%` locked).
   - Port 9000 listen backlog: `Recv-Q: 0` (down from `4097`).
   - Port 3483 listen backlog: `Recv-Q: 0` (down from `3924`).
   - JSON-RPC latency: `8.3 ms` (`TIME_TOTAL: 0.008299`, HTTP 200).
3. **HW-LMS-DISK-001**:
   - `/var/log/squeezeboxserver/server.log` truncated from 15 GB to 0 bytes; current size is `466 KB` (`< 5 MB`).
   - Root filesystem usage dropped from `73%` (21 GB used) to `20%` (5.4 GB used, 23 GB free).
   - `/etc/logrotate.d/lyrionmusicserver` updated with `maxsize 50M` and `su squeezeboxserver nogroup`, syntax validated via `logrotate -d`.
4. **HW-LMS-MEM-001**:
   - `vm.swappiness = 15` configured in `/etc/sysctl.d/99-lms-swappiness.conf`, `/etc/sysctl.d/99-zram-swappiness.conf`, and updated in `99-sbc.conf`. Applied live via `/usr/sbin/sysctl -w vm.swappiness=15` (`cat /proc/sys/vm/swappiness == 15`).
5. **Sibling Bridges (Invariant 44)**:
   - `ShairTunes2W`: Active (`PID 13172` `avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 38677`).
   - `CastBridge`: Active (`PID 13063` `squeeze2cast-linux-aarch64-static`) connected to port 3483.
   - `UPnP MediaRenderer`: Active (`curl http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` returned HTTP 200 in `15.4 ms`).
   - `SqueezeDSP`: Verified `Client.bypass: '1'` and `bypass: '1'` in `squeezedsp.prefs`.
6. **HW-HOST-BT-001 & Workstation Isolation**:
   - Orphaned duplicate `obexd` `PID 12302` terminated; only `PID 1845` (`systemd --user`) remains.
   - `/dev/ttyUSB0` untouched (mtime 16:11, zero commands targeted to serial port).
