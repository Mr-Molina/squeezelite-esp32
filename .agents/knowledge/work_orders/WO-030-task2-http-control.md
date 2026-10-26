# Work Order: WO-030 — Implement Task 2 (`src/http_control.rs` & `pub mod http_control;`)

**Work Order ID:** WO-030  
**Timestamp:** 2026-10-21T23:07:00-04:00  
**Assigned Subagent Role:** Task 2 Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`  
**Defect IDs:** Anti-Brick Gate 3 (`DeadManLeaseWatchdog`, 45s/60s) & Gate 4 (`handle_http_control_request` on Port 80)  
**Task Goal:** Implement `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`, export `pub mod http_control;` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`, verify all 3 unit tests pass, commit the change, and write the report and subagent summary.  

---

## 1. Context & Scope
- **Problem Statement:** `louder-esp32-app` requires a pure-Rust `no_std` module (`src/http_control.rs`) providing the Port 80 HTTP recovery server (`/status.json`, `/recovery.json`, `/reboot.json`, `/reboot_ota.json`) and the 45s/60s `DeadManLeaseWatchdog`.
- **Single Variable Constraint (Rule 5):** Create `src/http_control.rs` and add `pub mod http_control;` to `src/lib.rs` only.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Verify `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak` exists before editing `src/lib.rs`.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 21, Rule 52):** Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app http_control` and confirm exit code `0` with all 3 tests passing.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Creating `src/http_control.rs` from `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-2-brief.md` and exporting `pub mod http_control;` in `src/lib.rs` will compile cleanly and pass all 3 `http_control::tests::*` unit tests.
- **Predicted Outcome:** `cargo test -p louder-esp32-app http_control` exits `0` with 3/3 tests passing.
- **Falsification Criteria:** Any compiler error or test failure in `http_control`.

---

## 3. Detailed Instructions
1. Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-2-brief.md`.
2. Write `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs` using `write_file` and add `pub mod http_control;` to `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` using `edit_file`.
3. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app http_control`.
4. Commit your changes (`git add` + `git commit -m "feat(louder-esp32-app): add Port 80 HTTP recovery server and dead-man lease watchdog"`).
5. Write the SDD report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-2-report.md`, write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_230700_task2_implementer.md`, and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-21T23:13:26-04:00
- **Files Modified / Created (Absolute Paths):**
  - Created: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs`
  - Modified: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
- **Backup Verification:**
  - Backup Paths:
    - `/home/console-admin/.gemini/antigravity/brain/8f3bd86e-8bd4-4109-8aaf-f4cf6a0a01b3/scratch/lib.rs.bak`
    - `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`
- **Test / Verification Command & Results:**
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app http_control` -> 3 passed; 0 failed; exit code 0.
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app` -> 78 passed; 0 failed; exit code 0.
- **Observed Outcome vs Predicted Outcome:** Matches predicted outcome exactly. All unit tests compile cleanly and pass without regression.
- **Commit:** `76a5ea00` (`feat(louder-esp32-app): add Port 80 HTTP recovery server and dead-man lease watchdog`)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_230700_task2_implementer.md`
- **Report Path:** `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-2-report.md`
- **Handoff Path:** [WO-030-task2-http-control.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-030-task2-http-control.md)
