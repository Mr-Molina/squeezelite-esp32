# Subagent Summary: `20261025_wo144_cyd_link_implementer`

- **Work Order**: `WO-144` (Phase 3 Wave 1C)
- **Subagent Role**: CYD Controller & Inter-Board UART Link Implementer
- **Timestamp**: `2026-10-25T22:03:00-04:00`
- **Execution Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified and Created
All file modifications strictly followed safe APIs without `sed`/`awk`/`cat`, and backups were verified prior to edits:

1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/main.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/main.rs`
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/ui.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/ui.rs`
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/comms.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/comms.rs`
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/cyd.rs`
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/src/lib.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/lib.rs`
6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/tests/cyd_fuzz.rs`
   - Backup: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/tests/cyd_fuzz.rs` (Authorized by parent orchestrator per Rule 20)

---

## 2. Rationale for Each Change

### A. `CODE-CYD-001` (`cyd-controller-app/src/{main.rs, ui.rs}`)
1. **Watchdog Disable (`main.rs`)**:
   - Initialized `timg0`, `timg1`, and `rtc` in `xtensa_hw::run_hardware_firmware()` and disabled `timg0.wdt`, `timg1.wdt`, and `rtc.rwdt`. Prevents premature reset during blocking display initialization or heavy touch rendering loops.
2. **UART2 Bounded Write Loops (`main.rs`)**:
   - Replaced unbounded/single-shot `uart2.write(&pkt)` flushes (in initial sync and main loop) with a bounded write loop writing `&pkt[written..]` up to 64 iterations, avoiding truncated frame transmissions.
3. **1 ms RX Cadence & 16 ms Display Cadence (`main.rs`)**:
   - Changed delay loop from `delay.delay_millis(16)` to `delay.delay_millis(1)`. UART2 RX FIFO is drained non-blocking on EVERY 1 ms tick, preventing overflow of the 128-byte hardware FIFO at 460,800 baud. Touch polling, heartbeat, and display dirty region rendering are dispatched at `tick % 16 == 0` (~60 Hz).
4. **Link Active State & Header/Track Invalidation (`ui.rs`)**:
   - Added `NowPlayingUi::set_link_active(&mut self, active: bool)` and updated `update(&mut self, state: &CydTelemetryState)`. Setting `link_active` change marks `DIRTY_HEADER | DIRTY_TRACK | DIRTY_PROGRESS` dirty flags. `render_track_panel` renders `"UART LINK LOST"` in `UI_COLOR_OFFLINE_565` when offline and restores track title upon reconnect.
5. **Mirrored-Y Row Tolerance & Volume Division Rounding (`ui.rs`)**:
   - Widened mirrored-Y hit box range in `handle_touch_point` to `(9..=58).contains(&y)`, accurately capturing rows `9..=11` (mapped to `228..=230` in standard coordinates).
   - Applied integer division rounding to volume slider hit box: `((rel_x * 100 + 57) / 115).min(100) as u8`. Midpoint `x = 252` (`rel_x = 57`) now maps accurately to `50%` rather than truncating to `49%`.

### B. `CODE-CYD-002` (`cyd-controller-app/src/comms.rs`, `squeezelite-cyd-link/src/lib.rs`, `louder-esp32-app/src/cyd.rs`, `squeezelite-cyd-link/tests/cyd_fuzz.rs`)
1. **Metadata 2-Field vs 3-Field Parsing (`comms.rs`)**:
   - In `CydDisplayLink::process_line`, counted semicolons in `META=`: if `semi_count == 1`, parses as 2-field format `artist;title` (preserving titles that may contain delimiters from `CydLinkProtocol::parse_line`) and clears `album`. If `semi_count >= 2`, parses 3-field format `artist;album;title`.
2. **Volume Range Bounds Check & Quote Escaping (`squeezelite-cyd-link/src/lib.rs`)**:
   - In `parse_command`: on `CMD=VOL:`, validates `vol <= 100`, returning `Err(LinkError::ValueOutOfRange)` if `vol > 100`.
   - In `encode_command`: on `CydCommand::SetVolume(vol)`, validates `vol <= 100`, returning `Err(LinkError::ValueOutOfRange)` if `vol > 100`.
   - Added `pub type CydLinkError = LinkError;` type alias.
   - In `find_json_field`: rewrote scanner with `in_quotes` and `escaped` booleans to prevent keys appearing inside JSON string values from matching as object properties.
3. **CRLF & Empty Line Tolerance (`louder-esp32-app/src/cyd.rs`)**:
   - In `CydController::feed_all`: ignores `b'\r'` prior to pushing to scratch buffer. When `b == b'\n'` and scratch buffer is empty, skips parsing without incrementing `invalid_frame_count`.
4. **Fuzz Invariant Alignment (`squeezelite-cyd-link/tests/cyd_fuzz.rs`)**:
   - Updated `fuzz_command_set_volume_roundtrip` to fuzz valid range `0..=100u8`.
   - Added `fuzz_command_set_volume_out_of_range` verifying that `101..=255u8` returns `Err(LinkError::ValueOutOfRange)`.

---

## 3. Verification Evidence
1. **Unit & Integration Tests (`squeezelite-cyd-link` & `cyd-controller-app`)**:
   - `CARGO_TARGET_DIR=/tmp/target-wo144 cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cyd-link -p cyd-controller-app`:
     - `cyd-controller-app`: 41 passed, 0 failed.
     - `squeezelite-cyd-link`: 7 passed, 0 failed.
     - `squeezelite-cyd-link/tests/cyd_fuzz.rs`: 11 passed (5,500 random proptest cases), 0 failed.
     - Total: **59 tests passed, 0 failed, exit code 0**.
2. **Static Analysis & Clippy Quality Gate**:
   - `CARGO_TARGET_DIR=/tmp/target-wo144 cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cyd-link -p cyd-controller-app --all-targets -- -D warnings`:
     - Result: **0 errors, 0 warnings, exit code 0**.
3. **Xtensa Bare-Metal Cross-Compilation**:
   - `CARGO_TARGET_DIR=/tmp/target-wo144 cargo +esp check -Zbuild-std=core,alloc --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --target xtensa-esp32-none-elf -p cyd-controller-app`:
     - Result: **Clean build, exit code 0**.
4. **Louder App CYD Subsystem Tests**:
   - `CARGO_TARGET_DIR=/tmp/target-wo144 cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --lib cyd::tests`:
     - Result: **8 passed, 0 failed, exit code 0**.

---

## 4. Known Risks and Untested Assumptions
- Hardware-in-the-Loop verification on physical ESP32 CYD glass requires live UART cabling; all changes were verified via bare-metal cross-compilation (`xtensa-esp32-none-elf`) and unit/integration/proptest fuzz harnesses. No physical flashing was performed (compliant with `/dev/ttyUSB0` safety lock).
