# Subagent Summary: WO-208 S4 Opus Live Certification on BASEMENT

- **Work Order ID**: `WO-208`
- **Timestamp**: `2026-10-28T21:35:00-04:00`
- **Assigned Role**: `Extension Certifier (WO-208 S4 Live Opus Certification)`
- **Status**: `[BLOCKED / ERROR]`

## 1. Summary of Actions
1. **Pre-Flash Stage Audit**:
   - Executed `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota-s4-opus.bin`.
   - Verified `size=1603792`, `sha256=029f64ab10a491330ffeacc90ecc1876ba74637248f56286e4f9241c9d175889`, output `[STAGE-ONLY AUDIT PASS]`.
2. **LMS 10.0.0.10 Configuration**:
   - Installed `/var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus` (permissions `0644`, owner `squeezeboxserver:nogroup`).
   - Appended `ops ops * c8:2e:18:be:b8:34` (`-`) to `/etc/squeezeboxserver/custom-convert.conf` (backed up to `.bak_wo208`).
   - Patched SqueezeDSP `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` line 194 native preserve regex from `^(aac|ogg|alc)$` to `^(aac|ogg|alc|ops)$` (backed up to `.bak_wo208`).
   - Restarted LMS via `ssh 10.0.0.10 "sudo systemctl restart lyrionmusicserver"` and verified active.
3. **OTA Deployment (1-Flash Circuit Breaker Compliant)**:
   - Transitioned `172.22.1.15` to factory partition via `POST /recovery.json` (`recovery: 1`).
   - Uploaded staged binary `029f64ab` via `POST /flash.json` (1,603,792 bytes uploaded).
   - Booted into `ota_0` successfully.
4. **Live Gate Audits**:
   - **Gate 1 (`[EXECUTED & VERIFIED]`)**: Boot lease past 45s (`uptime_ms: 71,903`), `recovery: 0`, `rust_engine: true`, `otadata_prearmed_factory: true`, PSRAM intact (`free: 3534064`, `used: 660240`), `tas_pll_locked: true`, `i2c_nacks: 0`, `bt_enabled: true`.
   - **Gate 2 (`[EXECUTED & VERIFIED]`)**: Volume 55% verified (`vol_pct: 55`), Web UI HTTP 200 (`SIZE: 3928`).
   - **Gate 3 (`[BLOCKED / ERROR]`)**: LMS successfully delivered native `ops` stream (`stream_s called: format: ops url: file:///var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus`). Upon receiving `strm` format `'u'`, `"BASEMENT"` crashed immediately and reset to factory recovery.
   - **Gate 4 (`[BLOCKED]`)**: Blocked by Gate 3 crash.
   - **Gate 5 (`[BLOCKED / PARTIAL]`)**: `l2ping` passed (0% loss) in `ota_0`; SDP browse blocked in factory recovery.
   - **Gate 6 (`[EXECUTED & VERIFIED]`)**: CastBridge active (PID 36381), UPnP `MediaRenderer.xml` HTTP 200 OK (3,100 bytes).
5. **Certified S4 Baseline Snapshot**:
   - Preserved code and binary to `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_029f64ab/`.

## 2. Root Cause Analysis
- `CeltDecoder` in `rust/vendor/opus-rs` is **82,352 bytes**.
- `SilkDecoder` is **9,304 bytes**.
- `FixedVec` subframe scratch buffers in `OpusDecoder` are **46,080 bytes**.
- `OpusDecoder::init` (`rust/vendor/opus-rs/src/lib.rs:1658`) used `Box::new(CeltDecoder::new(...))` and `Box::new(SilkDecoder::new())` and `Box::new(FixedVec::from_value(...))`.
- In Rust, `Box::new(T)` evaluates `T` by value on the stack. `CeltDecoder::new` allocates `MaybeUninit<CeltDecoder>` (82,352 B) on the stack.
- The ESP32 FreeRTOS main task stack `.stack` in internal DRAM is only **36,288 bytes**.
- Calling `CeltDecoder::new` caused an immediate 82 KB stack overflow, corrupting memory and causing a hardware fault / watchdog reset into factory recovery.

## 3. Required Remediation
1. In `rust/vendor/opus-rs`:
   - In `fixedvec.rs`, add `boxed_from_value` allocating via `Box::<Self>::new_uninit()` and calling `write_from_value`.
   - In `celt.rs`, add `boxed_new` allocating via `Box::<Self>::new_uninit()` and calling `write_new`.
   - In `dec_api.rs`, add `boxed_new` allocating via `Box::<Self>::new_uninit()` and calling `write_new`.
   - In `lib.rs`, update `OpusDecoder::init` to use `boxed_new` and `boxed_from_value` so zero stack memory is used for decoders or large scratch buffers.
   - Add `OpusDecoder::boxed_new`.
2. In `squeezelite-decoders`:
   - Update `decode_single_packet` to call `opus_rs::OpusDecoder::boxed_new`.
3. Verify host tests, Clippy, Google Rust audit, build Xtensa release image, verify `.stack >= 36,000` B, and stage binary for authorized flash.

## 4. Modified Files
- `/etc/squeezeboxserver/custom-convert.conf` on 10.0.0.10 (backup: `/etc/squeezeboxserver/custom-convert.conf.bak_wo208`)
- `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm` on 10.0.0.10 (backup: `/var/lib/squeezeboxserver/cache/InstalledPlugins/Plugins/SqueezeDSP/Configuration.pm.bak_wo208`)
- `/var/lib/squeezeboxserver/cache/test_stereo_15s_48k.opus` on 10.0.0.10
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-208-s4-opus-live-certification.md`
