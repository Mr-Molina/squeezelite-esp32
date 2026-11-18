# Parity-gap branch distribution — design

Date: 2026-11-17. Status: draft for owner approval. Base: `MK5-Rust` at `6ace2d1b`.

## 1. Intent

The owner wants the remaining C-to-Rust parity gaps split into branches that
independent agents can each take, finish and merge. The reason is throughput:
one agent working the gaps in sequence is slow, and the gaps look separable.

They are not separable today. Every gap edits the same regions of
`rust/crates/louder-esp32-app/src/main.rs`, the network socket table is full,
and the memory budgets have no room. This design therefore puts a short serial
foundation in front of the parallel work, so that afterwards a gap really is
"one new file plus one registration line".

## 2. Goals, non-goals, anti-goals

**Goals**

- Each feature branch can be built, checked and merged without waiting on
  another feature branch.
- Every merged feature runs on real hardware, not only in host tests.
- Memory budgets (Invariant 48) hold after every merge, enforced by CI.
- BASEMENT is never touched by an agent.

**Non-goals**

- Gaps that need hardware the board does not have (see section 9).
- Implementing Spotify. Only a feasibility study is in scope.
- An ESP32-S3 port.

**Anti-goals** (the plan has failed if these happen, even with green CI)

- More code that is "implemented and tested" but never called by the firmware.
  This is how the project reached its present state.
- A branch that passes alone and breaks the memory budget once merged with
  the others.
- Two agents flashing the development module at the same time, or any agent
  flashing BASEMENT.

## 3. Facts the design rests on

Found during prep on 2026-11-16. Items marked (verified) were checked against
source by the author; the rest come from read-only investigations.

- The Core 0 half of `tasks.rs` never runs (verified). `run_core0_executor`
  has no call site. The firmware is one loop: `run_hardware_firmware` ->
  `bring_up()` -> `loop { pump_device_phase }`. That half of `tasks.rs` is a
  stale copy without AirPlay, Spotify or mDNS. The rest of the file is live or
  required: `run_core1_executor` and `core1_heartbeat` are called from
  `main.rs:2791` under `esp-rtos-core1` (which CI builds), the D1 `CTRL`/`EVT`
  channel scaffolding lives there, its host tests run in CI, and
  `tests/no_cfg_invariant.rs:72` requires the file to contain
  `feature = "runtime-flash-persist"`.
- The firmware depends on the C flash layout (verified). Gate 1 erases
  `otadata` on every boot (`main.rs:2521-2545`), so any reset before the boot
  lease completes lands in the C `factory` recovery image at `0x10000`. That
  is by design and is not a firmware failure.
- Wi-Fi credentials come from the C NVS partition at `0x9000`, otherwise from
  compile-time `SL_WIFI_SSID` / `SL_WIFI_PASS` (default SSID `"iot"`). With
  neither, the firmware resets to recovery.
- The player name `"BASEMENT"` is hard-coded for LMS, AirPlay, Spotify and
  Bluetooth (`board.rs:62`, `main.rs:4101`, `4132`, `4874`,
  `bt_a2dp.rs:1134`). There is no device-side build override; `SL_PLAYER_NAME`
  is host-only. An unmodified second board would appear as a second
  "BASEMENT".
- `SOCKET_SLOTS = 8` and all eight are used (verified): DHCP, HTTP, slimproto,
  audio, mDNS, RTSP, RTP, Spotify. Adding a socket takes about nine edits.
- No plug-in points exist. AirPlay, Spotify and mDNS are inline in
  `multi_proto_poll`; the "claim external PCM" block is copied five times and
  the volume path four times.
- Memory headroom: IRAM 1,436 B, `.dram2_uninit` 248 B, `.stack` 528 B over
  its floor. About 22.7 KB of DRAM is reclaimable at low risk and about
  12.5 KB of IRAM at medium risk.
- Internal heap is 64,512 B, 1,024 B under Invariant 48's 65,536 B floor
  (verified). CI does not check it.
- The default build erases and writes the settings journal sector at runtime
  in `amp_and_cyd_health_phase` (verified). It is not behind
  `runtime-flash-persist` and has no "not while playing" guard, which
  contradicts the docs and the `Cargo.toml` comment.
- Flash is not a constraint: 679,872 B free in `ota_0`.

## 4. Owner decisions (2026-11-17)

