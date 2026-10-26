# Subagent Summary: WO-106 (`Extension Certifier`)

- **Work Order**: `WO-106` ([WO-106-flash-wo105-and-certify-task10.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-106-flash-wo105-and-certify-task10.md))
- **Role**: `Extension Certifier` (`self`, Conversation ID `fbf04413-8be7-494e-9e3b-99f44aa00f31`)
- **Timestamp**: `2026-10-25T18:31:00-04:00`
- **Status**:
  - Step 1 (Pre-Flight SHA-256 & Recovery Status Verification): `[EXECUTED & VERIFIED]`
  - Step 2 (Single OTA Flash of `91dbd9d4` `WO-105`): `[EXECUTED & VERIFIED]`
  - Step 3.1 (`GET /status.json` Live Verification of Dual-Core `APP_CPU`, `4 MB PSRAM`, & `-51 dBm` Wi-Fi): `[EXECUTED & VERIFIED]`
  - Step 3.2–3.5 (`GET /` Web UI Dashboard & Subsequent Gates): `[BLOCKED / ERROR]` (`SmolDevice::capabilities()` `max_transmission_unit = 1500` > `esp-radio` `MTU = 1492` caused a deterministic slice bounds-check panic `slice_index_fail` on `GET /`; halted per 1-Flash-Attempt Circuit Breaker)

## 1. Files Created, Modified, or Deleted (Absolute Paths)
1. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/wo106_flash_and_poll.py` (Created)
   - **Rationale**: Executed pre-flight SHA-256 (`770d0850...`) and recovery verification, performed the single `POST /flash.json` upload of `91dbd9d4` (`WO-105`), and verified clean boot into `I2S-4MFlash-Rust` (`recovery: 0`).
2. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/wo106_certify_gates.py` (Created)
   - **Rationale**: Executed `[Gate 3.1]` (`GET /status.json`) and `[Gate 3.2]` (`GET /`), isolating the exact crash point to `GET /`.
3. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/scan_stack_frames.py` (Created)
   - **Rationale**: Audited stack frames across `.text` in `louder-esp32-app` ELF (`run_hardware_firmware = 8,480` B, `dispatch_ip = 1,648` B).
4. `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_mtu_panic.py` (Created)
   - **Rationale**: Verified `movi a12, 1492 (0x05d4)` at `0x4011a75e` and `core::slice::index::slice_index_fail` at `0x4011a764` inside `InterfaceInner::dispatch_ip<SmolTxToken>` (`0x4011a2b8`).
5. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-106-flash-wo105-and-certify-task10.md` (Modified, backup at `/home/console-admin/.gemini/antigravity/brain/fbf04413-8be7-494e-9e3b-99f44aa00f31/scratch/WO-106-flash-wo105-and-certify-task10.md.bak`)
   - **Rationale**: Appended the Rule 15 Fulfillment Section and updated status.
6. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/DIAGNOSTIC_LOG.md` (Modified, backup at `/home/console-admin/.gemini/antigravity/brain/fbf04413-8be7-494e-9e3b-99f44aa00f31/scratch/DIAGNOSTIC_LOG.md.bak`)
   - **Rationale**: Appended Entry 57 documenting the live `WO-105` verification (`core1_active: true`, `core1_loops: 493,383,236`, `rssi: -51`) and the disassembly proof of the `1500 > 1492` MTU slice panic on `GET /`.
7. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261025_wo106_extension_certifier.md` (Created)
   - **Rationale**: Rule 17 subagent summary.
8. `/home/console-admin/.gemini/antigravity/brain/fbf04413-8be7-494e-9e3b-99f44aa00f31/handoff.md` (Created)
   - **Rationale**: Rule 17 handoff report.

*(Zero files under `/home/console-admin/Github/squeezelite-esp32/rust/crates/**` were modified, strictly complying with Invariant 47.)*

