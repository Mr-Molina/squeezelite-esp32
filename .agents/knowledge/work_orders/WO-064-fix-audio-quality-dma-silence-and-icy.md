# Work Order `WO-064`: Fix Audio Quality in `louder-esp32-app` — Eliminate DMA Silence Injection, Prioritize DMA Refill Before MP3 Decode, Fix ICY `cont` Phase Reset, & Fix TAS5805M `0x4C` `0.5 dB`/Step Volume (`--stage-only`)

- **ID**: `WO-064`
- **Timestamp**: `2026-10-25T08:18:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Status**: `[IN PROGRESS]`

## 1. Objective & Scope
Remediate the four compounding root causes of `"THE AUDIO QUALITY S TERRIBLE"` (`18.2%` injected `512`-byte silence blocks on *Ibiza Global Radio* at `36,070` fps and `3.1..6.7%` on *WAV*/*WHYY*/*WDIY* in commit `41eb03a4`) across `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:

1. **Rule 35 Pre-Modification Backups**:
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak5`.
   - Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_pipeline.rs.bak5`.

2. **Fix 1 (`src/pipeline.rs` & `src/main.rs` — Descriptor-Aligned `1,024`-Byte Output Writes & Eliminate Mid-Waveform Silence Injection)**:
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (lines 21–22):
     - Change `pub const WRITE_SCRATCH_BYTES: usize = 512;` to `pub const WRITE_SCRATCH_BYTES: usize = 1024;` (`128` stereo frames = `1,024` bytes = 1 full `esp-hal` DMA descriptor!).
     - Change `pub const OUTPUT_I32_SCRATCH_SAMPLES: usize = 128;` to `pub const OUTPUT_I32_SCRATCH_SAMPLES: usize = 256;`.
   - In `Esp32HardwareI2sDma::write_dma()` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs:692-772`):
     - Change `let silence = [0u8; 512];` (line 705) to `let silence = [0u8; 1024];`.
     - Change `let mut scratch = [0u8; 512];` (line 729) to `let mut scratch = [0u8; 1024];`.
     - **DELETE lines 757–770 completely!** `write_dma()` must **NEVER** push silence bytes into the DMA buffer!
   - In `Esp32HardwareI2sDma::pump_silence_if_needed(&mut self, is_playing: bool, has_buffered_audio: bool)` (`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs:671-688`):
     - **CRITICAL**: Previously, `let pad = avail % 1024; if pad > 0 { transfer.push(&silence[..pad]); }` was OUTSIDE the threshold check and ran unconditionally on every main loop tick, overwriting the second `512` bytes of any partially filled `1,024`-byte DMA descriptor with `0x00` zeros!
     - Replace `pump_silence_if_needed` with:
       ```rust
       pub fn pump_silence_if_needed(&mut self, is_playing: bool, has_buffered_audio: bool) {
           self.ensure_running();
           if is_playing && has_buffered_audio {
               return;
           }
           if let Some(ref mut transfer) = self.transfer {
               let threshold = if is_playing { 10240 } else { 6144 };
               let avail = transfer.available_bytes();
               if avail >= threshold {
                   let silence = [0u8; 1024];
                   let pad = avail % 1024;
                   if pad > 0 {
                       let _ = transfer.push(&silence[..pad]);
                   }
                   while transfer.available_bytes() >= threshold {
                       if transfer.push(&silence) == 0 {
                           break;
                       }
                   }
               }
           }
       }
       ```
     - Update the two call sites in `src/main.rs`:
       - At `StreamCommandType::Start`: `pipeline.sink.driver.pump_silence_if_needed(false, false);`
       - In the main loop (after `run_firmware_tick`):
         ```rust
         let has_buffered_audio =
             pipeline.output_buffer.available_read() >= 8 || pipeline.has_pending_pcm();
         pipeline.sink.driver.pump_silence_if_needed(
             coordinator.state == louder_esp32_app::PlayerState::Playing,
             has_buffered_audio,
         );
         ```

3. **Fix 2 (`src/main.rs:56-88, 839-867, 958, 988-1001` — Expand `I2S0` DMA Buffer (`12 KB`) & `output_storage` (`12 KB`) and Implement DMA-First `run_firmware_tick` Scheduling)**:
   - In `run_hardware_firmware` (`src/main.rs:958`), expand `tx_buffer` from `esp_hal::dma_tx_stream_buffer!(8192, 1024)` to **`esp_hal::dma_tx_stream_buffer!(12288, 1024)`** (`12` descriptors of `1,024` bytes = `34.83 ms` of hardware DMA cushion).
   - In `FirmwareBssBuffers` (`src/main.rs:840, 854`), expand `ring_storage` from `[u8; 16384]` to **`[u8; 20480]`** (`20 KB`):
     - In `load_nvs_wifi_credentials` (`src/main.rs:816`), accept `nvs_buf: &mut [u8]` and use `let nvs_slice = &mut nvs_buf[..16_384];` so `load_nvs_wifi_credentials(&mut flash, &sta_mac, &mut fw_bufs.ring_storage)` works seamlessly.
     - At `src/main.rs:1000`, `let (stream_storage, output_storage) = ring_storage.split_at_mut(8192);` gives `8,192` B (`8 KB`) `stream_storage` and **`12,288` B (`12 KB` = `1,535` stereo frames = `34.81 ms`, exceeding a full `1,152`-frame MP3 decode!)** `output_storage`!
   - **CRITICAL (`src/main.rs:56-86` — `run_firmware_tick` DMA-First Scheduling)**:
     - Remember that `coordinator.process_audio_step(pipeline)` calls `pipeline.process_decode_step()` on EVERY iteration! Calling `coordinator.process_audio_step(pipeline)` 8 times in a loop caused `process_decode_step()` to decode 2–3 full MP3 frames (`9–14 ms` blocking CPU time) inside a single tick before refilling `I2S0` DMA!
     - Replace lines 56–80 of `run_firmware_tick` in `src/main.rs` with explicit DMA-first scheduling:
       ```rust
       let mut frames = 0;
       if coordinator.state == louder_esp32_app::PlayerState::Playing {
           // 1. Fast O(1) drain of any pending decoded PCM into output_buffer and immediately top up I2S0 DMA
           // BEFORE spending ~4.5 ms decoding a new MP3 frame!
           for _ in 0..12 {
               if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
                   let _ = pipeline.process_decode_step();
               }
               if !coordinator.dac_enabled || pipeline.output_buffer.available_read() < 8 {
                   break;
               }
               let written = pipeline.process_output_step().unwrap_or(0);
               frames += written;
               coordinator.frames_played = pipeline.frames_played;
               if written == 0 {
                   break;
               }
           }

           // 2. Now that hardware I2S0 DMA is topped up, decode at most 1 MP3 frame (or up to 4 fast PCM chunks)
           // if no pending PCM remains and output_buffer has space.
           let max_decode_steps = match pipeline.decoder {
               ActiveDecoder::Mp3(_) => 1,
               _ => 4,
           };
           for _ in 0..max_decode_steps {
               if !pipeline.has_pending_pcm()
                   && pipeline.output_buffer.available_write() >= 1024
                   && pipeline.stream_buffer.available_read() > 0
               {
                   if !matches!(pipeline.process_decode_step(), Ok(c) if c > 0) {
                       break;
                   }
               } else {
                   break;
               }
           }

           // 3. Top up I2S0 DMA again with the newly decoded frames and drain pending PCM into output_buffer.
           for _ in 0..12 {
               if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
                   let _ = pipeline.process_decode_step();
               }
               if !coordinator.dac_enabled || pipeline.output_buffer.available_read() < 8 {
                   break;
               }
               let written = pipeline.process_output_step().unwrap_or(0);
               frames += written;
               coordinator.frames_played = pipeline.frames_played;
               if written == 0 {
                   break;
               }
           }
           if pipeline.has_pending_pcm() && pipeline.output_buffer.available_write() >= 8 {
               let _ = pipeline.process_decode_step();
           }
       }
       coordinator.current_sample_rate = pipeline.sample_rate;
       ```

4. **Fix 3 (`src/main.rs:1769-1774, 1975-1978` — Prevent `cont` From Resetting `icy_meta_next` Out of Phase & Fill `stream_buffer` Completely)**:
   - At `src/main.rs:1769-1774` (`cmd.stream_continue()`):
     - Previously, when `cont` arrived from LMS after `parse_icy_metaint_header` had already initialized `icy_meta_interval` (or after `audio_sock` had already ingested the first TCP window of audio bytes when `!awaiting_cont`), `cont` unconditionally overwrote `icy_meta_interval = metaint as usize; icy_meta_next = metaint as usize;`, shifting `icy_meta_next` out of phase with the HTTP body and causing the ICY stripper to eat random `(c[0] * 16)`-byte chunks of MP3 audio frames every `metaint` bytes!
     - Change lines 1769–1774 to:
       ```rust
       } else if let Some(metaint) = cmd.stream_continue() {
           coordinator.awaiting_cont = false;
           if metaint > 0 && icy_meta_interval == 0 {
               let interval = metaint as usize;
               let already = (coordinator.stream_bytes_received as usize) % interval;
               icy_meta_interval = interval;
               icy_meta_next = interval - already;
               icy_meta_left = 0;
           }
       ```
   - At `src/main.rs:1975-1978` (`audio_sock` receive loop):
     - Change `if avail_write < 512 { break; }` to `if avail_write == 0 { break; }` so `stream_buffer` fills completely up to capacity and up to ICY metadata boundaries.
   - In `Esp32HardwareI2sDma::set_clock_rate()` (`src/main.rs:784-791`), add MPEG-2 / MPEG-2.5 radio sample rates:
     - `24_000 => (26, 1, 24),`
     - `22_050 => (28, 22, 63),`
     - `16_000 => (39, 1, 16),`
     - `12_000 => (52, 1, 12),`
     - `11_025 => (56, 44, 63),`

5. **Fix 4 (`src/main.rs:133-153, 1416-1426` — Exact `0.5 dB`/Step TAS5805M `0x4C` Hardware Volume Mapping)**:
   - Previously, `amp.set_volume_percent(55)` passed `55%` through `percent_to_attenuation(55)`, which maps `1..100%` over `-79.5 dB..0 dB` (`0.8 dB` per step), writing `0x79` (`-36.5 dB`) instead of `0x5D` (`-22.5 dB` = `0.5 dB` per step)!
   - Add a pure helper function in `src/main.rs`:
     ```rust
     pub fn lms_gain_to_tas5805m_vol_reg(gain: u32) -> u8 {
         let pct = lms_gain_to_hardware_volume_percent(gain);
         if pct == 0 {
             0xFF // VOL_MUTE
         } else {
             0x30 + (100 - pct.min(100)) // 0x30 (0.0 dB) at 100%, +1 (+0.5 dB attenuation) per 1% below 100%
         }
     }
     ```
   - In `ServerCommand::AudioGain` (`src/main.rs:1416-1426`), use:
     ```rust
     let max_gain = core::cmp::max(gain_left, gain_right);
     let vol_reg = lms_gain_to_tas5805m_vol_reg(max_gain);
     if amp.current_volume() != vol_reg {
         let _ = amp.set_volume(vol_reg);
     }
     let eff_gain = if max_gain > 0 { 65536 } else { 0 };
     pipeline.set_volume(eff_gain, eff_gain);
     ```
   - And in `StreamCommandType::Start` (`src/main.rs:1522-1525`), only call `amp.clear_faults()` + `amp.start_playback_via_hiz()` when `amp.mode() != louder_esp32_app::AmpMode::Play`.
   - Add unit test `test_lms_gain_to_tas5805m_vol_reg` in `src/main.rs`.

6. **5-Gate `--stage-only` Verification**:
   - Gate 1: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`).
   - Gate 2: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`).
   - Gate 3: `rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`).
   - Gate 4: Verify `.stack` (`0x3FFE0000 - _bss_end`) via `readelf -S` is `>= 28,000` bytes.
   - Gate 5: Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32` and verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` prints `[STAGE-ONLY AUDIT PASS]`.
   - Commit the verified changes to `MK5-Rust`.

## 2. Mandatory Constraints
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- **`--stage-only` Only**: Do NOT flash `172.22.1.15` in `WO-064` (`WO-065` will flash and verify).
- Verify `manage_task(list)` has 0 running tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T081800_wo064_audio_quality_fix.md`, update `WO-064` Fulfillment Section, and reply with your report.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Firmware Audio Quality Specialist
- **Commit Hash**: `92e10556` on branch `MK5-Rust`
- **Verification Gates**:
  - Gate 1: `cargo test --workspace` PASSED (41 cyd, 77 louder-esp32-app, 23 decoders, 18 proto, 10 proto_fuzz, 11 tas58xx tests passed, 0 failures)
  - Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` PASSED (0 warnings)
  - Gate 3: Xtensa release build (`cargo build --release --target xtensa-esp32-none-elf -p louder-esp32-app`) PASSED (`louder-esp32-app` ELF built cleanly)
  - Gate 4: `readelf -S` stack verification: `.stack` size `0x006e80` = 28,288 bytes (`>= 28,000` bytes required)
  - Gate 5: `espflash save-image` produced `731,328` B OTA binary; `verify_staged_ota_bin.py` PASSED (`[STAGE-ONLY AUDIT PASS]`, sha256 `424de9fb7c9498dbb8d288e1a2a45ef3da87c17445d72ab32a7864a828f5a011`)
- **Key Modifications**:
  - `pipeline.rs`: `WRITE_SCRATCH_BYTES = 1024`, `OUTPUT_I32_SCRATCH_SAMPLES = 256`.
  - `main.rs`:
    - Eliminated periodic 512B silence injection in `pump_silence_if_needed` when playing with buffered audio. Raised silence threshold to 10,240B.
    - Removed silence padding in `write_dma` when `avail >= 6144`.
    - Expanded I2S DMA tx stream buffer from 8192B to 12288B and BSS `ring_storage` from 16384B to 20480B.
    - Implemented 3-phase DMA-first scheduling in `run_firmware_tick` (drain & top-up -> decode at most 1 MP3 frame -> top-up & drain).
    - Fixed ICY metadata stripping bug on `cont`: only set interval if 0, and phase-align `icy_meta_next` to `interval - (stream_bytes_received % interval)`.
    - Fixed `audio_sock` recv loop to fill stream buffer down to `avail_write == 0`.
    - Added sample rates 24k, 22.05k, 16k, 12k, 11.025k to `set_clock_rate`.
    - Implemented `lms_gain_to_tas5805m_vol_reg` for exact 0.5 dB/step mapping and guarded against redundant I2C writes when volume unchanged.
    - Gated amp fault clearing and playback restart to non-Play states only.
    - Added unit test `test_lms_gain_to_tas5805m_vol_reg`.
