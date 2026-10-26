# Work Order `WO-051`: Fix ESP32 `dram_seg` `.stack` / `.bss` Budget (`nvs_buf` `.bss` Overlay + `STATIC_MP3_DEC` in `.bss` + Right-Sized `FirmwareBssBuffers`) & `--stage-only` Packaging

- **ID**: `WO-051`
- **Timestamp**: `2026-10-24T15:53:00-04:00`
- **Subagent Role**: Rust App & Hardware Implementer (`62c02190-686b-4c1c-839a-fe62582f3708`)
- **Status**: `[IN PROGRESS]`

## 1. Objective & Scope
Remediate the `28,984 B > 27,824 B` boot stack overflow in `dram_seg` (`0x3FFB0000..0x3FFE0000`) isolated in `WO-050`:
1. **`rust/crates/squeezelite-decoders/src/lib.rs`**:
   - In `StaticMp3Decoder::new()`, initialize `sample_rate: 0` (all-zero struct so LLVM places `STATIC_MP3_DEC` in `.bss` instead of `.data`, reclaiming `22,792` bytes from the flash `.bin` image!).
   - In `StaticMp3Decoder::reset_in_place(&mut self)`, set `self.sample_rate = 44100`, `nanomp3_core::__private::init(&mut self.decoder)`, `self.id3_skip_remaining = 0`.
   - In `impl AudioDecoder for StaticMp3Decoder { fn flush(&mut self) }`, call `self.reset_in_place()`.
2. **`rust/crates/louder-esp32-app/src/pipeline.rs`**:
   - Add `#[inline(always)]` to `StreamPipeline::new(...)` so LLVM constructs `StreamPipeline` directly in place (`sret`) in `run_hardware_firmware()` without an intermediate `12.6 KB` callee stack copy.
   - Set `#[cfg(target_arch = "xtensa")] pub const READ_SCRATCH_BYTES: usize = 1536;` (saves `512` B of stack while comfortably exceeding max MP3 frame size `1,440` B).
3. **`rust/crates/louder-esp32-app/src/main.rs`**:
   - In `FirmwareBssBuffers`:
     - Replace `stream_storage: [u8; 8192]` and `output_storage: [u8; 8192]` with `ring_storage: [u8; 16384]`.
     - Right-size the remaining socket/protocol buffers to reclaim `10,752` bytes directly into `.stack`:
       - `http_rx_buf: [u8; 1024]`
       - `http_tx_buf: [u8; 1024]`
       - `slim_rx_buf: [u8; 2048]`
       - `slim_tx_buf: [u8; 1024]`
       - `audio_rx_buf: [u8; 4096]`
       - `audio_tx_buf: [u8; 512]`
       - `slim_acc_buf: [u8; 1536]`
       - `audio_req_buf: [u8; 512]`
       - `audio_header_buf: [u8; 1024]`
   - Update `load_nvs_wifi_credentials(flash: &mut FlashStorage<'_>, sta_mac: &[u8; 6], nvs_buf: &mut [u8; 16_384]) -> Result<WifiStaCredentials, OtaSafetyError>`:
     - Fill `nvs_buf.fill(0xFF)`, read `NVS_PARTITION_OFFSET` into `nvs_buf`, and pass `nvs_buf` to `resolve_wifi_credentials`.
     - In `run_hardware_firmware()`, call `let fw_bufs = take_firmware_bss_buffers();` BEFORE `load_nvs_wifi_credentials`, pass `&mut fw_bufs.ring_storage` to `load_nvs_wifi_credentials(&mut flash, &sta_mac, &mut fw_bufs.ring_storage)`, and after NVS credentials are resolved, zero `fw_bufs.ring_storage.fill(0)` and split it:
       `let (stream_storage, output_storage) = ring_storage.split_at_mut(8192);`
     - This eliminates the `16,384`-byte stack array from `load_nvs_wifi_credentials` with **0 extra bytes of `.bss`** and **0 lines of `unsafe` union code**!
4. **Mandatory Verification Gates (`--stage-only`)**:
   - Gate 1: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` (`exit code 0`)
   - Gate 2: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` (`exit code 0`)
   - Gate 3: `rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`)
   - Gate 4: Inspect `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` with `readelf -S` to verify `.stack` size is `>= 38,000` bytes (`> 37 KB`) and `STATIC_MP3_DEC` is in `.bss` (shrinking `.data` and the `.bin` image by `~22.8 KB`).
   - Gate 5: Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32` and run `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` (`[STAGE-ONLY AUDIT PASS]`, `exit code 0`).
   - Commit changes to `MK5-Rust`.

## 2. Mandatory Constraints
- **DO NOT touch `/dev/ttyUSB0`** (Rule 3).
- **DO NOT flash or contact `172.22.1.15`** (`--stage-only` only).
- Follow Rule 15, Rule 17, Rule 21, Rule 23, Rule 24, Rule 35.

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Rust App & Hardware Implementer
- **Completion Timestamp**: 2026-10-24T15:58:55-04:00
- **Git Commit**: `96c7e60b240ffb60bc571b058a9831969a5a3a78` (branch: `MK5-Rust`)
- **Staged OTA Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Binary Size**: 727,312 bytes (27.07% of `ota_0`, 17.62% of 4MB flash image, reduced by 22,496 bytes)
- **SHA-256**: `b1973a9d8eea38c627c57116094a7d85f5d2b61a1d0cd68af19e1b0fd0c98ac3`
- **Validation Audit**: `[STAGE-ONLY AUDIT PASS]` (img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0')
- **Rule 35 Backups**:
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/lib.rs.wo051.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/pipeline.rs.wo051.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/main.rs.wo051.bak`

### Section Metrics (`readelf -S` Empirical Verification)
- `.stack` size: `0x0096b0` = **38,576 bytes** (`37.67 KB`, `>= 38,000` bytes). Exceeds boot stack peak (28,984 bytes) with a +9,592 byte headroom margin.
- `.data` size: `0x003d0c` = **15,628 bytes** (shrunk from 38,432 bytes, reclaiming **22,804 bytes**).
- `STATIC_MP3_DEC` symbol location: `3ffc5f94 b louder_esp32_app::xtensa_hw::take_static_mp3_decoder::STATIC_MP3_DEC` (`b` = `.bss`).

### Verification Results
1. Gate 1 (`cargo test --workspace`): PASS (179 tests passed, 0 failed)
2. Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`): PASS (0 warnings)
3. Gate 3 (`cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`): PASS (Exit code 0)
4. Gate 4 (`readelf -S` verification of `.stack` & `.data` sections): PASS (.stack = 38,576 bytes, .data shrunk to 15,628 bytes, STATIC_MP3_DEC in .bss)
5. Gate 5 (`espflash save-image` + `verify_staged_ota_bin.py`): PASS (`[STAGE-ONLY AUDIT PASS]`, Exit code 0, 727,312 bytes)
6. Scope Constraint: Strictly `--stage-only`. `/dev/ttyUSB0` and `172.22.1.15` were never accessed.

