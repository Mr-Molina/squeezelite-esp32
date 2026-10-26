# Work Order: WO-004 — Rule 50 Authoritative Upstream Rust Crate & Toolchain Version Audit

**Work Order ID:** WO-004  
**Timestamp:** 2026-10-20T20:22:30-04:00  
**Assigned Subagent Role:** Reconnaissance Lookout (Rust Crate & Toolchain Currency)  
**Target Files:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-004-rust-crate-currency.md`  
**Backup Files:** `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-004-rust-crate-currency.md.bak`  
**Defect IDs:** N/A  
**Task Goal:** Verify installed vs authoritative upstream latest versions for all Rust dependencies and Espressif toolchains in `rust/` per Rule 50.  

---

## 1. Context & Scope
- **Problem Statement:** Per Rule 50 (Mandatory Upstream Ground-Truth Verification), evaluating whether any dependency or toolchain in `MK5-Rust` requires upgrading or has upstream patches requires querying authoritative external sources (`crates.io`, GitHub releases) and recording the 3-part Rule 50 Evidence Schema.
- **Single Variable Constraint (Rule 5):** Read-only analysis of repository manifests; only modify `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-004-rust-crate-currency.md` to record fulfillment.
- **Pre-Modification Integrity (Rule 31):** Do not modify any `Cargo.toml` or `Cargo.lock` files.
- **Prohibited Actions (Rule 23, Rule 24):** Do NOT use `sed`, `awk`, or `cat` for file modifications. Use strict absolute paths.
- **Verification Requirement (Rule 21, Rule 50, Rule 52):** Every dependency row MUST include: (1) Installed local version string (`Cargo.toml` & `Cargo.lock`), (2) Authoritative upstream latest version string and release timestamp, and (3) Exact URL or command used to query upstream truth.

---

## 2. Hypothesis & Predicted Outcome (Rule 1, Rule 6)
- **Hypothesis:** One or more crates in `rust/Cargo.toml` or `rust/crates/*/Cargo.toml` (`heapless`, `thiserror`, `proptest`, `embedded-io`, `esp-hal`, `esp-backtrace`, `esp-println`, `esp-storage`, `claxon`, `lewton`, `nanomp3-core`, `syom`) have newer upstream versions or patches on `crates.io`.
- **Predicted Outcome:** Complete Rule 50 compliance table for all direct dependencies in `rust/` with upgrade/compatibility recommendations for `no_std` Xtensa ESP32/ESP32-S3 targets.
- **Falsification Criteria:** Every single crate in `rust/Cargo.toml` is already at the exact latest version on `crates.io`.

---

## 3. Detailed Instructions
1. Read `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`, `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.lock`, and every `Cargo.toml` under `/home/console-admin/Github/squeezelite-esp32/rust/crates/*/`.
2. Query `crates.io` or official GitHub releases via `search_web` for each direct dependency (`heapless`, `embedded-hal`, `embedded-io`, `thiserror`, `log`, `proptest`, `claxon`, `lewton`, `ogg`, `nanomp3-core`, `syom`, `esp-hal`, `esp-backtrace`, `esp-println`, `esp-storage`).
3. Also check the local Rust / Cargo / `espup` toolchain state on this Linux host (e.g., `rustc --version`, `cargo --version`, `rustup toolchain list`) so we know whether the `esp` Xtensa toolchain is installed on this Linux host or needs setup.
4. Append the complete Rule 50 Evidence Table and Fulfillment Section to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-004-rust-crate-currency.md` using `edit_file`.

---

## 4. Rule 50 Authoritative Upstream Rust Crate & Toolchain Evidence Table

### 4.1 Host & Target Toolchain Currency (`Rule 50`)

