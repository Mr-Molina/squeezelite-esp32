# System State Ledger (`MK5-Rust`)

- **Host Machine**: `pc-1` (Dell Precision Tower 3431)
- **Host OS**: Linux `x86_64` (Debian GNU/Linux 13 trixie/sid, kernel `6.12.113+deb13-amd64`)
- **Workspace Directory**: `/home/console-admin/Github/squeezelite-esp32`
- **Active Git Branch**: `MK5-Rust` (commit `579ffcde`, ahead 3 of `origin/MK5-Rust` at `b24f937a`)
- **Verified Code Baseline Commit**: `579ffcde464258064fb2670132f5de1098b46601` (Wave 3 `louder-esp32-app` T14–T16, Xtensa clippy cleanup, `const TE0` in flash `.rodata`, `#![forbid(unsafe_code)]` restored; `859 passed, 0 failed` host tests, `0` Clippy + GRS warnings)
- **Recent Wave & Housekeeping Commits**:
  - `5e6c042f`: Governance & repository housekeeping
  - `3f838ecb` / `b24f937a`: Wakeup/CVE/context sync (`origin/MK5-Rust`)
  - `c186c659`: Wave 1 (`T04`, `T05` decoder check, `T19`, `T23`, `T24`, `T25`; `845` host tests)
  - `f7b58cf0`: Wave 2 (`T05` coordinator pump test, `T17`, `T18`, `T20`, `T21` cspot, `T26` later amended by `579ffcde`, `T27`; `851` host tests)
  - `579ffcde` (`HEAD`): Wave 3 (`T14`, `T15`, `T16`, Xtensa clippy cleanup, `const TE0` in flash `.rodata`, `#![forbid(unsafe_code)]`; `859` host tests)

## Compilers & Toolchains
- **Host Rust Toolchain**: Stable/nightly `rustc 1.99.1` (`cargo test --workspace`)
- **Xtensa Bare-Metal Toolchain**: `+esp` (`rustc 1.97.0-nightly (1.97.0.0)` for target `xtensa-esp32-none-elf`, environment initialized via `/home/console-admin/export-esp.sh`)
- **Flash & Image Utilities**: `espup 0.18.0`, `espflash 4.6.0`, `cargo-espflash 4.6.0`, `esptool.py v4.12.0`

## Optimization & Caching Infrastructure (TokenEconomizer)
- **TokenEconomizer Status**: Active at `/home/console-admin/.tokeneconomizer/repos/squeezelite-esp32-949df7c471`
  - `repomap=on`: Live `repomap_python` at `/home/console-admin/.local/share/tokeneconomizer/venv/bin/python` (`1615` tokens cached for `3f838ecb99`)
  - `loopguard=nudge`: Envelope calibrated to `20 tool calls (60 ended-green turns)`
  - `microskills=envfacts`

## Rust Crate Topology
- **Workspace Crates (10)**:
  - `squeezelite-proto`
  - `squeezelite-audio-core`
  - `squeezelite-tas58xx`
  - `squeezelite-decoders`
  - `squeezelite-cyd-link`
  - `squeezelite-config`
  - `louder-esp32-app`
  - `cyd-controller-app`
  - `squeezelite-raop`
  - `squeezelite-cspot`
- **Standalone Crates**:
  - `nanomp3-core`
- **Vendored Crates (5)**:
  - `esp-radio`
  - `syom`
  - `lewton`
  - `alac`
  - `opus-rs`

## Active Customizations & Environment
- **Enabled Plugins (16)**: Chrome DevTools, Rust Pro, TokenEconomizer, Superpowers, GCalendar, GDocs, GDrive, GSheets, GSlides, Gemini API, Gemini Media Studio, Android CLI, etc.
- **MCP Servers (10)**: `blender`, `chrome_devtools`, `freecad`, `gemini-api_gemini-api-docs`, `hi3d`, `inkmcp`, `obsidian_vault`, `rea`, `spotify`, etc.
- **Skills & Personas**:
  - Workspace `.agents/skills.json` + `deep-code-analysis`
  - Cognitive personas loaded via Obsidian vault

## Verification Status
- `859 passed, 0 failed` host workspace tests passing (`cargo test --workspace --all-features --no-fail-fast`; Wave 1 `c186c659`: `845`, Wave 2 `f7b58cf0`: `851`, Wave 3 `579ffcde`: `859`)
- `0` Clippy warnings on host and 10-crate Xtensa target (`cargo +esp clippy --workspace --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings`)
- `0` Google Rust Audit errors/warnings (`google_rust_audit.py --strict`: `0 errors, 0 warnings`)
- `esp-rtos-core1` feature check passing (`louder-esp32-app` `xtensa-esp32-none-elf`)
- Release ELF section telemetry (`target/xtensa-esp32-none-elf/release/` at `579ffcde`):
  - `louder-esp32-app`: IRAM `129636` B, `.dram2_uninit` `98520` B, `.data.wifi` `3400` B, `.stack` `28528` B
  - `cyd-controller-app`: IRAM `18236` B, `.stack` `188856` B
- DRAM0 / Rule 64 baseline note: `louder-esp32-app` reflects the pre-existing `e6488153` Rule 64 DRAM0 baseline (`.stack = 28528` B) with **0-byte Wave 1–3 DRAM regression** (`const TE0` placed in flash `.rodata` in `579ffcde` and `#![forbid(unsafe_code)]` restored in `squeezelite-cspot`).
