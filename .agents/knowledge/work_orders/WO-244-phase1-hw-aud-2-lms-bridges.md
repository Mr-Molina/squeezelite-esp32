# Work Order: WO-244 — Phase 1 `HW-AUD-2`: LMS (`10.0.0.10`) `SqueezeDSP` Transcode Pipeline, Socket Backlog, & Multi-Protocol Bridge Micro-Auditor

- **Work Order ID**: `WO-244`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `LMS SqueezeDSP Transcode Pipeline, Socket Backlog, & Multi-Protocol Bridge Micro-Auditor` (`Model: "pro"`, `TypeName: "self"`, read-only)
- **Target Host / Component**: Lyrion Music Server (`10.0.0.10:9000` / `3483` / `9090`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & 8-Section Hardware Micro-Auditor Contract
1. **Role**: `LMS SqueezeDSP Transcode Pipeline, Socket Backlog, & Multi-Protocol Bridge Micro-Auditor` (`HW-AUD-2`)
2. **Target & Scope**: LMS `10.0.0.10` (`squeezeboxserver`, `SqueezeDSP`, `CastBridge`, `ShairTunes2W`, `UPnP MediaRenderer.xml`, `/var/lib/squeezeboxserver/prefs/`, `/var/log/squeezeboxserver/server.log`).
3. **Layer Mandate**: Inspect bottom-up from Layer 1 (Host CPU/Memory/Swap/Disk on `10.0.0.10`) -> Layer 2 (TCP socket listeners `9000`/`9090`/`3483` `Recv-Q`/`Send-Q`) -> Layer 3 (`SqueezeDSP` `custom-convert.conf` & client prefs for `c8:2e:18:be:b8:34`) -> Layer 4 (`server.log` & active transcode child processes).
4. **Pre-Identified Scout Anomalies**:
   - `ANOM-03`: In `WO-239`, LMS `10.0.0.10` had active processes `lame --mp3input --decode | SqueezeDSP --Clientid=c8:2e:18:be:b8:34 --bitsout=24 --formatout=MP3 | lame --preset insane` for `http://cdn-peer022.streaming-pro.com:8024/ibizaglobalradio.mp3`. Inspect `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf`, `Plugin.pm`, `Configuration.pm`, and `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs` via read-only SSH on `10.0.0.10` to determine why `SqueezeDSP` is in the MP3 pipeline, whether `Bypass` is active inside `SqueezeDSP`, and whether `ShairTunes2W` (`shairport_helper`) is currently running or needs attention.
   - `ANOM-05`: Verify `ss -tlnp` on `10.0.0.10`: in `ss -tlnp`, for `LISTEN` sockets, `Send-Q` (`4096`) is the configured maximum listen backlog (`somaxconn`) while `Recv-Q` (`0`) is the current number of unaccepted connections. Verify `Recv-Q == 0` and check if any `shairport_helper` processes are running for `BASEMENT`.
5. **Constraint**: **Read-only telemetry inspection on `10.0.0.10`. Do NOT modify any configs, restart services, or mutate player state.**
6. **Output Schema**: Report any actual findings using the 14-field hardware finding schema (`/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`), and explicitly stamp healthy subsystems as `VERIFIED_HEALTHY`.

## 2. Deliverables
Append your complete report to §3 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-244-phase1-hw-aud-2-lms-bridges.md`, verify `manage_task(list)` is empty, and return your structured findings in your final message.

---

## 3. Fulfillment Section
- **Audit Timestamp**: `2026-11-01T08:51:00-05:00`
- **Subagent Role**: `LMS SqueezeDSP Transcode Pipeline, Socket Backlog, & Multi-Protocol Bridge Micro-Auditor` (`HW-AUD-2`)
- **Target Host**: `10.0.0.10` (`lyrion`, Linux 6.12.33-v8+ aarch64, LMS 9.1.1 - 1779815526)
- **Overall Status**: `[EXECUTED & VERIFIED]` (Read-only telemetry audit complete; 1 defect identified, 5 subsystems verified healthy)

### 3.1 Subsystem Health & Verification Matrix

| Subsystem / Layer | Component / Target | Ground Truth Evidence | Verdict |
|---|---|---|---|
| **Layer 1: Host Compute** | `lyrion` (Raspberry Pi aarch64) | Uptime 6d 11h, load `0.09, 0.18, 0.24`, CPU 94% idle. RAM 409 MB free, Swap 0 si/so. Disk `/dev/root` 23 GB free (21% used). | `VERIFIED_HEALTHY` |
| **Layer 2: Socket Listeners** | Ports `9000`, `9090`, `3483` (`ANOM-05`) | `ss -tlnp` shows `Recv-Q: 0`, `Send-Q: 4096`. `/proc/sys/net/core/somaxconn` = `4096`. Accept queue is empty (0 pending unaccepted connections). | `VERIFIED_HEALTHY` |
| **Layer 3: AirPlay Bridge** | `ShairTunes2W` / `shairport_helper` | Helper intentionally skipped for `BASEMENT` (`c8:2e:18:be:b8:34`) via `Plugin.pm:311,351` (`model =~ /squeezeesp32/` && `squeezelite == 0`). Prevents mDNS collision with native ESP32 `squeezelite-raop`. | `VERIFIED_HEALTHY` |
| **Layer 3: Google Cast Bridge** | `CastBridge` (`squeeze2cast`) | PID 36381 active for `3-12:22:27` (0.6% CPU, 22 MB RSS). Stably managing `cc:cc:a0:42:cb:65` (`Basement TV (Cast)`) and `cc:cc:8e:1d:37:85`. | `VERIFIED_HEALTHY` |
| **Layer 3: UPnP MediaRenderer** | `Slim::Plugin::UPnP::MediaRenderer` | `noupnp: '0'` in `server.prefs`. `GET http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` returns `HTTP/1.1 200 OK` (3,105 B). | `VERIFIED_HEALTHY` |
| **Layer 4: Transcode Pipeline** | `SqueezeDSP` Transcode Pipeline (`ANOM-03`) | Active dual-lame transcode loop (`lame --decode | SqueezeDSP | lame --preset insane`) running for MP3 streams despite SqueezeDSP internal `bypass: '1'`. | `DEFECTIVE` (`CFG-001`) |

---

### 3.2 14-Field Defect Report: `CFG-001`

| Field | Value |
|---|---|
| **ID** | `CFG-001` |
| **Severity** | `MEDIUM` |
| **Safety Tier** | `TIER-2-SUBSYSTEM` |
| **Compliance** | `[NIST-800-53:CM-6, CIS-APP-1.2, HW-FAULT:CFG-TRANSCODE-LOOP]` |
| **Target & Component** | `lyrion / 10.0.0.10:9000` (LMS `squeezeboxserver`, Player `BASEMENT` / `c8:2e:18:be:b8:34`) |
| **Layer** | `L3-Driver-Firmware-Config` / `L4-Service-Logs` |
| **Config / Log Source** | `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` & `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf` |
| **Title** | Redundant MP3 Decode/DSP/Re-Encode Pipeline Active on Player `c8:2e:18:be:b8:34` Despite DSP Bypass |
| **Description** | Hypothesizing that SqueezeDSP's `Configuration.pm::removeNativeConversion()` deletes native direct-streaming entries (`command eq "-"`) from LMS's `Slim::Player::TranscodingHelper::commandTable` for MP3 streams because `mp3` is omitted from the format preservation whitelist (`$inputtype =~ /^(aac|ogg|alc|ops)$/`). When `c8:2e:18:be:b8:34` connects and plays an MP3 stream (`ibizaglobalradio.mp3`), SqueezeDSP's generated rule in `custom-convert.conf` (`mp3 mp3 * c8:2e:18:be:b8:34: [lame] --mp3input --decode | [SqueezeDSP] --formatout=MP3 | [lame] --preset insane`) forces the server to decode MP3 to PCM via `lame`, pass PCM through `SqueezeDSP` (which operates as a no-op due to `bypass: '1'`), and re-encode PCM back to 320kbps MP3 via `lame --preset insane`. This consumes host CPU cycles on `10.0.0.10` and subjects native MP3 audio to generational loss without applying any DSP filtering. |
| **Live Evidence** | 1. `ps -eo pid,user,args | grep -E 'SqueezeDSP|lame'`:<br>`42664 sh -c "/bin/lame" --mp3input --decode --silent - - \| "/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Bin/SqueezeDSP" --trackURL="http://cdn-peer022.streaming-pro.com:8024/ibizaglobalradio.mp3" --Clientid="c8:2e:18:be:b8:34" --bitsout=24 --formatout=MP3 \| /bin/lame -r --signed --little-endian -s 44.100 --bitwidth 32 -m s --preset insane - -`<br>2. `/var/log/squeezeboxserver/squeezedsp.log`:<br>`2026-11-01 07:43:34.617799 c8:2e:18:be:b8:34 LyrionDSPProcessAudio:ByPassProcess: Bypass mode enabled - minimal processing`<br>3. `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs`:<br>`Client.bypass: '1'`, `bypass: '1'`, `enabled: '0'`<br>4. `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` (lines 257-268):<br>`if ($inputtype =~ /^(aac|ogg|alc|ops)$/ && ($clientid eq 'c8:2e:18:be:b8:34' \|\| $clientid eq '*')) { next; }` (deletes native MP3 command). |
| **Remediation** | In Phase 3 remediation wave (`TIER-2-SUBSYSTEM`):<br>1. Update `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` line 258 to include `mp3`: `if ($inputtype =~ /^(aac|ogg|alc|ops|mp3)$/ ...)` for `c8:2e:18:be:b8:34`.<br>2. In `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf`, comment out the `mp3 mp3 * c8:2e:18:be:b8:34` block.<br>3. Restart `logitechmediaserver` or trigger transcode table rebuild. |
| **Rollback Command** | `sudo cp /var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm.bak_wo208 /var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm && sudo cp /var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf.bak_wo201 /var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/custom-convert.conf && sudo systemctl restart logitechmediaserver` |
| **Verification Read-Back** | During active playback of `ibizaglobalradio.mp3` on player `c8:2e:18:be:b8:34`, execute `ps -eo pid,args | grep -E 'SqueezeDSP|lame' | grep -v grep` on `10.0.0.10` and verify zero active transcode processes. Query `GET http://172.22.1.15/status.json` to confirm `"codec": "MP3"`, `frames_played` incrementing, and `i2c_nacks: 0`. |
| **Blocked By / Related** | Related to `ANOM-03`, `WO-201`, `WO-204`, `WO-208` |

---

### 3.3 Deep Diagnostic Investigation Findings

#### 1. Investigation of `ANOM-03` (SqueezeDSP Transcode Pipeline)
- **Question**: Why is `lame --mp3input --decode | SqueezeDSP --Clientid=c8:2e:18:be:b8:34 --bitsout=24 --formatout=MP3 | lame --preset insane` running for `ibizaglobalradio.mp3`? Is `Bypass` active?
- **Ground Truth**:
  - In `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs`, `bypass: '1'`, `Client.bypass: '1'`, and `enabled: '0'` are explicitly configured.
  - In `/var/log/squeezeboxserver/squeezedsp.log`, `LyrionDSPProcessAudio:ByPassProcess: Bypass mode enabled - minimal processing` is confirmed logged at stream startup (`2026-11-01 07:43:34`). The `SqueezeDSP` binary is running as a pure pass-through filter.
  - In `Configuration.pm::removeNativeConversion()`, the subroutine removes native direct passthrough rules (`command eq "-"`) unless the format is explicitly matched by `$inputtype =~ /^(aac|ogg|alc|ops)$/`. `mp3` was never added to this regex.
  - Because the native direct passthrough `mp3 mp3 * -` is removed, LMS matches the SqueezeDSP rule in `custom-convert.conf`:
    `mp3 mp3 * c8:2e:18:be:b8:34` -> `[lame] --mp3input --decode --silent $FILE$ - | [SqueezeDSP] --trackURL=$URL$ --Clientid="c8:2e:18:be:b8:34" --bitsout=24 --formatout=MP3`
  - Because SqueezeDSP produces raw PCM marked with format `MP3`, LMS invokes the default transcode pipeline rule to re-encode the stream to MP3 via `/bin/lame -r --signed --little-endian -s 44.100 --bitwidth 32 -m s --preset insane - -`.
  - **Conclusion**: The pipeline is completely redundant. SqueezeDSP is in bypass, yet two `lame` instances are actively executing on `10.0.0.10`.

#### 2. Investigation of `ANOM-05` (Socket Backlog & Listen Queue Analysis)
- **Question**: What is the root cause of `Send-Q: 4096` in `ss -tlnp`? Is the queue backlogged?
- **Ground Truth**:
  - `ss -tlnp` command output:
    ```text
    State  Recv-Q Send-Q Local Address:Port Peer Address:Port
    LISTEN 0      4096         0.0.0.0:9000      0.0.0.0:*
    LISTEN 0      4096         0.0.0.0:9090      0.0.0.0:*
    LISTEN 0      4096         0.0.0.0:3483      0.0.0.0:*
    ```
  - Linux kernel documentation for `ss(8)` establishes that for sockets in `LISTEN` state:
    - `Send-Q` displays the configured listen backlog limit (`backlog` parameter passed to `listen(2)` capped by `net.core.somaxconn`).
    - `Recv-Q` displays the current count of established unaccepted connections waiting in the socket's accept queue.
  - `/proc/sys/net/core/somaxconn` on `10.0.0.10` is verified to be exactly `4096`.
  - `Recv-Q` is verified to be `0` across all listening ports (`9000`, `9090`, `3483`).
  - **Conclusion**: There is zero queue congestion, zero backlog delay, and zero socket leaks. `ANOM-05` is completely benign; the socket backlog is healthy and operating exactly as designed.

#### 3. Investigation of Multi-Protocol Bridges
- **ShairTunes2W (`shairport_helper`)**:
  - Inspected `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/ShairTunes2W/Plugin.pm` lines 311 and 351:
    `next if ($client->modelName =~ /Bridge/ || $client->model =~/squeezeesp32/ || ($client->model =~ /squeezelite/ && !$client->firmware)) && !$prefs->get('squeezelite');`
  - In `/var/lib/squeezeboxserver/prefs/plugin/shairtunes.prefs`, `squeezelite: 0` is configured.
  - Server log confirms that when `c8:2e:18:be:b8:34` reconnected at `01:55:57`, ShairTunes2W intentionally skipped spawning a helper for `BASEMENT`.
  - This is optimal because `louder-esp32-app` runs native pure-Rust AirPlay 1 (`squeezelite-raop`) directly on the ESP32 (`172.22.1.15:5000`). Having `shairport_helper` running concurrently on LMS would create duplicate mDNS announcements on the LAN.
- **CastBridge (`squeeze2cast`)**:
  - `squeeze2cast-linux-aarch64-static` (PID 36381) has been active for 3 days, 12 hours without crashing.
  - Memory consumption is stable at 22 MB RSS (2.3% of host RAM), CPU usage is 0.6%.
  - Both configured Chromecast endpoints (`cc:cc:a0:42:cb:65` and `cc:cc:8e:1d:37:85`) are connected and active in LMS.
- **UPnP / DLNA MediaRenderer**:
  - `server.prefs` has `noupnp: '0'`.
  - `curl -s -I http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34` returned `HTTP/1.1 200 OK` (3,105 bytes). UPnP endpoint is active and advertising `BASEMENT`.

---

### 3.4 Verification Read-Back & Task Completion Barrier
- `manage_task(list)` was executed and confirmed: **0 background tasks running**.
- Strict read-only constraint observed: zero files modified on `10.0.0.10`, zero services restarted.
- Ready for Phase 2 Master Hardware Matrix consolidation.
