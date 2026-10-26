# Subagent Summary: WO-101 — Eliminate `run_hardware_firmware` `33.5 KB` Stack Frame & Fix PSRAM + Zero-Heap Fallback (`--stage-only`)

- **Timestamp**: `2026-10-25T16:50:30-04:00`
- **Role**: Rust App Stack-Frame & PSRAM Implementer (`WO-101`)
- **Commit SHA (`git rev-parse HEAD`)**: `7717733ff2975243d77dc6513d14be38a71d2c36`
- **Staged Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Staged Binary Size**: `787,008` bytes (`29.29%` of `ota_0`)
- **Staged Binary SHA-256**: `a2cf7600f274e53a93944709e88b6ea703905c51f43d96a164c4f9bf3e225a42`
- **ELF `.stack` Size**: `0x008dd0` (`36,304` bytes `>= 36,000` B)
- **`run_hardware_firmware` Stack Frame Size**: `0x21f0` (`8,688` bytes `<= 14,336` B, down from `33,568` bytes `0x8320`)
- **Free Stack Headroom above `_bss_end` (`0x3ffd7230`)**: `27,616` bytes (`26.97 KB` `>= 22,000` B)

## 1. Files Created, Modified, or Deleted

1. **Created (Backup per Rule 35)**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.wo101.bak`
   - **Rationale**: Pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
2. **Modified**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
   - **Rationale**:
     1. **Eliminated `run_hardware_firmware` Stack Frame Bloat (`33,568` B -> `8,688` B)**:
        - Removed `Esp32SettingsFlashPartition`, `NvsFlashBackend` (`~12.3 KB` on stack), and `MemConfigStore::<32>` (`~10.7 KB` on stack) from `run_hardware_firmware`.
        - Added `#[inline(never)] fn load_boot_audio_config` (`1,584` B temporary frame, popped on return) and `#[inline(never)] fn persist_audio_config` (`1,520` B temporary frame, popped on return) operating on `.bss` buffers (`ring_storage[..12_288]` at boot and `settings_scratch: [u8; 512]` at runtime) and a compact `MemConfigStore::<8>`, storing only `audio_cfg: AudioConfig` (`6` B) and `settings_seq: u32` (`4` B) in `run_hardware_firmware`.
        - Added `#[inline(never)] fn init_static_pipeline` (`352` B temporary frame, popped on return) to initialize `StreamPipeline` (`13,664` B) and `pending_resp_buf` (`2,064` B) in-place inside `.dram2_uninit` (`0x3ffe7e40..0x3ffebbd0`), shrinking `run_hardware_firmware`'s stack frame to `0x21f0` (`8,688` bytes) and leaving `27,616` bytes (`26.97 KB`) of free stack headroom in `.stack` (`36,304` bytes).
     2. **Fixed PSRAM XIP `SPI0` Corruption & `64 KB` Wi-Fi Heap Starvation**:
        - Disassembly of `esp-hal 1.2.2`'s `psram_init` (`0x40083df8`) proved that `psram_gpio_config` calls `0x4013c190 <esp_hal::gpio::io_mux_reg>` in `.text` (XIP Flash over `SPI0`) 14 times while reconfiguring `SPI0` dummy cycles and switching `SPI0` Flash pins (`CS=11, Q=7, D=8, HD=9, WP=10`) from IOMUX to GPIO Matrix, corrupting `40 MHz DIO` XIP instruction fetches, and `map_psram` returns `1` byte when PSRAM readback fails, panicking inside `esp_alloc::HEAP.add_region` (`tlsf.rs:19` `unwrap()`).
        - Replaced `Psram::new` with a non-destructive hardware MMU check (`DPORT_PRO_CACHE_CTRL1_REG` `0x3FF0_0044` and `SPI0_CACHE_SCTRL_REG` `0x3FF4_303C`) that only registers `0x3F80_0000` (`4 MB`) when external SRAM is already mapped (`>= 1_048_576` B).
        - Restored `ring_storage: [u8; 16_384]` in `FirmwareBssBuffers` (`.bss`) so when `psram_avail_bytes < 1_048_576`, `StreamPipeline` uses `ring_storage` (`8 KB + 8 KB` in `.bss`) with `0` bytes allocated from the `64 KB` internal DRAM `esp-radio` Wi-Fi heap.
3. **Updated**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
   - **Rationale**: Packaged `--stage-only` OTA binary (`787,008` B, SHA-256 `a2cf7600f274e53a93944709e88b6ea703905c51f43d96a164c4f9bf3e225a42`) and verified `[STAGE-ONLY AUDIT PASS]`.

## 2. Known Risks & Untested Assumptions
- Staged binary verified via all `--stage-only` gates (`cargo test --workspace`, `cargo clippy`, `xtensa-esp32-none-elf` release build, `readelf -S`, `objdump -d`, `verify_staged_ota_bin.py`). Live OTA flash to `172.22.1.15` was not executed per `--stage-only` mandate.
