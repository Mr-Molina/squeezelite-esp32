# Work Order: WO-221 — Wave 3B: Reconcile `louder-esp32-app/src/main.rs` Bare-Metal Multi-Protocol Runtime onto Phase 0+1 Architecture

- **Work Order ID**: `WO-221`
- **Timestamp**: `2026-10-31T16:54:30-04:00`
- **Assigned Subagent Role**: `Rust Bare-Metal Main Runtime Reconciler` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Reconcile `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` between `MK5-Rust` (`cd139b61`, Phase 0+1 `pump_device_phase` + `net_device` + `backend_device` + `tasks` + `config_handle` + `clock_table`) and `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`, 100% IRAM-resident `init_and_map_psram_4mb()`, mDNS `_raop._tcp.local` + `_spotify-connect._tcp.local`, AirPlay 1 RAOP `squeezelite-raop`, Spotify Connect `squeezelite-cspot`, `SourceArbiter`, and `WO-193` `GRS-001`–`GRS-012` audit remediations).

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
   - Do **NOT** modify `pipeline.rs`, `network.rs`, `coordinator.rs`, `lib.rs`, or any other file in `louder-esp32-app/src/` (owned by `WO-220`), and do **NOT** run `cargo` commands (since `WO-220` is concurrently running `cargo test --lib --tests` in `target/`; full `cargo test --workspace` and `xtensa` build will run in Wave 4 `WO-222`).
3. **Preserve Both Phase 0+1 `pump_device_phase` & `WO-197`–`WO-214`**:
   - Keep the 2,014-line legacy inline SlimProto loop deleted (use `coordinator.pump_device_phase(...)` + `SmoltcpSlimTransport` + `SmoltcpStreamSource` + `DeviceAudioBackend` from Phase 0+1).
   - Integrate `init_and_map_psram_4mb()` (`#[esp_hal::ram]`, `WO-197`) and live PSRAM heap telemetry (`esp_alloc::HEAP.used()` / `free()`).
   - Integrate `SourceArbiter` (`AirPlay > Spotify > Bluetooth > SlimProto`), mDNS (`UDP 5353` for `_raop._tcp.local` and `_spotify-connect._tcp.local`), AirPlay 1 RAOP (`RTSP 5000` + `RTP UDP 6000` via `squeezelite-raop`), and Spotify Connect (`ZeroConf` `/?action=getInfo` / `addUser` + `Spirc` / Vorbis feed via `squeezelite-cspot`) into the device loop alongside `pump_device_phase` and `bt_a2dp`.
4. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - Single-module `use` statements (`GRS-001`), zero glob imports outside `#[cfg(test)]` (`GRS-002`), zero production `.unwrap()` (`GRS-010`), dual `// JUSTIFICATION:` + `// SAFETY:` comments on every `unsafe` block/fn (`GRS-008`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
5. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. **Inspect Both Sides of `main.rs`**:
   - Read current `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (`cd139b61`).
   - Inspect `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 show local/wo157-wo214-codecs-airplay-spotify:rust/crates/louder-esp32-app/src/main.rs` (or `git diff b49854f3..local/wo157-wo214-codecs-airplay-spotify -- rust/crates/louder-esp32-app/src/main.rs`).
2. **Integrate `WO-193`/`WO-197`/`WO-211`/`WO-212`/`WO-214` onto Phase 0+1 `main.rs`**:
   - Port 100% IRAM-resident `init_and_map_psram_4mb()` (`#[cfg(target_arch = "xtensa")] #[esp_hal::ram]`) and real `esp_alloc::HEAP.used()` / `esp_alloc::HEAP.free()` reporting (`WO-197`).
   - Port the AirPlay 1 RAOP (`squeezelite-raop`), Spotify Connect (`squeezelite-cspot`), mDNS (`UDP 5353`), and `SourceArbiter` (`AudioSource::{SlimProto, Bluetooth, Spotify, AirPlay}`) state machines and socket handlers from `local/wo157-wo214-codecs-airplay-spotify:rust/crates/louder-esp32-app/src/main.rs` into the Phase 0+1 `main.rs` device loop (`#[cfg(target_arch = "xtensa")]`), keeping all large buffers heap-allocated in PSRAM (`alloc::boxed::Box` / `alloc::vec::Vec`) so `.stack` headroom (`>= 36,000` B) is preserved.
   - Apply all `WO-193` `GRS-001`–`GRS-012` audit remediations across both `#[cfg(target_arch = "xtensa")]` and `#[cfg(not(target_arch = "xtensa"))]`.
3. **Run Static Style Audit on `main.rs`**:
   - Execute `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs --strict` and resolve all `GRS-001`–`GRS-012` violations (`0 error(s), 0 warning(s)`).
4. **Deliverables**:
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-221-wave3b-louder-main-runtime-reconciliation.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo221_wave3b_main_runtime.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Summary of Changes**:
  - Reconciled `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` between `MK5-Rust` (`cd139b61`, Phase 0+1 `PlayerCoordinator` + `SmoltcpSlimTransport` + `SmoltcpStreamSource` + `Esp32AudioBackend` + `ConfigHandle`/`RamJournal` + `DeviceClockRegs`) and `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`):
    1. Ported 100% IRAM-resident `init_and_map_psram_4mb()` + `psram_send_cmd_spi1()` (`#[cfg(target_arch = "xtensa")] #[esp_hal::ram]`, `WO-197`), `squeezelite_decoders::opus::prewarm_opus()` (`WO-209`), and live PSRAM heap telemetry (`esp_alloc::HEAP.used()` / `esp_alloc::HEAP.free()`, falling back to `DEFAULT_PSRAM_FREE_BYTES`/`DEFAULT_PSRAM_USED_BYTES` when PSRAM is unmapped).
    2. Initialized `decode_us_max` and `decode_us_first` in `init_static_pipeline`.
    3. Expanded `SOCKET_SLOTS` to 8 (`dhcp`, `http`, `slim`, `audio`, `mdns` UDP 5353, `rtsp` TCP 5000, `rtp` UDP 6000, `spot` TCP 4070) with PSRAM heap-allocated RTSP/RTP/Spotify buffers so `.stack` headroom is preserved.
    4. Integrated `multi_proto_poll` (`mDNS` responder on `UDP 5353` for `_raop._tcp.local` and `_spotify-connect._tcp.local`, AirPlay 1 RAOP RTSP `TCP 5000` + RTP `UDP 6000` via `squeezelite-raop`, and Spotify Connect ZeroConf HTTP + Spirc/Vorbis on `TCP 4070` via `squeezelite-cspot`) and `SourceArbiter` (`AudioSource::{None, SlimProto, Bluetooth, Spotify, AirPlay}`) into `pump_device_phase` alongside `slim_session_phase`, `bt_poll_phase`, `output_decode_phase`, and `ConfigWriter` polling.
    5. Applied all `WO-193` `GRS-001`–`GRS-012` remediations (`use` single-module imports, zero production `.unwrap()`, dual `// JUSTIFICATION:` + `// SAFETY:` comments on every `unsafe` block/fn, and `<= 80` comment / `<= 100` code line widths) and preserved all `xtensa_hw` exports required by `src/tasks.rs`.
    6. Updated host unit tests (`test_run_firmware_tick_with_network_recovers_amp_fault_and_syncs_rate` with `PAGE_SELECT`/`BOOK_SELECT` I2C expectations, `test_helo_capabilities_flc_and_sample_rate` with `BASEMENT_HELO_CAPABILITIES` assertion, and `test_firmware_tick_arbiter_source_selection`).
- **Verification**:
  - Executed `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs --strict` -> `0 error(s), 0 warning(s)` (exit code `0`).
- **Handoff Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo221_wave3b_main_runtime.md`
