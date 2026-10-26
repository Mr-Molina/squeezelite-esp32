# Work Order `WO-055`: `/deep-code-audit` Micro-Auditor 3 — Chromecast (CastV2), Spotify Connect (`cspot`), & UPnP/DLNA Feature-Parity Audit

- **ID**: `WO-055`
- **Timestamp**: `2026-10-24T16:11:30-04:00`
- **Subagent Role**: Cast / Spotify Connect / UPnP Feature-Parity Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of Chromecast, Spotify Connect (`cspot`), and UPnP/DLNA across the repository (`components/`, `main/`, and `rust/crates/`) and live Lyrion Music Server (`10.0.0.10:9000`):
1. Inspect the C `squeezelite-esp32` codebase (`components/`, `main/`, `CMakeLists.txt`, `sdkconfig*`) to determine:
   - Does C `squeezelite-esp32` implement native Chromecast receiver (`CastV2` TLS / protobuf / mDNS `_googlecast._tcp`) on the ESP32 at all, or is Chromecast in the Squeezebox ecosystem handled via Lyrion Music Server's `CastBridge` / `ShairTunes2W` / `UPnPBridge` plugins?
   - How does C `squeezelite-esp32` implement on-device Spotify Connect (`cspot` in `components/cspot/`), and how does that compare to LMS `Spotty`?
   - Does C `squeezelite-esp32` implement any UPnP/DLNA renderer, or only LMS SlimProto + AirPlay (`raop`) + BT Sink (`bt_sink`) + Spotify Connect (`cspot`)?
2. Query live Lyrion Music Server (`http://10.0.0.10:9000/jsonrpc.js`) for installed/available plugins (`["can", ...]`, `serverstatus`, or `/settings/index.html` plugins list) to see what bridges (Spotty, AirPlay/ShairTunes2, Chromecast/CastBridge, UPnP) are active on `10.0.0.10`.
3. Formulate structured findings (`FA-CAST-*`, `FA-CSPOT-*`) with exact file/line citations comparing C `squeezelite-esp32` vs `rust/crates/*` vs LMS `10.0.0.10`.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify any repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3) and do not interrupt `172.22.1.15` playback.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `d093e06e-c78e-4ad7-9741-0131d77af7b4` (`Cast, Spotify Connect & UPnP Auditor`)
- **Key Findings (`FA-CAST-001`..`FA-CAST-002`, `FA-CSPOT-001`..`FA-CSPOT-003`)**:
  - `FA-CAST-001`: Neither C `squeezelite-esp32` nor `rust/crates/*` implements an on-device Chromecast (`CastV2`) receiver (which requires proprietary Google Cast device root CA certificates). In LMS (`10.0.0.10:9000`), `CastBridge v3.5.4` (`squeeze2cast-linux-aarch64-static`) bridges Chromecast devices on the LAN into LMS as virtual Squeezebox players (`cc:cc:...`).
  - `FA-CAST-002`: Neither C `squeezelite-esp32` nor `rust/crates/*` implements an on-device UPnP/DLNA renderer; UPnP in the Squeezebox ecosystem is provided by LMS plugins (`UPnPBridge` / `Whitebear` / `UPnP/DLNA Media Interface`).
  - `FA-CSPOT-001` & `FA-CSPOT-003`: C `components/spotify/` (`cspot`) implements an on-device C++ Spotify Connect ZeroConf/Mercury/Shannon/Vorbis receiver (`_spotify-connect._tcp`, `/spotify_info`, PSRAM-backed), whereas LMS `10.0.0.10:9000` runs `Spotty v4.62.3` (`spotty v2.1.2`) for LMS Spotify streaming. `rust/crates/*` currently has 0% on-device `cspot` Spotify Connect code.
