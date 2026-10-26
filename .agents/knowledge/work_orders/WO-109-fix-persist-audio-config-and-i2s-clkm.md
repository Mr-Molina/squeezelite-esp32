# Work Order: WO-109

- **Work Order ID**: `WO-109`
- **Timestamp**: `2026-10-25T19:14:30-04:00`
- **Parent Agent**: `Orchestrator (31deac42-e161-443d-9205-80c67d7541ab)`
- **Assigned Subagent Role**: `Rust App Flash-Journal & I2S CLKM Implementer` (`self`, `Model: "inherit"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Task Goal
Remediate the two root causes isolated during `WO-108` live hardware verification on `172.22.1.15` (`"BASEMENT"`):
1. **Runtime `SPI1` NOR Flash Deadlock in `persist_audio_config` (`src/main.rs:1010-1056, 1721, 1730, 1745`)**:
   - In `WO-108`, Gate 3.1 (`GET /status.json`), Gate 3.2 (`GET /` `3,928` B HTML), and the 1st `/cmd.json` call (`act=eq_bass` -> `settings_seq = 1`) all succeeded, but the 2nd `/cmd.json` call (`act=eq_loudness`) hung inside `persist_audio_config(&mut flash, settings_scratch, &audio_cfg)` until the 20s `TIMG0` WDT rolled back to Factory recovery.
   - ELF disassembly proved that `esp-storage 0.10.0`'s `FlashStorage::read_nor` calls `spiflash_read` WITHOUT parking `APP_CPU` (`self.multi_core_strategy.with(...)`), and `spiflash_erase_sector` masks interrupts (`rsil a7, 5`) for `45–80 ms` and reconfigures `SPI1` without disabling `PRO_CPU`/`APP_CPU` `SPI0` Cache while `4 MB` PSRAM (`0x3F80_0000`), XIP Flash `.text`, and `esp-radio` Wi-Fi interrupts are active.
2. **`I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) `div_b`/`div_a` Bitfield Swap (`src/main.rs:883`) & Initial `8,192` B DMA Pre-Fill (`src/main.rs:733, 1307`)**:
   - In ESP32 hardware (`esp-hal-1.2.2/src/i2s/master/low_level/v1.rs:32-33`), `I2S0_CLKM_CONF_REG` bits `[13:8]` are `clkm_div_a` (denominator = `63`) and bits `[19:14]` are `clkm_div_b` (numerator = `11`), whereas `src/main.rs:883` writes `((div_b & 0x3F) << 8) | ((div_a & 0x3F) << 14)`.
   - Pre-filling `tx_buffer` (`8,192` B) with `4 * 2,048 = 8,192` B (`46.4 ms`) instead of `2,048` B (`5.8 ms`) prevents `transfer.is_done()` underruns during boot and `12 ms` I2C/TCP main-loop ticks.

## 2. Exact File Modifications Required (`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`)

1. **`persist_audio_config` (`src/main.rs:1010-1056` and call sites at `src/main.rs:1721, 1730, 1745`)**:
   - Change `persist_audio_config` signature and implementation so it NEVER calls `flash.read_nor`, `flash.erase`, or `flash.write_nor` synchronously during the runtime HTTP/audio loop while `APP_CPU`, `4 MB` PSRAM `SPI0` cache, and `esp-radio` Wi-Fi are running:
     ```rust
     #[inline(never)]
     fn persist_audio_config(
         scratch: &mut [u8; 512],
         cfg: &AudioConfig,
         current_seq: u32,
     ) -> Option<u32> {
         let next_seq = current_seq.wrapping_add(1);
         let mut store = MemConfigStore::<8>::new();
         cfg.save_to(&mut store).ok()?;
         scratch.fill(0xFF);
         squeezelite_config::FlashJournal::save_to_sector_with_seq(&store, scratch, next_seq).ok()?;
         Some(next_seq)
     }
     ```
   - Update the 3 call sites in `src/main.rs` (`SetEqPreset`, `SetAnalogGain`, and `PlayerIrCommand`) to pass `(settings_scratch, &audio_cfg, settings_seq)`.
   - Note: `load_boot_audio_config(&mut flash, ...)` at boot (before `APP_CPU` and `esp-radio` Wi-Fi start) remains unchanged so `flash` is still used at boot and no unused-variable warning occurs.

2. **`Esp32HardwareI2sDma::set_clock_rate` (`src/main.rs:883`)**:
   - Fix the bitfield order of `div_a` (`bits [13:8]`, denominator) and `div_b` (`bits [19:14]`, numerator) in `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`):
     ```rust
     let new_val = flags | (div_num & 0xFF) | ((div_a & 0x3F) << 8) | ((div_b & 0x3F) << 14);
     ```

