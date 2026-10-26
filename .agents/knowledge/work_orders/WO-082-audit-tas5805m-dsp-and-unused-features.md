# Work Order: WO-082 — TI TAS5805M Digital Amplifier DSP, Modulation, Analog Gain, EQ/DRC & Telemetry Deep Audit

| Field | Value |
| :--- | :--- |
| **Work Order ID** | `WO-082` |
| **Timestamp** | `2026-10-25T11:06:00-04:00` |
| **Assigned Role** | TI TAS5805M Digital Amplifier & DSP Specialist (`Model: "pro"`, `TypeName: "research"`) |
| **Target Scope** | `rust/crates/squeezelite-tas58xx/`, `rust/crates/louder-esp32-app/`, and legacy C `tas5805m` driver (`components/adac/` / `components/squeezelite/`) |
| **Status** | `[IN PROGRESS]` |

## 1. Objective
Deeply audit our `squeezelite-tas58xx` Rust crate against the legacy C `tas5805m` driver in this repository and the TI TAS5805M register map to identify every hardware capability of the **TI TAS5805M** digital Class-D amplifier (`0x2D`) that is currently ignored, unconfigured, or suboptimally configured.

## 2. Required Inspection Areas
1. **Compare Legacy C `tas5805m` Driver vs. `rust/crates/squeezelite-tas58xx/`**:
   - Locate all TAS5805M / DAC initialization files in the C codebase (`components/`).
   - Compare every register write, book/page sequence, and configuration option against `squeezelite-tas58xx/src/lib.rs` and `registers.rs`.
2. **Audit Specific TAS5805M Hardware Capabilities**:
   - **Analog Gain (`AGAIN`, Register `0x54`)**: What is the default (`0x00` = `29.5 V` / `0 dB`) vs lower analog gain settings (`-0.5 dB` to `-15.5 dB` in `0.5 dB` steps) to match typical 12V/19V/20V USB-PD power supplies without rail clipping or hiss?
   - **PWM Modulation Scheme & Switching Frequency (`DEVICE_CTRL_1` `0x02`, `ANA_CTRL` `0x53`)**:
     - `BD` mode vs `1SPW` (1-Sided Pulse Width / Hybrid-Pro) mode, and `384 kHz` vs `768 kHz` switching frequency (`FSW`) + Spread-Spectrum clocking (`SS_CTRL0`).
   - **Digital Volume Ramp & Soft Mute (`DIG_VOL_CTRL` `0x4C`, `DIG_VOL_RAMP` `0x4E`, `AUTO_MUTE_CTRL` `0x50`, `AUTO_MUTE_TIME` `0x51`)**:
     - How fast does volume step/ramp, and is hardware automute configured properly?
   - **Built-in 48-Bit Hardware DSP (`Book 0x8C` / `Book 0x00`)**:
     - 15-biquad hardware EQ per channel, 3-band Dynamic Range Compression (DRC) / Automatic Gain Limiter (AGL), DC Blocking filter (`DSP_MISC` `0x66`), Mixer matrix (stereo / mono / L+R sub mix).
   - **Live Hardware Telemetry & Clock/Fault Monitoring (`0x37` `FS_MON`, `0x38` `BCK_MON`, `0x39` `CLKDET_STATUS`, `0x5F` `AUTOMUTE_STATE`, `0x70` `CHAN_FAULT`, `0x71` `GLOBAL_FAULT1`, `0x72` `GLOBAL_FAULT2`, `0x73` `OT_WARNING`)**:
     - Which diagnostic registers can be read over I2C (`400 kHz`) and exposed in `/status.json` + Web UI?

## 3. Deliverable
Return a structured Markdown report cataloging every TAS5805M register/feature (`Book`/`Page`/`Reg`), its current state in `squeezelite-tas58xx`, what the legacy C driver or datasheet recommends, and concrete improvements to maximize audio fidelity, SNR, thermal efficiency, and live telemetry.
