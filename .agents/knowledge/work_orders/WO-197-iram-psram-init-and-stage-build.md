# Work Order: WO-197

- **Work Order ID**: `WO-197`
- **Timestamp**: `2026-10-27T22:41:30-04:00`
- **Parent Conversation ID**: `6b0546bc-7be8-4fb1-bca2-7848797b09ed`
- **Assigned Subagent Role**: `Bare-Metal PSRAM Initializer & Stage-Only Builder`
- **Task Goal**: Implement a 100% IRAM-resident (`#[esp_hal::ram]`) `init_and_map_psram_4mb()` routine in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, remove the hardcoded PSRAM stats fallback at `main.rs:1950-1951`, and verify all 5 `--stage-only` build/audit/disassembly gates without flashing any device.
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Scope & Target Files

- **Exclusive Write Ownership**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (backup already created at `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/main.rs.bak_wo197`)
  - `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin` (staged binary output)
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo197_psram_stage.md`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-197-iram-psram-init-and-stage-build.md`
- **Strict Prohibitions**:
  - Do **NOT** flash or reboot `172.22.1.15` in `WO-197` (`--stage-only` only).
  - Do **NOT** open, probe, or touch `/dev/ttyUSB0`.
  - Do **NOT** use `sed`, `awk`, or `cat` to modify files (Rule 23).
  - Keep all lines `<= 100` characters (`GRS-012`), include dual `// JUSTIFICATION:` and `// SAFETY:` comments on every `unsafe` block/fn (`GRS-008`), and ban production `.unwrap()` (`GRS-010`).

---

## 2. Technical Specification (`louder-esp32-app/src/main.rs`)

### A. Implement `#[esp_hal::ram] unsafe fn init_and_map_psram_4mb() -> usize`
Add `init_and_map_psram_4mb()` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (gated under `#[cfg(target_arch = "xtensa")]`) so that every instruction executes from `.iram1` (`#[esp_hal::ram]`) and calls **only** inline volatile MMIO reads/writes and ROM functions (`0x4000_xxxx`/`0x4006_xxxx`), with **zero** calls into `.flash.text` (`0x400D_0000..0x403F_FFFF`) and **zero** loads from `.flash.rodata` (`0x3F40_0000..0x3F7F_FFFF`):
1. Declare ROM externs inside `init_and_map_psram_4mb`:
   - `esp_rom_gpio_connect_out_signal(gpio_num: u32, signal_idx: u32, out_inv: bool, oen_inv: bool)`
   - `esp_rom_gpio_connect_in_signal(gpio_num: u32, signal_idx: u32, inv: bool)`
   - `esp_rom_spiflash_config_clk(freqdiv: u8, spi: u8) -> i32`
   - `cache_sram_mmu_set_rom(cpu_no: u32, pid: u32, vaddr: u32, paddr: u32, psize: u32, num: u32) -> i32`
   - `static mut g_rom_spiflash_dummy_len_plus: u8`
