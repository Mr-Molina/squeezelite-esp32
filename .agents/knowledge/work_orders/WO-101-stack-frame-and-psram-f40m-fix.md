# Work Order: WO-101 — Eliminate `run_hardware_firmware` `33.5 KB` Stack Frame & Fix `PsramCacheF40mS40m` + Zero-Heap Fallback (`--stage-only`)

## Metadata
- **Work Order ID**: `WO-101`
- **Timestamp**: `2026-10-25T16:22:00-04:00`
- **Assigned Subagent Role**: Rust App Stack-Frame & PSRAM Implementer (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
Remediate the two forensic root causes isolated by `WO-100` (`DIAGNOSTIC_LOG.md` Entry 54):
1. Eliminate the `+23 KB` stack frame bloat in `run_hardware_firmware` (`33,568` B frame inside `36,336` B `.stack`, which left only `2,768` B for `WifiController::new` and ISRs) by removing `NvsFlashBackend` (`~12.3 KB` on stack) and `MemConfigStore::<32>` (`~10.7 KB` on stack) from `run_hardware_firmware` and replacing them with `#[inline(never)]` functions that operate on `.bss` scratch buffers and a compact `MemConfigStore::<8>`.
2. Fix `esp_hal::psram::Psram::new` in `src/main.rs` so it does NOT corrupt `40 MHz DIO` `SPI0` XIP instruction fetches or starve the `64 KB` internal DRAM `esp-radio` Wi-Fi heap, and verify via `xtensa-esp32-elf-objdump -d` that `run_hardware_firmware`'s stack frame is `<= 14,336` bytes (`<= 14 KB`, leaving `>= 22,000` B of free stack space inside `.stack`).

## Target Files & Scope
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

## Required Implementation Steps
1. **Eliminate Stack Allocation in `NvsFlashBackend` (`src/hal.rs` & `src/main.rs`)**:
   - In `src/main.rs`:
     - Do **NOT** keep `NvsFlashBackend` (`4,108` B `scratch`) or `MemConfigStore::<32>` (`5,380` B) as local variables inside `run_hardware_firmware`!
     - Write two `#[inline(never)]` functions in `mod xtensa_hw`:
       1. `#[inline(never)] fn load_boot_audio_config(flash: &mut FlashStorage<'_>, scratch: &mut [u8; 12_288]) -> (AudioConfig, u32)`:
          - Splits `scratch` into `(sector_a, rest) = scratch.split_at_mut(4096)` and `(sector_b, journal_scratch) = rest.split_at_mut(4096)`.
          - Reads `SETTINGS_PARTITION_OFFSET` (`0x3FE000`) into `sector_a` and `SETTINGS_PARTITION_OFFSET + 4096` (`0x3FF000`) into `sector_b`.
          - Uses `FlashJournal::load_active(sector_a, sector_b)` directly into a tiny `MemConfigStore::<8>` (only `8 * 168 = 1,344` B, inside this `#[inline(never)]` helper whose stack frame is immediately popped upon return!).
          - Returns `(AudioConfig::load_from(&store).unwrap_or_default(), active_seq)`.
       2. `#[inline(never)] fn persist_audio_config(flash: &mut FlashStorage<'_>, scratch: &mut [u8; 12_288], cfg: &AudioConfig) -> Option<u32>`:
          - Splits `scratch` into `sector_a` (`[0..4096]`), `sector_b` (`[4096..8192]`), and `out_sector` (`[8192..12288]`) in `.bss` — **ZERO `4,096`-byte arrays on the stack!**
          - Reads `0x3FE000` into `sector_a` and `0x3FF000` into `sector_b`.
          - Populates `MemConfigStore::<8>` from `cfg.save_to(&mut store)`.
          - Calls `FlashJournal::prepare_commit(sector_a, sector_b, &store, out_sector)`.
          - Erases and writes only the target sector (`0x3FE000` or `0x3FF000`) from `out_sector`, and returns `Some(new_seq)`.
     - In `run_hardware_firmware`, store only `audio_cfg: AudioConfig` (`6` bytes!) and `settings_seq: u32` (`4` bytes!), and when an HTTP control command mutates EQ/AGAIN/volume, call `if let Some(seq) = persist_audio_config(&mut flash, nvs_scratch, &audio_cfg) { settings_seq = seq; }`!
2. **Fix `Psram` Initialization & Zero-Heap `.bss` Ringbuffer Fallback (`src/main.rs`)**:
   - Inspect `/home/console-admin/.cargo/registry/src/*/esp-hal-1.2.2/src/psram/esp32.rs` (`psram_gpio_config`, `psram_cache_init`, `map_psram`).
   - Notice two critical facts in `esp-hal-1.2.2/src/psram/esp32.rs`:
     1. `PsramConfig::default()` uses `PsramCacheSpeed::PsramCacheF80mS40m`, which calls `esp_rom_spiflash_config_clk(_SPI_80M_CLK_DIV, _SPI_CACHE_PORT)` on `SPI0` (`_SPI_CACHE_PORT`) while CPU 0 is executing XIP instructions from a `40 MHz DIO` (`0x02 0x20`) flash image! Even worse, in `esp-hal-1.2.2/src/psram/esp32.rs`, check what `PsramCacheSpeed::PsramCacheF40mS40m` does in `psram_gpio_config` (`esp32.rs:981`): does `psram_gpio_config` STILL reconfigure `SPI0` (`_SPI_CACHE_PORT`) pins and dummy cycles from flash (without `#[ram]`) while XIP is active?!
     2. Read lines 480–530 and 970–1060 of `esp-hal-1.2.2/src/psram/esp32.rs` carefully!
        - If `psram_gpio_config` in `esp-hal 1.2.2` is NOT marked `#[ram]` (meaning it executes from `0x400D_xxxx` Flash XIP while reconfiguring `SPI0` `_SPI_CACHE_PORT` flash pins/clock/dummy cycles!), OR if `172.22.1.15` (`I2S-4MFlash-1681`) faulted inside `Psram::new` because `psram_gpio_config` is in `.flash.text` instead of `.rwtext` (IRAM):
        - Check `xtensa-esp32-elf-nm` / `objdump` on the ELF binary: are `esp_hal::psram::esp32::init_psram`, `psram_gpio_config`, and `psram_cache_init` located in `.flash.text` (`0x400dxxxx` / `0x401xxxxx`) instead of IRAM (`0x4008xxxx`)?!
        - **YES!** In `esp-hal 1.2.2`, `psram_gpio_config` and `psram_cache_init` are in `.flash.text` (`0x4011xxxx`) and NOT in IRAM (`0x4008xxxx`), AND when `squeezelite-esp32`'s C `recovery` bootloader (`ESP-IDF v4.3`) boots `ota_0` (`I2S-4MFlash`), if `Psram::new` re-runs `psram_gpio_config` from `.flash.text`, reconfiguring `SPI0` while executing from `.flash.text` crashes XIP!
        - Wait: check if the C bootloader (`I2S-4MFlash-1681`) ALREADY initialized PSRAM or if `0x3F80_0000` is mapped, OR check how `esp-hal` / ESP32 MMU (`DPORT_PRO_EXT_MEM_ENA_MASK`) works!
        - Let's verify in `esp-hal-1.2.2/src/psram/esp32.rs` and `components/` / `sdkconfig.defaults` in the repo!
        - AND in `FirmwareBssBuffers`, keep a dedicated `ring_storage: [u8; 16_384]` in `.bss` (while `nvs_scratch` is a slice of `ring_storage` during boot before `StreamPipeline::new`!).
          Wait: think about that!
          - During boot (`load_nvs_wifi_credentials` and `load_boot_audio_config`), `StreamPipeline` does NOT exist yet!
          - So `ring_storage: [u8; 16_384]` in `.bss` can be used as the `16 KB` `nvs_scratch` at boot (`0` extra `.bss` bytes, restoring `ring_storage` to `16,384` B and shrinking `FirmwareBssBuffers` by `12,288` B!), expanding `.stack` from `36,336` B (`0x8df0`) to **`48,624` bytes (`0xBDF0` = `47.5 KB`)**!
          - Let's double-check that math!
            - In working commit `0e2f4732`, `FirmwareBssBuffers` had `ring_storage: [u8; 16_384]` and did NOT have a separate `nvs_scratch: [u8; 12_288]`!
            - In `5a837d44`, `WO-093` replaced `ring_storage: [u8; 16_384]` with `nvs_scratch: [u8; 12_288]`.
            - If we have `ring_storage: [u8; 16_384]` in `FirmwareBssBuffers` (used for the single-pass `16 KB` NVS read at boot, and then used as the fallback `8 KB + 8 KB` stream/output buffers if PSRAM is not active, while a small `4 KB` or `8 KB` slice or `#[inline(never)]` stack buffer inside `persist_audio_config` handles `0x3FE000` commits since `run_hardware_firmware`'s stack frame will be only `~11 KB` out of `36.5 KB`!), then:
              1. `load_nvs_wifi_credentials` reads all `16,384` bytes (`0x9000..0xD000`) in a single pass into `ring_storage: [u8; 16_384]` (100% identical to working commit `0e2f4732`!).
              2. `run_hardware_firmware`'s stack frame drops from `33,568` B (`0x8320`) back down to **`~12,500` B (`~0x3100`)**, leaving **`~24,000` bytes (`24 KB`)** of free stack space inside `.stack` (`~36,528` B)!
3. **Mandatory Disassembly & `--stage-only` Verification Gates**:
   - Run `cargo test --workspace` (`exit code 0`).
   - Run `cargo clippy --workspace --all-targets -- -D warnings` (`exit code 0`).
   - Build `xtensa-esp32-none-elf` release ELF (`exit code 0`).
   - Check BOTH:
     1. `xtensa-esp32-elf-readelf -S`: `.stack >= 36,000` B.
     2. `xtensa-esp32-elf-objdump -d`: Disassemble `run_hardware_firmware` and verify its stack frame subtraction (`sub a1, a1, a8`) is **`<= 14,336` bytes (`<= 0x3800`)**, leaving **`>= 22,000` bytes** of free stack headroom above `_bss_end`!
   - Package `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and verify `verify_staged_ota_bin.py` (`[STAGE-ONLY AUDIT PASS]`).
   - Commit to `MK5-Rust` and report `git rev-parse HEAD` SHA, `run_hardware_firmware` stack frame size in bytes, `.stack` section size in bytes, and `.bin` SHA-256.

## Fulfillment Section
### Execution & Verification Summary (`[EXECUTED & VERIFIED]`)
- **Execution Timestamp**: `2026-10-25T16:50:30-04:00`
- **Commit SHA (`git rev-parse HEAD`)**: `7717733ff2975243d77dc6513d14be38a71d2c36`
- **Rule 35 Backup**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/main.rs.wo101.bak`
- **Root Causes Remediated in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`**:
  1. **Stack Frame Reduction (`33,568` B -> `8,688` B)**:
     - Removed `Esp32SettingsFlashPartition`, `NvsFlashBackend`, and `MemConfigStore::<32>` from `run_hardware_firmware`.
     - Added `#[inline(never)] fn load_boot_audio_config` (`entry a1, 0x630` = `1,584` B, popped on return) and `#[inline(never)] fn persist_audio_config` (`entry a1, 0x5f0` = `1,520` B, popped on return) using `.bss` scratch slices (`ring_storage[..12_288]` at boot and `settings_scratch: [u8; 512]` at runtime) and `MemConfigStore::<8>`.
     - Added `#[inline(never)] fn init_static_pipeline` (`entry a1, 0x160` = `352` B, popped on return) to initialize `StreamPipeline` (`13,664` B) and `pending_resp_buf` (`2,064` B) in-place inside `.dram2_uninit` (`0x3ffe7e40..0x3ffebbd0`), reducing `run_hardware_firmware`'s prologue stack allocation from `33,568` B (`0x8320`) to **`8,688` bytes (`entry a1, 0x21f0`)**, leaving **`27,616` bytes (`26.97 KB`)** of free stack headroom inside `.stack` (`36,304` bytes).
  2. **PSRAM `SPI0` XIP Safety & Zero-Heap `.bss` Ringbuffer Fallback**:
     - Disassembly of `esp-hal 1.2.2`'s `psram_init` (`0x40083df8`) proved that `psram_gpio_config` calls `0x4013c190 <esp_hal::gpio::io_mux_reg>` in `.text` (XIP Flash over `SPI0`) 14 times while reconfiguring `SPI0` dummy cycles and switching `SPI0` Flash pins (`CS=11, Q=7, D=8, HD=9, WP=10`) from IOMUX to GPIO Matrix, and `map_psram` returns `1` byte when PSRAM is absent, panicking in `esp-alloc` TLSF (`tlsf.rs:19` `unwrap()`).
     - Replaced `Psram::new` with a non-destructive hardware MMU check (`DPORT_PRO_CACHE_CTRL1_REG` `0x3FF0_0044` and `SPI0_CACHE_SCTRL_REG` `0x3FF4_303C`) and restored `ring_storage: [u8; 16_384]` in `FirmwareBssBuffers` (`.bss`), ensuring `0` bytes are taken from the `64 KB` internal DRAM `esp-radio` Wi-Fi heap when `psram_avail_bytes < 1_048_576`.
- **Verification Gates (`[EXECUTED & VERIFIED]`)**:
  1. `cargo test --workspace`: `exit code 0` (`216` unit/fuzz tests passed, `0` failures).
  2. `cargo clippy --workspace --all-targets -- -D warnings`: `exit code 0` (`0` warnings).
  3. `xtensa-esp32-none-elf` release build: `exit code 0`.
  4. `xtensa-esp32-elf-readelf -S`: `.stack` at `0x3ffd7230`, size `0x008dd0` = **`36,304` bytes (`>= 36,000` B)**.
  5. `xtensa-esp32-elf-objdump -d`: `4011d2a4 <_RNvNtCs205ZPEWQKAY_16louder_esp32_app9xtensa_hw21run_hardware_firmware>: 43e136 entry a1, 0x21f0` = **`8,688` bytes (`<= 14,336` B)**, leaving **`27,616` bytes (`>= 22,000` B)** of free stack headroom above `_bss_end` (`0x3ffd7230`).
  6. `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]` (`path=/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, `size=787,008` bytes, `sha256=a2cf7600f274e53a93944709e88b6ea703905c51f43d96a164c4f9bf3e225a42`).
- **Subagent Summary**: [20261025_wo101_stack_frame_psram_implementer.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo101_stack_frame_psram_implementer.md)
