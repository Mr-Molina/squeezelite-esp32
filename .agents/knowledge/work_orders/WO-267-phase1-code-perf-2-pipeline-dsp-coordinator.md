# Work Order: WO-267 — Phase 1 `CODE-PERF-2`: Audio Pipeline, `StreamBuffer`, `AudioRingBuffer`, DSP & `PlayerCoordinator` Hot-Path Auditor

- **Work Order ID**: `WO-267`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Audio Pipeline, RingBuffer, DSP & Coordinator Hot-Path Auditor (CODE-PERF-2)` (`Model: "pro"`, `TypeName: "research"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{pipeline.rs,tasks.rs,coordinator.rs}`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/{ringbuffer.rs,equalizer.rs,resampler.rs,volume.rs,fade.rs}`
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Read-Only Constraint
Audit `pipeline.rs`, `tasks.rs`, `coordinator.rs`, and `squeezelite-audio-core` for hot-path throughput bottlenecks, `StreamBuffer` / `AudioRingBuffer` copy overhead, `WO-251`–`WO-257` regressions, and per-sample DSP overhead (`PIPE-201`, `PIPE-202`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **`StreamPipeline::decode_stream_step` & `StreamBuffer::peek_contiguous` / `consume` Throughput**:
   - Inspect `pipeline.rs::decode_stream_step` (lines 480–650):
     - How many bytes does `stream_buffer.peek_contiguous()` pass to `self.decoder.decode(encoded, ...)`?
     - What happens when `StreamBuffer` wraps around the end of its ring buffer (e.g., if only 50 bytes remain before the wrap point in `StreamBuffer`, does `MP3`/`FLAC`/`AAC` decode fail with `0` frames decoded and `0` bytes consumed, deadlocking or starving the decoder at the ring wrap boundary?!).
     - Check if `StreamPipeline::decode_stream_step` only decodes **1 MP3 frame (`1,152` samples = `26.1` ms)** per call, or if `PCM_SCRATCH_FRAMES` (`1,152` or `512`?) limits batch decoding!
2. **Did `WO-255` (`check_decoder_sample_rate`, `max_decode_scratch_samples`, `resample_decoded_pcm`, or `retune_equalizer`) Introduce Per-Step Overhead?**:
   - Inspect `git diff e7fa3411..ac281d18 -- rust/crates/louder-esp32-app/src/pipeline.rs rust/crates/louder-esp32-app/src/coordinator.rs rust/crates/louder-esp32-app/src/tasks.rs`.
   - Check whether `check_decoder_sample_rate()` or `retune_equalizer()` is being triggered repeatedly on every decode step (e.g., if `active_sample_rate_hz` or `decoder.sample_rate()` oscillates or triggers transcendentals `libm::sinf`/`cosf`/`powf` on every frame!).
3. **`Equalizer`, `Volume`, and `AudioRingBuffer` Per-Sample Loop Efficiency**:
   - Check whether `Equalizer::process_interleaved` runs even when `eq_preset == Flat` and `loudness_level == 0`, or if `f32` conversions (`i32 -> f32 -> biquad x 10 -> i32`) run unnecessarily.
   - Check `AudioRingBuffer` atomic loads/stores per sample vs per slice in `write_internal` / `read_internal`.
4. **`STAT` Packet `output_buffer_fullness` Reporting (`ANOM-PERF-02`)**:
   - Check why LMS `server.log.1.gz` showed `output fullness -1` in `STAT-STMt`! Trace `coordinator.rs` / `squeezelite-proto` `StatData` serialization of `output_buffer_size` and `output_buffer_fullness`.

## 3. Fulfillment Summary (`c2906e30` — `[AUDIT COMPLETE]`)
- **`PIPE-201` (`HIGH`, `pipeline.rs:566-581, 620-646`)**: `max_decode_scratch_samples()` unconditionally clamps `max_samples` to `2304` for `ActiveDecoder::Mp3(_)`, overriding the FIR upsampling clamp (`527` frames at 44.1->96 kHz). When `resample_decoded_pcm` upsamples 1,152 MP3 frames into `pcm_scratch` (`2,304` samples), `while in_pos < in_end && out_pos < PCM_SCRATCH_SAMPLES` terminates at `out_pos == 2304` and silently drops the remaining ~1,356 resampled frames (>50% of audio) on every MP3 decode step! Fix by buffering unconsumed input frames or decoupling `resample_scratch` / skipping host resampler on native I2S clock rates.
- **`PIPE-202` (`LOW` / Verified)**: `StreamBuffer::peek` (`AudioRingBuffer::peek_at`) linearizes wrap-around reads into `read_scratch[..1728]`.
- **`PIPE-203` (`HIGH`, `coordinator.rs:1348-1350`, `tasks.rs:455, 1481-1486`)**: `PlayerCoordinator::pump_decode` breaks out of its `0..max_steps` loop as soon as a single frame (`available_read() > before`) is produced, capping decoding to 1 MP3 frame (`26.12 ms`) per cooperative ring lap instead of batch-decoding up to `max_steps` (e.g., 4–8 frames) while `output_buffer.available_write() >= 9216`.
- **`PIPE-204` (`LOW` / Verified)**: `check_decoder_sample_rate()` gates `retune_equalizer()` behind `sr != self.active_sample_rate_hz` (0 transcendental calls in steady state).
- **`PIPE-205` (`MEDIUM`, `pipeline.rs:146-152, 709-755`)**: `WRITE_SCRATCH_BYTES = 512` (64 stereo frames = `1.45 ms`) requires 16 loop iterations per `pump_output` call; expanding `WRITE_SCRATCH_BYTES` to `2048` (`OUTPUT_I32_SCRATCH_SAMPLES = 512`, `256` stereo frames = `5.8 ms`) in `.dram2_uninit` `STATIC_PIPELINE` (not on the stack!) cuts `process_output_step` call overhead by 4x.
- **`PIPE-206` (`LOW`, `squeezelite-proto/src/lib.rs:806, 845, 850`, `coordinator.rs:2002-2016`)**: `build_stat_with_metrics` hardcodes `stream_buffer_size: 65536` and `output_buffer_size: 65536` instead of reporting actual runtime PSRAM capacities (`262,144` B and `2,048,000` B).
