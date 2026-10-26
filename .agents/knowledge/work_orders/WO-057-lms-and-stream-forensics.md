# Work Order `WO-057`: `/deep-hardware-audit` + `/deep-code-audit` Micro-Auditor 1 — Live LMS (`10.0.0.10:9000`) & `"BASEMENT"` (`172.22.1.15`) Stream Forensics

- **ID**: `WO-057`
- **Timestamp**: `2026-10-24T17:42:00-04:00`
- **Subagent Role**: LMS Server & Stream Session Forensics Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Investigate live Lyrion Music Server (`10.0.0.10:9000`) and target `172.22.1.15` (`"BASEMENT"`, MAC `c8:2e:18:be:b8:34`) to determine what happened between `16:07` and `17:41` (`frames_played: 297255`, `i2c_acks: 280`, `mode: "stop"`, `playlist_tracks: 0`):
1. Query live LMS `http://10.0.0.10:9000/jsonrpc.js` and `http://10.0.0.10:9000/server.log` (or `/var/log/squeezeboxserver/server.log` via HTTP `/log.txt` or `/settings/server/log.html`) to see:
   - What tracks/URLs were played on `"BASEMENT"` (`c8:2e:18:be:b8:34`) or other players.
   - Why `"BASEMENT"` transitioned to `mode: "stop"` with `playlist_tracks: 0` after `297,255` frames (`~6.74` seconds).
   - What SlimProto `STAT` events (`STMd`, `STMu`, `STMn`, `STMo`, `STMs`, `STMt`, `STMf`) LMS received from `172.22.1.15` and whether LMS logged any stream or player errors for `c8:2e:18:be:b8:34`.
2. Perform a live read-only test or trace of what LMS sends in `strm 's'` + `cont` + HTTP response headers (`http://10.0.0.10:9000/stream.mp3?player=c8:2e:18:be:b8:34`) when playing radio vs Spotify vs local library tracks.
3. Return structured findings (`F-LMS-*`) with exact log lines and JSON-RPC evidence.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `7e22e321-64a2-4661-95ea-49b919fb52ad` (`LMS Server & Stream Session Forensics Auditor`)
- **Key Findings (`F-LMS-1`..`F-LMS-5`)**:
  - `F-LMS-1` & `F-LMS-2`: Between `14:08:00 PDT` (`17:08 EDT`) and `14:36:44 PDT` (`17:36 EDT`), the user attempted to play 4 streams on `"BASEMENT"` (`c8:2e:18:be:b8:34`):
    1. `https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3` (*Ibiza Global Radio*)
    2. `file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav`
    3. `https://whyy.streamguys1.com/whyy_tunein` (*WHYY*)
    4. `https://war.streamguys1.com:7883/wdiy_7880` (*WDIY*)
  - `F-LMS-3`: LMS `server.log` logged fatal errors on all 4 attempts:
    - `Slim::Player::Song::open (426) Error: Couldn't create command line for mp3 playback for [https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3]`
    - `Slim::Player::Song::open (426) Error: Couldn't create command line for wav playback for [file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav]`
    - `Slim::Player::Song::open (426) Error: Couldn't create command line for mp3 playback for [https://whyy.streamguys1.com/whyy_tunein]`
    - `Slim::Player::Song::open (426) Error: Couldn't create command line for mp3 playback for [https://war.streamguys1.com:7883/wdiy_7880]`
    - Root cause in `/settings/server/filetypes.html`: For player `c8:2e:18:be:b8:34`, the **SqueezeDSP (v0.1.72)** plugin intercepts format conversions (`mp3-flc-*-c8:2e:18:be:b8:34`, `wav-flc-*-c8:2e:18:be:b8:34`, `spt-flc-*-c8:2e:18:be:b8:34`) and outputs **FLAC (`flc`)**, while disabling/overriding native `mp3`/`wav`/`pcm` rules! Because `louder-esp32-app` only advertised `,pcm,wav,aif,mp3` (without `,flc` / native FLAC decoder) in `HELO`, once `SqueezeDSP` bound to `c8:2e:18:be:b8:34`, LMS had NO valid conversion path (`mp3 -> flc` blocked by missing `,flc` in `HELO`, and `mp3 -> mp3` / `wav -> pcm` overridden by `SqueezeDSP`)!
