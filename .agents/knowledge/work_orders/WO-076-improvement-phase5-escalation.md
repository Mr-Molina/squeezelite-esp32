# Work Order: WO-076 — `/improvement` Phase 5 Mechanical Rule, Invariant 44 & Pattern Library Escalation

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-076` |
| **Timestamp** | `2026-10-25T10:06:00-04:00` |
| **Assigned Role** | Infrastructure Administrator (`/improvement` Phase 5 Implementer) |
| **Target Scope** | `.agents/INFRASTRUCTURE_INVARIANTS.md`, `.agents/PATTERN_LIBRARY.yaml`, `/home/console-admin/.gemini/config/pattern-library.yaml`, `/home/console-admin/.gemini/config/rules/verify-autonomous-work.md`, `.agents/improvement-log.jsonl`, `/home/console-admin/.gemini/config/improvement-log.jsonl` |
| **Status** | `[EXECUTED & VERIFIED]` |

## 1. Objective
Execute Phase 5 of `/home/console-admin/.gemini/config/skills/improvement/SKILL.md` for user-approved improvements `IMP-2026-1025-001` (`PM-2026-1025-001`) and `IMP-2026-1025-002` (`PM-2026-1025-002`).

## 2. Mandatory Pre-Modification Backup Gate (Rule 35)
1. Create `.bak` copies of all 4 existing target files using `/bin/cp` with absolute paths before modifying them:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md.bak`
   - `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml.bak`
   - `/home/console-admin/.gemini/config/pattern-library.yaml.bak`
   - `/home/console-admin/.gemini/config/rules/verify-autonomous-work.md.bak`

## 3. Required Updates
1. **`/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`**:
   - Append `## Invariant 44: End-to-End Control, Stream Resync & Sibling-Daemon Regression Verification Gate`:
     - **Rule**: (1) Interactive UI controls MUST NOT be certified via static HTML byte-count checks (`curl GET /`); verification MUST execute at least one state-changing control request against the live endpoint and confirm backend state mutation. (2) Streaming audio decoders MUST advance/consume bytes on all error paths (`CorruptedFrame`, `NoFrame`, `ReservoirUnavailable`, oversized false-sync `frame_bytes`) so a single corrupt byte never deadlocks the ringbuffer. (3) Before and after restarting a parent service daemon (e.g., `logitechmediaserver`), agents MUST verify all previously active child bridge processes (`shairport_helper`, UPnP) remain running.
2. **`/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`**:
   - Append `PAT-2026-1025-001` (`logic` / `project`, `occurrences: 1`, `postmortem_ids: ["PM-2026-1025-001"]`, `mitigation_level: 3`, `automated_check` in `louder-esp32-app` & `squeezelite-decoders` unit tests) and `PAT-2026-1025-002` (`behavioral` / `global`, `occurrences: 1`, `postmortem_ids: ["PM-2026-1025-002"]`, `mitigation_level: 2`, `mechanical_invariant` Invariant 44).
3. **`/home/console-admin/.gemini/config/pattern-library.yaml`**:
   - Update `GLB-006` and `GLB-007`: increment `occurrences`, append `"PM-2026-1025-002"` to `postmortem_ids`, update `last_seen: "2026-10-25"`.
4. **`/home/console-admin/.gemini/config/rules/verify-autonomous-work.md`**:
   - Add explicit `MUST` and `MUST NOT` rules under `## MUST` and `## MUST NOT` enforcing:
     - End-to-end functional execution of interactive Web UI control endpoints (never rely solely on HTML download byte counts).
     - Pre/post-restart sibling helper process verification (`pgrep`/`ss`) when restarting multi-plugin parent daemons.
     - Strict separation between `[AUDIT COMPLETE]` and `[EXECUTED & VERIFIED]` when user-reported features remain unimplemented.
5. Remove the temporary `.bak` files after verifying all edits parse cleanly.

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent Role**: Infrastructure Administrator (`/improvement` Phase 5 Implementer)
- **Completed At**: `2026-10-25T10:12:00-04:00`
- **Actions Executed**:
  1. Created mandatory Rule 35 pre-modification `.bak` copies of all 4 targets using `/bin/cp`.
  2. Codified `Invariant 44: End-to-End Control, Stream Resync & Sibling-Daemon Regression Verification Gate` in `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md`.
  3. Appended `PAT-2026-1025-001` (logic / project / level 3) and `PAT-2026-1025-002` (behavioral / global / level 2) to `/home/console-admin/Github/squeezelite-esp32/.agents/PATTERN_LIBRARY.yaml`.
  4. Updated `GLB-006` and `GLB-007` in `/home/console-admin/.gemini/config/pattern-library.yaml` (incremented occurrences with session `conversation://31deac42-e161-443d-9205-80c67d7541ab`, date `'2026-10-25'`, `postmortem_ref: PM-2026-1025-002`, appended to `postmortem_ids`, updated `last_seen: '2026-10-25'`, escalated `GLB-007` to level 2).
  5. Enhanced `/home/console-admin/.gemini/config/rules/verify-autonomous-work.md` with explicit MUST and MUST NOT directives for end-to-end Web UI control execution, pre/post-restart sibling daemon checks, and strict audit vs. execution status separation, preserving valid YAML frontmatter `trigger: always_on`.
  6. Verified all 4 target files parse cleanly via Python YAML/text assertion scripts.
  7. Removed all temporary `.bak` files.

