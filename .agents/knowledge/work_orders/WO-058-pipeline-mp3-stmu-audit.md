# Work Order `WO-058`: `/deep-code-audit` Micro-Auditor 2 — Audio Stream Pipeline, `StaticMp3Decoder`, `STMd`/`STMu` State Machine, & Sample Format/Rate Audit

- **ID**: `WO-058`
- **Timestamp**: `2026-10-24T17:42:00-04:00`
- **Subagent Role**: Audio Stream Pipeline & Decoder Forensics Auditor (`TypeName: "research"`, `Model: "pro"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Conduct a zero-tolerance `/deep-code-audit` of `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, `src/pipeline.rs`, and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders/src/lib.rs` compared against C `components/squeezelite/slimproto.c`, `decode.c`, `mad.c`, and `output_i2s.c`:
1. **Why `frames_played` stopped at `297,255` (`~6.74s`) and LMS transitioned to `mode: "stop"`**:
   - Trace the exact `STMd` (`DecodeReady`) and `STMu` (`Underrun`) state machine in `rust/crates/louder-esp32-app/src/main.rs` (lines 1400–1750):
     - When `audio_sock` disconnects or closes, or when `stream_buffer` (`8,192` B) and `output_buffer` (`8,192` B) drain during a momentary network stall (`rssi: -82 dBm`), does `main.rs` send `STMu` (`Underrun`), causing LMS to think the track finished and stop playback (`mode: "stop"`)?!
     - Wait: in C `slimproto.c`, `STMu` means **END OF TRACK (Track finished playing after `STMd` EOF)**, NOT a mid-stream network buffer underrun (which is `STMo` = output underrun)!
     - Verify whether `main.rs` sends `STMu` or `STMd` at the wrong time!
2. **Audit `StaticMp3Decoder` & `StreamPipeline` Audio Fidelity, Sample Scaling, & Sample Rate (`44.1 kHz` vs `48 kHz`)**:
   - In `nanomp3-core/src/minimp3.rs:117-121`, `i32::from_synth(x)` multiplies `x` (`±32768`) by `65536.0` (producing full-scale 32-bit signed `i32`, `±2^31`), AND `PcmDecoder` (`squeezelite-decoders/src/pcm.rs` / `lib.rs`) shifts 16-bit PCM left by 16 (`<< 16`, also `±2^31`). Check whether `StaticMp3Decoder`, `Equalizer`, `VolumeScale`, and `EspI2sSink` handle sample scaling, endianness, and channel interleaving identically!
   - Check what happens when an MP3 stream has a `48,000 Hz` or `32,000 Hz` sample rate while `Esp32HardwareI2sDma::set_clock_rate` (`src/main.rs:694-700`) only sets `self.sample_rate_hz = sample_rate_hz` without updating the hardware `I2S0` clock divider!
3. **Audit ICY Metadata (`cont` `metaint` vs HTTP `icy-metaint` header)**:
   - Compare how C `slimproto.c` handles `cont` `metaint` and HTTP response headers against `main.rs` (`audio_header_buf` `1,024` B limit — what if HTTP headers from an Icecast/Shoutcast server exceed `1,024` bytes?!).
4. Return structured findings (`F-PIPE-*`) with exact file/line citations and root-cause proof.

## 2. Mandatory Constraints
- **Read-Only Audit**: Do NOT modify repository files.
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `caece85e-a280-47c7-983b-45d68e8784d2` (`Audio Stream Pipeline & Decoder Forensics Auditor`)
- **Key Findings (`F-PIPE-001`..`F-PIPE-006`)**:
  - `F-PIPE-001`: `main.rs:1868-1979` never emits `STMo` (`OutputUnderrun`) on mid-stream starvation and prematurely emits `STMd` + `STMu` (`TrackUnderrun` = End of Track), causing LMS to transition to `mode: "stop"` and clear the playlist at `297,255` frames (`6.74s`).
  - `F-PIPE-002`: `Esp32HardwareI2sDma::set_clock_rate` (`main.rs:694-700`) is a no-op on hardware `I2S0` registers, leaving `I2S0` fixed at `44.1 kHz` when decoding `48 kHz` or `32 kHz` radio streams.
  - `F-PIPE-003`: `audio_header_buf` (`1,024` B) sets `is_header_complete = true` when full (`audio_header_len >= 1024`), injecting residual ASCII HTTP headers into `stream_buffer`.
  - `F-PIPE-004`: ICY metadata bytes are stripped and discarded without sending `META` packets to LMS.
  - `F-PIPE-005`: Sample scaling (`Q31` 32-bit signed), `VolumeScale`, `Equalizer`, and stereo L/R word swap (`tx_right_first`) are mathematically verified clean.
  - `F-PIPE-006`: `8 KB` `stream_storage` + `8 KB` `output_storage` provides only `23.22 ms` of PCM output headroom, requiring `4 MB` PSRAM expansion.
