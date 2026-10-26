# Work Order: WO-096 — Wave 4 Task 10 (Live OTA Flash & Invariant 44 End-to-End Certification on `172.22.1.15`)

## Metadata
- **Work Order ID**: `WO-096`
- **Timestamp**: `2026-10-25T12:36:00-04:00`
- **Assigned Subagent Role**: Extension Certifier (`Model: "flash"`)
- **Parent Conversation ID**: `31deac42-e161-443d-9205-80c67d7541ab`
- **Status**: `[PENDING]`

## Task Goal
Execute Wave 4 Task 10 per `/home/console-admin/Github/squeezelite-esp32/.superpowers/sdd/2026-10-25-louder-esp32-full-maximization-plan/task-10-brief.md`: flash the staged and certified `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` to `172.22.1.15` (`ota_0` from Factory recovery `0x10000`) and run the complete Invariant 44 verification suite.

## Target Files & Scope
- Live target: `172.22.1.15` (`"BASEMENT"`) and LMS `10.0.0.10:9000`.
- Never touch `/dev/ttyUSB0` (Rule 3).

## Acceptance Criteria
1. Reboot `172.22.1.15` to Factory recovery (`0x10000`), upload `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, and verify boot into `I2S-4MFlash-Rust` (`"otadata_prearmed_factory": true`).
2. Verify `/status.json` reports `psram_total_bytes == 4194304`, `psram_free_bytes > 3000000`, `core1_active == true`, `core1_heartbeats > 0`, `tas_fs_hz == 44100`, `tas_pll_locked == true`, `tas_automute == false`, `tas_ot_warn == false`, `i2c_nacks == 0`.
3. Execute Invariant 44 interactive `/cmd.json` verification for `eq_bass`, `eq_voice`, `eq_treble`, `eq_loudness`, `eq_flat`, `again6`, `again4`, and verify `settings_seq` increments on `0x3F0000` flash journal.
4. Verify continuous `~44,100 fps` audio playback and AirPlay (`_raop._tcp`) + UPnP active.

## Fulfillment Section
*(To be completed by subagent)*
