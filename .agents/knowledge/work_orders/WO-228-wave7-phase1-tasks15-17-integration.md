# Work Order: WO-228 — Wave 7: Phase 1 Tasks 15–17 Pipeline & Coordinator Integration (`louder-esp32-app/src/{pipeline,coordinator,backend_device,tasks}.rs`)

- **Work Order ID**: `WO-228`
- **Timestamp**: `2026-11-01T00:18:30-04:00`
- **Assigned Subagent Role**: `Rust Phase 1 Pipeline & Coordinator Integrator` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{pipeline,coordinator,backend_device,tasks}.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Wire Phase 1 Tasks 15, 16, and 17 from `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-phase0-1-foundation-correct-player.md` (lines 320–375) into `louder-esp32-app/src/{pipeline,coordinator,backend_device,tasks}.rs`:
1. **Task 15 (Clock Enforcement + DMA-Corrected Elapsed Time)**:
   - Coordinator elapsed time for `STAT`: `frames_for_stat = frames_pushed.saturating_sub(backend.dma_outstanding_frames() as u64)`.
   - Defer hardware sample-rate switch (`pending_rate: Option<u32>`) when a gapless track boundary is queued (`output_buffer_available() > 0`), applying `backend.set_sample_rate(rate)` only when `active_marker_crossed()` / `take_crossed_boundary()` fires!
   - When `strm 's'` specifies an unverified rate (`!VERIFIED_RATES.contains(&rate)` and resampler is not engaged/available for that mode), refuse with `STMn` without calling `backend.set_sample_rate`.
   - Add Task 15 unit tests (`test_stat_elapsed_subtracts_dma_outstanding`, `test_rate_switch_deferred_until_marker_crossed`, `test_unverified_rate_refused_at_strm_s`).
2. **Task 16 (Runtime RBJ EQ + LMS `eqlz`/`loud` + Volume-Indexed Loudness Contour)**:
   - In `pipeline.rs`: store `eq_gains_db: [f32; 10]`, `loudness_level: u8`, and `active_sample_rate_hz: u32`; when `set_sample_rate`, `set_eq_gains_db`, `set_loudness_level`, or volume percent changes, compute `effective_gains[i] = (eq_gains_db[i] + loudness_gains[i]).clamp(-12.0, 12.0)` using `squeezelite_audio_core::equalizer::loudness_gains_db` and call `self.equalizer.retune(&effective_gains, active_sample_rate_hz as f32)`.
   - In `coordinator.rs`: handle LMS `eqlz` packet (10 `i8` gains, clamped to `±12 dB` -> `pipeline.set_eq_gains_db(...)` / `retune`) and `loud` packet (`u8` level `0..=10` stored and folded into EQ gain vector on volume change).
   - Add unit tests verifying `eqlz` and `loud` packet handling in `coordinator.rs` and runtime `retune` on sample-rate change in `pipeline.rs`.
