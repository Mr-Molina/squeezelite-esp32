# Subagent Summary: WO-107 — Align `SmolDevice` MTU to `1492` (`esp-radio` `WIFI_MTU`) & Program Initial `44,100 Hz` Fractional Clock Divider (`--stage-only`)

- **Work Order**: `WO-107` ([WO-107-fix-smol-mtu-1492-and-i2s-clock.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-107-fix-smol-mtu-1492-and-i2s-clock.md))
- **Subagent Role**: `Rust App MTU & I2S Clock Implementer` (`self`, Conversation ID `a3d90645-d9e4-4f49-a0ca-6eee9549623c`)
- **Timestamp**: `2026-10-25T18:47:30-04:00`
- **Status**: `[EXECUTED & VERIFIED]`
- **Git Commit (`MK5-Rust`)**: `dec55808e0e75c8daabb6c9c9664c44d523e6292` (`dec55808`)

## 1. Files Created, Modified, or Deleted (Absolute Paths)
1. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/main.rs.bak` (Created)
   - **Rationale**: Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs`.
2. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/lib.rs.bak` (Created)
   - **Rationale**: Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`.
3. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/WO-107-fix-smol-mtu-1492-and-i2s-clock.md.bak` (Created)
   - **Rationale**: Rule 35 pre-modification backup of `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-107-fix-smol-mtu-1492-and-i2s-clock.md`.
4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/main.rs` (Modified)
   - **Rationale**:
     - Defined `const WIFI_MAX_FRAME_LEN: usize = 1492;`, set `caps.max_transmission_unit = WIFI_MAX_FRAME_LEN;` in `SmolDevice::capabilities()`, and clamped `self.0.consume_token(len.min(WIFI_MAX_FRAME_LEN), f)` in `SmolTxToken::consume` so `smoltcp` never generates a frame exceeding `esp-radio`'s `1,492`-byte buffer (`ip_mtu = 1478`, TCP `local_mss = 1438`) and LLVM statically eliminates `slice_index_fail` from `InterfaceInner::dispatch_ip<SmolTxToken>`.
     - Updated `Esp32HardwareI2sDma::new` to initialize `sample_rate_hz: 0` and immediately call `driver.set_clock_rate(sample_rate_hz)`, and re-applied `set_clock_rate` inside `ensure_running()` when a transfer restarts.
     - Pre-filled `tx_buffer` with `2,048` B of silence before `i2s_tx.write(tx_buffer)`, constructed `Esp32HardwareI2sDma::new(i2s_transfer, 44_100)` before `pwdn.set_high()` / `amp.initialize(32)`, and moved the initial `amp.read_telemetry()` call to immediately after `amp.start_playback_via_hiz()`.
5. `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs` (Modified)
   - **Rationale**: Updated `Tas5805mTelemetry::is_pll_locked` to return `true` when `(self.clkdet_status & 0x08) != 0 || ((self.clkdet_status & 0x07) == 0 && self.detected_sample_rate_hz() > 0 && self.bck_mon >= 32)`, and added unit test `test_telemetry_pll_locked_zero_fault_and_error_bits`.
6. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/scratch/verify_wo107_elf.py` (Created)
   - **Rationale**: Verified `run_hardware_firmware` stack frame (`8,480` B) and confirmed `0` `call8 slice_index_fail` instructions inside `InterfaceInner::dispatch_ip<SmolTxToken>`.
7. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` (Modified)
   - **Rationale**: Packaged `--stage-only` OTA binary (`758,896` bytes, SHA-256 `99d71af7f3b4bdf5c8c130e68e191f21b5936f8a9d364673bda87a73ac98e024`).
8. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-107-fix-smol-mtu-1492-and-i2s-clock.md` (Modified)
   - **Rationale**: Appended Rule 15 Fulfillment Section and updated status to `[EXECUTED & VERIFIED]`.
9. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo107_mtu_i2s_implementer.md` (Created)
   - **Rationale**: Rule 17 subagent summary.
10. `/home/console-admin/.gemini/antigravity/brain/a3d90645-d9e4-4f49-a0ca-6eee9549623c/handoff.md` (Created)
    - **Rationale**: Rule 17 handoff report.

## 2. Verification Metrics (`--stage-only`)
- `cargo test --workspace`: `0` failures (`259` tests passed across workspace).
- `cargo clippy --workspace --all-targets -- -D warnings`: `0` warnings (`exit code 0`).
- `xtensa-esp32-elf-readelf -S`:
  - `.stack`: `0x008bd0` = `35,792` bytes (`>= 35,000` B gate PASS)
  - `.data`: `0x003dd0` = `15,824` bytes (`<= 20,000` B gate PASS)
  - `.bss`: `0x023440` = `144,448` bytes
  - `.dram2_uninit`: `0x004990` = `18,832` bytes
- Disassembly Audit (`verify_wo107_elf.py`):
  - `run_hardware_firmware` (`0x4011d324`, size `42,841` B): `entry a1, 8480` (`0x2120` B `<= 9,000` B gate PASS).
  - `InterfaceInner::dispatch_ip<SmolTxToken>` (`0x4011a2d0`, size `1,304` B): `entry a1, 1648`, `call8 slice_index_fail` count = **`0`**.
- `verify_staged_ota_bin.py`: `[STAGE-ONLY AUDIT PASS]`, `758,896` bytes (`28.24%` of `ota_0`), SHA-256 `99d71af7f3b4bdf5c8c130e68e191f21b5936f8a9d364673bda87a73ac98e024`.

## 3. Known Risks & Untested Assumptions
- Strictly `--stage-only`: `172.22.1.15` was not flashed or modified. Live OTA certification (`WO-108`) will verify full `4,827`-byte `GET /` dashboard delivery and `tas_fs_hz: 44100` / `tas_pll_locked: true` on `172.22.1.15`.