2. Verified MMIO addresses (`esp32-0.41.0` PAC):
   - `SPI0_BASE = 0x3FF4_3000`:
     - `SPI0_CTRL_REG = 0x3FF4_3008`
     - `SPI0_CTRL2_REG = 0x3FF4_3014`
     - `SPI0_USER_REG = 0x3FF4_301C`
     - `SPI0_USER1_REG = 0x3FF4_3020`
     - `SPI0_PIN_REG = 0x3FF4_3034`
     - `SPI0_CACHE_SCTRL_REG = 0x3FF4_3054`
     - `SPI0_SRAM_DRD_CMD_REG = 0x3FF4_305C`
     - `SPI0_SRAM_DWR_CMD_REG = 0x3FF4_3060`
     - `SPI0_EXT2_REG = 0x3FF4_30F8`
     - `SPI0_EXT3_REG = 0x3FF4_30FC`
     - `SPI0_DATE_REG = 0x3FF4_33FC`
   - `SPI1_BASE = 0x3FF4_2000`:
     - `SPI1_CMD_REG = 0x3FF4_2000`
     - `SPI1_ADDR_REG = 0x3FF4_2004`
     - `SPI1_CTRL_REG = 0x3FF4_2008`
     - `SPI1_CTRL2_REG = 0x3FF4_2014`
     - `SPI1_USER_REG = 0x3FF4_201C`
     - `SPI1_USER1_REG = 0x3FF4_2020`
     - `SPI1_USER2_REG = 0x3FF4_2024`
     - `SPI1_MOSI_DLEN_REG = 0x3FF4_2028`
     - `SPI1_MISO_DLEN_REG = 0x3FF4_202C`
     - `SPI1_PIN_REG = 0x3FF4_2034`
     - `SPI1_SLAVE_REG = 0x3FF4_2038`
     - `SPI1_W0_REG = 0x3FF4_2080`
     - `SPI1_EXT2_REG = 0x3FF4_20F8`
   - `DPORT_BASE = 0x3FF0_0000`:
     - `DPORT_PRO_CACHE_CTRL_REG = 0x3FF0_0040`
     - `DPORT_PRO_CACHE_CTRL1_REG = 0x3FF0_0044`
     - `DPORT_APP_CACHE_CTRL_REG = 0x3FF0_0058`
     - `DPORT_APP_CACHE_CTRL1_REG = 0x3FF0_005C`
     - `DPORT_HOST_INF_SEL_REG = 0x3FF0_00BC`
   - `IO_MUX_BASE = 0x3FF4_9000`:
     - `IO_MUX_GPIO16_REG = 0x3FF4_904C` (`PSRAM CS1`)
     - `IO_MUX_GPIO17_REG = 0x3FF4_9050` (`PSRAM CLK`)
     - `IO_MUX_GPIO9_REG = 0x3FF4_9054` (`SD2 / SPIHD`)
     - `IO_MUX_GPIO10_REG = 0x3FF4_9058` (`SD3 / SPIWP`)
     - `IO_MUX_GPIO11_REG = 0x3FF4_905C` (`FLASH CS0`)
     - `IO_MUX_GPIO6_REG = 0x3FF4_9060` (`FLASH CLK`)
     - `IO_MUX_GPIO7_REG = 0x3FF4_9064` (`SD0 / SPIQ`)
     - `IO_MUX_GPIO8_REG = 0x3FF4_9068` (`SD1 / SPID`)