| Component / Binary | Installed Local Version & State | Authoritative Upstream Latest & Release Timestamp | Exact Query / Source Used | Target / `no_std` Compatibility Assessment |
| :--- | :--- | :--- | :--- | :--- |
| **`rustc` (Host Stable)** | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` (`/home/console-admin/.cargo/bin/rustc`) | `1.99.1` (Released `2026-10-13`) | `/home/console-admin/.cargo/bin/rustc --version` & `rustup toolchain list` | **[CURRENT]** Active default `stable-x86_64-unknown-linux-gnu` toolchain; passes all 206 workspace unit/fuzz/stress tests. |
| **`cargo` (Host Stable)** | `cargo 1.99.1 (5f94df478 2026-08-27)` (`/home/console-admin/.cargo/bin/cargo`) | `1.99.1` (Released `2026-10-13`, build `2026-08-27`) | `/home/console-admin/.cargo/bin/cargo --version` | **[CURRENT]** Supports Cargo `resolver = "2"` and lockfile v4 (`Cargo.lock`). |
| **Espressif Xtensa Toolchain (`esp` / `esp-rs/rust-build`)** | **NOT INSTALLED** (`espup` / `espflash` absent from `/home/console-admin/.cargo/bin`; `esp` absent from `rustup toolchain list` and `/home/console-admin/.rustup/toolchains/esp`) | `v1.99.0.0` (Rust `1.99`, `esp-rs/rust-build` 2026 release) | `search_web(domain="github.com", query="\"esp-rs/rust-build\" latest release version xtensa rust toolchain 2026")` -> `https://github.com/esp-rs/rust-build/releases` | **[MISSING ON HOST]** Required for cross-compiling `xtensa-esp32-none-elf` and `xtensa-esp32s3-none-elf` bare-metal targets. Install via `cargo install espup espflash && espup install --toolchain-version 1.99.0.0`. |

---

### 4.2 Direct & Transitive Crate Dependency Currency (`Rule 50`)

