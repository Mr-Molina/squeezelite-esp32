# Subagent Summary: `WO-061` LMS AirPlay (`ShairTunes2W`), UPnP/DLNA Cast & `SqueezeDSP` (`F-LMS-3`) Remediation

- **Timestamp**: `2026-10-24T19:39:30-04:00`
- **Subagent Role**: Extension Certifier / LMS Bridge Specialist (`511af60d-679e-410c-840e-792313c545dc`)
- **Caller Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Summary of Actions & Empirical Evidence

### Task 1: Pre-Flight HTML Snapshots
- Captured pre-flight snapshots of Lyrion Music Server configuration from `http://10.0.0.10:9000`:
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_filetypes_snapshot.html` (`3.0 MB`)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_plugins_snapshot.html` (`344 KB`)
- Post-restart snapshots also preserved:
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_filetypes_post_restart.html` (`1.45 MB`)
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_plugins_post_restart.html` (`352 KB`)

### Task 2: Root Cause & Remediation of `F-LMS-3` (SqueezeDSP Transcoding Override)
- **Root Cause**: `SqueezeDSP v0.1.72` intercepted media streams for player `c8:2e:18:be:b8:34` (`BASEMENT`), forcing all formats (`mp3-flc-*-c8:2e:18:be:b8:34`, `wav-flc-*`, `spt-flc-*`) to transcode to FLAC (`flc`). Because `louder-esp32-app` advertised `,pcm,wav,aif,mp3` in `HELO` (missing `,flc`), LMS failed to build the transcoding pipeline and aborted with `Slim::Player::Song::open (426) Error: Couldn't create command line for mp3/wav playback`.
- **Remediation**: Disabled SqueezeDSP specifically on `c8:2e:18:be:b8:34` via JSON-RPC:
  `["c8:2e:18:be:b8:34", ["pref", "plugin.squeezedsp:enabled", "0"]]`.
  Read-back confirmed `_p2: '0'`.
- **Empirical Verification**:
  - Played `file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav` -> `mode: "play"`, `time: 1.93s`.
  - Played `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3` -> `mode: "play"`, `time: 1.96s`.
  - Inspected `/var/log/squeezeboxserver/server.log` (`http://10.0.0.10:9000/server.log?lines=100`): **ZERO new `Slim::Player::Song::open (426) Error` entries occurred**.

### Task 3: Enable AirPlay (`ShairTunes2W`) & UPnP/DLNA Cast (`UPnP`)
- Submitted settings form on `http://10.0.0.10:9000/settings/server/plugins.html` enabling `ShairTunes2W` ("ShairTunes2 (fork) - Airtunes for LMS v1.9.17") and `UPnP` ("UPnP/DLNA Media Interface v1.0").
- Triggered server restart via `/settings/server/plugins.html?restart=1` and `["restartserver"]`.
- Server restarted cleanly in 44.9s (`v9.1.1, perl 5.040001 - aarch64-linux-gnu-thread-multi`).
- Player `"BASEMENT"` (`c8:2e:18:be:b8:34` / `172.22.1.15:53800`) reconnected to LMS immediately.
- **AirPlay Live Verification**:
  - `server.log` confirmed `Plugins::ShairTunes2W::Plugin::publishPlayer` spawned `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/ShairTunes2W/Bin/shairport_helper-linux-aarch64` and created an AirPlay listener on TCP port `46189` for client `c8:2e:18:be:b8:34`.
  - ShairTunes2W settings page (`/settings/plugins/ShairTunes2W/settings/basic.html?playerid=c8:2e:18:be:b8:34`) confirmed:
    `<td><input name="enabled.c8:2e:18:be:b8:34" type="checkbox" checked ></td> <th>BASEMENT</th><th>c8:2e:18:be:b8:34</th>`
  - Connected directly to TCP `10.0.0.10:46189` and sent `OPTIONS * RTSP/1.0`:
    Received `RTSP/1.0 200 OK` with `Public: ANNOUNCE, SETUP, RECORD, PAUSE, FLUSH, TEARDOWN, OPTIONS, GET_PARAMETER, SET_PARAMETER` and `Audio-Jack-Status: connected; type=analog`.
- **UPnP Live Verification**:
  - Confirmed `UPnP/DLNA Media Interface (v1.0)` is `checked=checked` in Active Plugins section of `/settings/server/plugins.html`.

### Task 4: Spotty Settings Inspection
- Inspected `/settings/plugins/Spotty/settings/basic.html?playerid=c8:2e:18:be:b8:34`.
- Upstream note from author Michael Herger confirms:
  *"What happened to Connect mode? It's gone. With the latest API changes on Spotify's end I had to catch up..."*
- Standalone Spotify Connect per-player daemon mode was retired upstream by the plugin author; Spotify playback continues to operate seamlessly via LMS library streaming (`Spotty ogg -> pcm` at 44.1 kHz, which verified at 44,074 fps on `172.22.1.15`).

---

## 2. Modified Files & Artifacts
1. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_filetypes_snapshot.html` (Created)
2. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_plugins_snapshot.html` (Created)
3. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_filetypes_post_restart.html` (Created)
4. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lms_plugins_post_restart.html` (Created)
5. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024T192330_lms_airplay_squeezedsp.md` (Created)
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-061-lms-airplay-cast-squeezedsp-fix.md` (Updated)
7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` (Updated)
8. `/home/console-admin/.gemini/antigravity/brain/511af60d-679e-410c-840e-792313c545dc/handoff.md` (Created)

Zero `.rs` files or hardware serial devices were touched (Scope Isolation & Rule 3 strictly preserved).

---

## 3. Risks & Assumptions
- None. `c8:2e:18:be:b8:34` (`BASEMENT`) is continuously online (`uptime_ms: 12970632`, `frames_played: 3851776`, `amp_mode: "Play"`, `i2c_nacks: 0`) and successfully streaming from LMS.
