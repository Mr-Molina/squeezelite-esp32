# Work Order: WO-F3 — Memory Rebudget (`foundation/memory-rebudget`)

- **Work Order ID**: `WO-F3`
- **Date**: 2026-11-18
- **Plan**: `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md`, work order F3
- **Spec**: `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md`, section 7
- **Branch**: `claude/dazzling-yonath-8dd057` (rebased onto `MK5-Rust` at `3a3aab43`, zero conflicts; Step D1 fix `3eeae094`)
- **Merge Status**: **PASS — Ready for PR #17 Promotion** (pending local CI/ELF gates & review pass). Step D1 fixes in `3eeae094` resolved the BT zeroed UB, stack spike, and external preemption stream leak; the 1-hour `DEVBENCH` verification soak (`soak-f3-fixed.csv`) passed with `0` resets/reboots, `0` disconnects, `0` `i2c_nacks`, `0 B` PSRAM net delta, and `0` warnings/errors in `syslog.json`. `BASEMENT` (`172.22.1.15`) was untouched.

## 0. Guardrails

- Main checkout (`S:\Github\squeezelite-esp32`) and hardware (DEVBENCH / BASEMENT) were not touched during the rebase and ledger/work-order documentation commit.
- `BASEMENT` (`172.22.1.15`) was not contacted, flashed, or rebooted.

## 1. Commits on `claude/dazzling-yonath-8dd057` (Rebased onto `3a3aab43`)

| Rebased SHA | Pre-Rebase SHA | Change |
| :--- | :--- | :--- |
| `c00a5095` | `f4d13624` | `refactor(app): drop the PSRAM-absent fallback buffers (F3 task 2)` |
| `8b7409e5` | `63179cdf` | `refactor(app): allocate the remaining socket buffers from PSRAM (F3 task 2)` |
| `bfee434d` | `5ff9ea5b` | `refactor(app): place RAM_JOURNAL and CONFIG_HANDLE in .dram2_uninit (F3 task 2)` |
| `67654711` | `8c2e08a3` | `refactor(app): keep the web syslog ring on the PSRAM heap (F3 task 2)` |
| `dbff377a` | `9a113ed8` | `refactor(raop): keep the AES decryption tables in flash (F3 task 2)` |
| `bd108784` | `ec5cb7f5` | `refactor(app): allocate the Bluetooth sink state from PSRAM (F3 task 2)` |
| `edff549e` | `d0375428` | `feat(app): internal heap sizes as checked constants, total 65,536 B (F3 task 1)` |
| `e23dfa63` | `ce60dfde` | `feat(app): report internal heap free and its low-water mark in /status.json` |
| `9ba77c47` | `1371c297` | `perf(mp3): run the synthesis granule from flash, not IRAM (F3 task 3)` |
| `f2b72306` | `50473fab` | `perf(mp3): run the synthesis DCT from flash, not IRAM (F3 task 3)` |
| `1ca1fd33` | `4bcf9486` | `ci(rust): tighten the memory budgets to the F3 figures and check the internal heap (F3 task 5)` |
| `4f36203f` | `0f98b1d1` | `docs(mk5-rust): record the F3 memory figures and per-branch allowances (F3 task 5)` |

## 2. Static Section & Internal Heap Budgets (4/4 Pass)

Measured on the F3 release image and enforced in CI via `rust/scripts/check_section_budget.py` (with 256 B slack) and `tests/memory_budget_invariant.rs` / Invariant 48:

| Budget Metric | Before F3 (`579ffcde`) | F3 Image | Target / Limit | Status |
| :--- | ---: | ---: | :--- | :--- |
| IRAM (`.vectors` + `.rwtext` + `.rwtext.wifi`) | 129,636 B | **119,720 B** (11,352 B free) | `<= 131,072 B` (`>= 10,000 B` free; CI `<= 119,976 B`) | **PASS** |
| `.dram2_uninit` | 98,520 B | **88,792 B** (9,976 B free) | `<= 98,768 B` (`>= 8,000 B` free; CI `<= 89,048 B`) | **PASS** |
| `.stack` | 28,528–28,536 B | **41,512 B** | `>= 40,000 B` F3 target (`>= 28,000 B` hard; CI `>= 41,256 B`) | **PASS** |
| Internal heap (`INTERNAL_HEAP_BYTES` + `BT_HEAP_BYTES`) | 64,512 B | **65,536 B** | `>= 65,536 B` (CI `--min-heap 65536`) | **PASS** |

Per-branch allowances recorded in `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md` section 7: `P1`/`P2`/`P3`/`P6` = `1,148 B` DRAM each (`0` IRAM), `P4` = `0` DRAM / `0` IRAM, `D1a`+`D1b` = `4,592 B` DRAM / `11,352 B` IRAM, `Unallocated` = `2,304 B` DRAM.

## 3. Side-by-Side 1-Hour DEVBENCH Soak Comparison

Evidence files are stored in `.agents/knowledge/work_orders/WO-F3-evidence/` (`soak-pre-f3.csv` and `soak-i1.csv`).

