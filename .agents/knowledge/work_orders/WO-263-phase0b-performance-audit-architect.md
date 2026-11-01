# Work Order: WO-263 — Phase 0b: Joint Hardware & Code Performance Audit Architect

- **Work Order ID**: `WO-263`
- **Timestamp**: `2026-11-01T13:34:00-05:00`
- **Assigned Subagent Role**: `Joint Hardware & Code Performance Audit Architect` (`Model: "pro"`, `TypeName: "research"`)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`), LMS (`10.0.0.10`), and `/home/console-admin/Github/squeezelite-esp32/rust` (`84feda56`)
- **Status**: `[IN PROGRESS]`

## 1. Objective
Synthesize the 4 Phase 0a Scout Reports (`WO-259`..`WO-262`) and design the Phase 1 Micro-Auditor Committee Roster and Subsystem × Bottleneck Matrix to root-cause why **"Performance is abysmal"** and uncover all runtime, network, audio, CPU/IRAM, memory, and latency bottlenecks across the hardware and codebase.

## 2. Key Phase 0a Anomalies to Seed into Phase 1 Micro-Auditors
1. **`ANOM-PERF-01` (`CRITICAL` Live Audio Frame Throughput Deficit — `34,415.27` fps vs `44,100.00` fps)**:
   - `WO-259` measured `frames_played` advancing only `+347,904` frames over `10.109` seconds (`34,415.27` frames/sec = **78.0% of real-time 44.1 kHz playback**, causing severe audio stutter/underruns/slowdown).
   - Contrast with `WO-243` (pre-Phase-3 7.91h soak on `e7fa3411`), which sustained `44,100.0170` fps (`+0.39 PPM`).
2. **`ANOM-PERF-02` (Direct WAN HTTP Stream vs LAN LMS Proxy & `smoltcp` TCP Socket / Window / Poll Stalls)**:
   - `WO-260` discovered that after `WO-256`/`WO-258` bypassed `SqueezeDSP` on `10.0.0.10`, LMS sent a direct WAN redirect (`strm` with `server_ip = 0` / direct connection to `cdn-peer021.streaming-pro.com:8024` over WAN with higher RTT + small `smoltcp` TCP RX buffer, OR `smoltcp` SlimProto `snd_wnd: 2048` / `retrans: 0/22` / `output fullness -1` in `STAT-STMt`).
   - Also check if any code changes in `e7fa3411..ac281d18` (`WO-251`..`WO-257`) or `main.rs` / `tasks.rs` / `pipeline.rs` / `coordinator.rs` / `squeezelite-decoders` / `http_control.rs` introduced blocking polls, extra copies, sample-rate/resampler overhead, I2C polling stalls, or Core 0 / Core 1 contention!
3. **`ANOM-PERF-03` (`i2c_acks` Rate Still `5.34` ACKs/sec Despite `WO-251`/`WO-257` `PERF-601`)**:
   - `WO-259` measured `54` I2C ACKs over `10.109`s (`5.34` ACKs/sec) during steady-state playback. Why is I2C still being polled ~11 times every 2s (is `amp_health` or another caller still invoking `read_telemetry()` in addition to `check_faults_and_telemetry()` or polling on Core 0 while holding locks/bus)?
4. **`ANOM-PERF-04` (100% of Audio Decoders, RingBuffer, Resampler, Equalizer, and `smoltcp` Execute from 40 MHz SPI Flash `.flash.text` Instead of IRAM, with `opt-level = 2` and `lto = false`)**:
   - `WO-261` and `WO-262` showed `Cargo.toml` uses `opt-level = 2, lto = false`, and only `1,767` B of `louder-esp32-app` code resides in `.iram0.text` (`113.83` KB / `128` KB used, leaving `~14` KB of unused IRAM), while Core 0 (`PRO_CPU`) and Core 1 (`APP_CPU`) contend on the shared 40 MHz SPI0/SPI1 Flash + PSRAM bus for every instruction fetch in `nanomp3_core`, `AudioRingBuffer`, `Equalizer`, and `app_core_loop`!
5. **`ANOM-PERF-05` (`GRS-002` in `marker_fuzz.rs:13` and `GRS-010`/`GRS-012` Test Warnings)**:
   - `WO-262` flagged 1 `GRS-002` error in `louder-esp32-app/tests/marker_fuzz.rs:13` (`use proptest::prelude::*;`).

## 3. Required Output
Produce a concise Phase 0b Audit Architect Plan specifying **8 parallel Phase 1 Micro-Auditors** (`2` Hardware/Live-Telemetry Micro-Auditors + `6` Code Performance/Regression Micro-Auditors) with strict disjoint file/subsystem scopes, exact checklist seeds, and finding ID ranges.
