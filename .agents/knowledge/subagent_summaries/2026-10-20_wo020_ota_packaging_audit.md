# Subagent Summary: WO-020 ESP32 OTA Packaging & Hardware Audit

- **Date / Timestamp:** 2026-10-20T22:42:00-04:00
- **Subagent Role:** Extension Certifier / Infrastructure Administrator
- **Assigned Work Order:** WO-020 (`WO-020-ota-packaging-audit.md`)
- **Status:** `[EXECUTED & VERIFIED]`

---

## 1. Files Created / Modified
1. `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/cyd-controller-app.bin` (Created via `espflash save-image`)
2. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo020_ota_packaging_audit.md` (Created)
3. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-020-ota-packaging-audit.md` (Modified — Fulfillment section updated)

*(Note: `board.rs`, `amp.rs`, and `main.rs` were strictly UNTOUCHED, and no HTTP/reboot requests were sent to `172.22.1.15`).*

---

## 2. Rationale & Empirical Findings

### A. Flash Partition Geometry (`partitions.csv` & `sdkconfig.defaults`)
- **Flash Size:** 4MB (`CONFIG_ESPTOOLPY_FLASHSIZE_4MB=y`, `CONFIG_PARTITION_TABLE_OFFSET=0x8000`)
- **Partition Table Scheme:**
  * `0x009000` - `0x00D000` (`0x4000`, 16 KB): `nvs` (data/nvs)
  * `0x00D000` - `0x00F000` (`0x2000`, 8 KB): `otadata` (data/ota)
  * `0x00F000` - `0x010000` (`0x1000`, 4 KB): `phy_init` (data/phy)
  * `0x010000` - `0x160000` (`0x150000`, 1,376,256 B / ~1.31 MB): `recovery` (app/factory) — The recovery/web updater image
  * `0x160000` - `0x3F0000` (`0x290000`, 2,686,976 B / ~2.56 MB): `ota_0` (app/ota_0) — Primary running Squeezelite app partition
  * `0x3F0000` - `0x400000` (`0x10000`, 64 KB): `settings` (data/nvs)

### B. Target Image Packaging Verification (`espflash save-image`)
- Tested host toolchain command:
  ```bash
  /home/console-admin/.cargo/bin/espflash save-image --chip esp32 \
    /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/cyd-controller-app \
    /home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/cyd-controller-app.bin
  ```
- **Result:** Exit code 0. Generated 162,320-byte valid raw binary image (`cyd-controller-app.bin`, 3.93% partition utilization). Confirms `espflash save-image` is fully functional and ready to package ESP32 binaries.

### C. Build & Linker Audit on `xtensa-esp32-espidf` (`louder-esp32-app`)
1. **Release Mode (`opt-level=3`, `lto="fat"`):**
   - Failed during LLVM instruction selection:
     ```
     rustc-LLVM ERROR: Cannot select: 0x7f842be83cc0: i32 = Constant<-8192>
     In function: _RNvXNtCsaGyagnFqAwS_16louder_esp32_app8pipelineNtB2_13ActiveDecoderNtCsiS8k237h50c_20squeezelite_decoders12AudioDecoder6decode
     ```
   - **Root Cause:** Upstream Xtensa LLVM backend bug in instruction selection (`ISel`) when compiling `ActiveDecoder::decode` with fat LTO and constant `-8192` (`PCM_SCRATCH_SAMPLES`).
2. **Debug Mode (Non-LTO Codegen -> Linker):**
   - LLVM code generation succeeded and invoked `xtensa-esp32-elf-gcc`.
   - Linker failed with unresolved external references to ESP-IDF and C runtime symbols:
     * C math library (`libm`): `cosf`, `sinf`, `expf`, `atanf`, `exp2f`, `log`, `cos`, `sin`, `exp2`, `log2`, `powf`
     * POSIX memory: `posix_memalign`
     * LwIP network stack: `lwip_socket`, `lwip_connect`, `lwip_recv`, `lwip_send`, `lwip_setsockopt`, `lwip_getsockopt`, `lwip_getaddrinfo`, `lwip_freeaddrinfo`, `poll`, `ioctl`
     * ESP-IDF system runtime: `esp_fill_random`, `realpath`
   - **Linker Requirement:** Target `xtensa-esp32-espidf` using `std` requires linking against the compiled ESP-IDF static archives and linker scripts (typically managed via `esp-idf-sys` / `esp-idf-svc` or by linking against the IDF CMake build artifacts). Currently `louder-esp32-app` does not incorporate `esp-idf-sys` or an ESP-IDF build script, so native C IDF symbols are missing at link time.

### D. Physical Hardware vs HAL Mocks Audit in `louder-esp32-app`
- **Wi-Fi Hardware:** Does **NOT** initialize physical ESP32 Wi-Fi (`esp_wifi_init`). Uses `LoopbackWifiDriver` and an in-memory `WifiManager` state machine.
- **HTTP / OTA Server:** Does **NOT** implement an HTTP server or any OTA update/recovery endpoints (`/reboot_ota.json`, `/recovery.json`). These exist solely in the C recovery firmware.
- **Peripherals:**
  * Audio DMA: Uses `LoopbackI2sDma` wrapped in `EspI2sSink`.
  * Amplifier Control: Uses `TargetI2cBus` (256-byte simulated register array).
  * CYD UART: Uses `LoopbackUartPort` wrapped in `CydUartBridge`.
  * Flash NVS: Uses `MemFlashPartition` wrapped in `NvsFlashBackend`.
- **Conclusion:** `louder-esp32-app` is currently an architectural application core and simulation-capable host engine; flashing it directly over OTA to `172.22.1.15` would strip Wi-Fi and remote management.

---

## 3. Risks & Untested Assumptions
- **Risk:** OTA flashing to `ota_0` on `172.22.1.15` before `louder-esp32-app` has live ESP-IDF Wi-Fi network and FreeRTOS tasks integrated would render `172.22.1.15` unreachable over IP (requiring recovery partition boot via physical button or serial flashing).
- **Tested Verification:** Verified that `espflash save-image` works on host, and documented exact linker missing symbols on `xtensa-esp32-espidf`.
