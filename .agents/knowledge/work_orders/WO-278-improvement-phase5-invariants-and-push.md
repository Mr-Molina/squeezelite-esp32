# Work Order: WO-278 — `/improvement` Phase 5 & Phase 6: Codify Invariants 48 & 49 (`INFRASTRUCTURE_INVARIANTS.md`), Escalate `PAT-2026-1025-003` & Add `PAT-2026-1101-001` (`PATTERN_LIBRARY.yaml`), & Push to `origin/MK5-Rust`

- **Work Order ID**: `WO-278`
- **Timestamp**: `2026-11-01T16:08:30-05:00`
- **Assigned Subagent Role**: `Extension Certifier & Infrastructure Administrator` (`dbe6e096-d9a9-42c8-99b9-88770d7dc8d7`)
- **Target Host / Component**:
  - `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`
  - `/home/console-admin/Github/squeezelite-esp32/.agents/improvement-log.jsonl`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Scope
Following user approval in `/improvement` Phase 4, write **Invariant 48** and **Invariant 49** to `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`, update `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml` (`PAT-2026-1025-003` occurrence + new `PAT-2026-1101-001` and `PAT-2026-1101-002` entries), verify YAML/JSONL validity, commit all `.agents/*` updates to `MK5-Rust`, and `git push origin MK5-Rust`.

- **Pre-Modification Backups (already created)**:
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/INFRASTRUCTURE_INVARIANTS.md.bak_wo278`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/PATTERN_LIBRARY.yaml.bak_wo278`
  - `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/INDEX.md.bak_wo278`

## 2. Exact File Modifications (Use `edit_file` — NEVER `sed`/`awk`/`cat`)

### 1. `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`
Append **Invariant 48** and **Invariant 49** after Invariant 47:
```markdown
## Invariant 48: Bare-Metal `.dram2_uninit` Field Parity, PSRAM Region-0 Priority & `>= 64 KB` Internal Heap Gate
- **Rule**: (1) Every field added to `StreamPipeline` in `rust/crates/louder-esp32-app/src/pipeline.rs` MUST have a matching `core::ptr::addr_of_mut!((*ptr).<field>).write(...)` initializer in `rust/crates/louder-esp32-app/src/main.rs::init_static_pipeline()` so `MaybeUninit<StreamPipeline>` in `.dram2_uninit` never contains uninitialized pointer/niche bits (`Option<Box<Resampler>>`). (2) On boards with external PSRAM (`4 MB` `ESP32-WROVER`), `init_and_map_psram_4mb()` MUST be registered as `esp_alloc::HEAP` Region 0 BEFORE any `MemoryCapability::Internal` DRAM region so general `Box`/`Vec` allocations (`prewarm_opus`, `Resampler`, `audio_rx_vec`, `slim_rx_vec`) land in PSRAM. (3) Total `MemoryCapability::Internal` heap (`.bss` `heap_allocator!` + `.dram2_uninit` `STATIC_BT_HEAP`) MUST remain `>= 65,536` B (`65 KB`) whenever `esp-radio` Wi-Fi and BTDM coexist, while keeping `.stack >= 36,000` B, `.iram0.text <= 131,072` B, and `.dram2_uninit <= 98,768` B.

## Invariant 49: Live Streaming Throughput, PSRAM Socket Window, Twin-Path Parity & Latency Certification Gate
- **Rule**: (1) Every live certification on `"BASEMENT"` (`172.22.1.15`) MUST measure a `>= 15s` steady-state playback window and verify `frames_played/s` in `44,080..44,120` fps (`>= 99.95%` of `44.1 kHz`), `i2c_acks/s <= 3.05` ACKs/s (`FaultStatus::has_critical_amp_fault()` + 30s fault-recovery cooldown in both `tasks.rs` and `main.rs`), and cached `GET /?action=getInfo` TTFB `< 100 ms` (`ZEROCONF_DH_CACHE`). (2) When `4 MB` PSRAM is active, `smoltcp` TCP receive buffers MUST be allocated in PSRAM (`audio_rx_buf >= 65,536` B, `slim_rx_buf >= 8,192` B) with immediate egress `smol_iface.poll(...)` after socket drain, `PowerSaveMode::None` re-applied after Wi-Fi association, and LMS `mp3StreamingMethod == 1` (LAN proxy). (3) Any bug fix applied to `StaticMp3Decoder` or `tasks.rs` MUST be audited and applied symmetrically to its twin implementation (`Mp3Decoder` or `main.rs`).
```

