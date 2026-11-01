# Work Order: WO-256 — Phase 3 Wave 1A LMS Cluster: Remediate `CFG-001` (`SqueezeDSP` Native MP3 Passthrough on `10.0.0.10`)

- **Work Order ID**: `WO-256`
- **Timestamp**: `2026-11-01T09:14:00-05:00`
- **Assigned Subagent Role**: `Extension Certifier / LMS Bridge Specialist` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**:
  - Remote Lyrion Music Server (`10.0.0.10`):
    - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm`
    - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Remediate finding **`CFG-001` (`MEDIUM`, `TIER-2-SUBSYSTEM`)** on LMS host `10.0.0.10` so that when `SqueezeDSP` is in bypass/disabled mode (`Client.bypass: 1` / `enabled: 0`) or for native MP3 streams, `removeNativeConversion()` in `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm:258` preserves native `mp3` passthrough (`mp3 mp3 * -`) rather than forcing `lame --mp3input --decode | SqueezeDSP | lame --preset insane` for `"BASEMENT"` (`c8:2e:18:be:b8:34`).

## 2. Step-by-Step Instructions
1. **Pre-Modification Verification & Backups (`Rule 31`, `verify-autonomous-work.md`)**:
   - Over SSH (`console-admin@10.0.0.10`), record pre-restart active processes and ports (`pgrep -fa 'squeeze2cast|shairport|squeezeboxserver'` and `ss -tlnp | grep -E '9000|9090|3483'`).
   - Create verified backups on `10.0.0.10`:
     - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm.bak_wo256`
     - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf.bak_wo256`
   - Verify current syntax of `Configuration.pm` via `/usr/bin/perl -c /var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm`.
2. **Apply Single-Variable Fix (`Rule 5`, `Rule 23`)**:
   - In `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` around line 258 (`removeNativeConversion`), update the native conversion preservation regex from `^(aac|ogg|alc|ops)$` to `^(aac|ogg|alc|ops|mp3|flc)$` (using a Python script on `10.0.0.10` or SCP — never `sed`/`awk`/`cat`), and verify syntax via `/usr/bin/perl -c`.
   - Also check `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf` for `c8:2e:18:be:b8:34` (`"BASEMENT"`): since `"BASEMENT"` has `enabled: '0'` and `bypass: '1'` in `squeezedsp.prefs`, ensure `mp3 mp3 * c8:2e:18:be:b8:34` does not override `mp3 mp3 * -` when bypassed (or comment/remove the `c8:2e:18:be:b8:34` section in `custom-convert.conf` if `Configuration.pm` only regenerates `custom-convert.conf` on plugin setting changes).
3. **Restart LMS & Independently Verify Sibling Bridges + Native Streaming (`verify-autonomous-work.md`)**:
   - Restart `logitechmediaserver.service` on `10.0.0.10` (`sudo systemctl restart logitechmediaserver.service`).
   - Wait for port `9000`/`3483` readiness and verify:
     1. `CastBridge` (`squeeze2cast-linux-aarch64-static`) is active (`pgrep -fa squeeze2cast`).
     2. `UPnP MediaRenderer` (`http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`) returns `HTTP/1.1 200 OK`.
     3. Trigger/resume *Ibiza Global Radio* (`preset1` or `playlist play`) on `"BASEMENT"` (`c8:2e:18:be:b8:34`) and verify via `ps -ef | grep -E 'lame|SqueezeDSP'` on `10.0.0.10` that **ZERO** `lame | SqueezeDSP | lame` processes are spawned for `c8:2e:18:be:b8:34`, while `GET http://172.22.1.15/status.json` confirms `frames_played` is advancing at `~44,100` fps with `i2c_nacks: 0`!
4. **Handoff**:
   - Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo256_lms_cfg001.md` and update the Fulfillment Section of this Work Order before returning.
   - Ensure `manage_task(Action: "list")` returns zero active background tasks (`Rule 21`).

## 3. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T09:31:45-05:00`
- **Subagent Summary Reference**: [`20261101_wo256_lms_cfg001.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo256_lms_cfg001.md)
- **Live Empirical Evidence**:
  1. **Pre-Restart Audit**: Verified `squeezeboxserver` (PID 36357) and `CastBridge` (PID 36381) running with listeners on ports 9000, 9090, 3483. SqueezeDSP transcode pipeline was active for `c8:2e:18:be:b8:34` (PIDs 42664, 42665, 42666, 42674: `lame --mp3input --decode | SqueezeDSP | lame --preset insane`). Backups created and verified:
     - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm.bak_wo256`
     - `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf.bak_wo256`
  2. **Code & Config Remediation**:
     - Updated `Configuration.pm` line 194 preservation regex from `^(aac|ogg|alc|ops)$` to `^(aac|ogg|alc|ops|mp3|flc)$` and verified syntax via Perl (`SYNTAX_OK`).
     - Commented out `c8:2e:18:be:b8:34` overrides for `flc flc`, `mp3 flc`, `flc mp3`, and `mp3 mp3` in `custom-convert.conf`.
  3. **Post-Restart Sibling Bridges Verified**:
     - `sudo systemctl restart logitechmediaserver.service` executed; service returned `active`.
     - `CastBridge` (`squeeze2cast-linux-aarch64-static`) verified running (PID 44588).
     - Ports 9000, 9090, 3483 confirmed listening (`ss -tlnp`).
     - `UPnP MediaRenderer` returned `HTTP/1.1 200 OK` (3,105 bytes).
  4. **Native Streaming & Zero Transcode Verified**:
     - Resumed *Ibiza Global Radio* MP3 stream on `"BASEMENT"` (`c8:2e:18:be:b8:34`).
     - `ps -ef | grep -E 'lame|SqueezeDSP' | grep -v grep` returned zero processes (exit code 1). ZERO transcoding processes spawned.
     - LMS `server.log` confirmed direct upstream streaming: `stream_s called: format: mp3 url: http://cdn-peer022.streaming-pro.com:8024/ibizaglobalradio.mp3` with request string `[GET /ibizaglobalradio.mp3 HTTP/1.0`.
     - `GET http://172.22.1.15/status.json` confirmed `codec: "MP3"`, `tas_automuted: false`, `tas_pll_locked: true`, `i2c_nacks: 0`, and `frames_played` monotonically advancing past 3.7M frames.
  5. **Background Tasks Check**: `manage_task(Action: "list")` confirmed 0 active background tasks.
