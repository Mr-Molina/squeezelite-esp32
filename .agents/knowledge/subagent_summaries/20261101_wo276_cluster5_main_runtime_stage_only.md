# Subagent Summary: WO-276 — Perf Phase 3 Wave 1B Cluster 5: `main.rs` Bare-Metal Runtime Integration & Stage-Only Build

- **Date / Timestamp**: 2026-11-01T15:38:00-05:00
- **Subagent Role**: `Cluster 5 Bare-Metal Runtime Integrator & Stage-Only Packager`
- **Work Order**: `WO-276`
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, `nanomp3-core/src/minimp3.rs`, and `xtensa-esp32-none-elf` release build
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified / Created
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (Modified)
   - Rationale: Integrated Phase 3 Wave 1B performance enhancements + pre-flash memory hardening:
     - `PROT-504` & `PROT-502`: Initialized `output_threshold_frames: 4096` and `threshold_satisfied: false` in `init_static_pipeline`; implemented `can_read(&self)` on `Esp32VhciTransport` using `esp_radio::ble::have_hci_read_data()`.
     - `NET-401` & `HW-PERF-202`: Dynamic PSRAM socket buffer allocation (`8,192` B `slim_rx_buf` and `65,536` B `audio_rx_buf`) when `psram_avail_bytes >= 655_360`, increasing TCP receive window by 16x.
     - `NET-402`: Added immediate `smol_iface.poll(...)` egress directly after `slim_session_phase` in `pump_device_phase`.
     - `HW-PERF-103`: Added `wifi_connected_logged: bool` to `DeviceRuntimeState` and set `PowerSaveMode::None` immediately upon Wi-Fi connection to prevent modem sleep.
     - `CPU-102`: Expanded I2S0 DMA buffer to `16,384` B (`46.44 ms` audio cushion) with 16 descriptors placed in `.dram2_uninit`, freeing `8,192` B from `.bss` and expanding `.stack` headroom.
     - `CPU-103d`: Added `#[unsafe(link_section = ".iram1")]` to `write_dma` and `#[esp_hal::ram]` to `output_decode_phase`.
     - `CPU-104c`: Added `last_fault_recovery_ms: u64` and guarded amplifier fault recovery with `fault.has_critical_amp_fault()` and a 30-second cooldown in `amp_and_cyd_health_phase`.
     - `PROT-501`: Removed wildcard mDNS responder branch.
     - `PROT-503`: Replaced dynamic heap allocations with `[0u8; 1536]` and `[0u8; 1024]` stack buffers for RTP and Spotify HTTP.
     - `PROT-502`: Throttled `bt_poll_phase` to 2 ms intervals when idle.
     - `NET-404`: Increased `req_buf` to `1024` B in `http_serve_phase`.
     - **Pre-Flash Internal Heap Hardening (65 KB Total `MemoryCapability::Internal`)**:
       - `STATIC_BT_HEAP`: Placed `14 * 1024` B (`14 KB`) in `.dram2_uninit` and registered as Region 2 of `esp_alloc::HEAP`.
       - `heap_allocator!`: Increased `.bss` internal heap to `51 * 1024` B (`51 KB`). Total internal heap = `51 KB + 14 KB = 65 KB` (100% parity with `WO-257`).
       - `FirmwareBssBuffers.ring_storage`: Sized at `[u8; 4096]` (splitting at 2048), freeing `8,192` B in `.dram2_uninit`.
       - `boot_scratch`: Slices uninitialized `STATIC_BT_HEAP` for 12 KB flash config / NVS wifi credentials during early single-core boot prior to heap registration.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core/src/minimp3.rs` (Modified)
   - Rationale: Formatted GRS-008 justification comments on lines 1230 and 1395 to stay within the 80-character limit, achieving 100% compliance under `google_rust_audit.py --strict`.
3. `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin` (Created / Replaced)
   - Rationale: Packaged `--stage-only` OTA binary with verified 65 KB internal heap layout.
4. `/home/console-admin/.gemini/antigravity/brain/c5a8b4ca-61f6-4f13-8d2e-4e3d1aa7f879/task.md` (Updated)
   - Rationale: Active task ledger tracking backups, failure counter, atomic targets, and hypotheses.

---

## 2. Gate Verification Evidence

### Gate 1: Workspace Tests
- **Command**: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
- **Result**: `ok. 43 passed` (cyd_controller_app), `ok. 318 passed` (louder_esp32_app), `ok. 10 passed` (fuzz), `ok. 25 passed` (squeezelite_raop), `ok. 18 passed` (squeezelite_tas58xx).
- **Exit Code**: `0` (0 failed across all crates).

### Gate 2: Workspace Clippy
- **Command**: `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
- **Result**: 0 warnings, clean compilation.
- **Exit Code**: `0`.

### Gate 3: Google Rust Style Audit
- **Command**: `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/*/src --strict`
- **Result**: `0 error(s), 0 warning(s)`.
- **Exit Code**: `0`.

### Gate 4: Xtensa Release Build & ELF Section Audit
- **Command**: `source /home/console-admin/export-esp.sh && cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app --bin louder-esp32-app`
- **Result**: Built `target/xtensa-esp32-none-elf/release/louder-esp32-app` in 7.88s (`exit code 0`).
- **Section Audit via `llvm-readelf -S`**:
  - `.stack`: `0x0091b0` = `37,296` bytes (`>= 36,000` B target, margin `+1,296` B) -> **PASS**
  - `.iram0.text` (`.rwtext` + `.rwtext.wifi`): `0x006d7c + 0x01814c` = `126,664` bytes (`<= 131,072` B target, margin `+4,408` B free) -> **PASS**
  - `.dram2_uninit`: `0x0178d8` = `96,472` bytes (`<= 98,768` B `dram2_seg`, margin `+2,296` B free) -> **PASS**
  - Total `MemoryCapability::Internal` heap: `51 KB` (.bss) + `14 KB` (.dram2_uninit) = `65 KB` -> **PASS**

### Gate 5: `--stage-only` Packaging & Verification
- **Packaging Command**: `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin`
- **Packaging Output**: `App/part. size: 1,887,440/4,128,768 bytes, 45.71%` (`exit code 0`).
- **Verification Command**: `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/repo_scratch_archive/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin`
- **Verification Output**:
  `[STAGE-ONLY AUDIT PASS] path=/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-perf.bin size=1887440 (70.24% of ota_0) img_magic=0xE9 app_desc_magic=0xABCD5432 project_name='Squeezelite-ESP32' version='I2S-4MFlash-Rust' idf_ver='4.3.0' sha256=265872e611388dee54ce5d86c5e82b26d00aa7b96b168bb21d9f5ae07783a88c`
- **Exit Code**: `0`.

---

## 3. Risks & Assumptions
- **PSRAM Allocation Fallback**: If PSRAM initialization fails (`psram_avail_bytes < 655_360`), `slim_rx_slice` and `audio_rx_slice` fall back to static buffers in `fw_bufs` without runtime disruption.
- **Hardware Boundary**: Flashing, rebooting, and serial communication to `172.22.1.15` and `/dev/ttyUSB0` were strictly avoided per `--stage-only` isolation constraints.
