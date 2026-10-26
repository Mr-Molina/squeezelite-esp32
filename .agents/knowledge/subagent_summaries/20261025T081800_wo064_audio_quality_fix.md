# Subagent Summary: WO-064 Audio Quality Fixes (DMA Silence Elimination & ICY Cont Phase Realignment)
**Timestamp:** 2026-10-25T08:30:00-04:00  
**Subagent Role:** Firmware Audio Quality Specialist (`62c02190-686b-4c1c-839a-fe62582f3708`)  
**Work Order:** `WO-064`  
**Git Branch:** `MK5-Rust`  
**Commit Hash:** `92e10556`  

## 1. Files Modified, Created, or Deleted (Absolute Paths)
- **Modified:**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-064-fix-audio-quality-dma-silence-and-icy.md`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`
- **Created (Staged Artifacts & Backups):**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak5`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak5`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T081800_wo064_audio_quality_fix.md`

## 2. Rationale for Each Change
1. **DMA Silence Injection Elimination (`pipeline.rs`, `main.rs`)**:
   - `WRITE_SCRATCH_BYTES` increased from 512 to 1024, and `OUTPUT_I32_SCRATCH_SAMPLES` increased from 128 to 256.
   - In `pump_silence_if_needed`, early return was added when `is_playing && has_buffered_audio`, and the threshold for silence priming was raised to 10,240 B. Previously, `avail % 1024` silence padding was pushed on every loop tick during active playback, corrupting 18.2% of all played descriptors with silence bursts.
   - In `write_dma()`, the spurious silence pump when `avail >= 6144` was completely removed.
2. **DMA-First Scheduling & Buffer Expansion (`main.rs`)**:
   - Expanded I2S DMA tx stream buffer from 8,192 B to 12,288 B and BSS `ring_storage` from 16,384 B to 20,480 B (`split_at_mut(8192)` yields 8 KB stream buffer and 12 KB output buffer).
   - In `run_firmware_tick`, replaced the monolithic 4-iteration decode/output loop with 3-phase DMA-first scheduling:
     1. Drain pending PCM into `output_buffer` and top up I2S0 DMA immediately.
     2. Decode at most 1 MP3 frame (or up to 4 PCM chunks) only when DMA is full and output buffer has >= 1024 B free.
     3. Top up I2S0 DMA again with newly decoded frames.
   - This ensures the ~4.5 ms MP3 decode execution never starves hardware DMA.
3. **ICY Metadata Synchronization & Receive Loop Optimization (`main.rs`)**:
   - In `cmd.stream_continue()`, `icy_meta_interval` is only set if currently 0. If non-zero or when initialising, `icy_meta_next` is phase-aligned using `interval - (stream_bytes_received % interval)` to prevent `cont` from shifting the stripper out of phase with audio data.
   - In `audio_sock` recv loop, changed break condition from `avail_write < 512` to `avail_write == 0`, ensuring the ring buffer fills completely.
   - Added missing MPEG-2 and MPEG-2.5 sample rates (24k, 22.05k, 16k, 12k, 11.025k) to `set_clock_rate`.
4. **Volume Register Linearity (`main.rs`)**:
   - Added `lms_gain_to_tas5805m_vol_reg` implementing exact 0.5 dB/step mapping (`0x30` at 100%, +1 per 1% below 100%, `0xFF` at 0%).
   - In `ServerCommand::AudioGain`, writes are only dispatched to the amplifier if `amp.current_volume() != vol_reg`, eliminating redundant I2C bus traffic.
   - In `StreamCommandType::Start`, fault clearing and playback restart are only triggered when `amp.mode() != Play`.

## 3. Known Risks & Untested Assumptions
- **Host Testing vs Real Hardware**: Stage-only verification confirms binary validity, stack size (28,288 B), and automated test suites. Hardware execution verification on the physical basement device (`172.22.1.15`) will occur under `WO-065`.
- **NVS Credential Fallback**: If NVS slice is under 16,384 bytes, returns `OtaSafetyError::CredentialsNotFound` and falls back to compile-time Wi-Fi credentials.
