# Work Order `WO-179`: `/deep-code-audit` Phase 1 (`AUD-06`) — Multi-Codec Decoders Part 1 (PCM, MP3 & `nanomp3-core` Bitstream Engine) Auditor

- **ID**: `WO-179`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-06` — Multi-Codec Decoders Part 1 (PCM, MP3 & Bitstream Engine) Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-06`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (lines `1..=1400`)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/lib.rs` (226 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3.rs` (1,543 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/layer12.rs` (245 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/lanes.rs` (178 lines)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/tests.rs` (148 lines)
*(Skip `nanomp3-core/src/minimp3/tables.rs` unless checking specific constant bounds)*

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **PCM, WAV, AIFF & MP3 Decoding (`squeezelite-decoders` & `nanomp3-core`)**:
   - Audit `PcmDecoder` (16/24/32-bit LE/BE, RIFF WAV header parsing, AIFF `FORM`/`COMM`/`SSND` header parsing, byte alignment on partial chunks).
   - Audit `Mp3Decoder` and `StaticMp3Decoder` (ID3v2 synchsafe header skipping, false-sync recovery, bit reservoir continuity across chunk boundaries, mono-to-stereo upmixing).
   - Audit `nanomp3-core` for slice index bounds, corrupt bitstream safety, and zero dynamic heap allocation.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - `GRS-002`: Disallowed glob imports in `nanomp3-core/src/minimp3.rs:20` (`use self::tables::*;`) and `nanomp3-core/src/tests.rs:1` (`use super::*;`).
   - `GRS-003`: 15 definitions (`struct`, `enum`, `fn`) inside non-leaf `nanomp3-core/src/lib.rs` alongside `mod minimp3;`.
   - `GRS-008`: `unsafe` block at `nanomp3-core/src/minimp3/layer12.rs:230` lacking dual `// JUSTIFICATION:` + `// SAFETY:` comments.
   - `GRS-010`: 7 production `.unwrap()` calls in `nanomp3-core` (`minimp3.rs:1247, 1248, 1276, 1321`, `lanes.rs:72, 155`, `layer12.rs:126`).
   - `GRS-011`: `#[rustfmt::skip]` without justification comment at `nanomp3-core/src/minimp3/layer12.rs:32`.
   - `GRS-012`: Line length >100 cols / comments >80 cols.
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `23f00ae2-6efb-4b84-a273-c5919fe8d8e4`
- **Summary**: Audited `squeezelite-decoders/src/lib.rs:1..=1400` and `nanomp3-core/**`. Identified 11 findings (`LOGIC-001` `StaticMp3Decoder` terminal frame at EOF, `LOGIC-002`..`005` `PcmDecoder` WAV/AIFF chunk validation, `SEC-001` `GRS-008` in `layer12.rs`, `LOGIC-006` `GRS-010`, `LOGIC-007` `GRS-003` in `nanomp3-core/src/lib.rs`, `LOGIC-008` `GRS-002`, `LOGIC-009` `GRS-011`, `LOGIC-010` `GRS-012`).
