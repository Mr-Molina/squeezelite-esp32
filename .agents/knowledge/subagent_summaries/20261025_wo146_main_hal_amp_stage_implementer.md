# Subagent Summary: `WO-146` — Phase 3 Wave 2 `louder-esp32-app` Hardware Integration & `--stage-only` OTA Image Packager

- **Timestamp**: 2026-10-25T23:00:00-04:00
- **Work Order ID**: `WO-146`
- **Subagent Role**: `louder-esp32-app` Hardware Integration & `--stage-only` Packager
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified and Created

### Target Code Files Modified:
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs`
   - `percent_to_attenuation`: unified to 0.5 dB/step formula `0% => 0xFF`, `1..=100% => 0x30 + (100 - clamped)` matching `lms_gain_to_tas5805m_vol_reg`.
   - `AmpManager::with_address`: initialized `current_volume: cfg.initial_volume` (starts at `0x58` = `VOL_DEFAULT_M20DB`).
   - Mock I2C unit tests updated in `amp::tests` (`PAGE_SELECT 0x00 = 0x00` before `BOOK_SELECT 0x7F = 0x00`, start volume `0x58`).
   - Unused import `VOL_DEFAULT_MINUS_24DB` removed.

2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs`
   - Defined `HalError` with `BusWriteFailed`, `StorageBoundsError`, `Link(LinkError)`, `Storage(StorageError)` and `From` impls.
   - `EspI2sSink::write_frames`: bounded retry loop on partial frame, rounding `written_bytes &= !7; break;` after 64 retries.
   - `CydUartBridge::send_message` and `flush_outbound`: updated to return `Result<usize, HalError>`, returning `Err(HalError::BusWriteFailed)` if `offset < len`.
   - `NvsFlashBackend::commit_store`: eliminated 4KB stack temporary `let mut b = *self.flash.read_sector(1)?;`. Slices `s0` and `s1` passed by reference to `DualSectorFlashStore::active_sector(s0, s1)`, reading metadata directly from slice headers and serializing straight into `self.scratch`.
   - `NvsFlashBackend::erase_range`: returns `Err(HalError::StorageBoundsError)` if `range.is_empty() || range.start >= range.end || range.end > FLASH_SECTOR_SIZE * 2`.
   - Annotated `test_nvs_flash_backend_erase_all` with `#[allow(clippy::reversed_empty_ranges)]`.

3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
   - Added `#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]` for `core::arch::asm!("nop")`.
   - Defined `encode_i2s0_clkm_conf(orig, div_num, div_b, div_a)` at crate level and added `test_encode_i2s0_clkm_conf`.
   - Updated `set_clock_rate` to invoke `encode_i2s0_clkm_conf` (placing `div_b` in bits `[13:8]` and `div_a` in bits `[19:14]`).
   - Guarded `take_static_mp3_decoder` and `take_firmware_bss_buffers` with atomic `TAKEN` flags and `// SAFETY:` doc comments.
   - `FirmwareBssBuffers`: moved `settings_scratch: [u8; 4096]` into `#[link_section = ".dram2_uninit"]`, expanding `.stack` headroom from 34,216 B to 38,312 B (`0x0095a8`).
   - `APP_CORE_STACK`: annotated with `#[link_section = ".dram2_uninit"]`.
   - `app_core_loop`: replaced spin loop with `unsafe { core::arch::asm!("nop"); }` inside `for _ in 0..10_000`.
   - `load_boot_audio_config`: imports `SETTINGS_JOURNAL_OFFSET` from `ota_safety`.
   - `persist_audio_config`: takes `(scratch: &mut [u8; 4096], cfg: &AudioConfig, current_seq: &mut u32) -> bool`.
   - Tracked `settings_dirty` in `run_hardware_firmware`.
   - Handled `dhcpv4::Event::Deconfigured`: aborts all sockets (`slim`, `audio`, `http`), resets `helo_sent` and `audio_stream_active`.
   - Updated `tas_fs_hz` telemetry to pass hint `coordinator.current_sample_rate`.
   - Deferred HTTP reboot: parks `APP_CPU` and flushes `settings_scratch` to flash if `settings_dirty`.
   - SlimProto connect rate-limited with 1,000 ms cooldown (`last_slim_connect_ms`).
   - `ServerCommand::AudioGain`: derives volume percent matching `percent_to_attenuation`, persists audio config, and updates `cyd_bridge`.
   - `ServerCommand::Stream` & `codc`: reset MP3 decoder in place if already MP3, release `MP3_DEC_TAKEN` when transitioning to PCM, reset `audio_req_sent_offset = 0`.
   - `threshold_sent`, `track_started_sent`, `decode_ready_sent`: only marked `true` if `send_slice(...) == Ok(stat_len)`.
   - `audio_sock` HTTP GET send: partial offset tracking.
   - `audio_sock` RX drain: bounded `for _ in 0..32` while `stream_buffer.available_write() >= 512`.
   - `test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate`: mock I2C expectations updated (`PAGE_SELECT` before `BOOK_SELECT`, volume `0x58`).

