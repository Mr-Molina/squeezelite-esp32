# Work Order: WO-090 — Wave 2 Task 5 (`src/pipeline.rs` Multi-Codec Dispatcher, 10-Band EQ & PSRAM Buffers)

## Metadata
- **Work Order ID**: `WO-090`
- **Timestamp**: `2026-10-25T12:35:00-04:00`
- **Assigned Subagent Role**: Wave 2 Task 5 Audio Pipeline Implementer (`Model: "flash"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[DONE]`

## Task Goal
Implement Wave 2 Task 5 in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` per `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-5-brief.md`.

## Target Files & Scope
- Strictly restricted to:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
- Do **NOT** modify `src/amp.rs`, `src/hal.rs`, `src/http_control.rs`, or `src/main.rs`.
- Do **NOT** run `git add` or `git commit`.

## Acceptance Criteria
1. `StreamPipeline<'a>` accepts `(stream_storage: &'a mut [u8], output_storage: &'a mut [u8], pcm_out_buf: &'a mut [i16; 2304], mp3_decoder: &'a mut StaticMp3Decoder, eq: &'a mut Equalizer<10>)`.
2. Implements `ActiveCodec` (`Mp3`, `Flac(FlacDecoder)`, `Aac(AacDecoder)`, `Vorbis(VorbisDecoder)`, `Pcm`), `select_format(u8)`, `set_eq_preset(EqPreset)`, `eq_preset()`, `set_loudness(bool)`, `loudness_enabled()`.
3. Applies `self.eq.process_in_place` before `self.volume.process_in_place` in `step_audio_pipeline` across all codecs and generates unit tests for MP3, PCM, FLAC, AAC, Vorbis, and EQ presets.

## Fulfillment Section
- **Completion Timestamp**: `2026-10-25T12:55:00-04:00`
- **Assigned Implementer**: Wave 2 Task 5 Audio Pipeline Implementer
- **Status**: `[EXECUTED & VERIFIED]`
- **Report Path**: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-5-report.md`
- **Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo090_pipeline_implementer.md`

### Summary of Changes:
1. **`current_eq_preset` & Preset Management**:
   - Added `pub current_eq_preset: EqPreset` to `StreamPipeline<'a, S>`.
   - Initialized to `EqPreset::Flat` in `StreamPipeline::new(...)`.
   - Implemented `apply_eq_preset(&mut self, preset: EqPreset)` and `set_eq_preset(&mut self, preset: EqPreset)`.
   - Implemented `pub const fn eq_preset(&self) -> EqPreset`.
   - Implemented `set_loudness(&mut self, enabled: bool)` and `loudness_enabled(&self) -> bool`.
2. **Codec Selection & Compatibility**:
   - Re-exported `EqPreset` and defined `pub type ActiveCodec = ActiveDecoder;`.
   - Implemented `select_format(&mut self, format_byte: u8)` for MP3 (`'m'`), FLAC (`'f'`), AAC (`'a'`), Vorbis (`'o'`), and PCM (`'p'`).
   - Implemented `step_audio_pipeline(&mut self) -> Result<usize, DecoderError>`.
3. **Pre-Attenuation DSP Ordering**:
   - In `process_decode_step()`, reordered signal processing to apply `self.equalizer.process_interleaved(active_pcm)` directly before `scale_samples_inplace(active_pcm, self.volume)`.
4. **Verification**:
   - Ran `cargo test --manifest-path rust/Cargo.toml -p louder-esp32-app --lib -- pipeline`: 13 passed, 0 failed.
   - Ran `cargo check --manifest-path rust/Cargo.toml -p louder-esp32-app`: passed (exit code 0).
   - Strictly isolated to `src/pipeline.rs`; did not modify `src/amp.rs`, `src/hal.rs`, `src/http_control.rs`, or `src/main.rs`. Zero git staging/commits performed.
