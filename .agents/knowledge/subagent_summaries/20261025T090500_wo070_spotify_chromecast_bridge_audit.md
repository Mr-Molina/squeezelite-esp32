# Subagent Summary: `WO-070` Spotify Connect, Google Cast & UPnP Bridges Audit

- **Timestamp**: `2026-10-25T09:14:00-04:00`
- **Subagent Role**: Extension Certifier / LMS Bridge Specialist (`511af60d-679e-410c-840e-792313c545dc`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Executive Summary
An exhaustive audit of LAN audio bridge technologies on `10.0.0.10` (Lyrion Music Server 9.1.1 on Debian 13 trixie aarch64) and host `console-admin` (`10.0.0.90`) was performed for target player `"BASEMENT"` (`c8:2e:18:be:b8:34` / `172.22.1.15`).

1. **Apple AirPlay (`ShairTunes2W`) — WORKING (`[EXECUTED & VERIFIED]`)**:
   - `shairport_helper-linux-aarch64` (PID 8253) is actively running on `10.0.0.10:46657`, advertising `A1C6A30629A8@BASEMENT` on `_raop._tcp` via `avahi-daemon`.
   - Apple's AirPlay 1 / RAOP protocol uses an extracted, published 1024-bit RSA private key (`airport.key`), allowing open-source software to emulate an authentic AirPlay receiver.

2. **Google Chromecast Receiver — ARCHITECTURAL LIMITATION (`[EXECUTED & VERIFIED]`)**:
   - `CastBridge` (`squeeze2cast-linux-aarch64-static` v3.5.4, PID 6865) on LMS is strictly an **outbound transmitter / client** that discovers physical Chromecast/Google Nest devices on the LAN and presents them as LMS players.
   - Emulating an **inbound Google Cast receiver** requires implementing `CastV2` over TLS on TCP port `8009`. When a sender (Android, Chrome, YouTube, Spotify) initiates a cast session, it issues a `DeviceAuthMessage` challenge expecting an `AuthResponse` signed by a private key whose certificate (`CastDeviceCert`) chains directly to Google's proprietary Root CA.
   - These cryptographic keys are burned into hardware OTP fuses on certified Google silicon. Software emulators cannot pass modern sender validation without genuine Google hardware certificates, explaining why Google Cast receiver functionality does not and cannot exist in standard LMS plugins.

3. **Spotify Connect (`Spotty` / `librespot`) — DEPRECATED UPSTREAM IN SPOTTY (`[EXECUTED & VERIFIED]`)**:
   - `Spotty` v4.62.3 is installed on `10.0.0.10` using helper binary `spotty v2.1.2` (`librespot 0.8.0 3b817a9`).
   - The plugin author (Michael Herger) explicitly removed Spotify Connect daemon mode from `Spotty` (`PLUGIN_SPOTTY_SPOTIFY_CONNECT_DESC_LONG` in `strings.txt`: *"What happened to Connect mode? It's gone. With the latest API changes on Spotify's end I had to catch up with librespot development... As Connect mode had been 'spotty' anyway, I decided to remove it."*).
   - In Spotty v4, Spotify music can be browsed and played natively through LMS web/Material interfaces (using on-demand `--single-track` transcoding to Squeezelite), but LMS players cannot act as native Spotify Connect targets in the official Spotify app via Spotty.
   - **Alternative Solution**: An independent `librespot` daemon can be run with `--onevent` piping to an LMS stream/FIFO, or an embedded `cspot` receiver can run on the ESP32 firmware in future iterations.

4. **UPnP / DLNA MediaRenderer — AVAILABLE BUT CURRENTLY DISABLED (`[EXECUTED & VERIFIED]`)**:
   - LMS includes a built-in plugin `Slim::Plugin::UPnP::MediaRenderer` (`/usr/share/perl5/Slim/Plugin/UPnP/MediaRenderer.pm`).
   - When active, it automatically registers every connected LMS player (including `"BASEMENT"`) as a UPnP/DLNA MediaRenderer (`urn:schemas-upnp-org:device:MediaRenderer:1`) advertised via SSDP (`239.255.255.250:1900`).
   - In `server.prefs`, it is currently set to `noupnp: 1` (`<defaultState>disabled</defaultState>`). Enabling UPnP provides an immediate, standards-compliant casting target for Android and desktop streaming apps (e.g., BubbleUPnP, mconnect, VLC).

---

## 2. Files Modified or Created
1. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`:
   - *Rationale*: Appended diagnostic entry 49 recording all LMS bridge findings per Rule 11.
2. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T090500_wo070_spotify_chromecast_bridge_audit.md`:
   - *Rationale*: Standard subagent lifecycle summary artifact per Rule 17.
3. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-070-audit-spotify-connect-and-chromecast-bridges.md`:
   - *Rationale*: Updated Fulfillment Section to `[EXECUTED & VERIFIED]` with empirical evidence.
4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`:
   - *Rationale*: Updated status of `WO-070` in centralized ledger per Rule 14.

---

## 3. Risks & Untested Assumptions
- **Target Device Isolation**: `172.22.1.15` was left completely untouched throughout `WO-070` and continues streaming Ibiza Global Radio in pure-Rust mode (`435b5cf6`).
- **LMS UPnP Enabling**: Enabling `Slim::Plugin::UPnP::MediaRenderer` requires toggling `noupnp: 0` in `server.prefs` and restarting `squeezeboxserver`, which would briefly pause playback for all LMS players.
