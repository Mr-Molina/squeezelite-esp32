# Work Order: WO-252 — Phase 3 Wave 1A Cluster 2 Remediation (`squeezelite-decoders` & `nanomp3-core`)

- **Work Order ID**: `WO-252`
- **Timestamp**: `2026-11-01T09:14:00-05:00`
- **Assigned Subagent Role**: `Cluster 2 Multi-Codec Demuxers & Decoders Remediation Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/layer12.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Disjoint Scope
Remediate all **11 findings** (`LOGIC-301`, `LOGIC-302`, `PERF-301`, `SEC-301`, `SEC-302`, `LOGIC-303`, `LOGIC-304`, `SEC-303`, `LOGIC-305`, `LOGIC-306`, `PERF-303`) exclusively in `squeezelite-decoders/src/lib.rs` and `nanomp3-core/src/minimp3/layer12.rs`. **Do NOT modify any files outside those 2 crates.**

## 2. Findings to Remediate
1. **`LOGIC-301` (`HIGH`, `squeezelite-decoders/src/lib.rs:890-898, 945-948, 1162-1170, 1227-1230`)**:
   - In `Mp3Decoder` and `StaticMp3Decoder`, when computing `max_raw` from `xing.total_frames`, account for the skipped Xing/Info header frame (`let audio_frames = tf.saturating_sub(1) as u64;` when `tf > 0` and the Xing header frame was skipped without incrementing `raw_frames_decoded`, or increment `raw_frames_decoded` / adjust `total_frames` consistently). Update the unit test at `lib.rs:5288-5330` so its synthetic `total_frames` matches real LAME/Xing semantics (`total_frames` = 1 Xing frame + N audio frames, or if `tf` is <= audio frame count, handle both cleanly) and verify `enc_padding` tail samples are trimmed accurately.
2. **`LOGIC-302` (`HIGH`, `squeezelite-decoders/src/lib.rs:2048-2053, 2248-2265`)**:
   - In `FlacDecoder`, if `flush()` is called between consecutive FLAC tracks in a playlist, the next track begins with a new `b"fLaC"` (or `\x7fFLAC`) stream header that may have a different sample rate, channel count, or bit depth. Ensure that whenever new input in `decode()` begins with `b"fLaC"` (or `\x7fFLAC` after `flush()`), `FlacDecoder` replaces the cached 42-byte `STREAMINFO` header (`self.header_len = 0` before re-parsing `b"fLaC"`), while still supporting mid-stream `flush()` followed by raw FLAC audio frames (`0xFF 0xF8`) that reuse the existing `STREAMINFO`! Add a unit test switching between two FLAC streams with different sample rates (e.g., 44.1 kHz -> 48 kHz) across `flush()`.
3. **`PERF-301` (`HIGH`, `squeezelite-decoders/src/lib.rs:2818, 2927-2965, 3813, 3936-3975`)**:
   - In `AacDecoder` (`decode_mp4_stream`) and `AlacDecoder` (`decode`), allow MP4 metadata atoms (`moov`/`stsz` up to `MAX_MP4_SAMPLES = 65_536`, which requires up to `20 + 65536 * 4 = 262,164` bytes) to be buffered while `demux.mdat_remaining == 0 && !demux.in_mdat` by using `const MAX_MP4_META_BUF: usize = 270_336;` prior to entering `mdat` (and keeping `MAX_STREAM_BUF = 65_536` once inside `mdat`), or by streaming `stsz` entries incrementally.
4. **`SEC-301` (`MEDIUM`, `squeezelite-decoders/src/lib.rs:3624-3682, 4395-4451`)**:
   - In `VorbisDecoder` and `OpusDecoder`, validate the RFC 3533 Ogg page CRC-32 (`ogg_crc32(&self.stream_buf[..total_page_len])`) against bytes `22..26` of the Ogg page header (matching `FlacDecoder` at line 2091). Note: if any unit test constructs synthetic Ogg pages with `checksum == 0` or test pages, check all `squeezelite-decoders` unit tests and update synthetic Ogg page helpers in tests to compute `ogg_crc32` (or validate when `expected_crc != 0` / update test page builders to write `ogg_crc32`). Even better: update the test Ogg page builders in `squeezelite-decoders/src/lib.rs` to write valid `ogg_crc32` checksums so strict RFC 3533 CRC-32 validation passes 100% of unit tests!
5. **`SEC-302` (`MEDIUM`, `squeezelite-decoders/src/lib.rs:2113-2117, 3667-3671, 4438-4442`)**:
   - In `FlacDecoder`, `VorbisDecoder`, and `OpusDecoder`, check `if partial_pkt.len().saturating_add(seg_len) > MAX_STREAM_BUF` before `extend_from_slice` when assembling continued Ogg packets across pages, returning `Err(DecoderError::CorruptedFrame)` on overflow.
6. **`LOGIC-303` (`MEDIUM`, `squeezelite-decoders/src/lib.rs:3263-3270`)**:
   - In `AacDecoder::decode`, change `else if encoded_in.len() < 7` to `else if encoded_in.len() < 8` when `self.is_mp4.is_none()` and `!encoded_in.starts_with(&[0xFF, ...])` (so a 7-byte prefix of an MP4 box returns `NeedMoreData` instead of locking `self.is_mp4 = Some(false)`), while still allowing a valid 7-byte ADTS header (`encoded_in.len() == 7 && encoded_in[0] == 0xFF && (encoded_in[1] & 0xF0) == 0xF0`) to lock `self.is_mp4 = Some(false)`.
7. **`LOGIC-304` (`MEDIUM`, `squeezelite-decoders/src/lib.rs:682-688`)**:
   - In `parse_xing_lame_header`, remove the overly broad `|| frame[pos..pos + 4].iter().all(|&b| (0x20..=0x7E).contains(&b))` fallback and restrict to known encoder tags (`b"LAME"`, `b"Lavc"`, `b"Lavf"`, `b"GOGO"`, `b"L3.9"`, `b"XING"`, `b"Info"`).
8. **`SEC-303` (`LOW`, `nanomp3-core/src/minimp3/layer12.rs:237-243`)**:
   - Eliminate the `unsafe` pointer cast in `layer12::decode_frame` by using safe slice flattening without `.unwrap()` (`GRS-010`):
     ```rust
     let Ok(grbuf): Result<&mut [f32; 1152], _> = scratch.grbuf.as_flattened_mut().try_into() else {
         return 0;
     };
     ```
9. **`LOGIC-305` (`LOW`, `squeezelite-decoders/src/lib.rs:2532-2568`)**:
   - In `decode_asc_bytes`, use a bit reader over `asc` to parse `audioObjectType` (5 bits, or 5 + 6 bits if 31), `samplingFrequencyIndex` (4 bits, plus 24 bits if `0x0F`), and `channelConfiguration` (4 bits), so explicit SBR (`audioObjectType == 5`) and explicit 24-bit frequency indices extract the true `channel_configuration`.
10. **`LOGIC-306` (`LOW`, `squeezelite-decoders/src/lib.rs:2120-2128`)**:
    - In `FlacDecoder::ingest_ogg_flac_bytes`, require `self.ogg_partial_pkt.len() >= 51` (instead of `< 13`) for the `\x7fFLAC` identification header packet (1B `\x7f` + 4B `FLAC` + 2B version + 2B header count + 4B `fLaC` + 38B `STREAMINFO` = 51 bytes). Check existing Ogg-FLAC unit tests in `lib.rs` to ensure their identification packet is `>= 51` bytes.
11. **`PERF-303` (`LOW`, `squeezelite-decoders/src/lib.rs:3684-3694, 4453-4467`)**:
    - In `VorbisDecoder::decode` and `OpusDecoder::decode`, break out of the outer Ogg page demuxing `while` loop once `frames_out >= max_frames` so unconsumed pages remain in `self.stream_buf` rather than expanding `self.packet_queue` unboundedly.

## 3. Verification & Rules
- Use `CARGO_TARGET_DIR=/tmp/target-wo252` to avoid `.cargo-lock` contention:
  - `CARGO_TARGET_DIR=/tmp/target-wo252 /home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core`
  - `CARGO_TARGET_DIR=/tmp/target-wo252 /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core --all-targets -- -D warnings`
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src --strict`
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src --strict`
- Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo252_cluster2_implementer.md` and update the Fulfillment Section of this Work Order before returning.
- Ensure `manage_task(Action: "list")` returns zero active background tasks (`Rule 21`) before completing.

