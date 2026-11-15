# Work Order: WO-2026-1115-002 — Build, Stage-Validate, OTA-Deploy, & Certify Pure-Rust `MK5-Rust` (`579ffcde`) on `"BASEMENT"` (`172.22.1.15`)

- **Work Order ID**: `WO-2026-1115-002`
- **Timestamp**: `2026-11-15T09:22:00-05:00`
- **Assigned Subagent Roles**: `WO Registration and Stage-Only Builder`, `Extension Certifier - OTA Flash and Boot Gate`, `Extension Certifier - Invariant 49 and HW1 Benchmarks`, `Build Cleanup and Docs/State Updater`, `Dual Session Sync, Local Commit, and Preflight Verifier`
- **Target Host / Component**:
  - Live target `"BASEMENT"` (`http://172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`) via interface `eno1` (`src 10.0.0.90`)
  - Lyrion Music Server at `http://10.0.0.50:9000/jsonrpc.js` (reported live by `172.22.1.15/status.json`)
  - Repository `/home/console-admin/Github/squeezelite-esp32` (branch `MK5-Rust`, code baseline `579ffcde`)
- **Status**: `[EXECUTED & VERIFIED]`

## 0. Binding Safety & Hardware Guardrails
1. **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD display board `ESP32-D0WDQ6-V3`, MAC `d4:d4:da:5a:e7:3c`).
2. **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker (Invariant 47(3))**: Stream the validated binary to `POST http://172.22.1.15/flash.json` **at most ONCE** after `POST /recovery.json` confirms `"recovery": 1` and `"version": "I2S-4MFlash-1681"`. If `"BASEMENT"` rolls back to `"recovery": 1` or fails to reach `"recovery": 0` within 45 seconds, **STOP IMMEDIATELY** and report `[BLOCKED / ERROR]` without modifying any source files (`src/*`, `Cargo.toml`).
4. **Network Interface Binding (Rule 55)**: Always pass `--interface eno1` to `curl`, `-I eno1` to `ping`, and bind any Python test HTTP server explicitly to `10.0.0.90`.
5. **Silent Speaker Guard**: Before starting LMS playback, confirm/set `["mixer","volume","0"]` for player `c8:2e:18:be:b8:34` on `http://10.0.0.50:9000/jsonrpc.js` and verify `"vol_pct": 0` in `/status.json` during playback. At completion, send `["stop"]`, verify `"mode": "stop"`, restore the `Ibiza Global Radio` playlist entry (`https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3`) without playing, and only then restore `["mixer","volume","50"]` while confirming `"vol_pct": 0` and `"tas_automuted": true`.
6. **Artifact Backup Directory**: `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/` (unique `wo_20261115_002_<name>.bak` filenames; never overwrite existing backups).

## 1. Wave 1 Commands (`--stage-only` Build & Pre-Flight Image Validation)
1. Back up `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md` to `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/wo_20261115_002_INDEX.md.bak` and register `WO-2026-1115-002` as `[IN PROGRESS]`.
2. Build release ELF and save `.bin` image:
   ```bash
   source /home/console-admin/export-esp.sh
   cd /home/console-admin/Github/squeezelite-esp32/rust
   cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
   /home/console-admin/.cargo/bin/espflash save-image --chip esp32 \
     /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app \
     /home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/louder-esp32-app-579ffcde-ota.bin
   ```
3. Inspect ELF section sizes with `/home/console-admin/.rustup/toolchains/esp/xtensa-esp-elf/esp-16.2.0_20260914/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`:
   - Assert IRAM (`iram0.vectors + iram0.text`) `<= 131,072` B
   - Assert `.dram2_uninit <= 98,768` B
   - Assert `.stack >= 28,000` B (expected `28,528` B)
4. Validate staged `.bin` using a temporary script in `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/scratch/verify_staged_ota_bin.py` (with `encoding='utf-8'`, deleted after running):
   - Assert `288 <= size <= 2,686,976` (`0x290000` `ota_0` partition limit at `0x160000`) and `size % 4 == 0`
   - Assert byte `0x00 == 0xE9`
   - Assert little-endian `u32` at `0x20..0x24 == 0xABCD5432`
   - Assert UTF-8 C-string at `0x30..0x50 == "I2S-4MFlash-Rust"`
   - Assert UTF-8 C-string at `0x50..0x70 == "Squeezelite-ESP32"`
   - Compute `/usr/bin/sha256sum /home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/louder-esp32-app-579ffcde-ota.bin`.
