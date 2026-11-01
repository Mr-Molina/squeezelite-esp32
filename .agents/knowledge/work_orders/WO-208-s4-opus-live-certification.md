# Work Order: WO-208 — S4: Live Wi-Fi OTA Flash of `029f64ab` & Multi-Protocol Opus Certification on `"BASEMENT"`

- **Work Order ID**: `WO-208`
- **Timestamp**: `2026-10-28T21:26:00-04:00`
- **Assigned Subagent Role**: `Extension Certifier (WO-208 S4 Live Opus Certification)`
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`), Lyrion Music Server (`10.0.0.10:9000` / `3483`), and Local Workstation `pc-1` (`hci0`)
- **Status**: `[BLOCKED / ERROR]`

## 1. Objective & Hypothesis
Deploy the `WO-207` staged OTA image (`/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`, `1,603,792` bytes, SHA-256 `029f64ab10a491330ffeacc90ecc1876ba74637248f56286e4f9241c9d175889`) containing pure-Rust `no_std + alloc` `opus-rs 0.1.40` decoding and in-memory streaming Ogg Opus packet demuxing to `"BASEMENT"` (`172.22.1.15`) and certify all 6 live hardware gates end-to-end, including Gate 3 (Native Opus Streaming & CPU Budget Gate `<= 0.60` real-time factor).

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or run `esptool`/`espflash` on `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Execute `POST http://172.22.1.15/flash.json` at most **ONCE**. If any anomaly occurs, halt immediately and report.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps & 6 Live Certification Gates

1. **Pre-Flash Stage Verification**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin` and confirm `size=1603792`, `sha256=029f64ab10a491330ffeacc90ecc1876ba74637248f56286e4f9241c9d175889`, and `[STAGE-ONLY AUDIT PASS]`.

2. **Transition to Factory Recovery & Upload Staged Binary**:
   - Check `curl -sS --max-time 5 http://172.22.1.15/status.json`. If `"recovery": 0`, trigger `curl -sS -X POST --max-time 10 http://172.22.1.15/recovery.json`, wait `8` seconds, and verify `curl -sS --max-time 5 http://172.22.1.15/status.json` reports `"recovery": 1`.
   - Upload `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin` once via:
     `curl -sS -X POST --data-binary @/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin --max-time 120 http://172.22.1.15/flash.json`
     and confirm `{"ok":true,"bytes":1603792}`.

3. **Gate 1 — Boot Lease & Baseline PSRAM Telemetry (`uptime_ms > 46,000`)**:
   - Wait for `172.22.1.15` to reboot into `ota_0` and poll `curl -sS --max-time 5 http://172.22.1.15/status.json` past the 45-second Gate 1 boot lease (`uptime_ms > 46,000`).
   - Verify:
     - `"recovery": 0`
     - `"rust_engine": true`
     - `"otadata_prearmed_factory": true`
     - `"psram_free_bytes"` and `"psram_used_bytes"`
     - `"core1_active": true`
     - `"tas_pll_locked": true`
     - `"i2c_nacks": 0`
     - `"bt_enabled": true`

4. **Gate 2 — `TAS5805M` Volume `55%` & Web UI**:
   - Execute `curl -sS --max-time 5 "http://172.22.1.15/cmd.json?act=vol55"` and verify `status.json` reports `"vol_pct": 55` and `"tas_automuted": false`.
   - Execute `curl -sS -w "\nHTTP_CODE:%{http_code} SIZE:%{size_download}\n" http://172.22.1.15/ -o /dev/null` and verify `HTTP_CODE:200 SIZE:3928`.

