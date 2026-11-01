# Work Order: WO-264 — Phase 1 `HW-PERF-1`: `"BASEMENT"` (`172.22.1.15`) Live Audio Frame Throughput, I2C Bus Contention & Wi-Fi Jitter Auditor

- **Work Order ID**: `WO-264`
- **Timestamp**: `2026-11-01T13:37:00-05:00`
- **Assigned Subagent Role**: `Target Hardware, Bus Contention & Live Latency Profiler (HW-PERF-1)` (`Model: "flash"`, `TypeName: "self"`)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Read-Only Constraint
Audit `"BASEMENT"` (`172.22.1.15`) bottom-up (`L1 -> L4`) to diagnose `ANOM-PERF-01` (`34,415.27` frames/sec vs `44,100` frames/sec = 22% throughput deficit) and `ANOM-PERF-03` (`5.34` I2C ACKs/sec).
- **Constraint**: Strictly read-only telemetry inspection. Do NOT flash, reboot, or touch `/dev/ttyUSB0`.

## 2. Inspection Checklist
1. **Multi-Window Audio Frame Progression & Variance**:
   - Sample `GET http://172.22.1.15/status.json` 6 times at 3-second intervals (`t = 0, 3, 6, 9, 12, 15s`) and compute `frames_played/s`, `core1_loops/s`, and `i2c_acks/s` in each 3s window to determine whether `frames_played` stalls intermittently (bursty network starvation) or runs continuously slow (CPU/DMA starvation).
2. **I2C Bus Contention & Call-Site Correlation**:
   - Inspect `rust/crates/louder-esp32-app/src/main.rs` and `rust/crates/louder-esp32-app/src/amp.rs` around `amp_health` / `check_faults_and_telemetry` / `read_telemetry` to explain why `54` I2C ACKs occurred over `10.109`s (`5.34` ACKs/s = ~10.7 ACKs per 2s poll) and measure how many milliseconds Core 0 spends blocked on bit-banged/peripheral 100 kHz / 400 kHz I2C transactions per poll.
3. **Wi-Fi Latency & Jitter Under Load**:
   - Correlate `ping -c 20 -i 0.1 172.22.1.15` RTT spikes with `ESP_WIFI` power-save (`ModemSleep` vs `None`) settings in `main.rs`.
4. **Output Schema**:
   - Report findings using `HW-PERF-101`, `HW-PERF-102`, etc., with Severity, Safety Tier (`TIER-1-SAFE`, `TIER-2-SUBSYSTEM`, `TIER-3-STAGE-ONLY`), Empirical Evidence, Root Cause, Remediation, Rollback, and Verification Read-Back.

## 3. Fulfillment Section

### Execution & Verification Summary
- **Execution Date**: 2026-11-01T13:48:00-05:00
- **Auditor**: `Target Hardware, Bus Contention & Live Latency Profiler (HW-PERF-1)`
- **Target**: `"BASEMENT"` (`172.22.1.15`)
- **Mode**: Strictly Read-Only (0 flash writes, 0 reboots, zero access to `/dev/ttyUSB0`)
- **Active Tasks**: 0 background tasks (`manage_task` verified clean)

---

### Key Diagnostic Findings

#### `HW-PERF-101`: Multi-Window Frame Playback Deficit Root Cause: Intermittent Bursty Starvation & Track Resets (Rejecting DMA/CPU Slowdown Hypothesis)
- **Severity**: HIGH
- **Safety Tier**: `TIER-1-SAFE` (Diagnostic finding & tuning guidance)
- **Empirical Evidence**:
  - Sample series Run 1 (`t = 0, 3, 6, 9, 12, 15s`):
    - Window 0->1: Uptime 10,815,994 -> 10,818,959 ms; `frames_played` reset from 8,230,400 to 0 (track transition).
    - Window 1->2: `dframes = 0` (0.00 fps, buffering).
    - Window 2->3: `dframes = 99,904` (32,556.31 fps, playback initiated mid-window).
    - Window 3->4: `dframes = 130,944` in 2.970s = **44,088.89 fps** (nominal 44.1 kHz).
    - Window 4->5: `dframes = 131,840` in 2.988s = **44,123.16 fps** (nominal 44.1 kHz).
  - High-resolution 500 ms sampling test (`fast_sample.py`, 19 windows):
    - Over 15 consecutive active windows (samples [04] through [18]), instantaneous frame rate was: `44,079.5`, `44,055.8`, `44,288.0`, `43,962.0`, `44,172.5`, `44,208.8`, `44,077.8`, `44,120.2`, `44,009.0`, `44,065.6`, `43,992.2`, `44,289.0`, `43,885.7`, `44,346.5`, `44,045.2` fps.
    - Core 1 heartbeat loop ran steadily at `3,951.58` to `3,996.21` loops/sec without variation.
- **Root Cause**:
  - The 22% overall throughput deficit (`ANOM-PERF-01`, 34,415 fps) is **NOT** caused by an erroneous I2S clock divider, DMA underclocking, or CPU compute bottlenecks.
  - When actively playing, the I2S DMA sink commits audio at exactly 44,100 frames/sec.
  - Deficits occur exclusively due to **intermittent bursty starvation** (ring buffer underrun causing the I2S driver to inject silence via `pump_silence_if_needed`, which does not increment `frames_played`) and **track boundary resets** (`reach_track_start` resetting `frames_played = 0`).
- **Remediation**:
  - Optimize upstream network streaming buffer depths and eliminate Wi-Fi DTIM sleep latency (see `HW-PERF-103`).
