# Work Order `WO-161`: `/deep-code-audit` Phase 0a Scout 4 — Static Tool & `rust-pro` Prober

- **ID**: `WO-161`
- **Timestamp**: `2026-10-27T17:03:00-04:00`
- **Subagent Role**: Static Tool Prober (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
Execute `/deep-code-audit` Phase 0a Scout 4 + `rust-pro` Static Audit on `/home/console-admin/Github/squeezelite-esp32/rust`:
1. Run the `rust-pro` Google Rust Style & Unsafe Dual-Contract static auditor across `/home/console-admin/Github/squeezelite-esp32/rust/crates`:
   `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates --strict`
2. Run host Clippy check in `/home/console-admin/Github/squeezelite-esp32/rust`:
   `cargo clippy --workspace --all-targets -- -D warnings`
3. Probe if `cargo-audit`, `semgrep`, or `trivy` are installed (`command -v`) and capture any fast read-only hotspots.
4. Inventory all `unsafe` blocks across `/home/console-admin/Github/squeezelite-esp32/rust/crates` (file, line number, and whether `// JUSTIFICATION:` and `// SAFETY:` comments are present).
5. **Zero Phase Leakage Constraint**: Capture raw tool outputs and hotspots only (<90s). Do NOT modify any files. Append your Fulfillment Summary (in YAML format) to §2 of this Work Order file.

## 2. Fulfillment Section
*(To be completed by subagent)*
