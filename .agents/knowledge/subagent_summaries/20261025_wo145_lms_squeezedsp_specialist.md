# Subagent Summary: `WO-145` LMS `10.0.0.10` `SqueezeDSP` Bypass & Sibling Bridge Specialist

- **Date**: 2026-10-25T21:52:30-04:00
- **Work Order**: `WO-145`
- **Subagent Role**: LMS `10.0.0.10` `SqueezeDSP` Bypass & Sibling Bridge Specialist
- **Target Host**: Lyrion Music Server `10.0.0.10`
- **Target Player**: `"BASEMENT"` (`c8:2e:18:be:b8:34`, `172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Created, Modified, or Deleted

### Modified on Remote LMS Host (`10.0.0.10`):
1. `/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs`:
   - Updated under `_client:c8:2e:18:be:b8:34:` with `bypass: '1'` and `Client.bypass: '1'`.
   - Verified ownership `squeezeboxserver:nogroup` and mode `0644`.
2. `/var/lib/squeezeboxserver/prefs/SqueezeDSP/Settings/c8_2e_18_be_b8_34.settings.json`:
   - Updated to `{ "ClientName": "", "Revision": 0, "Client": { "Bypass": "1" } }`.
   - Verified ownership `squeezeboxserver:nogroup` and mode `0644`.

### Backups Created:
1. `10.0.0.10:/var/lib/squeezeboxserver/prefs/plugin/squeezedsp.prefs.bak.20261025`
2. `10.0.0.10:/var/lib/squeezeboxserver/prefs/SqueezeDSP/Settings/c8_2e_18_be_b8_34.settings.json.bak.20261025`
3. Local: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/squeezedsp.prefs.bak`
4. Local: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/c8_2e_18_be_b8_34.settings.json.bak`

### Local Workspace Files Created:
1. `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/update_squeezedsp.py` (offline Python updater script for `10.0.0.10`)
2. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo145_lms_squeezedsp_specialist.md` (this report)

---

## 2. Rationale for Each Change
- **SqueezeDSP Transcoding Overhead Elimination**: SqueezeDSP was previously intercepting native MP3 streams for `"BASEMENT"` (`c8:2e:18:be:b8:34`), running a decoding `lame` process, piping through the Go binary `SqueezeDSP`, and re-encoding through a second `lame` instance (`--preset insane`).
- **Dual Preference Architecture**: SqueezeDSP uses both the LMS Perl preferences (`plugin/squeezedsp.prefs`) and its own internal Go JSON settings store (`SqueezeDSP/Settings/<client_id>.settings.json`). Setting `bypass: 1` in both locations while `logitechmediaserver.service` was stopped ensures that on service startup, SqueezeDSP enters bypass mode and eliminates the second re-encoding `lame` process.

---

## 3. Empirical Verification Evidence

### A. Pre-Restart Baseline (Invariant 44 / `verify-autonomous-work.md`)
- `lyrionmusicserver.service`: Active (running)
- `squeeze2cast`: PID 12150 active
- `avahi-publish-service`: PID 14338 active (`A1C6A30629A8@BASEMENT _raop._tcp 36201`)
- SqueezeDSP pipeline previously running: `lame (decode) | SqueezeDSP | lame (insane re-encode)`

### B. Post-Restart Sibling Bridges (`verify-autonomous-work.md`)
- `CastBridge`: Active (PID 16746 `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/CastBridge/Bin/squeeze2cast-linux-aarch64-static`)
- `AirPlay (ShairTunes2W)`: Active (PID 16829 `/bin/avahi-publish-service A1C6A30629A8@BASEMENT _raop._tcp 44769 44769`)
- `UPnP/DLNA`: Query `curl -sS -I "http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=c8:2e:18:be:b8:34"` confirmed `HTTP/1.1 200 OK`, `Content-Length: 3105`, with `<friendlyName>BASEMENT</friendlyName>` and `<serialNumber>c8:2e:18:be:b8:34</serialNumber>`.

### C. Live Playback & Stream Pipeline Verification
- Triggered playback via JSON-RPC Preset 1 (*Ibiza Global Radio*, `http://cdn-peer012.streaming-pro.com:8024/ibizaglobalradio.mp3`).
- Verified downstream re-encoding `lame --preset insane` process was completely eliminated. SqueezeDSP operates in native bypass mode.
- Player status (`http://172.22.1.15/status.json`):
  - `frames_played`: Advanced continuously from `0` -> `101,312` -> `507,584` -> `988,864` -> `1,611,584`.
  - `amp_mode`: `"Play"`
  - `tas_automuted`: `false`
  - `tas_pll_locked`: `true`
  - `i2c_nacks`: `0`
  - `codec`: `"MP3"`
- Host socket status (`ss -tnp dst 172.22.1.15`):
  - Stream socket on `10.0.0.10:9000` connected to `172.22.1.15:52424` with healthy buffering (`Send-Q ~ 157k-169k`), zero socket errors.

---

## 4. Known Risks or Untested Assumptions
- None identified. Local Rust codebase and `/dev/ttyUSB0` were completely untouched per strict constraints.
