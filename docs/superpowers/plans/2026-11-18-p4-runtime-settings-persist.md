# P4 Runtime Settings Persistence (`feat/runtime-settings-persist`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make runtime settings flash persistence playback-safe (never erasing or writing NOR flash while audio is actively playing), stabilize dual-sector A/B ping-pong persistence (`0x3FE000` / `0x3FF000`) with CRC/sequence validation and brownout-safe write ordering, promote `runtime-flash-persist` cleanly, and verify settings survival and zero-underrun playback deferral on `DEVBENCH`.

**Architecture:** Anchored to the F4 `DeviceService` seam in `rust/crates/louder-esp32-app/src/services/mod.rs` and `SettingsService` (`src/services/settings.rs`). `SettingsService` remains the `100 ms` (`SETTINGS_POLL_INTERVAL_MS`) platform-free scheduler and adds a host-testable playback-deferral gate via `ServiceAudioHost::is_playing()` / `ServiceCtx::is_audio_playing()`. Hardware NOR flash erase/write, sector verification, and `Core1Ctl::park_for_flash_write()` / `unpark_after_flash_write()` stay isolated inside `DeviceSettingsPersistHost` in `src/main.rs`, keeping `config_handle.rs`, `coordinator.rs`, `pipeline.rs`, and `services/*.rs` free of Xtensa HAL or `target_arch` code (`tests/no_cfg_invariant.rs`). Zero new static buffers, zero IRAM placements, and zero sockets are added.

**Tech Stack:** Rust `no_std` (`esp-hal`, `esp-storage`, `squeezelite-config::FlashJournal`), host toolchain `1.92.0`, Espressif `esp` toolchain `1.97.0.0`, `devbench.py` hardware lock + flash helper, `check_section_budget.py`, `google_rust_audit.py`.

**Branch & Worktree Isolation:**
- Branch: `feat/runtime-settings-persist` created from post-F4 `MK5-Rust` (`foundation/service-seams` merged).
- Worktree: isolated git worktree on `MK5-Rust`; rebase onto `MK5-Rust` before the `DEVBENCH` hardware check and again before opening the PR (`gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust`).
- Never push directly to `MK5-Rust` and never merge.

**Hardware Lock & Safety Rules:**
- Acquire lock before any `DEVBENCH` flash/reset: `python rust/scripts/devbench.py acquire feat/runtime-settings-persist` (release with `release` when done).
- Pre-flash MAC check: `espflash board-info --port COM13` must match `DEVBENCH_MAC` (`ac:15:18:fd:93:9c`). Never flash `c8:2e:18:be:b8:34` (`BASEMENT`) or `d4:d4:da:5a:e7:3c` (`CYD`).
- Never touch `BASEMENT` (`172.22.1.15`), `/dev/ttyUSB0` on `pc-1`, or `POST /reboot_ota.json`.
- At most 2 flash attempts per check (enforced by `devbench.py flash`). Build with `SL_DEVICE_NAME=DEVBENCH`.
- **NVS / Flash Safety**: Only journal sectors `0x3FE000` (Slot A) and `0x3FF000` (Slot B) at `SETTINGS_JOURNAL_OFFSET` (`0x003F_E000`) are written. Never touch `nvs` (`0x9000`), `otadata` (`0xD000`) outside Gate 1, `recovery` (`0x10000`), or lower C `settings` NVS pages (`0x3F0000..0x3FE000`).

**Memory & Socket Budget Gates (P4 Allowance = `0 B` DRAM, `0 B` IRAM, `0` Sockets):**
- Sockets: `0` added (stays at `8 / 16` `SOCKET_SLOTS`).
- `IRAM` (`.vectors + .rwtext + .rwtext.wifi`): `<= 119,976 B` (Hard Invariant 48: `<= 131,072 B`)
- `.dram2_uninit`: `<= 89,048 B` (Hard Invariant 48: `<= 98,768 B`)
- `.stack`: `>= 41,256 B` (Hard Invariant 48: `>= 28,000 B`)
- Internal heap (`bring_up::HEAP + STATIC_BT_HEAP`): `>= 65,536 B`