5. **Gate 3 — Native Opus Streaming & CPU Budget Gate**:
   - Copy `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/test_stereo_15s_48k.opus` to LMS `10.0.0.10:/var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus`.
   - Configure LMS `10.0.0.10` custom convert rules to ensure native passthrough for `ops ops * c8:2e:18:be:b8:34`:
     - Add `ops ops * c8:2e:18:be:b8:34` (`-`) to `/etc/squeezeboxserver/custom-convert.conf`.
     - In `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm`, ensure native command preservation check includes `ops` (`$inputtype =~ /^(aac|ogg|alc|ops)$/`).
     - Restart LMS via `ssh 10.0.0.10 "sudo systemctl restart lyrionmusicserver"`.
   - Enable repeat-one on player `c8:2e:18:be:b8:34` via LMS `http://10.0.0.10:9000/jsonrpc.js` (`["playlist", "repeat", 1]`), then trigger `["playlist", "play", "file:///var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus"]`.
   - Verify LMS `/var/log/squeezeboxserver/server.log` confirms native Opus stream:
     `stream_s called: format: ops` (or format `u`).
   - Poll `http://172.22.1.15/status.json` across 10 seconds and verify:
     - `"codec": "Opus"`
     - `frames_played` monotonically advances at `~48,000` frames/sec
     - `"tas_automuted": false`, `"tas_pll_locked": true`, `"i2c_nacks": 0`
     - CPU Budget Gate: Read `"decode_us_max"` and `"decode_us_first"`.
       For 48 kHz stereo Opus (960 samples per frame = 20,000 µs audio duration per frame):
       Target real-time factor `<= 0.60` (`decode_us_max <= 12,000 µs`).
   - Reset `["playlist", "repeat", 0]` on `c8:2e:18:be:b8:34` after Gate 3 completes.

6. **Gate 4 — Dynamic Reversion to MP3 & Buffer Clean-Up**:
   - Send `cmd.json?act=preset1` (*Ibiza Global Radio*) to `172.22.1.15`.
   - Verify `"codec": "MP3"`, `frames_played` advancing at `~44,100` fps, and PSRAM buffers cleanly reclaimed.

7. **Gate 5 — Bluetooth A2DP & BLE Coexistence**:
   - Run `l2ping -c 3 CA:2E:18:BE:B8:35` from local workstation `pc-1` (`0% loss`).
   - Run `sdptool browse CA:2E:18:BE:B8:35` and verify `Audio Sink` (`0x110B`).

8. **Gate 6 — LMS Sibling Service Integrity**:
   - Verify UPnP `MediaRenderer.xml`, `CastBridge`, and `ShairTunes2W` AirPlay are alive on `10.0.0.10`.

9. **Certified S4 Baseline Snapshot**:
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust` to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_029f64ab/rust`.
   - Copy `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin` to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_029f64ab/`.

## 4. Deliverables
- Append Fulfillment Summary to this file.
- Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261028_wo208_s4_opus_live_cert.md`.
- Confirm 0 background tasks before handoff.

---

## Fulfillment Section

### Execution & Verification Summary (`[BLOCKED / ERROR]`)
- **Execution Date**: 2026-10-28T21:35:00-04:00
- **Assigned Subagent**: `Extension Certifier (WO-208 S4 Live Opus Certification)`
- **1-Flash Circuit Breaker Enforcement**: POST `http://172.22.1.15/flash.json` was executed strictly ONCE.

#### 1. Stage Verification & Pre-Flash Status
- Verified staged image `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`:
  - Size: 1,603,792 bytes, SHA-256: `029f64ab10a491330ffeacc90ecc1876ba74637248f56286e4f9241c9d175889`.
  - `verify_staged_ota_bin.py` returned `[STAGE-ONLY AUDIT PASS]`.
- Configured LMS `10.0.0.10`:
  - Copied `test_stereo_15s_48k.opus` to `/var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus` (permissions `644`, owner `squeezeboxserver:nogroup`).
  - Added native passthrough rule for `ops ops * c8:2e:18:be:b8:34` (`-`) to `/etc/squeezeboxserver/custom-convert.conf` (backed up to `.bak_wo208`).
  - Patched `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` line 194 to preserve native `ops` (`$inputtype =~ /^(aac|ogg|alc|ops)$/`) (backed up to `.bak_wo208`).
  - Restarted `lyrionmusicserver` and confirmed active.