| Crate Name | Declared in Manifest (`Cargo.toml`) | Resolved in `Cargo.lock` | Authoritative Upstream Latest & Release Date | Exact Query / Source URL (`Rule 33` `search_web`) | `no_std` / Xtensa Upgrade Compatibility Assessment |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`heapless`** | `0.8` (`rust/Cargo.toml` workspace) | `0.8.0` (direct) **AND** `0.9.3` (transitive via `esp-backtrace 0.20.0`, `embassy-sync 0.8.0`) | **`0.9.3`** (`2026-04-30`) | `search_web(domain="crates.io", query="\"heapless\" \"0.9.3\" \"embedded-io\" \"0.7.1\" exact release date crates.io")` (`https://crates.io/crates/heapless`) | **[UPGRADE AVAILABLE — MINOR BREAKING]** `Cargo.lock` currently duplicates `heapless` (`0.8.0` and `0.9.3`). Upgrading workspace `heapless` to `0.9.3` unifies the dependency graph with `esp-backtrace 0.20.0` and `embassy-sync 0.8.0`, Fully `no_std` compatible; note minor API changes in `Vec`/`String` capacity generics if any. |
| **`embedded-hal`** | `1.0` (`rust/Cargo.toml` workspace) | `1.0.0` (direct) & `0.2.7` (transitive via `embedded-hal-mock`) | **`1.0.0`** (`2024-01-09`) | `search_web(domain="crates.io", query="\"heapless\" \"embedded-hal\" \"embedded-io\" crates.io latest version release date")` (`https://crates.io/crates/embedded-hal`) | **[CURRENT]** At latest stable 1.0 release. Core `no_std` trait foundation for `squeezelite-tas58xx`, `louder-esp32-app`, and `cyd-controller-app`. |
| **`embedded-io`** | `0.6` (`rust/Cargo.toml` workspace) | `0.6.1` **AND** `0.7.1` (both transitive via `esp-hal 1.2.2`) | **`0.7.1`** (`2025-10-01`) | `search_web(domain="crates.io", query="\"heapless\" \"0.9.3\" \"embedded-io\" \"0.7.1\" exact release date crates.io")` (`https://crates.io/crates/embedded-io`) | **[UPGRADE AVAILABLE]** Declared in `[workspace.dependencies]` as `"0.6"`, though not currently imported by any workspace crate `Cargo.toml`. Upgrading workspace declaration to `"0.7"` (`0.7.1`) aligns with `esp-hal 1.2.2` and `embassy-sync 0.8.0`. Fully `no_std`. |
| **`thiserror`** | `1.0` (`default-features = false` in `rust/Cargo.toml`, optional in `squeezelite-decoders`) | `1.0.69` (direct) **AND** `2.0.21` (transitive via `defmt-parser 1.0.0`) | **`2.0.21`** (`2026-09-23`) | `search_web(domain="crates.io", query="\"thiserror\" \"log\" \"proptest\" \"bytemuck\" \"embedded-hal-mock\" crates.io latest version release date")` (`https://crates.io/crates/thiserror`) | **[UPGRADE RECOMMENDED FOR `no_std`]** `thiserror 1.0.69` requires `std` (`std::error::Error`), forcing `squeezelite-decoders` to gate `thiserror` behind the `std` feature. `thiserror 2.0.21` supports `#![no_std]` via `core::error::Error` (with `default-features = false`) and is already present in `Cargo.lock`. |
| **`log`** | `0.4` (`default-features = false` in `rust/Cargo.toml`) | `0.4.34` | **`0.4.34`** (`2026-08-22`) | `search_web(domain="crates.io", query="\"log\" \"0.4.34\" OR \"proptest\" \"1.12.0\" crates.io release date")` (`https://crates.io/crates/log`) | **[CURRENT]** Resolved lockfile version `0.4.34` matches latest upstream release. Fully `no_std` compatible with `default-features = false`. |
| **`proptest`** | `1.5` (`rust/Cargo.toml` workspace dev-dependency) | `1.12.0` | **`1.12.0`** (`2026-10-10`) | `search_web(domain="crates.io", query="\"log\" \"0.4.34\" OR \"proptest\" \"1.12.0\" crates.io release date")` (`https://crates.io/crates/proptest`) | **[CURRENT IN LOCKFILE]** `Cargo.lock` already resolves to latest upstream `1.12.0` (`Cargo.toml` minimum bound `"1.5"` can optionally be bumped to `"1.12"`). Host-only `[dev-dependencies]`. |
| **`claxon`** | `0.4` (`default-features = false, optional = true` in `squeezelite-decoders`) | `0.4.3` | **`0.4.3`** (`2020-08-09`) | `search_web(domain="crates.io", query="\"claxon\" \"lewton\" \"ogg\" \"nanomp3-core\" \"syom\" crates.io latest version release date")` (`https://crates.io/crates/claxon`) | **[CURRENT — `std` ONLY]** At latest upstream version (`0.4.3`), but depends on `std::io::Read`. Currently gated behind `squeezelite-decoders` `std` feature; bare-metal `no_std` FLAC decoding on Xtensa requires either a `no_std` FLAC reader or `esp-idf-sys`/`alloc` shim. |
| **`lewton`** | `0.10` (`default-features = false, features = ["ogg"], optional = true` in `squeezelite-decoders`) | `0.10.2` | **`0.10.2`** (`2021-01-20`) | `search_web(domain="crates.io", query="\"claxon\" \"lewton\" \"ogg\" \"nanomp3-core\" \"syom\" crates.io latest version release date")` (`https://crates.io/crates/lewton`) | **[CURRENT — `std` ONLY]** At latest upstream version (`0.10.2`), gated behind `std` feature in `squeezelite-decoders`. |
| **`ogg`** | Transitive via `lewton 0.10.2` (`features = ["ogg"]`) | `0.8.0` | **`0.9.2`** (`2025-01-12`) | `search_web(domain="crates.io", query="\"claxon\" \"lewton\" \"ogg\" \"nanomp3-core\" \"syom\" crates.io latest version release date")` (`https://crates.io/crates/ogg`) | **[PINNED BY `lewton`]** Upstream `ogg` is at `0.9.2`, but `lewton 0.10.2` pins `ogg = ^0.8.0`. Cannot be upgraded independently without replacing or patching `lewton`. |
| **`nanomp3-core`** | `0.2` (`squeezelite-decoders/Cargo.toml`) | `0.2.0` | **`0.2.0`** (`2026-09-30`) | `search_web(domain="crates.io", query="\"claxon\" \"lewton\" \"ogg\" \"nanomp3-core\" \"syom\" crates.io latest version release date")` (`https://crates.io/crates/nanomp3-core`) | **[CURRENT]** At latest upstream version (`0.2.0`). Pure-Rust, allocation-free, `#![no_std]` MP3 decoder compatible with Xtensa ESP32/ESP32-S3. |
| **`syom`** | `0.7` (`optional = true` in `squeezelite-decoders/Cargo.toml`) | `0.7.1` | **`0.7.1`** (`2026-10-02`) | `search_web(domain="crates.io", query="\"claxon\" \"lewton\" \"ogg\" \"nanomp3-core\" \"syom\" crates.io latest version release date")` (`https://crates.io/crates/syom`) | **[CURRENT — `std` GATED]** Resolved `0.7.1` is at latest upstream version (`2026-10-02`). Currently gated behind `std` feature in `squeezelite-decoders`. |
| **`esp-hal`** | `1.2.2` (`cyd-controller-app/Cargo.toml` under `cfg(target_arch = "xtensa")`) | `1.2.2` | **`1.2.2`** (`2026-09-18`) | `search_web(domain="crates.io", query="\"esp-hal\" \"esp-backtrace\" \"esp-println\" \"esp-storage\" \"esp-bootloader-esp-idf\" crates.io latest version release date")` (`https://crates.io/crates/esp-hal`) | **[CURRENT]** At latest upstream release (`1.2.2`). Used in `cyd-controller-app`; not yet added to `louder-esp32-app/Cargo.toml` `[target.'cfg(target_arch = "xtensa")'.dependencies]`. |
| **`esp-backtrace`** | `0.20.0` (`cyd-controller-app/Cargo.toml` under `cfg(target_arch = "xtensa")`) | `0.20.0` | **`0.20.0`** (`2026-08-26`) | `search_web(domain="crates.io", query="\"esp-hal\" \"esp-backtrace\" \"esp-println\" \"esp-storage\" \"esp-bootloader-esp-idf\" crates.io latest version release date")` (`https://crates.io/crates/esp-backtrace`) | **[CURRENT]** At latest upstream release (`0.20.0`). Pulls in `heapless 0.9.3`. |
| **`esp-println`** | `0.18.0` (`cyd-controller-app/Cargo.toml` under `cfg(target_arch = "xtensa")`) | `0.18.0` | **`0.18.0`** (`2026-08-26`) | `search_web(domain="crates.io", query="\"esp-hal\" \"esp-backtrace\" \"esp-println\" \"esp-storage\" \"esp-bootloader-esp-idf\" crates.io latest version release date")` (`https://crates.io/crates/esp-println`) | **[CURRENT]** At latest upstream release (`0.18.0`). |
| **`esp-storage`** | Transitive via `esp-bootloader-esp-idf 0.6.0` | `0.10.0` | **`0.10.0`** (`2026-08-26`) | `search_web(domain="crates.io", query="\"esp-hal\" \"esp-backtrace\" \"esp-println\" \"esp-storage\" \"esp-bootloader-esp-idf\" crates.io latest version release date")` (`https://crates.io/crates/esp-storage`) | **[CURRENT]** At latest upstream release (`0.10.0`). Can be added as a direct `xtensa` dependency in `louder-esp32-app` to back `NvsFlashBackend` with hardware NOR flash (`embedded-storage`). |
| **`esp-bootloader-esp-idf`** | `0.6.0` (`cyd-controller-app/Cargo.toml` under `cfg(target_arch = "xtensa")`) | `0.6.0` | **`0.6.0`** (`2026-08-26`) | `search_web(domain="crates.io", query="\"esp-hal\" \"esp-backtrace\" \"esp-println\" \"esp-storage\" \"esp-bootloader-esp-idf\" crates.io latest version release date")` (`https://crates.io/crates/esp-bootloader-esp-idf`) | **[CURRENT]** At latest upstream release (`0.6.0`). |
| **`bytemuck`** | `1.25` (`louder-esp32-app/Cargo.toml`) | `1.25.2` | **`1.25.2`** (`2026-07-19`) | `search_web(domain="crates.io", query="\"thiserror\" \"log\" \"proptest\" \"bytemuck\" \"embedded-hal-mock\" crates.io latest version release date")` (`https://crates.io/crates/bytemuck`) | **[CURRENT]** Resolved `1.25.2` is at latest upstream release. Fully `no_std` compatible. |
| **`embedded-hal-mock`** | `0.11` (`squeezelite-tas58xx` & `louder-esp32-app` dev-dependencies) | `0.11.1` | **`0.11.1`** (`2024-06-02`) | `search_web(domain="crates.io", query="\"thiserror\" \"log\" \"proptest\" \"bytemuck\" \"embedded-hal-mock\" crates.io latest version release date")` (`https://crates.io/crates/embedded-hal-mock`) | **[CURRENT]** Resolved `0.11.1` is at latest upstream release (`eh1` feature enabled). Host-only `[dev-dependencies]`. |