---

### Task 1: Expose `is_playing` on `ServiceAudioHost` & `ServiceCtx` (Playback Guard Seam)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/services/mod.rs` (trait `ServiceAudioHost`, `ServiceAudioAdapter`, `ServiceCtx`, and `services::mod` unit tests)

**Context:** Currently `ServiceAudioHost` (`src/services/mod.rs:139-170`) exposes `active_source(&self) -> AudioSource` and `has_external_audio_queued(&self) -> bool`, while `main.rs:3975` checks active playback via `s.coordinator.state == PlayerState::Playing || s.coordinator.external_playback`. Adding `is_playing(&self) -> bool` on `ServiceAudioHost` and `ServiceCtx::is_audio_playing(&self) -> bool` lets `SettingsService` defer NOR flash writes whenever any audio source (LMS SlimProto, AirPlay, Bluetooth, or Spotify) is actively playing, without `services/settings.rs` touching `PlayerCoordinator` directly.

- [ ] **Step 1: Write failing host test in `src/services/mod.rs`**

Add a unit test `service_ctx_reports_audio_playing_state` in `src/services/mod.rs` (`mod tests`) verifying:
1. When no `ServiceAudioHost` is attached (`ServiceCtx::new(...)`), `ctx.is_audio_playing()` returns `false`.
2. When `ServiceAudioAdapter` wraps a `PlayerCoordinator` with `PlayerState::Playing` or `external_playback == true`, `ctx.is_audio_playing()` returns `true`.
3. When `PlayerState::Paused` / `PlayerState::Stopped` and `external_playback == false`, `ctx.is_audio_playing()` returns `false`.

- [ ] **Step 2: Run the test and confirm it fails to compile**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::tests::service_ctx_reports_audio_playing_state
```
Expected: FAIL (`no method named 'is_audio_playing' found for struct 'ServiceCtx'`).

- [ ] **Step 3: Implement `ServiceAudioHost::is_playing` and `ServiceCtx::is_audio_playing`**

In `rust/crates/louder-esp32-app/src/services/mod.rs`:
1. Add to `pub trait ServiceAudioHost`:
   ```rust
   /// Returns `true` while SlimProto or an external audio source is actively playing.
   fn is_playing(&self) -> bool;
   ```
2. Implement in `ServiceAudioAdapter`:
   ```rust
   fn is_playing(&self) -> bool {
       self.coordinator.state == crate::coordinator::PlayerState::Playing
           || self.coordinator.external_playback
   }
   ```
3. Add to `ServiceCtx<'a>`:
   ```rust
   /// Returns `true` when an attached audio host is actively playing.
   #[must_use]
   pub fn is_audio_playing(&self) -> bool {
       self.audio
           .as_deref()
           .is_some_and(ServiceAudioHost::is_playing)
   }
   ```
4. Implement `fn is_playing(&self) -> bool` on any mock `ServiceAudioHost` structs in `src/services/mod.rs`, `src/services/airplay.rs`, and `src/services/spotify_facade.rs` tests.

