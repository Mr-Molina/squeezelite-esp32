# Work Order: WO-231 — Wave 8C: Phase 7 Peripheral & Platform Parity — Display Stack, Controls/IR/GPIO/LED/Battery, Multi-DAC & SPDIF BMC, Sleep & Multi-Board Profiles

- **Work Order ID**: `WO-231`
- **Timestamp**: `2026-11-01T00:18:30-04:00`
- **Assigned Subagent Role**: `Rust Phase 7 Peripheral & Platform Implementer` (`TypeName: "self"`)
- **Target Host / Component**: `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/{display_gds,controls,dac_platform,power_boards}.rs`
- **Status**: `[EXECUTED & VERIFIED]`

## 1. Objective & Hypothesis
Implement all Phase 7 (Peripheral & Platform Parity) subsystems from `/home/console-admin/Github/squeezelite-esp32/docs/superpowers/plans/2026-10-29-rust-full-parity-roadmap.md` (§Phase 7) across 4 clean, self-contained leaf modules in `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/`:
1. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs` (Display Stack)**:
   - Implement `DisplayDriverKind` (`Ssd1306`, `Sh1106`, `Sh1122`, `Ssd1322`, `Ssd1327`, `Ssd1351`, `Ssd1675`, `St7735`, `St7789`, `Ili9341`), `GdsFramebuffer` (1-bpp monochrome / 4-bpp grayscale / RGB565 tile converter + init command tables per driver), LMS display protocol packet handlers (`vfdc`, `grfe`, `grfb`, `grfs`, `grfg`, `grfa`, `visu`, `ANIC`, `SETD 0xFE` display width/height report), `TextScroller`, and `ExternalMetadataScreen` (rendering BT/AirPlay/Spotify title, artist, album, and progress bar) with unit tests.
2. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/controls.rs` (Physical Controls, IR, GPIO Expanders, LEDs & Battery)**:
   - Implement `ButtonDebouncer` (debounce, short/long/repeat press, and shifter button modifier), `RotaryEncoder` (quadrature state machine + knob-only & volume-lock modes), `IrDecoder` (`NEC` 32-bit and `RC5` 14-bit pulse/space decoder -> SlimProto `IR` frame payload & local action mapper), `GpioExpanderKind` (`Pca9535`, `Pca8574`, `Mcp23017`, `Mcp23s17`, `Mpr121`, `Aw9523` register init/read transactions), `ActrlsMap` (`actrls_config` JSON action parser), `LedStripVisualizer` (WS2812 GRB byte formatter for VU meter & spectrum `ledv`/`ledd` + status LEDs), and `BatteryMonitor` (ADC raw/mV -> filtered voltage & percentage for `STAT` battery reporting + headphone jack detect & speaker fault monitor) with unit tests.
3. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/dac_platform.rs` (Multi-DAC Platform & Software SPDIF BMC Encoder)**:
   - Implement `DacModel` (`Tas5805m`, `Tas57xx`, `Tas5713`, `Ac101`, `Wm8978`, `Es8388`, `Es8311`, `GenericI2c`, `I2sPassthrough`, `SpdifOut`), I2C register init/volume/mute sequences for each DAC model, `DacConfigSpec` (`dac_config` string parser e.g. `"model=TAS5805M,bck=26,ws=25,do=22,sda=21,scl=27,mute=19:0"`), `SpdifConfigSpec` (`spdif_config` parser), `SpdifBmcEncoder` (software IEC 60958 / S/PDIF biphase-mark encoder with `B`/`M`/`W` sync preambles, 24-bit audio sample encoding, validity/user/channel-status bits, and even parity bit into 64-bit BMC subframes for I2S DMA transmission), and `audo` / jack-mutes-amp policy with unit tests.
4. **`/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/power_boards.rs` (Sleep Management & Multi-Board Hardware Profiles)**:
   - Implement `SleepManager` (inactivity timeout tracker, GPIO wake pin mask, low-battery sleep trigger, spurious-wake filter, `aude` amp power GPIO control, and `deep_sleep` / `light_sleep` state transitions) and `BoardProfile` (`LouderEsp32`, `LouderEsp32S3`, `SqueezeAmp`, `MuseProto`, `TWatch2020`, `GenericI2s` with default `dac_config`, SPI/I2C/display/button/battery pin mappings) with unit tests.

## 2. Hard Safety Constraints & Disjoint Ownership
1. **Strict Absolute Paths (`Rule 24`)**: Every file path and command must begin with `/`. Never use `sed`, `awk`, or `cat` for file modifications (`Rule 23`).
2. **Disjoint File Ownership**:
   - You **OWN** strictly the 4 new files:
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/controls.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/dac_platform.rs`
     - `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/power_boards.rs`
   - Do **NOT** modify `lib.rs`, `main.rs`, or any other existing file (`lib.rs` and `main.rs` will wire these 4 modules in Wave 9 `WO-232`).