- **Rollback**: N/A (Read-only telemetry).
- **Verification Read-Back**: Multi-window rate reads at nominal 44,100 fps confirm hardware clock accuracy.

---

#### `HW-PERF-102`: I2C Bus Transaction Mechanics, Call-Site Correlation & Core 0 Contention Profile
- **Severity**: LOW / BENIGN
- **Safety Tier**: `TIER-1-SAFE`
- **Empirical Evidence**:
  - In `louder-esp32-app/src/main.rs`, `HW_ACK_COUNT` is incremented by 1 per *successful I2C transaction* (`read`, `write`, `write_read`), not per individual I2C bit clock ACK.
  - Periodic health poll in `amp_and_cyd_health_phase` (invoked every 2,000 ms) calls `amp.check_faults_and_telemetry()`:
    1. `select_page0_book0()` -> 3 `write` operations (`PAGE_SELECT=0`, `BOOK_SELECT=0`, `PAGE_SELECT=0`) = 3 ACKs.
    2. `write_read(CHAN_FAULT, &mut status)` = 1 ACK.
    3. `write_read(FS_MON, &mut clk_regs)` = 1 ACK.
    4. `write_read(AUTOMUTE_STATE, &mut automute)` = 1 ACK.
    - Total per clean poll = **6 I2C operations (6 ACKs)** every 2 seconds = **3.0 ACKs/sec**.
  - In `Standby`, `amp.rs` executes `check_faults` (4 ACKs) + `read_telemetry` (5 ACKs) = **9 ACKs**.
  - If a fault is flagged in `Play`, `amp.recover_from_fault()` executes `select_page0_book0` (3 ACKs) + 3 control writes = **6 additional ACKs** (yielding 12 ACKs in a 2s poll = 6.0 ACKs/sec).
  - Volume updates (`set_hw_volume` / `set_volume`) and track starts (`start_playback_via_hiz`) contribute 4 to 8 additional ACKs.
  - Observed multi-window telemetry deltas (`dacks = 6, 12, 18, 24`) are exact integer multiples of the 6-ACK transaction unit.
  - Bus frequency is initialized in `main.rs:1832`: `I2cConfig::default().with_frequency(Rate::from_khz(400))` (Fast-Mode 400 kHz).
  - Transaction bus timing:
    - 27 total bytes transferred per 6-operation poll @ 400 kHz (2.5 µs/bit = 22.5 µs/byte).
    - Cumulative bus time per 2-second poll: **~610 µs (0.61 ms)**.
    - Duty cycle: `0.61 ms / 2000 ms = 0.03%` of Core 0 execution time.
- **Root Cause**:
  - `ANOM-PERF-03` (5.34 ACKs/sec = ~10.7 ACKs/poll) is the normal product of the 2-second periodic health check (6 ACKs) plus occasional fault recovery checks (6 ACKs) or volume adjustments.
  - Contention on Core 0 from I2C is negligible (0.03% duty cycle) and is completely decoupled from Core 1.
- **Remediation**: No code modification required; telemetry counters reflect intended transactional behavior.
- **Rollback**: N/A.
- **Verification Read-Back**: Verified against `squeezelite-tas58xx/src/lib.rs` and `main.rs` I2C driver implementation.

---

#### `HW-PERF-103`: 802.11 ModemSleep Reversion & DTIM Packet Jitter Under Load
- **Severity**: MEDIUM
- **Safety Tier**: `TIER-2-SUBSYSTEM`
- **Empirical Evidence**:
  - `ping -c 20 -i 0.1 172.22.1.15` statistics:
    - `20 packets transmitted, 20 received, 0% packet loss`
    - `rtt min/avg/max/mdev = 3.048/45.478/100.722/33.454 ms`
    - High-density sampling reveals bimodal ping distributions: packets arriving during active TX/RX resolve in `3.0 - 5.5 ms`, while packets arriving when the radio is idling jump to `60.4 - 100.7 ms` (and up to `162.3 ms`).
  - Code inspection of `rust/crates/louder-esp32-app/src/main.rs`:
    - `wifi_controller.set_power_saving(PowerSaveMode::None)` is executed strictly during `init_wifi()` prior to connection.
    - In `wifi_and_lease_phase()`, `wifi_controller.connect_async()` is invoked.
    - Upon AP association, the Espressif binary 802.11 stack (`esp_wifi_start` / `esp_wifi_connect`) resets power saving to default `WIFI_PS_MIN_MODEM`.
    - `PowerSaveMode::None` is never re-enforced once the station enters connected state.
- **Root Cause**:
  - The Wi-Fi radio enters 802.11 DTIM modem sleep between beacon frames (100 ms AP beacon interval).
  - This induces periodic latency spikes of 50–100+ ms on incoming packets, creating bursty TCP delivery that starves the audio ring buffer during periods of marginal LMS packet pace.
- **Remediation**:
  - In `wifi_and_lease_phase()` (or immediately following successful `wifi_controller.is_connected()`), invoke `let _ = s.wifi_controller.set_power_saving(PowerSaveMode::None);` to ensure the modem remains continuously powered and eliminates DTIM sleep latency.
- **Rollback**: Revert to calling `set_power_saving` only in `init_wifi`.
- **Verification Read-Back**: Re-run `ping -c 20 -i 0.1 172.22.1.15`. When `PowerSaveMode::None` is active, RTT should stabilize at `2 - 4 ms` with `mdev < 2.0 ms`.