- [ ] **Step 4: Run tests and strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app --lib services
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/mod.rs rust/crates/louder-esp32-app/src/services/airplay.rs rust/crates/louder-esp32-app/src/services/spotify_facade.rs
git commit -m "feat(services): expose ServiceAudioHost::is_playing on ServiceCtx for playback-safe persistence"
```

---

### Task 2: Playback-Guarded Deferral in `SettingsService` (`src/services/settings.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/services/settings.rs`

**Context:** Today `SettingsService::poll` (`src/services/settings.rs:60-70`) calls `ctx.poll_settings_persist()` unconditionally every `100 ms` (`SETTINGS_POLL_INTERVAL_MS`), so `ConfigWriter::poll` commits and erases/writes flash even mid-track. We must defer calling `ctx.poll_settings_persist()` whenever `ctx.is_audio_playing()` is `true`, track deferral state (`deferred_while_playing` / `deferred_count`) for `/status.json` (`StatusExtras`), and emit a single edge-triggered syslog line on deferral entry/exit without flooding the syslog ring.

- [ ] **Step 1: Write failing host unit tests in `src/services/settings.rs`**

Add tests in `src/services/settings.rs`:
1. `settings_service_defers_persist_while_audio_playing`:
   - Attach a mock `ServiceAudioHost` with `playing = true` and `MockSettingsPersistHost` returning `true`.
   - Call `svc.poll(&mut ctx)` at `t = 100, 200, 300 ms`.
   - Assert `host.calls.is_empty()`, `svc.persist_count() == 0`, `svc.is_deferred() == true`, and `StatusExtras` reports `settings_deferred == true` (`StatusValue::Bool(true)`).
2. `settings_service_flushes_promptly_after_playback_stops`:
   - Continue from `playing = true` at `t = 300 ms` to `playing = false` at `t = 400 ms`.
   - Assert `host.calls == vec![400]`, `svc.persist_count() == 1`, `svc.is_deferred() == false`, and `StatusExtras` reports `settings_writes == 1` and `settings_deferred == false`.
3. `settings_service_keeps_100ms_throttle_after_deferral_clears`:
   - Verify polling at `t = 450 ms` (`< 100 ms` after `400 ms`) remains throttled (`host.calls == vec![400]`).

- [ ] **Step 2: Run the new tests and observe failure**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::settings::tests
```
Expected: FAIL (`no method named 'is_deferred'`, `host.calls` non-empty while playing).

- [ ] **Step 3: Implement playback deferral and status reporting in `SettingsService`**

In `rust/crates/louder-esp32-app/src/services/settings.rs`:
1. Add `deferred_while_playing: bool` and `deferred_polls: u32` fields to `SettingsService` (all primitive fields, zero static RAM).
2. Add `pub const fn is_deferred(&self) -> bool` and `pub const fn deferred_polls(&self) -> u32` getters (no `get_` prefix per `GRS-011`).
3. Update `DeviceService::poll(&mut self, ctx: &mut ServiceCtx<'_>)`:
   - Keep the `now_ms.saturating_sub(self.last_poll_ms) >= SETTINGS_POLL_INTERVAL_MS` check.
   - Advance `self.last_poll_ms = now_ms`.
   - If `ctx.is_audio_playing()`:
     - If `!self.deferred_while_playing && ctx.config().is_dirty()`, log one edge-triggered notice via `crate::http_control::push_runtime_syslog("INFO", "MESSAGING_CLASS_SYSTEM", "settings: flash persist deferred while audio playing", now_ms)` (or guard so it logs once per playback-deferral episode, never every 100 ms!).
     - Set `self.deferred_while_playing = true`, increment `self.deferred_polls` (`saturating_add(1)`), and return without calling `ctx.poll_settings_persist()`.
   - Otherwise:
     - Clear `self.deferred_while_playing = false`.
     - Call `let persisted = ctx.poll_settings_persist();` and update `last_persisted` / `persist_count` as before.
4. Update `DeviceService::status(&self, out: &mut StatusExtras)`:
   - Keep `out.push_u64("settings_writes", u64::from(self.persist_count));`
   - Append `out.push_bool("settings_deferred", self.deferred_while_playing);`
5. Update existing `settings_service_counts_persists_and_reports_status` and `settings_service_detached_host_is_noop` assertions in `settings.rs` for `extras.len() == 2` (`settings_writes` + `settings_deferred`).

