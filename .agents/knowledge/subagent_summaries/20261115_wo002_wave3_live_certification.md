# Subagent Summary: WO-2026-1115-002 Wave 3 (Invariant 49 & `HW1` Live Audio/Network Certification)

- **Work Order**: `WO-2026-1115-002`
- **Role**: `Extension Certifier - Invariant 49 and HW1 Benchmarks`
- **Timestamp**: `2026-11-15T09:35:00-05:00`
- **Status**: `[BLOCKED / ERROR]` (MP3 decode/I2C/RTT/LMS-restore verified; MP3 `fps` window overlapped startup buffering, local FLAC HTTP delivery blocked by host `ufw` policy on `10.0.0.90`, and post-restore `vol_pct` reported `65` with `tas_automuted: true`)

## 1. Files Created, Modified, and Deleted (Absolute Paths)

### Created
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave3_live_certification.md` (this Rule 17 summary)

### Modified
- None (`src/*`, `Cargo.toml`, `ufw` firewall rules, and repository files were not modified).

### Temporary & Deleted
- `/home/console-admin/.gemini/antigravity/brain/48abecab-97fb-4a4a-a643-562aeab51ad7/scratch/run_wave3.py` (orchestration script; deleted after execution)
- `/home/console-admin/.gemini/antigravity/brain/48abecab-97fb-4a4a-a643-562aeab51ad7/scratch/hw1_test_5s.wav` (intermediate 5.000s WAV; deleted immediately after FLAC conversion)
- `/home/console-admin/.gemini/antigravity/brain/48abecab-97fb-4a4a-a643-562aeab51ad7/scratch/wave3_results.json` (raw telemetry JSON; deleted after inspection)
- `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/scratch/hw1_test_5s.flac` (deterministic 5.000s 44100 Hz 16-bit stereo FLAC, 151,454 B; deleted after server shutdown)

## 2. Rationale & Raw Measurements (`--interface eno1`, `src 10.0.0.90`)

### Pre-Flight Routing & Host Firewall Observations
- `ip route get 10.0.0.50`: `10.0.0.50 dev eno1 src 10.0.0.90`
- `ip route get 172.22.1.15`: `172.22.1.15 via 10.0.0.1 dev eno1 src 10.0.0.90`
- `ufw status` / `iptables -S INPUT` / `ufw-user-input`: `ufw` is `active` (`-P INPUT DROP`) and permits inbound TCP on `eno1` only for ports `22`, `3389`, and `80` (`10.0.0.90:80` and `10.0.0.90:8444` are bound by Docker container `kiosk-ingress`). No host firewall or container state was modified.

### Step 1 — Initial State, Silent Lockout (`vol_pct == 0`), & Ibiza Global Radio MP3
- **Initial Device (`GET http://172.22.1.15/status.json`)**: `version="I2S-4MFlash-Rust"`, `recovery=0`, `rust_engine=true`, `uptime_ms=267390`, `frames_played=0`, `vol_pct=0`, `tas_automuted=true`, `tas_pll_locked=true`, `i2c_acks=426`, `i2c_nacks=0`, `decode_us_max=0`, `decode_us_first=0`, `codec="PCM"`.
- **Initial LMS (`10.0.0.50:9000/jsonrpc.js`, `c8:2e:18:be:b8:34`)**: `mode="stop"`, `mixer volume=50`, `player_connected=1`, `playlist_tracks=1`, `playlist_loop[0].url="https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"`.
- **Mixer Volume `0` Lockout**: `["mixer","volume","0"]` confirmed `mixer volume=0` in LMS and `vol_pct=0` in `/status.json` (`[EXECUTED & VERIFIED]`).
- **MP3 Play Start (`["play"]`, pre-index-skip `mp3_confirm`)**: `uptime_ms=275015`, `codec="MP3"`, `vol_pct=0`, `frames_played=46080`, `i2c_acks=439`, `i2c_nacks=0`, `decode_us_max=11458`, `decode_us_first=865` (`[EXECUTED & VERIFIED]`).
- **Playlist Index Skip (`["playlist","index","0"]`) & 16.072s Sample Window (`s1 -> s2`)**:
  - `s1` (`+4.5s` post-skip, `uptime_ms=279668`): `codec="MP3"`, `vol_pct=0`, `frames_played=0`, `i2c_acks=446`, `i2c_nacks=0`, `decode_us_max=11458`.
  - `s2` (`+16.0s` later, `uptime_ms=295740`): `codec="MP3"`, `vol_pct=0`, `frames_played=662656`, `i2c_acks=470`, `i2c_nacks=0`, `decode_us_max=12384`, LMS `mode="play"`, `mixer volume=0`.
  - **Computed Rates (`dt_s = 16.072s`)**:
    - `fps = 662656 / 16.072 = 41230.463 fps` (outside `44080.0..44120.0` because `s1` was sampled while `frames_played == 0` during ~1.04s of remaining HTTPS stream re-buffer after `playlist index 0`; `662656 / 44100 = 15.026s` of audio rendered in `16.072s`).
    - `acks_per_s = (470 - 446) / 16.072 = 1.4933 ACKs/s <= 3.05` (PASS)
    - `i2c_nacks = 0` (`i2c_nack_by_op={"read":0,"write":0,"write_read":0}`) (PASS)
    - `decode_us_max = 12384 us` (`0 < 12384 < 46000 us`) (PASS)

### Step 2 — Deterministic 5.000s FLAC Playback (`[BLOCKED / ERROR]`)
- **Artifact Generated & Verified**: `hw1_test_5s.flac` (`151,454` bytes). Both FLAC `STREAMINFO` header parsing and `/usr/bin/ffprobe` confirmed `codec_name="flac"`, `sample_rate=44100`, `channels=2`, `bits_per_raw_sample=16`, `duration_ts=220500` frames (`5.000s` exact).
- **Pre-FLAC Device Baseline (`uptime_ms=297781`)**: `frames_played=0`, `vol_pct=0`, `i2c_acks=474`, `i2c_nacks=0`, `decode_us_max=12384`.
- **Playback Attempt**: Served at `http://10.0.0.90:45531/hw1_test_5s.flac` via `ThreadingHTTPServer` bound to `10.0.0.90` and triggered via `["playlist","play","http://10.0.0.90:45531/hw1_test_5s.flac"]`.
- **Outcome (`14.6s` observation window)**: `requests_log=[]` (`0` HTTP requests reached `10.0.0.90:45531` due to host `ufw` `-P INPUT DROP` policy), `mid_flac_dev=null`, `codec` stayed `"MP3"`, `frames_played` remained `0` (`frames_delta=0`), `post_flac_dev` (`uptime_ms=312518`) reported `i2c_nacks=0`, `decode_us_max=12384`. Server closed cleanly and scratch FLAC removed.

### Step 3 — Bounded `eno1` Ping Probe (`[EXECUTED & VERIFIED]`)
- Command: `/usr/bin/ping -I eno1 -c 20 -i 0.1 172.22.1.15` (`rc=0`)
- Result: `20 packets transmitted, 20 received, 0% packet loss, time 1912ms`
- RTT: `min/avg/max/mdev = 2.539/8.785/27.055/6.688 ms`

### Step 4 — LMS & Device State Restoration
- Sent `["stop"]`, `["playlist","clear"]`, `["playlist","add","https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"]`, verified LMS `mode="stop"`, then sent `["mixer","volume","50"]`.
- Final LMS status: `mode="stop"`, `mixer volume=50`, `playlist_tracks=1`, `playlist_loop[0].url="https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"`.
- Final Device `/status.json` (`uptime_ms=313953`): `recovery=0`, `i2c_nacks=0`, `tas_automuted=true`, `tas_pll_locked=true`, `frames_played=0`, `decode_us_max=12384`, **`vol_pct=65`** (LMS volume `50` pushed `audg` gain mapping to `vol_pct=65`, while `tas_automuted=true` remains active in `stop`).
- `/messages.json` (`uptime_ms=315908`): `psram_free=1441212 psram_used=2753092 core1_active=true core1_loops=1061858 decode_us_max=12384`.

## 3. Untested Assumptions & Risks
- **FLAC Tail (`finish_input`) & Steady-State MP3 `fps` Unverified**: End-of-track FLAC `frames_played >= 220,400` and post-buffer steady-state MP3 `fps` (`44080..44120`) could not be certified within this Wave 3 run because (a) `ufw` on `10.0.0.90` blocks inbound HTTP on unprivileged ports while `kiosk-ingress` occupies `10.0.0.90:80`, and (b) sampling MP3 `s1` at `+4.5s` after `playlist index 0` caught `frames_played=0` prior to stream restart (~`5.5s` total stream setup latency).
- **Restore `vol_pct` Threshold Clarification**: Restoring LMS `mixer volume 50` causes the device to report `vol_pct=65` (with `tas_automuted=true` keeping the amplifier muted) rather than `vol_pct=0`.
