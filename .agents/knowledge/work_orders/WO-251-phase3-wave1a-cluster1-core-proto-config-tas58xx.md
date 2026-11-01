# Work Order: WO-251 — Phase 3 Wave 1A Cluster 1 Remediation (`squeezelite-audio-core`, `squeezelite-proto`, `squeezelite-config`, `squeezelite-tas58xx`, `cyd-controller-app`)

- **Work Order ID**: `WO-251`
- **Timestamp**: `2026-11-01T09:14:00-05:00`
- **Assigned Subagent Role**: `Cluster 1 Core, Proto, Config & TAS5805M Remediation Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/ringbuffer.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/src/ui.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Strict Disjoint Scope
Remediate all **9 findings** (`CONC-201`, `CONC-202`, `LOGIC-203`, `LOGIC-204`, `LOGIC-503`, `LOGIC-504`, `LOGIC-603`, `PERF-601`/`BUS-001`, `PERF-602`) exclusively in the 6 files listed above. **Do NOT modify any files outside those 5 crates** (`louder-esp32-app`, `squeezelite-decoders`, `nanomp3-core`, `squeezelite-raop`, and `squeezelite-cspot` are owned by parallel subagents).

## 2. Findings to Remediate
1. **`CONC-201` (`CRITICAL`, `squeezelite-audio-core/src/ringbuffer.rs:331-410`)**:
   - Remove `pub fn rewind_read(&self, count: usize)` from `Consumer<'ring, 'buf>` and change `rewind_read_internal(&mut self, count: usize)` to take `&mut self` (so `AudioRingBuffer::rewind_read(&mut self, count: usize)` is the sole entry point, statically preventing concurrent SPSC `Producer::write` + `rewind_read` data races). Update the doc-comment on `Consumer` accordingly.
2. **`CONC-202` (`HIGH`, `squeezelite-audio-core/src/ringbuffer.rs:305-328`)**:
   - In `read_internal` and `consume_internal`, execute `self.rewindable.fetch_update(Ordering::AcqRel, Ordering::Relaxed, ...)` *before* `self.read_pos.store(new_r, Ordering::Release)`.
3. **`LOGIC-203` (`CRITICAL`, `squeezelite-audio-core/src/equalizer.rs:526-532`)**:
   - In `Equalizer::process_interleaved`, remove the per-sample amplitude waveshaping block:
     ```rust
     if self.loudness_enabled {
         l *= horner_loudness(l.abs());
         r *= horner_loudness(r.abs());
     }
     ```
     (Loudness compensation is frequency-selective equalization via `loudness_gains_db` biquad filter gains, not instantaneous sample waveshaping. If `horner_loudness` is referenced by tests or `loudness_gains_db`, keep `horner_loudness` as a pure helper function or update tests so all unit tests pass with `0` warnings).
4. **`LOGIC-204` (`MEDIUM`, `squeezelite-audio-core/src/equalizer.rs:825-840`)**:
   - In `Equalizer<10>::retune`, do NOT unconditionally call `self.reset()` at the end of `retune()`, as zeroing biquad delay history (`hist_left`/`hist_right`) during active playback causes audible clicks/pops on volume adjustments. Only call `self.reset()` if `!self.enabled` (when transitioning to disabled) or if any history state contains `!is_finite()`.
5. **`LOGIC-503` (`MEDIUM`, `squeezelite-proto/src/lib.rs:578-600`)**:
   - In `parse_audg`, when `gain_left == 0 && gain_right == 0` (legacy `audg` format where 16.16 fixed-point gains at bytes `10..18` are `0`), if `old_left <= 0` or `old_right <= 0`, ensure legacy mute (`<= 0` dB) maps to `0` (`muted`) while positive `old_left`/`old_right` scale via `(old_left as u32) << 8`. Add a boolean field or method if helpful while preserving backwards compatibility with all existing callers and unit tests.
6. **`LOGIC-504` (`MEDIUM`, `squeezelite-config/src/lib.rs:258-274`)**:
   - In `DualSectorFlashStore::active_sector`, validate sectors using `self.validate_sector(0).ok().flatten()` and `self.validate_sector(1).ok().flatten()` (which verify entry-level CRCs in addition to sector headers) so a sector with a corrupted entry CRC falls back cleanly to the valid peer sector.
7. **`LOGIC-603` (`HIGH`, `squeezelite-tas58xx/src/lib.rs:56-61, 708-723`)**:
   - Add `pub const FAULT_CLEAR_ALL: u8 = 0x81;` (clearing both bit 7 `CLEAR_ANALOG_FAULT` and bit 0 `CLEAR_DIGITAL_FAULT` per TI TAS5805M datasheet §7.6.2.29) and write `FAULT_CLEAR_ALL` (`0x81`) in `recover_from_fault()`.Update any unit tests in `squeezelite-tas58xx/src/lib.rs` expecting `0x80` on `registers::FAULT_CLEAR` to accept `0x81`.
8. **`PERF-601` / `BUS-001` (`MEDIUM`, `squeezelite-tas58xx/src/lib.rs:814-839`)**:
   - Add a `pub fn check_faults_and_telemetry(&mut self) -> Result<(FaultStatus, Tas5805mTelemetry), DriverError<E>>` method on `Tas58xx<I2C, Play>` that calls `self.select_page0_book0()?` once, reads `0x70..=0x73` (4-byte burst), reads `0x37..=0x39` (`FS_MON`, `BCK_MON`, `CLKDET_STATUS`) in a single 3-byte burst `write_read(self.addr, &[registers::FS_MON], &mut clk_regs)?`, and reads `0x69` (`AUTOMUTE_STATE`), reducing the 2s health poll from `11` I2C transactions to `6` (and optimize `read_telemetry()` to also use the 3-byte burst read for `0x37..=0x39` while keeping fallback compatibility with mock I2C tests if any mock expects 1-byte reads—check existing tests in `squeezelite-tas58xx/src/lib.rs` first!).
9. **`PERF-602` (`LOW`, `cyd-controller-app/src/ui.rs:491-498`)**:
   - In `NowPlayingUi::sync_telemetry`, remove `|| new_icon != self.play_pause_icon` from the `if new_title != self.title_label || new_subtitle != self.subtitle_label` condition that sets `DIRTY_TRACK` (`DIRTY_TRANSPORT` is already set at line 502 when `new_icon != self.play_pause_icon`). Update any test in `cyd-controller-app/src/ui.rs` if affected.

## 3. Verification & Rules
- Use `CARGO_TARGET_DIR=/tmp/target-wo251` when running `cargo test` and `cargo clippy` so you do not contend with parallel subagents on `/home/console-admin/Github/squeezelite-esp32/rust/target/.cargo-lock`:
  - `CARGO_TARGET_DIR=/tmp/target-wo251 /home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-config -p squeezelite-tas58xx -p cyd-controller-app`
  - `CARGO_TARGET_DIR=/tmp/target-wo251 /home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-config -p squeezelite-tas58xx -p cyd-controller-app --all-targets -- -D warnings`
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src --strict` (and on `squeezelite-proto/src`, `squeezelite-config/src`, `squeezelite-tas58xx/src`, `cyd-controller-app/src`).
- Write a subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo251_cluster1_implementer.md` and update the Fulfillment Section of this Work Order before returning.
- Ensure `manage_task(Action: "list")` returns zero active background tasks (`Rule 21`) before completing.

## 4. Fulfillment Section

- **Execution Timestamp**: `2026-11-01T09:30:00-05:00`
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Subagent Summary Reference**: [`20261101_wo251_cluster1_implementer.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo251_cluster1_implementer.md)

