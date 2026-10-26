# Work Order: WO-020 — Audit ESP32 OTA Packaging & `172.22.1.15` OTA Hardware Requirements

**Work Order ID:** WO-020  
**Timestamp:** 2026-10-20T22:35:00-04:00  
**Assigned Subagent Role:** Extension Certifier / Infrastructure Administrator  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/partitions.csv` (Read-Only Audit)  
**Backup Files:** N/A (Read-Only Audit)  
**Defect IDs:** `HW-PREP-06`  
**Task Goal:** Audit what is required to link and package `louder-esp32-app` into a flashable ESP32 OTA binary (`.bin`) for `172.22.1.15` (`ota_0` at `0x160000`, `partitions.csv`) without triggering any reboot on `172.22.1.15` (strictly enforcing `staging-before-restart.md`).

---

## 1. Scope & Instructions
1. **Strict Guardrail (`staging-before-restart.md`)**:
   - Do **NOT** send any POST or reboot/OTA trigger request (`/reboot_ota.json`, `/recovery.json`, `/flash.json`) to `172.22.1.15`.
2. **Inspect Partition Table & OTA Architecture in Repo**:
   - Read `/home/console-admin/Github/squeezelite-esp32/partitions.csv` (and any `sdkconfig.defaults*` for `I2S-4MFlash`) to confirm exact partition offsets and sizes for `172.22.1.15`.
3. **Test Linking `louder-esp32-app` for ESP32 Target (`+esp`)**:
   - Source `/home/console-admin/export-esp.sh` and test what happens when running `cargo +esp build --release -Zbuild-std=std,panic_abort --target xtensa-esp32-espidf -p louder-esp32-app`.
   - Record the exact linker output and identify what crates/linker flags are required to produce a standalone ESP32 `.elf` and `.bin` image (`espflash save-image --chip esp32`) for `louder-esp32-app`, as well as whether `louder-esp32-app` currently initializes physical ESP32 Wi-Fi / HTTP OTA recovery endpoints or uses in-memory HAL mocks (`LoopbackWifiDriver`, `TargetI2cBus`, `LoopbackI2sDma`).
4. **Test Generating `espflash save-image` for `cyd-controller-app` (Reference Bare-Metal Binary)**:
   - Run `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/cyd-controller-app /home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/cyd-controller-app.bin` to verify `espflash save-image` works on this host.
5. Wait until all background tasks complete (`manage_task(list)` = 0 active tasks per Rule 21), write a Rule 17 summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo020_ota_packaging_audit.md`, and update the Fulfillment Section below to `[EXECUTED & VERIFIED]`.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T22:42:00-04:00
- **Files Modified / Created (Absolute Paths):**
  * `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/cyd-controller-app.bin` (Created)
  * `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo020_ota_packaging_audit.md` (Created)
  * `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-020-ota-packaging-audit.md` (Updated)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Audit Findings Summary:**
  1. **Partition Table & OTA Geometry:** `partitions.csv` specifies 4MB flash layout with factory recovery partition at `0x10000` (`0x150000` / ~1.31 MB), `ota_0` app partition at `0x160000` (`0x290000` / ~2.56 MB), and `settings` NVS at `0x3F0000` (`0x10000` / 64 KB).
  2. **Packaging Toolchain Validation:** Executed `/home/console-admin/.cargo/bin/espflash save-image --chip esp32` on `cyd-controller-app` ELF binary. Generated valid 162,320-byte flashable raw image (`cyd-controller-app.bin`) with zero errors.
  3. **Build & Linker Audit on `xtensa-esp32-espidf`:**
     - In `--release` (`opt-level=3`, `lto="fat"`), `rustc` fails with `rustc-LLVM ERROR: Cannot select: 0x7f842be83cc0: i32 = Constant<-8192>` in `ActiveDecoder::decode` due to an upstream Xtensa LLVM ISel bug.
     - Without fat LTO, the compiler reaches the link phase, failing with unresolved external references to ESP-IDF and C runtime symbols (`cosf`, `sinf`, `expf`, `posix_memalign`, `lwip_*`, `esp_fill_random`, `realpath`). Target `xtensa-esp32-espidf` with `std` requires linking against compiled ESP-IDF components (`esp-idf-sys` / `esp-idf-svc` or CMake build artifacts).
  4. **Hardware vs Mocks Audit:** `louder-esp32-app` currently utilizes in-memory mocks (`LoopbackWifiDriver`, `TargetI2cBus`, `LoopbackI2sDma`, `MemFlashPartition`) and does not initialize ESP32 Wi-Fi hardware (`esp_wifi_init`) or host an HTTP OTA recovery web server. Flashing directly to `ota_0` on `172.22.1.15` would strip Wi-Fi and remote management.
  5. **Safety Constraints:** Staging-before-restart guardrail observed; no HTTP POST, reboot, or OTA requests were directed to `172.22.1.15`. `board.rs`, `amp.rs`, and `main.rs` were left untouched.
