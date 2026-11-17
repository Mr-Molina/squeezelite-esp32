# Work Order: WO-F1 — Dev module "DEVBENCH" bring-up (`foundation/dev-module-bringup`)

- **Work Order ID**: `WO-F1`
- **Date**: 2026-11-17
- **Plan**: `docs/superpowers/plans/2026-11-17-parity-branch-distribution.md`, work order F1
- **Spec**: `docs/superpowers/specs/2026-11-17-parity-branch-distribution-design.md`, section 8
- **Target**: second Louder-ESP32 on `COM13` of the owner's Windows PC (CH340, `USB\VID_1A86&PID_7523`)
- **Status**: Tasks 1 to 3 done. **Task 4 is blocked on the owner confirming the install route in section 5.** Nothing has been written to the module.

## 0. Guardrails

- BASEMENT (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`) was not contacted in any way.
- The only operations run on `COM13` were `espflash board-info` and `espflash read-flash`. No write, no erase.
- The backup may hold Wi-Fi credentials. Its NVS and `settings` contents were not printed or inspected; only whether those regions are blank was computed. The backup is outside the repository and is never committed.

## 1. Identification

Command: `espflash board-info --port COM13 --non-interactive -S` (2026-11-17T14:52Z)

| Item | Value |
| :--- | :--- |
| Chip | `esp32` revision v3.1 (classic ESP32, dual core, 240 MHz, Wi-Fi + BT) |
| Crystal | 40 MHz |
| Flash size | 16 MB (`0x1000000`) |
| MAC | `ac:15:18:fd:93:9c` |
| Security features | None (no flash encryption, no secure boot) |

The MAC is neither BASEMENT's (`c8:2e:18:be:b8:34`) nor the CYD's (`d4:d4:da:5a:e7:3c`). It is recorded as `DEVBENCH_MAC` in `rust/scripts/devbench.py`.

## 2. Backup

Command: `espflash read-flash --port COM13 --non-interactive -S -B 460800 0x0 0x1000000 "%USERPROFILE%\devbench-backup\devbench-original.bin"`

| Item | Value |
| :--- | :--- |
| File | `C:\Users\jmolina\devbench-backup\devbench-original.bin` |
| Size | 16,777,216 B (exactly the flash size) |
| SHA-256 | `aa579ae1375ffba9ee8b1a770da9ee7d791f6fc6ab900caf4d280db4466204c9` |

A first read at 921600 baud failed with `espflash::read_flash::corrupt_data` and left an empty file, which was removed. The read at 460800 baud completed in about 6 min 40 s. Use 460800 or lower on this adapter.

To put the module back exactly as found: `espflash write-bin --port COM13 0x0 "%USERPROFILE%\devbench-backup\devbench-original.bin"`.

## 3. What is on the module

The module is not blank. It carries a complete squeezelite-esp32 C installation, release 1681.

Bootloader header at `0x1000`: valid image (magic `0xE9`), flash mode DIO, 80 MHz, header flash size 4 MB.

Partition table at `0x8000` (MD5 entry valid):

| Name | Type | Subtype | Offset | Size | End | Contents |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `nvs` | data | nvs | `0x9000` | `0x4000` | `0xD000` | not blank (not inspected) |
| `otadata` | data | ota | `0xD000` | `0x2000` | `0xF000` | blank |
| `phy_init` | data | phy | `0xF000` | `0x1000` | `0x10000` | blank |
| `recovery` | app | factory | `0x10000` | `0x140000` | `0x150000` | `Squeezelite-ESP32` version `I2S-4MFlash-1681`, built Sep 12 2024 09:35:13, IDF v4.3.5 |
| `ota_0` | app | ota_0 | `0x150000` | `0x2A0000` | `0x3F0000` | `Squeezelite-ESP32` version `I2S-4MFlash-1681`, built Sep 12 2024 09:35:40, IDF v4.3.5 |
| `settings` | data | nvs | `0x3F0000` | `0x10000` | `0x400000` | not blank (not inspected) |

Flash above `0x400000` (12 MB) is outside the table. 76.65% of the whole flash reads `0xFF`.

`otadata` is blank, so the bootloader starts the `factory` image: the module currently boots into C recovery. Whether Wi-Fi was ever provisioned is not known; `nvs` holds data but was not read.

## 4. Comparison with BASEMENT (`.agents/knowledge/DEVICE_STATE.md`)

| Item | BASEMENT (as recorded) | DEVBENCH (as read) | Same? |
| :--- | :--- | :--- | :--- |
| `nvs` | `0x9000` | `0x9000`, `0x4000` | yes |
| `otadata` | `0xD000` | `0xD000`, `0x2000` | yes |
| `factory` recovery | `0x10000`, C `I2S-4MFlash-1681` | `0x10000`, C `I2S-4MFlash-1681` | yes (same version string) |
| recovery size | `0x150000` (implied by `ota_0` at `0x160000`; matches `partitions.csv`) | `0x140000` | **no** |
| `ota_0` | `0x160000`, `0x290000` | `0x150000`, `0x2A0000` | **no**: 64 KiB lower, 64 KiB larger |
| `settings` | journal sector at `0x3FE000` | partition `0x3F0000..0x400000`, contains `0x3FE000` | yes |
| Chip | ESP32 rev v3.0 | ESP32 rev v3.1 | close |
| Flash | not recorded ("4MFlash" build) | 16 MB, table uses the first 4 MB | n/a |

The one difference is where `recovery` ends and `ota_0` starts. DEVBENCH has the layout of the upstream 1681 release (`flash_cmd.txt` in this repository also puts `squeezelite.bin` at `0x150000`); BASEMENT is recorded with the layout of this repository's `partitions.csv`. BASEMENT's figures come from `DEVICE_STATE.md` and were not re-read from the device.

What the difference means for the Rust firmware, from the source:

- The app does not use the `ota_0` offset to operate. It selects the boot slot by writing an `otadata` entry at `0xD000` (`ota_safety.rs`), and the bootloader finds `ota_0` through the partition table. `0x160000` appears in `http_control.rs` only inside status strings and comments.
- The offsets the firmware does use directly (`0xD000`, `0xE000`, NVS at `0x9000`, settings journal at `0x3FE000`) are identical on both boards.
- The release image is about 2.0 MB and fits both `ota_0` sizes. `devbench.py flash` enforces the smaller, BASEMENT's `0x290000`.
- A DEVBENCH image will report `ota_0_0x160000` in its OTA status text while actually sitting at `0x150000`. That is cosmetic.

## 5. Proposed install route — owner checkpoint

**No write has been made. Task 4 does not start until the owner confirms one of these.**

### Route A (recommended): keep the C layout already on the module

The bootloader, partition table and `factory` recovery image the Rust firmware needs are already there, and the recovery image is the same version as BASEMENT's. Nothing has to be sourced: no release download, no owner-supplied copy, no read-out of BASEMENT.

1. Owner: power the module and check Wi-Fi through the C recovery setup page. If it joins the network already, nothing to do. If it comes up as its own access point, provision it once there. No agent sees the credentials.
2. Agent, under the lock: build with `SL_DEVICE_NAME=DEVBENCH`, then write the app only:

   ```
   python rust/scripts/devbench.py acquire foundation/dev-module-bringup
   espflash save-image --chip esp32 rust/target/xtensa-esp32-none-elf/release/louder-esp32-app <image.bin>
   python rust/scripts/devbench.py flash foundation/dev-module-bringup <image.bin>
   ```

   which runs `espflash board-info --port COM13`, checks the MAC, then `espflash write-bin --port COM13 0x150000 <image.bin>`. This overwrites the C squeezelite app in `ota_0`; recovery stays.
3. Make the bootloader start `ota_0`. `otadata` is blank, so after step 2 the module still boots recovery. Two ways, owner picks:
   - **3a (recommended):** the agent writes one 32-byte select entry for `ota_0` (the bytes of `OtaSelectEntry::new_ota_0()`, padded to one `0x1000` sector with `0xFF`): `espflash write-bin --port COM13 0xD000 <otadata-ota0.bin>`. Needs no network action.
   - **3b:** the owner presses the "exit recovery" control on DEVBENCH's own C recovery page.
4. Roll-back at any time: write the backup from section 2 back to `0x0`.

Cost of Route A: DEVBENCH's `ota_0` sits 64 KiB lower than BASEMENT's. From the source this does not change firmware behaviour (section 4), but the two boards are then not byte-identical in layout.

### Route B: repartition to BASEMENT's recorded layout first

Write a partition table generated from this repository's `partitions.csv` (`recovery` `0x150000`, `ota_0` at `0x160000`) to `0x8000`, leave bootloader and recovery where they are (recovery is smaller than both sizes), then continue as Route A with `0x160000`:

```
espflash write-bin --port COM13 0x8000 <partitions.bin built from partitions.csv>
espflash write-bin --port COM13 0x160000 <image.bin>
```

and set `OTA_0_OFFSET = 0x160000` in `rust/scripts/devbench.py`. This gives the same offsets as BASEMENT. It costs one extra write to a boot-critical sector and needs `partitions.bin` built (ESP-IDF `gen_esp32part.py`, not installed on this PC as far as was checked). The old C app at `0x150000` would straddle the new boundary and is overwritten anyway.

### Not proposed

Reinstalling the whole C layout from a release archive. It would replace files that are already present and correct, and the upstream 1681 release exists in a 16-bit and a 32-bit build with no way to tell from here which one BASEMENT carries.

### Open points for Task 4

- `devbench.py flash` writes only `ota_0`. The Rust firmware erases `otadata` early in boot and pins `ota_0` after the boot lease, so a failed image leaves the module booting recovery, and the next serial flash will not start by itself. Task 4 should decide whether `flash` also rewrites the `ota_0` select entry (step 3a) each time.
- Confirm on the module that the lease fall-back and the way back behave as on BASEMENT (plan Task 4 Step 4).

## 6. Code delivered in this run

| Commit | Content |
| :--- | :--- |
| `feat(app): compile-time SL_DEVICE_NAME override for the device name` | `board::first_boot_defaults::PLAYER_NAME` from `option_env!("SL_DEVICE_NAME")`, default `"BASEMENT"`; literals replaced in `main.rs`, `bt_a2dp.rs`, `config_handle.rs`; `tests/device_name_invariant.rs` |
| `feat(tooling): dev-module lock and guarded flash helper` | `rust/scripts/devbench.py`, `rust/scripts/test_devbench.py`, one step in the host CI job |
| `fix(tooling): serialize dev-module lock changes and cover http_control name fallbacks` | fixes from an independent review of the two commits above |
| (this file's commit) | this work order; `OTA_0_OFFSET` set to the offset read from the module |

Decisions taken where the plan was silent or wrong:

- **`OTA_0_OFFSET = 0x150000`, not `0x160000`.** The plan says "the `ota_0` offset recorded in Task 2"; the module's table says `0x150000`. Wrong only if the owner picks Route B, and then it is a one-line change.
- **Three tests beyond the plan's seven** (`ReviewFixTests`), so the suite reports 10, not 7. They pin the review fixes.
- **Lock changes run under a guard file** (`dev-module.lock.guard`, held for milliseconds). Exclusive-create alone, as the plan asks, did not stop two agents both taking over the same stale lock; the review reproduced that.
- **The two-attempt count belongs to one lock.** `release` then `acquire` starts a new check with a new count. The spec says "at most two flash attempts per check", so this is read as intended; an agent that releases and re-acquires to get a third try is breaking the procedure, not the script.
- **`http_control.rs` name fallbacks** (`host_name` default and the Spotify remote name) were not in the plan's list and were replaced as well, because a DEVBENCH image would otherwise still call itself BASEMENT there.
- **`SL_DEVICE_NAME` must be exactly 8 bytes.** The Bluetooth SDP record in `bt_a2dp.rs` has a compile-time assert on the name length. `DEVBENCH` is 8 bytes; any other length fails the build, it is not truncated.

Known and left alone:

- `"BASEMENT"` remains in static text that cannot take a `const`: the `cfg-syst-name` value in the `/commands.json` template (`http_control.rs`) and four HTML input defaults in `web_ui.rs`. A DEVBENCH image shows `BASEMENT` in those until live values replace them.
- Tests in `board.rs` and `bt_a2dp.rs` assert the literal `BASEMENT` and fail if `cargo test` is run with `SL_DEVICE_NAME` set. The gates run with it unset.

## 7. Gates (2026-11-17, run from `rust/`, `SL_DEVICE_NAME` unset)

| Gate | Result |
| :--- | :--- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass, 40 test binaries, 0 failed |
| `cargo +esp clippy -p louder-esp32-app -p cyd-controller-app --target xtensa-esp32-none-elf ...` | pass |
| `cargo +esp check -p louder-esp32-app --features esp-rtos-core1 ...` | pass |
| `cargo +esp build --release -p louder-esp32-app ...` | pass |
| `python scripts/check_section_budget.py ...` | pass: IRAM 129,636 B (limit 131,072), `.dram2_uninit` 98,520 B (limit 98,768), `.stack` 28,528 B (floor 28,000) |
| `python -m unittest discover -s rust/scripts -p "test_*.py"` | 10 tests, OK |
| `SL_DEVICE_NAME=DEVBENCH cargo check -p louder-esp32-app` | compiles |

The firmware gates were run on the commit before this file and the `OTA_0_OFFSET` change, which touch no Rust code. There is no dev-module check in this run: nothing has been flashed.
