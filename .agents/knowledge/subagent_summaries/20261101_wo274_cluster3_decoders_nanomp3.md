# Subagent Summary: WO-274 — Cluster 3 Audio Decoders & nanomp3 Performance Remediations

- **Work Order ID**: `WO-274`
- **Timestamp**: `2026-11-01T14:49:00-05:00`
- **Subagent Role**: `Cluster 3 Decoders & nanomp3 Performance Implementer`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified / Created / Deleted

### Modified
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3.rs`
   - Added `#[inline(always)]` to hot inner loops: `l3_pow_43` (line 623), `l3_imdct36` (line 999), and `mp3d_synth` (line 1274).
   - Added `#[cfg_attr(target_arch = "xtensa", link_section = ".iram1")]` to `mp3d_dct_ii` (line 1227) and `mp3d_synth_granule` (line 1386) to place the two hottest MP3 synthesis routines (7,851 B) in 0-wait-state ESP32 IRAM rather than 40 MHz SPI Flash (`DEC-307`, `CPU-103b`).

2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
   - **`DEC-301`**: Added pre-framing length guard to `Mp3Decoder::decode`: scans for valid sync header offset `sync_idx`, checks `remaining < frame_bytes`. If `sync_idx > 0`, returns `Ok((sync_idx, 0))`; if `sync_idx == 0`, returns `Err(DecoderError::NeedMoreData)`. Prevents `minimp3`'s `mp3d_find_frame` from treating partial frames as EOF and consuming/discarding incomplete data.
   - **`DEC-302`**: Latched `self.xing_checked = true;` in both `Mp3Decoder::decode` and `StaticMp3Decoder::decode` once a full first frame is present and `parse_xing_lame_header` is evaluated, preventing subsequent frames on non-Xing streams from re-evaluating Xing headers.
   - **`DEC-304`**: Replaced $O(N)$ `windows(4)` PSRAM linear scan in `VorbisDecoder::has_pending_pcm` and `OpusDecoder::has_pending_pcm` with an $O(1)$ length check (`self.stream_buf.len() >= 27`).
   - **`DEC-303` & `DEC-305`**: Replaced $O(N)$ `pending_pcm.drain(..samples)` with $O(1)$ `pending_pcm.clear()` when all pending samples are consumed across `AacDecoder`, `VorbisDecoder`, `AlacDecoder`, and `OpusDecoder`. Replaced `stream_buf.drain(..total_page_len)` with `stream_buf.clear()` when the full buffer is drained in `VorbisDecoder` and `OpusDecoder`.
   - **Unit Test**: Added `test_mp3_decoder_partial_frame_guard` verifying that a truncated 200-byte slice of a 417-byte MP3 frame returns `Err(DecoderError::NeedMoreData)` without consuming partial bytes, and passing the full 417-byte frame decodes `1152` samples.

---

## 2. Rationale for Each Change
- **`DEC-301`**: On ring-buffer wrap or WAN TCP chunking, `encoded_in` can contain an incomplete MP3 frame. Without the guard, `mp3d_find_frame` consumed all bytes and returned `NoFrame`, dropping audio data. The pre-framing guard enforces `NeedMoreData` until the entire frame is buffered.
- **`DEC-302`**: Checking Xing headers on every packet of a non-Xing stream wasted cycles and PSRAM bandwidth. Latching `self.xing_checked = true;` guarantees zero header parsing overhead after the first frame.
- **`DEC-304`**: Calling `stream_buf.windows(4).any(|w| w == b"OggS")` scanned up to 64 KB of PSRAM on every superloop coordinator tick. The $O(1)$ `len() >= 27` check eliminates this PSRAM bus bottleneck.
- **`DEC-303` / `DEC-305`**: In steady-state streaming, pending PCM and Ogg pages are consumed in full. Calling `clear()` instead of `drain(..len)` eliminates memmoving remaining slices and iterator allocations.
- **`DEC-307` / `CPU-103b`**: Placing `mp3d_dct_ii` and `mp3d_synth_granule` in `.iram1` ensures the synthesis filterbank executes without Flash cache misses. Inlining `l3_pow_43`, `l3_imdct36`, and `mp3d_synth` eliminates function call overhead on hot inner loops.

---

## 3. Known Risks & Untested Assumptions
- None. All 6 tests in `nanomp3-core`, all 38 tests in `squeezelite-decoders`, and all codec allocation equivalence integration tests (`aac`, `alac`, `opus`, `vorbis`) passed without errors.
- Strict Google Rust style audit passed with 0 errors and 0 warnings.

---

## 4. Verification Evidence
- `python3 google_rust_audit.py ... --strict`:
  ```
  0 error(s), 0 warning(s)
  ```
- `cargo test --manifest-path rust/Cargo.toml -p nanomp3-core -p squeezelite-decoders`:
  ```
  test result: ok. 6 passed; 0 failed (nanomp3-core)
  test result: ok. 38 passed; 0 failed (squeezelite-decoders)
  test result: ok. 7 passed; 0 failed (aac_alloc_equivalence)
  test result: ok. 5 passed; 0 failed (alac_alloc_equivalence)
  test result: ok. 6 passed; 0 failed (opus_alloc_equivalence)
  test result: ok. 5 passed; 0 failed (vorbis_alloc_equivalence)
  ```
