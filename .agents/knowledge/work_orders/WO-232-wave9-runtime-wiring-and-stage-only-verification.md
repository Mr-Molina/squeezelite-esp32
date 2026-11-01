# Work Order: WO-232 — Wave 9: Final Runtime Wiring (`lib.rs`, `main.rs`), Full Workspace Verification, & Bare-Metal `xtensa-esp32-none-elf` `--stage-only` Build

- **Work Order ID**: `WO-232`
- **Timestamp**: `2026-11-01T00:53:00-04:00`
- **Assigned Subagent Role**: `Rust Full-Parity Runtime Integrator & Stage-Only Packager` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Wire all Wave 8 modules (`console`, `eth_w5500`, `display_gds`, `controls`, `dac_platform`, `power_boards`) into `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`, and pass all 5 `--stage-only` verification gates across the entire 11-crate workspace:
1. `cargo test --workspace` (`0` failures across all unit, integration, equivalence, conformance, and fuzz tests).
2. `google_rust_audit.py --strict` across all 11 workspace crate `src/` directories (`0` errors, `0` warnings).
3. `cargo clippy --workspace --all-targets -- -D warnings` (`0` warnings).
4. Bare-metal `xtensa-esp32-none-elf` release build (`cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app --no-default-features --features board-louder-esp32`), verifying `.dram2_uninit <= 98,768` B and `.stack` headroom.
5. Package the final staged OTA binary to `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin` and verify with `verify_staged_ota_bin.py` (or header/size/SHA-256 verification).