| Topic | Decision |
|---|---|
| Split approach | Foundation first |
| Test hardware | A second Louder-ESP32, same board as BASEMENT, is attached to the owner's Windows PC by USB-C. A CH340 adapter appears as `COM13`. |
| Flash rights | Agents may flash and reset the development module without asking each time, one agent at a time. BASEMENT rules are unchanged. |
| Merge bar | CI green, plus the feature shown working on the development module |
| Optional hardware on BASEMENT | None (no IR receiver, display or W5500) |
| Spotify | Study replacing the port-4070 facade with real functionality. Study only. |
| D1 | Included in full. This is the explicit authorization CLAUDE.md requires. |
| Module contents | Blank or unknown. F1 inspects, backs up, and proposes how to install the C layout before anything is written. |
| Who checks | The agent that wrote a branch flashes the development module and runs its check, with no per-flash confirmation. This is a pre-merge check, not certification under Invariant 47; Invariant 42's confirmation step does not apply to the module. Certification stays with the owner, on BASEMENT. |
| Pushes | Agents may push their own branch from this plan and open a pull request. They never push to `MK5-Rust` and never merge. |
| Bluetooth source | Out of this plan. |

## 5. Approaches considered

1. **Foundation first (chosen).** Serial branches add plug-in points, free
   memory and retire the dead `tasks.rs` copy; features then run in parallel.
   Cost: nothing parallel starts until the foundation lands.
2. **Library-only parallel.** Agents finish host-testable logic in their own
   modules and one integrator wires everything later. No conflicts, but it
   reproduces the anti-goal above.
3. **Parallel now, resolve at merge.** Every branch edits the same state
   struct and loop, and the memory total is unknown until the end. Rejected.

## 6. Branch map

Sizes: S is under a day of agent work, M a few days, L a week or more.

### Phase 0 — foundation (serial unless noted)

| ID | Branch | What it does | Done when |
|---|---|---|---|
| F0 | `docs/parity-corrections` | Corrects two wrong statements in section 0 of `docs/superpowers/plans/2026-10-29-rust-parity-analysis.md`: settings do reach flash at runtime in the default build, and OTA is not Rust-certified (the C recovery image does the flashing). | Doc merged. May run alongside any foundation branch. |
| F1 | `foundation/dev-module-bringup` | Identifies the module on `COM13` with `espflash board-info` (this resets the chip into its bootloader, so it is not read-only) and stops unless the MAC is neither BASEMENT's (`c8:2e:18:be:b8:34`) nor the CYD's (`d4:d4:da:5a:e7:3c`). Reads the partition table and backs up the full flash before any write. Reports what is on the module and proposes the install route (section 8) for the owner to confirm before the first write. Adds a compile-time device name override and builds the module's image as `DEVBENCH`, so it never appears as a second "BASEMENT". Installs current `MK5-Rust` and records a baseline: boot log, heap figures, one LMS stream, one AirPlay session, one Bluetooth sink session. Adds the module and the procedure in section 8 to `.agents/knowledge/DEVICE_STATE.md` and CLAUDE.md. | Baseline recorded; procedure merged. |
| F2 | `foundation/single-loop` | Removes the unreachable Core 0 code from `tasks.rs` (`run_core0_executor` and what only it uses). Keeps `run_core1_executor`, `core1_heartbeat`, the D1 channel scaffolding, the host tests that still apply, and the `runtime-flash-persist` reference `no_cfg_invariant` requires. Removes the duplicate `mod` declarations at `main.rs:45-56`. Rewrites Invariant 49 clauses 1 and 3, which both name `tasks.rs` as a second path. | CI green including the `esp-rtos-core1` check; dev-module baseline unchanged. |
| F3 | `foundation/memory-rebudget` | Reclaims DRAM (drop PSRAM-absent fallback buffers, socket buffers to PSRAM, journal statics to dram2, syslog ring, AES tables, heap split) and IRAM (cold `.iram1` code to flash). Raises internal heap to at least 65,536 B out of the reclaimed space, never out of `.stack`. Adds the heap figure to the CI budget script, tightens the limits so reclaimed space cannot be spent silently, and publishes the per-branch allowances in section 7. | CI green with the new limits; a one-hour playback soak on the dev module shows no underruns or resets. |
| F4 | `foundation/service-seams` | Adds the plug-in points: a service list (`init`, `on_link`, `poll`, `status`), `SOCKET_SLOTS` raised to 16 with buffers on PSRAM, shared `claim_external_pcm` / `release_external` / `set_local_volume` helpers, a slimproto extension hook, and status and HTTP extension hooks. Moves mDNS, AirPlay, the Spotify facade and the settings-journal write onto them as proof. No behaviour change. | CI green; LMS, AirPlay and Bluetooth sink behave as in the F1 baseline. |

Order: F1, F2, F3, F4, strictly in sequence. F2 needs F1's baseline to compare
against, and F3 comes before F4 because the larger socket table needs the
memory F3 frees. Only F0 runs alongside.

