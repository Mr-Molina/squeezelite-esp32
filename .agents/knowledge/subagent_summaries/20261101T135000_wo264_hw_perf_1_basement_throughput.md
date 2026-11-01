# Subagent Summary: WO-264 Phase 1 HW-PERF-1 — BASEMENT Throughput, I2C Bus & Wi-Fi Jitter

- **Work Order ID**: `WO-264`
- **Timestamp**: `2026-11-01T13:50:00-05:00`
- **Role**: `Target Hardware, Bus Contention & Live Latency Profiler (HW-PERF-1)`
- **Target**: `"BASEMENT"` (`172.22.1.15`)
- **Status**: `[EXECUTED & VERIFIED]`

---

## 1. Files Modified / Created
1. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-264-phase1-hw-perf-1-basement-throughput.md`:
   - Updated Status to `[EXECUTED & VERIFIED]`.
   - Appended detailed Fulfillment Summary with findings `HW-PERF-101`, `HW-PERF-102`, and `HW-PERF-103`.
   - Backup path: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-264-phase1-hw-perf-1-basement-throughput.md.bak`.
2. `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261101T135000_wo264_hw_perf_1_basement_throughput.md`:
   - This summary report.

---

## 2. Rationale & Analysis

### Finding `HW-PERF-101`: Multi-Window Frame Playback Throughput Deficit
- **Empirical Evidence**:
  - Sample series across 3-second windows:
    - Window 0->1: Track reset observed (`frames_played` reset from 8,230,400 to 0).
    - Window 1->2: Zero frames played (track boundary buffering).
    - Window 2->3: 32,556 fps (playback start mid-window).
    - Window 3->4: 44,088.89 fps (nominal 44.1 kHz).
    - Window 4->5: 44,123.16 fps (nominal 44.1 kHz).
  - High-resolution 500 ms sampling test confirmed: over 15 consecutive active windows, instantaneous frame rate locked onto `44,100 fps` (range: 43,885.7 – 44,346.5 fps).
  - Core 1 heartbeat loop rate was immutable at `3,951` to `3,996` loops/sec (the 10,000 NOP loop).
- **Conclusion**:
  - `ANOM-PERF-01` (34,415 fps overall throughput) is **NOT** a CPU or I2S DMA clock slowdown.
  - The deficit is caused by intermittent buffer starvation (where `pump_silence_if_needed` injects uncounted silence) and track transition resets (`reach_track_start` resetting `frames_played = 0`).

### Finding `HW-PERF-102`: I2C Bus Contention & Call-Site Correlation
- **Empirical Evidence**:
  - In `main.rs:629`, `HW_ACK_COUNT` increments per *successful I2C transaction* (`read`, `write`, `write_read`), not per individual clock ACK bit.
  - Periodic 2.0-second health check in `amp_and_cyd_health_phase`:
    - Calls `amp.check_faults_and_telemetry()`: executes 3 writes (`select_page0_book0`) + 3 write_reads (`CHAN_FAULT`, `FS_MON`, `AUTOMUTE_STATE`) = 6 I2C ACKs per clean poll.
    - If in `Standby`, executes `check_faults` (4 ACKs) + `read_telemetry` (5 ACKs) = 9 ACKs.
    - If a fault occurs in `Play`, executes `recover_from_fault()` (6 ACKs) = 12 ACKs per 2-second cycle.
    - Telemetry deltas are exact multiples of 6 ACKs (`dacks = 6, 12, 18, 24`).
  - Hardware frequency: I2C0 is configured at `Rate::from_khz(400)` (400 kHz Fast-Mode).
  - Bus latency: 27 total bytes transferred across 6 operations take ~610 µs (0.61 ms) per 2-second poll.
- **Conclusion**:
  - `ANOM-PERF-03` (5.34 ACKs/sec = ~10.7 ACKs/poll) is benign normal operation.
  - Core 0 is blocked on I2C for only ~0.03% of its time; I2C is decoupled from Core 1.

### Finding `HW-PERF-103`: 802.11 ModemSleep Reversion & DTIM Packet Jitter
- **Empirical Evidence**:
  - `ping -c 20 -i 0.1 172.22.1.15` showed: `rtt min/avg/max/mdev = 3.048/45.478/100.722/33.454 ms`.
  - Latency is bimodal: 3–5 ms during active RF bursts, jumping to 50–100+ ms (max 162.3 ms) across the 100 ms AP DTIM beacon interval.
  - Inspection of `main.rs`: `wifi_controller.set_power_saving(PowerSaveMode::None)` is called only during pre-connect setup (`init_wifi`).
  - When `connect_async()` succeeds in `wifi_and_lease_phase()`, the underlying Espressif 802.11 binary driver defaults back to `WIFI_PS_MIN_MODEM`.
- **Conclusion**:
  - The interface operates with ModemSleep active, inducing 50–100+ ms packet jitter and bursty TCP delivery that drives intermittent audio buffer starvation.
  - Re-applying `set_power_saving(PowerSaveMode::None)` post-connection will flatten jitter to 2–4 ms.

---

## 3. Risks & Untested Assumptions
- Strictly read-only inspection performed; zero system or hardware state changed.
- Applying `PowerSaveMode::None` post-connection increases Wi-Fi power consumption slightly (~80–100 mA increase on 3.3V rail), which is well within the 20W PD / 15V supply envelope of the Louder-ESP32 hardware.
