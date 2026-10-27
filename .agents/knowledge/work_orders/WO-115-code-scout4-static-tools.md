# Work Order `WO-115`: `/deep-code-audit` Phase 0a Scout 4 — Static Tool Prober

- **ID**: `WO-115`
- **Timestamp**: `2026-10-25T20:21:15-04:00`
- **Subagent Role**: Static Tool Prober (`Model: "flash"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are the **Static Tool Prober** (`Phase 0a` of `/deep-code-audit`) for `/home/console-admin/Github/squeezelite-esp32/rust`.
1. Probe the local environment for installed static analysis and compiler tools (`cargo clippy`, `cargo audit`, `semgrep`, `trivy`).
2. Run fast read-only checks on `/home/console-admin/Github/squeezelite-esp32/rust`:
   - `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets --message-format=short`
   - Check if `cargo-audit`, `semgrep`, or `trivy` are installed (`command -v ...`) and run fast read-only scans on `/home/console-admin/Github/squeezelite-esp32/rust` if available.
   - Count `unsafe` blocks across `/home/console-admin/Github/squeezelite-esp32/rust/crates/` (`grep -rn "unsafe" /home/console-admin/Github/squeezelite-esp32/rust/crates/`).
3. **Strict Anti-Leakage Constraint**: Capture raw tool outputs and line locations only. Skip missing tools gracefully. Do NOT manually audit AST/logic. Complete in <60s and return pure YAML.

## 2. Fulfillment Section
*(To be completed by subagent)*
