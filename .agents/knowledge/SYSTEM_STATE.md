# System State Ledger (`MK5-Rust`)

- **Host Machine**: `pc-1` (Dell Precision Tower 3431)
- **Host OS**: Linux `x86_64` (Debian GNU/Linux 13 trixie/sid, kernel `6.12.113+deb13-amd64`)
- **Workspace Directory**: `/home/console-admin/Github/squeezelite-esp32`
- **Active Git Branch**: `MK5-Rust` (ahead 20 of `origin/MK5-Rust` pending OAuth `workflow` scope)
- **Verified Code Baseline Commit**: `3102822a4897bd33c05724269a7e6ce70f37ef3f` (reconciled cross-OS AAC golden digest gate fix; `840 passed, 0 failed` tests, `0` Clippy + GRS warnings)
- **Recent Housekeeping Commits**:
  - `5e6c042f`: Governance & repository housekeeping
  - `3f838ecb` (`HEAD`): Active HEAD commit (`ahead 20` of origin pending OAuth `workflow` scope)

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
- `840 passed, 0 failed` workspace tests passing (`cargo test --workspace`)
- `0` Clippy warnings (`cargo clippy --workspace --all-targets -- -D warnings`)
- `0` Google Rust Audit errors/warnings (`google_rust_audit.py --strict`)
- Cleaned Rust target artifacts (`cargo clean`: 82,971 files / 22.5 GiB reclaimed)
- ELF section limits satisfied: `.stack = 37,288` B (>= 36,000 B), `.iram0.text = 126,944` B (<= 131,072 B), `.dram2_uninit = 97,560` B (<= 98,768 B).