### Phase 1 — parallel, starts when F4 merges

| ID | Branch | Gap | Size | Dev-module check |
|---|---|---|---|---|
| P1 | `feat/telnet-cli` | Telnet console, one merged command table, autoexec | M | Connect by Telnet, run commands, autoexec runs at boot |
| P2 | `feat/lms-discovery-dns` | LMS UDP discovery and a DNS resolver | M | Boots with no server address configured and finds the LMS; resolves a hostname stream URL |
| P3 | `feat/airplay-sync-resend` | Connect the existing jitter buffer, resend and NTP sync; bind the timing and control ports the SETUP reply already advertises | M | Ten minutes of AirPlay from an Apple sender without drift or dropouts; owner supplies the sender |
| P4 | `feat/runtime-settings-persist` | Make the runtime settings write safe: no write while audio is playing, Core 1 parked, behaviour documented. Works inside the settings service F4 created. | S | Change settings during playback 50 times; no reset, settings survive a power cycle |
| P5 | `study/spotify-connect` | Written study only: TLS options for `no_std` ESP32 with measured RAM and flash cost, the login and audio-key flow, what the existing crypto covers, licensing position, and a go/no-go with a branch plan | M | None; deliverable is a document under `docs/superpowers/specs/` |

P1 to P4 each own one service file. P5 touches no firmware code.

### Phase 2 — starts when P1 to P4 have merged (P5 need not have)

| ID | Branch | Gap | Size | Dev-module check |
|---|---|---|---|---|
| D1a | `d1/coordinator-command-event` | Host-testable half of D1: the coordinator becomes command/event driven over the existing `CTRL`/`EVT` channels, still on one core. It rewrites coordinator call sites that the Phase 1 services use, which is why it waits for them. | L | Baseline behaviour unchanged |
| P6 | `feat/softap-provisioning` | Boot honours journal Wi-Fi credentials; softAP with DHCP server and captive portal; real scan | L | Erase credentials, provision from a phone, reboot, module rejoins Wi-Fi |

D1a and P6 may run in parallel: one changes the coordinator, the other the
network bring-up.

### Phase 3 — after D1a and P6

| ID | Branch | Gap | Size | Dev-module check |
|---|---|---|---|---|
| D1b | `d1/core1-decode-output` | Decode and output move to Core 1 under `esp-rtos-core1` | L | Eight-hour mixed-codec soak; then the owner certifies on BASEMENT |

D1b runs alone. It edits `bring_up()` next to the Wi-Fi start that P6 changes,
and it must resolve the `compile_error!` at `main.rs:1900` that today forbids
runtime flash writes together with `esp-rtos-core1`, which P4 and P6 both
depend on.

## 7. Rules every feature branch follows

1. Branch from `MK5-Rust` after F4 has merged. Rebase before the dev-module
   check and again before merge.
2. Wire the feature into `main.rs` through the F4 plug-in points. A new
   service is one new file and one registration line. A branch that needs to
   edit `pump_device_phase`, the state struct or socket setup directly stops
   and reports, because that means a plug-in point is missing.
3. Memory: no new `.data`, `.bss` or `.dram2_uninit` statics and no new
   `.iram1` or `#[ram]` code beyond the branch's allowance. State lives on the
   PSRAM heap. F3 filled in this table from its closing measurement
   (`WO-F3-memory-rebudget.md`, 2026-11-18): 11,488 B of free DRAM, which is
   9,976 B of `.dram2_uninit` below its 98,768 B ceiling plus 1,512 B of
   `.stack` above the 40,000 B F3 target, and 11,352 B of free IRAM. DRAM is
   split 40 % to D1, 10 % each to P1, P2, P3 and P6, 20 % unallocated; all
   free IRAM goes to D1.

   | Branch | DRAM allowance | IRAM allowance | Sockets |
   |---|---|---|---|
   | P1 | 1,148 B | 0 | 1 TCP |
   | P2 | 1,148 B | 0 | 2 UDP (1 discovery, 1 DNS) |
   | P3 | 1,148 B | 0 | 2 UDP |
   | P4 | 0 | 0 | 0 |
   | P6 | 1,148 B | 0 | 2 UDP |
   | D1a, D1b (together) | 4,592 B | 11,352 B | 0 |
   | Unallocated | 2,304 B | 0 | |

   `rust/scripts/check_section_budget.py` enforces the F3 figures with 256 B
   of slack. A branch that uses its allowance moves the script default
   (`--max-dram2`, `--min-stack`, `--max-iram`) by at most that allowance in
   the same pull request and says so in its work-order file. The internal
   heap check (`--min-heap`, 65,536 B) has no allowance. The unallocated
   share is the owner's to assign.

