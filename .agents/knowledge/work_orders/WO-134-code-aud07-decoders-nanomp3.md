# Work Order `WO-134`: `/deep-code-audit` Phase 1 Micro-Auditor `AUD-07` — Audio Multi-Codec Decoders & MP3 Bitstream Auditor

- **ID**: `WO-134`
- **Timestamp**: `2026-10-25T20:44:00-04:00`
- **Subagent Role**: Audio Multi-Codec Decoders & MP3 Bitstream Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Code Micro-Auditor Contract)
1. **Role**: Audio Multi-Codec Decoders & MP3 Bitstream Auditor (`AUD-07`)
2. **Scope**:
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (`1,876` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/lib.rs` (`226` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3.rs` (`1,541` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/layer12.rs` (`245` LOC)
   - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/lanes.rs` (`178` LOC)
   *(Skip reading `tables.rs` static lookup tables unless checking bounds).*
3. **Context**: Multi-codec audio decoder suite (`PcmDecoder` for raw PCM/WAV/AIFF, `StaticMp3Decoder` zero-heap `.bss` MP3 decoder, `FlacDecoder`, `AacDecoder`, `VorbisDecoder`) and `nanomp3-core` bare-metal MPEG-1/2/2.5 Layer I/II/III bitstream decoder.
4. **Static Hotspots (`WO-115`)**:
   - `unsafe` pointer cast blocks in `nanomp3-core/src/minimp3.rs:1098` and `src/minimp3/layer12.rs:230` (`&mut *(grbuf.as_mut_ptr() as *mut [f32; 576])`).
   - Check `StaticMp3Decoder` bit-reservoir / false-sync handling on 320 kbps CBR/VBR streams, `PcmDecoder` WAV/AIFF chunk parsing bounds on malformed headers, and `FlacDecoder`/`AacDecoder`/`VorbisDecoder` heap allocation bounds on `xtensa` PSRAM.
5. **Seeds**: Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md` (`Input Boundaries & Parsing`, `Memory, Resource & Concurrency Safety`, `Error Handling & Edge Cases`).
6. **Constructive Framing (`GLB-016`)**: Perform a standard code quality, robustness, and maintainability review of the assigned module. Identify unhandled edge cases, resource cleanup gaps, input validation improvements, and type-safety enhancements, and provide standard remediation snippets.
7. **Constraint**: Phase 1 — Read-only analysis. Do NOT edit any files or run mutating commands. **NEVER touch `/dev/ttyUSB0`**. Keep within `<= 30` tool calls.
8. **Output Schema**: Report all findings using the exact 11-field schema in `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