- [ ] **Step 4: Run tests and strict Rust audit**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app services::settings::tests
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All `settings::tests` PASS, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/src/services/settings.rs
git commit -m "feat(settings): defer runtime flash writes while audio is playing"
```

---

### Task 3: Harden `RamJournal` & `DeviceSettingsPersistHost` (A/B Alternation, CRC/Seq Verification, Corrupt-Slot Fallback, Brownout-Safe Write)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/config_handle.rs` (if helper/validation assertions needed)
- Modify: `rust/crates/louder-esp32-app/tests/config_persistence.rs` (add corrupt-slot fallback & read-back verification tests)
- Modify: `rust/crates/louder-esp32-app/src/main.rs` (`DeviceSettingsPersistHost::poll_and_persist` + pre-reboot write)

**Context:** Inspect `main.rs:4025-4051` (`DeviceSettingsPersistHost`) and `config_handle.rs:1243-1266` (`RamJournal::commit_flash_verified`). Right now `DeviceSettingsPersistHost::poll_and_persist` erases the target sector and writes `[16..used_len]`, `[2048..4096]`, and `[..16]` (header last for brownout safety!), then calls `self.journal.mark_flash_written()` unconditionally **without**:
1. Checking that `FlashJournal::validate_sector(image.sector)` succeeds before erasing NOR flash;
2. Skipping redundant writes if the target flash sector already holds identical bytes;
3. Verifying the written NOR sector via read-back (`read_nor` + `FlashJournal::validate_sector` + byte comparison) before calling `mark_flash_written()` (so a failed/incomplete write does NOT advance `self.target` onto the healthy fallback slot!).

- [ ] **Step 1: Write failing host tests in `tests/config_persistence.rs`**

Add tests in `rust/crates/louder-esp32-app/tests/config_persistence.rs`:
1. `test_ping_pong_corrupt_higher_seq_falls_back_to_valid_slot`:
   - Write valid `state_for(1)` (`seq=1`) to slot A and corrupt `state_for(2)` (`seq=2`, flipped payload/CRC byte or torn header `[0..16] == 0xFF`) to slot B.
   - Assert `FlashJournal::select_dual_sector(&flash[0], &flash[1])` returns `Some((0, 1))` (slot A), `resolve_boot_audio` restores `state_for(1)`, and `RamJournal::new(..., Some((JournalSlot::A, 1)), FlashTargetPolicy::PingPong)` targets `JournalSlot::B` so slot A is never overwritten!
2. `test_commit_flash_verified_keeps_target_when_verify_fails`:
   - Stage a commit on `RamJournal` (`FlashTargetPolicy::PingPong`, `target() == JournalSlot::A`).
   - Run `commit_flash_verified` with a `read_fn` that returns corrupted bytes after `write_fn` returns `true`.
   - Assert `commit_flash_verified` returns `false`, `ram.pending_image().is_some()`, and `ram.target()` remains `JournalSlot::A` (not flipped to `B`, preserving `B` if `B` held the prior valid image).