4. Gates: the three host commands and the firmware job in
   `.github/workflows/rust-ci.yml`, all green.
5. Dev-module check as listed for the branch, following section 8. The serial
   log and the section-budget output go in the work-order file under
   `.agents/knowledge/work_orders/`.
6. Parity status moves to "Wired" at merge. Only the owner moves a row to
   "Certified", after checking on BASEMENT.
7. The existing invariants still apply, in particular 47 (boot order),
   48 (budgets) and the three-strikes rule. Invariant 42's confirmation step
   and Invariant 47 clause 3 apply to BASEMENT, not to the development module
   (owner decision, section 4).

## 8. Development-module procedure

- Install route: the module's contents are unknown, so F1 inspects and backs
  it up first and the owner confirms the route before the first write. The
  firmware needs the C layout around it (bootloader, partition table,
  `factory` recovery at `0x10000`, NVS with Wi-Fi credentials), so the
  expected route is to install that layout, let the owner provision Wi-Fi
  once through the C setup page, then write only the Rust app to `ota_0`.
  Where the C bootloader, partition table and recovery image come from is
  part of F1's proposal.
- Wi-Fi credentials are never typed, printed or committed by an agent. They
  come from the module's own NVS, or from `SL_WIFI_PASS` set by the owner in
  the PC's environment.
- A reset during the boot lease drops the module into C recovery. An agent
  that sees recovery after a power cycle checks the timing before calling it
  a firmware failure.
- Only the board whose MAC F1 recorded may be flashed. Before every flash the
  agent runs `espflash board-info --port COM13` and compares the MAC; on a
  mismatch it stops.
- One agent at a time. The lock is a file named `dev-module.lock` in the
  shared git directory (`git rev-parse --git-common-dir`), which every
  worktree on this PC sees and which is never committed. It holds the branch
  name and a timestamp. An agent creates it before flashing and deletes it
  when its check ends. A lock older than two hours may be taken over, and the
  takeover is reported.
- At most two flash attempts per check (Invariant 47). After a second failure
  the agent releases the lock, restores the last known-good `MK5-Rust` image
  and reports.
- Agents must run on the owner's PC to reach `COM13`. Cloud sessions cannot
  perform the check.
- BASEMENT (`172.22.1.15`) is never flashed, rebooted or sent an OTA by any
  agent. `/dev/ttyUSB0` on `pc-1` and `POST /reboot_ota.json` stay forbidden.

## 9. Out of scope

| Gap | Reason |
|---|---|
| Display drivers and routing | No panel on the board; the CYD companion covers display over UART |
| Rotary, buttons, IR, LED, visualiser | No such hardware attached |
| Other DACs and S/PDIF | The board has only the TAS5805M |
| W5500 Ethernet | No module attached |
| Sleep and battery | No battery hardware; the S3 port is a separate project |
| Bluetooth A2DP source | Owner decision: left out; may be planned separately after D1 |
| HTTPS streams, DSD | Not present in the C build, so not parity gaps |
| SqueezeNetwork | Author's call: the hosted service has been shut down, so P2 covers local LMS only. Owner may overrule. |

## 10. Calls made by the author, for the owner to check

- The unguarded runtime settings write stays as it is until P4. Gating it off
  earlier would stop settings persisting between two merges, and it is what
  BASEMENT runs today. F0 corrects the documentation meanwhile, and P4 is the
  first Phase 1 branch to start.
- `runtime-flash-persist` and `esp-rtos-core1` cannot be combined today
  (`compile_error!` at `main.rs:1900`). D1b must give the settings and
  credential writes a way to park Core 1. D1b owns that and must state it in
  its own design.
- The development module is named `DEVBENCH`, through a new compile-time
  name override added in F1.
- Invariant 49 is project-specific, so F2 edits it in this repository's copy
  only.
- softAP (P6) is in scope, because the development module can be taken off
  Wi-Fi without affecting BASEMENT.
- The Spotify facade stays as it is until the P5 study reports.
- F1 adds the standing push and flash permissions from section 4 to
  CLAUDE.md, scoped to this plan's branches and the development module, so
  that agents starting from a fresh session can see them.

## 11. Left to the builder

- The exact trait signatures for the F4 plug-in points.
- Which IRAM functions F3 moves to flash, chosen by measurement.
- The mechanism of the compile-time name override.
- The per-branch allowance numbers, once F3's reclaimed totals are measured.
- Each branch gets its own implementation plan; D1b and P6 are large enough
  to need their own design first.

## 12. Open owner questions

None. The one remaining owner checkpoint is inside F1: confirming the install
route before the first write to the development module.
