# Subagent Summary: WO-066 Stack Budget Restoration & Audio Quality Fix Preservation
**Timestamp:** 2026-10-25T08:44:00-04:00  
**Subagent Role:** Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)  
**Work Order:** `WO-066`  
**Git Branch:** `MK5-Rust`  
**Commit Hash:** `435b5cf6`  

## 1. Files Modified, Created, or Deleted (Absolute Paths)
- **Modified:**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-066-restore-36kb-stack-and-stage.md`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
- **Created (Staged Artifacts & Backups):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak6`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak6`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T084000_wo066_restore_stack_budget.md`

## 2. Rationale for Each Change
1. **Stack Budget & BSS Restoration**:
   - In `WO-065`, commit `92e10556` caused an early-boot stack overflow on `172.22.1.15` because expanding `tx_buffer` (8,192 -> 12,288 B) and `ring_storage` (16,384 -> 20,480 B) expanded `.bss` by 8,240 B, cutting `.stack` down from 36,528 B to 28,288 B. At the same time, expanding `WRITE_SCRATCH_BYTES` (512 -> 1024), `OUTPUT_I32_SCRATCH_SAMPLES` (128 -> 256), and `scratch` (512 -> 1024) increased runtime stack frame pressure.
   - `pipeline.rs`: Restored `WRITE_SCRATCH_BYTES = 512` and `OUTPUT_I32_SCRATCH_SAMPLES = 128`.
   - `main.rs`: Restored `tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024)` and `ring_storage: [u8; 16384]` (8 KB stream buffer, 8 KB output buffer).
   - `main.rs`: Restored `scratch = [0u8; 512]` and `silence = [0u8; 512]` in `write_dma` and `pump_silence_if_needed`, and set `threshold = if is_playing { 6144 } else { 4096 }`.
   - `main.rs`: Restored `load_nvs_wifi_credentials` parameter to `nvs_buf: &mut [u8; 16_384]`.
   - Result: `.stack` size is restored to exactly `0x008eb0` (36,528 bytes), matching known-stable commit `41eb03a4` byte-for-byte.
2. **Audio Quality Fix Preservation**:
   - Expanded DMA top-up loops in Phase 1 and Phase 3 of `run_firmware_tick` to 16 iterations (`512 * 16 = 8,192` B, enough to fill the entire DMA buffer).
   - Maintained 100% of zero-silence injection during active playback with buffered audio (`if is_playing && has_buffered_audio { return; }`).
   - Maintained zero trailing silence injection in `write_dma`.
   - Maintained 3-phase DMA-first scheduling in `run_firmware_tick`.
   - Maintained ICY `cont` phase alignment (`icy_meta_next = interval - already`).
   - Maintained linear 0.5 dB/step `0x4C` hardware volume mapping (`lms_gain_to_tas5805m_vol_reg`) and write-suppression when unchanged.
   - Maintained amp fault clearing and start playback gating to non-Play states only.

## 3. Known Risks & Untested Assumptions
- **Hardware Flashing Gate**: Stage-only verification confirms exact `.stack` restoration (36,528 bytes), clean compilation, and zero host test regressions. Live OTA deployment to `172.22.1.15` will be executed in the next deployment work order.