- [ ] **Step 2: Run tests to verify baseline behavior**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app --test config_persistence
```
Expected: Any newly added assertions fail if edge cases are uncovered, or pass once `commit_flash_verified` / `select_dual_sector` semantics are verified.

- [ ] **Step 3: Harden `DeviceSettingsPersistHost::poll_and_persist` in `src/main.rs`**

Update `DeviceSettingsPersistHost::poll_and_persist` (`src/main.rs:4025-4051`) to:
1. Validate `squeezelite_config::FlashJournal::validate_sector(image.sector).is_ok()` before parking Core 1 or erasing flash; if invalid, log an error and return `false`.
2. Reuse a single 4 KiB PSRAM-heap-allocated scratch buffer (`alloc::vec![0u8; JOURNAL_SECTOR_SIZE]`, or field on `DeviceRuntimeState` / `DeviceSettingsPersistHost` allocated on PSRAM heap — NEVER a `.bss`/`.dram2_uninit` static or 4 KiB stack array!) to:
   - Read target sector (`self.flash.read_nor(offset, &mut verify_buf)`); if `&verify_buf[..] == &image.sector[..]`, skip erase/write, call `self.journal.mark_flash_written()`, and return `true`.
   - Otherwise, call `self.core1.park_for_flash_write();`:
     - Erase `offset..offset + 4096`.
     - Write payload `[16..image.used_len]` at `offset + 16`.
     - Write NVS2 block `[2048..4096]` at `offset + 2048`.
     - Write 16-byte header `[..16]` at `offset` **last** (brownout-safe commit marker: if power drops before `[..16]` is written, `FlashJournal::validate_sector` rejects the incomplete sector on boot and falls back to the alternate slot).
   - Read back `offset` into `verify_buf` and confirm `&verify_buf[..image.used_len] == &image.sector[..image.used_len]` and `FlashJournal::validate_sector(&verify_buf)` succeeds before calling `self.journal.mark_flash_written()`.
   - Call `self.core1.unpark_after_flash_write();` unconditionally after flash I/O completes.
   - Log a concise syslog message (`settings: persisted seq={seq} slot={A|B}`) on success or `WARN` on verification failure.

- [ ] **Step 4: Run host + Xtensa compile/clippy gates and `google_rust_audit.py --strict`**

Run (from `rust/`):
```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p louder-esp32-app --test config_persistence
cargo +esp clippy -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py crates/louder-esp32-app/src --strict
```
Expected: All PASS, 0 warnings, 0 audit violations.

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/tests/config_persistence.rs rust/crates/louder-esp32-app/src/main.rs
git commit -m "fix(settings): add CRC validation, brownout-safe ordering, and read-back verify to DeviceSettingsPersistHost"
```

---

### Task 4: Promote `runtime-flash-persist` (`FlashTargetPolicy::PingPong`) & Update `tests/no_cfg_invariant.rs`

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/main.rs` (`SETTINGS_FLASH_TARGET`, lines `1567-1572`, `2829-2832`, `4030-4033`)
- Modify: `rust/crates/louder-esp32-app/Cargo.toml` (update `runtime-flash-persist` comment explaining promotion to default playback-guarded ping-pong while keeping feature key for backward CLI compatibility)
- Modify: `rust/crates/louder-esp32-app/tests/no_cfg_invariant.rs:56-74` (`runtime_flash_persist_gate_only_in_main` — **update ONLY in this P4 promotion task, never earlier!**)
- Modify: `docs/mk5-rust-services.md` / `docs/mk5-rust-todo.md` (document stabilized playback-guarded behavior)

**Context:**
- Before P4, `#[cfg(not(feature = "runtime-flash-persist"))]` selected `FlashTargetPolicy::FixedA` (`0x3FE000` only) and `tests/no_cfg_invariant.rs:73` asserted `include_str!("../src/main.rs").contains("feature = \"runtime-flash-persist\"")`.
- **Important `esp-rtos-core1` constraint**: `main.rs:1807-1811` has `#[cfg(all(feature = "runtime-flash-persist", feature = "esp-rtos-core1"))] compile_error!(...)`, and `.github/workflows/rust-ci.yml:93` runs `cargo +esp check -p louder-esp32-app --features esp-rtos-core1`. Because D1b (`d1/core1-decode-output`) has not landed yet, `esp-rtos-core1` only runs the heartbeat task on Core 1 while `compile_error!` guards against enabling `runtime-flash-persist` together with `esp-rtos-core1`.
- Therefore, in P4:
  1. Promote `SETTINGS_FLASH_TARGET = FlashTargetPolicy::PingPong` and `image.slot.offset(SETTINGS_JOURNAL_OFFSET)` unconditionally in `main.rs` (`1567`, `2829`, `4030`), removing those three `#[cfg(... "runtime-flash-persist")]` gates so every build uses brownout-safe A/B ping-pong (`0x3FE000` / `0x3FF000`) with playback deferral (`Task 2`) and read-back verification (`Task 3`).
  2. Keep the `#[cfg(all(feature = "runtime-flash-persist", feature = "esp-rtos-core1"))] compile_error!` guard (or document clearly if `main.rs` removes all `feature = "runtime-flash-persist"` references, verify `cargo +esp check --features esp-rtos-core1` still compiles!).
  3. Update `tests/no_cfg_invariant.rs` ONLY in this step:
     - First write the updated invariant test in `tests/no_cfg_invariant.rs` asserting **no** crate source file (including `main.rs` except any retained `esp-rtos-core1` compile_error if kept, OR zero `feature = "runtime-flash-persist"` across all files if the feature gate is retired completely). Confirm the test fails before editing `main.rs`, then edit `main.rs` and confirm it passes.