### 2. `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`
1. Under `PAT-2026-1025-003`, append `PM-2026-1101-002` to `postmortem_ids` and append the `2026-11-01` occurrence (`session: conversation://6b63bd01-ad40-4ba9-a8fa-687005ed880f`, `date: '2026-11-01'`, `postmortem_ref: PM-2026-1101-002`), updating `current_mitigation` to reference `Invariant 45 and Invariant 49 in .agents/INFRASTRUCTURE_INVARIANTS.md`.
2. Append two new patterns at the bottom of `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`:
   - `PAT-2026-1101-001` (`Bare-Metal .dram2_uninit Field Parity, PSRAM Region-0 Priority & 65 KB Internal Heap Gate`, `failure_type: logic`, `scope: project`, `escalation_level: 2`, `mitigation_level: 2`, `current_mitigation: mechanical_invariant — Invariant 48 in .agents/INFRASTRUCTURE_INVARIANTS.md`, `postmortem_ids: [PM-2026-1101-001]`).
   - `PAT-2026-1101-002` (`Twin-Path Implementation Parity & TAS5805M Transient Clock-Fault Filter Gate`, `failure_type: logic`, `scope: project`, `escalation_level: 3`, `mitigation_level: 3`, `current_mitigation: automation & mechanical_invariant — test_mp3_decoder_partial_frame_guard, test_fault_status_has_critical_amp_fault, and Invariant 49 in .agents/INFRASTRUCTURE_INVARIANTS.md`, `postmortem_ids: [PM-2026-1101-003]`).

## 3. Verification, Commit, & Push
1. Write a 5-line verification script in `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/verify_improvement_files.py` and run `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/verify_improvement_files.py` to confirm `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml` parses as valid YAML and `/home/console-admin/Github/squeezelite-esp32/.agents/improvement-log.jsonl` parses as valid JSONL (`exit code 0`).
2. Write your subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo278_improvement_phase5_and_push.md`, complete the Fulfillment Section below, and update `WO-278` to `[EXECUTED & VERIFIED]` in `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`.
3. Stage all modified/created `.agents/*` files, commit (`chore(improvement): codify Invariants 48 & 49 and PAT-2026-1101-001..002 (WO-278)`), and push to `origin/MK5-Rust`. Verify 0 active background tasks before returning.

## 4. Fulfillment Section

- **Execution Timestamp**: `2026-11-01T16:11:00-05:00`
- **Subagent Summary Artifact**: [`20261101_wo278_improvement_phase5_and_push.md`](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo278_improvement_phase5_and_push.md)
- **Fulfillment Status**: `[EXECUTED & VERIFIED]`

### Summary of Execution Results

1. **Codified Invariant 48 & Invariant 49 in `INFRASTRUCTURE_INVARIANTS.md`**:
   - `Invariant 48`: Bare-Metal `.dram2_uninit` Field Parity, PSRAM Region-0 Priority & `>= 64 KB` Internal Heap Gate.
   - `Invariant 49`: Live Streaming Throughput, PSRAM Socket Window, Twin-Path Parity & Latency Certification Gate.

2. **Updated Pattern Library in `PATTERN_LIBRARY.yaml`**:
   - Escalated `PAT-2026-1025-003`: Appended `PM-2026-1101-002` to `postmortem_ids` and added the `2026-11-01` occurrence, referencing `Invariant 45 and Invariant 49`.
   - Added `PAT-2026-1101-001` (`Bare-Metal .dram2_uninit Field Parity, PSRAM Region-0 Priority & 65 KB Internal Heap Gate`, escalation level 2, `PM-2026-1101-001`).
   - Added `PAT-2026-1101-002` (`Twin-Path Implementation Parity & TAS5805M Transient Clock-Fault Filter Gate`, escalation level 3, `PM-2026-1101-003`).

3. **Syntax & Schema Verification**:
   - Executed `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/verify_improvement_files.py` via Python:
     - `PATTERN_LIBRARY.yaml`: 21 patterns parsed cleanly (valid YAML).
     - `improvement-log.jsonl`: 10 records parsed cleanly (valid JSONL).
     - All assertion checks passed with exit code 0.

4. **Repository Push**:
   - Staged all `.agents/*` modifications, committed as `chore(improvement): codify Invariants 48 & 49 and PAT-2026-1101-001..002 (WO-278)`, and pushed to `origin/MK5-Rust`.
   - Verified 0 active background tasks.
