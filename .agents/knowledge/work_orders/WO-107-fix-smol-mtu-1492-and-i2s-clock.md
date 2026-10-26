# Work Order: WO-107

- **Work Order ID**: `WO-107`
- **Timestamp**: `2026-10-25T18:40:00-04:00`
- **Parent Agent**: `Orchestrator (31deac42-e161-443d-9205-80c67d7541ab)`
- **Assigned Subagent Role**: `Rust App MTU & I2S Clock Implementer` (`self`, `Model: "inherit"`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Task Goal
Remediate the two root causes isolated during `WO-106` live hardware verification on `172.22.1.15` (`"BASEMENT"`):
1. **`GET /` Slice Out-of-Bounds Panic (`1500 > 1492` `esp-radio` `WIFI_MTU` Mismatch)**:
   - `esp-radio-1.0.0-beta.1` (`src/wifi/mod.rs:117, 2019-2020`) sets `const MTU: usize = 1492` (`ESP_RADIO_CONFIG_WIFI_MTU`) and `WifiTxToken::consume_token(self, len: usize, f: F)` indexes `let mut buffer: [u8; 1492] = [0u8; 1492]; let buffer = &mut buffer[..len];`.
   - `SmolDevice::capabilities()` in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs:539` sets `caps.max_transmission_unit = 1500`, so when `GET /` (`4,827` B `DASHBOARD_HTML`) fills `STATIC_HTTP_TX_BUF` (`[u8; 1536]`), `smoltcp` dispatches a full-MSS Ethernet frame of `1,500` bytes (`1,446` B TCP payload + `54` B headers) and panics on `&mut buffer[..1500]` (`slice_index_fail` at `0x4011a764`).
2. **Initial `I2S0` `44,100 Hz` Fractional Clock Divider `(14, 11, 63)` & `Tas5805mTelemetry::is_pll_locked`**:
   - `Esp32HardwareI2sDma::new(transfer, 44_100)` (`src/main.rs:714`) initializes `self.sample_rate_hz = 44_100` without programming `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`), leaving `esp-hal`'s integer divider `(14, 0, 1)` (`44,642.86 Hz`, `+1.23%` error) active at boot so `TAS5805M` mis-detects `44.64 kHz` as `48000` Hz (`FS_MON = 0x09`) and reports `CLKDET_STATUS = 0x00`.

## 2. Exact File Modifications Required

### A. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`
1. **`SmolTxToken::consume` & `SmolDevice::capabilities()` (`src/main.rs:515-543`)**:
   - Define `const WIFI_MAX_FRAME_LEN: usize = 1492;` (matching `esp-radio`'s `ESP_RADIO_CONFIG_WIFI_MTU = 1492`).
   - In `SmolTxToken::consume` (`src/main.rs:519`), clamp `self.0.consume_token(len.min(WIFI_MAX_FRAME_LEN), f)`.
   - In `SmolDevice::capabilities()` (`src/main.rs:539`), set `caps.max_transmission_unit = WIFI_MAX_FRAME_LEN;` (`1492`), so `smoltcp` computes `ip_mtu = 1478` and TCP `local_mss = 1438` and never generates an Ethernet frame exceeding `1,492` bytes.
2. **`Esp32HardwareI2sDma::new` & `ensure_running` (`src/main.rs:708-738`)**:
   - In `Esp32HardwareI2sDma::new`: initialize `sample_rate_hz: 0` on `let mut driver = Self { transfer: Some(transfer), sample_rate_hz: 0, tx_enabled: true };`, call `let _ = driver.set_clock_rate(sample_rate_hz);`, and return `driver` so `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) is programmed with fractional divider `(14, 11, 63)` (`44,099.99 Hz`) immediately upon construction.
   - In `Esp32HardwareI2sDma::ensure_running`: if `i2s_tx.write(new_buf)` restarts the transfer (`self.transfer = Some(new_transfer);`), re-apply the fractional clock divider:
     ```rust
     let rate = self.sample_rate_hz;
     self.sample_rate_hz = 0;
     let _ = self.set_clock_rate(rate);
     ```
3. **Pre-fill `tx_buffer` with `2,048` B of Silence & Construct `Esp32HardwareI2sDma` Before `amp.initialize(32)` (`src/main.rs:1301-1335, 1456-1473`)**:
   - At `src/main.rs:1301`:
     ```rust
     let mut tx_buffer = esp_hal::dma_tx_stream_buffer!(8192, 1024);
     let _ = tx_buffer.push(&[0u8; 2048]);
     ```
   - Immediately after `let i2s_transfer = i2s_tx.write(tx_buffer).expect("Failed to start I2S0 TX streaming DMA");`, construct `let hw_i2s_driver = Esp32HardwareI2sDma::new(i2s_transfer, 44_100);` so `I2S0` emits a fractional `44,100.00 Hz` clock before `pwdn.set_high()` and `amp.initialize(32)`.
   - At `src/main.rs:1456-1473`, move the initial `if let Ok(telem) = amp.read_telemetry() { latest_telemetry = telem; }` call to immediately AFTER `let _ = amp.start_playback_via_hiz();` so `latest_telemetry` is read when `amp` is in `Play` mode.

### B. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`
1. **`Tas5805mTelemetry::is_pll_locked` (`src/lib.rs:226-228`)**:
   - Update `is_pll_locked` so it returns `true` either when bit 3 (`0x08`) of `CLKDET_STATUS` (`0x39`) is set OR when `CLKDET_STATUS` bits `2:0` (`0x07`) report zero clock faults while `FS_MON` (`0x37`) reports a valid sample rate (`detected_sample_rate_hz() > 0`) and `BCK_MON` (`0x38`) reports a valid BCK ratio (`>= 32`):
     ```rust
     #[must_use]
     pub const fn is_pll_locked(&self) -> bool {
         (self.clkdet_status & 0x08) != 0
             || ((self.clkdet_status & 0x07) == 0
                 && self.detected_sample_rate_hz() > 0
                 && self.bck_mon >= 32)
     }
     ```
   - Add a unit test in `squeezelite-tas58xx/src/lib.rs` verifying `is_pll_locked()` for:
     - `Tas5805mTelemetry::default()` -> `false`
     - `Tas5805mTelemetry { fs_mon: 0x08, bck_mon: 64, clkdet_status: 0x00, automute_state: 0x00 }` -> `true` (`detected_sample_rate_hz() == 44_100`)
     - `Tas5805mTelemetry { fs_mon: 0x08, bck_mon: 64, clkdet_status: 0x01, automute_state: 0x00 }` -> `false` (clock error bit set)

## 3. Mandatory `--stage-only` Verification Gates
1. `cargo test --workspace` on `x86_64-unknown-linux-gnu` (`0` failures).
2. `cargo clippy --workspace --all-targets -- -D warnings` (`0` warnings).
3. `cargo +esp build --release --target xtensa-esp32-none-elf -p louder-esp32-app --features esp32-hw`.
4. Verify via `xtensa-esp32-elf-readelf -S` and `xtensa-esp32-elf-objdump -d`:
   - `.stack >= 35,000` B
   - `run_hardware_firmware` stack frame (`entry sp, N`) `<= 9,000` B
   - `InterfaceInner::dispatch_ip<SmolTxToken>` has NO `slice_index_fail` branch triggered for `1492` when `caps.max_transmission_unit == 1492`.
5. Export image with `/home/console-admin/.cargo/bin/espflash save-image --chip esp32 --flash-size 4mb` to `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` and run `/usr/bin/python3 /home/console-admin/Github/squeezelite-esp32/scripts/verify_staged_ota_bin.py /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`.
6. Commit the changes cleanly on `MK5-Rust` and report the new commit hash and `.bin` SHA-256.
7. **DO NOT flash `172.22.1.15`** (strictly `--stage-only`; `WO-108` `Extension Certifier` will perform the flash).

## 4. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `dec55808e0e75c8daabb6c9c9664c44d523e6292` (`dec55808`)
- **Rule 35 Pre-Modification Backups**:
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/main.rs.bak`
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/lib.rs.bak`
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/WO-107-fix-smol-mtu-1492-and-i2s-clock.md.bak`
- **Summary of Changes**:
  1. **`SmolDevice::capabilities()` & `SmolTxToken::consume` (`src/main.rs:505-546`)**:
     - Defined `const WIFI_MAX_FRAME_LEN: usize = 1492;` matching `esp-radio`'s `ESP_RADIO_CONFIG_WIFI_MTU = 1492`.
     - Clamped `self.0.consume_token(len.min(WIFI_MAX_FRAME_LEN), f)` in `SmolTxToken::consume`.
     - Set `caps.max_transmission_unit = WIFI_MAX_FRAME_LEN;` (`1492`) in `SmolDevice::capabilities()` so `smoltcp` computes `ip_mtu = 1478` and TCP `local_mss = 1438` and never generates an Ethernet frame exceeding `1,492` bytes.
  2. **`Esp32HardwareI2sDma::new` & `ensure_running` (`src/main.rs:711-746`)**:
     - Initialized `sample_rate_hz: 0` in `Esp32HardwareI2sDma::new` and called `let _ = driver.set_clock_rate(sample_rate_hz);` before returning `driver` so `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) is programmed with fractional divider `(14, 11, 63)` (`44,099.99 Hz`) immediately at boot.
     - Re-applied `set_clock_rate` inside `ensure_running()` when `i2s_tx.write(new_buf)` restarts the transfer.
  3. **Pre-fill `tx_buffer` with `2,048` B of Silence & Early `Esp32HardwareI2sDma::new` Construction (`src/main.rs:1305-1337, 1465-1481`)**:
     - Pre-filled `tx_buffer` with `2,048` B of silence (`let _ = tx_buffer.push(&[0u8; 2048]);`) before `i2s_tx.write(tx_buffer)`.
     - Constructed `Esp32HardwareI2sDma::new(i2s_transfer, 44_100)` before `pwdn.set_high()` / `amp.initialize(32)`.
     - Moved the initial `amp.read_telemetry()` call to immediately after `amp.start_playback_via_hiz()`.
  4. **`Tas5805mTelemetry::is_pll_locked` (`rust/crates/squeezelite-tas58xx/src/lib.rs:226-234, 1237-1256`)**:
     - Updated `is_pll_locked()` to return `true` when `(self.clkdet_status & 0x08) != 0 || ((self.clkdet_status & 0x07) == 0 && self.detected_sample_rate_hz() > 0 && self.bck_mon >= 32)`.
     - Added unit test `test_telemetry_pll_locked_zero_fault_and_error_bits` covering default (`false`), zero-fault `44.1 kHz` (`fs_mon = 0x08`, `bck_mon = 64`, `clkdet_status = 0x00` -> `true`), and faulted `clkdet_status = 0x01` (`false`).
- **`--stage-only` Verification Gates**:
  - `cargo test --workspace`: `0` failures (`259` tests passed across workspace).
  - `cargo clippy --workspace --all-targets -- -D warnings`: `0` warnings (`exit code 0`).
  - `cargo +esp build --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app`: `exit code 0`.
  - `xtensa-esp32-elf-readelf -S`: `.stack = 0x008bd0` (`35,792` B `>= 35,000` B), `.data = 0x003dd0` (`15,824` B `<= 20,000` B), `.bss = 0x023440` (`144,448` B), `.dram2_uninit = 0x004990` (`18,832` B).
  - Disassembly Verification (`verify_wo107_elf.py`):
    - `run_hardware_firmware` (`0x4011d324`, size `42,841` B): `entry a1, 8480` (`0x2120` B `<= 9,000` B).
    - `InterfaceInner::dispatch_ip<SmolTxToken>` (`0x4011a2d0`, size `1,304` B): `entry a1, 1648`, `call8 slice_index_fail` count = **`0`** (statically eliminated by `len.min(1492)`).
  - `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, path `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`, size `758,896` bytes (`28.24%` of `ota_0`), SHA-256 `99d71af7f3b4bdf5c8c130e68e191f21b5936f8a9d364673bda87a73ac98e024`.
- **Handoff & Summary**:
  - `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo107_mtu_i2s_implementer.md`
  - `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/handoff.md`
