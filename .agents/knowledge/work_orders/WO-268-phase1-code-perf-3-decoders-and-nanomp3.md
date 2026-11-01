# Work Order: WO-268 — Phase 1 `CODE-PERF-3`: Audio Decoders (`squeezelite-decoders`, `nanomp3-core`, Vendored Codecs) CPU, Memory & Copy Bottleneck Auditor

- **Work Order ID**: `WO-268`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Audio Decoders, Demuxers & Memory Scratch Allocation Auditor (CODE-PERF-3)` (`Model: "pro"`, `TypeName: "research"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/**`
  - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/{alac,lewton,opus-rs,syom}/**`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Read-Only Constraint
Audit `squeezelite-decoders`, `nanomp3-core`, and vendored codecs for per-frame CPU overhead, `Vec::drain` / `copy_within` O(N) memmove storms in PSRAM, `WO-252` regressions, and Flash/PSRAM cache thrashing (`DEC-301`, `DEC-302`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **`Mp3Decoder` / `StaticMp3Decoder` (`squeezelite-decoders/src/lib.rs`) & `WO-252` Diff Inspection**:
   - Run `git diff cd689da1..ac281d18 -- rust/crates/squeezelite-decoders/src/lib.rs rust/crates/nanomp3-core/` and `git diff e7fa3411..ac281d18 -- rust/crates/squeezelite-decoders/src/lib.rs`.
   - Trace `Mp3Decoder::decode` and `StaticMp3Decoder::decode` line by line:
     - Does `parse_xing_lame_header(&encoded_in[..])` scan the entire `encoded_in` slice on EVERY decode call when `self.xing_checked` is false or across frames?
     - How does `Mp3Decoder` buffer or consume `encoded_in`? When `StreamBuffer::peek_contiguous` returns a partial MP3 frame at the end of the ring buffer (or `< 1,440` bytes), what does `nanomp3_core::Mp3Decoder::decode` return (`0` samples, `consumed = 0` or `consumed = encoded_in.len()`)?!
     - WAIT: Check what happens when `StreamBuffer` has e.g. `400` bytes at the end of its backing array and `60,000` bytes wrapped at index `0`! Does `nanomp3` see only `400` bytes from `peek_contiguous()`, fail to find a complete frame, and either drop the frame (`consumed = 400`) or stall?!
2. **PSRAM `Vec::drain(..consumed)` O(N) Memmove Storms Across All Decoders (`FlacDecoder`, `AacDecoder`, `VorbisDecoder`, `AlacDecoder`, `OpusDecoder`)**:
   - Inspect how `self.stream_buf.drain(..consumed)` or `self.pending_pcm.drain(..)` is used in `FlacDecoder`, `AacDecoder`, `VorbisDecoder`, `AlacDecoder`, and `OpusDecoder`. Shifting a 64 KB–270 KB `Vec<u8>` in 40 MHz SPI PSRAM via `drain(..consumed)` on every 1 KB frame copies megabytes per second over the SPI1 PSRAM bus!
   - Also check `WO-252`'s `has_pending_pcm` change in `VorbisDecoder` / `OpusDecoder`: `self.stream_buf.windows(4).any(|w| w == b"OggS")` — does that scan 64 KB of PSRAM on every check?!
3. **`nanomp3-core` Hot-Path Tables & Synthesis Filterbank**:
   - Inspect `nanomp3-core/src/minimp3.rs` (`l3_decode`, `l3_imdct36`, `synth_granule`) for stack/PSRAM access patterns and inlining opportunities.

## 3. Deliverable
Return a structured list of findings (`DEC-301`, `DEC-302`, ...) with exact `file:line` citations, byte-copy/cycle math, and concrete code remediations.
