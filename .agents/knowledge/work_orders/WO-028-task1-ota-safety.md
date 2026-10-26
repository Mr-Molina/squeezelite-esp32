# Work Order: WO-028 — Implement Task 1 (`src/ota_safety.rs` & `pub mod ota_safety;`)

**Work Order ID:** WO-028  
**Timestamp:** 2026-10-21T23:00:15-04:00  
**Assigned Subagent Role:** Task 1 Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`  
**Defect IDs:** Anti-Brick Gate 1 (`otadata` `0xD000` pre-arming), NVS `0x9000` Wi-Fi Decryptor (`network_wifi.c`), & `squeezelite-ota.c` `ota_header_check()` Verifier  
**Task Goal:** Implement `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`, export `pub mod ota_safety;` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`, verify all 6 unit tests pass, commit the change, and write the report and subagent summary.  

---

## 1. Context & Scope
- **Problem Statement:** `louder-esp32-app` requires a zero-dependency `no_std` module (`src/ota_safety.rs`) that reads Wi-Fi STA credentials from the 4MB flash `nvs` partition (`0x9000`), decrypts `"enc:"` passwords (`SHA256("WiFiNVSKey$Secur" || mac)[0..16]` XOR), verifies `otadata` (`0xD000..0xF000`) `0xFF` erase and `OtaSelectEntry` CRC32, and validates ESP-IDF OTA binary headers against `squeezelite-ota.c:ota_header_check()`.
- **Single Variable Constraint (Rule 5):** Create `src/ota_safety.rs` and add `pub mod ota_safety;` to `src/lib.rs` only.
- **Pre-Modification Integrity (Rule 31, Rule 35):** Copy `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak` before editing `src/lib.rs`.
- **Prohibited Actions (Rule 3, Rule 23):** Never touch `/dev/ttyUSB0`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 21, Rule 52):** Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app ota_safety` and confirm exit code `0` with all 6 tests passing.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** Creating `src/ota_safety.rs` from `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-1-brief.md` and exporting `pub mod ota_safety;` in `src/lib.rs` will compile cleanly and pass all 6 `ota_safety::tests::*` unit tests.
- **Predicted Outcome:** `cargo test -p louder-esp32-app ota_safety` exits `0` with 6/6 tests passing.
- **Falsification Criteria:** Any compiler error or test failure in `ota_safety`.

---

## 3. Detailed Instructions
1. Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-1-brief.md`.
2. Back up `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak`.
3. Write `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs` using `write_file` and add `pub mod ota_safety;` to `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs` using `edit_file`.
4. Run `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app ota_safety`.
5. Commit your changes (`git add` + `git commit`).
6. Write the SDD report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-1-report.md`, write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_230015_task1_implementer.md`, and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-21T23:03:40-04:00
- **Files Modified / Created (Absolute Paths):**
  - Created: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/ota_safety.rs`
  - Modified: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/lib.rs`
- **Backup Verification:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder_lib.rs.bak` created and verified prior to modification.
- **Test / Verification Command & Results:**
  - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app ota_safety` -> 6 passed; 0 failed (exit code 0).
  - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app` -> 75 passed; 0 failed (exit code 0).
- **Observed Outcome vs Predicted Outcome:** Matched prediction exactly. All 6 `ota_safety::tests::*` unit tests compiled cleanly and passed on first execution.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Commit:** `0c38d9ef` (`feat(louder-esp32-app): add pure-Rust NVS Wi-Fi decryptor and OTA safety verifier`)
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_230015_task1_implementer.md`
- **Handoff Path:** [task-1-report.md](file:///home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-1-report.md)
