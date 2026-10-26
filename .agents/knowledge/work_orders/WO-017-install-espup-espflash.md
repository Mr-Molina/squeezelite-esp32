# Work Order: WO-017 — Install `espup` (`+esp` Xtensa Rust Toolchain) and `espflash`

**Work Order ID:** WO-017  
**Timestamp:** 2026-10-20T22:10:45-04:00  
**Assigned Subagent Role:** Infrastructure Administrator  
**Target Files:** `/home/console-admin/.cargo/bin/{espup,espflash,cargo-espflash}`, `/home/console-admin/export-esp.sh`  
**Backup Files:** N/A (New toolchain binaries installation)  
**Defect IDs:** `HW-PREP-03`  
**Task Goal:** Install `espup` and `espflash` into `/home/console-admin/.cargo/bin/` and run `espup install` so that Cargo can cross-compile `xtensa-esp32-none-elf` / `xtensa-esp32s3-none-elf` / `xtensa-esp32-espidf` and flash `/dev/ttyUSB0`.

---

## 1. Scope & Instructions
1. Install `espup` and `espflash`:
   - Check if `cargo-binstall` is installed (`/home/console-admin/.cargo/bin/cargo-binstall`), or install `espup` and `espflash` using `/home/console-admin/.cargo/bin/cargo install espup espflash --locked` (or prebuilt release if faster, or `cargo install`).
2. Provision the Espressif Xtensa Rust toolchain (`+esp`):
   - Execute `/home/console-admin/.cargo/bin/espup install` (which installs the `esp` Rust toolchain supporting `xtensa-esp32-none-elf`, `xtensa-esp32s3-none-elf`, `xtensa-esp32-espidf`, `xtensa-esp32s3-espidf` and writes `/home/console-admin/export-esp.sh`).
3. Empirically Verify Installation (Rule 21 & Rule 52):
   - Source `/home/console-admin/export-esp.sh` and verify:
     - `/home/console-admin/.cargo/bin/rustup toolchain list` shows `esp`
     - `/home/console-admin/.cargo/bin/rustc +esp --version` exits `0`
     - `/home/console-admin/.cargo/bin/espflash --version` exits `0`
     - `/home/console-admin/.cargo/bin/espflash board-info --port /dev/ttyUSB0` (non-destructive read) exits `0`
4. Write a Rule 17 subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo017_esp_toolchain_install.md` and update the Fulfillment Section of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-017-install-espup-espflash.md` to `[EXECUTED & VERIFIED]`.
5. Note (Rule 21): Do NOT emit completion until all background commands have exited with code `0` and `manage_task(list)` has zero active tasks.

---

## 4. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T22:15:00-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/.cargo/bin/espup` (v0.18.0)
  - `/home/console-admin/.cargo/bin/espflash` (v4.6.0)
  - `/home/console-admin/.cargo/bin/cargo-espflash` (v4.6.0)
  - `/home/console-admin/export-esp.sh`
  - `/home/console-admin/.rustup/toolchains/esp`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/2026-10-20_wo017_esp_toolchain_install.md`
- **Verification Evidence:**
  - `rustup toolchain list` includes `esp`
  - `rustc +esp --version` -> `rustc 1.97.0-nightly (8ea53bcd7 2026-07-08) (1.97.0.0)` (exit `0`)
  - `espflash --version` -> `espflash 4.6.0` (exit `0`)
  - `espflash board-info --port /dev/ttyUSB0` -> ESP32 v3.0, 4MB Flash, MAC d4:d4:da:5a:e7:3c (exit `0`)
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
