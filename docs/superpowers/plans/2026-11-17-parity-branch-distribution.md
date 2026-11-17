# Parity-Gap Branch Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land a serial foundation (test board, single firmware loop, memory headroom, plug-in points) and then hand each remaining parity gap to an independent agent as its own branch.

**Architecture:** One branch per work order. Foundation branches F1 to F4 merge strictly in order; F0 runs alongside. After F4, a feature is one file under `src/services/` plus one registration line, so Phase 1 branches do not edit shared code. D1 is split into a host-testable half (Phase 2) and a device half (Phase 3).

**Tech Stack:** Rust `no_std` on ESP32 (`esp-hal`, `esp-radio`, `smoltcp`, `embassy`), host toolchain 1.92.0, Espressif `esp` toolchain 1.97.0.0, `espflash` 4.6.0, Python 3 for scripts.

**Spec:** `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md`

**How to use this plan:** each `## Work order` below is assigned to one agent and becomes one branch and one pull request. F0, F1 and F2 are written to step level and can start now. F3 and F4 are written to task level with their gates and interfaces fixed. Phase 1 to 3 work orders are assignable briefs: their first task is to write their own step-level plan against the code F4 merged, because their exact call sites do not exist yet.

All paths are relative to the repository root. `APP` means `rust/crates/louder-esp32-app`.

## Global Constraints