5. Write Rule 17 summary `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave1_stage_only_build.md`.

## 2. Wave 2 Commands (Single-Shot OTA Flash & Gate 1/2 Verification)
1. Confirm `GET http://172.22.1.15/status.json` (`curl --interface eno1 -sS --max-time 5`) and verify `sha256sum` + byte size of `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/louder-esp32-app-579ffcde-ota.bin`.
2. Reboot to Factory recovery (`0x10000`):
   ```bash
   /usr/bin/curl --interface eno1 -sS --max-time 10 -X POST http://172.22.1.15/recovery.json
   ```
   Wait 6–10s and poll `GET http://172.22.1.15/status.json` (`--interface eno1`) up to 25s until `"recovery": 1` and `"version": "I2S-4MFlash-1681"`.
3. Stream validated binary ONCE to `ota_0` (`0x160000`):
   ```bash
   /usr/bin/curl --interface eno1 -sS --max-time 90 -X POST \
     -H 'Content-Type: application/octet-stream' \
     --data-binary @/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/louder-esp32-app-579ffcde-ota.bin \
     http://172.22.1.15/flash.json
   ```
4. Wait ~20s for boot into `ota_0`, sample `GET http://172.22.1.15/status.json` once before `uptime_ms == 45000`, and sample again once `uptime_ms > 50000` ms (past 46s boot lease):
   - Assert `"recovery": 0` (HALT immediately if `"recovery": 1`!)
   - Assert `"rust_engine": true`, `"version": "I2S-4MFlash-Rust"`
   - Assert `psram_free_bytes + psram_used_bytes == 4194304`
   - Assert `"core1_active": true`, `"core1_loops" > 0`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, `"disconnect_count": 0`.
5. Verify Gate 2 HTTP endpoints (`--interface eno1`): `GET /`, `/config.json`, `/messages.json`, `/commands.json`, and run `curl --interface eno1 -sS -o /dev/null -w 'http_code=%{http_code} ttfb=%{time_starttransfer}s total=%{time_total}s\n' 'http://172.22.1.15/?action=getInfo'` twice (first primes `ZEROCONF_DH_CACHE`, second asserts `ttfb < 0.100s`).
6. Write Rule 17 summary `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave2_ota_and_boot_gates.md`.

## 3. Wave 3 & Wave 3R Commands (Invariant 49 & `HW1` Live Audio/Network Certification, Muted `vol_pct == 0`)

### Wave 3 Initial Run (`48abecab`, Partial PASS + Re-Plan Triggers)
- Passed: LMS volume lockout (`vol_pct: 0`), MP3 `decode_us_first=865`, `decode_us_max=12384` us, `i2c_acks/s=1.4933 <= 3.05`, `i2c_nacks=0`, `ping -I eno1 -c 20 -i 0.1 172.22.1.15` (`0%` loss, `min/avg/max/mdev = 2.539/8.785/27.055/6.688 ms`), clean LMS restoration (`tas_automuted: true`).
- Blocked/Re-planned: UFW (`-P INPUT DROP`) blocked HTTP :45531 on `10.0.0.90`, and MP3 `s1` overlapped initial pre-buffering (`frames_played=0`).

