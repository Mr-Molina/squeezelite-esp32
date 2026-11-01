# Work Order `WO-171`: `/deep-hardware-audit` Phase 1 Auditor 4 (`HW-AUD-04`) — `"BASEMENT"` (`172.22.1.15`) Audio Clock (`I2S0`), TI `TAS5805M` (`0x2D` `automuted`/`vol_pct: 5`), & Native Lossless `FlacDecoder` PSRAM Pipeline Micro-Auditor

- **ID**: `WO-171`
- **Timestamp**: `2026-10-27T17:20:30-04:00`
- **Subagent Role**: `"BASEMENT"` (`172.22.1.15`) Audio Clock, TI `TAS5805M`, & Native `FlacDecoder` Pipeline Micro-Auditor (`Model: "pro"`, `TypeName: "research"`)
- **Status**: `[IN PROGRESS]`

## 1. Scope & Instructions (8-Section Hardware Micro-Auditor Contract)
1. **Role**: `"BASEMENT" (172.22.1.15) Audio Clock (I2S0), TI TAS5805M (0x2D automuted/vol_pct: 5), & Native Lossless FlacDecoder PSRAM Pipeline Micro-Auditor` (`HW-AUD-04`)
2. **Target & Scope**: Embedded Target `"BASEMENT"` (`172.22.1.15`), `http://172.22.1.15/status.json`, `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{main.rs,pipeline.rs,amp.rs}`, and `/home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs`.
3. **Layer Mandate**: Audit strictly bottom-up:
   - `L1-Physical-Power-Thermal`: TI `TAS5805M` (`0x2D`) analog gain (`again_code: 8` = `-4.0 dB`), digital volume (`vol_pct: 5`, `mixer_volume: 5` on LMS), and `tas_automuted: true` when `I2S0` is pumping zero-filled silence frames while idle.
   - `L2-Bus-Interconnect`: `I2S0_CLKM_CONF_REG` (`0x3FF4_F0AC`) fractional clock dividers for `44,100 Hz`, `48,000 Hz`, `88,200 Hz`, and `96,000 Hz` (`MaxSampleRate=96000` in `BASEMENT_HELO_CAPABILITIES`), `tas_fs_hz: 44100`, `tas_bck_ratio: 64`, `tas_pll_locked: true`, `i2c_nacks: 0`.
   - `L3-Driver-Firmware-Config`: Verify why `172.22.1.15` currently advertises `MaxSampleRate=48000,pcm,wav,aif,mp3` (because `WO-155` Rev 3 ELF with `MaxSampleRate=96000,flc,pcm,wav,aif,mp3` has not yet been packaged into an OTA `.bin` and flashed via `WO-156` Rev 2!), and check whether `Esp32HardwareI2sDma::set_clock_rate` in `src/main.rs` supports `88,200 Hz` and `96,000 Hz` since `BASEMENT_HELO_CAPABILITIES` now advertises `MaxSampleRate=96000`!
   - `L4-Service-Logs`: Verify `FlacDecoder` PSRAM buffer allocation (`3 x 64 KB = 196,608 B` in `4 MB` PSRAM) and `ActiveDecoder::Flac` sample rate / bit depth wiring in `pipeline.rs` and `main.rs`.
4. **Seeds**: Embedded / MCU / IoT Hardware; Power, Thermal & Clocking Configurations (`NIST-800-53:SI-2`, `CIS-HW-1.1`, `HW-FAULT:BUS-DROPOUT`).
5. **Pre-Identified Scout Anomalies**:
   - `172.22.1.15` is currently at `vol_pct: 5` (`tas_automuted: true` during idle silence) and running the pre-`flc` `WO-156` Rev 1 binary (`MaxSampleRate=48000,pcm,wav,aif,mp3`), whereas `main.rs:33` advertises `MaxSampleRate=96000,flc,pcm,wav,aif,mp3`.
   - Check if `Esp32HardwareI2sDma::set_clock_rate` (`src/main.rs:860-900`) handles `88_200` and `96_000` Hz when `MaxSampleRate=96000` is advertised!
6. **Shell & Script Safety**: Read-only `curl` and file inspection only.
7. **Constraint**: **Read-only telemetry and code inspection. Do NOT modify any files or hardware state.**
8. **Output Schema**: Report all findings using the exact 14-field Hardware Finding Schema from `/home/console-admin/.gemini/config/skills/deep-hardware-audit/references/finding-schema.md`.

## 2. Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]`
- **Subagent**: `e809f1dd-c31a-4445-8e05-a3cc8e4f1ed1`
- **Summary**: Identified `BUS-171` (`88_200 => (7, 11, 63)` fractional clock divider skew in `main.rs:900` yielding `87,113 Hz` `-1.23%` error instead of `(7, 5, 58)` `88,200.46 Hz`) and `FW-171` (live firmware on `172.22.1.15` still advertising `MaxSampleRate=48000` awaiting `WO-156` Rev 2 OTA flash); verified `tas_automuted: true` at `vol_pct: 5` and `FlacDecoder` `196,608` B PSRAM allocation safety.
