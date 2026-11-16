# squeezelite-esp32 (MK5-Rust)

Rust port of squeezelite (a Logitech Media Server player) for ESP32. The active goal is the Rust conversion; the legacy C/ESP-IDF tree (`components/`, `main/`) is reference only. Default branch: `MK5-Rust`.

## Read first

- `docs/mk5-rust-todo.md` — the task list and crate map. Start here for any Rust work.
- `.agents/INFRASTRUCTURE_INVARIANTS.md` — binding guardrails. Read it before hardware, OTA, git-remote or multi-agent work; it is not auto-loaded because of its size.
- `.agents/AGENTS.md` — agent roles and delegation structure.
- `.agents/sessions.md` — session ledger, newest entry first. Read the top entry for continuity; prepend a new entry when closing out a work order.
- `.agents/knowledge/` — `DEVICE_STATE.md`, `SYSTEM_STATE.md`, `ACTIVE_BUGS.md`, `DIAGNOSTIC_LOG.md`, and `work_orders/`.

## Hard safety rules (live device "BASEMENT", 172.22.1.15)

- Never touch `/dev/ttyUSB0`.
- Never call `POST /reboot_ota.json`.
- Do not flash, reboot or OTA the device without explicit owner approval in the current session.
- `D1` (Core 1 decode/output migration) is owner-gated: do not start it without explicit authorization.

## Rust workspace

All Rust lives under `rust/` (Cargo workspace, toolchain pinned in `rust/rust-toolchain.toml`). Run from `rust/`; these are the CI gates in `.github/workflows/rust-ci.yml`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI also has a `firmware` job (Espressif `+esp` toolchain): Xtensa clippy, the `esp-rtos-core1` feature check, the release build of `louder-esp32-app`, and `scripts/check_section_budget.py` on the resulting ELF. Commands are in `docs/mk5-rust-todo.md` section 2.

## Conventions

- Final responses that ask for permission or stop work end with a `### Proposal Summary` block (Invariant 70).
- Commit style: conventional commits, e.g. `docs(mk5-rust): ...`, `chore(repo): ...`.
- Do not push to `origin` without the owner's go-ahead.
