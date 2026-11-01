# Work Order: WO-261 — Phase 0a Code Scout 1: `MK5-Rust` (`84feda56`) Topology, LOC, Build Profile & Commit Diff Inventory

- **Work Order ID**: `WO-261`
- **Timestamp**: `2026-11-01T13:28:30-05:00`
- **Assigned Subagent Role**: `Topology, LOC & Build Profile Scout` (`Model: "flash"`, `TypeName: "research"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust`
- **Status**: `[IN PROGRESS]`

## 1. Objective & Strict Phase 0a Anti-Leakage Guardrail
Execute a fast (<90s), strictly **read-only** Phase 0a repository topology, LOC, Cargo profile, and commit-diff inventory.
- **DO NOT** inspect algorithm logic or propose fixes (Phase 1 Micro-Auditors handle deep code inspection).

## 2. Inventory Tasks
1. **Per-File LOC Inventory**:
   - List exact line counts (`wc -l`) for all `.rs` files in `/home/console-admin/Github/squeezelite-esp32/rust/crates/**/src/*.rs` and top-level counts for `/home/console-admin/Github/squeezelite-esp32/rust/vendor/*`.
2. **Cargo Build & Optimization Profiles**:
   - Inspect `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/Cargo.toml`, and `.cargo/config.toml` for `[profile.release]`, `[profile.dev]`, `opt-level`, `lto`, `codegen-units`, and per-package overrides.
3. **Recent Commit Diff Surface (`cd689da1..84feda56` & `e7fa3411..ac281d18`)**:
   - Run `git -C /home/console-admin/Github/squeezelite-esp32 diff --stat e7fa3411..ac281d18` and `git -C /home/console-admin/Github/squeezelite-esp32 diff --stat cd689da1..ac281d18` to list every modified file and insertion/deletion count.

## 3. Deliverable
Return the structured Topology, LOC, Profile, and Diff inventory report.