### Output Staged OTA Image Created:
4. `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
   - Generated via `espflash save-image --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz`.
   - Size: 759,760 bytes (0.72 MB).
   - SHA-256: `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`.
   - Magic: `0xE9` at 0x00, `0xABCD5432` at 0x20.
   - Version: `"I2S-4MFlash-Rust"`.
   - Project: `"Squeezelite-ESP32"`.

---

## 2. 5-Gate Verification Evidence

1. **Gate 1 — Full Workspace Unit, Stress & Fuzz Suite**:
   - `cargo test --workspace --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
   - Output: `103 passed` in `louder-esp32-app` lib, `7 passed` in `louder-esp32-app` bin tests, `41 passed` in `cyd-controller-app`, `24 passed` in `squeezelite-decoders`, `20 passed` + `10 fuzz passed` in `squeezelite-proto`, `16 passed` in `squeezelite-tas58xx`. Total 221+ passed, 0 failed. Exit code `0`.

2. **Gate 2 — Full Workspace Clippy Audit**:
   - `cargo clippy --workspace --all-targets --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -- -D warnings`
   - Output: 0 warnings, 0 errors. Exit code `0`.

3. **Gate 3 — Bare-Metal Xtensa Release Builds**:
   - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app` -> Exit code `0`.
   - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p cyd-controller-app` -> Exit code `0`.

4. **Gate 4 — ELF `.stack` Budget & Disassembly Proof**:
   - Section `.stack`: `0x0095a8` (38,312 bytes >= 36,000 B Invariant 47 threshold).
   - `run_hardware_firmware` stack frame: `8,496` bytes (`imm=1062` * 8), leaving 29,816 bytes of unused stack headroom. Strictly `< 10,000` bytes.
   - `app_core_loop`: Verified `nop` instruction in inner loop (`40080b3d: 0020f0 nop`).
   - `encode_i2s0_clkm_conf`: Verified unit test and bit-level encoding (`div_b` in `[13:8]`, `div_a` in `[19:14]`).

5. **Gate 5 — `--stage-only` OTA Binary Packaging & Header Audit**:
   - Binary: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/louder-esp32-app-basement-ota.bin`
   - Size: `759,760` B (`< 2,686,976` B, 18.40% partition capacity).
   - SHA-256: `b4efecc78c52fb47fb62f22175b19e44adbac78fe3e0dda77f888d82e75862ff`.
   - Verified magic bytes `0xE9`, `0xABCD5432`, `"I2S-4MFlash-Rust"`, `"Squeezelite-ESP32"`.
   - Physical `/dev/ttyUSB0` was untouched. Target `172.22.1.15` was untouched (per `staging-before-restart.md`).

---

## 3. Risks & Invariants
- Invariant 65 / Rule 44: Local serial `/dev/ttyUSB0` was never accessed or probed.
- `staging-before-restart.md`: Firmware was packaged strictly `--stage-only` into artifact scratch storage without rebooting or flashing live hardware.
