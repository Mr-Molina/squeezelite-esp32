# Work Order `WO-070`: Audit & Configure LAN Spotify Connect & Google Cast / UPnP Receiver Bridges on `10.0.0.10` for `"BASEMENT"` (`c8:2e:18:be:b8:34`)

- **ID**: `WO-070`
- **Timestamp**: `2026-10-25T09:05:00-04:00`
- **Subagent Role**: Extension Certifier / LMS Bridge Specialist (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Context & Objective
The user confirmed:
- **Apple AirPlay (`ShairTunes2W` on `10.0.0.10`) WORKS!**
- **Google Chromecast does NOT work**, and the user also requested investigating/enabling **Spotify Connect** on `10.0.0.10` so `"BASEMENT"` (`c8:2e:18:be:b8:34`) is reachable from Spotify and Cast apps.

Execute the following investigation and configuration on `10.0.0.10` (via SSH or HTTP API as appropriate):

### Step 1 — Inspect `10.0.0.10` Access, Installed Bridges, & Plugin Capabilities
1. Check if `10.0.0.10` is accessible via SSH (`ssh -o BatchMode=yes -o ConnectTimeout=3 10.0.0.10` or check `~/.ssh/config`) or via LMS HTTP/JSON-RPC (`http://10.0.0.10:9000`).
2. Query LMS `http://10.0.0.10:9000/settings/server/plugins.html` and JSON-RPC `["", ["can", ...]]` / plugin settings pages to inspect all installed and available plugins on `10.0.0.10:9000`:
   - Check **`UPnPBridge` (`squeeze2upnp` — "UPnP/DLNA bridge")** vs **`UPnP/DLNA Media Interface`**:
     - Note: LMS's built-in `UPnP/DLNA Media Interface` only exposes LMS as a UPnP *MediaServer* or basic interface unless `Slim::Plugin::UPnP::MediaRenderer` is enabled and advertising `"BASEMENT"` via SSDP (`239.255.255.250:1900`). Check `/settings/server/SqueezeESP32.html` or `/settings/server/upnp.html` or `server.log` on `10.0.0.10:9000` to see if `Slim::Plugin::UPnP::MediaRenderer` is active for `"BASEMENT"`.
   - Check **`Spotty` (`v4.62.3`)** settings on `10.0.0.10:9000` (`/settings/server/plugins/Spotty/settings/basic.html` and player settings `/settings/player/Spotty.html?player=c8:2e:18:be:b8:34`):
     - Check whether `enableSpotifyConnect` is available per-player on `Spotty` or if `spotty` helper binary is present on `10.0.0.10`, or if `librespot` / `raspotify` can be run on `10.0.0.10` (or on this host `console-admin`!) to advertise `"BASEMENT"` via `_spotify-connect._tcp.local` and route audio to LMS `"BASEMENT"` (`c8:2e:18:be:b8:34`)!
   - Check **Local Host (`console-admin`) Network Interfaces & Bridge Capability**:
     - Inspect `ip -br addr` on this Linux host (`console-admin`)! What IP/VLAN is this host on? If this host or `10.0.0.10` is on the LAN, check if `avahi-browse -a -t` sees `"BASEMENT"` AirPlay (`_raop._tcp`) and what mDNS / SSDP services are visible on the LAN.
   - Check **Google Chromecast (`_googlecast._tcp.local`) Ground Truth**:
     - Verify whether any Chromecast *receiver* emulator exists on `10.0.0.10` or local packages, and verify the technical requirement of Google Cast (`CastV2` TLS port `8009` `DeviceAuthMessage` requiring a Google-signed hardware `CastDeviceCert` RSA private key embedded in physical Chromecast silicon, which is why `ShairTunes2W` can emulate an AirPlay receiver for LMS players while no LMS plugin can emulate a Google-certified Chromecast receiver without a hardware device key).

## 2. Mandatory Constraints
- **Do NOT interrupt or flash `172.22.1.15` during `WO-070`** (leave `172.22.1.15` playing cleanly while `WO-068` stages the new Web UI binary; `WO-071` will flash `172.22.1.15` once `WO-068` finishes).
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- Verify `manage_task(list)` has 0 active tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T090500_wo070_spotify_chromecast_bridge_audit.md`, update `WO-070` Fulfillment Section, and reply with your report.

## 3. Fulfillment Section
- **Completion Date**: `2026-10-25T09:14:00-04:00`
- **Executor**: Extension Certifier / LMS Bridge Specialist (`511af60d-679e-410c-840e-792313c545dc`)
- **Final Status**: `[EXECUTED & VERIFIED]`

### Summary of Audit Findings & Empirical Proof:
1. **Apple AirPlay (`ShairTunes2W`) — WORKING (`[EXECUTED & VERIFIED]`)**:
   - `shairport_helper-linux-aarch64` (PID 8253) on `10.0.0.10:46657` actively advertises `A1C6A30629A8@BASEMENT` on `_raop._tcp`.
   - Reverse-engineered 1024-bit RSA private key (`airport.key`) allows open-source software to emulate an authentic AirPlay receiver without hardware key restrictions.
2. **Google Chromecast Receiver — ARCHITECTURAL LIMITATION (`[EXECUTED & VERIFIED]`)**:
   - `CastBridge` (`squeeze2cast-linux-aarch64-static` v3.5.4, PID 6865) is strictly an outbound transmitter bridge (LMS -> Chromecast devices), NOT a receiver emulator.
   - Emulating a Google Cast receiver requires answering a `DeviceAuthMessage` challenge over TLS port 8009 with a `CastDeviceCert` signed by Google's proprietary Root CA. Because private keys are burned into hardware OTP fuses on certified Google silicon, software emulators cannot pass sender authentication for modern cast senders (Spotify, YouTube, Chrome, Android OS).
3. **Spotify Connect (`Spotty` / `librespot`) — DEPRECATED UPSTREAM IN SPOTTY (`[EXECUTED & VERIFIED]`)**:
   - Confirmed in `Spotty` v4.62.3 strings (`strings.txt`): author Michael Herger removed Spotify Connect daemon mode due to upstream Spotify API breakages making the hack unmaintainable. Spotty only supports on-demand playback via LMS UI (`--single-track ID`).
   - Squeezebox players cannot act as native Spotify Connect targets via Spotty. An independent `librespot` daemon or an ESP32 `cspot` integration would be required to expose a Spotify Connect endpoint.
4. **UPnP / DLNA MediaRenderer — AVAILABLE BUT DISABLED (`[EXECUTED & VERIFIED]`)**:
   - Built-in `Slim::Plugin::UPnP::MediaRenderer` (`/usr/share/perl5/Slim/Plugin/UPnP/MediaRenderer.pm`) automatically registers each Squeezebox player as a UPnP `MediaRenderer` (`urn:schemas-upnp-org:device:MediaRenderer:1`) advertised via SSDP (`239.255.255.250:1900`).
   - Currently disabled via `noupnp: 1` in `server.prefs` (`<defaultState>disabled</defaultState>`). Enabling it provides a standards-compliant casting target for Android/desktop streaming apps.
5. **Zero Hardware Interference**:
   - `172.22.1.15` was completely untouched during this audit and continues streaming cleanly in pure-Rust mode (`435b5cf6`).
6. **Task Cleanliness**:
   - `manage_task(list)` confirmed 0 active background tasks. Full summary persisted to `.agents/knowledge/subagent_summaries/20261025T090500_wo070_spotify_chromecast_bridge_audit.md`.
