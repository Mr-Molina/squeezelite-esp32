# Work Order: WO-266 — Phase 1 `CODE-PERF-1`: Bare-Metal Runtime, Dual-Core (`PRO_CPU` / `APP_CPU`) Split, Main Loop Cadence & IRAM Placement Auditor

- **Work Order ID**: `WO-266`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Bare-Metal Runtime, CPU Cores & IRAM Placement Auditor (CODE-PERF-1)` (`Model: "pro"`, `TypeName: "research"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{main.rs,hal.rs,amp.rs}`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Read-Only Constraint
Audit `main.rs`, `hal.rs`, and `amp.rs` with zero-tolerance for CPU stalls, Core 0 / Core 1 division of labor, SPI Flash cache contention, I2S DMA buffer sizing, and main loop polling overhead (`CPU-101`, `CPU-102`, ...).
- **Constraint**: Read-only code inspection. Do NOT modify any files.

## 2. Inspection Checklist
1. **Core 0 (`PRO_CPU`) vs Core 1 (`APP_CPU`) Architecture & `decode_stream_step` Placement**:
   - Inspect `main.rs` (`app_core_loop` on Core 1 vs `bring_up` / `net_poll_phase` / `audio_render_phase` / `multi_proto_poll` / `bt_poll_phase` on Core 0).
   - Trace EXACTLY what runs on Core 1 (`app_core_loop`) vs what runs on Core 0! Wait: is `pipeline.decode_stream_step()` or I2S DMA push running on Core 1 or Core 0, and how often does it run per loop iteration?
   - How many frames are decoded/rendered per loop iteration (`audio_render_phase` / `app_core_loop`), and what happens when Core 0 is busy in `net_poll_phase`, `multi_proto_poll`, `bt_poll_phase`, or `amp_health`?
2. **I2S DMA Descriptor Count / Buffer Depth & Starvation Window**:
   - Inspect `main.rs` I2S0 initialization (`dma_rx_tx_buffers!` or DMA buffer size in bytes/frames) and `push_pcm` / `write_dma_frames`. How many milliseconds of audio can the hardware I2S DMA buffer hold before underrunning if the feeder loop is delayed by even 5–20 ms?
3. **SPI Flash Cache & External PSRAM Bus Contention (`#[esp_hal::ram]`)**:
   - Identify which hot-path functions in `main.rs`, `pipeline.rs`, `squeezelite-audio-core`, and `nanomp3-core` are currently in `.flash.text` and should be placed in `.iram0.text` (`#[esp_hal::ram]` / `#[link_section = ".rwtext"]`) within the ~14 KB free IRAM budget.
4. **I2C Polling Overhead in `main.rs` & `amp.rs` (`ANOM-PERF-03`)**:
   - Trace why `i2c_acks` is still advancing at `5.34` ACKs/sec (`~11` ACKs per 2s) in `main.rs` / `amp.rs` despite `WO-251`/`WO-257`! Check what `AmpManager::check_faults_and_telemetry` or `main.rs`'s `amp_health` loop actually executes on hardware.

## 3. Deliverable
Return a structured list of findings (`CPU-101`, `CPU-102`, ...) with exact `file:line` citations, mathematical cycle/latency calculations, CWE/taxonomy tags, and concrete code remediations.
