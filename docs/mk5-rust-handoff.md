# MK5-Rust handoff (2026-11-18)

Audience: an agent taking over this repository with no access to earlier
conversations. This file tells you where things stand, what to do next, what
you must not do, and where each claim comes from. It is a snapshot. Where it
disagrees with a ledger or the code, check the code and say so.

Written at `MK5-Rust` = `f957e255` (equal to `origin/MK5-Rust`), from a Windows
11 host. Facts marked **(checked)** were verified on 2026-11-18 against git,
GitHub or the source. Everything else is taken from the project's own ledgers
and is only as fresh as they are.

## 0. Current state, 2026-11-18 19:40 UTC (read this first)

Sections 1 to 10 were written at `f957e255` and are kept as background. Where
they disagree with this section, this section is newer. Everything here was
checked against git, GitHub and the files named, at the time above, unless it
says otherwise.

### 0.1 Where things are

| Item | State |
|---|---|
| `origin/MK5-Rust` | `3283d457` plus the commit that carries this section. PRs `#2` to `#13` merged. No pull request open. |
| Other branches on `origin` | `backup/f3-memory-rebudget-4bcf9486` only (see 0.3). |
| Main checkout `S:\Github\squeezelite-esp32` | fast-forwarded to `origin/MK5-Rust`, clean. |
| Merged since `f957e255` | `#12` flaky torn-write test fixed (section 7.1 is done); `#11` `rust-analyzer` in the pinned toolchain; `#10` and `#13` this file. |
| Plan | F0, F1, F2 merged. **F3 in progress, not merged.** F4 and everything after it not started. |

### 0.2 Local worktrees on the owner's PC

All under `.claude/worktrees/`. "Merged" means the remote branch was deleted
after its squash merge, so git shows the local branch as ahead although its
content is in `MK5-Rust`.

| Worktree | Branch | State |
|---|---|---|
| `dazzling-yonath-8dd057` | `claude/dazzling-yonath-8dd057` | **F3, live work.** 11 commits, not pushed under its own name; three uncommitted doc edits. See 0.3. |
| `foundation-dev-module-bringup-a01186` | `claude/foundation-dev-module-bringup-a01186` | merged (`#7`), clean |
| `blissful-noether-5d05cd` | `claude/blissful-noether-5d05cd` | merged (`#8`), clean |
| `docs-parity-corrections-7166ab` | `claude/docs-parity-corrections-7166ab` | merged (`#6`), clean |
| `github-repo-analysis-76cda3` | `claude/github-repo-analysis-76cda3` | merged (`#10`), clean |
| `repo-setup-plugins-ea5237` | `claude/repo-setup-plugins-ea5237` | merged (`#11`), clean |
| `jolly-kirch-b9dcf3` | `test/config-torn-write-flake` | merged (`#12`), clean |
| `project-analysis-next-steps-a48b5c` | detached at `origin/MK5-Rust` | nothing of its own |

