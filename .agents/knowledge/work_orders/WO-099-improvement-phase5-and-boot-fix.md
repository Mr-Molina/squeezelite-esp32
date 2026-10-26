# Work Order: WO-099 — `/improvement` Phase 5 + `WO-096` Boot Rollback Fix & `--stage-only` Build

## Metadata
- **Work Order ID**: `WO-099`
- **Timestamp**: `2026-10-25T15:56:30-04:00`
- **Assigned Subagent Role**: Rust App & Improvement Implementer (`self`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
1. Apply the user-approved `/improvement` Phase 5 mitigations (`IMP-2026-1025-003`..`006`) to `.agents/INFRASTRUCTURE_INVARIANTS.md`, `.agents/PATTERN_LIBRARY.yaml`, and `/home/console-admin/.gemini/config/pattern-library.yaml`.
2. Remediate all root causes of the `WO-096` boot rollback in `rust/crates/louder-esp32-app/src/ota_safety.rs` and `rust/crates/louder-esp32-app/src/main.rs`, add unit test coverage for split 12 KB NVS Page 0 + Page 3 credential resolution, verify all 5 `--stage-only` gates, and commit to `MK5-Rust`.

## Target Files & Scope
- `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`
- `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`
- `/home/console-admin/.gemini/config/pattern-library.yaml`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

## Acceptance Criteria
1. **Rule 35 Pre-Modification Backups**:
   - Create backups of `.agents/INFRASTRUCTURE_INVARIANTS.md`, `.agents/PATTERN_LIBRARY.yaml`, `/home/console-admin/.gemini/config/pattern-library.yaml`, `rust/crates/louder-esp32-app/src/ota_safety.rs`, and `rust/crates/louder-esp32-app/src/main.rs` under `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/` before editing.
2. **Apply `/improvement` Phase 5 Mitigations**:
   - Append **Invariant 45** (Hardware & Workspace Feature-Maximization Audit Gate), **Invariant 46** (8-Byte Stereo Frame DMA Alignment & Underrun Zero-Fill Gate), and **Invariant 47** (Split NVS Page-0 Namespace Pinning, Gate-1-First Boot Order, & Extension Certifier Source-Edit Ban) to `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`.
   - Append `PAT-2026-1025-003`, `PAT-2026-1025-004`, `PAT-2026-1025-005` and append the `PM-2026-1025-006` occurrence to `PAT-2026-1014-001` in `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`.
   - Append the `PM-2026-1025-026` occurrence to `GLB-017` in `/home/console-admin/.gemini/config/pattern-library.yaml`.
3. **Remediate `WO-096` Boot Rollback in `src/ota_safety.rs` & `src/main.rs`**:
   - Inspect `/home/console-admin/.cargo/registry/src/*/esp-hal-1.2.2/src/psram/esp32.rs` and `/home/console-admin/.cargo/registry/src/*/esp-alloc-0.11.0/src/macros.rs` to verify how `Psram::new` and `psram_allocator!` initialize external PSRAM on ESP32-WROVER (`CLK=17, CS=16`) and how `esp_alloc::HEAP` selects between `Internal` and `External` memory capabilities.
   - **Critical Check on `esp_alloc::HEAP` Capability Priority**:
     - When `esp_alloc::heap_allocator!(size: 64 * 1024)` (Internal DRAM) is registered BEFORE `psram_allocator!` (External PSRAM), calling `alloc::vec![0u8; 524_288]` (standard `GlobalAlloc::alloc`) tries `region[0]` (`64 KB` Internal DRAM) first, which fails, and then falls back to `region[1]` (`4 MB` PSRAM) — **WAIT**: what about small allocations made by `Box::new(FlacDecoder::new())` or `esp-radio` Wi-Fi driver? If `region[0]` (`64 KB` Internal DRAM) is registered first, `esp-radio`'s internal DRAM allocations use `region[0]`. **WAIT**: why did `5a837d44` roll back on `172.22.1.15`?
     - Check BOTH causes discovered during `WO-096`:
       1. **Cause A (Split 12 KB NVS Read in `load_nvs_wifi_credentials`)**: In `src/main.rs:893-914`, Pass 1 read `0x9000..0xC000` (Pages 0, 1, 2) and Pass 2 read `0xA000..0xD000` (Pages 1, 2, 3), overwriting Page 0 (`0x9000..0xA000`, where `config_ns_id` and `aplist_ns_id` are defined). Fix `load_nvs_wifi_credentials` (and add a helper in `src/ota_safety.rs` with unit test `test_split_12kb_nvs_read_with_page0_namespaces_and_page3_credentials`) so `nvs_buf[0..4096]` stays pinned with Page 0 (`0x9000..0xA000`) while Pass 2 reads `NVS_PARTITION_OFFSET + 8192` (`0xB000..0xD000`, Pages 2 & 3) into `nvs_buf[4096..12_288]`!
       2. **Cause B (Boot Order, `Psram::new`, Core 1 IRAM, and `SETTINGS_PARTITION_OFFSET`)**:
          - Check what `WO-096` found when bisecting `src/main.rs` on `172.22.1.15`! Why did `WO-096` inspect `esp_hal::psram::Psram::new` vs `psram_allocator!`? Read the last 30 steps of `/home/console-admin/.gemini/antigravity/brain/8a197cfd-50c4-4cfc-a310-70bfed0f30e9/.system_generated/logs/transcript.jsonl` via a tiny Python script if needed (remember: you are the subagent, never dump raw JSONL to stdout, only print step summaries!) to verify whether `172.22.1.15` rolled back due to `load_nvs_wifi_credentials`, `psram_allocator!`, `CpuControl::start_app_core`, or `Esp32SettingsFlashPartition`!
          - Ensure:
            - Gate 1 (`otadata` `0xD000..0xF000` erase + read-back verification) is the **very first operation** after `esp_hal::init` and `wdt` init.
            - `load_nvs_wifi_credentials` (with Page 0 pinned in `nvs_buf[0..4096]` for Pass 2) and `Esp32SettingsFlashPartition::new` (`SETTINGS_PARTITION_OFFSET = 0x3FE000`, without erasing/writing flash at boot if `load_store` returns `Err`) run BEFORE starting PSRAM or Core 1.
            - Core 1 (`APP_CPU`) runs an `#[esp_hal::ram]` function (`#[esp_hal::ram] fn app_core_loop() -> !`) so Core 1 never fetches instructions from SPI0 flash cache.
            - Verify how `esp_hal::psram` initializes PSRAM on `172.22.1.15` (`ESP32-WROVER`) and ensure that if `psram.raw_parts()` returns `psram_size == 0` for any reason, the firmware does not panic in `unwrap()` or OOM!
4. **Verify All 5 `--stage-only` Gates**:
   - `cargo test --workspace` (`exit code 0`)
   - `cargo clippy --workspace --all-targets -- -D warnings` (`exit code 0`)
   - `source /home/console-admin/export-esp.sh && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app` (`exit code 0`)
   - `xtensa-esp32-elf-readelf -S` confirms `.stack >= 36,000` B (`0x8CA0`)
   - `espflash save-image --chip esp32 ... /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` returns `[STAGE-ONLY AUDIT PASS]`.
   - Commit changes to `MK5-Rust` and report full `git rev-parse HEAD` SHA and binary SHA-256.

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Commit SHA (`git rev-parse HEAD`)**: `cbc35902f1d09db362ba026fc13b57aa3f2d445c`
- **Staged Binary Path**: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
- **Staged Binary Size**: `757,440` bytes (`28.19%` of `ota_0`)
- **Staged Binary SHA-256**: `c61a9df6d32dd74224c6f3d335a49a47a025fd426292a4b1a3fc26dcb422c217`
- **ELF `.stack` Size**: `0x008df0` (`36,336` bytes `>= 36,000` B)
- **Summary**:
  1. Created Rule 35 pre-modification backups in `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/backups_maximization/`.
  2. Applied `/improvement` Phase 5 mitigations (`Invariant 45`..`47` in `.agents/INFRASTRUCTURE_INVARIANTS.md`, `PAT-2026-1025-003`..`005` + `PAT-2026-1014-001` in `.agents/PATTERN_LIBRARY.yaml`, and `PM-2026-1025-026` under `GLB-017` in `/home/console-admin/.gemini/config/pattern-library.yaml`).
  3. Implemented `load_nvs_wifi_credentials_split_12kb` and `test_split_12kb_nvs_read_with_page0_namespaces_and_page3_credentials` in `rust/crates/louder-esp32-app/src/ota_safety.rs`, moved Gate 1 (`otadata` erase) + NVS/Settings flash reads before PSRAM/Core1 initialization in `rust/crates/louder-esp32-app/src/main.rs`, set `SETTINGS_PARTITION_OFFSET = 0x3FE000` without boot-time flash erase/write, guarded `Psram::new` (`raw_parts()`), and placed `app_core_loop` in IRAM (`#[esp_hal::ram]`).
  4. Verified all 5 `--stage-only` gates (`217` workspace tests pass, `clippy -D warnings` clean, `xtensa-esp32-none-elf` release build clean, `.stack == 36,336` B, and `verify_staged_ota_bin.py` `[STAGE-ONLY AUDIT PASS]`).
- **Handoff Report**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo099_rust_improvement_implementer.md`
