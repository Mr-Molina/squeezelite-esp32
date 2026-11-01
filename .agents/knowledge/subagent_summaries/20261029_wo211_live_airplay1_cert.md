# Subagent Summary: WO-211 Pure-Rust On-Device AirPlay 1 (RAOP) Integration & Live Hardware Certification

- **Date**: 2026-10-29
- **Work Order**: `WO-211`
- **Role**: `Rust AirPlay Integration & Extension Certifier`
- **Status**: `[EXECUTED & VERIFIED]`
- **Binary Hash**: SHA-256 `59de037a38fca4699ea15d791e726c3c226777137b9fee03557accdd45f6e3a0` (`1,677,376` bytes)

---

## 1. Summary of Changes

### A. Modified Files
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
   - Added features `["alloc", "multicast", "socket-udp"]` to `smoltcp` dependency.
   - Added `features = ["alloc"]` to `squeezelite-raop` dependency.
   - Backup: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml.bak_wo211`
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/rtsp.rs`
   - Added `set_ip_address(&mut self, ip: [u8; 4])` to `RtspServer` for dynamic network configuration.
   - Backup: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop/src/rtsp.rs.bak_wo211`
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
   - Implemented `RaopStreamClient` client wrapper for automated RTSP/RTP host testing.
   - Added `test_raop_stream_client_lifecycle` unit test.
   - Backup: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs.bak_wo211`
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
   - Exported `RaopStreamClient`.
   - Backup: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs.bak_wo211`
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
   - Allocated mDNS (UDP 5353, multicast `224.0.0.251`), RTSP (TCP 5000), and RTP (UDP 6000) socket buffers in external PSRAM via `alloc::vec!`.
   - Wired `RtspServer` and `RtpAudioReceiver` into smoltcp event loop.
   - Handled RTSP `OPTIONS`, `ANNOUNCE`, `SETUP`, `RECORD`, `SET_PARAMETER` (volume), `FLUSH`, `TEARDOWN`.
   - Ingested decoded ALAC 16-bit 44.1 kHz stereo PCM into `StreamPipeline` and reported `"codec": "AirPlay"` during active AirPlay sessions.
   - Implemented dynamic reversion to MP3/LMS when AirPlay session ends.
   - Backup: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs.bak_wo211`

### B. Created Verification Scripts & Artifacts
1. `/home/console-admin/Github/squeezelite-esp32/scratch/stream_airplay_alac.py`
   - Automated end-to-end AirPlay 1 certification runner implementing RTSP session lifecycle and UDP RTP ALAC packet streaming.
2. `/home/console-admin/Github/squeezelite-esp32/scratch/baseline_airplay1_59de037a/`
   - Archived certified baseline binary and full `rust/` source snapshot.

---

## 2. Live Hardware Certification Evidence (`172.22.1.15`)

1. **Gate 1 (Boot Lease & Memory Safety)**:
   - Device booted `59de037a` from `ota_0` and remained continuously online past 45s lease (`uptime_ms > 465,000`).
   - `recovery: 0`, `rust_engine: true`, `otadata_prearmed_factory: true`.
   - PSRAM intact: `3,438,812` bytes free, `755,492` bytes used (zero leaks across multiple codec transitions).
   - `core1_active: true`, `tas_pll_locked: true`, `i2c_nacks: 0`, `bt_enabled: true`.
2. **Gate 2 (TAS5805M & Web UI)**:
   - Volume control via `cmd.json?act=vol55` set `vol_pct: 55` without I2C errors (`tas_automuted: false`).
   - Web UI `GET /` served `HTTP 200 OK` (3,928 bytes).
3. **Gate 3 (mDNS AirPlay Discovery)**:
   - Query to UDP 5353 returned 537-byte DNS response with `_raop._tcp`, `_airplay._tcp`, `BASEMENT.local`, and TXT record with AirPlay capabilities (`ch=2`, `sr=44100`, `ss=16`, `tp=UDP`, `am=airesp32`).
4. **Gate 4 (Direct AirPlay ALAC Audio Streaming)**:
   - Full RTSP handshake completed with 200 OK responses.
   - Streamed 250 RTP ALAC packets over UDP port 6000 at ~8ms intervals.
   - Monitored `status.json` live: `"codec": "AirPlay"`, `frames_played` monotonically advanced by `+135,488` frames, `tas_pll_locked: true`, `tas_automuted: false`, `i2c_nacks: 0`.
   - Sent RTSP `TEARDOWN`.
5. **Gate 5 (Dynamic Reversion to MP3/LMS)**:
   - Post-TEARDOWN, `codec` cleanly reverted to `"MP3"`.
   - Resumed playback on LMS `10.0.0.10:9000`; `frames_played` advanced monotonically to `1,369,600+` frames with `amp_mode: Play`, `tas_pll_locked: true`, `i2c_nacks: 0`.
6. **Gate 6 (Bluetooth Coexistence & Sibling Bridges)**:
   - `sudo l2ping -c 3 CA:2E:18:BE:B8:35` confirmed `0% loss` (3 sent, 3 received, ~40ms RTT).
   - `sdptool browse CA:2E:18:BE:B8:35` confirmed `Audio Sink (0x110b)` and `AVDTP 0x0103`.
   - All 16 LMS players and sibling bridges (Cast, SqueezeESP32) remain active on `10.0.0.10:9000`.
7. **Task Concurrency Barrier**:
   - `manage_task(list)` confirmed 0 active background tasks.

---

## 3. Invariants & Safety Compliance
- **Invariant 47 / single-target-troubleshooting.md**: `/dev/ttyUSB0` was untouched and unopened.
- **Invariant 42**: `POST /reboot_ota.json` was never called.
- **1-Flash Circuit Breaker**: Flashed staged binary to `172.22.1.15` strictly ONCE.
- **Rule 21**: 0 background tasks running at handoff.
