# Work Order: WO-032 — Implement Task 3 (`Cargo.toml`, `src/lib.rs`, `src/main.rs` on `xtensa-esp32-none-elf`)

**Work Order ID:** WO-032  
**Timestamp:** 2026-10-21T23:16:00-04:00  
**Assigned Subagent Role:** Task 3 Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_Cargo.toml.bak`, `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`, `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak`  
**Defect IDs:** 4-Gate Anti-Brick Architecture, `esp_app_desc!("I2S-4MFlash-Rust", "Squeezelite-ESP32", ...)`, `esp-rtos` + `esp-radio` + `smoltcp` multi-socket loop, and physical `I2S0` DMA (`BCK=26, WS=25, DOUT=22`)  
**Task Goal:** Wire the 4-Gate Anti-Brick Architecture, `esp_app_desc!`, `esp-rtos` + `esp-radio` + `smoltcp` networking, and physical `I2S0` DMA into `Cargo.toml` and `src/main.rs`, verify all host unit tests, Clippy, and `xtensa-esp32-none-elf` release compilation pass (`exit code 0`), commit the changes, and write the report and subagent summary.  

---

## 1. Context & Scope
- **Problem Statement:** `louder-esp32-app` on `target_arch = "xtensa"` must execute all 4 Anti-Brick Gates (`otadata` `0xD000` erase to `0xFF`, `TIMG0` 20s WDT + `custom_halt`, 45s/60s `DeadManLeaseWatchdog`, Port 80 HTTP recovery server), decrypt `"iot"` credentials from `0x9000` NVS, associate via `esp-radio` + `smoltcp`, connect to LMS `10.0.0.10:3483`, and drive `I2S0` DMA (`BCK=GPIO26`, `WS=GPIO25`, `DOUT=GPIO22`) + `I2C0` TAS5805M (`0x2D`).
- **Single Variable Constraint (Rule 5):** Update `Cargo.toml` and `src/main.rs` (`src/lib.rs` already exports `http_control` and `ota_safety` from Tasks 1 & 2) as specified in `task-3-brief.md`.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Copy `Cargo.toml` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_Cargo.toml.bak` and `src/main.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak` before editing.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 21, Rule 52):** Run:
  1. `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace`
  2. `/home/console-admin/.cargo/bin/cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
  3. `/bin/bash -c "source /home/console-admin/export-esp.sh && /home/console-admin/.cargo/bin/rustup run esp cargo build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app"`

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Updating `Cargo.toml` and `src/main.rs` per `task-3-brief.md` will pass all workspace tests, pass Clippy with 0 warnings, and compile a release ELF for `xtensa-esp32-none-elf` (`exit code 0`).
- **Predicted Outcome:** All 3 verification commands exit `0`.
- **Falsification Criteria:** Any compiler error, Clippy warning, or test failure on host or `xtensa-esp32-none-elf`.

---

## 3. Detailed Instructions
1. Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-3-brief.md` (note: read lines 1-816).
2. Create Rule 35 backups for `Cargo.toml` and `src/main.rs` (`louder_lib.rs.bak` already exists).
3. Edit `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` using `edit_file`.
4. Run all 3 verification commands in Step 5 (`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`). Fix any warnings or compiler diagnostics on `xtensa-esp32-none-elf` if encountered.
5. Commit your changes (`git add` + `git commit -m "feat(louder-esp32-app): wire 4-gate anti-brick OTA, esp-radio Wi-Fi, smoltcp, and I2S0 DMA"`).
6. Write the SDD report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-3-report.md`, write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_231600_task3_implementer.md`, and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-21T23:27:49-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
  - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.lock`
  - `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-3-report.md`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_231600_task3_implementer.md`
- **Backup Verification:**
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_Cargo.toml.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_main.rs.bak`
  - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/WO-032-task3-xtensa-hw.md.bak`
- **Test / Verification Command & Results:**
  - `cargo test --workspace`: exit code `0` (all 75 louder-esp32-app tests and workspace tests passed).
  - `cargo clippy --workspace --all-targets -- -D warnings`: exit code `0` (0 warnings).
  - `rustup run esp cargo build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`: exit code `0` (built release ELF binary `louder-esp32-app`, 1,047,084 bytes).
- **Observed Outcome vs Predicted Outcome:** Matched prediction. Bare-metal release binary compiled cleanly with 0 warnings after resolving macro parameter order and API parity.
- **Commit:** `7c5c0539` (`feat(louder-esp32-app): wire 4-gate anti-brick OTA, esp-radio Wi-Fi, smoltcp, and I2S0 DMA`)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_231600_task3_implementer.md`
- **Report Path:** `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-3-report.md`
- **Handoff Path:** [WO-032-task3-xtensa-hw.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-032-task3-xtensa-hw.md)