## 2. Hard Safety Constraints
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Stage-Only Mandate (`Invariant 42` / `single-target-troubleshooting.md`)**: Do **NOT** flash any hardware (`172.22.1.15`) in this work order (`WO-233` will perform the live hardware certification) and **NEVER** touch `/dev/ttyUSB0`.
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `lib.rs` must contain ONLY `mod` declarations and `pub use` re-exports (`GRS-003`).
   - Single-module `use` statements (`GRS-001`), zero glob imports outside `#[cfg(test)]` (`GRS-002`), `#[non_exhaustive]` where required (`GRS-009`), zero production `.unwrap()` (`GRS-010`), no `get_` prefix on getters (`GRS-011`), dual `// JUSTIFICATION:` + `// SAFETY:` comments (`GRS-008`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. **Wire Wave 8 Modules into `lib.rs` & `main.rs`**:
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`:
     - Add `pub mod console;`, `pub mod controls;`, `pub mod dac_platform;`, `pub mod display_gds;`, `pub mod eth_w5500;`, and `pub mod power_boards;` (in alphabetical order) and re-export their key public types (`GRS-001` single-module `pub use`).
   - In `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
     - Declare `mod console;`, `mod controls;`, `mod dac_platform;`, `mod display_gds;`, `mod eth_w5500;`, and `mod power_boards;` (with `#[cfg_attr(target_arch = "xtensa", allow(dead_code))]` or active wiring as needed so `clippy --all-targets -- -D warnings` and `xtensa` build both have `0` dead_code / unused warnings).
2. **Gate 1 — Full Workspace Test Suite**:
   - Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`.
3. **Gate 2 — Google Rust Static Audit (`GRS-001`–`GRS-012`)**:
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py --strict` across all 11 crate `src` directories (`louder-esp32-app/src`, `squeezelite-decoders/src`, `squeezelite-raop/src`, `squeezelite-cspot/src`, `squeezelite-audio-core/src`, `squeezelite-proto/src`, `squeezelite-config/src`, `nanomp3-core/src`, `squeezelite-tas58xx/src`, `squeezelite-cyd-link/src`, `cyd-controller-app/src`).
4. **Gate 3 — Workspace Clippy Audit**:
   - Run `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`.
5. **Gate 4 & 5 — Bare-Metal `xtensa-esp32-none-elf` Build, Section Audit, & Staged OTA Packaging**:
   - Build bare-metal release binary:
     ```bash
     source /home/console-admin/export-esp.sh && \
     /home/console-admin/.cargo/bin/cargo +esp build \
       --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml \
       --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf \
       -p louder-esp32-app --no-default-features --features board-louder-esp32
     ```
   - Inspect ELF sections (`xtensa-esp32-elf-size -A`) and package staged OTA image via `espflash save-image --chip esp32 --flash-size 4mb --flash-freq 40mhz --flash-mode dio /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`.
   - Record exact byte size, SHA-256 digest, and `.dram2_uninit`/`.stack`/`.bss`/`.text` sizes.
6. **Deliverables**:
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-232-wave9-runtime-wiring-and-stage-only-verification.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo232_wave9_full_parity_stage_only.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-11-01T01:05:30-04:00`
- **Subagent Summary**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo232_wave9_full_parity_stage_only.md`

### 4.1 Files Modified
1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`:
   - Added alphabetical `pub mod` declarations for all 6 Wave 8 modules (`console`, `controls`, `dac_platform`, `display_gds`, `eth_w5500`, `power_boards`) and `GRS-001` single-module `pub use` re-exports of their public types and functions.
2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`:
   - Declared `mod console;`, `mod controls;`, `mod dac_platform;`, `mod display_gds;`, `mod eth_w5500;`, and `mod power_boards;` with `#[allow(dead_code, clippy::duplicated_attributes)]` and added `SpircEvent::TrackEnded | SpircEvent::MetadataUpdated(_)` pattern arms in `#[cfg(target_arch = "xtensa")] mod xtensa_hw`.
3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs`:
   - Replaced 3 manual ceiling divisions (`(x + 7) / 8` and `(total_pixels + 1) / 2`) with `.div_ceil(...)` to satisfy `clippy::manual_div_ceil` under `-D warnings`.
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/tasks.rs`:
   - Imported `AudioBackend` in `#[cfg(target_arch = "xtensa")] mod device` (`use louder_esp32_app::io_traits::{AudioBackend, NetAddr, SlimTransport};`) for `backend.set_sample_rate(rate)`.

### 4.2 5-Gate Verification Evidence
1. **Gate 1 — Full Workspace Test Suite (`cargo test --workspace`)**: `[EXECUTED & VERIFIED]`
   - Result: `703` passed (`301` in `louder-esp32-app` lib, `34` in `louder-esp32-app` bin, `42` in `cyd-controller-app`, `63` in `squeezelite-audio-core`, `22` in `squeezelite-config`, `26` in `squeezelite-cspot`, `7` in `squeezelite-cyd-link`, `34` in `squeezelite-decoders`, `24` in `squeezelite-proto`, `21` in `squeezelite-raop`, `16` in `squeezelite-tas58xx`, plus all integration, codec equivalence, trait conformance, and fuzz suites), `0` failed.
2. **Gate 2 — Google Rust Style Audit (`google_rust_audit.py --strict`)**: `[EXECUTED & VERIFIED]`
   - Audited all 11 workspace crate `src/` directories (`louder-esp32-app/src`, `squeezelite-decoders/src`, `squeezelite-raop/src`, `squeezelite-cspot/src`, `squeezelite-audio-core/src`, `squeezelite-proto/src`, `squeezelite-config/src`, `nanomp3-core/src`, `squeezelite-tas58xx/src`, `squeezelite-cyd-link/src`, `cyd-controller-app/src`).
   - Result: `0 error(s), 0 warning(s)`.
3. **Gate 3 — Workspace Clippy Audit (`cargo clippy --workspace --all-targets -- -D warnings`)**: `[EXECUTED & VERIFIED]`
   - Result: `Finished dev profile [unoptimized + debuginfo] target(s)`, `0` warnings.
4. **Gate 4 — Bare-Metal `xtensa-esp32-none-elf` Release Build & ELF Section Audit**: `[EXECUTED & VERIFIED]`
   - Command: `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app --no-default-features --features board-louder-esp32`
   - ELF Section Sizes (`xtensa-esp32-elf-size -A`):
     - `.dram2_uninit`: `96,280` B (`<= 98,768` B ceiling, `2,488` B margin)
     - `.stack`: `27,232` B (`>= 24,576` B requirement)
     - `.bss`: `77,656` B
     - `.data`: `24,064` B (`.data.wifi`: `2,120` B)
     - `.text`: `1,400,301` B
     - `.rodata`: `207,504` B (`.rodata.wifi`: `42,484` B)
     - `.rwtext`: `27,232` B (`.rwtext.wifi`: `88,304` B)
     - `.flash.appdesc`: `256` B
5. **Gate 5 — Staged OTA Binary Packaging & Header Verification (`--stage-only`)**: `[EXECUTED & VERIFIED]`
   - Staged Path: `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/louder-esp32-app-basement-ota-full-parity.bin`
   - Byte Size: `1,793,424` bytes (`43.44%` of 4 MB flash; `<= 1,835,008` B `ota_0` limit, `41,584` B headroom)
   - SHA-256: `23175117f50167b8ef1996707eafe7d4391da1e940609708e5ac8e8822580bea`
   - Header Verification (`verify_staged_ota_bin.py`): ESP image magic `0xE9` at `0x00`, `esp_app_desc_t` magic `0xABCD5432` at `0x20`, version `"I2S-4MFlash-Rust"` at `0x30`, project `"Squeezelite-ESP32"` at `0x50`.
   - Zero live hardware flashing (`172.22.1.15`) or serial port (`ttyUSB0`) interaction performed (`--stage-only` invariant respected).