---

### 4.3 Key Architectural & Dependency Audit Findings
1. **Duplicate `heapless` (`0.8.0` vs `0.9.3`) & `embedded-io` (`0.6.1` vs `0.7.1`) in `Cargo.lock`:**
   - `[workspace.dependencies]` in `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml` pins `heapless = "0.8"` (`0.8.0`) and `embedded-io = "0.6"` (`0.6.1`), whereas `esp-hal 1.2.2` and `esp-backtrace 0.20.0` depend on `heapless 0.9.3` and `embedded-io 0.7.1`. Upgrading workspace `heapless` to `"0.9"` (`0.9.3`) and `embedded-io` to `"0.7"` (`0.7.1`) eliminates duplicate crate versions in `Cargo.lock`.
2. **`thiserror` `1.0.69` vs `2.0.21` (`#![no_std]` Compatibility):**
   - `[workspace.dependencies]` pins `thiserror = { version = "1.0", default-features = false }` (`1.0.69`), while `defmt-parser 1.0.0` already pulls `thiserror 2.0.21` into `Cargo.lock`. Upgrading workspace `thiserror` to `2.0` (`2.0.21`) enables `#![no_std]` error derives via `core::error::Error`.
3. **Missing Local Xtensa Toolchain (`espup` / `esp`) & `louder-esp32-app` Bare-Metal Target Wiring:**
   - While `cyd-controller-app` has `[target.'cfg(target_arch = "xtensa")'.dependencies]` configured with current `esp-hal 1.2.2`, `esp-println 0.18.0`, `esp-backtrace 0.20.0`, and `esp-bootloader-esp-idf 0.6.0`, `louder-esp32-app` currently lacks `xtensa` HAL dependencies in `Cargo.toml` and uses `std` (`println!`, `vec!`) in `src/main.rs`.
   - Furthermore, the Linux host currently has only `stable`/`nightly`/`1.88`/`1.92` (`x86_64-unknown-linux-gnu`) installed; `espup` (`v1.99.0.0` toolchain) is not yet installed on this host.

