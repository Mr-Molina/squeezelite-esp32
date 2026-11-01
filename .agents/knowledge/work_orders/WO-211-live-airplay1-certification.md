# Work Order: WO-211 — Phase 1: Pure-Rust On-Device AirPlay 1 (RAOP) Integration & Live Hardware Certification on `"BASEMENT"`

- **Work Order ID**: `WO-211`
- **Timestamp**: `2026-10-29T09:47:00-04:00`
- **Assigned Subagent Role**: `Rust AirPlay Integration & Extension Certifier`
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`), `rust/crates/louder-esp32-app/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Integrate the pure-Rust `squeezelite-raop` engine completed in `WO-210` into the `louder-esp32-app` firmware runtime, pass all `--stage-only` verification gates, deploy via 1-Flash Circuit Breaker to `"BASEMENT"` (`172.22.1.15`), and certify on-device AirPlay 1 discovery, streaming, and hardware playback across all live gates.

*Hypothesis*: Integrating `squeezelite-raop` with `smoltcp` UDP mDNS (5353), TCP RTSP (5000), and UDP RTP (6000) audio receiver will allow `"BASEMENT"` to be directly discovered on the LAN as an AirPlay speaker and stream 16-bit 44.1 kHz ALAC audio directly to the TAS5805M amplifier in pure Rust, without requiring LMS server-side transcoding or `ShairTunes2W` bridge.

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash to `172.22.1.15` at most **ONCE** via Factory recovery `POST /flash.json`.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.
5. **Memory Section Limits**: `.stack >= 36,000` B, `.dram2_uninit <= 98,768` B. Sockets and packet buffers must reside in external PSRAM (`esp_alloc::HEAP`), not task stack.

## 3. Implementation & Verification Steps

1. **Firmware Integration in `louder-esp32-app`**:
   - In `network.rs` / `main.rs`:
     - Add `rtsp_socket` (`smoltcp::socket::tcp::Socket`) listening on TCP port 5000.
     - Add `rtp_socket` (`smoltcp::socket::udp::Socket`) listening on UDP port 6000.
     - Add `mdns_socket` (`smoltcp::socket::udp::Socket`) bound to UDP port 5353, join multicast group `224.0.0.251`.
     - In event loop:
       - On mDNS query for `_raop._tcp` or `_airplay._tcp`, send binary mDNS response built by `squeezelite_raop::mdns::MdnsBuilder`.
       - On RTSP TCP activity: feed request into `squeezelite_raop::rtsp::RtspServer`, send response.
       - On RTP UDP packet: decrypt and decode via `squeezelite_raop::rtp::RtpAudioReceiver`, push PCM into `StreamPipeline`.
       - On `SET_PARAMETER` volume: apply volume to `AmpManager` / TAS5805M.
2. **Stage-Only Build & Verification**:
   - `cargo test --workspace` -> 0 failures.
   - `python3 google_rust_audit.py ... --strict` -> 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings` -> 0 warnings.
   - Build release binary:
     `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
   - Verify `.stack >= 36,000` B and `.dram2_uninit <= 98,768` B.
   - Package staged image:
     `espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/louder-esp32-app scratch/louder-esp32-app-basement-ota-airplay1.bin`
   - Run `python3 verify_staged_ota_bin.py scratch/louder-esp32-app-basement-ota-airplay1.bin` -> `[STAGE-ONLY AUDIT PASS]`.
3. **1-Flash Deployment to `"BASEMENT"` (`172.22.1.15`)**:
   - Transition `"BASEMENT"` to Factory recovery (`POST http://172.22.1.15/recovery.json`, verify `recovery: 1`).
   - Stream staged binary via `POST http://172.22.1.15/flash.json` strictly ONCE.
4. **Certify Live Hardware Gates**:
   - Gate 1: Boot lease past 45s (`uptime_ms > 46,000`), `recovery: 0`, `rust_engine: true`, `otadata_prearmed_factory: true`, PSRAM intact.
   - Gate 2: TAS5805M volume and Web UI HTTP 200.
   - Gate 3: Live mDNS verification: query `172.22.1.15:5353` / scan for `_raop._tcp` and `_airplay._tcp` on LAN; confirm `"BASEMENT"` is visible as an AirPlay speaker.
   - Gate 4: Direct AirPlay audio playback from an AirPlay sender on the LAN to `172.22.1.15:5000`: stream ALAC audio, verify `frames_played` advancing, `tas_pll_locked: true`, `tas_automuted: false`, `i2c_nacks: 0`.
   - Gate 5: Dynamic reversion to SlimProto/MP3 when AirPlay disconnects.
   - Gate 6: Bluetooth A2DP coexistence and LMS sibling bridges intact.