### 4.1 Actions Taken
1. **`CONC-201`**: Removed `Consumer::rewind_read(&self)` and changed `rewind_read_internal(&mut self)` to take `&mut self` in `squeezelite-audio-core/src/ringbuffer.rs`.
2. **`CONC-202`**: Reordered `fetch_update(Ordering::AcqRel, Ordering::Relaxed)` before `read_pos.store(new_r, Ordering::Release)` in `read_internal` and `consume_internal` in `squeezelite-audio-core/src/ringbuffer.rs`.
3. **`LOGIC-203`**: Removed instantaneous per-sample `horner_loudness(l.abs())` waveshaping multiplication from `Equalizer::process_interleaved` in `squeezelite-audio-core/src/equalizer.rs`.
4. **`LOGIC-204`**: In `Equalizer<10>::retune` in `squeezelite-audio-core/src/equalizer.rs`, conditionally reset biquad history only when `!self.enabled` or when history contains non-finite values. Added `test_retune_preserves_biquad_delay_history_during_playback`.
5. **`LOGIC-503`**: Updated `parse_audg` in `squeezelite-proto/src/lib.rs` to map legacy negative/zero dB to `0` (muted) and positive values to `(old as u32) << 8` when 16.16 raw gains are `0`. Added `test_audg_legacy_positive_old_gain_scaled`.
6. **`LOGIC-504`**: In `DualSectorFlashStore::active_sector` in `squeezelite-config/src/lib.rs`, validated sectors with `FlashJournal::validate_sector(sector_a)` and `FlashJournal::validate_sector(sector_b)`, verifying entry CRCs. Added `test_dual_sector_active_sector_corrupted_entry_fallback`.
7. **`LOGIC-603`**: Added `pub const FAULT_CLEAR_ALL: u8 = 0x81;` and used it in `recover_from_fault()` in `squeezelite-tas58xx/src/lib.rs`. Updated test expectation in `test_recover_from_fault_three_step_sequence`.
8. **`PERF-601` / `BUS-001`**: Implemented `check_faults_and_telemetry()` on `Tas58xx<I2C, Play>` and optimized `read_telemetry()` burst reads in `squeezelite-tas58xx/src/lib.rs`. Added `test_check_faults_and_telemetry_coalesced_reads`.
9. **`PERF-602`**: Removed `|| new_icon != self.play_pause_icon` from `DIRTY_TRACK` condition in `cyd-controller-app/src/ui.rs`. Added `test_ui_play_pause_transition_does_not_dirty_track_region`.

### 4.2 Verification Matrix
- `cargo test`: 199 passed; 0 failed across all 5 crates.
- `cargo clippy`: 0 errors, 0 warnings with `--all-targets -- -D warnings`.
- `google_rust_audit.py --strict`: 0 errors, 0 warnings across all 5 crate `src` directories.
- `manage_task(list)`: 0 active background tasks.
