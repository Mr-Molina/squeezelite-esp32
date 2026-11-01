# Work Order `WO-180`: `/deep-code-audit` Phase 1 (`AUD-07`) — Multi-Codec Decoders Part 2 (Native FLAC in PSRAM, AAC & Vorbis) Auditor

- **ID**: `WO-180`
- **Timestamp**: `2026-10-27T17:25:00-04:00`
- **Subagent Role**: `AUD-07` — Multi-Codec Decoders Part 2 (Native FLAC in PSRAM, AAC & Vorbis) Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are **Micro-Auditor `AUD-07`** (`/deep-code-audit` Phase 1 Cohort 1 + `rust-pro`).
Read `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Assigned Files & Line Ranges (STRICT BOUNDARY)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (lines `1401..=2818`)

### Domain Checklist & Hardware / `rust-pro` Invariants to Verify
1. **Native RFC 9639 Lossless `FlacDecoder` in `4 MB` PSRAM, `AacDecoder` & `VorbisDecoder`**:
   - Audit `FlacDecoder` (`lib.rs:1401..`):
     - `fLaC` stream marker and `STREAMINFO` / metadata block skipping across partial chunks.
     - Frame header sync (`0x3FFE`), UTF-8 coded frame/sample number parsing, block size (`16..=65535` vs `MAX_FLAC_BLOCK_SAMPLES = 4608` scratch capacity!), sample rate, channel assignment (Independent, Left-Side, Right-Side, Mid-Side), bit depth (`8, 12, 16, 20, 24, 32`), and CRC-8 header validation.
     - Subframe decoding: Constant, Verbatim, Fixed LPC (`order 0..=4`), and QLP Linear Predictor (`order 1..=32`) — verify 64-bit accumulator arithmetic (`i64`) when `bps == 24` + stereo decorrelation (`bps = 25`) + `qlp_coeff` (`up to 16 bits`), Rice residual partition decoding (4-bit and 5-bit Rice parameters, escape codes, unary quotient overflow/DoS bounds), and wasted-bits-per-sample (`wasted_bits`) shift bounds.
     - Byte alignment, CRC-16 frame footer verification (or resync on corrupt frames), and `stream_buf` / `pending_pcm` compaction.
   - Audit `AacDecoder` and `VorbisDecoder` overflow/pending sample retention and error recovery.
2. **`rust-pro` Google Rust Style Bible (`GRS-001`..`GRS-012`)**:
   - Check `GRS-001`..`GRS-012` compliance across `squeezelite-decoders/src/lib.rs:1401..=2818` (including `GRS-012` line lengths).
3. **Output Format**:
   - Return ONLY a structured YAML `findings:` list matching `/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`. Do NOT modify any files.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `b064c391-73be-45c4-9a1f-5cd8653ce74a`
- **Summary**: Audited `squeezelite-decoders/src/lib.rs:1401..=2818`. Identified 9 findings (`LOGIC-001` `CRITICAL` 4608-sample FLAC block support and `MAX_FLAC_BLOCK_SAMPLES * 2` scratch buffer, `LOGIC-002` spurious `b"fLaC"` scan inside compressed payload, `LOGIC-003` 16-bit QLP precision `2..=16` and `qlp_shift >= 0`, `PERF-001`..`002`, `SEC-001` Rice unary quotient bound, `QA-001`, `LOGIC-004`..`005`).
