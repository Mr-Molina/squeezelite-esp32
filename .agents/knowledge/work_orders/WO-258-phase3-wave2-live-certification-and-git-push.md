# Work Order: WO-258 — Phase 3 Wave 2 Live `"BASEMENT"` (`172.22.1.15`) OTA Certification & Final Git Push to `origin/MK5-Rust`

- **Work Order ID**: `WO-258`
- **Timestamp**: `2026-11-01T09:22:30-05:00`
- **Assigned Subagent Role**: `Extension Certifier & Infrastructure Administrator` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**:
  - Live `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`)
  - Lyrion Music Server (`10.0.0.10:9000` / `:3483`)
  - Git Repository `/home/console-admin/Github/squeezelite-esp32` (Branch: `MK5-Rust`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
1. Flash the staged Phase 3 remediated OTA binary (`/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-phase3-remediated.bin`) to `"BASEMENT"` (`172.22.1.15`) using the strict 1-Flash Circuit Breaker (`POST /recovery.json` -> `POST /flash.json`).
2. Certify all 6 Live Hardware Gates on `"BASEMENT"` (`172.22.1.15`):
   - **Gate 1 (Boot Lease & Hardware Health)**: Wait 60s for the Gate 3 45s lease window (`uptime_ms > 46,000`), verify `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `psram_free_bytes + psram_used_bytes == 4,194,304`, `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, AND verify `BUS-001`/`PERF-601` (`i2c_acks` delta over 10s reduced from `~5.9` ACKs/s to `~3.0` ACKs/s).
   - **Gate 2 (Web UI, Phase 3 REST APIs, & TAS5805M Control)**: Verify `GET /` (`200 OK`, `3,928` B), `/cmd.json?act=vol55`, `/config.json`, `/scan.json`, `/messages.json`, `/commands.json` (`200 OK`).
   - **Gate 3 (SlimProto Multi-Codec & Native MP3 Streaming Without `lame | SqueezeDSP`)**: Verify FLAC playback and *Ibiza Global Radio* (`preset1`) MP3 playback (`frames_played` advancing at `~44,100` fps, `i2c_nacks: 0`, and `0` `lame | SqueezeDSP` processes on `10.0.0.10`).
   - **Gate 4 (AirPlay 1 RAOP & `SourceArbiter` Preemption + Decoder Restoration)**: Verify RTSP `5000` `200 OK`, ALAC UDP `6000` preemption, and clean restoration of LMS codec after `TEARDOWN` (`LOGIC-206`).
   - **Gate 5 (Spotify Connect ZeroConf)**: Verify `GET /?action=getInfo` (`status: 101`, `remoteName: "BASEMENT"`).
   - **Gate 6 (Bluetooth Classic/BLE Coexistence & LMS Sibling Bridges)**: Verify `l2ping -c 3 CA:2E:18:BE:B8:35` (`0%` loss), `sdptool browse`, `CastBridge`, and `UPnP MediaRenderer`.
3. Commit all Phase 3 remediation changes (`WO-251`–`WO-258`) and push to `origin/MK5-Rust` (verifying `Invariant 13` & `Invariant 24`: zero `scratch/` or `.bin`/`.elf` files staged, zero secrets).

## 2. Fulfillment Section (`[EXECUTED & VERIFIED]`)
- **Fulfillment Timestamp**: `2026-11-01T11:35:00-05:00`
- **Subagent Role**: `Extension Certifier & Infrastructure Administrator`
- **Handoff Path**: `.agents/knowledge/subagent_summaries/20261101_wo258_live_cert_and_push.md`

### Certification Evidence Table
| Gate | Target / Action | Evidence & Measured Values | Status |
|---|---|---|---|
| **Pre-Flight OTA** | `POST /recovery.json` -> `POST /flash.json` | Binary size `1,800,000` B, SHA-256 `65b8953817d7b5d59dcc06adf4b92ea74c64c2bb8b09fdfdc8a897fd00f599e2`. Uploaded via raw octet-stream `HTTP 200 OK`, flashed 100%, rebooted into pure-Rust partition. | `[EXECUTED & VERIFIED]` |
| **Gate 1** | Boot Lease, Hardware Health, & `BUS-001` Coalesced I2C | `uptime_ms > 46,000` (`81,888` ms), `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `psram_free_bytes` (`1,521,992`) + `psram_used_bytes` (`2,672,312`) = `4,194,304` B (4 MB intact), `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`. `i2c_acks` over 10.03s: `30` delta = `2.99` ACKs/s (reduced from `~5.9` ACKs/s). | `[EXECUTED & VERIFIED]` |
| **Gate 2** | Web UI, REST APIs, & TAS5805M Control | `GET /` -> `200 OK` (`3,928` B); `cmd.json?act=vol51` -> `vol_pct: 51`. REST endpoints `/config.json`, `/scan.json`, `/messages.json`, `/commands.json` all returned `200 OK` JSON. | `[EXECUTED & VERIFIED]` |
| **Gate 3** | SlimProto Multi-Codec & Native MP3 Streaming | Native FLAC (`/var/lib/squeezeboxserver/cache/test_stereo_16bit.flac`) played with `"codec": "FLAC"`, `frames_played` monotonically advancing (`10,368 -> 29,504`), `tas_pll_locked: true`, `i2c_nacks: 0`. Native MP3 (*Ibiza Global Radio* `preset1`) played with `"codec": "MP3"`, `frames_played` advancing (`103,168 -> 233,856`), PSRAM reclaimed (`1,317,088 -> 1,521,956`). `ps -ef | grep -E 'lame|SqueezeDSP'` returned strictly 0 processes on `10.0.0.10`. | `[EXECUTED & VERIFIED]` |
| **Gate 4** | AirPlay 1 RAOP & `SourceArbiter` Preemption + Restoration | RTSP `5000` `OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`, `SET_PARAMETER` returned `200 OK`. UDP `6000` ALAC streamed: `"codec": "AirPlay"`, `frames_played` advancing (+84,424 frames), `tas_pll_locked: true`, `i2c_nacks: 0`. RTSP `TEARDOWN` returned `200 OK` and restored LMS MP3 stream (`LOGIC-206`). | `[EXECUTED & VERIFIED]` |
| **Gate 5** | Spotify Connect ZeroConf & `SourceArbiter` Preemption | `GET /?action=getInfo` returned `status: 101`, `remoteName: "BASEMENT"`, `brandDisplayName: "cspot"`. `cmd.json?act=spotify_play` switched to `"codec": "Spotify"`, `tas_pll_locked: true`, `i2c_nacks: 0`. `cmd.json?act=spotify_stop` cleanly restored LMS stream (`"codec": "MP3"`, `frames_played` advancing). | `[EXECUTED & VERIFIED]` |
| **Gate 6** | Bluetooth Classic/BLE & LMS Sibling Bridges | `l2ping -c 3 CA:2E:18:BE:B8:35` passed `0%` packet loss (`92.02ms`, `36.00ms`, `39.84ms`). `sdptool browse` verified `"Audio Sink" (0x110b)` and `AVDTP 0x0103`. LMS `10.0.0.10`: `squeeze2cast` (PID 49931) running, UPnP `MediaRenderer.xml` returned `200 OK` (3,105 B). | `[EXECUTED & VERIFIED]` |
