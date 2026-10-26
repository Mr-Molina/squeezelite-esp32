# Work Order `WO-062`: Pure-Rust Firmware Audio, DMA, SlimProto & Web UI Remediation (`--stage-only`)

- **ID**: `WO-062`
- **Timestamp**: `2026-10-24T19:23:30-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Remediate all critical live hardware and streaming defects (`F-HW-01`..`04`, `F-STREAM-001`..`006`, `F-PIPE-001`..`002`, `FA-CORE-002`, `FA-CORE-004`) in `rust/crates/louder-esp32-app` and verify the `--stage-only` OTA binary:

1. **Backups First (Rule 35)**:
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` -> `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak4`
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` -> `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak4`
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` -> `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_http_control.rs.bak4`
2. **Fix `F-HW-01` & `F-HW-02` (`I2S0` DMA Descriptor-Aligned `1,024`-Byte Silence Commits in `src/main.rs`)**:
   - In `Esp32HardwareI2sDma::pump_silence_if_needed()` (`src/main.rs:611-622`), `dma_tx_stream_buffer!(8192, 1024)` uses `1,024`-byte descriptors and `esp-hal`'s `advance_tx_stream_descriptors` ONLY flips a descriptor to `Owner::Dma` when all `1,024` bytes are written! Pushing `512` bytes when `available_bytes() >= 6144` left the descriptor owned by `Owner::Cpu` (`0` bytes committed to DMA), causing 19–24 TAS5805M `CLOCK_FAULT` resets (`i2c_acks: 183 -> 280`).
   - Fix `pump_silence_if_needed()` to use a `1,024`-byte zero chunk (or push two `512`-byte chunks per iteration = `1,024` bytes) while `transfer.available_bytes() >= 4096` (so at least `4,096` bytes = 4 full `1,024`-byte descriptors are ALWAYS committed to `Owner::Dma`, whether playing or idle!).
   - Also in `Esp32HardwareI2sDma::write_dma_bytes()`, ensure `pump_silence_if_needed()` runs after writing audio bytes if `available_bytes() >= 6144` (so if audio only had `< 1,024` bytes, silence pads the remainder of the descriptor to guarantee `Owner::Dma` ownership and prevent `BCK`/`WS` clock starvation).
3. **Fix `F-STREAM-003` (`slim_tx_buf` `2,048` B Expansion & Reliable `RESP` Transmission in `src/main.rs`)**:
   - In `FirmwareBssBuffers` (`src/main.rs:743`), expand `slim_tx_buf` from `1,024` B to `2,048` B and `audio_header_buf` from `1,024` B to `2,048` B (`+2 KB` `.bss` total, keeping `.stack > 36.5 KB`).
   - Ensure that when `SlimprotoBuilder::build_resp(&mut resp_buf, &audio_header_buf[..audio_header_len])` builds `RESP`, if `slim_sock.send_slice(&resp_buf[..resp_len])` sends `< resp_len` bytes (partial write), the remaining `resp_buf[sent..resp_len]` bytes are tracked and sent on subsequent loop iterations before any `STAT` frame can interleave or truncate `RESP`!
4. **Fix `F-STREAM-001` & `F-STREAM-002` (HTTP Header Overflow Handling & Direct `icy-metaint:` Header Parsing in `src/main.rs`)**:
   - When reading HTTP response headers from `audio_sock`, do NOT treat `audio_header_len >= audio_header_buf.len()` as `is_header_complete = true` unless `\r\n\r\n` has actually been seen! If headers exceed `2,048` B, keep scanning incoming bytes (tracking a 4-byte sliding window for `\r\n\r\n`) while capping `audio_header_len` at `2,048` B, and only set `is_header_complete = true` (and push post-`\r\n\r\n` audio body bytes into `stream_buffer`) once `\r\n\r\n` is found!
   - When `\r\n\r\n` is found in `audio_header_buf`, parse `icy-metaint:` (case-insensitively, e.g., `icy-metaint:` or `ICY-METAINT:`) directly from the HTTP headers if `icy_metaint == 0`, and set `icy_metaint` and `icy_bytes_until_meta = meta_int` so ICY metadata is stripped even if LMS omits or delays the `cont` frame!
