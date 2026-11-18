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

## Dev module "DEVBENCH" (standing permissions, owner decision 2026-11-17)

A second Louder-ESP32 on `COM13` of the owner's Windows PC (MAC `ac:15:18:fd:93:9c`) is the shared test board. Details are in `.agents/knowledge/DEVICE_STATE.md`. None of this applies to BASEMENT.

- Agents may reset and flash DEVBENCH without asking each time, one agent at a time, and only through `rust/scripts/devbench.py` (`acquire`, `flash`, `select-ota0`, `release`). At most two flash attempts per check.
- The helper writes only the app in `ota_0` and the `ota_0` select sector at `0xD000`. Never write the bootloader, partition table, recovery, `nvs` or `settings`.
- Build its image with `SL_DEVICE_NAME=DEVBENCH`. Never type, print or commit Wi-Fi credentials.
- Agents working a work order from `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md` may push their own branch and open a pull request against `MK5-Rust`. They never push to `MK5-Rust` and never merge.

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