---

## 5. Subagent Fulfillment Section (Rule 15, Rule 17)
- **Execution Timestamp:** 2026-10-20T20:26:30-04:00
- **Files Modified / Created (Absolute Paths):**
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-004-rust-crate-currency.md` (Modified: appended Section 4 Rule 50 Evidence Table and completed Section 5 Fulfillment Section)
  - `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-004-rust-crate-currency.md.bak` (Created/Verified backup prior to edit per Rule 35)
- **Backup Verification:** Verified `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/WO-004-rust-crate-currency.md.bak` exists (`BACKUP_VERIFIED`, exit code `0`). Zero repository source/manifest files (`Cargo.toml`, `Cargo.lock`) were modified (`Rule 31`).
- **Test / Verification Command & Results:**
  - `/home/console-admin/.cargo/bin/rustc --version` -> `rustc 1.99.1 (58cf8f9c3 2026-10-13)` (exit code `0`)
  - `/home/console-admin/.cargo/bin/cargo --version` -> `cargo 1.99.1 (5f94df478 2026-08-27)` (exit code `0`)
  - `/home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace` -> **206 passed, 0 failed** across all 8 workspace crates (exit code `0`).
- **Observed Outcome vs Predicted Outcome:** Hypothesis confirmed. While most resolved crates in `Cargo.lock` (`embedded-hal 1.0.0`, `log 0.4.34`, `proptest 1.12.0`, `claxon 0.4.3`, `lewton 0.10.2`, `nanomp3-core 0.2.0`, `syom 0.7.1`, `bytemuck 1.25.2`, `embedded-hal-mock 0.11.1`, `esp-hal 1.2.2`, `esp-backtrace 0.20.0`, `esp-println 0.18.0`, `esp-storage 0.10.0`, `esp-bootloader-esp-idf 0.6.0`) match authoritative upstream latest releases, workspace direct dependencies `heapless = "0.8"` (`0.8.0` vs upstream `0.9.3`), `embedded-io = "0.6"` (`0.6.1` vs upstream `0.7.1`), and `thiserror = "1.0"` (`1.0.69` vs upstream `2.0.21`) lag behind newer upstream releases that are already pulled transitively into `Cargo.lock` by `esp-hal`/`esp-backtrace`/`defmt-parser`. Additionally, `ogg 0.8.0` lags upstream `0.9.2` due to `lewton 0.10.2`'s version pin, and the `esp` Xtensa Rust toolchain (`v1.99.0.0`) is not yet installed on this Linux host.
- **Fulfillment Status:** `[EXECUTED & VERIFIED]`
- **Summary Path:** N/A (Read-Only repository audit)
- **Handoff Path:** `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-004-rust-crate-currency.md`