5. **Fix `F-STREAM-006` & `F-PIPE-001` (Continuous Decode Loop & `STMo` vs `STMu` Underrun Semantics in `src/main.rs`)**:
   - In `run_firmware_tick` (`src/main.rs:56-68`), instead of only calling `pipeline.process_decode_step()` when `output_buffer.available_read() == 0`, call `pipeline.process_decode_step()` in a bounded loop (up to 4 iterations per tick) while `pipeline.output_buffer.available_write() >= 2304 && pipeline.stream_buffer.available_read() > 0`! This keeps `output_buffer` continuously full instead of waiting for it to drain to `0`!
   - In `src/main.rs:1954-1979`, fix the `STMd` / `STMu` / `STMo` state machine:
     - `STMd` (`DecodeReady`): Emit once when `audio_http_eof` is `true` AND `pipeline.stream_buffer.available_read() == 0` (meaning all compressed input has been decoded).
     - `STMu` (`TrackUnderrun` = End of Track): ONLY emit `STMu` (and transition `pipeline.is_playing = false`) when **`audio_http_eof == true` AND `pipeline.stream_buffer.available_read() == 0` AND `pipeline.output_buffer.available_read() == 0`**!
     - If `pipeline.output_buffer.available_read() == 0` while `audio_http_eof == false` (the HTTP stream is still open, e.g., live radio or mid-track network jitter), do NOT stop playback and do NOT emit `STMu`! Keep `pipeline.is_playing = true` so `audio_sock` refills and playback seamlessly resumes!
6. **Fix `F-HW-03`, `F-HW-04` & `F-PIPE-002` (Perceptual Hardware Volume on TAS5805M `0x4C`, Unity PCM Scaling, & Dynamic `I2S0` Sample Rate in `src/main.rs`)**:
   - In `SlimprotoFrame::Audg` (`src/main.rs:1299-1325`):
     - LMS sends `audg.gain_left` / `audg.gain_right` on a 50 dB logarithmic curve (`0..=65536`, where `51%` mixer volume is `gain = 1842` = `-31.0 dB` and `100%` is `65536` = `0 dB`).
     - Keep `pipeline.volume.set_gain_q16(65536)` at unity (`1.0x`) when `max_gain > 0` (and `0` when muted/`max_gain == 0`) so software bit-shifting never destroys 24-bit dynamic range, and map `max_gain` to TAS5805M hardware digital volume (`amp.set_volume_percent(vol_pct, &mut i2c)` -> register `0x4C` `DIG_VOL_CTRL`) using a perceptual curve (or integer log2 approximation so `gain = 0 -> 0%`, `gain = 1842 (51% LMS) -> ~55%` on `amp.set_volume_percent` = `-22 dB` on TAS5805M `0x4C`, `gain = 65536 (100% LMS) -> 100%` = `0 dB` `0x30` on TAS5805M `0x4C`)!
   - In `Esp32HardwareI2sDma::set_clock_rate(sample_rate)` (`src/main.rs`), update the ESP32 `I2S0` clock divider (`CLKM_CONF` register at `0x3FF4_F0AC`) when `sample_rate` changes (`32,000`, `44,100`, `48,000` Hz) and call `sink.set_sample_rate(sr)` when `StaticMp3Decoder` reports a decoded frame's `sample_rate` (`F-PIPE-002`).
7. **Fix `FA-CORE-002` & `FA-CORE-004` (Unsupported Codec Guard & Embedded Web UI Dashboard on `GET /` in `src/http_control.rs` and `src/main.rs`)**:
   - Guard the `else` branch in `StreamCommandType::Start` / `SlimprotoFrame::Codc` so unsupported compressed formats do not decode as raw `PcmDecoder` static noise.
   - In `src/http_control.rs` (`handle_http_request`), add support for `GET /` (and `GET /index.html`) returning a clean, self-contained HTML/JS status & control dashboard (`HTTP/1.1 200 OK`, `Content-Type: text/html; charset=utf-8`) that polls `/status.json` every 2 seconds and provides status telemetry + Recovery / Reboot controls, while keeping all existing `/status.json`, `/recovery.json`, `/reboot.json`, and `/reboot_ota.json` tests and behavior 100% intact.
