# Subagent Summary: WO-109 — Eliminate Runtime `SPI1` NOR Flash Deadlock in `persist_audio_config` & Fix `I2S0_CLKM_CONF_REG` `div_a`/`div_b` Bitfield Order (`--stage-only`)

- **Work Order**: `WO-109` ([WO-109-fix-persist-audio-config-and-i2s-clkm.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-109-fix-persist-audio-config-and-i2s-clkm.md))
- **Subagent Role**: `Rust App Flash-Journal & I2S CLKM Implementer` (`self`, Conversation ID `a3d90645-d9e4-4f49-a0ca-6eee9549623c`)
- **Timestamp**: `2026-10-25T19:19:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `da35fa61e92084c64b5710594a52a2a5f091c08d` (`da35fa61`)

## 1. Files Created, Modified, or Deleted (Absolute Paths)
1. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/main.rs.wo109.bak` (Created)
   - **Rationale**: Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
2. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/WO-109-fix-persist-audio-config-and-i2s-clkm.md.bak` (Created)
   - **Rationale**: Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-109-fix-persist-audio-config-and-i2s-clkm.md`.
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (Modified)
   - **Rationale**:
     - Changed `persist_audio_config` (`src/main.rs:1017-1029`) to `(scratch: &mut [u8; 512], cfg: &AudioConfig, current_seq: u32) -> Option<u32>` so it serializes the `FlashJournal` sector into `scratch` in RAM and returns `Some(next_seq)` with zero runtime `SPI1` NOR Flash `read_nor`/`erase`/`write_nor` calls while `APP_CPU`, `4 MB` PSRAM `SPI0` cache, and `esp-radio` Wi-Fi are running, and updated all 3 call sites (`SetEqPreset`, `SetAnalogGain`, and `PlayerIrCommand`).
     - Fixed `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) bitfield order in `Esp32HardwareI2sDma::set_clock_rate` (`src/main.rs:889`) to `((div_a & 0x3F) << 8) | ((div_b & 0x3F) << 14)` so `clkm_div_a` (`bits [13:8]`, denominator = `63`) and `clkm_div_b` (`bits [19:14]`, numerator = `11`) produce `44,099.99 Hz`.
     - Added `#[inline(never)] pub fn prefill_silence(buf: &mut esp_hal::dma::DmaTxStreamBuf)` pushing `4 * 2,048 = 8,192` B (`46.4 ms`) of silence before `i2s_tx.write(...)` in both `ensure_running` (`#[inline(never)]`) and `run_hardware_firmware`, keeping `run_hardware_firmware`'s stack frame at `8,480` B.
4. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/verify_wo109_elf.py` (Created)
   - **Rationale**: Verified `.stack >= 35,000` B, `run_hardware_firmware <= 9,000` B (`8,480` B), and `0` `spiflash_*` calls inside `persist_audio_config`.
5. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (Modified)
   - **Rationale**: Packaged `--stage-only` OTA binary (`756,240` bytes, SHA-256 `8b74c66645a20ee1e680e2bf31e1bd56edb705b67baf65df57cc5295ec56d8f4`).
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-109-fix-persist-audio-config-and-i2s-clkm.md` (Modified)
   - **Rationale**: Appended Rule 15 Fulfillment Section and updated status to `[EXECUTED & VERIFIED]`.
7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo109_persist_clkm_implementer.md` (Created)
   - **Rationale**: Rule 17 subagent summary.
8. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/handoff.md` (Modified)
   - **Rationale**: Rule 17 handoff report.

## 2. Verification Metrics (`--stage-only`)
- `cargo test --workspace`: `0` failures (`259` tests passed).
- `cargo clippy --workspace --all-targets -- -D warnings`: `0` warnings (`exit code 0`).
- `xtensa-esp32-elf-readelf -S`:
  - `.stack`: `0x008bd0` = `35,792` bytes (`>= 35,000` B gate PASS)
  - `.data`: `0x003dd0` = `15,824` bytes (`<= 20,000` B gate PASS)
  - `.bss`: `0x023440` = `144,448` bytes
  - `.dram2_uninit`: `0x004990` = `18,832` bytes
- Disassembly Audit (`verify_wo109_elf.py`):
  - `run_hardware_firmware` (`0x4011d1d0`, size `42,804` B): `entry a1, 8480` (`0x2120` B `<= 9,000` B gate PASS).
  - `persist_audio_config` (`0x4011c98c`, size `2,113` B): `entry a1, 1472`, **`0` calls** to `spiflash_*` or `FlashStorage`.
- `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, `756,240` bytes (`28.14%` of `ota_0`), SHA-256 `8b74c66645a20ee1e680e2bf31e1bd56edb705b67baf65df57cc5295ec56d8f4`.

## 3. Known Risks & Untested Assumptions
- Strictly `--stage-only`: `172.22.1.15` was not flashed or modified. Live OTA certification (`WO-110`) will verify all 7 `/cmd.json` interactive mutations, `tas_fs_hz: 44100`, and the 35s audio soak on `172.22.1.15`.
