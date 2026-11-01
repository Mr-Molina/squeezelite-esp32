# Work Order: WO-272 — Perf Phase 3 Wave 1A Cluster 1: LMS (`10.0.0.10`) `mp3StreamingMethod=1` Proxied Streaming Remediation (`HW-PERF-201`)

- **Work Order ID**: `WO-272`
- **Timestamp**: `2026-11-01T14:32:00-05:00`
- **Assigned Subagent Role**: `Extension Certifier / LMS Stream Routing Specialist` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**: Lyrion Music Server (`10.0.0.10:9000`) player `"BASEMENT"` (`c8:2e:18:be:b8:34`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Remediate `HW-PERF-201` on Lyrion Music Server (`10.0.0.10:9000`) for `"BASEMENT"` (`c8:2e:18:be:b8:34`) so that LMS proxies external WAN MP3 streams over its Gigabit Ethernet connection and delivers them to `172.22.1.15` over the `5 ms` local LAN via `GET /stream.mp3?player=c8:2e:18:be:b8:34` with **zero transcoding** (`0` `lame` processes).
- **Constraint**: Do NOT modify any local Rust repository files in `/home/console-admin/Github/squeezelite-esp32/rust`. Do NOT restart `logitechmediaserver.service` unless needed (setting `playerpref` via JSON-RPC takes effect immediately on the next track/stream start).

## 2. Implementation & Verification Steps
1. **Backup Player Prefs on `10.0.0.10`**:
   - Via SSH (`console-admin@10.0.0.10`), copy `/var/lib/squeezeboxserver/prefs/server.prefs` to `/tmp/server.prefs.bak_wo272`.
2. **Apply `mp3StreamingMethod = 1` (Proxied Streaming) via JSON-RPC**:
   - Query current `mp3StreamingMethod` for `c8:2e:18:be:b8:34`:
     `curl -sS -X POST http://10.0.0.10:9000/jsonrpc.js -H 'Content-Type: application/json' -d '{"id":1,"method":"slim.request","params":["c8:2e:18:be:b8:34",["playerpref","mp3StreamingMethod","?"]]}'`
   - Set `mp3StreamingMethod` to `"1"` (Proxied):
     `curl -sS -X POST http://10.0.0.10:9000/jsonrpc.js -H 'Content-Type: application/json' -d '{"id":2,"method":"slim.request","params":["c8:2e:18:be:b8:34",["playerpref","mp3StreamingMethod","1"]]}'`
   - Independently read back `["c8:2e:18:be:b8:34", ["playerpref", "mp3StreamingMethod", "?"]]` to confirm `_p2: "1"`.
3. **Trigger Stream Restart & Verify LAN Proxy Socket (`10.0.0.10:9000 -> 172.22.1.15`)**:
   - Re-trigger `"Ibiza Global Radio"` (`["c8:2e:18:be:b8:34", ["playlist", "play", "http://listenssl.ibizaglobalradio.com:8024/ibizaglobalradio.mp3"]]` or preset 1) via JSON-RPC.
   - Wait 3 seconds, then verify on `10.0.0.10`:
     - `/usr/bin/ss -tnaip '( sport = :9000 or sport = :3483 )'` shows an active `ESTAB` HTTP stream connection from `10.0.0.10:9000` to `172.22.1.15`!
     - `/usr/bin/pgrep -af 'lame|SqueezeDSP'` returns **0 processes** (confirming native MP3 proxy without transcoding!).
   - Sample `GET http://172.22.1.15/status.json` twice 5 seconds apart and record the immediate improvement in `frames_played/s` from LAN proxying alone!
   - Also verify sibling bridges (`CastBridge` `squeeze2cast` process and `http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`) remain active.

## 3. Fulfillment Section

- **Execution Timestamp**: `2026-11-01T14:36:00-05:00`
- **Subagent Summary Artifact**: [`20261101_wo272_lms_mp3_proxy.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo272_lms_mp3_proxy.md)
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`

### Summary of Execution Results
1. **Backup**: Confirmed backup of `/var/lib/squeezeboxserver/prefs/server.prefs` to `/tmp/server.prefs.bak_wo272` on `10.0.0.10` (547,331 bytes).
2. **Preference Configuration**: Configured `mp3StreamingMethod = 1` for player `c8:2e:18:be:b8:34` via JSON-RPC. Independent read-back confirmed `_p2 = "1"`.
3. **Socket Verification**: Verified via `/usr/bin/ss -tnaip 'dst 172.22.1.15'` on `10.0.0.10` that active socket `ESTAB 10.0.0.10:9000 -> 172.22.1.15:58725` is established and serving proxied HTTP stream data to the ESP32.
4. **Transcoder Verification**: Executed `/usr/bin/pgrep -af 'lame|SqueezeDSP'` on `10.0.0.10` confirming **0 transcoding processes** active. Native MP3 stream proxying active.
5. **Player Performance & Frame Rate**: Sampled `http://172.22.1.15/status.json` twice (17.421s delta, 768,000 frames played), demonstrating a rock-solid nominal playback rate of **44,084.7 frames/s** (99.965% of nominal 44,100 Hz), zero I2C errors (`i2c_nacks: 0`), and TAS PLL locked.
6. **Sibling Service Integrity**: Confirmed CastBridge (`squeeze2cast`, PID 49931) and UPnP MediaRenderer XML endpoint (`http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34`, HTTP 200 OK) remain fully operational.
7. **Codebase Boundary**: Confirmed zero repository modifications to `/home/console-admin/Github/squeezelite-esp32/rust`.