3. Execution sequence inside `init_and_map_psram_4mb` (`PsramCacheF40mS40m`, `PsramClkModeNorm`, `ESP32-D0WDQ6` / `WROVER-E`):
   - Wait for `SPI0` (`read_volatile(SPI0_EXT2_REG) & 0x7 != 0`) and `SPI1` (`read_volatile(SPI1_EXT2_REG) & 0x7 != 0` and `read_volatile(SPI1_CMD_REG) & (1 << 18) != 0`) to be idle.
   - Set `SPI0_EXT3_REG = 0x1` and clear `usr_prep_hold` (bit 23) in `SPI1_USER_REG`.
   - Initialize `SPI1` (`psram_spi_init`): clear `trans_inten` (bit 9) and `mode` (bit 30) in `SPI1_SLAVE_REG`, clear `ck_idle_edge` (bit 29) in `SPI1_PIN_REG`, clear `ck_out_edge` (bit 7) and `doutdin` (bit 0) in `SPI1_USER_REG`, clear `wr_bit_order` (bit 26) and `rd_bit_order` (bit 25) in `SPI1_CTRL_REG`, write `0` to `SPI1_USER1_REG`, zero `SPI1_W0_REG..+16`, and set `cs_hold` (bit 4) + `cs_setup` (bit 5) in `SPI1_USER_REG` and `hold_time = 0, setup_time = 0` (`& !0xFF`) in `SPI1_CTRL2_REG`.
   - Route PSRAM `CLK` (`GPIO17`) to `SPICLK_OUT_IDX` (`0`): `esp_rom_gpio_connect_out_signal(17, 0, false, false)`.
   - Configure `SPI0` dummy cycles & `40 MHz` clock (`PsramCacheF40mS40m`, `extra_dummy = 1`):
     - Read `rd_mode_reg = read_volatile(SPI0_CTRL_REG)`.
     - If `(rd_mode_reg & (1 << 24)) != 0` (`SPI_FREAD_QIO_M`): `spi_cache_dummy = 3`.
     - Else if `(rd_mode_reg & (1 << 23)) != 0` (`SPI_FREAD_DIO_M`): `spi_cache_dummy = 1` and set `usr_addr_bitlen` (bits `31:26`) of `SPI0_USER1_REG` to `27`.
     - Else: `spi_cache_dummy = 7`.
     - Write `1` to `g_rom_spiflash_dummy_len_plus` offset `0` (`_SPI_CACHE_PORT`) and offset `1` (`_SPI_FLASH_PORT`).
     - Update `SPI0_USER1_REG` `usr_dummy_cyclelen` (bits `7:0`) to `spi_cache_dummy + 1` and set `usr_dummy` (bit 29) in `SPI0_USER_REG`.
     - Call `esp_rom_spiflash_config_clk(2, 0)` and `esp_rom_spiflash_config_clk(2, 1)`.
     - Set `FUN_DRV = 2` (`(val & !(0x3 << 10)) | (2 << 10)`) on `IO_MUX_GPIO6_REG` and `IO_MUX_GPIO17_REG`.
     - Connect GPIO Matrix signals:
       - `esp_rom_gpio_connect_out_signal(11, 5, false, false)` (`CS0`)
       - `esp_rom_gpio_connect_out_signal(16, 6, false, false)` (`CS1`)
       - `esp_rom_gpio_connect_out_signal(7, 1, false, false)` + `esp_rom_gpio_connect_in_signal(7, 1, false)` (`SPIQ / SD0`)
       - `esp_rom_gpio_connect_out_signal(8, 2, false, false)` + `esp_rom_gpio_connect_in_signal(8, 2, false)` (`SPID / SD1`)
       - `esp_rom_gpio_connect_out_signal(10, 4, false, false)` + `esp_rom_gpio_connect_in_signal(10, 4, false)` (`SPIWP / SD3`)
       - `esp_rom_gpio_connect_out_signal(9, 3, false, false)` + `esp_rom_gpio_connect_in_signal(9, 3, false)` (`SPIHD / SD2`)
     - Configure `IO_MUX` functions (`MCU_SEL` bits `14:12` and `FUN_IE` bit 9 on data lines):
       - `IO_MUX_GPIO6_REG`: `(val & !(0x7 << 12)) | (1 << 12)` (`FUNC_SD_CLK_SPICLK = 1`)
       - `IO_MUX_GPIO11_REG`: `(val & !(0x7 << 12)) | (2 << 12)` (`PIN_FUNC_GPIO = 2`)
       - `IO_MUX_GPIO16_REG`: `(val & !(0x7 << 12)) | (2 << 12)` (`PIN_FUNC_GPIO = 2`)
       - `IO_MUX_GPIO17_REG`: `(val & !(0x7 << 12)) | (2 << 12)` (`PIN_FUNC_GPIO = 2`)
       - `IO_MUX_GPIO7_REG`, `IO_MUX_GPIO8_REG`, `IO_MUX_GPIO9_REG`, `IO_MUX_GPIO10_REG`: `((val & !(0x7 << 12)) | (2 << 12)) | (1 << 9)` (`PIN_FUNC_GPIO = 2` + `FUN_IE = 1`)
     - Re-assert `esp_rom_gpio_connect_out_signal(28, 256, false, false)`, `esp_rom_gpio_connect_out_signal(29, 256, false, false)`, `esp_rom_gpio_connect_out_signal(17, 0, false, false)`.
   - Set CS timing on `SPI1` and `SPI0`:
     - Set `cs_hold` (bit 4) and `cs_setup` (bit 5) in `SPI1_USER_REG` and `SPI0_USER_REG`.
     - Clear `hold_time` (`7:4`) and `setup_time` (`3:0`) (`& !0xFF`) in `SPI1_CTRL2_REG` and `SPI0_CTRL2_REG`.
   - Send PSRAM QPI mode entry on `SPI1` (supporting both warm reboot from QPI and cold boot from SPI):
     - Helper closure/block inlined in `init_and_map_psram_4mb` to send an 8-bit `addr` (`cmd_byte << 24`, `usr_addr_bitlen = 7`) on `SPI1` with `CS1` enabled (`cs1_dis = 0, cs0_dis = 1`) and `DPORT_HOST_INF_SEL_REG |= 1 << 14`:
       1. First send `0xF5` (`PSRAM_EXIT_QMODE`) in QPI mode (`fwrite_qio = 1, fread_qio = 1`), restoring `SPI1` registers afterward.
       2. Then send `0x35` (`PSRAM_ENTER_QMODE`) in SPI mode (`fwrite_qio = 0, fread_qio = 0`), restoring `SPI1` registers afterward.
   - Configure `SPI0` SRAM cache registers (`psram_cache_init` for `PsramCacheF40mS40m`, `PsramVaddrMode::Normal`, `PsramClkModeNorm`, `extra_dummy = 1`):
     - Clear bits `31:30` of `SPI0_DATE_REG` (`0x3FF4_33FC`).
     - In `SPI0_CACHE_SCTRL_REG` (`0x3FF4_3054`):
       - Clear `usr_sram_dio` (bit 1)
       - Set `usr_sram_qio` (bit 2)
       - Set `usr_rd_sram_dummy` (bit 4)
       - Set `cache_sram_usr_rcmd` (bit 5)
       - Set `sram_dummy_cyclelen` (bits `21:14`) to `(5 + 1) = 6` (`PSRAM_FAST_READ_QUAD_DUMMY + extra_dummy`)
       - Set `sram_addr_bitlen` (bits `27:22`) to `23`
       - Set `cache_sram_usr_wcmd` (bit 28)
     - Write `(7 << 28) | 0xEB` to `SPI0_SRAM_DRD_CMD_REG` (`0x3FF4_305C`).
     - Write `(7 << 28) | 0x38` to `SPI0_SRAM_DWR_CMD_REG` (`0x3FF4_3060`).
     - Clear `pro_dram_hl` (bit 16) and `pro_dram_split` (bit 11) in `DPORT_PRO_CACHE_CTRL_REG` (`0x3FF0_0040`).
     - Clear `app_dram_hl` (bit 16) and `app_dram_split` (bit 11) in `DPORT_APP_CACHE_CTRL_REG` (`0x3FF0_0058`).
     - Clear `pro_cache_mask_dram1` (bit 3), `pro_cache_mask_opsdram` (bit 5), and `pro_cmmu_sram_page_mode` (bits `8:6`) in `DPORT_PRO_CACHE_CTRL1_REG` (`0x3FF0_0044`).
     - Clear `app_cache_mask_dram1` (bit 3), `app_cache_mask_opsdram` (bit 5), and `app_cmmu_sram_page_mode` (bits `8:6`) in `DPORT_APP_CACHE_CTRL1_REG` (`0x3FF0_005C`).
     - Clear `cs1_dis` (bit 1) in `SPI0_PIN_REG` (`0x3FF4_3034`).
   - Map `4 MB` (`0x3F80_0000`, `128` pages of `32 KB`) into `PRO_CPU` and `APP_CPU` MMU via ROM:
     - `let _ = cache_sram_mmu_set_rom(0, 0, 0x3F80_0000, 0, 32, 128);`
     - `write_volatile(DPORT_APP_CACHE_CTRL1_REG, read_volatile(DPORT_APP_CACHE_CTRL1_REG) & !(1 << 3));`
     - `let _ = cache_sram_mmu_set_rom(1, 0, 0x3F80_0000, 0, 32, 128);`
   - Probe `0x3F80_0000..0x3FC0_0000` (`MAX_MEM_SIZE = 4 * 1024 * 1024`):
     - Test if `(0x3F80_0000 as *mut u32)` writes `0xA55A_5AA5` and reads back `0xA55A_5AA5`; if not, also send `0x35` (`PSRAM_ENTER_QMODE`) in `PsramCmdQpi` mode (`fwrite_qio = 1`) as a fallback.
     - Run the full `4 MB` stride verification across `(1023..MAX_MEM_SIZE).step_by(1024)`: write `0x7F` to `ptr.add(i)`, then read back `ptr.add(i)` across `(1023..MAX_MEM_SIZE).step_by(1024)` (and clear `ptr.add(i).write_volatile(0)` after verifying).
     - If `guessed_size >= 1_048_576`, return `guessed_size`; otherwise re-mask `pro_cache_mask_dram1 | pro_cache_mask_opsdram` (`(1 << 3) | (1 << 5)`) in `DPORT_PRO_CACHE_CTRL1_REG`, set `cs1_dis = 1` in `SPI0_PIN_REG`, and return `0`.

