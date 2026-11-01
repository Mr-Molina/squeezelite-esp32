# Subagent Summary: WO-197 — 100% IRAM PSRAM Initializer & Stage-Only OTA Build

- **Work Order**: `WO-197`
- **Role**: `Bare-Metal PSRAM Initializer & Stage-Only Builder`
- **Timestamp**: `2026-10-27T22:51:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Files Created, Modified, or Deleted

1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (**Modified**; backup at `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/main.rs.bak_wo197`):
   - Added `#[esp_hal::ram] unsafe fn psram_send_cmd_spi1(cmd_byte: u32, qpi_mode: bool)` (`0x40080bf8` in `.rwtext`/`.iram1`) and `#[esp_hal::ram] unsafe fn init_and_map_psram_4mb() -> usize` (`0x40080d68` in `.rwtext`/`.iram1`).
   - Updated `psram_avail_bytes` initialization (`main.rs:1769-1799`) to call `init_and_map_psram_4mb()` and register `0x3F80_0000..0x3F80_0000 + mapped_bytes` (`4,194,304` B) with `esp_alloc::HEAP` when `mapped_bytes >= 1_048_576`.
   - Updated `/status.json` PSRAM heap stats reporting (`main.rs:2306-2318`) to initialize `free = 0u32; used = 0u32;` instead of the hardcoded `3_538_944` / `655_360` constants.
   - Updated the deferred HTTP reboot action (`main.rs:2469-2485`) to mask `PRO_CACHE_MASK_DRAM1 | PRO_CACHE_MASK_OPSDRAM` (`(1 << 3) | (1 << 5)`) in `0x3FF0_0044` and set `cs1_dis` (`1 << 1`) in `0x3FF4_3034` before `flash.erase` / `flash.write`.
2. `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin` (**Modified / Exported**):
   - Exported via `espflash save-image --chip esp32` (`957,440` bytes, `35.63%` of `ota_0`, SHA-256 `5444e41ba2a7cdee27efc630598d25782c23a56bc221af4ee847085dbc103d67`).
3. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-197-iram-psram-init-and-stage-build.md` (**Modified**):
   - Appended Fulfillment Summary and updated status to `[EXECUTED & VERIFIED]`.
4. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo197_psram_stage.md` (**Created**):
   - This summary document.

## 2. Verification Gates (`--stage-only`)

- **Gate 1 (`google_rust_audit.py --strict`)**: `0 error(s), 0 warning(s)` (`exit code 0`).
- **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**: `0` warnings (`exit code 0`).
- **Gate 3 (`cargo test --workspace`)**: `0` failures (`exit code 0`).
- **Gate 4 (Xtensa Release Build & Disassembly Audit)**:
  - `psram_send_cmd_spi1` (`0x40080bf8..0x40080d68`) and `init_and_map_psram_4mb` (`0x40080d68..0x400812ae`) reside 100% in `.rwtext` (`.iram1`), with all `l32r` literals placed in `.rwtext_start` (`0x40080408..0x40080594`) and all calls targeting ROM (`0x40009f0c`, `0x40009edc`, `0x40062bc8`, `0x400097f4`) or `.rwtext` (`0x40080bf8`). Zero calls to `.flash.text` and zero reads from `.flash.rodata`.
  - `.stack` = `0x009020` (`36,896` bytes `>= 36,000` bytes).
  - `.dram2_uninit` = `0x018098` (`98,456` bytes `<= 98,768` bytes).
- **Gate 5 (`verify_staged_ota_bin.py`)**:
  - `[STAGE-ONLY AUDIT PASS] path=/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin size=957440 (35.63% of ota_0) img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0' sha256=5444e41ba2a7cdee27efc630598d25782c23a56bc221af4ee847085dbc103d67`

## 3. Risks & Untested Assumptions

- Staged binary has not yet been uploaded or booted on `172.22.1.15` (`--stage-only` gate enforced per `WO-197`). Live OTA deployment awaits explicit user approval.
