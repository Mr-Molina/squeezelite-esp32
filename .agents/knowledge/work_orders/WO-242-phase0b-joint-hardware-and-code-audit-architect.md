# Work Order: WO-242 — Phase 0b: Joint Hardware & Code Audit Architect Slicing & Committee Assembly

- **Work Order ID**: `WO-242`
- **Timestamp**: `2026-11-01T08:40:00-05:00`
- **Assigned Subagent Role**: `Audit Architect` (`Model: "pro"`, `TypeName: "research"`, read-only)
- **Target Host / Component**: `"BASEMENT"` (`172.22.1.15`), LMS (`10.0.0.10`), and `/home/console-admin/Github/squeezelite-esp32/rust` (`e7fa3411`)
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective
Synthesize the 4 Phase 0a Micro-Scout reports (`WO-238`..`WO-241`) into a unified **Raw Hardware & Code Scan Report** and slice the 7.67-hour live soak + `80,458`-LOC `MK5-Rust` workspace (`e7fa3411`) into tightly-scoped Phase 1 Hardware & Code Micro-Auditors:
- Read `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-238-phase0a-hardware-scout-basement-soak.md`
- Read `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-239-phase0a-hardware-scout-lms-and-logs.md`
- Read `/home/console-admin/.gemini/antigravity/brain/59184978-2cf0-45be-871e-bfb54a1e8021/scratch/inventory.yaml`
- Read `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/work_orders/WO-241-phase0a-code-scout-surfaces-and-static-tools.md`
- Read `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/checklist-seeds.md` and `/home/console-admin/.gemini/config/skills/deep-code-audit/references/checklist-seeds.md`

### Key Phase 0a Anomalies & Focus Areas to Inject into Phase 1 Micro-Auditors:
1. **Live Hardware Soak Telemetry (`WO-238` & `WO-239`)**:
   - `"BASEMENT"` (`172.22.1.15`) has `uptime_ms = 27,623,149` (`7.673` hours), `frames_played = 138,695,417`, `0` I2C NACKs, `0` B PSRAM leak (`1,519,160` B free / `2,675,144` B used).
   - **Anomaly 1 (`i2c_acks_per_sec = 6.95`)**: Why is `i2c_acks` incrementing at `~6.95/sec` (`165,470` total ACKs over 7.67 hours) during steady-state playback? Inspect `main.rs` I2C polling (`check_faults()` / `poll_telemetry()` frequency) to check if I2C bus polling is occurring every ~280ms and whether that is optimal or unnecessary bus traffic.
   - **Anomaly 2 (`vol_pct: 51` vs LMS `mixer_volume: 31`)**: Why does `/status.json` report `"vol_pct": 51` while LMS `status` reports `"mixer volume": 31`? Check `audg` gain curve mapping in `main.rs` / `coordinator.rs`.
   - **Anomaly 3 (`SqueezeDSP` active on `10.0.0.10` for `ibizaglobalradio.mp3`)**: In `WO-239`, LMS `10.0.0.10` spawned `lame --mp3input --decode | SqueezeDSP | lame --preset insane` for `c8:2e:18:be:b8:34` even though `plugin.squeezedsp:enabled` was previously set to `0` (`WO-061`) and `WO-196` added a `FLAC24` template in `SqueezeDSP/custom-convert.conf`. Inspect why `SqueezeDSP` is intercepting `mp3` streams on `10.0.0.10`.
   - **Anomaly 4 (`frames_played_per_sec = 42,248.22` over a 5.6s HTTP sample window)**: Check whether the 5.6s curl wall-clock interval included HTTP/script startup overhead or whether `frames_played` over the 3,128s track duration (`138,695,417` frames / `3,145`s = `44,100.2` fps) is bit-exact `44,100` Hz.
2. **Codebase Slicing (`WO-240` & `WO-241` —Waves 6–10 `WO-224`..`WO-235` Full-Parity Code)**:
   - Slice the workspace into a focused, high-signal cohort of **8 Micro-Auditors** (2 Hardware Micro-Auditors + 6 Code Micro-Auditors) covering:
     1. `HW-AUD-1`: `"BASEMENT"` (`172.22.1.15`) 7.67h Soak Physical/Bus/I2C/DMA/PSRAM/Clock & Volume Telemetry Micro-Auditor
     2. `HW-AUD-2`: LMS (`10.0.0.10`) `SqueezeDSP` Transcode Pipeline, Socket Backlog, & Multi-Protocol Bridge Micro-Auditor
     3. `CODE-AUD-1`: Bare-Metal Runtime, `unsafe` MMIO/PSRAM/Core1, & `init_static_pipeline` Micro-Auditor (`louder-esp32-app/src/main.rs`)
     4. `CODE-AUD-2`: Audio Pipeline, Coordinator, `SourceArbiter`, RBJ Equalizer & Polyphase Resampler Micro-Auditor (`louder-esp32-app/src/{pipeline,coordinator,tasks,backend_device}.rs`, `squeezelite-audio-core/src/{equalizer,resampler,ringbuffer,volume,fade}.rs`)
     5. `CODE-AUD-3`: Multi-Codec Container Demuxers (`MP4/esds`, `Ogg-FLAC`, `Xing/LAME` Gapless) & Vendored Decoders Micro-Auditor (`squeezelite-decoders/src/lib.rs`, `nanomp3-core`)
     6. `CODE-AUD-4`: AirPlay 1 (`squeezelite-raop`) & Spotify Connect (`squeezelite-cspot`) Protocol & Crypto Micro-Auditor
     7. `CODE-AUD-5`: Phase 3 Network Autonomy (`http_control.rs`, `wifi.rs`, `ota_safety.rs`, `console.rs`, `eth_w5500.rs`, `network.rs`) Micro-Auditor
     8. `CODE-AUD-6`: Phase 4 Bluetooth (`bt_a2dp.rs`) & Phase 7 Peripherals (`display_gds.rs`, `controls.rs`, `dac_platform.rs`, `power_boards.rs`, `amp.rs`) Micro-Auditor

## 2. Deliverables
Return the structured YAML **Committee Roster** with exact file/line scopes, pre-identified anomalies, checklist seeds, and compliance tags for all 8 Micro-Auditors (`HW-AUD-1`..`2` and `CODE-AUD-1`..`6`).

---

## 3. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent ID**: `6517d9d1-504c-4059-867d-9919b3d83204`
- **Summary**: Delivered the 8-Auditor Committee Roster (`HW-AUD-1`..`2` and `CODE-AUD-1`..`6`) with pre-identified Phase 0a anomalies (`ANOM-01`..`05`, `CODE-UNSAFE-01`..`03`, `SURFACE-01`..`04`), checklist seeds, and compliance tags.
