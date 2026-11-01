# Work Order: WO-225 — Wave 6B: Phase 2 Codec Completion — MP4/M4A AAC Demux, Ogg-FLAC Container, & MP3 LAME/Xing Gapless Trim (`squeezelite-decoders` & `nanomp3-core`)

- **Work Order ID**: `WO-225`
- **Timestamp**: `2026-10-31T23:48:30-04:00`
- **Assigned Subagent Role**: `Rust Codec Container & Gapless Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/` & `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Complete all remaining Phase 2 items in `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (§Phase 2):
1. **AAC in MP4/M4A container (`esds`/`stsc`/`stsz`/`stco`/`mdat`)**: Extend `AacDecoder` in `squeezelite-decoders` so it automatically detects ISO-BMFF (`ftyp`/`moov`/`mdat` with `mp4a` sample entry and `esds` AudioSpecificConfig) in addition to raw ADTS streams, extracting AAC access units and feeding them with a synthesized 7-byte ADTS header (or direct config) into `syom`.
2. **Ogg-FLAC wrapper for native `FlacDecoder`**: Extend `FlacDecoder` in `squeezelite-decoders` so that when the stream begins with an Ogg page (`"OggS"` with payload `\x7fFLAC\x01\x00`), it transparently unwraps Ogg page segments into the native RFC 9639 `fLaC` bitstream buffer (`stream_buf` in PSRAM).
3. **MP3 LAME/Xing gapless trim (`enc_delay` / `enc_padding`)**: Add LAME/Xing/Info header inspection (`"Xing"` / `"Info"` tag + 24-bit LAME encoder delay/padding at offset `+21` after `"LAME"`/`"Lavc"`/`"Lavf"`) to `Mp3Decoder` in `squeezelite-decoders` (and/or `nanomp3-core`), trimming the leading `enc_delay + 529` decoder-delay stereo frames and trailing `enc_padding` frames at EOF!
4. **Memory Budget Enforcement (`DecodeError::UnsupportedFeature`)**: Enforce PSRAM stream/frame bounds in decoders so streams exceeding supported channel/sample-rate/buffer ceilings return a deterministic `DecodeError::UnsupportedFeature` (allowing `PlayerCoordinator` to emit `STMn`).

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly:
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/**`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/**`
   - Do **NOT** modify `squeezelite-audio-core`, `squeezelite-config`, `squeezelite-raop`, `squeezelite-cspot`, `louder-esp32-app`, or `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]`, single-module `use` (`GRS-001`), `#[non_exhaustive]` on public enums/structs (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
   - Note: If `squeezelite-decoders/src/lib.rs` is large, you may organize new container helpers into clean leaf modules (`src/mp4_aac.rs`, `src/ogg_flac.rs`, `src/mp3_xing.rs`) or keep `GRS-003` strictly satisfied if adding submodules! (Remember `GRS-003`: if a crate has any `mod foo;` in `src/lib.rs`, then `src/lib.rs` MUST contain ONLY `mod` declarations and `pub use` re-exports! Check whether `squeezelite-decoders/src/lib.rs` currently has `mod` declarations or is a single leaf `lib.rs` file before deciding.)
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. Inspect `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/`.
2. Implement:
   - **MP4/M4A (`mp4a` / `esds`) AAC container demuxer** in `AacDecoder`: parse `moov`/`trak`/`mdia`/`minf`/`stbl` (`esds` AudioSpecificConfig -> profile, sample_rate_index, channel_config; `stsz` sample sizes; `stsc`/`stco` chunk offsets) and wrap each `mdat` AAC access unit in a 7-byte ADTS header so `syom` decodes M4A AAC streams seamlessly alongside raw ADTS streams.
   - **Ogg-FLAC (`OggS` + `\x7fFLAC`) demuxer** in `FlacDecoder`: detect `"OggS"` magic, verify CRC-32 / segment table, strip the 9-byte `\x7fFLAC\x01\x00..` Ogg-FLAC mapping header before `"fLaC"`, and feed unwrapped FLAC metadata/audio packets into the native RFC 9639 `FlacDecoder`.
   - **MP3 Xing/Info/LAME gapless header parser & sample trimmer** in `Mp3Decoder`: inspect the first MPEG frame for `"Xing"` or `"Info"` + LAME 24-bit `enc_delay` (12 bits) and `enc_padding` (12 bits), skip the silent Xing header frame itself, trim `enc_delay` leading stereo frames, and expose `enc_delay_frames(&self) -> u16` / `enc_padding_frames(&self) -> u16` / `total_frames(&self) -> Option<u32>`.
3. Add comprehensive unit tests in `squeezelite-decoders` verifying:
   - MP4/M4A `mp4a`/`esds` demuxing and ADTS synthesis + decoding.
   - Ogg-FLAC container unwrapping + bit-exact decoding parity against native FLAC.
   - MP3 Xing/LAME header parsing (`enc_delay`, `enc_padding`, silent Xing frame skip, and leading/trailing sample trim).
4. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core` (`0` failures) and `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src /home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src --strict` (`0 error(s), 0 warning(s)`).
5. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo225_wave6b_codecs_phase2.md`.

---

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo225_wave6b_codecs_phase2.md`
- **Deliverables Completed**:
  1. **ISO-BMFF MP4/M4A (`ftyp`/`moov`/`esds`/`stsc`/`stsz`/`stco`/`co64`/`mdat` for `mp4a`) AAC Demux & ADTS Header Synthesis (`[EXECUTED & VERIFIED]`)**:
     - Implemented `AacAudioSpecificConfig`, `parse_esds_audio_specific_config`, `synthesize_adts_header`, and lazy-allocated `AacMp4Demuxer` inside `AacDecoder` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`).
     - Automatically distinguishes ISO-BMFF streams (`ftyp`/`moov`/`styp`/`free`/`skip`/`wide`/`mdat`) from raw ADTS (`0xFF 0xF0..0xF9`) streams, walks `moov`/`trak`/`mdia`/`minf`/`stbl`/`stsd`/`mp4a` (`v0`/`v1`/`v2` + `wave`)/`esds` (`0x03` -> `0x04` -> `0x05`), parses `stsz`, `stsc`, `stco`, and `co64` tables with inter-chunk gap skipping, synthesizes a 7-byte MPEG-4 ADTS header (`protection_absent = 1`) around each `mdat` access unit, and feeds `syom::Decoder`.
  2. **Ogg-FLAC (`"OggS"` + `\x7fFLAC\x01\x00`) Container Unwrapping & RFC 3533 CRC-32 (`[EXECUTED & VERIFIED]`)**:
     - Implemented `pub const fn ogg_crc32(page: &[u8]) -> u32` (RFC 3533 direct polynomial `0x04C11DB7`, zeroing checksum field at `22..26`) and `FlacDecoder::ingest_ogg_flac_bytes` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`.
     - Automatically detects `"OggS"` magic vs native `"fLaC"`, validates every Ogg page's CRC-32 and lacing segment table, strips the 9-byte `\x7fFLAC\x01\x00..` mapping header from the initial BOS packet before `"fLaC"`, and feeds unwrapped metadata/audio packets into `self.stream_buf` for bit-exact RFC 9639 decoding.
  3. **MP3 Xing/Info/LAME (`enc_delay` & `enc_padding`) Gapless Parsing & Trimming (`[EXECUTED & VERIFIED]`)**:
     - Implemented `XingLameInfo`, `parse_xing_lame_header`, and `apply_mp3_gapless_trim` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` and wired into both `Mp3Decoder` and `StaticMp3Decoder`.
     - Parses `"Xing"` / `"Info"` headers across MPEG-1/2/2.5 mono/stereo side-info offsets and 24-bit LAME `enc_delay` (12 bits MSB) + `enc_padding` (12 bits LSB) at offset `+21` after `"LAME"`/`"Lavc"`/`"Lavf"`/`"GOGO"`.
     - Skips the silent Xing/Info header frame (`0` PCM frames emitted), trims `enc_delay` leading stereo frames (plus optional `+529` via `with_decoder_delay(true)`), trims `enc_padding` trailing stereo frames at EOF, and exposes `enc_delay_frames(&self) -> u16`, `enc_padding_frames(&self) -> u16`, `total_frames(&self) -> Option<u32>`, and `xing_info(&self) -> Option<XingLameInfo>`.
  4. **Memory Budget & Unsupported Feature Ceilings (`[EXECUTED & VERIFIED]`)**:
     - Exported `pub type DecodeError = DecoderError;`, `DecoderError::UnsupportedFeature` (alias constant for `DecoderError::UnsupportedSampleFormat` so `PlayerCoordinator` emits `STMn`), `MAX_MP4_SAMPLES = 65536`, `MIN_SAMPLE_RATE = 8000`, and `MAX_SAMPLE_RATE = 192_000`.
     - Enforced channel (`1..=2`), sample rate (`8_000..=192_000` Hz), block size (`<= 4608`), and sample table (`<= 65536`) bounds across decoders.
- **Verification Evidence**:
  - `cargo test -p squeezelite-decoders -p nanomp3-core`: 34 unit tests + 23 integration tests + 6 `nanomp3-core` unit tests + 1 doc-test passed (`0` failures, exit code `0`).
  - `cargo clippy -p squeezelite-decoders -p nanomp3-core --all-targets -- -D warnings`: `0` warnings (exit code `0`).
  - `google_rust_audit.py .../squeezelite-decoders/src .../nanomp3-core/src --strict`: `0 error(s), 0 warning(s)` (exit code `0`).
