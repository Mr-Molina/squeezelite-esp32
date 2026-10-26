# Subagent Summary: `WO-062` Pure-Rust Firmware Audio, DMA, SlimProto & Web UI Remediation (`--stage-only`)

- **Timestamp**: `2026-10-24T19:43:30-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Parent Agent**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Work Order**: [`WO-062-rust-firmware-audio-remediation.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-062-rust-firmware-audio-remediation.md)
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit**: `41eb03a4` on branch `MK5-Rust`

---

## 1. Files Modified, Created, or Deleted

### Modified
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`:
   - Added static embedded HTML dashboard `DASHBOARD_HTML` served with `Content-Type: text/html; charset=utf-8` on `GET /` and `GET /index.html`.
   - Preserved `GET /status.json` returning JSON schema without regression.
   - Added unit test `serves_html_dashboard_on_root_and_index_html`.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - `F-HW-01` & `F-HW-02`: Fixed `Esp32HardwareI2sDma::pump_silence_if_needed()` to align silence pushes to 1,024-byte DMA descriptor boundaries (`avail % 1024` pad) and maintain at least 4,096 B committed to `Owner::Dma`. Added silence padding in `write_dma()` when `avail >= 6144` to prevent clock starvation.
   - `F-HW-03` & `F-PIPE-002`: Implemented `lms_gain_to_hardware_volume_percent(gain: u32) -> u8` using Q8 fixed-point integer log2 (`gain = 0 -> 0%`, `gain = 1842 -> 55%`, `gain = 65536 -> 100%`). Applied hardware volume directly via `amp.set_volume_percent(vol_pct)` on TAS5805M (`0x4C`) while keeping software PCM scaling at unity (`65536`) to preserve 24-bit dynamic range.
   - `F-HW-04`: In `Esp32HardwareI2sDma::set_clock_rate()`, dynamically reconfigured ESP32 `I2S0_CLKM_CONF` register (`0x3FF4_F0AC`) divider bits (`div_num`, `div_b`, `div_a`) on sample rate transitions (32k/44.1k/48k).
   - `F-STREAM-001` & `F-STREAM-002`: Replaced unsafe buffer-full header termination with a continuous 4-byte sliding window (`\r\n\r\n` / `\n\n`) scanner, preserving unconsumed audio body bytes. Implemented case-insensitive `parse_icy_metaint_header()` to extract metadata intervals directly from headers if `icy_meta_interval == 0`.
   - `F-STREAM-003`: Expanded `slim_tx_buf` and `audio_header_buf` in `FirmwareBssBuffers` to 2,048 bytes. Added `pending_resp_buf` tracking to buffer partial `RESP` writes across loop ticks before interleaving `STAT` frames.
   - `F-STREAM-006` & `F-PIPE-001`: Implemented bounded 4-step continuous decode loop in `run_firmware_tick` whenever `output_buffer.available_write() >= 2304 && (stream_buffer.available_read() > 0 || has_pending_pcm())`. Strictly gated `STMu` (`TrackUnderrun`) on `audio_http_eof == true`, full stream & output buffer drain, and complete `RESP` transmission.
   - `FA-CORE-002`: Added codec validation guarding unsupported compressed formats from falling back to raw PCM static noise (muting sink instead).
   - Expanded Port 80 response buffer to 1,536 bytes to accommodate HTML dashboard payload.
   - Added unit tests `test_lms_gain_to_hardware_volume_percent_curve` and `test_parse_icy_metaint_header`.
3. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-062-rust-firmware-audio-remediation.md`:
   - Updated header to `[EXECUTED & VERIFIED]` and recorded full fulfillment summary.
4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`:
   - Updated `WO-062` row to `[EXECUTED & VERIFIED]`.

### Created
1. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak4` (Rule 35 backup)
2. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak4` (Rule 35 backup)
3. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_http_control.rs.bak4` (Rule 35 backup)
4. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (Staged OTA binary: 731,632 bytes)
5. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261024T192330_rust_audio_remediation.md` (this report)

---

## 2. Rationale for Each Change
- **Descriptor Alignment & Silence Pumping**: `esp-hal` DMA TX buffers only transition descriptor ownership from CPU to DMA once all 1,024 bytes of the descriptor are filled. Pushing unaligned chunks left the descriptor CPU-owned, starving `I2S0` FIFO and causing TAS5805M clock faults (`0x2D` ACK resets). Padding to 1,024-byte multiples guarantees continuous clock transmission.
- **Hardware Volume & Unity PCM**: LMS mixer volume is logarithmic (`0..=65536`). Scaling PCM samples digitally by shifting reduced 24-bit audio to ~10 bits at normal listening levels. Preserving unity PCM scaling (65536) and mapping LMS `audg` to TAS5805M `0x4C` hardware register achieves full 24-bit resolution and clean dynamic range.
- **Dynamic Sample Rate Clocking**: ESP32 `I2S0_CLKM_CONF` divider must be updated at runtime when sample rates switch between 32 kHz, 44.1 kHz, and 48 kHz.
- **Header Parsing & ICY Metadata**: Long HTTP headers (>1,024 B) previously caused premature header completion when `audio_header_buf` filled, leaking HTTP header text into the audio decoder. Continuous sliding window scanning eliminates this failure mode, while direct `icy-metaint:` header parsing ensures metadata is stripped immediately even if LMS delays `cont`.
- **Buffer & RESP Reliability**: Partial TCP socket sends on `RESP` frames would corrupt or interleave with `STAT` frames. Buffering `pending_resp` across ticks and gating `STAT` emission ensures atomic delivery.
- **Continuous Decoding & Proper STMu Semantics**: Output buffers draining to zero before decoding caused audible stuttering. Decoding in a bounded loop while output buffer has space maintains full buffer reserves. Gating `STMu` on `audio_http_eof` prevents premature playback termination on live streams or transient network jitter.

---

## 3. Known Risks & Untested Assumptions
- **Live Over-The-Air Behavior**: In accordance with the mandatory `--stage-only` constraint, the firmware was NOT flashed to live hardware (`172.22.1.15` / `/dev/ttyUSB0`). Live end-to-end audio verification is deferred to `WO-063`.
- **PSRAM Availability**: `STATIC_MP3_DEC` resides in internal SRAM `.bss`. As verified in Gate 4, total internal stack is 36,528 bytes (well above the 34,000 B minimum requirement), leaving ample head room.

---

## 4. Verification Evidence
1. **Gate 1 (`cargo test --workspace`)**:
   - `louder-esp32-app`: 77 passed (lib), 5 passed (main).
   - All workspace crates: 100% PASS with exit code 0.
2. **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**:
   - 0 warnings, 0 errors, exit code 0.
3. **Gate 3 (Xtensa Release Build)**:
   - Target: `xtensa-esp32-none-elf`
   - Build command: `rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`
   - Exit code: 0
4. **Gate 4 (`readelf -S` .stack Size)**:
   - Address: `0x3ffd7150`
   - Size: `0x008eb0` = `36,528` bytes (exceeds `>= 34,000` B threshold by 2,528 bytes).
5. **Gate 5 (`espflash save-image` & `verify_staged_ota_bin.py`)**:
   - Path: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
   - Size: 731,632 bytes (27.23% of `ota_0` partition)
   - SHA-256: `3e4b235dd457b94e0d32a373f2c7e5804d7a2772b11f3a88c51c2845f16108f1`
   - Magic: `0xE9`, App Desc Magic: `0xABCD5432`
   - Project: `"Squeezelite-ESP32"`, Version: `"I2S-4MFlash-Rust"`
   - Output: `[STAGE-ONLY AUDIT PASS]`