- [ ] **Step 1: Update `tests/no_cfg_invariant.rs` first (TDD red step)**

Edit `rust/crates/louder-esp32-app/tests/no_cfg_invariant.rs:56-74` so `runtime_flash_persist_gate_not_used` (or updated `runtime_flash_persist_gate_only_in_main`) asserts that `src/main.rs` uses `FlashTargetPolicy::PingPong` without gating `SETTINGS_FLASH_TARGET` or `image.slot.offset(SETTINGS_JOURNAL_OFFSET)` behind `feature = "runtime-flash-persist"` (and none of the library modules mention `feature = "runtime-flash-persist"`).

- [ ] **Step 2: Run `no_cfg_invariant` and see it fail**

Run (from `rust/`):
```powershell
cargo test -p louder-esp32-app --test no_cfg_invariant
```
Expected: FAIL because `main.rs` still gates `SETTINGS_FLASH_TARGET` and `offset` behind `feature = "runtime-flash-persist"`.

- [ ] **Step 3: Promote `PingPong` in `src/main.rs` and update `Cargo.toml` comment**

1. In `src/main.rs:1567-1572`, set `const SETTINGS_FLASH_TARGET: FlashTargetPolicy = FlashTargetPolicy::PingPong;` unconditionally.
2. In `src/main.rs:2829-2832` and `4030-4033`, use `let offset = image.slot.offset(SETTINGS_JOURNAL_OFFSET);` unconditionally.
3. Keep `runtime-flash-persist = []` in `Cargo.toml` with an updated comment noting A/B ping-pong + playback-guarded persistence is now active by default (D1b still owns Core-1 flash parking under `esp-rtos-core1`).

- [ ] **Step 4: Run all 8 CI host & firmware gates + section budget**

Run (from `rust/` and repo root):
```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app
```
And from repo root:
```powershell
python -m unittest discover -s rust/scripts -p "test_*.py"
python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict
```
Expected: All 8 gates PASS; `check_section_budget.py` passes with default thresholds (`IRAM <= 119,976 B`, `.dram2_uninit <= 89,048 B`, `.stack >= 41,256 B`, `internal heap >= 65,536 B`).

- [ ] **Step 5: Commit**

```powershell
git add rust/crates/louder-esp32-app/tests/no_cfg_invariant.rs rust/crates/louder-esp32-app/src/main.rs rust/crates/louder-esp32-app/Cargo.toml
git commit -m "feat(settings): promote playback-guarded A/B ping-pong settings persistence to default"
```

---

### Task 5: Live `DEVBENCH` Verification & Work-Order Log

**Files:**
- Create: `.agents/knowledge/work_orders/WO-P4-runtime-settings-persist.md`
- Modify: `docs/mk5-rust-services.md`, `docs/mk5-rust-todo.md`, `.agents/sessions.md`

- [ ] **Step 1: Rebase onto `MK5-Rust` and re-run all 8 CI + ELF gates**

```powershell
git fetch origin
git rebase origin/MK5-Rust
```
Re-run all 8 gates from Task 4 Step 4.

- [ ] **Step 2: Acquire `DEVBENCH` lock and verify MAC (`ac:15:18:fd:93:9c`)**