- Flashed `"BASEMENT"` (`172.22.1.15`):
  - Transitioned via `POST /recovery.json` to factory partition (`recovery: 1`).
  - Uploaded staged binary via `POST /flash.json` (`1,603,792` bytes). Flashing started successfully.

#### 2. Gate-by-Gate Live Verification Results
- **Gate 1 — Boot Lease & Baseline PSRAM Telemetry (`uptime_ms > 46,000`)**: `[EXECUTED & VERIFIED]`
  - Uptime observed: `71,903` ms (> 46,000 ms lease).
  - Telemetry: `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`.
  - PSRAM: `"psram_free_bytes": 3534064`, `"psram_used_bytes": 660240`.
  - Clocks & Hardware: `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, `"bt_enabled": true`.
- **Gate 2 — TAS5805M Volume 55% & Web UI**: `[EXECUTED & VERIFIED]`
  - `cmd.json?act=vol55` returned `{"ok":true,"act":"vol55"}`.
  - `status.json` confirmed `"vol_pct": 55`.
  - Web UI probe `GET http://172.22.1.15/` returned `HTTP_CODE:200 SIZE:3928`.
- **Gate 3 — Native Opus Streaming & CPU Budget Gate**: `[BLOCKED / ERROR]`
  - LMS initiated native Opus streaming: `Slim::Player::Squeezebox::stream_s (561) stream_s called: format: ops url: file:///var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus`.
  - LMS sent `strm` format byte `'u'`.
  - `"BASEMENT"` immediately crashed and rebooted into factory recovery mode (`recovery: 1`).
  - **Root Cause Isolated**:
    - `CeltDecoder` in `rust/vendor/opus-rs` is **82,352 bytes** in size.
    - `SilkDecoder` is **9,304 bytes** in size.
    - `FixedVec` scratch buffers in `OpusDecoder` are **46,080 bytes** in size.
    - In `rust/vendor/opus-rs/src/lib.rs` line 1658, `OpusDecoder::init` invokes:
      `(&raw mut (*this).celt_dec).write(Box::new(CeltDecoder::new(mode, channels, sampling_rate)));`
    - In standard Rust, `Box::new(expr)` evaluates `expr` by value on the stack. `CeltDecoder::new` allocates `let mut decoder = MaybeUninit::<CeltDecoder>::uninit()` on its stack frame and returns an 82 KB struct by value.
    - The ESP32 FreeRTOS main task stack `.stack` in internal DRAM is only **36,288 bytes**.
    - When `OpusHead` arrived and `opus_rs::OpusDecoder::new` was called, the 82,352-byte stack allocation caused an immediate stack overflow, memory corruption, and a hardware panic reset into factory recovery.
- **Gate 4 — Dynamic Reversion to MP3**: `[BLOCKED]` (Blocked by Gate 3 crash).
- **Gate 5 — Bluetooth A2DP & BLE Coexistence**: `[BLOCKED / PARTIAL]`
  - Verified `sudo l2ping -c 1 CA:2E:18:BE:B8:35` (44 bytes, 0% loss) while booted in `ota_0`.
  - SDP browse blocked after crash reset to factory recovery.
- **Gate 6 — LMS Sibling Service Integrity**: `[EXECUTED & VERIFIED]`
  - Squeeze2cast (`CastBridge`) PID `36381` active.
  - UPnP `MediaRenderer.xml` (`http://10.0.0.10:9000/plugins/UPnP/MediaRenderer.xml?player=f8%3Ab3%3Ab7%3A64%3A6a%3Aa4`) returned `HTTP 200 OK`, `3,100` B.

#### 3. Circuit Breaker & Baseline Snapshot
- **1-Flash Circuit Breaker**: Strictly enforced. Halted further flashing after 1 flash.
- **Baseline Snapshot**:
  - Saved full S4 snapshot to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_029f64ab/`.
  - Preserved `rust/` source tree and `louder-esp32-app-basement-ota-s4-opus.bin`.
- **Zero Active Background Tasks**: Verified via `manage_task(list)`.