## 4. Fulfillment Section
- **Completion Timestamp**: `2026-11-01T09:37:30-05:00`
- **Assigned Implementer**: `Cluster 2 Multi-Codec Demuxers & Decoders Remediation Implementer`
- **Execution Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Artifact**: [`20261101_wo252_cluster2_implementer.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo252_cluster2_implementer.md)

### Remediated Findings Matrix
| Finding ID | Severity | File & Lines | Status | Verification Summary |
|---|---|---|---|---|
| `LOGIC-301` | `HIGH` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | `max_raw` uses `(tf as u64).saturating_sub(1)` for skipped Xing frame; unit tests updated and verified |
| `LOGIC-302` | `HIGH` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Stream switch across `flush()` resets cached 42-byte `STREAMINFO`; `test_flac_stream_switch_across_flush` passed |
| `PERF-301` | `HIGH` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Added `MAX_MP4_META_BUF = 270_336` for pre-mdat atom ingestion; `test_alac_max_stream_buf_overflow` passed |
| `SEC-301` | `MEDIUM` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Added RFC 3533 Ogg CRC-32 check in `VorbisDecoder` & `OpusDecoder`; corrupt page rejected with `CorruptedFrame` |
| `SEC-302` | `MEDIUM` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Added `partial_packet.len().saturating_add(seg_len) > MAX_STREAM_BUF` check across Flac, Vorbis, and Opus |
| `LOGIC-303` | `MEDIUM` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Changed `< 7` to `< 8` when `self.is_mp4.is_none()`; verified in `test_aac_insufficient_data` |
| `LOGIC-304` | `MEDIUM` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Restricted Xing/LAME encoder tags to known vendor identifiers (`LAME`, `Lavc`, `Lavf`, `GOGO`, `L3.9`, `XING`, `Info`) |
| `SEC-303` | `LOW` | `nanomp3-core/src/minimp3/layer12.rs` | `[EXECUTED & VERIFIED]` | Eliminated `unsafe` pointer cast via `scratch.grbuf.as_flattened_mut().try_into()`; zero-unsafe & zero-panic |
| `LOGIC-305` | `LOW` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Implemented `AscBitReader` for AudioSpecificConfig; handles explicit SBR (AOT 5) and 24-bit sample rate |
| `LOGIC-306` | `LOW` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Required `self.ogg_partial_pkt.len() >= 51` for `\x7fFLAC` header packet; verified in Ogg-FLAC tests |
| `PERF-303` | `LOW` | `squeezelite-decoders/src/lib.rs` | `[EXECUTED & VERIFIED]` | Broken out of Ogg demux loop once `frames_out >= max_frames`; updated `has_pending_pcm()` |

### Verification Evidence
- `cargo test --manifest-path rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core`: **Pass (6 + 37 unit tests + 23 integration tests + 1 doc test)**
- `cargo clippy --manifest-path rust/Cargo.toml -p squeezelite-decoders -p nanomp3-core --all-targets -- -D warnings`: **Pass (0 warnings)**
- `python3 google_rust_audit.py ... --strict`: **Pass (0 errors, 0 warnings across both crates)**
