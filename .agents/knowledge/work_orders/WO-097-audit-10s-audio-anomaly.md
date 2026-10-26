# Work Order: WO-097 — Forensic Diagnosis of the ~10-Second Audio Playback Anomaly on `172.22.1.15`

## Metadata
- **Work Order ID**: `WO-097`
- **Timestamp**: `2026-10-25T13:14:30-04:00`
- **Assigned Subagent Role**: Live Audio Anomaly Forensics Auditor (`Model: "pro"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[IN PROGRESS]`

## Task Goal
Diagnose with 100% empirical certainty why `"BASEMENT"` (`172.22.1.15`) plays ~10 seconds of audio before triggering an audio anomaly.

## Diagnostic Scope & Required Evidence
1. **Live Telemetry Capture (`172.22.1.15/status.json` & LMS `10.0.0.10:9000`)**:
   - Query current `/status.json` on `172.22.1.15` and current player status on `10.0.0.10:9000/jsonrpc.js`.
   - Trigger a clean track/stream restart (or observe over a 20-second window at 250ms–500ms intervals) and log `audio_bytes`, `ring_avail`, `mp3_frames`, `pcm_frames`, `i2s_restarts`, `i2c_nacks`, `strm_count`, `http_200`, `last_err`, `uptime_ms`.
2. **Code Path Forensics in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, `src/pipeline.rs`, `src/amp.rs`, and `squeezelite-codecs/src/mp3.rs`**:
   - **Check 1 — I2S0 Circular DMA `dma_transfer.push()` & Underrun Behavior**:
     - Inspect how `main.rs` pushes decoded PCM bytes into `dma_transfer`.
     - What happens when `dma_transfer.push(&dma_bytes[..byte_len])` returns `Ok(written)` where `written < byte_len` (partial write when the 32 KB circular DMA buffer fills up)? Does `main.rs` silently drop the remaining `byte_len - written` bytes of the decoded MP3 frame (or worse, if `written` is not a multiple of 4, shift L/R channels or byte-swap `i16` MSB/LSB)?
     - What happens when `pipeline` returns `0` decoded samples on a loop tick while `dma_transfer` is running in circular DMA mode? Does `esp-hal` circular DMA replay stale audio from the circular buffer when not fed fast enough, or does `main.rs` pace decoding against `dma_transfer.available()`?
   - **Check 2 — Decoding Pacing vs. 44.1 kHz Real-Time Clock**:
     - Look at how many MP3 frames `main.rs` decodes per second when the HTTP socket has buffered data vs when `dma_transfer.push()` doesn't block!
     - Wait: if `dma_transfer.push()` does NOT block (it just writes into available circular DMA space and returns `Ok(0)` when full, or overwrites), how fast is `pipeline` decoding the HTTP stream?!
     - Calculate: if `main.rs` calls `pipeline.process_decode_step()` every loop iteration WITHOUT waiting for `dma_transfer.available() >= 4608` (one MP3 frame = 1152 stereo samples = 4608 bytes), does `pipeline` decode when `dma_transfer` has `< 4608` bytes free, dropping samples on every frame once the initial buffer fills or draining the entire HTTP buffer at CPU speed?!
   - **Check 3 — 10-Second Periodic Tasks / `AmpManager::periodic_check` / Watchdogs / Sample Rate (`44.1 kHz` vs `48 kHz`)**:
     - Check every timer or tick counter in `main.rs` (`periodic_check`, `STMt`, `STAT`, `i2s_restarts`, etc.).
     - Check what sample rate the current stream on LMS (`10.0.0.10:9000`) actually has (`44.1 kHz` vs `48 kHz`, mono vs stereo) vs the hardcoded `Rate::from_hz(44_100)` I2S clock.

## Acceptance Criteria
1. Cite exact live telemetry numbers from `172.22.1.15/status.json` and exact line numbers in `src/main.rs` / `src/pipeline.rs`.
2. Return a clear, Ranked Root-Cause Forensics Report with exact line references so we can incorporate the fix directly into Wave 3 (`Task 7` in `src/main.rs` / `src/pipeline.rs`).

## Fulfillment Section

- **Fulfillment Status**: `[AUDIT COMPLETE]`
- **Timestamp**: `2026-10-25T13:31:00-04:00`
- **Auditor**: Live Audio Anomaly Forensics Auditor (`WO-097`, `f32d675a-dc76-4b0d-9557-39f4bfc52e46`)
- **Live Telemetry & LMS Ground Truth**:
  - High-frequency (500ms) sampling of `http://172.22.1.15/status.json` showed continuous 44.1 kHz playback (`482,240` frames at `T+0.0s` -> `693,568` frames / `15.727s` at `T+4.6s`), followed by an abrupt reset to `frames: 0` at `T+5.1s`.
  - LMS (`10.0.0.10:9000/jsonrpc.js`) playlist inspection showed the active playlist consists of 16-second DJMix demo WAV tracks (`"title": "DJMix Groove WAV"`, `"duration": 16`, `"bitrate": "1411kbps CBR"`, `"url": "djmix://play?spec=...\"start_sec\":0.0,\"end_sec\":13.448792,\"outro_start_sec\":13.448792..."`) where LMS initiates track transition at `13.44s`.
- **Code-Level Root Causes Identified in `rust/crates/louder-esp32-app/src/main.rs`**:
  1. **Circular DMA 8-Byte Stereo Frame Desynchronization (`src/main.rs:807-821`)**: In `Esp32HardwareI2sDma::write_dma`, `let n = transfer.push(&scratch[..chunk_bytes]); let aligned_n = n - (n % 8);` returns `aligned_n` to the caller when `n % 8 != 0`, leaving the extra `n % 8` bytes committed inside the hardware DMA ring buffer and permanently shifting subsequent 64-bit stereo I2S frames (channel swap or byte-shift digital noise).
  2. **Unaligned Silence Injection (`src/main.rs:734-745`)**: In `pump_silence_if_needed`, `let pad = avail % 1024;` pushes `pad` bytes of silence without enforcing `pad % 8 == 0`, breaking 64-bit stereo frame alignment in the I2S DMA buffer.
  3. **Stale Audio Replay on DMA Underrun Restart (`src/main.rs:712-722`)**: When the 8 KB DMA stream buffer underruns (`transfer.is_done() == true`), `ensure_running()` stops the transfer and calls `i2s_tx.write(buf)`. In `esp-hal 1.2.2` (`dma/buffers/mod.rs:1453`), `write(buf)` defaults `pre_filled` to `buffer.len()` (`8192` B), immediately re-marking all 8 DMA descriptors as owned by hardware and replaying 46.4 ms of stale audio from the old buffer.
  4. **46.4 ms Total Buffer Starvation on Uncompressed 1411 kbps WAV (`src/main.rs:1117, 1159`)**: On the currently flashed firmware (`0e2f4732`), `stream_storage` (`8 KB`) + `output_storage` (`8 KB` = `23.2 ms`) + `tx_buffer` (`8 KB` = `23.2 ms`) provides only `46.4 ms` of audio cushion for 1411 kbps WAV streams.
  5. **Abrupt Pipeline Flush on Track Transition `strm Start` (`src/main.rs:1616-1671`)**: When LMS sends `StreamCommandType::Start` at the `13.44s` transition point, `main.rs` unconditionally calls `pipeline.flush()` and resets `frames_played = 0`, cutting off the tail of the track.
