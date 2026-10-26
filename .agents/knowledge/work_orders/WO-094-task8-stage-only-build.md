# Work Order: WO-094 — Wave 3 Task 8 (`--stage-only` Release Build, `.stack >= 36,000` B & `verify_staged_ota_bin.py`)

## Metadata
- **Work Order ID**: `WO-094`
- **Timestamp**: `2026-10-25T12:35:00-04:00`
- **Assigned Subagent Role**: Wave 3 Task 8 Build & Stage-Only Certifier (`Model: "flash"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[EXECUTED & VERIFIED]`

## Task Goal
Execute Wave 3 Task 8 per `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-8-brief.md`: run full host `cargo test --workspace`, build the `xtensa-esp32-none-elf` release ELF, verify `.stack >= 36,000` B via `readelf -S`, generate `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, and run `verify_staged_ota_bin.py` (`--stage-only`).

## Target Files & Scope
- Read-only / build verification and staging to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`.
- Do **NOT** flash `172.22.1.15` (`--stage-only` gate).

## Acceptance Criteria
1. `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` passes 100%.
2. Release build on `xtensa-esp32-none-elf` succeeds with `.stack >= 36,000` B (`>= 0x8CA0`).
3. `verify_staged_ota_bin.py` outputs `[STAGE-ONLY AUDIT PASS]`.

## Fulfillment Section
### Execution & Verification Summary
- **Execution Date**: `2026-10-25T13:44:00-04:00`
- **Subagent**: Wave 3 Task 8 Build & Stage-Only Certifier (`WO-094`)
- **Final Status**: `DONE` (`[EXECUTED & VERIFIED]`)
- **Verification Evidence**:
  1. `cargo test --workspace`: 216 tests passed across all workspace crates (`cyd_controller_app`, `louder_esp32_app`, `squeezelite_audio_core`, `squeezelite_config`, `squeezelite_cyd_link`, `squeezelite_decoders`, `squeezelite_proto`, `squeezelite_tas58xx`). 0 failed.
  2. Release ELF Build: `Finished release [optimized] target(s)` (`xtensa-esp32-none-elf`).
  3. `.stack` Inspection: `readelf -S` confirms `.stack` size `0x008df0` (36,336 bytes, `>= 0x008ca0` / 36,000 B).
  4. Packaging: `espflash save-image --chip esp32 --flash-size 4mb` generated `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (755,104 bytes, sha256 `ead0766b2f625e40af3fca7b30960222a9b8a614524b1ec8457d90fbc7e0e995`).
  5. Stage-Only Header Audit: `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py` reported `[STAGE-ONLY AUDIT PASS]` (`img_magic=0xE9`, `app_desc_magic=0xABCD5432`, `project_name='Squeezelite-ESP32'`, `version='I2S-4MFlash-Rust'`, `idf_ver='4.3.0'`).
  6. Gate Check: Hardware at `172.22.1.15` remains untouched (`--stage-only` verified).
- **Report**: `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-8-report.md`
