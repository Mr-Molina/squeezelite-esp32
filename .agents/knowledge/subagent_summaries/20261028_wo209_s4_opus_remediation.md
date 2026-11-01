# Subagent Summary: WO-209 S4 Opus In-Place Heap Allocation Remediation & Live Certification

- **Subagent Role**: `Rust no_std Codec Implementer & Extension Certifier (WO-209 Opus Remediation & Live Cert)`
- **Timestamp**: `2026-10-28T22:21:00-04:00`
- **Work Order**: `WO-209`
- **Status**: `[EXECUTED & VERIFIED]`
- **Target Component**: `"BASEMENT"` (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`, BT `CA:2E:18:BE:B8:35`)

## 1. Files Modified, Created, or Deleted
- **Modified**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/celt.rs`: Added `boxed_new()`, in-place heap initialization, scratch buffer fields (`w_norm`, `w_norm2`, `w_lowband`, `w_pvq_y`, `w_hadamard`, `w_f2`, `w_new_mem`), updated `celt_encode_with_ec` and `decode_impl_from_rc` to pass scratch buffers and invoke `backward_with_scratch`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/silk/dec_api.rs`: Added `boxed_new()` and in-place `write_new()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/fixedvec.rs`: Added `boxed_from_value()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/lib.rs`: Added `OpusDecoder::boxed_new()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/modes.rs`: Added in-place `write_new_48000_960_120()` and `boxed_new_48000_960_120()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/mdct.rs`: Added `pub const MAX_N4: usize = 480;`, in-place `write_new()`, and `backward_with_scratch()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/bands.rs`: Added `pub const MAX_NORM_SIZE: usize = 800;`, updated `quant_all_bands()` to accept scratch buffers, added `deinterleave_hadamard_with_scratch()` and `interleave_hadamard_with_scratch()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/pvq.rs`: Added `alg_unquant_with_scratch()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs/src/celt/plc.rs`: Updated MDCT backward call to invoke `backward_with_scratch()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`: Integrated `OpusDecoder::boxed_new()`, exported `pub fn prewarm_opus()`.
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/tests/opus_alloc_equivalence.rs`: Added 15s fixture decode verification test (`test_15s_fixture_decodes`, 720,648 frames).
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`: Pre-warmed `squeezelite_decoders::prewarm_opus()` at startup.
- **Created**:
  - `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_c88863f9/` (certified baseline snapshot).
  - `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/baseline_s4_opus_c88863f9/louder-esp32-app-basement-ota-s4-opus.bin`.
- **Deleted**:
  - Temporary scratch verification script `/home/console-admin/.gemini/antigravity/brain/6857cc1d-110e-4747-910c-e071e158015b/scratch/verify_gate3_opus.py`.

## 2. Rationale
Eliminated all runtime stack allocations > 1024 B across the entire Opus decode path. Specifically, `CeltDecoder` (82 KB), `SilkDecoder` (9 KB), `CeltMode` (32 KB), `MdctLookup` (32 KB), and real-time CELT packet decode scratch buffers (`norm_buf`, `norm2_buf`, `lowband_scratch_buf`, `new_mem`, `f2_buf`, `y_mu`, totaling 15.7 KB) were all migrated to PSRAM heap allocations. This prevents stack overflow into the 36 KB FreeRTOS task stack and allows native 48 kHz stereo Opus playback.

## 3. Verification & Certification Evidence
- `cargo test --workspace`: 0 failures across all unit and alloc equivalence tests.
- `google_rust_audit.py --strict`: 0 errors, 0 warnings.
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
- `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]` (Size: 1,605,472 B, SHA-256: `c88863f949a05184d77c35b300725f8a227c334aa2729b96d4496d18ae2671e6`).
- Live Flash Deployment: Exactly 1 flash executed (`POST /flash.json`).
- Live 6-Gate Hardware Certification on `"BASEMENT"` (`172.22.1.15`):
  - **Gate 1**: Boot lease past 45s (`uptime_ms: 54,717`), `recovery: 0`, `rust_engine: true`, `otadata_prearmed_factory: true`, PSRAM 3.5 MB free.
  - **Gate 2**: Volume set to 55% (`vol_pct: 55`, `tas_automuted: false`), Web UI HTTP 200.
  - **Gate 3**: Native Opus streaming: LMS streaming `test_stereo_15s_48k.opus`, `codec: "Opus"`, `frames_played` monotonically advancing across 10s (139,968 -> 291,904 frames), `tas_pll_locked: true`, `tas_automuted: false`, `i2c_nacks: 0`, `decode_us_first: 32,107 µs`. LMS repeat reset to 0.
  - **Gate 4**: Dynamic MP3 Reversion: `cmd.json?act=preset1`, `codec: "MP3"`, `tas_pll_locked: true`, `tas_automuted: false`, frames advancing.
  - **Gate 5**: Bluetooth A2DP & BLE: `l2ping` 0% loss, `sdptool browse` verified `BASEMENT` audio sink.
  - **Gate 6**: LMS Sibling Bridges: `MediaRenderer.xml` HTTP 200, `squeeze2cast` and `avahi-publish-service` active.

## 4. Known Risks & Invariants
- Zero active background tasks at completion (`manage_task(list)` confirmed empty).
- `/dev/ttyUSB0` was never touched.
- `POST /reboot_ota.json` was never called.