```powershell
python rust/scripts/devbench.py acquire feat/runtime-settings-persist
espflash board-info --port COM13
```
Verify MAC equals `ac:15:18:fd:93:9c`. Stop immediately if MAC matches `BASEMENT` (`c8:2e:18:be:b8:34`) or `CYD` (`d4:d4:da:5a:e7:3c`).

- [ ] **Step 3: Build `DEVBENCH` release image and flash via `devbench.py` (max 2 attempts)**

```powershell
$env:SL_DEVICE_NAME = 'DEVBENCH'
Push-Location rust
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
Pop-Location
python rust/scripts/devbench.py flash feat/runtime-settings-persist rust/target/xtensa-esp32-none-elf/release/louder-esp32-app
```
Verify serial boot reaches `[ANTI-BRICK GATE 1]`, DHCP lease, and `"boot: ota_0 confirmed healthy; otadata pinned to ota_0"` (do not reset inside the 4s boot lease).

- [ ] **Step 4: Run live `DEVBENCH` playback-deferral + 50-mutation + power-cycle survival check**

1. Start audio playback on `DEVBENCH` (e.g., Bluetooth A2DP sink or AirPlay or LMS stream).
2. Mutate a persisted setting (e.g., volume/EQ/player_name via HTTP `POST`) **50 times** while audio is playing.
3. Verify via `/status.json` (`settings_deferred: true`), `/messages.json` (`settings: flash persist deferred while audio playing`), and serial log: **0 resets**, **0 I2S underruns**, and **0 flash writes** occurred while playing.
4. Stop playback and wait `> 2,500 ms` (`DEBOUNCE_MS = 2000` + `100 ms` poll). Verify `/messages.json` logs a single debounced write (`settings_writes` increments, slot alternates A <-> B).
5. Power-cycle or reboot `DEVBENCH` (after boot lease completes) and verify via `GET /config.json` / `/status.json` that the final mutated setting survived reboot cleanly.
6. Release `DEVBENCH` lock: `python rust/scripts/devbench.py release feat/runtime-settings-persist`.

- [ ] **Step 5: Record evidence, update docs/sessions, commit, and open PR**

Save full serial log, `check_section_budget.py` output, and `/status.json` + `/config.json` evidence in `.agents/knowledge/work_orders/WO-P4-runtime-settings-persist.md`. Update `.agents/sessions.md` and `docs/mk5-rust-services.md` / `docs/mk5-rust-todo.md` (status `"Wired"` — never `"Certified"`). Commit and open PR against `MK5-Rust`:
```powershell
git add .agents/knowledge/work_orders/WO-P4-runtime-settings-persist.md .agents/sessions.md docs/mk5-rust-services.md docs/mk5-rust-todo.md
git commit -m "docs(p4): record DEVBENCH runtime settings persistence verification"
git push -u origin feat/runtime-settings-persist
gh pr create --repo Mr-Molina/squeezelite-esp32 --base MK5-Rust --head feat/runtime-settings-persist
```

---

## Verification Checklist (Pre-PR Promotion)
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes (including `services::settings::tests`, `tests/config_persistence.rs`, `tests/no_cfg_invariant.rs`, `tests/service_registry_invariant.rs`, `tests/memory_budget_invariant.rs`)
- [ ] `python -m unittest discover -s rust/scripts -p "test_*.py"` passes
- [ ] `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` passes
- [ ] `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` passes
- [ ] `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` + `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app` passes (`IRAM <= 119,976 B`, `.dram2_uninit <= 89,048 B`, `.stack >= 41,256 B`, `internal heap >= 65,536 B`)
- [ ] `python C:\Users\jmolina\.gemini\config\plugins\rust-pro\skills\rust-pro\scripts\google_rust_audit.py rust/crates/louder-esp32-app/src --strict` passes (0 violations)
- [ ] Live `DEVBENCH` (`COM13`, MAC `ac:15:18:fd:93:9c`) check passed: 50 settings mutations during playback caused 0 resets / 0 underruns / deferred flash write, post-stop write succeeded, and settings survived reboot; lock released.