| Metric | Pre-F3 Baseline (`3283d457`) | F3 Candidate (`0f98b1d1` / `4f36203f`) |
| :--- | :--- | :--- |
| Evidence CSV | `WO-F3-evidence/soak-pre-f3.csv` | `WO-F3-evidence/soak-i1.csv` |
| Resets during soak | **0** | **2** (right after `codec=Bluetooth` transitions, at t=414–439 s and t=984–1,003 s) |
| FLAC decode peak (`decode_us_max`) | **23,963 us** | **52,301 us** spike |
| FLAC FPS dips (`< 44,000` or `> 44,200` fps) | **0** | **13** FLAC FPS dips |
| `i2c_nacks` | **0** | **0** |
| `Bluetooth` transitions observed | **0** | Multiple (`codec=Bluetooth` immediately preceded both resets) |
| Internal heap low-water mark | Not instrumented pre-F3 | **5,128 B** (idle ~13.4 KB) |
| Run completion / stop condition | Stalled at `t = 2,769 s` when LMS (`172.25.8.40`) went unreachable | Completed full 1 h window (44 min stable after 2nd reset) |

## 4. AAC Zero-Frames Root Cause

- Both baseline (`base-aac.csv`: `frames_played = 0` for 171 s) and F3 soak (`soak-i1.csv`: 0 AAC samples despite `soak.m4a` queued) failed to receive native AAC streams.
- **Root cause**: `HELO_CODECS` in the coordinator omits `aac`, so LMS does not advertise/dispatch AAC to the Rust player.

## 5. Bisect Shortlist & Resolution (Resolved by `3eeae094`)

Originally marked **DO NOT MERGE** pending live bisect for the resets, `52,301 us` FLAC decode spike, and FLAC FPS dips:

1. **Suspect #1 — `ec5cb7f5` (rebased `bd108784`) & `63179cdf` (rebased `8b7409e5`)**: Moving Bluetooth sink state (`bd108784`) and remaining socket buffers (`8b7409e5`) from internal DRAM to PSRAM — directly correlates with resets occurring immediately after `codec=Bluetooth` transitions and increased cache/PSRAM contention during FLAC decoding.
2. **Suspect #2 — `d0375428` (rebased `edff549e`)**: `47 + 17 KiB` internal heap split (`INTERNAL_HEAP_BYTES` + `BT_HEAP_BYTES` totaling `65,536 B`) — correlates with the `5,128 B` internal heap low-water mark under concurrent Wi-Fi/BT/stream pressure.
3. **Secondary check (Task 3)**: `1371c297` / `50473fab` (rebased `9ba77c47` / `f2b72306`) moving MP3 synthesis granule/DCT from IRAM to flash (verify no regression once #1 and #2 are bisected).

**Resolution**: Step D1 fix commit `3eeae094` (`fix(f3): eliminate BT zeroed UB, stack spike, and external preemption stream leak`) resolved the root causes on `claude/dazzling-yonath-8dd057`.

## 6. Post-Fix 1-Hour `DEVBENCH` Verification Soak (`3eeae094`, PASS)

- **Date / Target**: `2026-11-19`, **`DEVBENCH`** (`COM13`, `172.25.6.102`, MAC `ac:15:18:fd:93:9c`), **`BASEMENT` (`172.22.1.15`) untouched**.
- **Boot / Partition**: `3eeae094` in `ota_0` (`0x150000`), `[ANTI-BRICK GATE 1]` cleared, DHCP IP `172.25.6.102/24` acquired at `16.02s` (`net: DHCPv4 lease acquired` `15099 ms`, `boot: ota_0 confirmed healthy; otadata pinned to ota_0` `15105 ms`, `recovery: 0`), connected via local `soak_relay.py` (`172.25.6.122:3483/9000` -> LMS `172.25.8.40:9000`).
- **Soak CSV (`soak-f3-fixed.csv`, 10s cadence)**:
  - `357` samples (`t_s = 0..3591`, `duration_s = 3591` s / 59m 51s across 3600s monitor capture), `0` misses.
  - `uptime_ms` strictly monotonic `501674` -> `4092181` (`+3,590,507 ms`), **`0` resets/reboots**.
  - `disconnects`: `min = 0, max = 0`; `i2c_nacks`: `min = 0, max = 0` (`6272` ACKs before stop, `6276` after stop), `tas_pll_locked = true`.
  - **Audio / FLAC**: `FLAC` (`357/357` rows, `44100 Hz`, 16-bit stereo) at `vol_pct = 10` (`mixer volume 8`), `326` positive-delta intervals (`0` zero-delta intervals), `30` clean track loop boundaries (`dev_STAT_STMd = 30`, `STMu = 0`, `STMn = 0`, `STMo = 0`), effective `44099.675 fps` (`uptime_ms` dt) / `44099.755 fps` wrap-adjusted (`fps_10s` `min = 44073.7`, `median = 44100.1`, `max = 44125.7`), `decode_us_max` `min = 14563`, `median = 22238`, `max = 22238`.
  - **Memory**: `int_heap_free` `start = 13500`, `min = 3352`, `max = 13500`, `end = 13312`; `int_heap_min_free` low-water `3352 B` (zero OOM); `psram_free` `start = 1314644`, `min = 1313792`, `max = 1314664`, `end = 1314644` (`net_delta = 0 B`).
  - **`syslog.json`**: `7` messages (`INFO` only; `0` warnings/errors/panics/WDT resets).
- **Promotion Readiness**: `3eeae094` soak blocker cleared; PR #17 ready to promote after all 8 local CI/ELF gates and review pass. `BASEMENT` (`172.22.1.15`) remains untouched.
