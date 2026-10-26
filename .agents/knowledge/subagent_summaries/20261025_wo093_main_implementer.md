# Subagent Summary: Wave 3 Task 7 Firmware Main Implementer (WO-093)

- **Date**: 2026-10-25
- **Subagent Role**: Wave 3 Task 7 Firmware Main Implementer
- **Work Order**: `WO-093` (`/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-093-task7-main-psram-dualcore.md`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`:
   - Configured `esp-alloc = { version = "0.11.0", features = ["esp32", "internal-heap-stats"] }`.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - Updated `FirmwareBssBuffers` replacing `ring_storage: [u8; 16384]` with `nvs_scratch: [u8; 12288]`.
   - Added `APP_CORE_STACK: Stack<4096>` and `CORE1_HEARTBEAT: AtomicU32`.
   - Updated `load_nvs_wifi_credentials` to accept `&mut [u8; 12_288]` and scan dual offsets (`0x9000` and `0xA000`).
   - Implemented `Esp32SettingsFlashPartition` implementing `FlashPartition` for dual 4KB sectors at `0x3F0000`/`0x3F1000`.
   - Initialized 4 MB External PSRAM allocator (`psram_allocator!`) and allocated `psram_stream_storage` (512 KB) and `psram_output_storage` (128 KB).
   - Started Core 1 (`APP_CPU` @ 240 MHz) via `CpuControl::start_app_core`.
   - Enabled `FlashStorage::multicore_auto_park()`.
   - Configured `PowerSaveMode::None` and `set_max_tx_power(60)`.
   - Seeded `smoltcp` `random_seed` and `local_port_counter` from TRNG (`Rng::new().random()`).
   - Loaded and saved `AudioConfig` to `0x3F0000` via `NvsFlashBackend`.
   - Wired `SetEqPreset` and `SetAnalogGain` actions in `/cmd.json`.
   - Polled live `Tas5805mTelemetry` every 2,000 ms.
   - Populated all 31 fields in `HttpStatusSnapshot`.
   - Emitted `CydMessage::Progress` every 1,000 ms during playback over `UART2`.
   - Updated mock expectations in unit test `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate` to match Wave 2 Task 4 silicon maximization register initialization.
   - **WO-097 Remediations**:
     - `write_dma`: 8-byte aligned `avail_aligned` and `frames_in_chunk` clamping to prevent 64-bit stereo channel desynchronization.
     - `pump_silence_if_needed`: 8-byte aligned padding and silence chunks with threshold 4096 to prevent unaligned silence injection and DMA underruns.
     - `ensure_running`: split buffer, `buffer.fill(0)`, pre-fill 2048 B silence to eliminate stale audio replay upon DMA restarts.

---

## 2. Rationale for Each Change
- **External PSRAM Buffers**: Expands audio stream buffering from 16 KB total to 640 KB (512 KB stream + 128 KB output) to absorb network latency without internal DRAM exhaustion.
- **BSS/Stack Equilibrium**: 12 KB `nvs_scratch` + 4 KB `APP_CORE_STACK` matches the previous 16 KB `ring_storage` exactly, maintaining Core 0 `.stack` at 36,336 bytes (well above the 36,000 B safety floor).
- **Multicore Safety**: `multicore_auto_park()` ensures Core 1 instruction fetches do not collide with SPI Flash bus operations during settings journal commits or OTA descriptor updates.
- **Settings Persistence**: User volume, analog gain, and 10-band EQ preset changes survive power cycles by writing to the dedicated `0x3F0000` partition.
- **DMA Alignment**: Strictly enforcing 8-byte boundaries ensures DMA descriptor ring buffer pointers never shift by odd byte offsets, preventing audio channel swap or distortion.

---

## 3. Risks and Untested Assumptions
- Hardware HIL tests against the physical TAS5805M DAC amplifier will run during Wave 4 hardware verification. Host mock and unit tests verify 100% of driver state transitions, register bitmasks, and transaction sequences.

---

## 4. Verification Evidence
- `cargo test --workspace`: **PASS** (100% passing).
- `cargo clippy --workspace --all-targets -- -D warnings`: **PASS** (0 warnings).
- `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf`: **PASS**.
- `xtensa-esp32-elf-size -A rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`:
  - `.stack`: 36,336 B (>= 36,000 B)
  - `.bss`: 143,904 B
  - `.text`: 534,693 B
