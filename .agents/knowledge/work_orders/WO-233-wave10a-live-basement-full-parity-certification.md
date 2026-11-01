# Work Order: WO-233 — Wave 10A: Live `"BASEMENT"` (`172.22.1.15`) OTA Deployment & 6-Gate Full Multi-Protocol Certification

- **Work Order ID**: `WO-233`
- **Timestamp**: `2026-11-01T01:08:00-04:00`
- **Assigned Subagent Role**: `Extension Certifier` (`TypeName: "self"`)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, BT MAC `CA:2E:18:BE:B8:35`), Lyrion Music Server (`10.0.0.10:9000` / `3483`)
- **Status**: `[BLOCKED / ERROR]`

## 1. Objective & Hypothesis
Deploy the verified full-parity staged OTA image `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` (`1,793,424` bytes, SHA-256 `23175117f50167b8ef1996707eafe7d4391da1e940609708e5ac8e8822580bea`) to `"BASEMENT"` (`172.22.1.15`) via the 1-Flash Circuit Breaker (`POST /recovery.json` -> verify `recovery: 1` -> `POST /flash.json` strictly once) and certify all 6 live hardware gates across Phase 0+1 and Phases 2–7.

## 2. Hard Safety Constraints
1. **Strict Serial Isolation (`single-target-troubleshooting.md` / Invariant 47)**: **NEVER** open, probe, reset, or flash `/dev/ttyUSB0` (separate CYD board).
2. **Forbidden Endpoint (`Invariant 42`)**: **NEVER** call `POST http://172.22.1.15/reboot_ota.json`.
3. **1-Flash Circuit Breaker**: Flash to `172.22.1.15` at most **ONCE** via Factory recovery `POST http://172.22.1.15/flash.json`.
4. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
5. **Disjoint File Ownership**: Do **NOT** modify any files under `/home/console-admin/Github/squeezelite-esp32/rust/` or `/home/console-admin/Github/squeezelite-esp32/docs/` (`WO-234` owns `docs/` in parallel). You may append findings to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`, your work order file, and your subagent summary file.
6. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution & 6-Gate Live Certification Steps
1. **Pre-Flight Check**:
   - Verify `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` has size `1,793,424` B and SHA-256 `23175117f50167b8ef1996707eafe7d4391da1e940609708e5ac8e8822580bea`.
   - Query current `/status.json` on `http://172.22.1.15/status.json`.
2. **1-Flash OTA Deployment to `"BASEMENT"` (`172.22.1.15`)**:
   - If `"recovery"` is `0`, transition `"BASEMENT"` into Factory recovery (`0x10000`) via `/usr/bin/curl -s -X POST http://172.22.1.15/recovery.json`, wait for reboot (~8–12s), and verify `http://172.22.1.15/status.json` (or Factory recovery status) shows recovery mode active (`recovery: 1`).
   - Upload `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` strictly **ONCE** via `POST http://172.22.1.15/flash.json` (`--data-binary @/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`).
3. **Gate 1 — Boot Lease & Hardware Health (`> 45s`)**:
   - Wait for `"BASEMENT"` to boot into `ota_0` (`0x1D0000`) and run past the 45-second boot lease (`uptime_ms > 46,000`).
   - Verify `/status.json` reports `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"psram_free_bytes"` + `"psram_used_bytes"` = `4,194,304` B (`4 MB` PSRAM intact), `"core1_active": true`, `"tas_pll_locked": true`, `"i2c_nacks": 0`, and `"bt_enabled": true`.
4. **Gate 2 — Web UI, Phase 3 REST APIs, & TAS5805M Control**:
   - Verify `GET http://172.22.1.15/` returns `HTTP 200 OK`.
   - Verify `GET http://172.22.1.15/cmd.json?act=vol55` sets `"vol_pct": 55` and `"tas_automuted": false`.
   - Verify Phase 3 REST endpoints (`GET /config.json`, `GET /scan.json`, `GET /messages.json`, `GET /commands.json`) if routed in the active HTTP server or via `/status.json`.
5. **Gate 3 — Live SlimProto Multi-Codec Playback (`FLAC`, `AAC`, `Vorbis`, `ALAC`, `Opus`, `MP3`) on LMS `10.0.0.10`**:
   - Trigger playback on player `c8:2e:18:be:b8:34` via LMS JSON-RPC (`http://10.0.0.10:9000/jsonrpc.js`) or `cmd.json?act=preset1`:
     - Verify native `FLAC` (`file:///var/lib/squeezeboxserver/cache/test_stereo_16bit.flac` or local test tracks in `/var/lib/squeezeboxserver/cache/`) and `MP3` (*Ibiza Global Radio* `cmd.json?act=preset1`), plus any cached `AAC`/`Vorbis`/`ALAC`/`Opus` tracks on `10.0.0.10`, confirming `"codec"` switches accurately, `"frames_played"` advances monotonically at ~44.1/48 kHz, PSRAM is reclaimed cleanly on track changes, `"tas_pll_locked": true`, and `"i2c_nacks": 0`.
6. **Gate 4 — Live AirPlay 1 (RAOP) & Multi-Source `SourceArbiter` Preemption**:
   - Verify RTSP port `5000` (`OPTIONS * RTSP/1.0`) and/or live AirPlay ALAC streaming preempts LMS (`"codec": "AirPlay"`, `frames_played` advancing) and `TEARDOWN` cleanly restores LMS playback (`"codec": "MP3"`).
