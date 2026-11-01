# Work Order: WO-218 — Wave 2A: Port Shared Crates, Vendored `no_std` Codecs, `squeezelite-raop`, & `squeezelite-cspot` onto `MK5-Rust` (`cd139b61`)

- **Work Order ID**: `WO-218`
- **Timestamp**: `2026-10-31T16:27:00-04:00`
- **Assigned Subagent Role**: `Rust Shared Crates & Codec Reconciler` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/` (Library Crates & Vendored Codecs)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Port and reconcile all shared library crates (`squeezelite-decoders`, `squeezelite-raop`, `squeezelite-cspot`, `squeezelite-audio-core`, `squeezelite-proto`, `squeezelite-config`, `nanomp3-core`, `squeezelite-tas58xx`, `squeezelite-cyd-link`, `cyd-controller-app`) and vendored `no_std` codec crates (`rust/vendor/{alac,lewton,opus-rs,syom}`) from safety branch `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`) onto `MK5-Rust` (`cd139b61`), preserving 100% of both the incoming Phase 0+1 features (`FadeEngine`, `GainStage`, `ChannelMode`, `truncate_unread`, `peek_at`, C-parity `HELO`/backoff/`serv`/`STAT`, staging-free journal) and the `WO-157`–`WO-214` features & `GRS-001`–`GRS-012` audit remediations.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN**:
     - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`
     - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.lock`
     - `/home/console-admin/Github/squeezelite-esp32/rust/rustfmt.toml`
     - `/home/console-admin/Github/squeezelite-esp32/rust/vendor/{alac,lewton,opus-rs,syom}/**`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/{squeezelite-raop,squeezelite-cspot,squeezelite-decoders,nanomp3-core,squeezelite-tas58xx,squeezelite-cyd-link,cyd-controller-app,squeezelite-audio-core,squeezelite-config,squeezelite-proto}/**`
   - Do **NOT** modify anything under `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/` or `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/` (owned by `WO-219` and `WO-220`).
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**: All modified and newly added `.rs` files in your owned crates (including new Phase 1 files `squeezelite-audio-core/src/{fade,volume}.rs`) MUST pass `google_rust_audit.py --strict` with `0` errors and `0` warnings.
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Execution Steps
1. **Checkout Non-Overlapping Crates & Vendor Directories from `local/wo157-wo214-codecs-airplay-spotify` (`627f740b`)**:
   - Execute:
     ```bash
     /usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 checkout local/wo157-wo214-codecs-airplay-spotify -- \
       /home/console-admin/Github/squeezelite-esp32/rust/rustfmt.toml \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/alac \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/lewton \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/opus-rs \
       /home/console-admin/Github/squeezelite-esp32/rust/vendor/syom \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-raop \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cspot \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-decoders \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/nanomp3-core \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-cyd-link \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/cyd-controller-app \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/output.rs \
       /home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/tests/proto_fuzz.rs
     ```
2. **Reconcile the 5 Shared Overlapping Files (`b49854f3..cd139b61` vs `b49854f3..627f740b`)**:
   - Inspect diffs using `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff cd139b61..local/wo157-wo214-codecs-airplay-spotify -- <path>` and `/usr/bin/git -C /home/console-admin/Github/squeezelite-esp32 diff b49854f3..cd139b61 -- <path>`:
     1. `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`: Combine workspace members (`squeezelite-raop`, `squeezelite-cspot`), `[workspace.dependencies]` (including `smoltcp` with `udp`/`dns` from `cd139b61` AND codec patches), `[workspace.lints]`, and `[patch.crates-io]` (`esp-radio`, `alac`, `lewton`, `opus-rs`, `syom`).
     2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/ringbuffer.rs` (plus `lib.rs`, `fade.rs`, `volume.rs`): Preserve Phase 1 `truncate_unread`, `peek_at`, `FadeEngine`, `GainStage`, `ChannelMode` while merging `WO-189` `GRS-001`–`GRS-012` and safety remediations. Ensure `lib.rs` obeys `GRS-003` (`mod` + `pub use` only).
     3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` and `tests/config_fuzz.rs`: Preserve Phase 0 staging-free journal view/writer and `set_player_name` alongside `WO-190` audit remediations (`GRS-001`–`GRS-012`) and reserved `"vol"` key exclusion in `config_fuzz.rs`.
     4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`: Preserve Phase 1 C-parity `HELO` capability builder, reconnect backoff, `serv` syncgroup, `STAT`/`DSCO` sequencing, and pause/skip timing while merging `WO-190` audit remediations (`GRS-001`–`GRS-012`) and ensuring `flc`, `aac`, `ogg`, `alc`, `ops` capabilities are supported.
3. **Verify Tests, Clippy & `google_rust_audit.py --strict` on Owned Crates**:
   - Run:
     ```bash
     /home/console-admin/.cargo/bin/cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml \
       -p squeezelite-decoders -p squeezelite-raop -p squeezelite-cspot \
       -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-config \
       -p nanomp3-core -p squeezelite-tas58xx -p squeezelite-cyd-link
     ```
   - Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py` with `--strict` on each owned crate's `src` directory (`squeezelite-decoders/src`, `squeezelite-raop/src`, `squeezelite-cspot/src`, `squeezelite-audio-core/src`, `squeezelite-proto/src`, `squeezelite-config/src`, `nanomp3-core/src`, `squeezelite-tas58xx/src`, `squeezelite-cyd-link/src`, `cyd-controller-app/src`) and resolve any `GRS-001`–`GRS-012` issues.
4. **Deliverables**:
   - Append Fulfillment Summary to §4 of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-218-wave2a-shared-crates-and-codecs.md`.
   - Write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo218_wave2a_shared_crates.md`.

---

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Completed At**: `2026-10-31T16:49:00-04:00`
- **Summary of Actions & Reconciliations**:
  1. **Non-Overlapping Checkout (`local/wo157-wo214-codecs-airplay-spotify` `627f740b`)**:
     - Checked out `/home/console-admin/Github/squeezelite-esp32/rust/rustfmt.toml`, vendored `no_std` codec crates `/home/console-admin/Github/squeezelite-esp32/rust/vendor/{alac,lewton,opus-rs,syom}`, crates `/home/console-admin/Github/squeezelite-esp32/rust/crates/{squeezelite-raop,squeezelite-cspot,squeezelite-decoders,nanomp3-core,squeezelite-tas58xx,squeezelite-cyd-link,cyd-controller-app}`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/{equalizer,output}.rs`, and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/tests/proto_fuzz.rs`.
  2. **Shared Overlapping File Reconciliations (`b49854f3..cd139b61` + `b49854f3..627f740b`)**:
     - `/home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml`: Combined workspace members (`squeezelite-raop`, `squeezelite-cspot`), `[workspace.dependencies]`, `[workspace.lints.rust]`, `[workspace.lints.clippy]`, and `[patch.crates-io]` (`syom`, `lewton`, `alac`, `opus-rs`, `esp-radio`, `nanomp3-core`).
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/{lib,ringbuffer,fade,volume}.rs`: Preserved Phase 1 `truncate_unread`, `peek_at`, `FadeEngine`, `FadeDirection`, `GainStage`, and `ChannelMode` while merging `WO-189` `rewind_read_internal` atomic ordering, dual `// JUSTIFICATION:` + `// SAFETY:` (`GRS-008`) comments across all `unsafe` blocks, and `GRS-012` comment/code width formatting. Confirmed `lib.rs` obeys `GRS-003`.
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs` and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/tests/config_fuzz.rs`: Preserved Phase 0 staging-free `JournalView`, `SectorWriter`, `validate_sector`, `select_dual_sector`, `set_player_name`, and `prop_assume!(k1 != "vol")` alongside `WO-190` `GRS-001`–`GRS-012` remediations (`truncate_utf8_char_boundary`, `format_u32_decimal`, `parse_mac_colon_hex`, `set_wifi_credentials_encrypted`, `decrypt_wifi_password_into`, `erase_to_factory_defaults`, `validate_eq_gains`).
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-proto/src/lib.rs`: Preserved Phase 1 `switch_server` returning `Option<([u8; 4], Option<&'a [u8]>)>`, `SYNCGROUP_ID_LEN`, `SQUEEZENETWORK_MARKER_IP`, `build_capabilities`, and `next_backoff_ms` while merging `WO-190` binary `autostart` (`1`/`3`), `build_stat_full` `frames_played / 44_100` elapsed time calculation, `build_butn`, null-terminated informational tag skipping in `DiscoveryParser::parse_response`, and `GRS-012` formatting.
- **Verification Evidence**:
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p squeezelite-raop -p squeezelite-cspot -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-config -p nanomp3-core -p squeezelite-tas58xx -p squeezelite-cyd-link`: **PASS (`exit code 0`, 0 failures across all unit, integration, equivalence, stress, and proptest fuzz suites)**.
  - `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p cyd-controller-app`: **PASS (`exit code 0`, 42/42 tests pass)**.
  - `cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-decoders -p squeezelite-raop -p squeezelite-cspot -p squeezelite-audio-core -p squeezelite-proto -p squeezelite-config -p nanomp3-core -p squeezelite-tas58xx -p squeezelite-cyd-link -p cyd-controller-app --all-targets -- -D warnings`: **PASS (`exit code 0`, 0 warnings)**.
  - `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py --strict` across all 10 owned crates' `src` directories: **PASS (`exit code 0`, `0 error(s), 0 warning(s)`)**.
  - Scope isolation verified: zero modifications under `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/` or `/home/console-admin/Github/squeezelite-esp32/rust/vendor/esp-radio/`.
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo218_wave2a_shared_crates.md`
