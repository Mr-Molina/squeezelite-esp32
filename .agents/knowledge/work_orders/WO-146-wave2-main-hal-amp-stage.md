# Work Order `WO-146`: Phase 3 Wave 2 — `louder-esp32-app` Hardware Integration, Workspace Verification & `--stage-only` OTA Image Packager

- **ID**: `WO-146`
- **Timestamp**: `2026-10-25T22:18:30-04:00`
- **Subagent Role**: `louder-esp32-app` Hardware Integration & `--stage-only` Packager (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. File Scope & Constraints
- **Target Source Files Owned by `WO-146`**:
  1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
  2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
  3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- **Staged OTA Output Path (`--stage-only` — DO NOT flash `172.22.1.15` or `/dev/ttyUSB0`!)**:
  - `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
- **Backups Already Created**: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/`
- **Hardware Safety**: **NEVER touch `/dev/ttyUSB0`**. **DO NOT** send `POST /recovery.json` or `POST /flash.json` to `172.22.1.15` (per `staging-before-restart.md`, Tier-3 firmware is strictly `--stage-only`).

## 2. Assigned Remediations (`CODE-*` & `HW-*`)

### A. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
1. **`CODE-TAS-001` / `HW-I2C-001` (`amp.rs:57-65, 96-107`)**:
   - In `percent_to_attenuation(percent: u8) -> u8`: unify with `0.5 dB`/step (`0% => 0xFF`, `1..=100% => 0x30 + (100 - clamped)` so `100% => 0x30`, `50% => 0x62`, `1% => 0x93`), matching `lms_gain_to_tas5805m_vol_reg`. Update unit tests in `amp.rs` asserting `percent_to_attenuation`.
   - In `AmpManager::with_address`: initialize `current_attenuation: cfg.initial_volume` instead of hardcoded `0x60` (update any unit test in `amp.rs` expecting `0x60` if `cfg.initial_volume == 0x58`).
2. **`CODE-TAS-004` (`amp.rs:195-210` & `amp::tests`)**:
   - Update mock I2C expectations in `amp::tests` to match `WO-142`'s `PAGE_SELECT (0x00) = 0x00` before `BOOK_SELECT (0x7F) = 0x00` order in `try_init_into_standby` and `reconfirm_book_page_and_clear_faults`.
   - In `AmpManager::initialize`: ensure `Hi-Z` (`0x02`) is staged before `configure_silicon_maximization` (or verify `amp::tests` expectations match).

### B. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
3. **`CODE-HAL-001` (`hal.rs:525-535`)**:
   - In `NvsFlashBackend::commit_store`: eliminate the `4,096`-byte stack temporary `let mut b = *self.flash.read_sector(1)?;`. Pass `self.flash.read_sector(0)?` and `self.flash.read_sector(1)?` directly by reference to `DualSectorFlashStore::active_sector(s0, s1)` (and read the active sequence number directly from the slice headers) before serializing into `&mut self.scratch`.
4. **`CODE-HAL-002` (`hal.rs:203-220`)**:
   - In `EspI2sSink::write_frames`: ensure `aligned_end - written_bytes >= 8` before calling `self.driver.write_dma(&self.dma_scratch[written_bytes..aligned_end])` (if `< 8`, break out of the loop cleanly) and round `n` down to an 8-byte frame boundary (`n & !7`) if needed so a partial write never loops 64 times into false `Err(OutputError::HardwareFault)`.
5. **`CODE-HAL-003` (`hal.rs:364-376, 561-573`)**:
   - In `CydUartBridge::send_message`: if the write loop exits with `offset < bytes.len()`, return `Err(HalError::BusWriteFailed)` instead of `Ok(offset)`.
   - In `NvsFlashBackend::erase_range`: return `Err(HalError::StorageBoundsError)` if `range.is_empty() || range.start >= range.end || range.end > FLASH_SECTOR_SIZE * 2`.

### C. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
6. **`CODE-MAIN-001` / `HW-CLK-001` (`main.rs:884-896`)**:
   - Define a pure helper function `pub fn encode_i2s0_clkm_conf(orig: u32, div_num: u32, div_b: u32, div_a: u32) -> u32`:
     ```rust
     pub const fn encode_i2s0_clkm_conf(orig: u32, div_num: u32, div_b: u32, div_a: u32) -> u32 {
         (orig & !0x000F_FFFF)
             | (div_num & 0xFF)
             | ((div_b & 0x3F) << 8)
             | ((div_a & 0x3F) << 14)
     }
     ```
   - Call `encode_i2s0_clkm_conf(val, div_num, div_b, div_a)` inside `Esp32HardwareI2sDma::set_clock_rate` (`main.rs:891`), placing `div_b` (`11`, numerator) in bits `[13:8]` and `div_a` (`63`, denominator) in bits `[19:14]` per ESP32 TRM Register 12.6 and `esp-hal-1.2.2/src/i2s/master/low_level/v1.rs:32-33`.
   - Add a unit test in `main.rs` verifying `encode_i2s0_clkm_conf(0, 14, 11, 63)` produces `(14) | (11 << 8) | (63 << 14)` (`(val >> 8) & 0x3F == 11` and `(val >> 14) & 0x3F == 63`).
7. **`CODE-MAIN-002` / `HW-I2C-001` (`main.rs:1937-1949`)**:
   - In `ServerCommand::AudioGain { gain_left, gain_right, .. }`: derive `let new_pct = if vol_reg >= 0xFF { 0 } else { 100u8.saturating_sub(vol_reg.saturating_sub(0x30)) };`, update `current_vol_pct = new_pct; audio_cfg.volume = new_pct;`, call `persist_audio_config(settings_scratch, &audio_cfg, &mut settings_seq);`, and update `cyd_bridge`.
8. **`CODE-MAIN-003` & `CODE-MAIN-004` / `HW-MEM-001` (`main.rs:938, 980, 1018-1029, 1756-1770`)**:
   - Import `SETTINGS_JOURNAL_OFFSET` (`0x003F_E000`) from `louder_esp32_app::ota_safety::SETTINGS_JOURNAL_OFFSET` (`CODE-MAIN-004`) instead of defining a shadowed `const SETTINGS_PARTITION_OFFSET: u32 = 0x3FE000;`.
   - Expand `settings_scratch: [u8; 4096]` in `FirmwareBssBuffers`.
   - Track `let mut settings_dirty = false;` (set to `true` whenever `persist_audio_config` succeeds in RAM).
   - In the deferred HTTP reboot block (`HttpControlAction::RebootToRecovery` and `HttpControlAction::PersistOta0AndReboot`, right before `esp_hal::system::software_reset()`), if `settings_dirty` is `true`, park `APP_CPU` (`unsafe { cpu_ctrl.park_core(esp_hal::system::Cpu::AppCpu); }`) and commit `settings_scratch` (`4,096` B) to `SETTINGS_JOURNAL_OFFSET` (`0x003F_E000`) via `let _ = flash.erase(SETTINGS_JOURNAL_OFFSET, SETTINGS_JOURNAL_OFFSET + 4096); let _ = flash.write(SETTINGS_JOURNAL_OFFSET, settings_scratch);`.
9. **`CODE-MAIN-005` & `CODE-MAIN-011` / `HW-CPU-001` & `HW-CPU-002` (`main.rs:967-978`)**:
   - Annotate `APP_CORE_STACK: esp_hal::system::Stack<4096>` with `#[link_section = ".dram2_uninit"]` so moving `4,096` B out of `.bss` offsets the `+3,584` B expansion of `settings_scratch` and increases `PRO_CPU` `.stack` headroom by `+512` B (`36,240 -> 36,752` B)!
   - In `app_core_loop()`, replace `core::hint::spin_loop();` with `unsafe { core::arch::asm!("nop"); }` inside `for _ in 0..10_000` so LLVM cannot eliminate the delay loop on Xtensa.
10. **`CODE-MAIN-006` (`main.rs:2302-2321, 2445-2467, 2566-2626`)**:
    - Only set `threshold_sent = true`, `track_started_sent = true`, and `decode_ready_sent = true` AFTER confirming `slim_sock.may_send()` and `slim_sock.send_slice(&pkt[..len]) == Ok(len)`.
11. **`CODE-MAIN-007` (`main.rs:925-934, 1031-1035`)**:
    - Guard `take_static_mp3_decoder()` and `take_firmware_bss_buffers()` with `static TAKEN: AtomicBool = AtomicBool::new(false); assert!(!TAKEN.swap(true, Ordering::AcqRel));` and `// SAFETY:` comments.
12. **`CODE-MAIN-008` / `HW-NET-001` (`main.rs:2386-2391, 2469-2528`)**:
    - Track `audio_req_sent_offset: usize` when sending `audio_req_buf[..audio_req_len]` over `audio_sock` so partial TCP writes complete cleanly, and allow up to `32` iterations (`for _ in 0..32`) in the `audio_sock` RX drain loop while `pipeline.stream_buffer.available_write() >= 512`.
13. **`CODE-MAIN-009` (`main.rs:1517-1520, 1795-1812`)**:
    - On `dhcpv4::Event::Deconfigured`, abort `slim_sock`, `audio_sock`, and `http_sock` and reset `helo_sent = false` and `audio_stream_active = false`.
    - Gate `!slim_sock.is_open()` reconnect attempts with a `1,000 ms` cooldown (`last_slim_connect_ms`).
14. **`CODE-MAIN-010` / `HW-I2C-002` (`main.rs:735-753, 1613-1621`)**:
    - In `Esp32HardwareI2sDma::ensure_running()`, treat `None => true` for `needs_restart`.
    - At lines `1613-1621`, pass `coordinator.current_sample_rate` to `t.detected_sample_rate_hz_with_hint(coordinator.current_sample_rate)` so `tas_fs_hz` reports `44100` during `44.1 kHz` playback.

## 3. Mandatory 5-Gate `--stage-only` Verification & Packaging (`verify-autonomous-work.md`)
Run and document the exact terminal output for all 5 gates:
1. **Gate 1 — Full Host Workspace Unit, Stress & Fuzz Suite**:
   - `cargo test --workspace --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml` (`exit code 0`).
2. **Gate 2 — Full Workspace Clippy Zero-Warning Audit**:
   - `cargo clippy --workspace --all-targets --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -- -D warnings` (`exit code 0`).
3. **Gate 3 — Bare-Metal Xtensa Release Build (`louder-esp32-app` & `cyd-controller-app`)**:
   - Source `/home/console-admin/export-esp.sh` and run:
     - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --features esp32-xtensa` (`exit code 0`).
     - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p cyd-controller-app` (`exit code 0`).
4. **Gate 4 — ELF `.stack` Budget & Disassembly Proof (`readelf -S` & `objdump -d`)**:
   - Verify `.stack` in `target/xtensa-esp32-none-elf/release/louder-esp32-app` is `>= 36,240` bytes (`~36,752` B = `0x008f90`) and `run_hardware_firmware` stack frame is `< 10,000` bytes.
   - Verify via `xtensa-esp32-elf-objdump -d` that `set_clock_rate` writes `div_b` (`11`) to bits `[13:8]` and `div_a` (`63`) to bits `[19:14]`, and `app_core_loop` executes `nop` inside its inner loop.
5. **Gate 5 — `--stage-only` OTA Binary Packaging & Header Audit**:
   - Package `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz`.
   - Compute `sha256sum` and verify ESP-IDF OTA header fields (`0xE9` magic at `0x00`, `0xABCD5432` `esp_app_desc_t` magic at `0x20`, `"Squeezelite-ESP32"` project name at `0x50`, `"I2S-4MFlash-Rust"` version at `0x30`, size `< 2,686,976` B).
6. Write your summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo146_main_hal_amp_stage_implementer.md` (Rule 17) and append the Fulfillment Summary below (Rule 15).

## 4. Fulfillment Section
- **Execution Date**: 2026-10-25T23:00:00-04:00
- **Final Status**: `[EXECUTED & VERIFIED]`
- **Handoff Document**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo146_main_hal_amp_stage_implementer.md`

### Completed Action Items:
1. `amp.rs`:
   - `percent_to_attenuation` unified with 0.5 dB/step formula `0% => 0xFF`, `1..=100% => 0x30 + (100 - clamped)`.
   - `AmpManager::with_address` initializes `current_volume: cfg.initial_volume` (0x58 = -20 dB).
   - Mock I2C unit tests updated (`PAGE_SELECT 0x00` before `BOOK_SELECT 0x00`, start volume `0x58`).
2. `hal.rs`:
   - Defined `HalError` with `BusWriteFailed`, `StorageBoundsError`, `Link(LinkError)`, `Storage(StorageError)` and `From` conversions.
   - `EspI2sSink::write_frames`: bounded retry loop on partial frame, rounding `written_bytes &= !7; break;` after 64 retries.
   - `CydUartBridge::send_message` and `flush_outbound`: returns `Err(HalError::BusWriteFailed)` on partial writes.
   - `NvsFlashBackend::commit_store`: eliminated 4KB stack temporary `let mut b = *self.flash.read_sector(1)?;`. Read sectors by reference, serialize directly into `self.scratch`.
   - `NvsFlashBackend::erase_range`: validated bounds checking.
3. `main.rs`:
   - Added `#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]`.
   - Defined `encode_i2s0_clkm_conf` and verified via `test_encode_i2s0_clkm_conf`.
   - Updated `set_clock_rate` to invoke `encode_i2s0_clkm_conf` (placing `div_b` in bits `[13:8]` and `div_a` in bits `[19:14]`).
   - `take_static_mp3_decoder` and `take_firmware_bss_buffers` protected with atomic `TAKEN` checks and doc comments.
   - `FirmwareBssBuffers.settings_scratch` expanded to 4,096 B.
   - `APP_CORE_STACK` annotated with `#[link_section = ".dram2_uninit"]`.
   - `app_core_loop` executes `core::arch::asm!("nop")` inside loop.
   - `load_boot_audio_config` imports `SETTINGS_JOURNAL_OFFSET` from `ota_safety`.
   - `persist_audio_config` takes `(scratch: &mut [u8; 4096], cfg: &AudioConfig, current_seq: &mut u32) -> bool`.
   - `dhcpv4::Event::Deconfigured` aborts all active sockets and resets state.
   - `tas_fs_hz` telemetry passes hint `coordinator.current_sample_rate`.
   - Deferred HTTP reboot flushes `settings_scratch` to flash if `settings_dirty`.
   - SlimProto connect rate-limited with 1,000 ms cooldown.
   - `ServerCommand::AudioGain` updates volume percent, persists audio config, and notifies `cyd_bridge`.
   - `ServerCommand::Stream` & `codc` reset MP3 decoder in place, release `MP3_DEC_TAKEN` on PCM transition, and reset `audio_req_sent_offset = 0`.
   - `threshold_sent`, `track_started_sent`, `decode_ready_sent` only set after confirmed socket send.
   - `audio_sock` HTTP GET send tracks partial offset; RX drain loop bounds iterations to 32.
   - `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate` mock expectations updated.

### 5-Gate Verification Results:
- **Gate 1 (Host Workspace Tests)**: `221+ passed, 0 failed` (`exit code 0`).
- **Gate 2 (Workspace Clippy)**: `0 warnings, 0 errors` (`exit code 0`).
- **Gate 3 (Xtensa Target Builds)**: Both `louder-esp32-app` and `cyd-controller-app` compiled cleanly for `xtensa-esp32-none-elf` (`exit code 0`).
- **Gate 4 (ELF Section & Disassembly Audit)**:
  - `.stack`: 38,312 bytes (`0x0095a8` >= 36,000 B Invariant 47 threshold).
  - `run_hardware_firmware` stack frame: 8,496 bytes (< 10,000 B). Over 29.8 KB unused stack headroom.
  - `app_core_loop`: Verified `40080b3d: 0020f0 nop` in inner loop.
  - `encode_i2s0_clkm_conf`: Verified bitfields (`div_b` in [13:8], `div_a` in [19:14]).
- **Gate 5 (`--stage-only` OTA Image Packaging)**:
  - Staged Image: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
  - Size: 759,760 bytes (0.72 MB < 2.56 MB max).
  - SHA-256: `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`.
  - Header: `0xE9` at 0x00, `0xABCD5432` at 0x20, `"I2S-4MFlash-Rust"` at 0x30, `"Squeezelite-ESP32"` at 0x50.
  - Invariants: `/dev/ttyUSB0` was untouched; live endpoint `172.22.1.15` was untouched. Strict `--stage-only`.
