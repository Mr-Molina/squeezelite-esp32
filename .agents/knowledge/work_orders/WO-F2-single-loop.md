# Work Order: WO-F2 — One firmware loop (`foundation/single-loop`)

- **Work Order ID**: `WO-F2`
- **Date**: 2026-11-18
- **Plan**: `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md`, work order F2
- **Spec**: `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md`, sections 7 and 8
- **Branch**: `claude/blissful-noether-5d05cd`, stacked on F1 (`claude/foundation-dev-module-bringup-a01186`, PR `#7`, tip `aed5c388`) because `origin/MK5-Rust` did not yet hold `rust/scripts/devbench.py`. Must merge after PR `#7`.
- **Status**: Tasks 1 to 3 done. The dev-module check covered boot, network, HTTP status and the `otadata` pin. It did **not** cover an LMS stream, AirPlay or Bluetooth (section 5). Only the owner merges.

## 0. Guardrails

- BASEMENT (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`) was not contacted in any way.
- DEVBENCH was written only through `rust/scripts/devbench.py`, under the lock, one flash attempt (of two allowed). Only the app in `ota_0` at `0x150000` was written. `select-ota0` was not needed.
- No Wi-Fi credential was typed, printed or committed. The serial capture withholds any line matching a credential pattern before it reaches disk; one line was withheld (the same one F1 saw, right after the Gate 1 line).

## 1. What changed

| Commit | Change |
| :--- | :--- |
| `705a83a9` `refactor(app): remove the unreachable core-0 task ring from tasks.rs` | `tasks.rs` goes from 2,031 to 191 lines. Kept: `StreamParams`, `CoordinatorCmd`, `AudioEvent`, `CTRL`, `EVT`, `CHANNEL_DEPTH`, `device::core1_heartbeat`, `device::run_core1_executor`, and two tests. Removed: `RingTask`, `Phase`, `PHASE_ORDER`, `FirmwareState`, `Shared`, every ring block, `run_core0_executor`. `tests/no_cfg_invariant.rs` changed first (both new assertions seen failing, then passing). |
| `a4d19c47` `refactor(app): use the library modules instead of second copies in main.rs` | The six `mod` lines (`console`, `controls`, `dac_platform`, `display_gds`, `eth_w5500`, `power_boards`) are gone from `main.rs`. |
| Task 3 commit | Invariant 49 clauses 1 and 3, `docs/mk5-rust-todo.md` section 1, this file, the session entry. |

## 2. Decisions (where the plan was silent or did not fit)

1. **Base branch.** `origin/MK5-Rust` had no `rust/scripts/devbench.py` on 2026-11-18, so the work is based on F1's branch tip `aed5c388`. F1 branched before PR `#6` merged, so this branch does not contain `#6` either; it is a docs-only change and does not touch the files changed here.
2. **`mod device` gate.** The plan keeps `device::core1_heartbeat` and `device::run_core1_executor`. Both exist only under `esp-rtos-core1`, so the whole `mod device` is now gated on `all(target_arch = "xtensa", feature = "esp-rtos-core1")` instead of carrying a per-item gate inside an otherwise empty module. The default build compiles nothing from it. The `esp-rtos-core1` check gate passes.
3. **`test_wo255_amp_health_consecutive_fault_throttling`: deleted.** It asserted on local arithmetic that mirrored the removed `tasks.rs::amp_health` (`CONSECUTIVE_FAULTS`, `LAST_RECOVERY_MS`); it called no production code. `main.rs` has its own cooldown in a different form (`now_ms.saturating_sub(s.last_fault_recovery_ms) >= 30_000`), which this test never exercised.
4. **`test_wo255_bt_source_arbiter_priority_transitions`: kept.** It tests the library's `SourceArbiter`, which is untouched.
5. **The four phase-ring tests: deleted** (the plan says three; there were four: discriminants, DMA-first order, lap and wrap, ring-task ownership). All four test only the removed `Phase`/`RingTask` types.
6. **Task 2 needed no imports.** `main.rs` referred to none of the six modules by path, so the plan's `use louder_esp32_app::console;` lines would have been unused imports. Only the `mod` lines were deleted.
7. **Comments that pointed at removed code** were corrected in the same commit as Task 1, beyond the one the plan names (`main.rs:1885`): `Cargo.toml` (two feature comments), `main.rs` (`run_firmware_tick` doc), `coordinator.rs` (one test comment), `config_handle.rs` and `net_device.rs` (module docs). Comment-only; `no_cfg_invariant` still passes.
8. **Invariant 49 clause 3.** The plan says to drop the twin-path requirement. The `tasks.rs`/`main.rs` twin is gone, but `StaticMp3Decoder` and `Mp3Decoder` both still exist in `squeezelite-decoders`, so the clause keeps that half: a fix to one is audited against the other. The title's "Twin-Path Parity" became "Single Device Path". Clause 2 and the throughput and latency figures of clause 1 are unchanged word for word. Historical copies of the old text in `WO-278` were left as they are.
9. **Image size: 32 B larger, not "no larger".** See section 4. Accepted and reported instead of chased: no budgeted section grew.
10. **Lock name.** The lock was taken as `foundation/single-loop` (the plan's work-order name), as F1 did with its own, not the git branch name.
11. **Reset for the serial capture.** `devbench.py` has no reset-and-capture command. The boot log was taken with a one-off pyserial script (RTS pulse on `COM13`, 115200 baud, credential filter) while holding the lock, after `/status.json` showed the previous boot past DHCP with `otadata` pinned (`otadata_prearmed_factory: false`, `uptime_ms` 6,170), so outside the early-reset window. A `capture` subcommand in `devbench.py` would make this repeatable for later work orders; not added here (out of F2's scope).

## 3. Gates (2026-11-18, run from `rust/`, `SL_DEVICE_NAME` unset)

Run in full three times: on the Task 1 tree, on the Task 2 tree (`a4d19c47`), and on the final tree after the comment corrections of section 6. Same result each time except the host test count. The DEVBENCH image rebuilt from the final tree was compared with the one that was flashed (section 5).

| Gate | Result |
| :--- | :--- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass, 40 test binaries, 0 failed; 856 tests after Task 1, 828 after Task 2 (the bin target no longer runs second copies of the six modules' tests; the library still runs them) |
| `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` | pass |
| `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` | pass (`run_core1_executor` survived) |
| `cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` | pass |
| `python scripts/check_section_budget.py target/xtensa-esp32-none-elf/release/louder-esp32-app` | pass: IRAM 129,636 B (limit 131,072, headroom 1,436), `.dram2_uninit` 98,520 B (limit 98,768, headroom 248), `.stack` 28,536 B (floor 28,000, headroom 536) |
| `python -m unittest discover -s rust/scripts -p "test_*.py"` (repository root) | 11 tests, OK |

Red before green (Task 1 Step 2), `cargo test -p louder-esp32-app --test no_cfg_invariant` before the deletion:

```
---- tasks_has_no_core0_executor stdout ----
tasks.rs still contains `run_core0_executor`
---- runtime_flash_persist_gate_only_in_main stdout ----
tasks.rs gates on runtime-flash-persist
test result: FAILED. 3 passed; 2 failed
```

## 4. Size against the F1 baseline

F1's tip (`aed5c388`) was rebuilt in this worktree so that both builds embed the same source paths. `xtensa-esp32-elf-size -A`, default device name:

| Section | F1 `aed5c388` | F2 `a4d19c47` | Change |
| :--- | ---: | ---: | ---: |
| IRAM (`.vectors` + `.rwtext` + `.rwtext.wifi`) | 129,636 | 129,636 | 0 |
| `.dram2_uninit` | 98,520 | 98,520 | 0 |
| `.data` | 22,264 | 22,256 | -8 |
| `.bss` | 76,880 | 76,880 | 0 |
| `.stack` | 28,528 | 28,536 | +8 |
| `.text` (flash) | 1,564,633 | 1,564,681 | +48 |
| `.rodata` (flash) | 244,288 | 244,288 | 0 |
| `espflash save-image` output | 2,007,104 | 2,007,136 | +32 |

The plan expected the release image to be no larger. It is 32 B larger (48 B of flash `.text`, 0.0008 % of the `ota_0` partition), and DRAM is 8 B smaller, which shows up as 8 B more stack. The whole change arrives with Task 1; Task 2 changes the image bytes but not its size. The removed code was already dropped by the linker, so nothing large could come back. Why `.text` moved by 48 B was not established; removing 1,800 lines from the crate changes how the compiler partitions and inlines the rest, which is the likely cause, not a confirmed one. Flash `.text` has no limit in `check_section_budget.py`.

## 5. Dev-module check on DEVBENCH (2026-11-18)

| Item | Value |
| :--- | :--- |
| Lock | `python rust/scripts/devbench.py acquire foundation/single-loop`, released at the end (lock file confirmed gone) |
| Before | `GET /status.json`: `I2S-4MFlash-Rust`, `recovery: 0`, `uptime_ms` 83,273,807 (F1's image, up about 23 h) |
| Build | `SL_DEVICE_NAME=DEVBENCH cargo +esp build --release -p louder-esp32-app --target xtensa-esp32-none-elf -Zbuild-std=core,alloc` at `a4d19c47`; budget identical to section 3 |
| Image | `rust/target/devbench/app.bin`, 2,007,136 B, SHA-256 `ae9792187acae61c5d897a4fd511b6cc05e94a0e3bb5b35dd63e658c07565c74` |
| Flash | `python rust/scripts/devbench.py flash foundation/single-loop rust/target/devbench/app.bin`: MAC `ac:15:18:fd:93:9c`, `flash attempt 1 of 2 ... -> COM13 @ 0x150000`, `Binary successfully written to flash!` |
| Final tree | The image rebuilt from the final tree (after the comment corrections of section 6) has the same SHA-256 as the one flashed, so the module runs the code of this branch's tip |
| First boot after flash | Rust image, `recovery: 0`, `otadata_prearmed_factory: false`; `select-ota0` not needed |

Boot log, serial, 115200 baud, 45 s captured after a reset pulse. Left column is seconds since the pulse; bootloader lines arrive in one burst. Nothing was printed after the DHCP line.

```
   0.433 ets Jul 29 2019 12:21:46
   0.433 
   0.433 rst:0x1 (POWERON_RESET),boot:0x13 (SPI_FAST_FLASH_BOOT)
   0.433 configsip: 0, SPIWP:0xee
   0.433 clk_drv:0x00,q_drv:0x00,d_drv:0x00,cs0_drv:0x00,hd_drv:0x00,wp_drv:0x00
   0.433 mode:DIO, clock div:1
   0.433 load:0x3fff0030,len:6424
   0.433 load:0x40078000,len:15632
   0.433 load:0x40080400,len:4348
   0.433 entry 0x40080688
   0.433 I (27) boot: ESP-IDF v4.3.5-dirty 2nd stage bootloader
   0.433 I (27) boot: compile time 09:34:07
   0.433 I (27) boot: chip revision: v3.1
   0.433 I (31) qio_mode: Enabling default flash chip QIO
   0.433 I (36) boot.esp32: SPI Speed      : 80MHz
   0.433 I (41) boot.esp32: SPI Mode       : QIO
   0.433 I (45) boot.esp32: SPI Flash Size : 4MB
   0.433 I (50) boot: Enabling RNG early entropy source...
   0.433 I (55) boot: Partition Table:
   0.433 I (59) boot: ## Label            Usage          Type ST Offset   Length
   0.433 I (66) boot:  0 nvs              WiFi data        01 02 00009000 00004000
   0.433 I (74) boot:  1 otadata          OTA data         01 00 0000d000 00002000
   0.433 I (81) boot:  2 phy_init         RF data          01 01 0000f000 00001000
   0.433 I (89) boot:  3 recovery         factory app      00 00 00010000 00140000
   0.433 I (96) boot:  4 ota_0            OTA app          00 10 00150000 002a0000
   0.433 I (104) boot:  5 settings         WiFi data        01 02 003f0000 00010000
   0.433 I (111) boot: End of partition table
   0.433 I (115) esp_image: segment 0: paddr=00150020 vaddr=3f400020 size=46150h (287056) map
   0.433 I (203) esp_image: segment 1: paddr=00196178 vaddr=3ffc0000 size=06438h ( 25656) load
   0.433 I (211) esp_image: segment 2: paddr=0019c5b8 vaddr=40080000 size=03a60h ( 14944) load
   0.433 I (216) esp_image: segment 3: paddr=001a0020 vaddr=400d0020 size=17e00ch (1564684) map
   1.011 I (648) esp_image: segment 4: paddr=0031e034 vaddr=40083a60 size=1c004h (114692) load
   1.011 I (702) boot: Loaded app from partition at offset 0x150000
   1.011 I (702) boot: Disabling RNG early entropy source...
   1.011 [ANTI-BRICK GATE 1] otadata (0xD000..0xF000) erased & verified 0xFF (Factory recovery pre-armed)
   1.011 [one line withheld by the capture filter: it matched the credential pattern]
   1.403 [HIL 1/5..5/5] Hardware Peripherals (GPIO33/34, I2C0 0x2D, I2S0 26/25/22, UART2) Initialized
   4.922 [DHCPv4] Acquired IP 172.25.6.102/24
```

Against F1's baseline (`WO-F1` section 8.4): same bootloader and partition table, `Loaded app from partition at offset 0x150000`, the Gate 1 line, the HIL line at 1.40 s (F1: 1.41 s), DHCP on the same address at 4.92 s (F1: 3.80 s; DHCP time varies with the network). Segment 3 is `17e00ch` (1,564,684 B) against F1's `17dfdch`, which is the 48 B of section 4.

`GET /messages.json` at `uptime_ms` 62,271:

```
  599 WARN net: DHCPv4 lease deconfigured
 3995 INFO net: DHCPv4 lease acquired
 4002 INFO boot: ota_0 confirmed healthy; otadata pinned to ota_0
62271 INFO psram_free=1441164 psram_used=2753140 core1_active=true core1_loops=208984 decode_us_max=0
```

`GET /status.json` at `uptime_ms` 62,166:

| Field | F2 | F1 baseline |
| :--- | :--- | :--- |
| `version` / `recovery` / `rust_engine` | `I2S-4MFlash-Rust` / `0` / `true` | same |
| `otadata_prearmed_factory` | `false` | `false` |
| `psram_free_bytes` + `psram_used_bytes` | 1,441,164 + 2,753,140 = 4,194,304 | 1,441,164 + 2,753,140 |
| `core1_active` / `core1_loops` | `true` / 208,630 at 62.2 s | `true` / 215,151 at 64.0 s (same rate, about 3,360 per second) |
| `tas_pll_locked` / `i2c_nacks` | `true` / 0 | `true` / 0 |
| `disconnect_count` / `rssi` | 0 / -67 dBm | 0 / -70 dBm |
| `host_name` in `/config.json` | `DEVBENCH` | `DEVBENCH` |
| `vol_pct` / `tas_automuted` / `frames_played` | 80 / `true` / 0 | 80 / `true` / not recorded |

**Not checked, and why.** The plan's check also lists an LMS stream, AirPlay and a Bluetooth sink session. None was run:

- **LMS stream.** From this PC, `10.0.0.50:9000` (the LMS) and `10.0.0.10:9000` (the module's first-boot LMS target) both timed out. The module is on `172.25.6.0/24` with no route to the LMS, as on 2026-11-17. No workaround was attempted and the module's LMS setting was not changed. `frames_played` stayed 0, so the decode and output path of this image has not produced audio on hardware.
- **AirPlay and Bluetooth sink.** Both need a person with a sending device next to the module. Not run.
- **Internal-heap figure.** The firmware still prints PSRAM figures only (F3 Task 1).

What the check does show: the image boots from `ota_0`, passes Gate 1, initialises the amplifier and I2S, joins Wi-Fi, takes a lease, pins `otadata`, serves HTTP, and core 1 runs its heartbeat at the baseline rate. F1 left the same three audio items open for the same reasons, so the F2 result is equal to the F1 baseline on everything the baseline holds.

## 6. Review

One independent whole-branch review (fresh context, read-only) ran against `aed5c388..a4d19c47` and the Invariant 49 edit. No critical finding: at `aed5c388` the only use of `tasks` in `main.rs` was `run_core1_executor`, the six modules' tests all still run in the library target, and Invariant 49 clause 2 is byte-identical.

Fixed in the Task 3 commit (comments only): the review found that three comments re-pointed by Task 1 credited `main.rs` with rules it does not implement (`Cargo.toml` `runtime-flash-persist`, `Core1Ctl::unpark_after_flash_write`, the doc above `CONFIG_HANDLE_SRC` in `no_cfg_invariant.rs`), plus two stale phrases (`CTRL` doc in `tasks.rs`, "`config_writer` task" in `config_handle.rs`). They now describe `main.rs` as it is.

Findings about `main.rs` itself. All three were already true before F2 (the code that differed never ran) and F2 does not change device behaviour; they are recorded here because the removed file was the only place the other variant was written down. Last copy: `git show aed5c388:rust/crates/louder-esp32-app/src/tasks.rs`. **Each needs an owner decision; none was acted on.**

1. **Settings are written to flash at runtime in every build, with no play-state gate.** The 100 ms config poll at the end of the device loop in `main.rs` parks APP_CPU, erases and writes the settings sector whenever the debounced journal has a pending image, discards the erase/write results and calls `mark_flash_written()` unconditionally. `runtime-flash-persist` only selects the target sector. The removed `runtime_flash_commit` skipped the write while playing or while Bluetooth audio was active, marked the journal written only on success and retried after 60 s; the old `Cargo.toml` comment said the default build writes flash only before a reboot (WO-108/WO-109). The code and that statement disagree; which one is intended is the owner's call.
2. **External sources start without flushing the stream buffer on the device.** The removed `bt_poll` called `begin_external_playback_with_pipeline` (flushes `pipeline.stream_buffer`, LOGIC-206); `main.rs` calls plain `begin_external_playback()` at its five call sites. Whether `main.rs` omits the flush on purpose (a suspended LMS stream can resume) was not established.
3. **Amplifier fault recovery in `main.rs` has no test.** `main.rs` uses a flat 30 s between recoveries (and none in the first 30 s after boot); the removed code retried up to three times before the cooldown. The deleted `test_wo255_amp_health_consecutive_fault_throttling` tested neither. The cooldown sits in Xtensa-only code, so a host test needs a small extracted helper.

Minor, left as they are: `PlayerCoordinator::take_crossed_rate` now has no caller (the rate switch is applied inside `coordinator.tick`); `test_wo255_bt_source_arbiter_priority_transitions` would sit better in `coordinator.rs`; `tasks_has_no_core0_executor` is a string check, so it catches the old ring coming back, not a rewrite under new names.
