# Work Order `WO-178`: `/deep-code-audit` Phase 1 (`AUD-05`) — Audio DSP Pipeline, 10-Band Biquad EQ & Lock-Free SPSC Ringbuffer Auditor

- **ID**: `WO-178`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-05` — Audio DSP Pipeline, 10-Band EQ & Ringbuffer Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-05`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (all 1,140 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/lib.rs` (19 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/ringbuffer.rs` (521 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs` (866 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/output.rs` (130 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/volume.rs` (117 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/tests/stress_test.rs` (175 lines)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Audio Pipeline, DSP & Lock-Free Concurrency**:
   - Audit `StreamPipeline` (`pipeline.rs`): codec switching (`Pcm`, `Mp3`, `StaticMp3`, `Flac`, `Aac`, `Vorbis`), `process_decode_step`, corrupt frame resync, EQ sample rate updates, and output buffer draining.
   - Audit `AudioRingBuffer` (`ringbuffer.rs`): `UnsafeCell` aliasing, `AtomicUsize` `Ordering::Acquire`/`Ordering::Release` memory barriers across `PRO_CPU` and `APP_CPU`, wrap-around boundary math, and `rewind`/`clear`.
   - Audit `Equalizer10Band` (`equalizer.rs`) and `VolumeControl` (`volume.rs`): fixed-point/floating-point saturation, NaN/Inf guards, and Horner loudness compensation.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-008`: All 9 `unsafe` blocks/impls in `squeezelite-audio-core/src/ringbuffer.rs` (lines `35, 39, 145, 149, 224, 232, 275, 319, 327`) lack the mandatory dual `// JUSTIFICATION:` + `// SAFETY:` comments!
   - `GRS-012`: Code lines >100 cols / comments >80 cols.
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `5e36abd5-49e7-4e6e-bf72-bde54e3d9dbd`
- **Summary**: Audited `louder-esp32-app/src/pipeline.rs` and `squeezelite-audio-core/**`. Identified 8 findings (`CONC-005` `rewind_read_internal` ordering, `LOGIC-005` `horner_loudness` volume parameter vs sample amplitude, `LOGIC-006`..`008`, `PERF-002` `READ_SCRATCH_BYTES = 1728` on `xtensa`, `STYLE-001` `GRS-008` in `ringbuffer.rs`, `STYLE-002` `GRS-012`).
