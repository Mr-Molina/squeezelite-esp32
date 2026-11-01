# Subagent Summary: WO-255 Phase 3 Wave 1B Cluster 5 Pipeline, Coordinator & Tasks Remediation

- **Work Order ID**: `WO-255`
- **Timestamp**: `2026-11-01T10:05:00-05:00`
- **Role**: `Cluster 5 Pipeline, Coordinator & Tasks Remediation Implementer`
- **Status**: `[EXECUTED & VERIFIED]`
- **Handoff Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo255_cluster5_implementer.md`

## 1. Files Modified (Strict Disjoint Scope)
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`

### Backups Created (Rule 35)
- `/home/console-admin/.gemini/antigravity/brain/36895258-eb47-4867-9168-4e21344ef240/backups/pipeline.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/36895258-eb47-4867-9168-4e21344ef240/backups/coordinator.rs.bak`
- `/home/console-admin/.gemini/antigravity/brain/36895258-eb47-4867-9168-4e21344ef240/backups/tasks.rs.bak`

## 2. Findings Remediated & Implementation Details

### `LOGIC-201` (`CRITICAL`) & `PERF-201` (`HIGH`, `pipeline.rs`)
- **Issue**: `StreamPipeline::resample_decoded_pcm` used `[0i32; 512]` (2 KiB stack spike) and `alloc::vec![0i32; max_out_samples]` per decode step, while dropping unconsumed frames if output exceeded `PCM_SCRATCH_SAMPLES`.
- **Remediation**:
  - Implemented `max_decode_scratch_samples(&self) -> usize` to clamp decode step frames when `to_hz > from_hz`, guaranteeing that resampled output never exceeds `PCM_SCRATCH_FRAMES`.
  - In `resample_decoded_pcm`, relocated input frames to the end of `pcm_scratch` via `copy_within` and processed chunks in 64-frame windows using a bounded local buffer (`[0i32; 256]`) directly into `self.pcm_scratch[..out_pos]`.
  - Maintained zero added struct size in `StreamPipeline` and zero per-step heap allocations (`alloc::vec!`).
  - Added unit test `test_pipeline_resampler_upsample_clamp_and_flush_tail`.

### `LOGIC-202` (`HIGH`, `pipeline.rs`)
- **Issue**: Dynamic sample rate changes discovered from `self.decoder.sample_rate()` were not propagating to the equalizer or active resampler during streaming.
- **Remediation**:
  - Implemented `check_decoder_sample_rate(&mut self)` invoked during `decode_stream_step`.
  - If `sr > 0` and `sr != self.active_sample_rate_hz`, updates `self.active_sample_rate_hz = sr` and retunes the equalizer via `self.retune_equalizer()`.
  - If a resampler is active with `r.from_hz() != sr`, reconfigures via `configure_resampler(sr, r.to_hz())` or clears via `clear_resampler()` if `sr == r.to_hz()`.
  - Added unit test `test_pipeline_dynamic_sample_rate_retunes_and_reconfigures_resampler`.

### `LOGIC-207` (`LOW`, `pipeline.rs`)
- **Issue**: Resampler FIR filter group-delay tails were dropped abruptly on track boundaries and stream transitions.
- **Remediation**:
  - Added `pub fn flush_resampler_tail(&mut self) -> usize` to drain `resampler.flush(...)` into `self.output_buffer`.
  - Updated `clear_resampler(&mut self)` to flush the FIR tail before dropping the resampler instance.
  - Verified in pipeline tests.

### `LOGIC-206` (`CRITICAL`, `coordinator.rs`)
- **Issue**: External playback preemption (`begin_external_playback`) and release (`end_external_playback`) did not flush compressed/partial stream bytes or restore LMS decoders, causing decoder corruption on stream return.
- **Remediation**:
  - Added `begin_external_playback_with_pipeline` which flushes `pipeline.stream_buffer` before raw PCM is written.
  - Added `reinstall_lms_decoder` which flushes `stream_buffer` and reinstalls the decoder matching `self.current_format` (`FLAC`, `MP3`, `AAC`, `Vorbis`, `Opus`, `PCM`).
  - Added `end_external_playback_and_restore` combining external playback teardown with decoder re-installation.
  - Maintained backward compatibility for existing callers of `begin_external_playback` and `end_external_playback`.
  - Added unit test `test_wo255_external_playback_restore_lms_decoder_and_flush`.

### `LOGIC-205` (`MEDIUM`, `coordinator.rs`)
- **Issue**: `handle_command` for opcode `eqlz` rejected short payloads (`< 10` bytes), ignoring partial band updates.
- **Remediation**:
  - Changed condition from `if payload.len() >= 10` to `if !payload.is_empty()`.
  - Iterates up to 10 bands from `payload`, converting `b as i8` to dB gains, leaving unspecified bands at 0 dB, and applies via `pipeline.set_eq_gains_db(&gains)`.
  - Added unit test `test_wo255_short_eqlz_payload_applies_partial_band_gains`.

### `CONC-203` (`HIGH`, `tasks.rs`)
- **Issue**: `tasks::bt_poll` did not register Bluetooth audio transitions with `SourceArbiter`, bypassing source prioritization hierarchy.
- **Remediation**:
  - Integrated `bt_poll` with `s.coordinator.request_source(AudioSource::Bluetooth)` upon stream start.
  - Calls `s.coordinator.release_source(AudioSource::Bluetooth)` and `s.coordinator.end_external_playback_and_restore(s.pipeline)` upon stream stop.
  - Added unit test `test_wo255_bt_source_arbiter_priority_transitions`.

### `LOGIC-603` & `PERF-601` (`HIGH`/`MEDIUM`, `tasks.rs`)
- **Issue**: In `tasks::amp_health`, persistent amplifier faults triggered continuous, unthrottled `recover_from_fault()` calls.
- **Remediation**:
  - Added `CONSECUTIVE_FAULTS: AtomicU8` and `LAST_RECOVERY_MS: AtomicU32` tracking inside `tasks.rs` (`mod device`).
  - If 3 consecutive recovery attempts fail without an operational window, applies a 30-second cooldown before retrying fault clearing.
  - Added unit test `test_wo255_amp_health_consecutive_fault_throttling`.

## 3. Verification Evidence
1. **Google Rust Style Audit (`google_rust_audit.py --strict`)**:
   - Command: `python ~/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py .../louder-esp32-app/src --strict`
   - Result: `0 error(s), 0 warning(s)` (Exit Code `0`).
2. **Cargo Unit Tests**:
   - `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- test_pipeline`:
     - Result: `12 passed; 0 failed`.
   - `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- coordinator::tests`:
     - Result: `92 passed; 0 failed`.
   - `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --bin louder-esp32-app -- tasks::tests`:
     - Result: `7 passed; 0 failed`.
   - `cargo check --manifest-path rust/Cargo.toml -p louder-esp32-app --lib`:
     - Result: Passed with Exit Code `0`.
3. **Background Process Verification**:
   - `manage_task(Action: "list")`: 0 background tasks active.

## 4. Risks & Untested Assumptions
- Hardware-specific Xtensa tasks (`mod device` in `tasks.rs`) require physical target verification during Stage Gate OTA testing on the physical Basement target (`172.22.1.15`).
- `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate` in `main.rs:3942` asserts against legacy mock byte `[120, 128]` rather than `WO-251`'s updated `[120, 129]` (`FAULT_CLEAR_ALL`). This is scoped to Cluster 6 (`WO-257`) in `main.rs`.
