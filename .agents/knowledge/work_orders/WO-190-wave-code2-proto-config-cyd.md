# Work Order `WO-190`: `/deep-code-audit` Phase 3 (`Wave CODE-2`) — SlimProto, Config, CYD Link & CYD Controller Remediation

- **ID**: `WO-190`
- **Timestamp**: `2026-10-27T18:28:00-04:00`
- **Subagent Role**: `Rust SlimProto, Config & CYD Implementer` (`Model: "pro"`, `TypeName: "self"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Scope & Instructions
You are **Wave CODE-2 Implementer** (`/deep-code-audit` Phase 3 + `rust-pro`).
Read `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/audit_master_matrix.md` and `/home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/SKILL.md`.

### Strictly Assigned Files (DO NOT TOUCH ANY OTHER FILES)
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link/**`
- `/home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app/**`

### Required Functional & `rust-pro` Remediations
1. **`squeezelite-proto/src/lib.rs` & `tests/proto_fuzz.rs`**:
   - **`CODE-PROTO-STAT-001` (`HIGH`)**: In `SlimprotoBuilder::build_stat_full` (lines 926-949), compute `elapsed_seconds` and `elapsed_ms` from `frames_played` (at `44_100` Hz, matching `StatBuilder::build_stat` at line 760: `let elapsed_seconds = frames_played / 44_100; let elapsed_ms = ((frames_played as u64 * 1000) / 44_100) as u32;`) instead of using `timestamp_ms` (jiffies). Update any unit/property tests in `squeezelite-proto/src/lib.rs` and `squeezelite-proto/tests/proto_fuzz.rs` (`fuzz_stat_builder_soundness`) that asserted `elapsed_seconds == ts / 1000` and `elapsed_ms == ts` to assert the track-relative calculation from `frames_played`.
   - **`CODE-PROTO-STRM-001` (`MEDIUM`)**: In `SlimprotoParser::parse_strm` (line 518), update `let autostart = payload[1] == b'1' || payload[1] == b'3' || payload[1] == 1 || payload[1] == 3;`.
   - **`CODE-PROTO-BUTN-001` (`MEDIUM`)**: Add `SlimprotoBuilder::build_butn(time_ms: u32, button_code: u16, pressed: bool, out: &mut [u8]) -> Result<usize, ParseError>` formatting a 16-byte `BUTN` frame (`b"BUTN"`, `8u32.to_be_bytes()`, `time_ms.to_be_bytes()`, `((button_code as u32) + if pressed { 0x0001_0000 } else { 0 }).to_be_bytes()`) + unit test.
   - **`CODE-PROTO-DISC-001` (`MEDIUM`)**: In `DiscoveryParser::parse_response` (lines 1144-1157), only treat `payload[tag_end]` as a 1-byte TLV length when `tlv_len <= 64 && tag_end + 1 + tlv_len <= payload.len()`; otherwise fall back to scanning for a null terminator `0`.
   - **`GRS-002`**: Replace glob imports in `squeezelite-proto/tests/proto_fuzz.rs:6-7` with explicit imports.
2. **`squeezelite-config/src/lib.rs` & `tests/config_fuzz.rs`**:
   - **`AUD13-DOC-002`**: Fix doc comment formula at `squeezelite-config/src/lib.rs:254` to `(seq_a.wrapping_sub(seq_b) as i32) >= 0`.
   - **`GRS-002`**: Replace glob imports in `squeezelite-config/tests/config_fuzz.rs:6-7` with explicit imports.
3. **`squeezelite-cyd-link/src/lib.rs` & `tests/cyd_fuzz.rs`**:
   - **`GRS-002`**: Replace glob imports in `squeezelite-cyd-link/tests/cyd_fuzz.rs:7-8` with explicit imports.
4. **`cyd-controller-app/src/{main,drivers,comms,ui,lib}.rs`**:
   - **`CODE-CYD-WDT-001` (`HIGH`, `main.rs`)**: Re-enable `timg0.wdt` (`Stage0` timeout 10 seconds) after TFT/HIL init in `xtensa_hw::run_hardware_firmware` and call `wdt.feed()` in the 1 ms main loop.
   - **`CODE-CYD-CAL-001` (`MEDIUM`, `drivers.rs:515-535`)**: In `Xpt2046Touch::calibrate_raw`, calibrate `raw_x` against `cal_x_min`/`x_range` and `raw_y` against `cal_y_min`/`y_range` appropriately when `self.swap_xy` is true (`cx` uses `raw_y` with `cal_y_min`/`y_range`, `cy` uses `raw_x` with `cal_x_min`/`x_range`, while preserving defaults when `cal_x == cal_y`).
   - **`CODE-CYD-IRQ-001` (`MEDIUM`, `main.rs:145-149`)**: Remove the redundant `if !self.is_irq_low() { return 0; }` inside `Esp32TouchSpiBus::read_raw_12bit` (since `Xpt2046Touch::read` already checks `is_irq_low()`).
   - **`GRS-001` (`main.rs:30-39`)**: Flatten nested `use esp_hal::{ ... }` imports so zero `::` appear inside `{ ... }` braces.
5. **`GRS-012` Line & Comment Widths**:
   - Wrap all code lines `<= 100` cols and standalone comments `<= 80` cols across all 4 assigned crates (`squeezelite-proto`, `squeezelite-config`, `squeezelite-cyd-link`, `cyd-controller-app`), including `tests/*.rs`.
6. **Verification Gate**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link /home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app --strict` and verify **0 errors and 0 warnings** (`exit code 0`).
   - Run `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-proto -p squeezelite-config -p squeezelite-cyd-link -p cyd-controller-app` and verify 100% pass.
   - Write a summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code2.md`, update the Fulfillment Section of this Work Order, and send your completion report.

## 2. Fulfillment Section
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`
- **Completion Timestamp**: `2026-10-27T19:10:00-04:00`
- **Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261027_wave_code2.md`

### Completed Remediations
1. **`squeezelite-proto`**:
   - `CODE-PROTO-STAT-001` (`HIGH`): In `SlimprotoBuilder::build_stat_full`, updated `elapsed_seconds` and `elapsed_ms` calculation from `frames_played / 44_100`. Updated unit tests and property fuzz test `fuzz_stat_builder_soundness` in `tests/proto_fuzz.rs`.
   - `CODE-PROTO-STRM-001` (`MEDIUM`): In `SlimprotoParser::parse_strm`, supported numeric autostart (`payload[1] == 1 || payload[1] == 3 || payload[1] == b'1' || payload[1] == b'3'`).
   - `CODE-PROTO-BUTN-001` (`MEDIUM`): Added `SlimprotoBuilder::build_butn` formatting 16-byte `BUTN` frames + added unit test `test_build_butn_encodes_slimproto_frame`.
   - `CODE-PROTO-DISC-001` (`MEDIUM`): In `DiscoveryParser::parse_response`, added safe check `tlv_len <= 64 && tag_end + 1 + tlv_len <= payload.len()`, falling back to null-terminated tag scanning.
   - `GRS-002`: Replaced glob imports in `tests/proto_fuzz.rs` with explicit imports.
   - `GRS-012`: Wrapped lines <= 100 cols and comments <= 80 cols.
2. **`squeezelite-config`**:
   - `AUD13-DOC-002`: Fixed doc comment formula at `src/lib.rs:254` to `(seq_a.wrapping_sub(seq_b) as i32) >= 0`.
   - `GRS-002`: Replaced glob imports in `tests/config_fuzz.rs` with explicit imports (including `KeyValueStorage`).
   - `GRS-012`: Wrapped lines <= 100 cols and comments <= 80 cols.
3. **`squeezelite-cyd-link`**:
   - `GRS-002`: Replaced glob imports in `tests/cyd_fuzz.rs` with explicit imports.
   - `GRS-012`: Wrapped lines <= 100 cols and comments <= 80 cols.
4. **`cyd-controller-app`**:
   - `CODE-CYD-WDT-001` (`HIGH`): Re-enabled `wdt0` post-HIL with 10s Stage0 timeout (`wdt0.set_timeout(MwdtStage::Stage0, Duration::from_millis(10_000)); wdt0.enable(); wdt0.feed();`) and added `wdt0.feed()` in the 1 ms main loop.
   - `CODE-CYD-CAL-001` (`MEDIUM`): Fixed `Xpt2046Touch::calibrate_raw` when `swap_xy` is true to map `raw_y` to screen X with `cal_y_min`/`y_range` and `raw_x` to screen Y with `cal_x_min`/`x_range`. Added unit test `test_xpt2046_asymmetric_calibration_with_swap_xy`.
   - `CODE-CYD-IRQ-001` (`MEDIUM`): Removed redundant `if !self.is_irq_low() { return 0; }` check in `Esp32TouchSpiBus::read_raw_12bit`.
   - `GRS-001`: Flattened `use esp_hal::{...}` in `src/main.rs`.
   - `GRS-012`: Wrapped lines <= 100 cols and comments <= 80 cols across `src/{main,drivers,comms,ui}.rs`.

### Verification Evidence
1. **Google Rust Style Static Audit (`--strict`)**:
   - `/usr/bin/python3 google_rust_audit.py squeezelite-proto --strict`: **0 errors, 0 warnings**
   - `/usr/bin/python3 google_rust_audit.py squeezelite-config --strict`: **0 errors, 0 warnings**
   - `/usr/bin/python3 google_rust_audit.py squeezelite-cyd-link --strict`: **0 errors, 0 warnings**
   - `/usr/bin/python3 google_rust_audit.py cyd-controller-app --strict`: **0 errors, 0 warnings**
2. **Cargo Test Suite (`CARGO_TARGET_DIR=/tmp/cargo-wave-code2`)**:
   - `squeezelite-proto`: 21 unittests + 10 fuzz tests passed (31 passed, 0 failed)
   - `squeezelite-config`: 15 unittests + 9 fuzz tests passed (24 passed, 0 failed)
   - `squeezelite-cyd-link`: 7 unittests + 11 fuzz tests passed (18 passed, 0 failed)
   - `cyd-controller-app`: 42 unittests passed (42 passed, 0 failed)
   - **Total Tests**: 115 passed; 0 failed; 0 ignored.