3. **Google Rust Style & API Guidelines (`GRS-001`–`GRS-012`)**:
   - `#![forbid(unsafe_code)]`, single-module `use` statements (`GRS-001`), zero glob imports outside `#[cfg(test)]` (`GRS-002`), `#[non_exhaustive]` on public structs/enums (`GRS-009`), zero `.unwrap()` in non-test code (`GRS-010`), no `get_` prefix on getters (`GRS-011`), and line widths `<= 100` code / `<= 80` comments (`GRS-012`).
4. **No Background Tasks at Handoff (`Rule 21`)**: Verify `manage_task(list)` returns zero active tasks before completing.

## 3. Verification & Deliverables
1. Run `/usr/bin/python3 /home/console-admin/.gemini/config/plugins/rust-pro/skills/rust-pro/scripts/google_rust_audit.py /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/controls.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/dac_platform.rs /home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/power_boards.rs --strict` (`0 error(s), 0 warning(s)`).
2. Also verify each of the 4 files compiles and passes its unit tests standalone via `/home/console-admin/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc --edition 2021 --test <file> -o /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/test_<name> && /home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/test_<name>` (and delete the temporary test binaries in `/home/console-admin/.gemini/antigravity/brain/6b63bd01-ad40-4ba9-a8fa-687005ed880f/scratch/test_*` afterwards!).
3. Append Fulfillment Summary to §4 of this file and write subagent summary to `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo231_wave8c_phase7_peripherals.md`.

---

## 4. Fulfillment Section

- **Status**: `[EXECUTED & VERIFIED]`
- **Files Created**:
  1. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/display_gds.rs` (`DisplayDriverKind`, `DisplayColorMode`, `DisplayBusArbiter`, `GdsFramebuffer`, `VisualizerMode`, `LmsDisplayCommand`, `build_anic_frame`, `build_setd_display_size_frame`, `GdsDisplayEngine`, `TextScroller`, `ExternalSourceTag`, `ExternalMetadataScreen`, plus 5 unit tests).
  2. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/controls.rs` (`ControlAction`, `ButtonGesture`, `ButtonDebouncer`, `build_butn_frame`, `EncoderMode`, `RotaryEvent`, `RotaryEncoder`, `IrProtocol`, `IrFrame`, `IrDecoder`, `build_ir_frame`, `GpioExpanderKind`, `ActrlsEntry`, `ActrlsMap`, `StatusLedState`, `LedStripVisualizer`, `BatteryMonitor`, plus 5 unit tests).
  3. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/dac_platform.rs` (`DacModel`, `DacConfigSpec`, `SpdifConfigSpec`, `SpdifPreamble`, `SpdifBmcEncoder`, `AudioOutputPolicy`, plus 4 unit tests).
  4. `/home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/power_boards.rs` (`SleepState`, `SleepManager`, `BoardProfile`, plus 3 unit tests).
- **Verification Evidence**:
  - `google_rust_audit.py --strict` across all 4 files: `0 error(s), 0 warning(s)` (exit code `0`).
  - Standalone `rustc --edition 2021 --test` across all 4 files: `17 passed; 0 failed` (`5` in `display_gds.rs`, `5` in `controls.rs`, `4` in `dac_platform.rs`, `3` in `power_boards.rs`), temporary test binaries cleaned up.
- **Subagent Summary Path**: `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/subagent_summaries/20261031_wo231_wave8c_phase7_peripherals.md`
