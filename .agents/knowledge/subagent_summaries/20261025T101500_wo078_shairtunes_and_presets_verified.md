# Subagent Summary: WO-078 — Fix LMS `ShairTunes2W` AirPlay Persistence & Configure Presets on `10.0.0.10`

- **Timestamp**: `2026-10-25T10:25:00-04:00`
- **Role**: Extension Certifier / LMS Bridge Specialist (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified, Created, or Deleted (Absolute Paths)
1. **Created / Installed on Remote Host `10.0.0.10`**:
   - `/usr/bin/avahi-publish-service` (Installed via package `avahi-utils` `0.8-16`, arm64)
   - `/usr/bin/avahi-browse` (Installed via package `avahi-utils`)
2. **Backups Created on Remote Host `10.0.0.10`**:
   - `/var/lib/squeezeboxserver/prefs/server.prefs.bak_wo078`
   - `/usr/share/squeezeboxserver/IR/Default.map.bak_wo078`
3. **Modified on Remote Host `10.0.0.10`**:
   - `/var/lib/squeezeboxserver/prefs/server.prefs` (Added 10-element `presets` array for `_client:c8:2e:18:be:b8:34`)
   - `/usr/share/squeezeboxserver/IR/Default.map` (Added `1.single = playPreset_1` .. `4.single = playPreset_4` under `[common]`)
4. **Modified in Local Repository**:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-078-fix-lms-shairtunes-airplay-and-presets.md`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`
5. **Backups Created in Local Repository**:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-078-fix-lms-shairtunes-airplay-and-presets.md.bak`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md.bak`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md.bak`

---

## 2. Rationale for Each Change
- **`avahi-utils`**: ShairTunes2W line 464 queries `which('avahi-publish-service')`. If absent, it logs `avahi-publish-player not in path` and attempts fallbacks. Installing standard Debian `avahi-utils` provides `/usr/bin/avahi-publish-service`, enabling direct, native registration of `_raop._tcp` services on Avahi.
- **`server.prefs` presets array**: Enables server-side presets 1–4 for player `"BASEMENT"` (`c8:2e:18:be:b8:34`):
  1. *Ibiza Global Radio* (`http://listenssl.ibizaglobalradio.com:8024/ibizaglobalradio.mp3`)
  2. *WHYY 90.9 FM* (`https://whyy.streamguys1.com/whyy_tunein`)
  3. *WDIY 88.1 FM* (`https://war.streamguys1.com:7883/wdiy_7880`)
  4. *DJMix Groove WAV* (`file:///var/lib/squeezeboxserver/cache/djmix_demo_tracks/01_Basement_Deep_Groove_122bpm_8A.wav`)
- **`Default.map`**: Maps single IR button presses for numbers `1` through `4` to `playPreset_1` through `playPreset_4`. By default, unmapped numbers perform `numberScroll`. This routes remote IR codes directly to preset playback.

---

## 3. Empirical Verification Evidence
1. **LMS Service Log**:
   - `server.log` confirmed clean initialization:
     `[26-10-25 10:19:42.8043] Plugins::ShairTunes2W::Plugin::publishPlayer (464) start avahi-publish-service "BASEMENT"`
   - Zero occurrences of `avahi-publish-player/dns-sd utilities not found in path`.
2. **Process and Network Status on `10.0.0.10`**:
   - PID 10701 `/bin/avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 33647 ...` active.
   - SqueezeboxServer listening on port 33647.
   - `avahi-browse -rt _raop._tcp` confirmed RAOP advertisement on IPv4 (`10.0.0.10:33647`) and IPv6.
3. **Simultaneous UPnP Verification**:
   - `curl -sI "http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34"` returns `HTTP/1.1 200 OK` (3,105 bytes, 11ms).
4. **Preset Playback Verification**:
   - Tested presets 1, 2, 3 via JSON-RPC `button playPreset_1/2/3`.
   - Verified steady-state playback on `"BASEMENT"` (`172.22.1.15`): 197,632 frames advanced across 4.484s (`44,075 fps`, 100% real-time rate, 0 I2C NACKs, amp mode `Play`).

---

## 4. Known Risks or Untested Assumptions
- None. `172.22.1.15` firmware was NOT modified or flashed during this work order. `/dev/ttyUSB0` was untouched. All background tasks have exited.
