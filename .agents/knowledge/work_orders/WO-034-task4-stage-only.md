# Work Order: WO-034 — Implement Task 4 (`--stage-only` `espflash save-image` & `verify_staged_ota_bin.py`)

**Work Order ID:** WO-034  
**Timestamp:** 2026-10-21T23:29:15-04:00  
**Assigned Subagent Role:** Task 4 Implementer (`Model: "flash"`, `TypeName: "self"`)  
**Target Files:** `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py`  
**Backup Files:** N/A (New Staged Artifact & Script in `scratch/`)  
**Defect IDs:** `--stage-only` Binary Packaging & `squeezelite-ota.c:ota_header_check()` Pre-Flight Audit  
**Task Goal:** Package `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` into `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` via `espflash save-image --chip esp32`, write and execute `verify_staged_ota_bin.py`, and record the `[STAGE-ONLY AUDIT PASS]` output.  

---

## 1. Context & Scope
- **Problem Statement:** Before asking user permission for Task 5 (`172.22.1.15` Wi-Fi OTA), the compiled bare-metal ELF must be packaged into an ESP-IDF OTA `.bin` image (`--stage-only`) and verified byte-for-byte against `squeezelite-ota.c:ota_header_check()` (`0xE9` at `0x00`, `0xABCD5432` at `0x20`, `"I2S-4MFlash-Rust"` at `0x30`, `"Squeezelite-ESP32"` at `0x50`, `size <= 0x290000`).
- **Single Variable Constraint (Rule 5):** Create staged `.bin` and `verify_staged_ota_bin.py` in `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/` only.
- **Pre-Modification Integrity (Rule 31):** Verify `/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app` exists.
- **Prohibited Actions (Rule 3, Rule 23, `staging-before-restart.md`):** Never touch `/dev/ttyUSB0`. Never send any HTTP request to `172.22.1.15`. Never use `sed`, `awk`, or `cat` for file modifications. Never spawn subagents.
- **Verification Requirement (Rule 21, Rule 52):** Run `espflash save-image` and `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` and confirm `[STAGE-ONLY AUDIT PASS]` (`exit code 0`).

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** `espflash save-image --chip esp32` will package the compiled `louder-esp32-app` ELF with `esp_app_desc!("I2S-4MFlash-Rust", "Squeezelite-ESP32", ...)` placed at offset `0x20`, and `verify_staged_ota_bin.py` will pass all assertions (`exit code 0`).
- **Predicted Outcome:** `verify_staged_ota_bin.py` prints `[STAGE-ONLY AUDIT PASS]` with `project_name='Squeezelite-ESP32'`, `version='I2S-4MFlash-Rust'`, `app_desc_magic=0xABCD5432`, and `exit code 0`.
- **Falsification Criteria:** Any assertion failure in `verify_staged_ota_bin.py`.

---

## 3. Detailed Instructions
1. Read `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-4-brief.md`.
2. Run `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`.
3. Write `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` using `write_file` and run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py`.
4. Write the SDD report to `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-4-report.md`, write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_232915_task4_implementer.md`, and complete the Fulfillment Section below.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-21T23:36:20-04:00
- **Files Modified / Created (Absolute Paths):**
  - Created: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`
  - Created: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py`
  - Created: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-21-basement-172-22-1-15-anti-brick-rust-ota-plan/task-4-report.md`
  - Created: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_232915_task4_implementer.md`
- **Backup Verification:** N/A (New staged artifacts in `scratch/`)
- **Test / Verification Command & Results:**
  1. `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` -> saved 659,776 byte image, exit code `0`.
  2. `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` -> `[STAGE-ONLY AUDIT PASS]` (`size=659776`, `sha256=856697537255742d58d5b2a7dfaebf0fc718207255cad3e458e44f72fff4cae6`, `img_magic=0xE9`, `app_desc_magic=0xABCD5432`, `project_name='Squeezelite-ESP32'`, `version='I2S-4MFlash-Rust'`), exit code `0`.
- **Observed Outcome vs Predicted Outcome:** Exactly matched predicted outcome; all assertions passed with exit code `0`.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261021_232915_task4_implementer.md`
- **Handoff Path:** [WO-034-task4-stage-only.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-034-task4-stage-only.md)