### B. Update `main.rs:1417-1444`, `main.rs:1950-1951`, and `main.rs:2111-2120`
1. At `main.rs:1417-1444`:
   - Call `let mapped_bytes = unsafe { init_and_map_psram_4mb() };`
   - If `mapped_bytes >= 1_048_576`, call `esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(0x3F80_0000 as *mut u8, mapped_bytes, esp_alloc::MemoryCapability::External.into()))` and evaluate `mapped_bytes` as `psram_avail_bytes`.
2. At `main.rs:1950-1951`:
   - Change `let mut free = 3_538_944u32; let mut used = 655_360u32;` to `let mut free = 0u32; let mut used = 0u32;` so `/status.json` always reports real live `esp_alloc::HEAP.stats()` for PSRAM.
3. At `main.rs:2111-2120` (deferred HTTP reboot action right before `flash.erase` / `software_reset()`):
   - Before `if settings_dirty`, mask `PRO_CACHE_MASK_DRAM1 | PRO_CACHE_MASK_OPSDRAM` (`(1 << 3) | (1 << 5)`) in `0x3FF0_0044` and set `cs1_dis` (`1 << 1`) in `0x3FF4_3034` so `SPI1` NOR Flash write/erase right before reboot cannot collide with `SPI0` PSRAM cache.

