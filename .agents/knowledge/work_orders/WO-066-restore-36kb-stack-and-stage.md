# Work Order `WO-066`: Restore Exact `36,528`-Byte `.stack` & Stack Frame of `41eb03a4` While Preserving All Audio Quality Remediations (`--stage-only`)

- **ID**: `WO-066`
- **Timestamp**: `2026-10-25T08:40:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Status**: `[IN PROGRESS]`

## 1. Objective & Scope
In `WO-065`, commit `92e10556` triggered an early-boot stack overflow on `172.22.1.15` (safely caught by Gate 1 Anti-Brick recovery to `0x10000`) because expanding `tx_buffer` (`8192 -> 12288`) and `ring_storage` (`16384 -> 20480`) added `8,240` bytes to `.bss` (shrinking `.stack` from `36,528` B in working commit `41eb03a4` down to `28,288` B), while expanding `WRITE_SCRATCH_BYTES` (`512 -> 1024`), `OUTPUT_I32_SCRATCH_SAMPLES` (`128 -> 256`), and `write_dma` `scratch` (`512 -> 1024`) added `1,536` bytes to the active stack frame.

Restore the **exact `36,528`-byte `.stack` (`0x008eb0`) and exact stack frame size of working commit `41eb03a4`** while preserving 100% of the zero-silence-injection, DMA-first scheduling, ICY `cont` phase alignment, and `0.5 dB`/step `0x4C` hardware volume fixes:

1. **In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs` (lines 21–22)**:
   - Restore `pub const WRITE_SCRATCH_BYTES: usize = 512;` and `pub const OUTPUT_I32_SCRATCH_SAMPLES: usize = 128;` (saving `1,024` bytes on `StreamPipeline` stack frame).
2. **In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`**:
   - **DMA & Output Loops in `run_firmware_tick` (lines 60–112)**:
     - Change `for _ in 0..12` in Phase 1 and Phase 3 of `run_firmware_tick` to `for _ in 0..16` (since `512` B $\times 16 = 8,192$ B, two consecutive `512`-byte iterations fill a complete `1,024`-byte DMA descriptor and 16 iterations can fill the entire `8,192`-byte DMA buffer!).
   - **In `Esp32HardwareI2sDma::pump_silence_if_needed` (lines 719–740)**:
     - Since `tx_buffer` is `8,192` bytes, set `let threshold = if is_playing { 6144 } else { 4096 };` while keeping `if is_playing && has_buffered_audio { return; }` and keeping `let pad = avail % 1024;` **inside** `if avail >= threshold`:
       ```rust
       pub fn pump_silence_if_needed(&mut self, is_playing: bool, has_buffered_audio: bool) {
           self.ensure_running();
           if is_playing && has_buffered_audio {
               return;
           }
           if let Some(ref mut transfer) = self.transfer {
               let threshold = if is_playing { 6144 } else { 4096 };
               let avail = transfer.available_bytes();
               if avail >= threshold {
                   let silence = [0u8; 512];
                   let pad = avail % 1024;
                   if pad > 0 {
                       let mut rem = pad;
                       while rem > 0 {
                           let c = rem.min(silence.len());
                           let n = transfer.push(&silence[..c]);
                           if n == 0 {
                               break;
                           }
                           rem -= n;
                       }
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
       *(Note: using `let silence = [0u8; 512];` instead of `[0u8; 1024]` also saves another `512` bytes of stack in `pump_silence_if_needed`!)*
   - **In `Esp32HardwareI2sDma::write_dma` (lines 757, 781)**:
     - Restore `let silence = [0u8; 512];` and `let mut scratch = [0u8; 512];` (saving `512` bytes of stack in `write_dma`), and keep `write_dma` completely free of any trailing silence injection.
   - **In `load_nvs_wifi_credentials` & `FirmwareBssBuffers` & `run_hardware_firmware` (lines 830–1030)**:
     - Restore `nvs_buf: &mut [u8; 16_384]` in `load_nvs_wifi_credentials`.
     - Restore `ring_storage: [u8; 16384]` in `FirmwareBssBuffers` (`let (stream_storage, output_storage) = ring_storage.split_at_mut(8192);`).
     - Restore `let tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024);`.
3. **5-Gate `--stage-only` Verification**:
   - Gate 1: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`).
   - Gate 2: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`).
   - Gate 3: `rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`).
   - Gate 4: Verify via `readelf -S` that `.stack` is **`0x008eb0` (`36,528` bytes)** — matching working commit `41eb03a4` byte-for-byte!
   - Gate 5: Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32` and verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` prints `[STAGE-ONLY AUDIT PASS]`.
   - Commit the verified changes to `MK5-Rust`.

## 2. Mandatory Constraints
- **NEVER touch `/dev/ttyUSB0`** (Rule 3).
- **`--stage-only` Only**: Do NOT flash `172.22.1.15` in `WO-066`.
- Verify `manage_task(list)` has 0 running tasks (Rule 21), write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025T084000_wo066_restore_stack_budget.md`, update `WO-066` Fulfillment Section, and reply with your report.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Commit Hash**: `435b5cf6` on branch `MK5-Rust`
- **Verification Gates**:
  - Gate 1: `cargo test --workspace` PASSED (41 cyd, 77 louder-esp32-app, 23 decoders, 18 proto, 10 proto_fuzz, 11 tas58xx tests passed, 0 failures)
  - Gate 2: `cargo clippy --workspace --all-targets -- -D warnings` PASSED (0 warnings)
  - Gate 3: Xtensa release build (`cargo build --release --target xtensa-esp32-none-elf -p louder-esp32-app`) PASSED (`louder-esp32-app` ELF built in 4.49s)
  - Gate 4: `readelf -S` stack verification: `.stack` size `0x008eb0` = `36,528` bytes (EXACT byte-for-byte match with working commit `41eb03a4`)
  - Gate 5: `espflash save-image` produced `731,312` B OTA binary; `verify_staged_ota_bin.py` PASSED (`[STAGE-ONLY AUDIT PASS]`, sha256 `9a7f58a1c8bdec8477fdbcdd69cab1f9abaa57416b925cdae53c6099e42670b5`)
- **Remediated State**:
  - `pipeline.rs`: Restored `WRITE_SCRATCH_BYTES = 512` and `OUTPUT_I32_SCRATCH_SAMPLES = 128`.
  - `main.rs`:
    - Restored `tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024)`.
    - Restored `FirmwareBssBuffers::ring_storage` to `[u8; 16384]`.
    - Restored `scratch = [0u8; 512]` and `silence = [0u8; 512]` in `write_dma`.
    - Restored `silence = [0u8; 512]` and `threshold = if is_playing { 6144 } else { 4096 }` in `pump_silence_if_needed`.
    - Restored `load_nvs_wifi_credentials` parameter to `nvs_buf: &mut [u8; 16_384]`.
    - Expanded Phase 1 and Phase 3 DMA top-up loops to `0..16` iterations.
    - Preserved 100% of zero-silence injection during active playback, 3-phase DMA-first scheduling, ICY `cont` phase alignment, and linear 0.5 dB/step `0x4C` hardware volume mapping.