3. **Task 17 (Polyphase Resampler Stage)**:
   - In `pipeline.rs` & `coordinator.rs`: add optional `Resampler` stage (`Option<alloc::boxed::Box<squeezelite_audio_core::Resampler>>`) between decoder output and EQ/GainStage, plus `max_sample_rate: u32` (`0` = unlimited) configuration on `PlayerCoordinator`.
   - Engage `Resampler` (e.g., to `48_000` or `44_100` Hz, or `max_sample_rate` when `stream_rate > max_sample_rate` or when `resampler_enabled` is configured for unverified rates) and add test `test_engages_only_for_unverified_or_capped_rates`.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly:
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/backend_device.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`
   - Do **NOT** modify `http_control.rs`, `wifi.rs`, `ota_safety.rs`, `bt_a2dp.rs`, `lib.rs`, or `main.rs` (owned concurrently by `WO-229`–`WO-231` and `WO-232`).
   - Do **NOT** break any existing public signatures used by `main.rs` or `louder-esp32-app/tests/*`!
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - Zero `#[cfg(...target_arch...)]` in `coordinator.rs` and `pipeline.rs` (`no_cfg_invariant.rs` test must stay green!).
   - Single-module `use` statements (`GRS-001`), `#[non_exhaustive]` where required (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Verification & Deliverables
1. Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/backend_device.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs --strict` (`0 error(s), 0 warning(s)`).
2. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo228_wave7_phase1_tasks15_17.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Files Modified**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`
- **Deliverables Implemented**:
  1. **Task 15 (Clock Enforcement + DMA-Corrected Elapsed Time)**:
     - Added `dma_outstanding_frames: u32`, `pending_rate: Option<u32>`, `crossed_rate: Option<u32>`, `take_crossed_rate()`, and `frames_for_stat()` (`pipeline.frames_played.saturating_sub(self.dma_outstanding_frames as u64)`) to `PlayerCoordinator`.
     - Updated `tick`, `report_stream_connected`, `flush_command`, `sync_cyd_telemetry`, and `send_metrics_stat` in `coordinator.rs` to report `self.frames_for_stat(pipeline)`.
     - Deferred hardware clock switches (`pending_rate = Some(sample_rate)`) in `start_stream` when `!output_idle`, applying `backend.set_sample_rate(rate)` only when `pipeline.active_marker_crossed()` fires in `note_output` (or via `take_crossed_rate()` in `tasks.rs::i2s_feed`).
     - Refused unverified sample rates (`!VERIFIED_RATES.contains(&sample_rate)`) at `strm 's'` with `STMn` without calling `backend.set_sample_rate`.
     - Updated `tasks.rs::i2s_feed` to apply `s.coord.take_crossed_rate()` to `s.backend.set_sample_rate(rate)` when gapless boundaries cross during the external audio pump.
     - Added unit tests `test_stat_elapsed_subtracts_dma_outstanding`, `test_rate_switch_deferred_until_marker_crossed`, and `test_unverified_rate_refused_at_strm_s`.
  2. **Task 16 (Runtime RBJ EQ + LMS `eqlz`/`loud` + Volume-Indexed Loudness Contour)**:
     - Added `eq_gains_db: [f32; 10]`, `loudness_level: u8`, `active_sample_rate_hz: u32`, `set_eq_gains_db`, `eq_gains_db`, `set_loudness_level`, `loudness_level`, and `retune_equalizer` to `StreamPipeline` in `pipeline.rs`.
     - Wired `retune_equalizer()` to fold `squeezelite_audio_core::equalizer::loudness_gains_db(vol_pct, self.loudness_level)` into `eq_gains_db` (clamped to `[-12.0, 12.0]`) and invoke `self.equalizer.retune(&effective, fs_hz)` whenever sample rate, EQ gains, preset, loudness level, or requested volume changes.
     - Wired LMS `eqlz` (10 signed `i8` gains clamped to `[-12.0, 12.0]`) and `loud` (`0..=10` level) commands in `coordinator.rs::handle_command`.
     - Added unit tests `test_runtime_eq_retunes_on_sample_rate_and_loudness_changes` (`pipeline.rs`) and `test_eqlz_and_loud_commands_retune_pipeline_eq` (`coordinator.rs`).
  3. **Task 17 (Polyphase Resampler Stage)**:
     - Added `resampler: Option<Box<Resampler>>`, `configure_resampler`, `clear_resampler`, `is_resampler_engaged`, and `resample_decoded_pcm` to `StreamPipeline` in `pipeline.rs`, executing between decoder PCM output and EQ processing and propagating the resampled output rate to `TrackMarker`.
     - Added `max_sample_rate`, `resampler_enabled`, and `resampler_quality` configuration to `PlayerCoordinator` in `coordinator.rs`, engaging `Resampler` when `max_sample_rate > 0 && rate_hint > max_sample_rate` or when `unverified_rate && resampler_enabled`.
     - Added unit tests `test_pipeline_resampler_stage_downsamples_96k_to_48k` (`pipeline.rs`) and `test_engages_only_for_unverified_or_capped_rates` (`coordinator.rs`).
- **Verification**:
  - `cargo test -p louder-esp32-app --lib`: `279 passed; 0 failed` (`exit code 0`).
  - `cargo test -p louder-esp32-app --test no_cfg_invariant --test session_replay`: `5 passed; 0 failed` (`exit code 0`).
  - `google_rust_audit.py` (`--strict`) on `pipeline.rs`, `coordinator.rs`, `backend_device.rs`, `tasks.rs`: `0 error(s), 0 warning(s)` (`exit code 0`).
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo228_wave7_phase1_tasks15_17.md`
