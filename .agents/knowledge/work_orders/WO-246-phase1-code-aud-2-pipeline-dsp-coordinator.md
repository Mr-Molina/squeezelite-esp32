# Work Order: WO-246 — Phase 1 `CODE-AUD-2`: Audio Pipeline, Coordinator, `SourceArbiter`, RBJ Equalizer & Polyphase Resampler Micro-Auditor

- **Work Order ID**: `WO-246`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `Audio Pipeline, Coordinator, SourceArbiter, RBJ Equalizer & Polyphase Resampler Micro-Auditor` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (`3,124` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/coordinator.rs` (`6,436` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs` (`1,685` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/backend_device.rs` (`346` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/sink.rs` (`199` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/{equalizer,resampler,ringbuffer,volume,fade,output,lib}.rs` (`3,965` LOC)
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Micro-Auditor Contract
1. **Role**: `Audio Pipeline, Coordinator, SourceArbiter, RBJ Equalizer & Polyphase Resampler Micro-Auditor` (`CODE-AUD-2`)
2. **Scope**: Strictly the files listed above.
3. **Focus Areas & Pre-Identified Hotspots**:
   - Audit the 9 `unsafe` items in `squeezelite-audio-core/src/ringbuffer.rs` (`AudioRingBuffer`, `Producer`, `Consumer` SPSC memory ordering `Acquire`/`Release` fences and wrap-around slice bounds).
   - Audit `Resampler` (`squeezelite-audio-core/src/resampler.rs`) and its integration in `pipeline.rs` (`configure_resampler`, `clear_resampler`, fixed-point Q30 accumulation, history indexing, and stack footprint).
   - Audit `Equalizer` (`squeezelite-audio-core/src/equalizer.rs`) runtime RBJ `retune`, Fletcher-Munson `loudness_gains_db`, and `eqlz`/`loud` SlimProto opcode handling in `coordinator.rs` & `pipeline.rs`.
   - Audit `SourceArbiter` priority transitions (`AirPlay` > `Spotify` > `Bluetooth` > `SlimProto`) in `coordinator.rs`, including `begin_external_playback` / `end_external_playback` state preservation.
   - Audit `audg` volume curve and balance handling in `squeezelite-audio-core/src/volume.rs` and `coordinator.rs`.
4. **Constraint**: **Read-only code inspection. Do NOT modify any files.**
5. **Output Schema**: Use the standardized 11-field code finding schema (`/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`) with OWASP, CWE, SOC 2, and `GRS-*` compliance tags.

## 2. Deliverables
Return your complete structured findings in your final message.

## 3. Fulfillment Section
- **Status**: `[AUDIT COMPLETE]`
- **Subagent ID**: `58fca3e3-924a-415a-992d-cf013c5cc919`
- **Summary**:
  - Audited `squeezelite-audio-core` and `louder-esp32-app/src/{pipeline,coordinator,tasks,backend_device,sink}.rs` (`15,755` LOC) and reported 11 findings (`4 CRITICAL`, `4 HIGH`, `2 MEDIUM`, `1 LOW`): `CONC-201` (`ringbuffer.rs` `Consumer::rewind_read` SPSC data race), `LOGIC-201` (`pipeline.rs` `resample_decoded_pcm` unconsumed frame drop), `LOGIC-203` (`equalizer.rs` `horner_loudness` sample-amplitude waveshaping), `LOGIC-206` (`coordinator.rs`/`main.rs` external playback preemption decoder/buffer state loss), `CONC-202` (`ringbuffer.rs` `read_pos`/`rewindable` ordering), `PERF-201` (`pipeline.rs` per-step `alloc::vec!`), `LOGIC-202` (dynamic `Resampler` rate reconfiguration), `CONC-203` (`tasks.rs` `bt_poll` `SourceArbiter` bypass), `LOGIC-204` (`Equalizer::retune` history reset click), `LOGIC-205` (`coordinator.rs` `< 10` byte `eqlz`), and `LOGIC-207` (`clear_resampler` tail flush).