## 2. Key Findings & Root Cause Summary
1. **`WO-105` Fixes Verified Live on `172.22.1.15`**:
   - `[Poll 2 @ +25.7s]` (`uptime_ms = 12,848`) and `[Gate 3.1 @ +62s]` (`uptime_ms = 46,227`) returned `HTTP 200 OK` (`730` B / `731` B) on `GET /status.json`:
     - `"core1_active": true` and `"core1_loops"` increased from `132,229,346` to `493,383,236`, proving warm-reboot `park_core(Cpu::AppCpu)` + MMU/cache sync completely resolved `Err(Error::CoreAlreadyRunning)`.
     - `"rssi": -51` dBm (`+31 dB` stronger than `-82 dBm` in `WO-104`), proving `ScanMethod::AllChannels` + `5,000 ms` reconnect cadence selected the local `"iot"` AP.
     - Continuous runtime stability verified past `46.2 seconds` (`> 45s` boot lease watchdog).
2. **Root Cause 1 — `SmolDevice::capabilities()` `max_transmission_unit = 1500` Exceeds `esp-radio` `WIFI_MTU = 1492` (`slice_index_fail` Panic on `GET /`)**:
   - In `esp-radio-1.0.0-beta.1/esp_config.yml:12-15` and `src/wifi/mod.rs:117, 2019-2020`, `const MTU: usize = esp_config_int!(usize, "ESP_RADIO_CONFIG_WIFI_MTU")` is **`1,492` bytes**, and `WifiTxToken::consume_token(self, len: usize, f: F)` indexes `let mut buffer: [u8; 1492] = [0u8; 1492]; let buffer = &mut buffer[..len];`.
   - In `src/main.rs:539`, `SmolDevice::capabilities()` sets `caps.max_transmission_unit = 1500` (`Medium::Ethernet`), so `smoltcp` computes `ip_mtu = 1486` and TCP `local_mss = 1446`.
   - Before `WO-103`, `http_tx_buf` was `[u8; 1024]`, capping Ethernet frames at `1,078` B (`<= 1,492`). After `WO-103` enlarged `STATIC_HTTP_TX_BUF` to `[u8; 1536]`, `GET /status.json` (`879` B -> `933` B Ethernet frame `<= 1,492`) still fits, but `GET /` (`4,827` B `DASHBOARD_HTML`) fills `STATIC_HTTP_TX_BUF` with `1,536` bytes.
   - On the next `smol_iface.poll()`, `smoltcp` dispatches a full-MSS TCP segment (`1,446` B payload + `20` B TCP + `20` B IPv4 + `14` B Ethernet = **`1,500` bytes**) and calls `WifiTxToken::consume_token(1500, ...)`, which immediately panics on `&mut buffer[..1500]` (`0x4011a75e: movi a12, 1492` -> `0x4011a764: slice_index_fail`)!
   - **Fix**: Change `caps.max_transmission_unit = 1500;` to `caps.max_transmission_unit = 1492;` in `SmolDevice::capabilities()` (`src/main.rs:539`).
3. **Root Cause 2 — `Esp32HardwareI2sDma::new` Skips Initial Fractional Clock Divider `(14, 11, 63)` for `44,100 Hz`**:
   - `Esp32HardwareI2sDma::new(transfer, 44_100)` (`src/main.rs:714`) sets `self.sample_rate_hz = 44_100` without programming `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`), and `set_clock_rate(44_100)` early-returns when `self.sample_rate_hz == sample_rate_hz`, leaving `esp-hal`'s integer divider `(14, 0, 1)` (`44,642.86 Hz`, `+1.23%` error) active at boot so `TAS5805M` reports `tas_fs_hz: 48000` and `tas_pll_locked: false`.
   - **Fix**: Initialize `sample_rate_hz: 0` in `Esp32HardwareI2sDma::new` and call `let _ = s.set_clock_rate(sample_rate_hz);` before returning `s`.

## 3. Known Risks & Untested Assumptions
- `172.22.1.15` is currently online in Factory recovery (`"recovery": 1`, `"version": "I2S-4MFlash-1681"`, `"rssi": -50`) and ready for the next OTA flash once `caps.max_transmission_unit = 1492` and the initial `set_clock_rate(44_100)` call are compiled.