5. **Snapshot Certified Baseline**:
   - Copy `rust/` to `scratch/baseline_airplay1_<hash>/rust`.
   - Copy `louder-esp32-app-basement-ota-airplay1.bin` to `baseline_airplay1_<hash>/`.

## 4. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `.agents/knowledge/subagent_summaries/20261029_wo211_live_airplay1_cert.md`.
- Confirm 0 background tasks before handoff.

---

## 5. Fulfillment Summary

- **Execution Date**: 2026-10-29
- **Certifier**: `Rust AirPlay Integration & Extension Certifier`
- **Binary Hash**: SHA-256 `59de037a38fca4699ea15d791e726c3c226777137b9fee03557accdd45f6e3a0` (`1,677,376` bytes, `62.43%` of `ota_0`)
- **Final Status**: `[EXECUTED & VERIFIED]`

### Summary of Live Hardware Gate Results:
1. **Stage-Only Verification (`[EXECUTED & VERIFIED]`)**:
   - `cargo test --workspace`: 220/220 tests pass (`exit code 0`).
   - `google_rust_audit.py --strict`: 0 errors, 0 warnings.
   - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings (`exit code 0`).
   - Xtensa release ELF memory: `.stack = 36,160` B (`>= 36,000` B target), `.dram2_uninit = 98,456` B (`<= 98,768` B target).
   - Staged binary verified by `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]`.
2. **1-Flash Circuit Breaker Deployment (`[EXECUTED & VERIFIED]`)**:
   - Flashed staged binary via `POST http://172.22.1.15/flash.json` strictly ONCE after transition to Factory recovery (`POST http://172.22.1.15/recovery.json`).
3. **Gate 1: Boot Lease & Anti-Brick Verification (`[EXECUTED & VERIFIED]`)**:
   - Uptime exceeded 45s lease (`uptime_ms > 465,000 ms`), `recovery: 0`, `rust_engine: true`, `otadata_prearmed_factory: true`, PSRAM intact (`3,438,812` B free, `755,492` B used), `core1_active: true`, `tas_pll_locked: true`, `i2c_nacks: 0`, `bt_enabled: true`.
4. **Gate 2: TAS5805M Hardware Volume & Web UI (`[EXECUTED & VERIFIED]`)**:
   - `cmd.json?act=vol55` confirmed `vol_pct: 55`, `tas_automuted: false`.
   - Web UI `GET /` returned `HTTP 200 OK` (3,928 bytes).
5. **Gate 3: Live mDNS AirPlay Discovery (`[EXECUTED & VERIFIED]`)**:
   - Live UDP 5353 mDNS query returned 537-byte DNS response with `_raop._tcp`, `_airplay._tcp`, `BASEMENT.local`, and TXT records (`txtvers=1`, `ch=2`, `sr=44100`, `ss=16`, `tp=UDP`, `am=airesp32`).
6. **Gate 4: Direct AirPlay ALAC Audio Streaming (`[EXECUTED & VERIFIED]`)**:
   - Executed `scratch/stream_airplay_alac.py`:
     - RTSP `OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`, `SET_PARAMETER` handshakes passed with `200 OK`.
     - Streamed 250 RTP ALAC packets over UDP port 6000 at ~8ms pacing.
     - Live read-back of `status.json` confirmed `"codec": "AirPlay"`, `frames_played` advanced by `+135,488` frames, `tas_pll_locked: true`, `tas_automuted: false`, `i2c_nacks: 0`.
     - RTSP `TEARDOWN` succeeded.
7. **Gate 5: Dynamic Reversion to MP3/LMS (`[EXECUTED & VERIFIED]`)**:
   - Following `TEARDOWN`, `codec` immediately reverted to `"MP3"`.
   - Resumed playback on LMS `10.0.0.10:9000`; `frames_played` monotonically advanced to `1,369,600+` frames with `codec=MP3`, `amp_mode=Play`, `pll=True`, `automuted=False`, `i2c_nacks=0`.
8. **Gate 6: Bluetooth Coexistence & LMS Bridges (`[EXECUTED & VERIFIED]`)**:
   - `sudo l2ping -c 3 CA:2E:18:BE:B8:35` confirmed `0% loss` (3 sent, 3 received, ~40ms RTT).
   - `sdptool browse CA:2E:18:BE:B8:35` confirmed `Audio Sink (0x110b)`, `L2CAP PSM 25`, `AVDTP 0x0103`.
   - All 16 LMS players and sibling bridges (Cast, SqueezeESP32) remain active and healthy on `10.0.0.10:9000`.
9. **Baseline Snapshot (`[EXECUTED & VERIFIED]`)**:
   - Preserved certified baseline at `/home/console-admin/Github/squeezelite-esp32/scratch/baseline_airplay1_59de037a/`.
10. **Background Tasks Barrier (`[EXECUTED & VERIFIED]`)**:
    - `manage_task(list)` confirmed 0 active background tasks.