---

## 3. Mandatory Verification Gates (`--stage-only`)

1. **Gate 1 (`rust-pro` Static Audit)**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict` -> must output **`0 error(s), 0 warning(s)`** (`exit code 0`).
2. **Gate 2 (Host Clippy)**:
   - Run `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings` -> **`0` warnings** (`exit code 0`).
3. **Gate 3 (Host Unit & Fuzz Tests)**:
   - Run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` -> **`0` failures** (`exit code 0`).
4. **Gate 4 (Xtensa Release Build, IRAM Disassembly Audit, & `.stack` / `.dram2_uninit` Check)**:
   - Build `louder-esp32-app` for `xtensa-esp32-none-elf` in `--release`.
   - Run `xtensa-esp32-elf-objdump -d` on `init_and_map_psram_4mb` in `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`:
     - Verify `init_and_map_psram_4mb` resides in `.iram1` (`0x4008_xxxx..0x4009_xxxx`).
     - Verify **zero** `call8`/`call4`/`call12` targets in `init_and_map_psram_4mb` point to `.flash.text` (`0x400D_0000..0x403F_FFFF`) and **zero** `l32r` literals load from `.flash.rodata` (`0x3F40_0000..0x3F7F_FFFF`).
   - Run `xtensa-esp32-elf-readelf -S`:
     - Verify `.stack >= 36,000` bytes (`0x8CA0`).
     - Verify `.dram2_uninit <= 98,768` bytes (`0x181D0`).
5. **Gate 5 (`espflash save-image` & `verify_staged_ota_bin.py`)**:
   - Export `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin` and run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/verify_staged_ota_bin.py` -> must output **`[STAGE-ONLY AUDIT PASS]`** and report exact byte size and SHA-256.

---

## Fulfillment Section

- **Completed By**: Subagent `09f49b66-af57-4193-9b23-be04943fb742` (`Bare-Metal PSRAM Initializer & Stage-Only Builder`)
- **Timestamp**: `2026-10-27T22:51:00-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wo197_psram_stage.md`
- **Key Deliverables & Verification Results**:
  1. Implemented `#[esp_hal::ram] unsafe fn psram_send_cmd_spi1` (`0x40080bf8`) and `#[esp_hal::ram] unsafe fn init_and_map_psram_4mb() -> usize` (`0x40080d68`) in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, wired `init_and_map_psram_4mb()` into `psram_avail_bytes`, removed the hardcoded `3_538_944` / `655_360` fallback in `/status.json`, and masked `PRO_CACHE_MASK_DRAM1 | PRO_CACHE_MASK_OPSDRAM` + `cs1_dis` prior to deferred HTTP reboot flash writes.
  2. **Gate 1 (`google_rust_audit.py --strict`)**: `0 error(s), 0 warning(s)` (`exit code 0`).
  3. **Gate 2 (`cargo clippy --workspace --all-targets -- -D warnings`)**: `0` warnings (`exit code 0`).
  4. **Gate 3 (`cargo test --workspace`)**: `0` failures (`exit code 0`).
  5. **Gate 4 (Xtensa Release Build & IRAM Disassembly Audit)**:
     - `psram_send_cmd_spi1` (`0x40080bf8..0x40080d68`) and `init_and_map_psram_4mb` (`0x40080d68..0x400812ae`) execute 100% from `.rwtext` (`.iram1`), with all `l32r` literals in `.rwtext_start` (`0x40080408..0x40080594`) and all `callx8` instructions targeting ROM (`0x40009f0c`, `0x40009edc`, `0x40062bc8`, `0x400097f4`) or `.rwtext` (`0x40080bf8`). Zero `.flash.text` calls and zero `.flash.rodata` loads.
     - `.stack` = `0x009020` (`36,896` B `>= 36,000` B); `.dram2_uninit` = `0x018098` (`98,456` B `<= 98,768` B).
  6. **Gate 5 (`espflash save-image` & `verify_staged_ota_bin.py`)**:
     - `[STAGE-ONLY AUDIT PASS] path=/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/scratch/louder-esp32-app-basement-ota.bin size=957440 (35.63% of ota_0) img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0' sha256=5444e41ba2a7cdee27efc630598d25782c23a56bc221af4ee847085dbc103d67`
  7. Zero network writes to `172.22.1.15` and zero serial probes on `/dev/ttyUSB0`.
