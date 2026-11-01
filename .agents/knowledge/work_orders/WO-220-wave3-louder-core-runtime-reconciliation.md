# Work Order: WO-220 — Wave 3A: Reconcile `louder-esp32-app` Library Surface (`pipeline.rs`, `network.rs`, `coordinator.rs`, `lib.rs`) onto Phase 0+1 Architecture

- **Work Order ID**: `WO-220`
- **Timestamp**: `2026-10-31T16:54:30-04:00`
- **Assigned Subagent Role**: `Rust Library Runtime Reconciler` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{pipeline,network,coordinator,lib}.rs` & `tests/*`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Hypothesis
Reconcile the 4 core library files of `louder-esp32-app` (`pipeline.rs`, `network.rs`, `coordinator.rs`, `lib.rs`) between `MK5-Rust` (`cd139b61`, Phase 0+1 unified `io_traits` / `pump_device_phase` architecture, `FadeEngine`, `GainStage`, Balance, gapless track boundaries) and `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`, native `no_std` PSRAM codecs `Flac`/`Aac`/`Vorbis`/`Alac`/`Opus`, `decode_us_max`, `SourceArbiter`, and `GRS-001`–`GRS-012` audit remediations).

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN**:
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/tests/**` (only if minor `GRS-001`–`GRS-012` or signature adjustments are needed)
   - Do **NOT** modify `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (owned concurrently by `WO-221`). When running `cargo test`, pass `--lib` and `--test '*'` (not `--bins` or `--all-targets`) so you never compile `src/main.rs` while `WO-221` is editing it.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `lib.rs` must contain ONLY `mod` declarations and `pub use` re-exports (`GRS-003`).
   - Single-module `use` statements (`GRS-001`), zero glob imports outside `#[cfg(test)]` (`GRS-002`), zero production `.unwrap()` (`GRS-010`), dual `// JUSTIFICATION:` + `// SAFETY:` comments (`GRS-008`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps (In Dependency Order)
1. **Reconcile `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`**:
   - Inspect `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..local/wo157-wo214-codecs-airplay-spotify -- rust/crates/louder-esp32-app/src/pipeline.rs`.
   - On top of the Phase 1 `pipeline.rs` (single-slot/gapless track boundary, `FadeEngine`, `GainStage`, Balance):
     - Enable `ActiveDecoder::{Flac, Aac, Vorbis, Alac, Opus}` unconditionally (`no_std + alloc` and `std`) using `squeezelite_decoders::{FlacDecoder, AacDecoder, VorbisDecoder, AlacDecoder, OpusDecoder}`.
     - Add `decode_us_max` and `decode_us_first` tracking fields and methods (`record_decode_us`, `decode_us_max`, `decode_us_first`, `reset_decode_timing`).
     - Apply `WO-192` `GRS-001`–`GRS-012` audit remediations while preserving all Phase 1 methods and signatures used by `coordinator.rs` and `louder-esp32-app/tests/*`.
2. **Reconcile `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs`**:
   - Inspect `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..local/wo157-wo214-codecs-airplay-spotify -- rust/crates/louder-esp32-app/src/network.rs`.
   - Preserve Phase 0+1 `stream_codec` integration and `SlimTransport`/`StreamSource` trait implementations while merging `WO-192` `GRS-001`–`GRS-012` audit fixes.
3. **Reconcile `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`**:
   - Inspect `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..local/wo157-wo214-codecs-airplay-spotify -- rust/crates/louder-esp32-app/src/coordinator.rs`.
   - On top of the Phase 0+1 unified `PlayerCoordinator` (`io_traits`, `pump_device_phase`, C-parity `HELO`/`STAT`/`serv`/backoff):
     - Add `AudioSource` (`SlimProto`, `Bluetooth`, `Spotify`, `AirPlay`) and `SourceArbiter` (`WO-212`, priority `AirPlay > Spotify > Bluetooth > SlimProto` with dynamic LMS pause/resume tracking) + unit tests.
     - Ensure `strm` codec dispatch handles `'f'` (`Flac`), `'a'` (`Aac`), `'o'` (`Vorbis`), `'l'` (`Alac`), and `'u'` (`Opus`) unconditionally (`no_std + alloc` and `std`).
     - Apply `WO-192` `GRS-001`–`GRS-012` audit fixes while keeping all Phase 0+1 integration test entry points (`louder-esp32-app/tests/*`) compatible.
4. **Reconcile `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`**:
   - Combine re-exports from `cd139b61` and `local/wo157-wo214-codecs-airplay-spotify` (`AudioSource`, `SourceArbiter`, etc.), ensuring `lib.rs` has strictly `mod` and `pub use` statements (`GRS-003`) and single-module imports (`GRS-001`).
5. **Verify Library & Integration Tests and `google_rust_audit.py`**:
   - Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib --tests` (`0` failures).
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs --strict` (`0` errors, `0` warnings).
6. **Deliverables**:
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-220-wave3-louder-core-runtime-reconciliation.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo220_wave3a_library_runtime.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Timestamp**: `2026-10-31T17:13:15-04:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo220_wave3a_library_runtime.md`

### Reconciled Files
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` — Combined Phase 0+1 (`cd139b61`) submodule declarations and re-exports with `AudioSource`, `SourceArbiter` (from `coordinator`) and `RaopStreamClient` (from `network`), enforcing strict `GRS-001`–`GRS-003` and `GRS-012` hygiene.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` — Preserved Phase 0+1 single-slot `TrackBoundary`, `FadeEngine`, `ChannelMode`, `mp3_spare` zero-heap slot, and `skip_output_frames` while enabling `ActiveDecoder::{Flac, Aac, Vorbis, Alac, Opus}` unconditionally (`no_std + alloc` and `std`), setting bare-metal `READ_SCRATCH_BYTES = 1728`, adding `decode_us_max` / `decode_us_first` telemetry fields and methods (`record_decode_us`, `decode_us_max`, `decode_us_first`, `reset_decode_timing`), resetting equalizer history on sample-rate change, dispatching `'l'` (`Alac`) and `'u'` (`Opus`) in `select_format`, and formatting to `GRS-012` with zero `target_arch` `cfg` gates.
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs` — Preserved Phase 0+1 `stream_codec` integration and `SlimTransport`/`StreamSource` trait implementations while merging the `LmsDiscovery::discover_server` deadline loop, `RaopStreamClient` RTSP lifecycle methods + unit tests, and `GRS-001`–`GRS-012` compliance.
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` — Preserved Phase 0+1 unified `PlayerCoordinator` (`io_traits`, `pump_decode`/`pump_output`, C-parity `HELO`/`STAT`/`serv`) while adding `AudioSource` (`None`, `SlimProto`, `Bluetooth`, `Spotify`, `AirPlay`) and `SourceArbiter` (`AirPlay > Spotify > Bluetooth > SlimProto`), wiring `arbiter` into `PlayerCoordinator` (`start_stream`, `StreamCommandType::Stop`, `flush_command`, `active_source`, `request_source`, `release_source`), enabling `AudioFormat::Alac` and `AudioFormat::Opus` in `select_decoder`, adding unit tests, and formatting to `GRS-012` with zero `target_arch` `cfg` gates.

### Verification Evidence
- **Google Rust Static Audit (`GRS-001`..`GRS-012`)**:
  - Command: `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/network.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs --strict`
  - Output: `0 error(s), 0 warning(s)` (exit code `0`).
- **Library & Integration Test Suite**:
  - Command: `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib --test '*'`
  - Output: `313 passed; 0 failed` (`261` unit tests in `src/lib.rs`, `6` in `tests/config_persistence.rs`, `1` in `tests/icy_metadata_passthrough.rs`, `1` in `tests/marker_fuzz.rs`, `4` in `tests/no_cfg_invariant.rs`, `1` in `tests/session_replay.rs`, `39` in `tests/trait_conformance.rs`; exit code `0`).
  - Note: In Cargo, `--tests` also compiles `--bins` unit tests (`src/main.rs`, owned concurrently by `WO-221`), whereas `--lib --test '*'` runs strictly the library unit tests and all integration test targets in `tests/*.rs`.
