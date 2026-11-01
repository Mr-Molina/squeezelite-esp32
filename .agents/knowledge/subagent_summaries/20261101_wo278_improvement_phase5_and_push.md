# Subagent Summary: WO-278 Improvement Phase 5 & 6 Invariants Codification & origin/MK5-Rust Git Push

- **Subagent Role**: Extension Certifier & Infrastructure Administrator
- **Timestamp**: `2026-11-01T16:11:00-05:00`
- **Work Order ID**: `WO-278`
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Summary of Actions & Scope Integrity

### Target Files Modified
- `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`
- `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-278-improvement-phase5-invariants-and-push.md`
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/INDEX.md`

### Files Created
- `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101_wo278_improvement_phase5_and_push.md` (this summary)
- `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/verify_improvement_files.py` (syntax verification script)

### Codebase Isolation
- Confirmed zero modifications to `/home/console-admin/Github/squeezelite-esp32/rust/`.
- All modifications strictly confined to `.agents/`.

---

## 2. Invariants Codified

### Invariant 48: Bare-Metal `.dram2_uninit` Field Parity, PSRAM Region-0 Priority & `>= 64 KB` Internal Heap Gate
- **Rule**:
  1. Every field added to `StreamPipeline` in `pipeline.rs` MUST have a matching `core::ptr::addr_of_mut!((*ptr).<field>).write(...)` initializer in `main.rs::init_static_pipeline()`.
  2. On boards with external PSRAM (`4 MB ESP32-WROVER`), `init_and_map_psram_4mb()` MUST be registered as `esp_alloc::HEAP` Region 0 BEFORE any `MemoryCapability::Internal` DRAM region.
  3. Total `MemoryCapability::Internal` heap (`.bss` `heap_allocator!` + `.dram2_uninit` `STATIC_BT_HEAP`) MUST remain `>= 65,536 B` (`65 KB`) whenever `esp-radio` Wi-Fi and BTDM coexist, while keeping `.stack >= 36,000 B`, `.iram0.text <= 131,072 B`, and `.dram2_uninit <= 98,768 B`.

### Invariant 49: Live Streaming Throughput, PSRAM Socket Window, Twin-Path Parity & Latency Certification Gate
- **Rule**:
  1. Every live certification on `"BASEMENT"` (`172.22.1.15`) MUST measure a `>= 15s` steady-state playback window and verify `frames_played/s` in `44,080..44,120` fps (`>= 99.95%` of `44.1 kHz`), `i2c_acks/s <= 3.05` ACKs/s (`FaultStatus::has_critical_amp_fault()` + 30s fault-recovery cooldown in both `tasks.rs` and `main.rs`), and cached `GET /?action=getInfo` TTFB `< 100 ms` (`ZEROCONF_DH_CACHE`).
  2. When `4 MB` PSRAM is active, `smoltcp` TCP receive buffers MUST be allocated in PSRAM (`audio_rx_buf >= 65,536 B`, `slim_rx_buf >= 8,192 B`) with immediate egress `smol_iface.poll(...)` after socket drain, `PowerSaveMode::None` re-applied after Wi-Fi association, and LMS `mp3StreamingMethod == 1` (LAN proxy).
  3. Any bug fix applied to `StaticMp3Decoder` or `tasks.rs` MUST be audited and applied symmetrically to its twin implementation (`Mp3Decoder` or `main.rs`).

---

## 3. Pattern Library Updates

1. **Escalated `PAT-2026-1025-003`**:
   - Added `PM-2026-1101-002` to `postmortem_ids`.
   - Appended `2026-11-01` recurrence occurrence (`session: conversation://6b63bd01-ad40-4ba9-a8fa-687005ed880f`).
   - Updated `current_mitigation` to reference both `Invariant 45 and Invariant 49 in .agents/INFRASTRUCTURE_INVARIANTS.md`.
2. **Added `PAT-2026-1101-001`**:
   - Name: `Bare-Metal .dram2_uninit Field Parity, PSRAM Region-0 Priority & 65 KB Internal Heap Gate`
   - Failure Type: `logic`, Scope: `project`, Escalation Level: `2`, Mitigation Level: `2`
   - Mitigation: `mechanical_invariant — Invariant 48 in .agents/INFRASTRUCTURE_INVARIANTS.md`
   - Postmortem IDs: `[PM-2026-1101-001]`
3. **Added `PAT-2026-1101-002`**:
   - Name: `Twin-Path Implementation Parity & TAS5805M Transient Clock-Fault Filter Gate`
   - Failure Type: `logic`, Scope: `project`, Escalation Level: `3`, Mitigation Level: `3`
   - Mitigation: `automation & mechanical_invariant — test_mp3_decoder_partial_frame_guard, test_fault_status_has_critical_amp_fault, and Invariant 49 in .agents/INFRASTRUCTURE_INVARIANTS.md`
   - Postmortem IDs: `[PM-2026-1101-003]`

---

## 4. Verification Evidence

- Executed `/usr/bin/python3 /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/verify_improvement_files.py`:
  - `PATTERN_LIBRARY.yaml`: 21 patterns parsed cleanly (valid YAML).
  - `improvement-log.jsonl`: 10 records parsed cleanly (valid JSONL).
  - Assertions confirming presence, keys, occurrences, and escalation levels for all updated/new patterns verified (exit code 0).
