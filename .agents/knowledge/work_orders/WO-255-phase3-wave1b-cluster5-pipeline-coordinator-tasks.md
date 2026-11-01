# Work Order: WO-255 — Phase 3 Wave 1B Cluster 5 Remediation (`pipeline.rs`, `coordinator.rs`, `tasks.rs`)

- **Work Order ID**: `WO-255`
- **Timestamp**: `2026-11-01T09:18:00-05:00`
- **Assigned Subagent Role**: `Cluster 5 Pipeline, Coordinator & Tasks Remediation Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`
- **Status**: `[EXECUTED & VERIFIED]`
- **Handoff Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo255_cluster5_implementer.md`

## 1. Objective & Strict Disjoint Scope
Remediate all **7 findings** (`LOGIC-201`, `LOGIC-206`, `PERF-201`, `LOGIC-202`, `CONC-203`, `LOGIC-205`, `LOGIC-207`) plus `LOGIC-603`/`PERF-601` caller updates in `pipeline.rs`, `coordinator.rs`, and `tasks.rs`.

## 2. Findings to Remediate
1. **`LOGIC-201` (`CRITICAL`) & `PERF-201` (`HIGH`, `louder-esp32-app/src/pipeline.rs:556-583`)**:
   - In `StreamPipeline::resample_decoded_pcm`:
     - Eliminate both the `let mut stack_out = [0i32; 512];` (2 KiB stack spike) and `let mut heap_out = alloc::vec![0i32; max_out_samples];` (per-step heap allocation) AND prevent dropping unconsumed input frames when `max_out_samples > PCM_SCRATCH_SAMPLES`!
     - How to do this with **0 bytes of added struct size in `StreamPipeline`** (preserving `.dram2_uninit` and `.stack` budgets!):
       - Add a boxed scratch slice `scratch_out: alloc::boxed::Box<[i32]>` inside the heap-allocated `Box<Resampler>` (or process `self.pcm_scratch[..in_samples]` in small chunk windows of e.g. `64` stereo frames = `128` samples into a small `[0i32; 256]` local buffer or a `Vec<i32>` stored inside `StreamPipeline` / `Resampler` that is allocated **once** in PSRAM when `configure_resampler` is called, and cap `max_frames` passed to `self.decoder.decode(...)` in `decode_stream_step` when `self.resampler.is_some()` so that `max_frames * to_hz / from_hz` never exceeds `PCM_SCRATCH_FRAMES`!).
       - Let's verify that math: in `decode_stream_step`, `max_frames` is computed at line ~495 (`let max_frames = ...`). If `self.resampler` is active with `to_hz > from_hz` (upsampling), clamping `max_frames` passed to `self.decoder.decode` to `(PCM_SCRATCH_FRAMES * from_hz) / to_hz - 4` guarantees that 100% of decoded frames (`frames_decoded`) fit inside `PCM_SCRATCH_SAMPLES` after upsampling (`consumed == frames_decoded`), so **zero decoded frames are ever dropped**! And storing a pre-allocated `resampler_scratch: Option<alloc::boxed::Box<[i32; PCM_SCRATCH_SAMPLES]>>` (or allocating a `Vec<i32>` once inside `configure_resampler`, or processing in 64-frame blocks from the end/front) eliminates the per-step `alloc::vec!` in `resample_decoded_pcm`! (Note: if you add any field to `StreamPipeline`, you MUST also initialize it in `main.rs::init_static_pipeline`, so keeping `StreamPipeline`'s fields unchanged—e.g. by clamping `max_frames` in `decode_stream_step` and using a small 64-frame chunk loop in `resample_decoded_pcm`, or storing the scratch buffer inside `Resampler`—avoids touching `StreamPipeline` layout at all!).
2. **`LOGIC-202` (`HIGH`, `louder-esp32-app/src/pipeline.rs:514-525`)**:
   - In `StreamPipeline::decode_stream_step`, when `sr > 0` is discovered from `self.decoder.sample_rate()`:
     - If `sr != self.active_sample_rate_hz`, update `self.active_sample_rate_hz = sr;` and `self.retune_equalizer();`.
     - Furthermore, if `self.resampler` is `Some(ref r)` and `r.from_hz() != sr`, reconfigure `self.configure_resampler(sr, r.to_hz())` (or if `sr == r.to_hz()`, `self.clear_resampler()`).
3. **`LOGIC-207` (`LOW`, `louder-esp32-app/src/pipeline.rs:1324-1327`)**:
   - Add a `pub fn flush_resampler_tail(&mut self)` helper (or drain `resampler.flush(...)` into `output_buffer` on stream EOF before `clear_resampler()`), so the FIR group delay tail is not abruptly truncated on track EOF.
4. **`LOGIC-206` (`CRITICAL`, `louder-esp32-app/src/coordinator.rs:650-683`)**:
   - In `PlayerCoordinator::begin_external_playback` and `end_external_playback`:
     - Add a `pub fn end_external_playback_and_restore(&mut self, pipeline: &mut StreamPipeline<S>)` (or in `reinstall_lms_decoder` / `end_external_playback`) so that when external playback preempts an open LMS stream, `pipeline.stream_buffer.flush()` is called to clear any partial compressed frames before raw external PCM is written, and when external playback ends, `reinstall_lms_decoder(pipeline)` restores the active `self.current_format` decoder (`FLAC`/`MP3`/`AAC`/`Vorbis`/`Opus`/`PCM`) and flushes residual external PCM from `stream_buffer`!
5. **`LOGIC-205` (`MEDIUM`, `louder-esp32-app/src/coordinator.rs:1636-1652`)**:
   - In `PlayerCoordinator::handle_command` for `opcode == *b"eqlz"`, replace `if payload.len() >= 10` with `if !payload.is_empty()` so short `eqlz` payloads (`1..=10` band gains) still apply their gains via `pipeline.set_eq_gains_db(&gains)`.
6. **`CONC-203` (`HIGH`, `louder-esp32-app/src/tasks.rs:851-884`)**:
   - In `tasks::bt_poll`:
     - Register Bluetooth audio transitions with `SourceArbiter`: when Bluetooth audio starts (`s.bt_sink.state().avdtp_stream_started()`), call `s.coordinator.request_source(crate::coordinator::AudioSource::Bluetooth)` (which allows Bluetooth `priority 2` to preempt SlimProto `priority 1`, while yielding to Spotify `priority 3` and AirPlay `priority 4`).
     - When Bluetooth audio stops, call `s.coordinator.release_source(crate::coordinator::AudioSource::Bluetooth)` and `s.coordinator.reinstall_lms_decoder(s.pipeline)`.
7. **`LOGIC-603` & `PERF-601` (`HIGH`/`MEDIUM`, `louder-esp32-app/src/tasks.rs:1515-1527`)**:
   - In `tasks::amp_health`, throttle consecutive automatic `recover_from_fault()` calls (e.g., using a `static ConsecutiveFaults` / counter so persistent hard OC/OTSD faults do not hammer `recover_from_fault()` indefinitely without cooldown).

## 3. Verification & Rules
- Verify tests, Clippy, and `google_rust_audit.py --strict` before returning.
- Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo255_cluster5_implementer.md` and update the Fulfillment Section of this Work Order.