The worktree `mk5-rust-focus-9f9a35` and its branch
`claude/repo-state-report-9b30ad`, listed in section 2, no longer exist. Their
one unpushed commit `39799bd3` ("add feature-parity audit and P01-P30
follow-on backlog", `docs/mk5-rust-parity-audit.md`, 394 lines) is still in
the object store but no branch points to it, so git will eventually discard
it. Who removed the worktree, and whether that was a decision to drop the
commit, is not known. To keep it: `git branch parity-audit 39799bd3`.

The merged worktrees can be removed when the owner says so. Do not remove
`dazzling-yonath-8dd057`.

### 0.3 F3 (`foundation/memory-rebudget`): what exists and what is missing

The work is in worktree `dazzling-yonath-8dd057`. Its 11 commits are also on
`origin` as `backup/f3-memory-rebudget-4bcf9486`, pushed by another session
as a safety copy; it is not the working branch and has no pull request.
Delete it once F3 has its own branch on `origin`.

Committed (tip `4bcf9486`):

- Task 1: heap sizes as checked constants, total 65,536 B.
- Task 2: PSRAM-absent fallback buffers dropped; socket buffers, web syslog
  ring and Bluetooth sink state on PSRAM; `RAM_JOURNAL` and `CONFIG_HANDLE`
  in `.dram2_uninit`; AES tables in flash.
- Task 3: the MP3 synthesis granule and DCT run from flash, not IRAM.
- Task 5: `check_section_budget.py` tightened to the F3 figures with 256 B of
  slack, plus an internal-heap check and its own unit tests;
  `tests/memory_budget_invariant.rs`.
- Extra: `/status.json` reports internal heap free and its low-water mark.

Measured by running the budget script on that worktree's release image
(built 2026-11-18 18:18 UTC):

| Measure | Before F3 | F3 image | F3 target |
|---|---|---|---|
| IRAM used | 129,636 B | 119,720 B (11,352 free) | >= 10,000 free |
| `.dram2_uninit` used | 98,520 B | 88,792 B (9,976 free) | >= 8,000 free |
| `.stack` | 28,536 B | 41,512 B | >= 40,000 B |
| Internal heap | 64,512 B | 65,536 B | >= 65,536 B |

All four static targets are met. `git merge-tree` shows the branch merges
into `3283d457` without conflicts. Its gates were not re-run for this
section.

Not committed: edits to `.agents/INFRASTRUCTURE_INVARIANTS.md`,
`docs/mk5-rust-todo.md` and the design spec (the allowance table: 1,148 B of
DRAM each to P1, P2, P3 and P6; 4,592 B of DRAM and 11,352 B of IRAM to D1;
2,304 B unallocated). No `WO-F3` work-order file and no session entry exist.

### 0.4 The F3 soak is not clean: read this before merging F3

F3's merge bar is a one-hour mixed-codec soak on DEVBENCH with no underruns
and no resets. A soak ran on 2026-11-18 from 18:15 to 19:16 UTC. Its raw
files were in a temporary folder and are copied to
`.agents/knowledge/work_orders/WO-F3-evidence/`. Reading `soak-i1.csv`
(361 samples, one every 10 s):

- **The device restarted twice.** `uptime_ms` fell from 462,470 to 21,313
  between t = 414 s and 439 s, and from 565,986 to 14,802 between t = 984 s
  and 1,003 s. After the second restart it ran 44 minutes without another.
- **Frame rate.** 15 of 358 samples are outside 44,000 to 44,200 frames/s.
  Most are in the first minute (the codec column reads `Bluetooth`) and
  around the two restarts; three are during FLAC playback at t = 404, 601
  and 1,134 s (42,543 to 43,958).
- **Codec coverage.** Samples: FLAC 232, MP3 120, Bluetooth 8, PCM 1. **No
  AAC sample**, although `soak.m4a` was queued, and `base-aac.csv` shows
  `frames_played` at 0 for 171 s. AAC has not been shown to play on DEVBENCH.
- **Heap.** Internal heap low-water mark 5,128 B; idle about 13.4 KB.
  `disconnects` and `i2c_nacks` stayed 0.

What this does not tell you: why the device restarted. It may be the F3
changes (code moved out of IRAM, state moved to PSRAM), the 5 KB heap
low-water mark, or the test setup. Nobody has compared against a soak of the
pre-F3 image, and no pre-F3 image has ever been soaked on DEVBENCH. This
reading was made from the raw file by a session that did not run the soak;
the session that ran it has not written its own conclusion.

Do not merge F3 on the static figures alone. First establish whether the
image before F3 (`3283d457`) also restarts under the same soak. If it does
not, bisect F3's commits; task 3 (MP3 out of IRAM) and the Bluetooth sink
move to PSRAM are the first suspects, and each is one commit.

### 0.5 DEVBENCH as it stands

- Lock: `dev-module.lock` in the shared git directory is held by
  `foundation/memory-rebudget` since 2026-11-18 18:12 UTC, one flash attempt
  used. It goes stale two hours after that. Check with the owner that the F3
  session has stopped before taking it over.
- The module runs the F3 image and is at `172.25.6.102` on the owner's
  `172.25.6.0/24` network. The owner's PC is `172.25.6.122`.
- LMS for audio checks: `http://172.25.8.40:9000/` (owner, 2026-11-18). The
  F3 session reached it through a relay script on the PC (port 9000) and
  served test files from the PC on port 8123. The relay, monitor and LMS
  helper scripts are kept, unreviewed, in
  `.agents/knowledge/work_orders/WO-F3-evidence/`; the audio test files and
  the serial capture script are not.
- Volume was set to 8 to 10 for the soak. Check it before playing anything.

### 0.6 Next steps, in order

1. Confirm with the owner that the earlier F3 session has stopped. Then work
   in `dazzling-yonath-8dd057`, or branch from the backup.
2. Resolve 0.4: soak the pre-F3 image, compare, bisect if needed. Stop and
   report after two flash attempts per check.
3. Get AAC playing on DEVBENCH, or record why it does not.
4. Write `WO-F3-memory-rebudget.md` with the budget output before and after
   each task and the soak result; commit the three doc edits; add a session
   entry; rebase on `origin/MK5-Rust`; run all gates; push; open the PR.
5. After F3 merges: F4 (`foundation/service-seams`) in the plan. Note the
   5 KB runtime heap low-water mark when sizing anything that allocates
   internal RAM.

### 0.7 Owner decisions still open

- Keep or drop commit `39799bd3` (0.2).
- Assign the 2,304 B of unallocated DRAM, or leave it in reserve.
- The three F2 review findings in section 7.2.
- Usage: on 2026-11-18 the weekly limit for the top model stood at 85 %; the
  owner asked sessions to stop and hand off at 95 %. A cheaper model is
  enough for step 4; steps 2 and 3 are diagnosis.

---

## 1. The project in one paragraph

A Rust rewrite of squeezelite (a Lyrion/Logitech Media Server player) for the
ESP32, running bare metal (`no_std`, `esp-hal 1.0`, `esp-radio`, `smoltcp`,
`embassy`) on a Sonocotta Louder-ESP32 board with a TAS5805M amplifier and
4 MB PSRAM. All Rust is under `rust/`. The C/ESP-IDF tree (`components/`,
`main/`, root build files) is reference only. The current goal is feature
parity with the C player, delivered as a planned series of branches.

## 2. Repositories and branches (checked)

| Item | Value |
|---|---|
| `origin` | `Mr-Molina/squeezelite-esp32`, public fork, default branch `MK5-Rust` |
| `upstream` | `sle118/squeezelite-esp32`, default `master-v4.3`, read-only reference |
| Branches on `origin` | `MK5-Rust` only |
| Open pull requests on `origin` | none |
| Issues on `origin` | disabled; work is tracked in files (section 4) |
| Merged PRs | `#2` to `#9`, 2026-11-16 to 2026-11-18 |
| Last six CI runs on `MK5-Rust` | green except `2d1a7039` (section 7.1) |

`gh` resolves to **upstream** by default in this checkout. Always pass
`--repo Mr-Molina/squeezelite-esp32`.

Upstream tracking: `ACTIVE_BUGS.md` records upstream followed through
`ee1720c7`, which is still the tip of `upstream/master-v4.3`. Upstream has 62
open issues and 17 open PRs; none is a task here.

### Local worktrees and branches on the owner's PC (checked)

All are clean. Their remote branches were deleted after the squash merges, so
`git branch -vv` shows them as ahead of `MK5-Rust` although their content is
merged.

| Worktree (under `.claude/worktrees/`) | Branch | State |
|---|---|---|
| (main checkout `S:\Github\squeezelite-esp32`) | `MK5-Rust` at `b1adb38c` | three commits behind `origin/MK5-Rust`; fast-forward before use |
| `foundation-dev-module-bringup-a01186` | `claude/foundation-dev-module-bringup-a01186` | merged as PR `#7` |
| `blissful-noether-5d05cd` | `claude/blissful-noether-5d05cd` | merged as PR `#8` |
| `docs-parity-corrections-7166ab` | `claude/docs-parity-corrections-7166ab` | merged as PR `#6` |
| `mk5-rust-focus-9f9a35` | `claude/repo-state-report-9b30ad` | **one unmerged, unpushed commit** `39799bd3` "add feature-parity audit and P01-P30 follow-on backlog" |
| `project-analysis-next-steps-a48b5c` | detached at `f957e255` | nothing of its own |
| `github-repo-analysis-76cda3` | `claude/github-repo-analysis-76cda3` | holds this file, uncommitted |

Do not delete worktrees or branches without the owner's say. `39799bd3`
predates the parity-branch plan and may overlap it; ask the owner whether to
keep or drop it.

## 3. Rules that bind you

The full text is in `CLAUDE.md` and `.agents/INFRASTRUCTURE_INVARIANTS.md`.
Read the invariants file before hardware, flash, git-remote or multi-agent
work. The ones that most often matter:

**BASEMENT, the live device (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`).**
- Never flash, reboot or OTA it without the owner's approval in the current
  session.
- Never call `POST /reboot_ota.json`. Never touch `/dev/ttyUSB0` on `pc-1`
  (that port is the separate CYD display board, MAC `d4:d4:da:5a:e7:3c`).
- It runs `579ffcde` in `ota_0`, which is older than F1 and F2.

**DEVBENCH, the test board (`COM13`, MAC `ac:15:18:fd:93:9c`).**
- Standing permission to flash and reset it, one agent at a time, only through
  `rust/scripts/devbench.py` (`acquire`, `flash`, `select-ota0`, `release`).
  At most two flash attempts per check.
- The helper writes only the app in `ota_0` (`0x150000`) and the select sector
  at `0xD000`. Never write the bootloader, partition table, recovery, `nvs` or
  `settings`.
- Build its image with `SL_DEVICE_NAME=DEVBENCH` (the name must be exactly
  8 bytes).
- Reachable only from the owner's Windows PC. A cloud session cannot run a
  dev-module check.
- No lock is held now: `dev-module.lock` is absent from the shared git
  directory **(checked)**.

**Git.**
- Never push to `MK5-Rust` and never merge. Push only your own work-order
  branch and open a PR against `MK5-Rust`. Only the owner merges.
- Do not push anything else to `origin` without the owner's go-ahead.
- Conventional commits (`feat(app): ...`, `docs(mk5-rust): ...`), one task per
  commit, body explaining why.

**Secrets.** Never type, print, log or commit Wi-Fi credentials. The DEVBENCH
flash backup at `C:\Users\jmolina\devbench-backup\` may hold credentials and
stays outside the repository.

**Working method.**
- Invariant 14: three failed attempts at one error, then stop and write up a
  research request.
- Invariant 39: cite evidence before stating a cause.
- Invariant 69: never fix a failure by disabling a feature without consent.
- Invariant 70: a final response that asks permission or stops work ends with
  a `### Proposal Summary` block.
- `D1` (decode and output on core 1) was owner-gated. The owner authorized it
  in full on 2026-11-17 as part of the plan below (spec section 4), but only
  in its planned position: `D1a` in Phase 2, `D1b` in Phase 3. `CLAUDE.md`
  still words it as gated. Do not start it early; if in doubt, ask.

## 4. Where the truth is written

| File | Use |
|---|---|
| `CLAUDE.md` | Auto-loaded rules and gates |
| `docs/mk5-rust-todo.md` | Crate map, commands, device facts, the finished `T01`-`T27` list, section 5 gap list |
| `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md` | **The active plan.** Work orders F0-F4, P1-P6, D1a, D1b |
| `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md` | Why the plan is shaped that way; owner decisions (section 4); rules for feature branches (section 7); dev-module procedure (section 8) |
| `docs/superpowers/plans/2026-10-29-rust-parity-analysis.md` | **Section 0 only** is accurate. Sections 1-5 overstate what runs on the device |
| `.agents/sessions.md` | Session ledger, newest first. Read the top entry; prepend one when you close a work order |
| `.agents/knowledge/DEVICE_STATE.md` | BASEMENT and DEVBENCH hardware, partitions, recovery markers |
| `.agents/knowledge/work_orders/` | One file per work order with logs. `WO-F1-*.md` and `WO-F2-*.md` are the baselines you compare against |
| `.agents/INFRASTRUCTURE_INVARIANTS.md` | Binding rules; 46-49 are specific to this firmware |
| `.agents/PATTERN_LIBRARY.yaml` | Past failure patterns |
| `rust/vendor/esp-radio/VENDOR.md` | Local patches to the vendored radio crate |

Known to be stale; do not trust without checking:

- `.agents/knowledge/SYSTEM_STATE.md` describes the Linux host `pc-1`, toolchain
  `rustc 1.99.1` and a baseline of `579ffcde`. Current work runs on Windows;
  the host toolchain is pinned to `1.92.0` in `rust/rust-toolchain.toml`
  **(checked)**.
- `.agents/knowledge/work_orders/INDEX.md` has no row for `WO-F2`
  **(checked)**, and its `WO-F1` row predates the merge.
- `.agents/sessions.md` has no entry for PR `#9` or for this handoff.
- `docs/DEVELOPMENT_ENVIRONMENT.md` covers the legacy C build only.
- `docs/mk5-rust-todo.md` and `CLAUDE.md` still describe `D1` as blocked
  (see section 3).
- Absolute paths in older ledgers (`/home/console-admin/...`) are from the
  Linux host.

## 5. State of the firmware

**Structure.** One loop on core 0: `run_hardware_firmware` -> `bring_up()` ->
`loop { pump_device_phase }` in `rust/crates/louder-esp32-app/src/main.rs`.
Core 1 runs a heartbeat only. `tasks.rs` holds just the `CTRL`/`EVT` channel
types and the core-1 executor (F2). `tests/no_cfg_invariant.rs` fails if a
second device loop comes back.

**What runs on the device** (parity analysis, section 0):

| State | Subsystems |
|---|---|
| Certified on BASEMENT (at `579ffcde`) | SlimProto playback of FLAC and MP3 at 44.1 kHz; Wi-Fi station; part of the REST API; boot-slot hand-off to the C recovery image |
| Wired, no hardware evidence | PCM, AAC, Vorbis, ALAC, Opus; other sample rates; gapless, fade, gain; EQ, loudness, resampler; mDNS; Bluetooth A2DP sink; AirPlay audio (no timing or control channel); CYD link |
| Library-only (host-tested, never called) | Telnet console, displays, controls, other DACs and S/PDIF, W5500 Ethernet, power boards, Bluetooth A2DP source, LMS discovery, softAP and captive portal |
| Not functional | Spotify Connect (a placeholder on TCP 4070) |

OTA is not implemented in Rust: the firmware selects the boot slot and resets,
and the C recovery image writes the app.

**Memory, release ELF at F2** (`WO-F2` section 3). This is the main
constraint on all further work.

| Section | Value | Limit | Headroom |
|---|---|---|---|
| IRAM (`.vectors` + `.rwtext` + `.rwtext.wifi`) | 129,636 B | <= 131,072 | 1,436 B |
| `.dram2_uninit` | 98,520 B | <= 98,768 | 248 B |
| `.stack` | 28,536 B | >= 28,000 | 536 B |
| Internal heap | 64,512 B | >= 65,536 (Invariant 48) | **1,024 B short**; CI does not check it |

Flash is not a constraint (about 680 KB free in `ota_0`).

**Other device facts.** The I2S DMA buffer is 16 KB, about 46 ms at 44.1 kHz;
anything that blocks the loop longer underruns. Only whole 8-byte stereo
frames go into the DMA (Invariant 46). Every field added to `StreamPipeline`
needs a matching initializer in `init_static_pipeline()` (Invariant 48).
Decoder output is bit-exact against reference decoders in tests; do not change
rounding casually.

## 6. The plan and where it stands

Foundation branches merge strictly in order; features follow in parallel.

| ID | Branch name in the plan | Status |
|---|---|---|
| F0 | `docs/parity-corrections` | merged, PR `#6` |
| F1 | `foundation/dev-module-bringup` | merged, PR `#7`; closed by the owner with the audio baseline not recorded |
| F2 | `foundation/single-loop` | merged, PR `#8`; same audio items not checked |
| **F3** | `foundation/memory-rebudget` | **next; not started** |
| F4 | `foundation/service-seams` | after F3 |
| P4, P1, P2, P3, P5 | Phase 1, parallel, after F4 (P4 first) | not started |
| D1a, P6 | Phase 2, after P1-P4 | not started |
| D1b | Phase 3, after D1a and P6 | not started |

### F3 in brief

Full text: plan lines 351-400. Targets, all required before merge:

| Measure | Now | Target |
|---|---|---|
| `.stack` | 28,536 B | >= 40,000 B |
| `.dram2_uninit` free | 248 B | >= 8,000 B |
| IRAM free | 1,436 B | >= 10,000 B |
| Internal heap | 64,512 B | >= 65,536 B |

Tasks: (1) heap sizes as tested constants in `board.rs`
(`INTERNAL_HEAP_BYTES`, `BT_HEAP_BYTES`), raised only from reclaimed memory;
(2) DRAM reclaim, one commit per item: PSRAM-absent fallback buffers, socket
buffers to PSRAM, `RAM_JOURNAL` and `CONFIG_HANDLE` out of `.bss`,
`WEB_SYSLOG_RING`, AES tables to flash; (3) IRAM reclaim by measuring `.iram1`
symbols in the linker map, keeping the core-1 entry and DMA write path in
IRAM; (4) one-hour soak on DEVBENCH; (5) tighten
`rust/scripts/check_section_budget.py` to measured values plus 256 B and fill
the allowance table in spec section 7.

Paste the budget script output into the work-order file before and after every
task. If a target is missed after all tasks, stop and report; do not merge.

### Blocker for F3 that needs the owner

F3 tasks 3 and 4 require playback on DEVBENCH (MP3, FLAC, AAC, Bluetooth
sink) with zero underruns. **No Rust image has ever played audio on
DEVBENCH**: its network (`172.25.6.0/24`) had no route to the LMS at
`10.0.0.50`, its LMS target is still the first-boot default `10.0.0.10`, and
`frames_played` stayed 0 in both F1 and F2. Before F3 can finish, the owner
has to put the module on a network that reaches the LMS (or point it at one),
and be present with a sender for Bluetooth. Raise this at the start of F3, not
at task 3. Tasks 1 and 2 need only a boot check and can proceed meanwhile.

When audio first plays on DEVBENCH, turn the volume down first (`vol_pct`
read 80).

## 7. Open items outside the plan

### 7.1 Flaky property test in `squeezelite-config` (checked)

CI run `44328182324` on `2d1a7039` failed in
`fuzz_dual_sector_torn_write_recovery`
(`rust/crates/squeezelite-config/tests/config_fuzz.rs:126`). The next two
pushes passed with no change to that test, so the failure is random.

- Minimal failing input from the CI log: `v1 = [0]`, `v2 = [255]`,
  `tear_pct = 97`. Assertion at line 174: recovered `[255]`, expected `[0]`.
- The test simulates a torn write by filling sector B with `0xFF` from the
  tear point. The journal keeps its CRCs in the sector and entry headers and
  the value bytes come last (`save_to_sector_with_seq`, around
  `src/lib.rs:303-366`). With a one-byte value of `0xFF` and a tear at 97 %,
  only that last byte is "torn", and it already was `0xFF`. Sector B is
  unchanged and still valid, so the loader correctly returns `v2`.
- Reading: this is a defect in the test, not in `FlashJournal`. That reading
  comes from the source and the logged input; nobody has reproduced it
  locally yet.
- Suggested fix, small and separate from any work order: make the tear
  corrupt the sector in every case (for example, skip inputs where the torn
  tail equals the original bytes with `prop_assume!`, or tear with a value
  that differs from the original), and add the logged case as a fixed
  regression test. Do not weaken the recovery assertion.

Until fixed, any PR can fail CI at random on this test. A re-run clears it,
but record that you re-ran and why.

### 7.2 Findings from the F2 review that need an owner decision

Recorded in `WO-F2-single-loop.md` section 6. None was acted on.

1. Settings are written to flash at runtime in every build with no "not while
   playing" guard, and the write result is ignored. P4 is planned to fix this;
   the guarded variant now exists only in history
   (`git show aed5c388:rust/crates/louder-esp32-app/src/tasks.rs`).
2. External sources (Bluetooth, AirPlay) start without flushing the stream
   buffer; `main.rs` calls `begin_external_playback()` at five sites where the
   removed code used the flushing variant. Intent not established.
3. Amplifier fault recovery in `main.rs` (flat 30 s cooldown) has no test.

### 7.3 Small tooling gaps

- `devbench.py` has no reset-and-capture command. F2 used a one-off pyserial
  script with a credential filter. A `capture` subcommand would save every
  later work order that step.
- The firmware prints PSRAM figures only; there is no internal-heap figure
  (F3 task 1 addresses the constant, not the printout).
- CI warns that `actions/checkout@v4` targets the deprecated Node 20 runtime.
- F2's image grew 32 B where no growth was expected; cause not established,
  no budgeted section grew.

### 7.4 Hardware certification still owed (owner only)

Everything in the "Wired" row of section 5 has never been confirmed on
BASEMENT. Only the owner certifies, on BASEMENT, after a branch merges.

## 8. How to build and test

Run from `rust/`. Host toolchain `1.92.0` (pinned). Espressif toolchain
`esp` `1.97.0.0`, used as `+esp`. `espflash` 4.6.0.

```bash
# Host gates (CI job "Rust CI")
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Firmware gates (CI job "Firmware (xtensa-esp32)")
cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings
cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc
python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app
```

From the repository root:

```bash
python -m unittest discover -s rust/scripts -p "test_*.py"
```

Notes:
- Do not use `--workspace` with the Xtensa target. It builds
  `squeezelite-decoders` with `std` and fails in `byteorder`.
- CI runs on push and pull request to `MK5-Rust`. Expect about 828 host tests
  after F2.
- The merge bar is all of the above green **and** the work order's dev-module
  check, with the serial log saved in its work-order file.

### Dev-module check

From the repository root, with `<name>` the work-order name from the plan
(F1 and F2 used `foundation/...`, not the git branch name):

```bash
python rust/scripts/devbench.py acquire <name>
espflash save-image --chip esp32 rust/target/xtensa-esp32-none-elf/release/louder-esp32-app rust/target/devbench/app.bin
python rust/scripts/devbench.py flash <name> rust/target/devbench/app.bin
python rust/scripts/devbench.py select-ota0 <name>   # only if it came up in C recovery
python rust/scripts/devbench.py release <name>
```

Things that look like failures and are not:
- The firmware erases `otadata` about 1 s after reset and writes it back when
  DHCP completes (3 to 5 s). A reset in between boots the C recovery image.
  `select-ota0` brings the Rust image back without a reflash. Markers are in
  `DEVICE_STATE.md`.
- Serial reads at 921600 baud corrupt on this adapter. Use 460800 or lower;
  the log is at 115200.
- A lock older than two hours may be taken over; report the takeover.

Baseline to compare against: `WO-F2-single-loop.md` section 5 (boot log,
`/status.json`, `/messages.json`).

## 9. Closing a work order

1. All gates green locally, budget output recorded.
2. Dev-module check done and logged; lock released.
3. Work-order file under `.agents/knowledge/work_orders/`, plus a row in
   `INDEX.md`.
4. New entry prepended to `.agents/sessions.md`: scope, outcomes, what was
   not checked and why, notes for the next session.
5. Debug scripts and scratch files removed (Invariant 13); no secrets
   (Invariant 24).
6. Push your branch, open a PR against `MK5-Rust` on
   `Mr-Molina/squeezelite-esp32`, and stop. Say plainly what was not verified.

## 10. Suggested first session

1. Fast-forward the main checkout to `origin/MK5-Rust`; work in a fresh
   worktree branched from it.
2. Read `CLAUDE.md`, the top entry of `.agents/sessions.md`, plan lines
   351-400, and `WO-F2-single-loop.md`.
3. Ask the owner three things before starting F3: whether DEVBENCH can be
   given a route to an LMS; whether to fix the flaky test (7.1) first as its
   own small PR; what to do with commit `39799bd3`.
4. Run all gates on an untouched tree and record the budget output as the F3
   "before" figures.
5. Start F3 task 2 (DRAM reclaim), one item per commit.

Housekeeping the owner may want folded into the first PR or done separately:
the `WO-F2` row in `INDEX.md`, a session entry for PR `#9`, the `D1` wording
in `CLAUDE.md` and the todo, and a refresh of `SYSTEM_STATE.md` for the
Windows host.