### Wave 3R Remediation Run (`run_wave3r_cert.py` -> Native `10.0.0.50` FLAC + Steady-State MP3)
Create and run ONE standalone UTF-8 Python script `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/scratch/run_wave3r_cert.py` (`encoding='utf-8'`, binding `http.client.HTTPConnection(..., source_address=('10.0.0.90', 0))`, deleted immediately after completion):
1. **Safety Lockout**: Send `["stop"]` and `["mixer","volume","0"]` to `http://10.0.0.50:9000/jsonrpc.js` for `c8:2e:18:be:b8:34`; confirm LMS `"mixer volume" == 0` and `http://172.22.1.15/status.json` `"vol_pct" == 0`.
2. **Native 45.000s (`1,984,500`-frame) FLAC Steady-State, PSRAM TCP Windows, & EOF Flush (`HW1` b & c + Invariant 49)**:
   - Trigger `["playlist","play","/var/lib/squeezeboxserver/cache/test_stereo_16bit.flac"]` on `10.0.0.50:9000/jsonrpc.js`.
   - Poll `http://172.22.1.15/status.json` every `0.5s` (up to `12s`) until `"codec" == "FLAC"` and `frames_played >= 88200` (`>= 2.0s` active playback, confirming `"vol_pct" == 0`). Capture `flac_s1`.
   - During active FLAC streaming, run `ssh -o BatchMode=yes -o ConnectTimeout=4 console-admin@10.0.0.50 "/usr/bin/ss -tnaip 'dst 172.22.1.15'; /usr/bin/pgrep -af 'lame|SqueezeDSP' || true"` to inspect `:3483` (`snd_wnd:8192`) and `:9000` (`snd_wnd:65535`/`65536`) and verify `0` transcoding processes.
   - Wait until `15.5s` after `flac_s1` and capture `flac_s2`: verify `flac_fps` in `44,080.0..44,120.0`, `flac_acks_per_s <= 3.05`, `i2c_nacks == 0`, and non-zero FLAC `decode_us_max < 46,000` us.
   - Continue polling every `0.5s` until `frames_played >= 1,880,000`, then tight-poll every `0.035s` (`35 ms`) until `frames_played` resets to `0` or stops advancing for `1.5s` (or LMS `mode == "stop"`, max 12s), recording `peak_flac_frames_played` and final FLAC `decode_us_max`. Assert `peak_flac_frames_played >= 1,981,500` (out of `1,984,500` total frames).
3. **Steady-State MP3 15.5s Window (`HW1` a & c + Invariant 49)**:
   - Confirm LMS `"mixer volume" == 0`, trigger `["playlist","play","https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"]`, poll every `0.5s` (up to `15s`) until `"codec" == "MP3"` and `frames_played >= 88200` (`"vol_pct" == 0`), capture `mp3_s1`, wait `15.5s`, capture `mp3_s2`: verify `mp3_fps` in `44,080.0..44,120.0`, `mp3_acks_per_s <= 3.05`, `i2c_nacks == 0`, and non-zero `decode_us_max < 46,000` us.
4. **Clean LMS Restoration & Summary**:
   - Send `["stop"]`, `["playlist","clear"]`, `["playlist","add","https://cdn-peer032.streaming-pro.com:8025/ibizaglobalradio.mp3"]`, verify LMS `"mode" == "stop"`, then send `["mixer","volume","50"]`. Confirm `http://172.22.1.15/status.json` shows `"recovery": 0`, `"i2c_nacks": 0`, `"tas_automuted": true`, `"frames_played": 0`.
   - Delete `run_wave3r_cert.py` (confirm `scratch/` empty) and write Rule 17 summary `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave3r_live_certification.md`.

