# Subagent Summary: WO-252 — Cluster 2 Multi-Codec Demuxers & Decoders Remediation

- **Timestamp**: `2026-11-01T09:37:00-05:00`
- **Assigned Subagent Role**: `Cluster 2 Multi-Codec Demuxers & Decoders Remediation Implementer`
- **Work Order ID**: `WO-252`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified, Created, or Deleted

### Modified
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/layer12.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`

### Created / Deleted (Temporary)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3/layer12.rs.bak_wo252` (Created for pre-modification backup, deleted post-verification)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs.bak_wo252` (Created for pre-modification backup, deleted post-verification)

---

## 2. Rationale for Each Change

1. **`LOGIC-301` (`squeezelite-decoders/src/lib.rs`)**:
   - In `Mp3Decoder` and `StaticMp3Decoder`, when computing `max_raw` from `xing.total_frames`, account for the skipped Xing/Info header frame via `let audio_frames = (tf as u64).saturating_sub(1);` before computing `audio_frames * (xing.samples_per_frame as u64) - enc_padding`. Updated unit test `test_mp3_xing_lame_header_parse_and_gapless_trim` to match real LAME/Xing semantics where `total_frames` is 4 (1 Xing header + 3 audio frames).

2. **`LOGIC-302` (`squeezelite-decoders/src/lib.rs`)**:
   - In `FlacDecoder::decode`, when `self.header_len > 0` and incoming input begins with a new stream identifier (`b"fLaC"`, `b"\x7fFLAC"`, or `b"OggS"`), clear `self.stream_buf`, reset `self.header_len = 0`, and set `self.is_ogg_flac = None`, allowing clean reconfiguration across track boundaries. Preserved cached `STREAMINFO` when resuming mid-stream with raw FLAC audio frames (`0xFF 0xF8`). In `ingest_ogg_flac_bytes`, clear cached `stream_buf` and reset `header_len` when an Ogg-FLAC `\x7fFLAC` header packet is detected. Added unit test `test_flac_stream_switch_across_flush` switching 44.1 kHz -> 48 kHz across `flush()`.

3. **`PERF-301` (`squeezelite-decoders/src/lib.rs`)**:
   - Defined `pub const MAX_MP4_META_BUF: usize = 270_336;` to allow large MP4 metadata atoms (`moov`/`stsz` up to `MAX_MP4_SAMPLES = 65_536`) to be buffered prior to entering `mdat`. In `AacDecoder::decode_mp4_stream` and `AlacDecoder::decode`, used `MAX_MP4_META_BUF` while in `SearchingAtoms` / before `ReadingMdat`, reverting to `MAX_STREAM_BUF = 65_536` once in `ReadingMdat`. Updated `test_alac_max_stream_buf_overflow` to verify the metadata ceiling.

4. **`SEC-301` (`squeezelite-decoders/src/lib.rs`)**:
   - In `VorbisDecoder::decode` and `OpusDecoder::decode`, added RFC 3533 Ogg page CRC-32 validation via `ogg_crc32(&self.stream_buf[..total_page_len])` against bytes `22..26`. Corrupted pages are drained and rejected with `Err(DecoderError::CorruptedFrame)`. Added unit test `test_ogg_vorbis_and_opus_crc_and_overflow_protection`.

5. **`SEC-302` (`squeezelite-decoders/src/lib.rs`)**:
   - In `FlacDecoder::ingest_ogg_flac_bytes`, `VorbisDecoder::decode`, and `OpusDecoder::decode`, added `if partial_packet.len().saturating_add(seg_len) > MAX_STREAM_BUF` guards before assembling continued packets across pages, draining corrupted buffers and returning `Err(DecoderError::CorruptedFrame)`.

6. **`LOGIC-303` (`squeezelite-decoders/src/lib.rs`)**:
   - In `AacDecoder::decode`, changed `else if encoded_in.len() < 7` to `else if encoded_in.len() < 8` when determining MP4 container vs ADTS stream, preventing 7-byte prefixes of MP4 boxes from prematurely locking `self.is_mp4 = Some(false)`. Updated `test_aac_insufficient_data` to verify 7-byte non-ADTS inputs return `NeedMoreData` without locking `is_mp4`.

7. **`LOGIC-304` (`squeezelite-decoders/src/lib.rs`)**:
   - In `parse_xing_lame_header`, replaced the loose printable ASCII check with a strict check against known encoder tags (`b"LAME"`, `b"Lavc"`, `b"Lavf"`, `b"GOGO"`, `b"L3.9"`, `b"XING"`, `b"Info"`).

8. **`SEC-303` (`nanomp3-core/src/minimp3/layer12.rs`)**:
   - Eliminated the `unsafe` pointer cast in `layer12::decode_frame` using safe slice flattening without `.unwrap()` (`GRS-010`):
     ```rust
     let Ok(grbuf): Result<&mut [f32; 1152], _> =
         scratch.grbuf.as_flattened_mut().try_into()
     else {
         return 0;
     };
     ```

9. **`LOGIC-305` (`squeezelite-decoders/src/lib.rs`)**:
   - In `decode_asc_bytes`, implemented `AscBitReader` to parse `audioObjectType` (5 bits, or 5+6 if 31), `samplingFrequencyIndex` (4 bits, plus 24 bits if `0x0F`), and `channelConfiguration` (4 bits). Properly extracts channel configuration and resolves explicit SBR (`audioObjectType == 5`) and explicit 24-bit frequency indices. Added unit test `test_decode_asc_bytes_explicit_frequency_and_sbr`.

10. **`LOGIC-306` (`squeezelite-decoders/src/lib.rs`)**:
    - In `FlacDecoder::ingest_ogg_flac_bytes`, required `self.ogg_partial_pkt.len() >= 51` (was `< 13`) for the `\x7fFLAC` identification packet (1B `\x7f` + 4B `FLAC` + 2B version + 2B header count + 4B `fLaC` + 38B `STREAMINFO` = 51 bytes).

11. **`PERF-303` (`squeezelite-decoders/src/lib.rs`)**:
    - In `VorbisDecoder::decode` and `OpusDecoder::decode`, added `if frames_out >= max_frames { break; }` to break out of the outer Ogg page demuxing loop early so unconsumed pages remain in `self.stream_buf` instead of expanding `self.packet_queue` unboundedly. Updated `has_pending_pcm` in both decoders to check `self.stream_buf.windows(4).any(|w| w == b"OggS")`.

---

## 3. Known Risks or Untested Assumptions
- None. All 11 findings were tested and verified against their specific test cases as well as full regression test suites.
- Strict isolation maintained: No files outside `squeezelite-decoders` and `nanomp3-core` were modified.

---

## 4. Verification Evidence
- `cargo test -p squeezelite-decoders -p nanomp3-core`: **Pass (6 nanomp3 tests, 37 squeezelite tests, 23 integration tests, 1 doc test: 100% OK)**
- `cargo clippy -p squeezelite-decoders -p nanomp3-core --all-targets -- -D warnings`: **Pass (0 warnings)**
- `google_rust_audit.py ... --strict`: **Pass (0 errors, 0 warnings)**