- BASEMENT (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`) is never flashed, rebooted or sent an OTA by any agent. Never touch `/dev/ttyUSB0` on `pc-1`. Never call `POST /reboot_ota.json`.
- The development module is the Louder-ESP32 on `COM13` of the owner's Windows PC, named `DEVBENCH`. Agents may flash and reset it without asking, one at a time, under the lock in F1 Task 3. At most two flash attempts per check.
- Before every flash: `espflash board-info --port COM13`, and the MAC must equal the one F1 recorded in `.agents/knowledge/DEVICE_STATE.md`. It must never be `c8:2e:18:be:b8:34` (BASEMENT) or `d4:d4:da:5a:e7:3c` (CYD).
- Wi-Fi credentials are never typed, printed, logged or committed by an agent.
- Merge bar: the three host gates and the firmware job in `.github/workflows/rust-ci.yml` are green, and the work order's dev-module check passes with the serial log saved in its work-order file under `.agents/knowledge/work_orders/`.
- Agents push only their own branch from this plan and open a pull request against `MK5-Rust`. They never push to `MK5-Rust` and never merge. Use `--repo Mr-Molina/squeezelite-esp32` with `gh`.
- Memory (Invariant 48): IRAM (`.vectors` + `.rwtext` + `.rwtext.wifi`) <= 131,072 B; `.dram2_uninit` <= 98,768 B; `.stack` >= 28,000 B; internal heap >= 65,536 B. After F3 the script limits are tighter than these and a branch may raise them only by its allowance.
- Feature branches add no `.data`, `.bss` or `.dram2_uninit` statics and no `.iram1` or `#[ram]` code beyond their allowance. State lives on the PSRAM heap.
- A feature branch that needs to edit `pump_device_phase`, `DeviceRuntimeState`, or socket setup directly stops and reports: a plug-in point is missing.
- `tests/no_cfg_invariant.rs` stays green: no `target_arch` in `coordinator.rs` or `pipeline.rs`, no `feature =` in `config_handle.rs`.
- No new workspace crates and no new external dependencies without the owner's approval. The settings journal `VERSION = 1` is not bumped.
- Commits are conventional commits ending with the `Co-Authored-By` line the session supplies. Each work order prepends an entry to `.agents/sessions.md` when it closes.
- Parity status moves to "Wired" at merge. Only the owner sets "Certified", after checking on BASEMENT.

Gates, run from `rust/`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app
```

## Review Focus

1. **Two agents reach for the module at once, or a crashed agent leaves the lock behind.** Expected: the second agent waits or reports; a lock older than two hours can be taken over and the takeover is reported. Pinned by F1 Task 3 tests.
2. **`COM13` is not the board everyone thinks it is** (CH340 IDs are generic; the CYD uses the same adapter). Expected: the flash helper refuses on any MAC other than the recorded one. Pinned by F1 Task 3 tests.
3. **A service is registered but never polled, or implemented but never registered.** Expected: every service in the registry receives `poll` each lap, and every file under `src/services/` is registered. Pinned by F4 Task 1 tests.
4. **A branch passes alone and breaks the budget after its neighbours merge.** Expected: the budget script fails on the rebased branch before merge. Pinned by F3 Task 5 (tight limits) and the rebase rule in every feature brief.
5. **The module resets inside the boot lease and comes up in C recovery.** Expected: the check procedure reports "recovery after early reset", not "firmware failed". Pinned by F1 Task 4 (documented procedure and log markers).

---

## Work order F0: `docs/parity-corrections`

May run alongside any foundation branch. No dev-module check.

### Task 1: Correct section 0 of the parity analysis

**Files:**
- Modify: `docs/superpowers/plans/2026-10-29-rust-parity-analysis.md:38`
- Modify: `APP/Cargo.toml` (the comment on `runtime-flash-persist`)
- Modify: `docs/mk5-rust-todo.md` (any sentence saying settings reach flash only on reboot)

- [ ] **Step 1: Confirm the two facts in source before editing**

Run: `grep -n "s.flash.erase\|park_for_flash_write" rust/crates/louder-esp32-app/src/main.rs`
Expected: hits inside `amp_and_cyd_health_phase` (about lines 4634-4655) with no `#[cfg(feature = "runtime-flash-persist")]` on the block.

- [ ] **Step 2: Replace row 8 of the section 0 table with**

```markdown
| 8 | OTA, recovery, settings persistence | OTA: not implemented in Rust. The Rust firmware selects the boot slot and resets; the C recovery image writes app images. That hand-off is certified on BASEMENT. | The default build erases and writes the settings journal sector at runtime in `amp_and_cyd_health_phase`, with no playback guard. `runtime-flash-persist` only changes the journal offset. Not hardware-certified. Corrected 2026-11-17. |
```

- [ ] **Step 3: Make the `Cargo.toml` comment and `docs/mk5-rust-todo.md` say the same thing**

- [ ] **Step 4: Commit and open the PR**

```bash
git add docs/superpowers/plans/2026-10-29-rust-parity-analysis.md docs/mk5-rust-todo.md rust/crates/louder-esp32-app/Cargo.toml
git commit -m "docs(mk5-rust): correct OTA and settings-persistence parity statements"
```

---

## Work order F1: `foundation/dev-module-bringup`

Starts first. Contains the one owner checkpoint in this plan (Task 2, Step 4).

### Task 1: Compile-time device name

**Files:**
- Modify: `APP/src/board.rs:62` (`first_boot_defaults::PLAYER_NAME`)
- Modify: `APP/src/main.rs:4101`, `:4132`, `:4874`; `APP/src/bt_a2dp.rs:1134`; `APP/src/config_handle.rs:336`
- Create: `APP/tests/device_name_invariant.rs`

**Interfaces:**
- Produces: `board::first_boot_defaults::PLAYER_NAME: &str`, equal to the build-time environment variable `SL_DEVICE_NAME` when set, otherwise `"BASEMENT"`. Every later work order builds DEVBENCH images with `SL_DEVICE_NAME=DEVBENCH`.

- [ ] **Step 1: Write the failing test**

```rust
// APP/tests/device_name_invariant.rs
const FILES: [(&str, &str); 3] = [
    ("main.rs", include_str!("../src/main.rs")),
    ("bt_a2dp.rs", include_str!("../src/bt_a2dp.rs")),
    ("config_handle.rs", include_str!("../src/config_handle.rs")),
];

#[test]
fn device_name_literal_lives_only_in_board() {
    for (name, src) in FILES {
        let hits: Vec<_> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("\"BASEMENT\"") && !l.contains("assert"))
            .map(|(i, _)| i + 1)
            .collect();
        assert!(hits.is_empty(), "{name} hard-codes the device name at lines {hits:?}");
    }
}
```

- [ ] **Step 2: Run it and see it fail**

Run: `cargo test -p louder-esp32-app --test device_name_invariant`
Expected: FAIL listing the lines above.

- [ ] **Step 3: Implement**

In `board.rs`: `pub const PLAYER_NAME: &str = match option_env!("SL_DEVICE_NAME") { Some(n) => n, None => "BASEMENT" };`. Replace each literal in the other files with that constant. Names longer than the existing `heapless::String` capacities are truncated the way the current code already truncates.

- [ ] **Step 4: Run the gates**

Run: `cargo test --workspace` then the firmware build with `SL_DEVICE_NAME` unset.
Expected: PASS; the existing `board.rs` tests still see `"BASEMENT"`.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(app): compile-time SL_DEVICE_NAME override for the device name"
```

### Task 2: Identify, back up and propose the install route

**Files:**
- Create: `.agents/knowledge/work_orders/WO-F1-dev-module-bringup.md`

- [ ] **Step 1: Identify the board** (this resets it into its bootloader)

Run: `espflash board-info --port COM13`
Expected: chip type ESP32, a flash size, and a MAC. Stop and report if the MAC is `c8:2e:18:be:b8:34` or `d4:d4:da:5a:e7:3c`, or if the chip is not a classic ESP32.

- [ ] **Step 2: Back up the whole flash to a directory outside the repository**

Run: `espflash read-flash --port COM13 0x0 <flash size from Step 1> "$USERPROFILE/devbench-backup/devbench-original.bin"`
Expected: a file of exactly the flash size. Record its SHA-256 in the work-order file. The backup may contain credentials; it is never committed.

- [ ] **Step 3: Read the partition table from the backup** (offset `0x8000`, length `0xC00`) and record it in the work-order file. Compare it with BASEMENT's layout in `.agents/knowledge/DEVICE_STATE.md`: `nvs` at `0x9000`, `otadata` at `0xD000`, `factory` recovery at `0x10000`, `ota_0`, `settings` at `0x3FE000`.

- [ ] **Step 4: Owner checkpoint. Write the proposed install route into the work-order file and stop until the owner confirms it.**

The proposal states: what is on the module; where the C bootloader, partition table and recovery image will come from (a release binary of this repository's C firmware, or a copy the owner supplies; never a read-out of BASEMENT); the exact `espflash write-bin` commands and offsets; and that the owner provisions Wi-Fi once through the C setup page.

### Task 3: Lock and guarded flash helper

**Files:**
- Create: `rust/scripts/devbench.py`
- Create: `rust/scripts/test_devbench.py`
- Modify: `.github/workflows/rust-ci.yml` (one step in the host job, the one that runs `cargo test --workspace`: `python3 -m unittest discover -s rust/scripts -p "test_*.py"`)

**Interfaces:**
- Produces, as a command line used by every later work order:
  - `python rust/scripts/devbench.py acquire <branch>`: exit 0 and creates `<git common dir>/dev-module.lock` containing the branch and a UTC timestamp; exit 3 when another branch holds a lock younger than two hours; takes over an older lock and prints `TAKEOVER from <branch>`.
  - `python rust/scripts/devbench.py release <branch>`: removes the lock when held by `<branch>`; exit 3 otherwise.
  - `python rust/scripts/devbench.py check-mac <mac>`: exit 0 only when `<mac>` equals the `DEVBENCH_MAC` constant; exit 4 for anything else, with a specific message for the BASEMENT and CYD MACs.
  - `python rust/scripts/devbench.py flash <branch> <image.bin>`: requires the lock to be held by `<branch>`, runs `espflash board-info --port COM13`, applies `check-mac`, counts attempts in the lock file and refuses a third, then writes the image to the `ota_0` offset recorded in Task 2.
- Functions in the module, for the tests: `acquire(lock_path: Path, branch: str, now: datetime) -> str` returning `"ok"`, `"busy"` or `"takeover"`; `release(lock_path: Path, branch: str) -> bool`; `mac_allowed(mac: str) -> bool`; `record_attempt(lock_path: Path) -> int`.

- [ ] **Step 1: Write the failing tests**

```python
# rust/scripts/test_devbench.py
import tempfile, unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
import devbench

T0 = datetime(2026, 11, 17, 12, 0, tzinfo=timezone.utc)

class LockTests(unittest.TestCase):
    def setUp(self):
        self.lock = Path(tempfile.mkdtemp()) / "dev-module.lock"

    def test_second_branch_is_busy(self):
        self.assertEqual(devbench.acquire(self.lock, "feat/a", T0), "ok")
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(minutes=5)), "busy")

    def test_same_branch_reacquires(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.acquire(self.lock, "feat/a", T0 + timedelta(minutes=5)), "ok")

    def test_stale_lock_is_taken_over(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.acquire(self.lock, "feat/b", T0 + timedelta(hours=2, seconds=1)), "takeover")

    def test_release_only_by_holder(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertFalse(devbench.release(self.lock, "feat/b"))
        self.assertTrue(devbench.release(self.lock, "feat/a"))
        self.assertFalse(self.lock.exists())

    def test_third_attempt_is_refused(self):
        devbench.acquire(self.lock, "feat/a", T0)
        self.assertEqual(devbench.record_attempt(self.lock), 1)
        self.assertEqual(devbench.record_attempt(self.lock), 2)
        with self.assertRaises(devbench.TooManyAttempts):
            devbench.record_attempt(self.lock)

class MacTests(unittest.TestCase):
    def test_basement_and_cyd_are_refused(self):
        self.assertFalse(devbench.mac_allowed("c8:2e:18:be:b8:34"))
        self.assertFalse(devbench.mac_allowed("D4:D4:DA:5A:E7:3C"))

    def test_only_recorded_mac_is_allowed(self):
        self.assertTrue(devbench.mac_allowed(devbench.DEVBENCH_MAC.upper()))
        self.assertFalse(devbench.mac_allowed("00:11:22:33:44:55"))
```

- [ ] **Step 2: Run and see them fail**

Run: `python -m unittest discover -s rust/scripts -p "test_*.py"`
Expected: FAIL, `No module named 'devbench'`.

- [ ] **Step 3: Implement `rust/scripts/devbench.py`** with the functions and commands above. `DEVBENCH_MAC` is the MAC recorded in Task 2. The lock path comes from `git rev-parse --git-common-dir`. The lock is created with exclusive-create so two simultaneous `acquire` calls cannot both succeed.

- [ ] **Step 4: Run the tests, then commit**

Run: `python -m unittest discover -s rust/scripts -p "test_*.py"`
Expected: 7 tests, OK.

```bash
git add rust/scripts/devbench.py rust/scripts/test_devbench.py .github/workflows/rust-ci.yml
git commit -m "feat(tooling): dev-module lock and guarded flash helper"
```

### Task 4: Install, baseline and document

Runs only after the owner has confirmed Task 2 Step 4.

**Files:**
- Modify: `.agents/knowledge/DEVICE_STATE.md`, `CLAUDE.md`, `docs/mk5-rust-todo.md`
- Modify: `.agents/knowledge/work_orders/WO-F1-dev-module-bringup.md`

- [ ] **Step 1: Install the C layout by the confirmed route; the owner provisions Wi-Fi through the C setup page.**
- [ ] **Step 2: Build with `SL_DEVICE_NAME=DEVBENCH` and flash through `devbench.py flash`.**
- [ ] **Step 3: Record the baseline in the work-order file:** full boot log up to the lease; the heap figures the firmware prints; ten minutes of an LMS stream; one AirPlay session; one Bluetooth sink session; the budget script output. Confirm the LMS shows `DEVBENCH` and `BASEMENT` as two players.
- [ ] **Step 4: Trigger one reset inside the boot lease and record the log lines that show the fall-back to C recovery and the way back to the Rust image.** These lines are the markers every later check uses to tell an early reset from a firmware failure.
- [ ] **Step 5: Document.** `DEVICE_STATE.md` gains a DEVBENCH section: MAC, port, partition offsets, backup location and hash, install route, recovery markers. `CLAUDE.md` gains, under the hard safety rules: the DEVBENCH standing flash permission (one agent at a time, through `rust/scripts/devbench.py`, two attempts per check), and the standing permission to push this plan's branches and open pull requests, never to `MK5-Rust`.
- [ ] **Step 6: Commit and open the PR.**

```bash
git commit -am "docs(agents): record DEVBENCH dev module, baseline and standing permissions"
```

---

## Work order F2: `foundation/single-loop`

Starts when F1 has merged.

### Task 1: Remove the unreachable Core 0 code from `tasks.rs`

**Files:**
- Modify: `APP/src/tasks.rs`
- Modify: `APP/tests/no_cfg_invariant.rs:56-73`
- Modify: `APP/src/main.rs:1885` (comment naming `tasks::runtime_flash_commit`)

**Interfaces:**
- Produces: `tasks.rs` keeps exactly `StreamParams`, `CoordinatorCmd`, `AudioEvent`, `CTRL`, `EVT`, `CHANNEL_DEPTH`, `device::core1_heartbeat`, `device::run_core1_executor`, and the tests for those. D1a builds on these names.

- [ ] **Step 1: Change the invariant test first**

Rename `runtime_flash_persist_gate_only_in_tasks_and_main` to `runtime_flash_persist_gate_only_in_main`, add `("tasks.rs", include_str!("../src/tasks.rs"))` to the `libs` array, and replace the last assertion with:

```rust
assert!(include_str!("../src/main.rs").contains("feature = \"runtime-flash-persist\""));
```

Add:

```rust
#[test]
fn tasks_has_no_core0_executor() {
    let src = include_str!("../src/tasks.rs");
    for gone in ["run_core0_executor", "fn run_ring", "enum RingTask", "enum Phase", "fn http_serve"] {
        assert!(!src.contains(gone), "tasks.rs still contains `{gone}`");
    }
    assert!(src.contains("fn run_core1_executor"));
}
```

- [ ] **Step 2: Run and see both fail**

Run: `cargo test -p louder-esp32-app --test no_cfg_invariant`
Expected: FAIL in both tests.

- [ ] **Step 3: Delete from `tasks.rs`** `RingTask`, `Phase`, and everything in `mod device` except `core1_heartbeat` and `run_core1_executor` and the imports they need. Delete the three phase-ring tests. For `test_wo255_amp_health_consecutive_fault_throttling` and `test_wo255_bt_source_arbiter_priority_transitions`: keep each if what it tests lives outside the deleted code, otherwise delete it and say so in the PR description.

- [ ] **Step 4: Run every gate, including the `esp-rtos-core1` check**

Expected: all PASS. The `esp-rtos-core1` check proves `run_core1_executor` survived.

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(app): remove the unreachable core-0 task ring from tasks.rs"
```

### Task 2: Remove the duplicate module declarations

**Files:**
- Modify: `APP/src/main.rs:45-56`

- [ ] **Step 1: Delete the six `mod` lines** (`console`, `controls`, `dac_platform`, `display_gds`, `eth_w5500`, `power_boards`) and import what `main.rs` uses from the library crate (`use louder_esp32_app::console;` and so on). Unused imports are removed, not silenced.
- [ ] **Step 2: Run every gate.** Expected: PASS, and the release image is no larger than before (compare the budget script output with F1's baseline).
- [ ] **Step 3: Commit**

```bash
git commit -am "refactor(app): use the library modules instead of second copies in main.rs"
```

### Task 3: Invariant 49 and the dev-module check

**Files:**
- Modify: `.agents/INFRASTRUCTURE_INVARIANTS.md:77-78`
- Modify: `docs/mk5-rust-todo.md`, `.agents/sessions.md`

- [ ] **Step 1: Rewrite Invariant 49 clauses 1 and 3** so they name `main.rs` as the only device path and drop "in both" and the twin-path requirement. Keep the throughput, socket-window and latency clauses word for word.
- [ ] **Step 2: Dev-module check.** Flash through `devbench.py`; boot log, LMS stream, AirPlay and Bluetooth sink match the F1 baseline.
- [ ] **Step 3: Commit, open the PR.**

```bash
git commit -am "docs(agents): Invariant 49 names main.rs as the single device path"
```

---

## Work order F3: `foundation/memory-rebudget`

Starts when F2 has merged. Every task ends with the budget script output pasted into the work-order file, before and after.

**Targets.** If any is missed after all tasks, stop and report rather than merge:

| Measure | Today | Target after F3 |
|---|---|---|
| `.stack` | 28,528 B | >= 40,000 B (reclaimed `.data`/`.bss` becomes stack) |
| `.dram2_uninit` free | 248 B | >= 8,000 B |
| IRAM free | 1,436 B | >= 10,000 B |
| Internal heap | 64,512 B | >= 65,536 B |

### Task 1: Heap floor as a tested constant

**Files:** Modify `APP/src/board.rs`, `APP/src/main.rs:1211`, `:2633`.

**Interfaces:**
- Produces: `board::INTERNAL_HEAP_BYTES: usize` and `board::BT_HEAP_BYTES: usize`, used by the two allocation sites.

- [ ] **Step 1: Failing test in `board.rs`:** `assert!(INTERNAL_HEAP_BYTES + BT_HEAP_BYTES >= 65_536);`
- [ ] **Step 2: Introduce the constants at today's values (52,224 and 12,288); the test fails.**
- [ ] **Step 3: Do not commit yet. After Task 2 has freed the space, raise `INTERNAL_HEAP_BYTES` to 53,248 (total 65,536), see the test pass, and commit this task then.** The 1,024 B must come from reclaimed memory, not from today's 528 B of `.stack` headroom.

### Task 2: DRAM reclaim, one commit per item

Each item: measure, change, measure, run all gates.

- [ ] Drop the PSRAM-absent fallback buffers in `BSS_BUFFERS` (about 10,240 B). The board always has PSRAM; boot already fails without it.
- [ ] Move the remaining socket buffers to PSRAM (about 3,584 B). PSRAM heap region 0 ordering (Invariant 48) is unchanged.
- [ ] Move `RAM_JOURNAL` and `CONFIG_HANDLE` out of `.bss` (about 4,232 B).
- [ ] Shrink or move `WEB_SYSLOG_RING` to PSRAM (about 3,352 B).
- [ ] AES tables to flash (about 1,280 B).

### Task 3: IRAM reclaim by measurement

- [ ] List our own `.iram1` symbols by size from the linker map. Move code that does not run in interrupt context or with the flash cache disabled back to flash, largest first, until the IRAM target is met. The core-1 entry and the DMA write path stay in IRAM (Invariant 47).
- [ ] After each move: one-hour mixed-codec playback on DEVBENCH with zero underruns in the log.

### Task 4: Soak

- [ ] One-hour playback soak on DEVBENCH (MP3, FLAC, AAC, Bluetooth sink): no underruns, no resets, heap low-water mark recorded.

### Task 5: Tighten the limits and publish allowances

**Files:** Modify `rust/scripts/check_section_budget.py:52-54`, `.agents/INFRASTRUCTURE_INVARIANTS.md` (Invariant 48 note), the allowance table in the spec section 7.

- [ ] **Step 1: Set the script defaults to the measured values with 256 B of slack** (`--max-iram`, `--max-dram2`, `--min-stack`). The Invariant 48 ceilings stay as the hard outer limits in the docstring.
- [ ] **Step 2: Fill the allowance table.** Of the measured free DRAM: 40% to D1, 10% each to P1, P2, P3 and P6, 20% unallocated. IRAM: all to D1. A feature PR that needs its allowance raises the script default by at most that amount in the same PR.
- [ ] **Step 3: All gates, commit, PR.**

---

## Work order F4: `foundation/service-seams`

Starts when F3 has merged. No behaviour change: the F1 baseline must be reproduced.

### Task 1: The service registry

**Files:**
- Create: `APP/src/services/mod.rs`
- Create: `APP/tests/service_registry_invariant.rs`
- Modify: `APP/src/lib.rs`, `APP/src/main.rs` (`DeviceRuntimeState`, `bring_up`, `pump_device_phase`)

**Interfaces (the contract for every Phase 1 branch):**

```rust
pub trait DeviceService {
    fn name(&self) -> &'static str;
    /// Called once in bring-up. Adds this service's sockets.
    fn init(&mut self, ctx: &mut ServiceCtx<'_>);
    /// Called when the link gains or loses its address.
    fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool);
    /// Called once per loop lap. Must return without blocking.
    fn poll(&mut self, ctx: &mut ServiceCtx<'_>);
    /// Appends this service's fields to the status report.
    fn status(&self, out: &mut StatusExtras);
}

/// Registered in exactly one place: `services::register_all`.
pub fn register_all(reg: &mut ServiceRegistry);
```

`ServiceCtx` gives a service: the time in milliseconds, the local IPv4 address when the link is up, socket creation and access by handle, the audio helpers from Task 2, read access to the configuration handle, and the slimproto send hook from Task 3. F4 fixes the field types against the types `main.rs` already uses and documents the final signatures in the module's rustdoc; those are binding for Phase 1. `SOCKET_SLOTS` becomes 16.

- [ ] **Step 1: Host tests first:** a registry with two recording services receives `init` once, `on_link(true)` once and `poll` once per lap, in registration order.
- [ ] **Step 2: Invariant test:** every `.rs` file in `APP/src/services/` other than `mod.rs` is named in `register_all` (source-scan test in the style of `no_cfg_invariant.rs`). This is the test that stops "implemented but never wired".
- [ ] **Step 3: Implement, wire into `bring_up` and `pump_device_phase`, run all gates.**

### Task 2: Shared audio helpers

- [ ] Replace the five copies of the "claim external PCM" block and the four copies of the volume path with `claim_external_pcm`, `release_external` and `set_local_volume`, each with a host test, reachable from `ServiceCtx`.

### Task 3: Slimproto, status and HTTP hooks

- [ ] `SlimExtension` in the coordinator (`on_frame`, `poll_outbound`), with no `target_arch` in `coordinator.rs`; `StatusExtras` in the status JSON; an HTTP action hook so a service can own a path without editing the `http_serve_phase` match.

### Task 4: Move the existing inline features onto the registry

- [ ] One commit each: `services/mdns.rs`, `services/airplay.rs`, `services/spotify_facade.rs`, `services/settings.rs` (the journal write from `amp_and_cyd_health_phase`, behaviour unchanged). `multi_proto_poll` ends up empty and is removed.

### Task 5: Dev-module check and hand-over document

- [ ] Reproduce the F1 baseline on DEVBENCH.
- [ ] Write `docs/mk5-rust-services.md`: how to add a service, the final signatures, the socket and memory rules. Phase 1 briefs point here.

---

## Phase 1 work orders (parallel, start when F4 has merged)

Each brief's first task: read `docs/mk5-rust-services.md` and the spec, then write a step-level plan under `docs/superpowers/plans/` for this work order. Each service is split into a platform-free state machine with host tests and a thin `DeviceService` adapter. Rebase on `MK5-Rust` and rerun the budget script before the dev-module check and again before asking for merge.

| ID | Branch | Owns | Reuses | Sockets | Dev-module check |
|---|---|---|---|---|---|
| P4 | `feat/runtime-settings-persist` | `APP/src/services/settings.rs` | `config_handle.rs`, `squeezelite-config::FlashJournal` | 0 | Change a setting during playback 50 times: no reset, no underrun. Power-cycle: settings survive. No write happens while audio is playing (log proves deferral). |
| P1 | `feat/telnet-cli` | `APP/src/services/telnet.rs`; merges the two command tables into `APP/src/console.rs` | `console.rs` | 1 TCP | Telnet connects on port 23, each command answers, `autoexec` runs at boot, a second client is refused cleanly, a dropped client frees the socket. |
| P2 | `feat/lms-discovery-dns` | `APP/src/services/discovery.rs`, `APP/src/services/dns.rs` | the discovery codec in the slimproto crate | 2 UDP | With no server address configured, DEVBENCH finds the LMS and connects. A stream URL with a hostname plays. With the DNS server unreachable, playback of IP-addressed streams is unaffected. |
| P3 | `feat/airplay-sync-resend` | `APP/src/services/airplay.rs` | the jitter buffer, resend, NTP sync in the AirPlay crate | 2 UDP (timing 6002, control 6001) | Ten minutes from an Apple sender (owner supplies): no audible dropouts, drift under 50 ms, resend requests visible in the log when packets are dropped. |
| P5 | `study/spotify-connect` | `docs/superpowers/specs/2026-MM-DD-spotify-connect-study.md` only | none | 0 | None. Deliverable: TLS options for `no_std` ESP32 with RAM and flash cost measured by a throwaway build that is not merged; the login and audio-key flow; what the existing crypto covers; licensing position; go/no-go; a branch plan if go. |

P4 starts first because the unguarded runtime write is live until it lands.

## Phase 2 work orders (start when P1 to P4 have merged)

| ID | Branch | Scope | Dev-module check |
|---|---|---|---|
| D1a | `d1/coordinator-command-event` | The coordinator is driven by `CoordinatorCmd` over `CTRL` and reports through `AudioEvent` over `EVT`, still on one core. Host tests cover every command and event. Writes its own design doc first. | F1 baseline reproduced; no change in underrun count over a one-hour soak. |
| P6 | `feat/softap-provisioning` | Boot honours journal Wi-Fi credentials ahead of NVS; softAP with a DHCP server and captive portal as `APP/src/services/provisioning.rs`; `/scan.json` and `/ap.json` return real data. Writes its own design doc first. | Erase credentials; provision from a phone; reboot; DEVBENCH rejoins Wi-Fi. A wrong passphrase returns to the portal without a reset loop. |

## Phase 3 work order (starts when D1a and P6 have merged)

| ID | Branch | Scope | Dev-module check |
|---|---|---|---|
| D1b | `d1/core1-decode-output` | Decode and output run on Core 1 under `esp-rtos-core1`. Resolves the `compile_error!` at `main.rs:1900` by giving the settings and credential writes a way to park Core 1. Core-1 entry stays in IRAM; Gate 1 boot order is unchanged (Invariant 47). Writes its own design doc first. | Eight-hour mixed-codec soak with settings changes and one provisioning cycle. Then the owner certifies on BASEMENT. |

## Not in this plan

Display, rotary/buttons/IR/LED/visualiser, other DACs and S/PDIF, W5500 Ethernet, sleep and battery, Bluetooth A2DP source, HTTPS streams, DSD, SqueezeNetwork. Reasons are in the spec, section 9.
