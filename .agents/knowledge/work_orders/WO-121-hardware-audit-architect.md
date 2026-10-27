# Work Order `WO-121`: `/deep-hardware-audit` Phase 0b — Hardware Audit Architect & Committee Assembly (`[BASEMENT]`)

- **ID**: `WO-121`
- **Timestamp**: `2026-10-25T20:27:00-04:00`
- **Subagent Role**: Hardware Audit Architect (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions
You are the **Phase 0b Hardware Audit Architect** for `/deep-hardware-audit` of `[BASEMENT]` (`172.22.1.15`, Wi-Fi MAC `c8:2e:18:be:b8:34`, Sonocotta Louder-ESP32 `ESP32-WROVER` + TI `TAS5805M` at `I2C0 0x2D`) and its companion Lyrion Music Server / Bridge appliance (`10.0.0.10`).
Read:
- `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/discovery-protocol.md` (§2)
- `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/checklist-seeds.md`
- `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`
- `/home/console-admin/Github/squeezelite-esp32/.agents/INFRASTRUCTURE_INVARIANTS.md` (Invariants 44–47)

### Consolidated Phase 0a Raw Hardware Scan Report (`WO-116`..`WO-119`)
1. **Target Identity & Uptime (`WO-116`, `WO-118`)**:
   - `"BASEMENT"` (`172.22.1.15`, MAC `c8:2e:18:be:b8:34`, `Sonocotta Louder-ESP32` `ESP32-WROVER` dual-core Xtensa LX6 @ 240 MHz, `4 MB` SPI Flash + `4 MB` Quad-SPI PSRAM).
   - Running firmware: `"version": "I2S-4MFlash-Rust"`, `"recovery": 0`, `"rust_engine": true`, `"otadata_prearmed_factory": true`, `"uptime_ms": 3627597` (`> 60 min` continuous uptime).
2. **Memory, Dual-Core & ELF Layout (`WO-116`, `WO-117`)**:
   - External PSRAM (`SPI0` `0x3F800000`): `psram_used_bytes: 655360` (`512 KB` stream + `128 KB` output), `psram_free_bytes: 3538944`.
   - Core 1 (`APP_CPU`): `core1_active: true`, `core1_loops: 3491479452` (`+47,290,232` loops in `4,062 ms` = `11.64M` loops/sec).
   - Xtensa ELF (`/home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app`):
     - `.stack`: `35,792` B (`0x3ffd7430`), `run_hardware_firmware` prologue `entry a1, 8480` (`8,480` B frame = `23.69%` of `.stack`).
     - `.bss`: `144,448` B (`0x3ffb3ff0`), `.data`: `15,824` B (`0x3ffb0000`), `.dram2_uninit`: `18,832` B (`0x3ffe7e40`), `.text`: `535,729` B, `.rodata`: `84,568` B.
   - Note from `WO-117`: Staged OTA binary at old conversation path `.../31deac42.../scratch/louder-esp32-app-basement-ota.bin` is `ABSENT`.
3. **Audio Clocking, I2S0 DMA & TI TAS5805M (`I2C0 0x2D`) Telemetry (`WO-116`, `WO-118`, `WO-119`)**:
   - `tas_pll_locked: true`, `tas_automuted: false`, `tas_bck_ratio: 64`, `tas_fs_hz: 48000` (while `codec_sample_rate: 44100`), `again_code: 8` (`-4.0 dB`), `vol_pct: 80`, `amp_mode: "Play"`.
   - `i2c_acks: 14524` (`+16` ACKs across `4,062 ms` = `3.94` polls/sec), `i2c_nacks: 0`.
   - `frames_played`: `147,077,056` (`+84,608` frames per `2,031 ms` interval = **`41,658.3` frames/sec** vs nominal `44,100` frames/sec = `94.46%` rate during `SqueezeDSP` + `lame --preset insane` `320 kbps` transcoding!).
   - Volume Synchronization Anomaly (`WO-119`): LMS `10.0.0.10` reports `mixer_volume: 10` for `c8:2e:18:be:b8:34`, while `172.22.1.15/status.json` reports `vol_pct: 80`.
   - Settings Persistence (`WO-116`, `WO-118`): `settings_seq: 7` is held in RAM (`settings_scratch`), because runtime `SPI1` NOR Flash writes deadlock while `APP_CPU` + `PSRAM` (`SPI0`) + Wi-Fi are active (`WO-109`).
4. **Wi-Fi RF, TCP Socket Queue & LMS Bridge Host `10.0.0.10` (`WO-116`, `WO-117`, `WO-118`, `WO-119`)**:
   - `"BASEMENT"` Wi-Fi RSSI: `-48 dBm` on SSID `"iot"` (4 BSSIDs visible on `wlx98482766488b`: Ch 1 `02:EC:DA:B7:C8:B4` 70%, Ch 11 `BA:FB:E4:D7:70:10` 70%, Ch 11 `86:2A:A8:87:C5:B8` 50%, Ch 6 `02:EC:DA:BD:BB:44` 39%); ping `0%` loss (`avg 4.833 ms`).
   - Active TCP Sockets on `10.0.0.10`:
     - SlimProto `10.0.0.10:3483 <-> 172.22.1.15:56006` (`ESTAB`, `send_q: 0`, `recv_q: 0`)
     - HTTP Audio Stream `10.0.0.10:9000 <-> 172.22.1.15:56007` (`ESTAB`, **`send_q: 173,371` bytes** queued on LMS socket!).
   - LMS Transcode Pipeline active on `10.0.0.10`: `lame` decoder (`PID 14340`) -> `SqueezeDSP` (`PID 14341`) -> `lame --preset insane` (`320 kbps` MP3 encoder, `PID 14349`), plus `ShairTunes2W` (`avahi-publish-service` `PID 14338` port `36201`), UPnP `MediaRenderer.xml` (`HTTP 200`), and `CastBridge` (`PID 12150`).

### Deliverable
Slice the hardware/firmware/appliance inspection into **6 tightly-scoped Phase 1 Hardware Micro-Auditors** across a single parallel cohort (covering L1 Physical/RF/Power/Clocks $\rightarrow$ L2 Bus/I2C/I2S/SPI/TCP $\rightarrow$ L3 Firmware/Registers/Memory/Bootloader $\rightarrow$ L4 LMS Bridges/Transcode/Logs). Return the YAML **Committee Roster** with all 8 required prompt sections per auditor (`references/discovery-protocol.md` & `SKILL.md`). Never touch `/dev/ttyUSB0`.

## 2. Fulfillment Section
*(To be completed by subagent)*