## 4. Fulfillment Section

- **Execution Status**: `[EXECUTED & VERIFIED]`
- **Completed Timestamp**: `2026-11-01T10:05:00-05:00`
- **Summary Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo255_cluster5_implementer.md`

### Summary of Actions
1. **`LOGIC-201` & `PERF-201` (`pipeline.rs`)**:
   - Replaced stack allocation `[0i32; 512]` and per-step heap allocation `alloc::vec![0i32; max_out_samples]` in `resample_decoded_pcm` with 64-frame chunk window processing into `pcm_scratch` via `copy_within`.
   - Clamped decoded frames via `max_decode_scratch_samples` so resampled frames never exceed `PCM_SCRATCH_FRAMES`. 0 bytes added to `StreamPipeline` struct.
2. **`LOGIC-202` (`pipeline.rs`)**:
   - Implemented `check_decoder_sample_rate` to retune equalizer and reconfigure or clear resampler when dynamic sample rate transitions occur during streaming.
3. **`LOGIC-207` (`pipeline.rs`)**:
   - Added `flush_resampler_tail` to drain resampler FIR group delay tail into `output_buffer` before `clear_resampler()`.
4. **`LOGIC-206` (`coordinator.rs`)**:
   - Implemented `begin_external_playback_with_pipeline`, `reinstall_lms_decoder`, and `end_external_playback_and_restore` to flush stream buffer and restore LMS decoder matching `current_format`.
5. **`LOGIC-205` (`coordinator.rs`)**:
   - Updated `eqlz` opcode parsing to accept partial `1..=10` band gain payloads.
6. **`CONC-203` (`tasks.rs`)**:
   - Registered Bluetooth audio transitions with `SourceArbiter` in `tasks::bt_poll`.
7. **`LOGIC-603` & `PERF-601` (`tasks.rs`)**:
   - Added fault recovery rate-limiting in `tasks::amp_health` with a 30s cooldown after 3 consecutive failures.

### Verification Results
- `google_rust_audit.py --strict`: `0 error(s), 0 warning(s)`
- `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- test_pipeline`: 12 passed; 0 failed
- `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- coordinator::tests`: 92 passed; 0 failed
- `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --bin louder-esp32-app -- tasks::tests`: 7 passed; 0 failed
- `cargo check --manifest-path rust/Cargo.toml -p louder-esp32-app --lib`: exit code 0
- Background tasks: 0 active tasks
