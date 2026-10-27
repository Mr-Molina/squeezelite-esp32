# Work Order `WO-123`: `/deep-hardware-audit` Phase 1 Micro-Auditor 2 — Audio Clock Domain, I2S0 DMA Ringbuffer & Rate Mismatch

- **ID**: `WO-123`
- **Timestamp**: `2026-10-25T20:31:00-04:00`
- **Subagent Role**: Audio Clock Domain, I2S0 DMA Ringbuffer & Rate Mismatch Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: Audio Clock Domain, I2S0 DMA Ringbuffer & Rate Mismatch Micro-Auditor
2. **Target & Scope**: `[BASEMENT]` (`172.22.1.15`, Sonocotta Louder-ESP32 `ESP32-WROVER` + TI `TAS5805M`). Subsystems: ESP32 `I2S0` peripheral & `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) fractional divider, `DmaTxCircularBuffer` (`8,192` B), and `frames_played` throughput (`41,658.3` fps vs `44,100` fps).
3. **Layer Mandate**: Audit bottom-up: L1 (`I2S0` clock divider `(14, 11, 63)` = `44,099.99 Hz` vs `TAS5805M` `FS_MON` `0x37` reporting `0x09` = "44.1 or 48 kHz" mapped to `tas_fs_hz: 48000`) -> L2 (`I2S0` circular DMA `tx_buffer` `8,192` B, `available_bytes() & !7`, `ensure_running()` zero-fill, Invariant 46) -> L3 (`run_firmware_tick` in `src/main.rs` & `StreamPipeline` in `src/pipeline.rs`: how many MP3 frames / PCM bytes are decoded and pushed per tick, and why `frames_played` advances at `41,658.3` fps during 320 kbps MP3 streaming!) -> L4 (`status.json` `frames_played` telemetry over a 10-second multi-sample window).
4. **Seeds**: `Power, Thermal & Clocking Configurations`, `PCIe, USB & Peripheral Interconnects`, `Embedded / MCU / IoT Hardware`. Compliance: `NIST-800-53:CM-6`, `HW-FAULT:BUS-DROPOUT`.
5. **Pre-Identified Scout Anomalies**:
   - Anomaly A: `frames_played` in `WO-118` advanced by `169,216` frames in `4,062 ms` = `41,658.3` frames/sec (`94.46%` of `44,100` Hz), whereas in `WO-067`/`WO-079` (before `SqueezeDSP` re-enabled or before PSRAM/EQ/I2C polling changes) it ran at `44,094` fps! Determine the exact root cause in `src/main.rs` / `src/pipeline.rs` (e.g., main-loop tick duration with `read_telemetry()` I2C polling + `process_decode_step()` 1-MP3-frame cap or DMA buffer size `8,192` B = `23.2 ms` underrunning during long ticks!).
   - Anomaly B: `squeezelite-tas58xx/src/lib.rs` maps `FS_MON == 0x09` ("44.1 kHz or 48 kHz" in TI datasheet) unconditionally to `48_000`, so `status.json` shows `"tas_fs_hz": 48000` when playing `44,100` Hz audio.
6. **Shell & Script Safety**: Read-only Linux commands (`curl -sS http://172.22.1.15/status.json`) and read `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{main.rs,pipeline.rs}` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`. **NEVER touch `/dev/ttyUSB0`**.
7. **Constraint**: Phase 1 — Read-only telemetry and source inspection. Do NOT modify any files or configs.
8. **Output Schema**: Report findings using the 14-field hardware finding schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
*(To be completed by subagent)*
