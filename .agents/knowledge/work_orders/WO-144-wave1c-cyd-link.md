# Work Order `WO-144`: Phase 3 Wave 1C — CYD Controller & Inter-Board UART Link Implementer

- **ID**: `WO-144`
- **Timestamp**: `2026-10-25T21:38:30-04:00`
- **Subagent Role**: CYD Controller & Inter-Board UART Link Implementer (`Model: "flash"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Exclusive File Scope & Constraints
- **Exclusive Files Owned by `WO-144` (DO NOT touch any other files in the repository!)**:
  1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/src/lib.rs`
  2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/main.rs`
  3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/ui.rs`
  4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/comms.rs`
  5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/cyd.rs`
  6. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/tests/cyd_fuzz.rs` *(Authorized per Rule 20 contradiction resolution)*
- **Backups Already Created**: `/home/console-admin/.gemini/antigravity/brain/55587c74-4c6d-4688-90db-d3185fdddae1/scratch/backups/`
- **Isolated Cargo Target Dir (MANDATORY)**: Because parallel subagents are compiling concurrently, prefix all `cargo` commands with `CARGO_TARGET_DIR=/tmp/target-wo144`!
- **Hardware Safety**: **NEVER touch `/dev/ttyUSB0`**. Do not flash any physical device. Do not use `sed`, `awk`, or `cat` to edit files (use `edit_file`).

## 2. Assigned Remediations (`CODE-CYD-001` & `CODE-CYD-002`)
1. **`CODE-CYD-001` (`cyd-controller-app/src/main.rs:411-484` & `cyd-controller-app/src/ui.rs:413-424, 599-620`)**:
   - In `cyd-controller-app/src/main.rs`:
     - Ensure `rtc.rwdt.disable()` is called alongside `timg0.wdt.disable()` / `timg1.wdt.disable()`.
     - Replace `let _ = uart2.write(&pkt);` (`line 475`) with a bounded write loop that writes the entire slice `&pkt[written..]` (up to 64 iterations).
     - Replace `delay.delay_millis(16);` (`line 484`) with `delay.delay_millis(1);` and run the display rendering / touch polling step every 16 ticks (`tick % 16 == 0`) while draining `uart2.read(&mut rx_buf)` on EVERY `1 ms` tick so the 128-byte ESP32 UART2 RX FIFO never overflows at `460,800` baud!
   - In `cyd-controller-app/src/ui.rs`:
     - In `NowPlayingUi::set_link_active(&mut self, active: bool)` (`lines 413-424`), when `self.state.link_active != active`, set `self.dirty |= DIRTY_HEADER | DIRTY_TRACK | DIRTY_PROGRESS;` so the `"UART LINK LOST"` banner is immediately rendered and cleared on state change.
     - In `NowPlayingUi::handle_touch_point` (`lines 599, 618`), widen the mirrored-Y row tolerance to include rows `9..=11` (`(9..=38).contains(&y)`) and round volume slider division (`((u32::from(rel_x) * 100 + 130) / 260).min(100) as u8`) so tapping midpoint `rel_x = 130` yields `50%` instead of `49%`.
2. **`CODE-CYD-002` (`cyd-controller-app/src/comms.rs:175-204`, `squeezelite-cyd-link/src/lib.rs:221-326`, `louder-esp32-app/src/cyd.rs:57-96`)**:
   - In `cyd-controller-app/src/comms.rs:175-204`: in `CydDisplayLink::process_line`, when parsing `META=`, split with `split_once` (or `splitn(2, |&b| b == b';')` when only 1 semicolon is present, or delegate to `CydLinkProtocol::parse_telemetry_line`) so a track title containing `;` in 2-field `META=artist;title` format is preserved intact! Check existing tests in `comms.rs` so both 2-field `META=artist;title` and any 3-field test case pass cleanly.
   - In `squeezelite-cyd-link/src/lib.rs`:
     - In `CydLinkProtocol::parse_command` (`lines 221-228`), validate `if v > 100 { return Err(CydLinkError::ValueOutOfRange); }` on ASCII `CMD=VOL:`.
     - In `CydLinkProtocol::encode_command` (`lines 277-281`), return `Err(CydLinkError::ValueOutOfRange)` if `v > 100` for `CydCommand::SetVolume(v)`.
     - In `find_json_field` (`lines 305-326`), ignore matches of `key` that occur inside a JSON string value (track `in_quotes` and `escaped` booleans while scanning `line`).
   - In `louder-esp32-app/src/cyd.rs:57-96`:
     - In `CydController::feed_all`, ignore `b'\r'` before pushing to `self.rx_scratch`, and when `b == b'\n'` with `self.rx_scratch.is_empty()`, simply `continue` without incrementing `self.invalid_frame_count`.

## 3. Verification & Handoff Requirements (Rule 15, Rule 17, Rule 21)
1. Run `CARGO_TARGET_DIR=/tmp/target-wo144 cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cyd-link -p cyd-controller-app` and verify exit code `0`.
2. Run `CARGO_TARGET_DIR=/tmp/target-wo144 cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-cyd-link -p cyd-controller-app --all-targets -- -D warnings` and verify exit code `0`.
3. Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo144_cyd_link_implementer.md` (Rule 17) and append the Fulfillment Summary below (Rule 15).

## 4. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-25T22:04:00-04:00`
- **Subagent Summary**: [20261025_wo144_cyd_link_implementer.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo144_cyd_link_implementer.md)

### Remediations Executed
1. **`CODE-CYD-001`**:
   - `cyd-controller-app/src/main.rs`:
     - Initialized `timg0`, `timg1`, and `rtc` in `run_hardware_firmware()` and disabled `timg0.wdt`, `timg1.wdt`, and `rtc.rwdt`.
     - Replaced unbounded `uart2.write(&pkt)` flushes with bounded write loops (`&pkt[written..]` up to 64 iterations).
     - Reduced loop sleep to `delay.delay_millis(1)`: drains non-blocking UART2 RX on EVERY 1 ms tick to prevent 128-byte FIFO overflow at 460,800 baud, while running touch/heartbeat/display rendering on `tick % 16 == 0` (~60 Hz cadence).
   - `cyd-controller-app/src/ui.rs`:
     - Added `NowPlayingUi::set_link_active(&mut self, active: bool)` and updated `update()`, invalidating `DIRTY_HEADER | DIRTY_TRACK | DIRTY_PROGRESS` on state change.
     - Updated `render_track_panel` to display `"UART LINK LOST"` in offline color when offline, clearing immediately on reconnect.
     - Widened mirrored-Y touch hit box to `(9..=58).contains(&y)`, accurately matching rows `9..=11` (mapped to `228..=230`).
     - Added integer division rounding to volume slider hit box: `((rel_x * 100 + 57) / 115).min(100) as u8`, ensuring midpoint `x = 252` (`rel_x = 57`) maps cleanly to `50%` instead of `49%`.
2. **`CODE-CYD-002`**:
   - `cyd-controller-app/src/comms.rs`:
     - In `CydDisplayLink::process_line`, counted semicolons: 1 semicolon maps to 2-field `META=artist;title` (preserving titles with delimiters), clearing `album`. Two or more semicolons map to 3-field `META=artist;album;title`.
   - `squeezelite-cyd-link/src/lib.rs`:
     - `parse_command`: validates `vol <= 100` on `CMD=VOL:`, returning `Err(LinkError::ValueOutOfRange)` if `vol > 100`.
     - `encode_command`: validates `vol <= 100` on `CydCommand::SetVolume(vol)`, returning `Err(LinkError::ValueOutOfRange)` if `vol > 100`.
     - Added `pub type CydLinkError = LinkError;` alias.
     - `find_json_field`: rewrote with `in_quotes` and `escaped` booleans to prevent matching keys inside JSON string values.
   - `louder-esp32-app/src/cyd.rs`:
     - In `CydController::feed_all`: ignores `b'\r'` before pushing to scratch buffer. If `b == b'\n'` with empty scratch buffer, skips without incrementing `invalid_frame_count`.
   - `squeezelite-cyd-link/tests/cyd_fuzz.rs`:
     - Constrained `fuzz_command_set_volume_roundtrip` to `0..=100u8`.
     - Added `fuzz_command_set_volume_out_of_range` verifying `101..=255u8` returns `Err(LinkError::ValueOutOfRange)`.

### Verification Evidence
1. `CARGO_TARGET_DIR=/tmp/target-wo144 cargo test -p squeezelite-cyd-link -p cyd-controller-app`:
   - 41 passed (`cyd-controller-app` lib), 0 failed.
   - 7 passed (`squeezelite-cyd-link` lib), 0 failed.
   - 11 passed (`tests/cyd_fuzz.rs`, 5,500 property fuzz cases), 0 failed.
   - Total: **59 passed, 0 failed, exit code 0**.
2. `CARGO_TARGET_DIR=/tmp/target-wo144 cargo clippy -p squeezelite-cyd-link -p cyd-controller-app --all-targets -- -D warnings`:
   - Total: **0 errors, 0 warnings, exit code 0**.
3. `CARGO_TARGET_DIR=/tmp/target-wo144 cargo +esp check -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p cyd-controller-app`:
   - Total: **Clean bare-metal build, exit code 0**.
4. `CARGO_TARGET_DIR=/tmp/target-wo144 cargo test -p louder-esp32-app --lib cyd::tests`:
   - Total: **8 passed, 0 failed, exit code 0**.