3. **Full `8,192` B (`46.4 ms`) Silence Pre-Fill in `ensure_running` (`src/main.rs:733-736`) & `run_hardware_firmware` (`src/main.rs:1307-1310`)**:
   - In both `ensure_running` and `run_hardware_firmware` (before `i2s_tx.write(...)`), push `4` chunks of `2,048` B (`8,192` B total) of silence into the `8,192`-byte `DmaTxStreamBuf`:
     ```rust
     let silence = [0u8; 2048];
     for _ in 0..4 {
         let _ = new_buf.push(&silence);
     }
     ```
     (and `for _ in 0..4 { let _ = tx_buffer.push(&silence); }` at `src/main.rs:1307`).

## 3. Mandatory `--stage-only` Verification Gates
1. `cargo test --workspace` on `x86_64-unknown-linux-gnu` (`0` failures).
2. `cargo clippy --workspace --all-targets -- -D warnings` (`0` warnings).
3. `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`.
4. Verify via `xtensa-esp32-elf-readelf -S` and `xtensa-esp32-elf-objdump -d`:
   - `.stack >= 35,000` B
   - `run_hardware_firmware` stack frame (`entry a1, N`) `<= 9,000` B
   - `persist_audio_config` contains `0` calls to `spiflash_read`, `spiflash_erase_sector`, or `spiflash_write`.
5. Export image with `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 --flash-size 4mb` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and run `/usr/bin/python3 /home/console-admin/Github/squeezelite-esp32/scripts/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`.
6. Commit cleanly on `MK5-Rust` and report the new commit hash and `.bin` SHA-256.
7. **DO NOT flash `172.22.1.15`** (strictly `--stage-only`; `WO-110` `Extension Certifier` will perform the flash).

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `da35fa61e92084c64b5710594a52a2a5f091c08d` (`da35fa61`)
- **Rule 35 Pre-Modification Backups**:
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/main.rs.wo109.bak`
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/WO-109-fix-persist-audio-config-and-i2s-clkm.md.bak`
- **Summary of Changes**:
  1. **Zero-`SPI1` Runtime `persist_audio_config` (`src/main.rs:1017-1029, 1690-1718`)**:
     - Changed `persist_audio_config(scratch: &mut [u8; 512], cfg: &AudioConfig, current_seq: u32) -> Option<u32>` to serialize `FlashJournal::save_to_sector_with_seq(&store, scratch, next_seq)` in RAM and return `Some(next_seq)` with zero runtime `flash.read_nor`, `flash.erase`, or `flash.write_nor` calls while `APP_CPU`, `4 MB` PSRAM `SPI0` cache, and `esp-radio` Wi-Fi are running.
     - Updated the 3 call sites (`SetEqPreset`, `SetAnalogGain`, and `PlayerIrCommand`) to pass `(settings_scratch, &audio_cfg, settings_seq)`.
  2. **`I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) `div_a`/`div_b` Bitfield Order (`src/main.rs:889`)**:
     - Fixed `let new_val = flags | (div_num & 0xFF) | ((div_a & 0x3F) << 8) | ((div_b & 0x3F) << 14);` so `clkm_div_a` (`bits [13:8]`, denominator = `63`) and `clkm_div_b` (`bits [19:14]`, numerator = `11`) match ESP32 hardware (`44,099.99 Hz`).
  3. **Full `8,192` B (`46.4 ms`) Silence Pre-Fill (`src/main.rs:726-753, 1282-1283`)**:
     - Added `#[inline(never)] pub fn prefill_silence(buf: &mut esp_hal::dma::DmaTxStreamBuf)` pushing `4 * 2,048 = 8,192` B of silence (`46.4 ms`) before `i2s_tx.write(...)` in both `ensure_running` (also `#[inline(never)]`) and `run_hardware_firmware`, keeping `run_hardware_firmware`'s stack frame at `8,480` B (`<= 9,000` B).
- **`--stage-only` Verification Gates**:
  - `cargo test --workspace`: `0` failures (`259` tests passed).
  - `cargo clippy --workspace --all-targets -- -D warnings`: `0` warnings (`exit code 0`).
  - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`: `exit code 0`.
  - `xtensa-esp32-elf-readelf -S`: `.stack = 0x008bd0` (`35,792` B `>= 35,000` B), `.data = 0x003dd0` (`15,824` B `<= 20,000` B), `.bss = 0x023440` (`144,448` B), `.dram2_uninit = 0x004990` (`18,832` B).
  - Disassembly Verification (`verify_wo109_elf.py`):
    - `run_hardware_firmware` (`0x4011d1d0`, size `42,804` B): `entry a1, 8480` (`0x2120` B `<= 9,000` B).
    - `persist_audio_config` (`0x4011c98c`, size `2,113` B): `entry a1, 1472`, **`0` calls** to `spiflash_*` or `FlashStorage`.
  - `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, path `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, size `756,240` bytes (`28.14%` of `ota_0`), SHA-256 `8b74c66645a20ee1e680e2bf31e1bd56edb705b67baf65df57cc5295ec56d8f4`.
- **Handoff & Summary**:
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo109_persist_clkm_implementer.md`
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/handoff.md`