8. **Stage-Only Verification**:
   - Run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`).
   - Run `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`).
   - Build release ELF: `rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`.
   - Verify `.stack` (`0x3FFE0000 - _bss_end`) via `readelf -S` is `>= 34,000` bytes.
   - Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32` and verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` prints `[STAGE-ONLY AUDIT PASS]`.

## 2. Mandatory Constraints
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- Do NOT flash `172.22.1.15` in this work order (`--stage-only` only; `WO-063` will flash after verification).
- Before delivering completion (`send_message`), verify `manage_task(list)` has zero running tasks (Rule 21), write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024T192330_rust_audio_remediation.md`, and append your Fulfillment Summary below (Rule 15 & Rule 17).

## 3. Fulfillment Section
- **Execution Status**: `[EXECUTED & VERIFIED]`
- **Commit**: `41eb03a4` on branch `MK5-Rust`
- **Summary of Remediations Applied**:
  1. `F-HW-01` & `F-HW-02`: Fixed `Esp32HardwareI2sDma::pump_silence_if_needed()` to align silence pushes to 1,024-byte DMA descriptor boundaries (`avail % 1024` pad) and maintain at least 4,096 B committed to `Owner::Dma`. Added silence padding in `write_dma()` when `avail >= 6144` to prevent clock starvation.
  2. `F-HW-03` & `F-PIPE-002`: Implemented `lms_gain_to_hardware_volume_percent(gain: u32) -> u8` using Q8 fixed-point integer log2 (`gain = 0 -> 0%`, `gain = 1842 -> 55%`, `gain = 65536 -> 100%`). Applied hardware volume directly via `amp.set_volume_percent(vol_pct)` on TAS5805M (`0x4C`) while keeping software PCM scaling at unity (`65536`) to preserve 24-bit dynamic range.
  3. `F-HW-04`: In `Esp32HardwareI2sDma::set_clock_rate()`, dynamically reconfigured ESP32 `I2S0_CLKM_CONF` register (`0x3FF4_F0AC`) divider bits (`div_num`, `div_b`, `div_a`) on sample rate transitions (32k/44.1k/48k).
  4. `F-STREAM-001` & `F-STREAM-002`: Replaced unsafe buffer-full header termination with a continuous 4-byte sliding window (`\r\n\r\n` / `\n\n`) scanner, preserving unconsumed audio body bytes. Implemented case-insensitive `parse_icy_metaint_header()` to extract metadata intervals directly from headers if `icy_meta_interval == 0`.
  5. `F-STREAM-003`: Expanded `slim_tx_buf` and `audio_header_buf` in `FirmwareBssBuffers` to 2,048 bytes. Added `pending_resp_buf` tracking to buffer partial `RESP` writes across loop ticks before interleaving `STAT` frames.
  6. `F-STREAM-006` & `F-PIPE-001`: Implemented bounded 4-step continuous decode loop in `run_firmware_tick` whenever `output_buffer.available_write() >= 2304 && (stream_buffer.available_read() > 0 || has_pending_pcm())`. Strictly gated `STMu` (`TrackUnderrun`) on `audio_http_eof == true`, full stream & output buffer drain, and complete `RESP` transmission.
  7. `FA-CORE-002` & `FA-CORE-004`: Added codec validation guarding unsupported compressed formats from falling back to raw PCM static noise (muting sink instead). Served embedded status dashboard on `GET /` and `GET /index.html` with live status telemetry and Recovery / OTA reboot controls.
- **Stage-Only Verification Evidence**:
  - Gate 1: `cargo test --workspace` -> `77 passed; 0 failed` in `louder-esp32-app`, all workspace tests PASS.
  - Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` -> PASS (0 warnings).
  - Gate 3: Xtensa release build -> Finished `release` profile [optimized] target(s) in 3.46s.
  - Gate 4: `.stack` section size -> `readelf -S` shows `.stack` size `0x008eb0` = `36,528` bytes (>= 34,000 B requirement).
  - Gate 5: `espflash save-image` & `verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]` (SHA-256 `3e4b235dd457b94e0d32a373f2c7e5804d7a2772b11f3a88c51c2845f16108f1`, 731,632 bytes, 27.23% of `ota_0`).
