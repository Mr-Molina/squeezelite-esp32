# Work Order: WO-247 — Phase 1 `CODE-AUD-3`: Multi-Codec Container Demuxers (`MP4/esds`, `Ogg-FLAC`, `Xing/LAME` Gapless) & Decoders Micro-Auditor

- **Work Order ID**: `WO-247`
- **Timestamp**: `2026-11-01T08:43:00-05:00`
- **Assigned Subagent Role**: `Multi-Codec Container Demuxers & Vendored Decoders Micro-Auditor` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` (`5,706` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/stream_codec.rs` (`973` LOC)
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/{lib,decoder,minimp3}.rs` & `minimp3/{lanes,layer12,tables}.rs` (`6,451` LOC)
- **Status**: `[AUDIT COMPLETE]`

## 1. Objective & Micro-Auditor Contract
1. **Role**: `Multi-Codec Container Demuxers & Vendored Decoders Micro-Auditor` (`CODE-AUD-3`)
2. **Scope**: Strictly `squeezelite-decoders/src/lib.rs`, `louder-esp32-app/src/stream_codec.rs`, and `nanomp3-core/src/*`.
3. **Focus Areas & Pre-Identified Hotspots**:
   - Audit Phase 2 container demuxers and gapless trim in `squeezelite-decoders/src/lib.rs`:
     - MP4/M4A `ftyp`/`moov`/`trak`/`mdia`/`minf`/`stbl`/`stsd`/`mp4a`/`esds` atom parser and 7-byte ADTS header synthesis (`AudioSpecificConfig` `object_type`, `freq_index`, `chan_config`, atom size integer overflow/underflow bounds, truncated box handling).
     - Ogg-FLAC `"OggS"` page unwrapping, RFC 3533 CRC-32 validation, `\x7fFLAC` 51-byte identification header stripping, and multi-segment lacing table assembly.
     - MP3 `Xing`/`Info`/`LAME` tag parser (`parse_xing_lame_gapless`, 24-bit packed `enc_delay` and `enc_padding` at `0x9D..0x9F`, ID3v2 skip offset, and front/tail sample trimming in `Mp3Decoder` and `StaticMp3Decoder`).
   - Audit the 1 `unsafe` block in `nanomp3-core/src/minimp3/layer12.rs:241` for alignment, initialization, and bounds safety.
   - Audit `FlacDecoder`, `AacDecoder`, `VorbisDecoder`, `AlacDecoder`, and `OpusDecoder` stack footprint and PSRAM buffer lifecycle on track switch.
4. **Constraint**: **Read-only code inspection. Do NOT modify any files.**
5. **Output Schema**: Use the standardized 11-field code finding schema (`/home/console-admin/.gemini/config/skills/deep-code-audit/references/finding-schema.md`) with OWASP, CWE, SOC 2, and `GRS-*` compliance tags.

## 2. Deliverables
Return your complete structured findings in your final message.

## 3. Fulfillment Section
- **Status**: `[AUDIT COMPLETE]`
- **Subagent ID**: `f125b7e8-ca69-442f-a757-2823b6ac61d8`
- **Summary**:
  - Audited `squeezelite-decoders`, `stream_codec.rs`, and `nanomp3-core` (`13,130` LOC) and reported 11 unique findings (`3 HIGH`, `4 MEDIUM`, `4 LOW`): `LOGIC-301` (`Mp3Decoder`/`StaticMp3Decoder` Xing frame count off-by-one in `max_raw`), `LOGIC-302` (`FlacDecoder::flush()` preserving prior `header_len = 42`), `PERF-301` (MP4 `stsz` atom vs `MAX_STREAM_BUF = 65536`), `SEC-301` (missing `ogg_crc32` in `VorbisDecoder` and `OpusDecoder`), `SEC-302` (unbounded Ogg continued packet accumulation), `LOGIC-303` (`AacDecoder` 7-byte initial MP4 chunk misclassified as ADTS), `LOGIC-304` (false-positive LAME tag match on arbitrary 4-byte ASCII), `SEC-303` (`nanomp3-core` `layer12.rs:241` replaceable `unsafe`), `LOGIC-305` (explicit SBR AOT 5 `decode_asc_bytes`), `LOGIC-306` (Ogg-FLAC `< 51` B header check), and `PERF-303` (bounded Ogg page demux under backpressure).
