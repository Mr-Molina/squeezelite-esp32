# Work Order `WO-195`: `/deep-hardware-audit` Phase 3 — Independent Hardware State Verifier (`Wave HW-1` Counter-Verification)

- **ID**: `WO-195`
- **Timestamp**: `2026-10-27T18:47:00-04:00`
- **Subagent Role**: `Independent Hardware State Verifier` (`Model: "pro"`, `TypeName: "research"`, `08128862`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are the **Independent Hardware State Verifier** (`/deep-hardware-audit` Phase 3 Counter-Verification Gate).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/hardware_audit_master_matrix.md` and `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_hw1.md`.

### Verification Mandate (Read-Only Counter-Verification)
1. **Verify `HW-LMS-CPU-001` & `HW-LMS-NET-001` on `10.0.0.10` (`ssh 10.0.0.10`)**:
   - Confirm `ps -o pid,pcpu,pmem,stat,comm -C perl,lyrionmusicserver` shows CPU `< 10%` and state `S`.
   - Confirm `ss -lnt '( sport = :9000 or sport = :3483 )'` shows `Recv-Q == 0` on both ports.
   - Confirm `/var/lib/squeezeboxserver/prefs/log.conf` has `log4perl.logger.player.source = ERROR` and `/var/lib/squeezeboxserver/prefs/server.prefs` has `flc-wav`/`flc-pcm` enabled and `repeat: 0` for `_client:c8:2e:18:be:b8:34`.
   - Confirm `curl -sS -m 5 -X POST http://10.0.0.10:9000/jsonrpc.js -d '{"id":1,"method":"slim.request","params":["c8:2e:18:be:b8:34",["status","-",1,"tags:uB"]]}'` returns HTTP 200 JSON in `< 100 ms`.
2. **Verify `HW-LMS-DISK-001` & `HW-LMS-MEM-001` on `10.0.0.10`**:
   - Confirm `/var/log/squeezeboxserver/server.log` size is `< 5 MB` and `df -h /` shows `< 30%` usage.
   - Confirm `/proc/sys/vm/swappiness` is `15` and `/var/backups/lms-pre-audit-20261027/` exists with non-empty backup files.
3. **Verify Sibling Bridges (`verify-autonomous-work.md`)**:
   - Confirm `avahi-publish-service` (`A1C6A30629A8@BASEMENT _raop._tcp`), `squeeze2cast`, `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`, and `SqueezeDSP bypass: '1'` are all active and healthy.
4. **Verify `HW-HOST-BT-001` on `pc-1`**:
   - Confirm `pgrep -a obexd` shows only a single `obexd` instance (`PID 1845`). Never touch `/dev/ttyUSB0`.
5. **Output**:
   - Return a structured YAML `verifications:` list with `status: "VERIFIED" | "FAILED"` and live terminal evidence for each finding.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary**:
  1. `HW-LMS-CPU-001` & `HW-LMS-NET-001`: Independently verified on `10.0.0.10` (`PID 13040` `0.0%` CPU, state `Ss`, `Recv-Q == 0 / 4096` on ports `9000` and `3483`, `log.conf` `player.source = ERROR`, `flc-wav`/`flc-pcm` enabled, `repeat: 0`, `/jsonrpc.js` HTTP 200 in `11.6 ms`).
  2. `HW-LMS-DISK-001` & `HW-LMS-MEM-001`: Independently verified `server.log` `3.3 MB` (`< 5 MB`), `df -h /` `20%` (`23 GB` free), `swappiness == 15`, and `/var/backups/lms-pre-audit-20261027/` non-empty.
  3. Sibling Bridges: Independently verified `ShairTunes2W` (`PID 13172`), `CastBridge` (`PID 13063`), `UPnP MediaRenderer.xml` (`HTTP 200 OK`, `3,105` B), and `SqueezeDSP bypass: 1`.
  4. `HW-HOST-BT-001`: Independently verified single `obexd` (`PID 1845`) on `pc-1` without touching `/dev/ttyUSB0`.