## 4. Wave 4 Commands (Cache Cleanup, Docs & Ledgers Update, Local Commit, Preflight)
1. Run `source /home/console-admin/export-esp.sh && cargo clean --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
2. Back up to `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/wo_20261115_002_<name>.bak` and update `docs/mk5-rust-todo.md` (`HW1` `[x]` + open count `0`), `DEVICE_STATE.md`, `SYSTEM_STATE.md`, `DIAGNOSTIC_LOG.md`, this Work Order Fulfillment Section, and `INDEX.md`.
3. Back up and sync dual `sessions.md`, commit locally on `MK5-Rust` (do NOT `git push`), refresh `te repomap`, ensure `scratch/` is clean, and confirm `git status --ignored -sb` clean + `wakeup_bootstrap.py` + `goodbye_preflight.py` exit `0`.

## 5. Fulfillment Summary

- **Wave 1 (`WO Registration and Stage-Only Builder`, `[EXECUTED & VERIFIED]`)**: Registered `WO-2026-1115-002` in `INDEX.md`, compiled `louder-esp32-app` release ELF (`xtensa-esp32-none-elf`, `exit code 0` in `2m 02s`), packaged `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/louder-esp32-app-579ffcde-ota.bin` (`2,007,568` B, SHA-256 `954f842661487857dd95aa89eaeb1a4468a80228049b878340c314d3dfd92fa0`), verified AppDesc (`0xE9`, `0xABCD5432`, `"I2S-4MFlash-Rust"`, `"Squeezelite-ESP32"`), and confirmed ELF limits (IRAM `129,636 <= 131,072` B, `.dram2_uninit` `98,520 <= 98,768` B, `.stack` `28,528 >= 28,000` B). Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave1_stage_only_build.md`.
- **Wave 2 (`Extension Certifier - OTA Flash and Boot Gate`, `[EXECUTED & VERIFIED]`)**: Transitioned `172.22.1.15` (`--interface eno1`) to Factory recovery (`"recovery": 1`, `"I2S-4MFlash-1681"`), streamed validated binary ONCE to `POST /flash.json` (`HTTP 200`, `3.906468s`; zero `/reboot_ota.json`, zero `/dev/ttyUSB0`), and passed Gate 1 (`uptime_ms = 14,931` and `68,658 > 50,000`, `"recovery": 0`, `"rust_engine": true`, PSRAM `4,194,304` B, `core1_active: true`, `tas_pll_locked: true`, `i2c_nacks: 0`, `disconnect_count: 0`, `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"`) and Gate 2 (`/`, `/config.json`, `/messages.json`, `/commands.json`, cached `/?action=getInfo` `ttfb = 0.014481s < 0.100s` after `0.505121s` priming). Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave2_ota_and_boot_gates.md`.
- **Wave 3 (`Extension Certifier - Invariant 49 and HW1 Benchmarks`, `[BLOCKED / ERROR]` Partial -> Re-Planned as Wave 3R)**: Verified `ping -I eno1 -c 20 -i 0.1 172.22.1.15` (`20/20`, `0%` loss, `min/avg/max/mdev = 2.539/8.785/27.055/6.688 ms`), MP3 `decode_us_first = 865`, `decode_us_max = 12,384` us, `acks_per_s = 1.4933`, `i2c_nacks = 0`, and LMS restoration; blocked on host `ufw` `-P INPUT DROP` blocking `:45531` and MP3 `s1` pre-buffer overlap (`frames_played = 0`). Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave3_live_certification.md`.
- **Wave 3R (`Extension Certifier - Invariant 49 and HW1 Benchmarks`, `[EXECUTED & VERIFIED]`, 17/17 checks pass, muted `vol_pct == 0`)**:
  - Verified native `45.000s` (`1,984,500`-frame) FLAC steady-state (`flac_fps = 44,102.63` fps, `flac_acks_per_s = 1.3437` ACKs/s, `i2c_nacks = 0`, `decode_us_max = 25,718` us, `decode_us_first = 624` us), live `10.0.0.50` TCP windows (`:9000` `snd_wnd:52594` `rcv_wnd:42496`, `:3483` `snd_wnd:8192`, `0` transcoders), and `35 ms` EOF tight-poll (`peak_flac_frames_played = 1,984,500`, `100.000%` of `1,984,500` frames).
  - Verified steady-state MP3 (*Ibiza Global Radio*: `mp3_fps = 44,101.46` fps, `mp3_acks_per_s = 1.5373` ACKs/s, `i2c_nacks = 0`, `decode_us_max = 14,013` us, `decode_us_first = 949` us) and clean LMS/device restoration (`mode = "stop"`, `mixer volume = 50`, `playlist_tracks = 1`, `recovery = 0`, `i2c_nacks = 0`, `tas_automuted = true`, `frames_played = 0`). Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave3r_live_certification.md`.
- **Wave 4A-2 (`Build Cleanup and Docs/State Updater`, `[EXECUTED & VERIFIED]`)**: Backed up `DIAGNOSTIC_LOG.md`, `WO-2026-1115-002-basement-ota-deploy-and-verify.md`, and `INDEX.md` to `/home/console-admin/.gemini/antigravity/brain/41a3bf17-ecf0-4763-ae21-7f5eb1ef2af6/backups/wo_20261115_002_w4a2_*.bak`, appended the complete `WO-2026-1115-002` diagnostic entry to `DIAGNOSTIC_LOG.md`, appended this Fulfillment Summary, and updated `WO-2026-1115-002` Status and `INDEX.md` to `[EXECUTED & VERIFIED]`. Summary: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261115_wo002_wave4a_docs_and_ledgers.md`.