7. **Gate 5 — Live Spotify Connect (`cspot`) ZeroConf Discovery & Playback**:
   - Verify `GET http://172.22.1.15/?action=getInfo` returns HTTP 200 JSON with `status: 101`, `remoteName: "BASEMENT"`, and `deviceID: "142137fd329622137a149016c82e18beb834"`.
8. **Gate 6 — Bluetooth Classic/BLE Coexistence & LMS Sibling Bridges**:
   - Run `/usr/bin/sudo -n /usr/bin/l2ping -c 3 CA:2E:18:BE:B8:35` (`0% loss`) and `/usr/bin/sdptool browse CA:2E:18:BE:B8:35` (`Audio Sink 0x110b`) while Wi-Fi audio is streaming.
   - Verify LMS `10.0.0.10` (`:9000` & `:3483`) and UPnP/Cast/AirPlay bridges remain active and healthy.
9. **Deliverables**:
   - Append a concise synthesis to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md`.
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-233-wave10a-live-basement-full-parity-certification.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo233_wave10a_live_basement_cert.md`.

---

## 4. Fulfillment Section
- **Overall Status**: `[BLOCKED / ERROR]` (1-Flash Circuit Breaker engaged after Gate 1 rollback to Factory recovery `0x10000`; root causes isolated in ELF & `main.rs`/`pipeline.rs`)
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo233_wave10a_live_basement_cert.md`
- **Step-by-Step Execution & Gate Verification**:
  1. **Pre-Flight Check (`[EXECUTED & VERIFIED]`)**:
     - Verified `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` has exact size `1,793,424` B and SHA-256 `23175117f50167b8ef1996707eafe7d4391da1e940609708e5ac8e8822580bea`.
     - Pre-flash `GET http://172.22.1.15/status.json` confirmed `"BASEMENT"` online (`"recovery": 0`, `"version": "I2S-4MFlash-Rust"`, `"uptime_ms": 224201334`).
  2. **1-Flash OTA Deployment (`[EXECUTED & VERIFIED]`)**:
     - Transitioned `"BASEMENT"` (`172.22.1.15`) to Factory recovery (`0x10000`) via `POST http://172.22.1.15/recovery.json`, waited 12s, and verified `GET http://172.22.1.15/status.json` reported `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).
     - Uploaded `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` strictly **ONCE** via `POST http://172.22.1.15/flash.json` (`File uploaded. Flashing started.`).
  3. **Gate 1 — Boot Lease & Hardware Health (`[BLOCKED / ERROR]`)**:
     - After waiting 60 seconds for flash completion, `ota_0` (`0x1D0000`) boot, and the 45-second lease window, `GET http://172.22.1.15/status.json` returned `"recovery": 1` (`"version": "I2S-4MFlash-1681"`).
     - The `ota_0` image executed the Gate 1 `otadata` pre-arm (`0xD000..0xF000` erased to `0xFF`) at the start of `bring_up()` and then panicked/reset back to Factory recovery (`0x10000`).
     - Strictly honored the **1-Flash Circuit Breaker**: halted all further flashing immediately. `"BASEMENT"` (`172.22.1.15`) remains online and safe in Factory recovery (`"recovery": 1`).
  4. **Gates 2–6 (`[BLOCKED / ERROR]`)**:
     - Blocked by Gate 1 boot rollback.
- **Forensic Root Causes Isolated**:
  1. **Root Cause 1 (CRITICAL — Uninitialized `StreamPipeline` Fields in `.dram2_uninit`, `main.rs:1602-1661`)**:
     - `WO-228` added `eq_gains_db: [f32; 10]`, `loudness_level: u8`, `active_sample_rate_hz: u32`, and `resampler: Option<Box<Resampler>>` to `StreamPipeline` (`pipeline.rs:330-337`), but `init_static_pipeline()` (`main.rs:1602-1661`) initializes `STATIC_PIPELINE: MaybeUninit<HwStreamPipeline>` in `.dram2_uninit` (`0x3fff0380`, `14,144` B) field-by-field via `addr_of_mut!` without zeroing the memory (`ptr.write_bytes(0, 1)`) and without writing those 4 fields.
     - Uninitialized non-zero SRAM bytes at `(*ptr).resampler` are interpreted via Rust's non-null pointer niche as `Some(Box<Resampler>)` pointing to a wild address, causing immediate UB (`LoadProhibited`/`StoreProhibited` or invalid heap free panic) during `amp_apply_boot_config()` (`retune_equalizer()`) or stream/resampler reset.
  2. **Root Cause 2 (HIGH — `Resampler` 13 KB Stack Construction & `.stack` / Internal Heap Regression, `pipeline.rs:1322-1356`, `main.rs:1103`)**:
     - `<StreamPipeline>::configure_resampler` (`0x4013d4f4`) allocates a `13,008` B stack frame (`entry a1, 13008`) because `Box::new(Resampler::new(...))` constructs the `~12.5 KB` `Resampler` struct on the stack before copying to heap, while `.stack` in the ELF was compressed to `27,232` B (`0x3ffd95a0`, violating the `>= 36,000` B Invariant 47 threshold) and `Internal` DRAM heap was reduced to `64 KB` (`18 KB` `.dram2_uninit` + `46 KB` `.bss`).
