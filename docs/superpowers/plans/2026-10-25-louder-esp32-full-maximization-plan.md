# Sonocotta Louder-ESP32 Full Hardware, Silicon & DSP Maximization Implementation Plan

> **For agentic workers:** REQUIRED: Use `superpowers:subagent-driven-development` (if subagents available) or `superpowers:executing-plans` to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remediate every hardware, silicon, DSP, memory, dual-core, persistence, networking, and telemetry finding uncovered by `WO-081`, `WO-082`, and `WO-083` on `"BASEMENT"` (`172.22.1.15`, **Sonocotta Louder-ESP32**: `ESP32-WROVER` dual-core 240 MHz + 4 MB Flash + 4 MB Quad-SPI PSRAM + TI `TAS5805M` at I2C `0x2D`).

**Architecture:** We organize the remediation into **4 Dependency-Ordered Multi-Agent Waves (10 Atomic Tasks)**:
- **Wave 1 (Tasks 1–3, Parallel Disjoint Library Crates)**: Upgrade [`squeezelite-tas58xx`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-tas58xx/src/lib.rs) (TI TAS5805M register fixes, Hybrid-Pro `1SPW` modulation `0x02=0x02`, out-of-phase PWM `0x53=0x01`, Analog Gain `AGAIN` `0x54`, triangle Spread-Spectrum EMI reduction `0x6B=0x01`, volume ramp `0x4E`, automute `0x50`, and live `FS_MON`/`BCK_MON`/`CLKDET_STATUS`/`AUTOMUTE_STATE` silicon telemetry), [`squeezelite-audio-core`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-audio-core/src/equalizer.rs) (5 Schur-Cohn-stable RBJ 10-band biquad EQ presets: `Flat`, `BassBoost`, `Loudness`, `Vocal`, `Rock`), and [`squeezelite-config`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/squeezelite-config/src/lib.rs) (`AudioConfig` persistence for `analog_gain_code` and `loudness_enabled`).
- **Wave 2 (Tasks 4–6, Parallel Disjoint App Subsystems in `louder-esp32-app`)**: Upgrade [`src/amp.rs`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/amp.rs) & [`src/hal.rs`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/hal.rs), [`src/pipeline.rs`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/pipeline.rs), and [`src/http_control.rs`](file:///home/console-admin/Github/squeezelite-esp32/rust/crates/louder-esp32-app/src/http_control.rs) (interactive Web UI controls + live JSON telemetry for 4 MB PSRAM usage, Core 1 `APP_CPU` loop counter, TAS5805M `FS_MON`/`BCK_MON`/`PLL`/`Automute`/`AGAIN`, and EQ Preset buttons).
- **Wave 3 (Tasks 7–8, Core Firmware Integration & `--stage-only` Gate)**: Wire 4 MB External Quad-SPI PSRAM (`esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram)`) with **`512 KB` stream buffer + `128 KB` output buffer** in PSRAM (`32x` larger than before!), start **Core 1 (`APP_CPU` @ 240 MHz)** via `CpuControl::start_app_core`, mount `Esp32SettingsFlashPartition` (`0x3F0000`..`0x3F2000`) with `NvsFlashBackend` (`FlashJournal`) for persistent settings across reboots, seed `smoltcp` TCP from hardware `Rng`, disable Wi-Fi modem sleep (`PowerSaveMode::None`) + raise TX power (`60` = `15 dBm`), emit live `UART2` (`GPIO4`/`GPIO36`) CYD telemetry, and pass all 5 `--stage-only` gates (`.stack >= 36,000` B).
- **Wave 4 (Tasks 9–10, Final Code Review & Live OTA Certification on `172.22.1.15`)**: Independent whole-branch review, safe OTA flash from Factory recovery (`0x10000`), and end-to-end Invariant 44 verification across Web UI `/cmd.json` (EQ presets, `AGAIN` analog gain, volume, presets), 4 MB PSRAM read-back, Core 1 `APP_CPU` telemetry, TAS5805M silicon telemetry, `0x3F0000` flash persistence, AirPlay (`_raop._tcp`), UPnP (`MediaRenderer.xml`), and host Wi-Fi (`wlx98482766488b`) + Bluetooth (`hci0`).

**Tech Stack:** Rust (`edition = "2024"`, `#![no_std]`), `esp-hal 1.2.2` (`psram`, `CpuControl`, `rng::Rng`, `i2s`, `i2c`, `uart`), `esp-alloc 0.11.0` (`psram_allocator!`, `heap_allocator!`, `MemoryCapability`), `esp-radio 1.0.0-beta.1` (`PowerSaveMode::None`), `esp-storage 0.10.0` (`FlashStorage::multicore_auto_park`), `smoltcp 0.12.0`.

---

## Anti-Brick & Memory Safety Invariants (Mandatory Across All Waves)

1. **4-Gate Single-Boot Trial Protection (`src/ota_safety.rs`)**:
   - `otadata` (`0xD000..0xF000`) is erased to `0xFF` immediately on boot (`prearm_factory_fallback_on_otadata`), guaranteeing that any panic, watchdog timeout (`20s` `timg0.wdt`), or power cycle automatically boots the untouched Factory `recovery` partition (`0x10000`, `"version": "I2S-4MFlash-1681"`).
   - Never call `POST /reboot_ota.json` unless explicitly requested by the user.
2. **Never Touch `/dev/ttyUSB0` (Rule 3)**:
   - `/dev/ttyUSB0` belongs to another benchtop project and must never be probed, opened, or flashed.
3. **SPI0 Cache & PSRAM Safety During Flash I/O**:
   - On `ESP32-WROVER`, SPI Flash and Quad-SPI PSRAM (`0x3F80_0000..0x3FC0_0000`) share `SPI0`/`SPI1`.
   - Every buffer passed to `FlashStorage::read`, `FlashStorage::write`, or `FlashStorage::erase` (`load_nvs_wifi_credentials`, `prearm_factory_fallback_on_otadata`, and `Esp32SettingsFlashPartition`) **MUST** reside in internal DRAM (`.bss` or stack), NEVER in PSRAM, and `FlashStorage` **MUST** use `.multicore_auto_park()` so Core 1 (`APP_CPU`) is automatically parked while SPI0 cache is disabled.
4. **Internal Heap vs. 4 MB PSRAM Heap Isolation**:
   - `esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024)` registers Region 0 (`MemoryCapability::Internal`, `64 KB`) for `esp-radio` Wi-Fi DMA/OSI allocations.
   - `esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram)` registers Region 1 (`MemoryCapability::External`, `4,194,304` B).
   - Audio ringbuffers (`psram_stream_storage = alloc::vec![0u8; 524_288]` and `psram_output_storage = alloc::vec![0u8; 131_072]`) are both `> 65,536` B, so `esp_alloc` deterministically places 100% of both buffers in Region 1 (`External` 4 MB PSRAM) while leaving 100% of Region 0 (`64 KB` internal heap) free for `esp-radio`.
   - Replacing `ring_storage: [u8; 16384]` in `FirmwareBssBuffers` with `nvs_scratch: [u8; 12288]` (`12 KB` internal DRAM) + `APP_CORE_STACK: Stack<4096>` (`4 KB` internal DRAM) keeps the total `.bss` footprint and Core 0 `.stack` (`36,528` B = `0x008eb0`) byte-for-byte identical to battle-tested commit `0e2f4732`.

---

## File Modification & Ownership Matrix

| Wave | Task | Assigned Subagent Role | Files Modified | Overlap |
| :--- | :--- | :--- | :--- | :--- |
| **Wave 1** | **Task 1** | TAS5805M Silicon Driver Implementer | `rust/crates/squeezelite-tas58xx/src/lib.rs` | None |
| **Wave 1** | **Task 2** | DSP & Equalizer Preset Implementer | `rust/crates/squeezelite-audio-core/src/equalizer.rs`, `rust/crates/squeezelite-audio-core/src/lib.rs` | None |
| **Wave 1** | **Task 3** | NVS FlashJournal Schema Implementer | `rust/crates/squeezelite-config/src/lib.rs` | None |
| **Wave 2** | **Task 4** | App Amplifier & Telemetry Implementer | `rust/crates/louder-esp32-app/src/amp.rs`, `rust/crates/louder-esp32-app/src/hal.rs` | None |
| **Wave 2** | **Task 5** | App Stream Pipeline EQ Implementer | `rust/crates/louder-esp32-app/src/pipeline.rs` | None |
| **Wave 2** | **Task 6** | Web UI Dashboard & Control Implementer | `rust/crates/louder-esp32-app/src/http_control.rs` | None |
| **Wave 3** | **Task 7** | ESP32 PSRAM, Dual-Core & NVS Integrator | `rust/crates/louder-esp32-app/src/main.rs` | None |
| **Wave 3** | **Task 8** | Stage-Only Build & Stack Gate Certifier | `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` | None |
| **Wave 4** | **Task 9** | Final Whole-Branch Code Reviewer (`pro`) | Read-only audit of `git diff` & staged `.bin` | None |
| **Wave 4** | **Task 10** | Live Hardware & Connectivity Certifier | Live OTA flash to `172.22.1.15` & Invariant 44 verification | None |

---

## Wave 1: Parallel Disjoint Library Crate Upgrades (Tasks 1–3)

### Task 1: TI TAS5805M Silicon Maximization, Register Fixes & Live Telemetry (`squeezelite-tas58xx`)

**Files:**
- Modify: `rust/crates/squeezelite-tas58xx/src/lib.rs`

- [ ] **Step 1: Write failing unit tests in `rust/crates/squeezelite-tas58xx/src/lib.rs` for corrected register addresses, fault bitmasks, `configure_silicon_maximization`, `set_analog_gain`, and `read_telemetry`**

Add the following tests inside `mod tests` at the bottom of `rust/crates/squeezelite-tas58xx/src/lib.rs`:

```rust
    #[test]
    fn test_datasheet_register_addresses_and_fault_bitmasks() {
        assert_eq!(registers::FS_MON, 0x37);
        assert_eq!(registers::BCK_MON, 0x38);
        assert_eq!(registers::CLKDET_STATUS, 0x39);
        assert_eq!(registers::DIG_VOL_LEFT, 0x4C);
        assert_eq!(registers::DIG_VOL_RIGHT, 0x4D);
        assert_eq!(registers::DIG_VOL_CTRL2, 0x4E);
        assert_eq!(registers::AUTO_MUTE_CTRL, 0x50);
        assert_eq!(registers::AUTO_MUTE_TIME, 0x51);
        assert_eq!(registers::ANA_CTRL, 0x53);
        assert_eq!(registers::AGAIN, 0x54);
        assert_eq!(registers::DSP_MISC, 0x66);
        assert_eq!(registers::AUTOMUTE_STATE, 0x69);
        assert_eq!(registers::SS_CTRL0, 0x6B);

        assert_eq!(registers::FAULT1_PVDD_UV, 0x01);
        assert_eq!(registers::FAULT1_PVDD_OV, 0x02);
        assert_eq!(registers::FAULT1_CLOCK_FAULT, 0x04);
        assert_eq!(registers::FAULT1_BQ_WR_ERROR, 0x20);
        assert_eq!(registers::FAULT1_OTP_CRC_ERROR, 0x80);
        assert_eq!(registers::FAULT2_OTSD, 0x02);
        assert_eq!(registers::FAULT2_OTW, 0x08);
    }

    #[test]
    fn test_configure_silicon_maximization_and_telemetry() {
        let mut i2c = MockI2c::default();
        i2c.die_id = 0x00;
        let driver = Tas58xx::new(i2c, 0x2D);
        let mut standby = driver.init().expect("init ok");

        standby
            .configure_silicon_maximization(0x08)
            .expect("silicon maximization ok");

        let mut play = standby.enable_play().expect("play ok");
        play.set_analog_gain(0x0C).expect("set_analog_gain ok");

        let telem = play.read_telemetry().expect("read_telemetry ok");
        assert_eq!(telem.detected_sample_rate_hz(), 48_000);
        assert_eq!(telem.bck_ratio(), 64);
        assert!(telem.is_pll_locked());
        assert!(!telem.is_automuted());
    }
```

- [ ] **Step 2: Run `cargo test -p squeezelite-tas58xx` to verify the new tests fail to compile**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx`
Expected: Compilation failure (`registers::FS_MON`, `configure_silicon_maximization`, `read_telemetry` not found).

- [ ] **Step 3: Implement the datasheet-accurate register constants, fault bitmasks, `Tas5805mTelemetry`, `configure_silicon_maximization`, `set_analog_gain`, and `read_telemetry` in `rust/crates/squeezelite-tas58xx/src/lib.rs`**

1. In `pub mod registers` (`rust/crates/squeezelite-tas58xx/src/lib.rs` lines 48–126), update and add the exact TI TAS5805M (`SLASEH5B`) register constants and bitmasks:
```rust
    /// `FS_MON`: Detected I2S frame sync (LRCLK) frequency monitor (`0x37`).
    pub const FS_MON: u8 = 0x37;
    /// `BCK_MON`: Detected BCK-to-FS clock ratio monitor (`0x38`).
    pub const BCK_MON: u8 = 0x38;
    /// `CLKDET_STATUS`: Clock detector & PLL lock status (`0x39`, bit 3 = PLL locked).
    pub const CLKDET_STATUS: u8 = 0x39;
    /// `DIG_VOL_LEFT` / `DIG_VOL_CTRL`: Master digital volume (`0x4C`, `0.5 dB`/step, `0x30` = `0.0 dB`).
    pub const DIG_VOL_LEFT: u8 = 0x4C;
    /// `DIG_VOL_RIGHT`: Digital volume ramp / fine control (`0x4D`).
    pub const DIG_VOL_RIGHT: u8 = 0x4D;
    /// `DIG_VOL_CTRL2`: Digital volume ramp rate configuration (`0x4E`).
    pub const DIG_VOL_CTRL2: u8 = 0x4E;
    /// `AUTO_MUTE_CTRL`: Hardware automute enable (`0x50`, `0x07` = L/R independent + grouped automute).
    pub const AUTO_MUTE_CTRL: u8 = 0x50;
    /// `AUTO_MUTE_TIME`: Hardware automute threshold time (`0x51`).
    pub const AUTO_MUTE_TIME: u8 = 0x51;
    /// `ANA_CTRL`: Class-D analog modulation phase control (`0x53`, `0x01` = L/R out-of-phase PWM to halve PVDD ripple).
    pub const ANA_CTRL: u8 = 0x53;
    /// `AGAIN`: Analog gain attenuation (`0x54`, bits `4:0`, `0.5 dB`/step from `0x00` = `0.0 dB` / `29.5 V` down to `0x1F` = `-15.5 dB`).
    pub const AGAIN: u8 = 0x54;
    /// `DSP_MISC`: DSP miscellaneous control (`0x66`).
    pub const DSP_MISC: u8 = 0x66;
    /// `AUTOMUTE_STATE`: Live L/R hardware automute status flags (`0x69`, bit 0 = Left muted, bit 1 = Right muted).
    pub const AUTOMUTE_STATE: u8 = 0x69;
    /// `SS_CTRL0`: Spread-spectrum PWM EMI reduction (`0x6B`, `0x01` = Triangle spread spectrum enabled).
    pub const SS_CTRL0: u8 = 0x6B;

    /// `DEVICE_CTRL_1` value: 1SPW (Hybrid-Pro) modulation, BTL stereo, 384 kHz FSW (`0x02`).
    pub const CTRL1_1SPW_384KHZ_BTL: u8 = 0x02;

    // GLOBAL_FAULT1 (0x71) bitmasks per TI TAS5805M datasheet (SLASEH5B Section 7.6.1.29):
    /// Bit 0: PVDD Undervoltage fault (`PVDD_UV_FAULT`).
    pub const FAULT1_PVDD_UV: u8 = 0x01;
    /// Bit 1: PVDD Overvoltage fault (`PVDD_OV_FAULT`).
    pub const FAULT1_PVDD_OV: u8 = 0x02;
    /// Bit 2: I2S Clock fault (`CLK_FAULT`).
    pub const FAULT1_CLOCK_FAULT: u8 = 0x04;
    /// Bit 5: Biquad coefficient write error (`BQ_WR_ERROR`).
    pub const FAULT1_BQ_WR_ERROR: u8 = 0x20;
    /// Bit 7: OTP CRC error (`OTP_CRC_ERROR`).
    pub const FAULT1_OTP_CRC_ERROR: u8 = 0x80;

    // GLOBAL_FAULT2 (0x72) and OT_WARNING (0x73) bitmasks:
    /// `GLOBAL_FAULT2` (0x72) Overtemperature shut-down (`OTSD_FAULT`).
    pub const FAULT2_OTSD: u8 = 0x02;
    /// `GLOBAL_FAULT2` (0x72) / `OT_WARNING` (0x73) Overtemperature warning (`OTW`, 135 C).
    pub const FAULT2_OTW: u8 = 0x08;
```
(Update `decode_faults` in `src/lib.rs` to check `(FaultStatus.ot_warning & 0x02) != 0 || (FaultStatus.global_fault2 & registers::FAULT2_OTW) != 0` so both hardware `0x73` bit 1 and existing unit tests pass.)

2. Define `Tas5805mTelemetry` in `rust/crates/squeezelite-tas58xx/src/lib.rs` and re-export it at crate root:
```rust
/// Live silicon telemetry read from the TI TAS5805M (`0x37..=0x39` and `0x69`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tas5805mTelemetry {
    /// Raw `FS_MON` (`0x37`) register value.
    pub fs_mon: u8,
    /// Raw `BCK_MON` (`0x38`) register value.
    pub bck_mon: u8,
    /// Raw `CLKDET_STATUS` (`0x39`) register value.
    pub clkdet_status: u8,
    /// Raw `AUTOMUTE_STATE` (`0x69`) register value.
    pub automute_state: u8,
}

impl Tas5805mTelemetry {
    /// Decodes `FS_MON` (`0x37` bits `3:0`) into the detected sample rate in Hz.
    #[must_use]
    pub const fn detected_sample_rate_hz(&self) -> u32 {
        match self.fs_mon & 0x0F {
            0x02 => 8_000,
            0x04 => 16_000,
            0x06 => 32_000,
            0x08 => 44_100,
            0x09 => 48_000,
            0x0B => 96_000,
            _ => 0,
        }
    }

    /// Returns the detected BCK-to-FS ratio (`BCK_MON` `0x38`).
    #[must_use]
    pub const fn bck_ratio(&self) -> u16 {
        self.bck_mon as u16
    }

    /// Returns `true` if `CLKDET_STATUS` (`0x39`) bit 3 indicates internal PLL lock (`1`) and no clock error.
    #[must_use]
    pub const fn is_pll_locked(&self) -> bool {
        (self.clkdet_status & 0x08) != 0
    }

    /// Returns `true` if both Left and Right channels are currently hardware-automuted (`0x69` bits `1:0` == `0b11`).
    #[must_use]
    pub const fn is_automuted(&self) -> bool {
        (self.automute_state & 0x03) == 0x03
    }
}
```

3. Add `configure_silicon_maximization(&mut self, again_code: u8) -> Result<(), DriverError<E>>`, `set_analog_gain(&mut self, again_code: u8) -> Result<(), DriverError<E>>`, and `read_telemetry(&mut self) -> Result<Tas5805mTelemetry, DriverError<E>>` to both `Tas58xx<I2C, Standby>` and `Tas58xx<I2C, Play>`:
   - `configure_silicon_maximization(again_code)`:
     1. `self.select_page0_book0()?;`
     2. `self.write_reg(registers::DEVICE_CTRL_1, registers::CTRL1_1SPW_384KHZ_BTL)?;` (`0x02 = 0x02`, Hybrid-Pro 1SPW modulation)
     3. `self.write_reg(registers::ANA_CTRL, 0x01)?;` (`0x53 = 0x01`, out-of-phase L/R PWM to cut PVDD bus ripple in half)
     4. `self.write_reg(registers::AGAIN, again_code.min(0x1F))?;` (`0x54`, analog gain attenuation)
     5. `self.write_reg(registers::DIG_VOL_CTRL2, 0x00)?;` (`0x4E = 0x00`, smooth `0.5 dB` every sample period ramp)
     6. `self.write_reg(registers::AUTO_MUTE_CTRL, 0x07)?;` (`0x50 = 0x07`, enable L/R hardware automute)
     7. `self.write_reg(registers::SS_CTRL0, 0x01)?;` (`0x6B = 0x01`, enable triangle spread-spectrum EMI reduction)
   - `set_analog_gain(again_code)`:
     1. `self.select_page0_book0()?;`
     2. `self.write_reg(registers::AGAIN, again_code.min(0x1F))`
   - `read_telemetry()`:
     1. `self.select_page0_book0()?;`
     2. Read `FS_MON (0x37)`, `BCK_MON (0x38)`, `CLKDET_STATUS (0x39)`, and `AUTOMUTE_STATE (0x69)`, returning `Tas5805mTelemetry`.
   - In `MockI2c` inside `mod tests`, return `0x09` (`48 kHz`) for `0x37`, `64` for `0x38`, `0x08` (`PLL locked`) for `0x39`, and `0x00` for `0x69` by default so unit tests verify the telemetry decoding.

- [ ] **Step 4: Run `cargo test -p squeezelite-tas58xx` and `cargo clippy -p squeezelite-tas58xx`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx && cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-tas58xx --all-targets -- -D warnings`
Expected: All unit tests pass (`exit code 0`), `0` Clippy warnings.

---

### Task 2: 10-Band Biquad Equalizer Presets (`Flat`, `BassBoost`, `Loudness`, `Vocal`, `Rock`) (`squeezelite-audio-core`)

**Files:**
- Modify: `rust/crates/squeezelite-audio-core/src/equalizer.rs`
- Modify: `rust/crates/squeezelite-audio-core/src/lib.rs`

- [ ] **Step 1: Write failing unit tests in `rust/crates/squeezelite-audio-core/src/equalizer.rs` for `EqPreset` and `Equalizer::<10>::apply_preset`**

Add the following tests to `mod tests` in `rust/crates/squeezelite-audio-core/src/equalizer.rs`:

```rust
    #[test]
    fn test_eq_presets_are_schur_cohn_stable_and_modify_spectrum() {
        let mut eq: Equalizer<10> = Equalizer::new();
        assert!(!eq.is_enabled());

        for code in 1..=4u8 {
            let preset = EqPreset::from_u8(code);
            eq.apply_preset(preset);
            assert!(eq.is_enabled(), "preset {:?} must enable EQ", preset);
        }

        // Verify BassBoost amplifies a low-frequency 62.5 Hz sine wave
        eq.apply_preset(EqPreset::BassBoost);
        let mut peak_out = 0i32;
        for n in 0..2048 {
            let theta = (n as f32) * core::f32::consts::TAU * 62.5 / 44_100.0;
            // Taylor-series / Bhaskara sine approximation for no_std test compatibility
            let x = theta % core::f32::consts::TAU;
            let x_pi = if x > core::f32::consts::PI { x - core::f32::consts::TAU } else { x };
            let sin_val = (16.0 * x_pi * (core::f32::consts::PI - x_pi.abs()))
                / (5.0 * core::f32::consts::PI * core::f32::consts::PI
                    - 4.0 * x_pi.abs() * (core::f32::consts::PI - x_pi.abs()));
            let sample = (sin_val * 100_000.0) as i32;
            let (l, _) = eq.process_frame(sample, sample);
            if n > 512 && l.abs() > peak_out {
                peak_out = l.abs();
            }
        }
        assert!(peak_out > 125_000, "BassBoost should boost 62.5 Hz (peak={peak_out})");

        // Verify Flat bypasses EQ (0-cycle passthrough)
        eq.apply_preset(EqPreset::Flat);
        assert!(!eq.is_enabled());
        assert_eq!(eq.process_frame(123_456, -654_321), (123_456, -654_321));
    }
```

- [ ] **Step 2: Run `cargo test -p squeezelite-audio-core` to verify failure**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core`
Expected: Compilation failure (`EqPreset`, `is_enabled`, and `apply_preset` not defined).

- [ ] **Step 3: Implement `EqPreset`, `BiquadCoeffs::BYPASS`, helper methods (`is_enabled`, `set_enabled`, `reset`, `set_band`), and `Equalizer::<10>::apply_preset` in `rust/crates/squeezelite-audio-core/src/equalizer.rs` and re-export in `src/lib.rs`**

1. In `rust/crates/squeezelite-audio-core/src/equalizer.rs`, add `pub const BYPASS: Self = Self::bypass();` on `BiquadCoeffs`, and add helper methods on `Equalizer<N>`:
   - `pub const fn is_enabled(&self) -> bool { self.enabled }`
   - `pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }`
   - `pub fn reset(&mut self) { self.reset_history(); }`
   - `pub fn set_band(&mut self, index: usize, coeffs: BiquadCoeffs) -> bool { self.set_filter(index, coeffs).is_ok() }`
   And define `EqPreset`:
```rust
/// Built-in 10-band ISO graphic equalizer presets (`31.25 Hz` .. `16 kHz` @ `44.1 kHz`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EqPreset {
    /// Bit-perfect 0-cycle bypass (`0.0 dB` across all 10 bands).
    #[default]
    Flat = 0,
    /// Warm low-end shelf (`+5.5 dB` @ 31 Hz, `+4.5 dB` @ 62 Hz, `+3.0 dB` @ 125 Hz, `+1.5 dB` @ 250 Hz).
    BassBoost = 1,
    /// Fletcher-Munson loudness contour (`+4.5 dB` sub-bass, `+3.5 dB` bass, `-1.0 dB` 1 kHz, `+3.0 dB` 8-16 kHz air).
    Loudness = 2,
    /// Midrange presence boost for spoken word/radio (`-2.0 dB` sub-bass, `+3.5 dB` 1-2 kHz, `+2.5 dB` 4 kHz).
    Vocal = 3,
    /// Classic V-curve rock contour (`+4.0 dB` 62-125 Hz, `-1.5 dB` 500 Hz-1 kHz, `+3.5 dB` 4-8 kHz).
    Rock = 4,
}

impl EqPreset {
    /// Converts a numeric preset index (`0..=4`) into an `EqPreset` (unknown values map to `Flat`).
    #[must_use]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::BassBoost,
            2 => Self::Loudness,
            3 => Self::Vocal,
            4 => Self::Rock,
            _ => Self::Flat,
        }
    }

    /// Returns the canonical short name for JSON telemetry and Web UI display.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::BassBoost => "Bass",
            Self::Loudness => "Loudness",
            Self::Vocal => "Vocal",
            Self::Rock => "Rock",
        }
    }
}
```
2. Add precomputed, Schur-Cohn-verified (`is_stable() == true`) RBJ peaking/shelving biquad coefficient tables (`[BiquadCoeffs; 10]`) for `BassBoost`, `Loudness`, `Vocal`, and `Rock` at `44,100 Hz` (`Q = 1.414`, ISO octave centers `31.25`, `62.5`, `125`, `250`, `500`, `1000`, `2000`, `4000`, `8000`, `16000` Hz), and implement `impl Equalizer<10>`:
   - `pub fn apply_preset(&mut self, preset: EqPreset)`:
     - When `EqPreset::Flat`, resets all bands to `BiquadCoeffs::BYPASS`, clears history (`self.reset()`), and sets `self.set_enabled(false)`.
     - For `BassBoost`, `Loudness`, `Vocal`, and `Rock`, loads all 10 bands via `self.set_band(idx, coeffs)` (returning `debug_assert!(ok)`), resets delay history (`self.reset()`), and sets `self.set_enabled(true)`.
3. Re-export `EqPreset` in `rust/crates/squeezelite-audio-core/src/lib.rs`:
   `pub use equalizer::{BiquadCoeffs, EqPreset, Equalizer};`

- [ ] **Step 4: Run `cargo test -p squeezelite-audio-core` and `cargo clippy -p squeezelite-audio-core`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core && cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-audio-core --all-targets -- -D warnings`
Expected: All unit tests pass (`exit code 0`), `0` Clippy warnings.

---

### Task 3: Persistent Analog Gain & Loudness Schema in `FlashJournal` (`squeezelite-config`)

**Files:**
- Modify: `rust/crates/squeezelite-config/src/lib.rs`

- [ ] **Step 1: Write a failing test in `rust/crates/squeezelite-config/src/lib.rs` verifying `analog_gain_code` and `loudness_enabled` round-trip and backwards compatibility**

Add to `mod tests` in `rust/crates/squeezelite-config/src/lib.rs`:

```rust
    #[test]
    fn test_audio_config_analog_gain_and_loudness_roundtrip() {
        let mut store = MemConfigStore::<16>::new();
        let mut sector_a = [0xFFu8; SECTOR_SIZE];
        let mut sector_b = [0xFFu8; SECTOR_SIZE];

        let mut cfg = AudioConfig::default();
        assert_eq!(cfg.analog_gain_code, 8); // Default -4.0 dB (0x08)
        assert!(!cfg.loudness_enabled);

        cfg.volume = 55;
        cfg.equalizer_preset = 2;
        cfg.analog_gain_code = 12; // -6.0 dB (0x0C)
        cfg.loudness_enabled = true;

        cfg.save_to(&mut store).expect("save_to ok");
        let seq = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b)
            .expect("journal save ok");
        assert_eq!(seq, 1);

        let (loaded_store, loaded_seq) =
            FlashJournal::load_from_dual_sectors(&sector_a, &sector_b).expect("journal load ok");
        assert_eq!(loaded_seq, 1);
        let loaded = AudioConfig::load_from(&loaded_store).expect("load_from ok");
        assert_eq!(loaded.volume, 55);
        assert_eq!(loaded.equalizer_preset, 2);
        assert_eq!(loaded.analog_gain_code, 12);
        assert!(loaded.loudness_enabled);
    }
```

- [ ] **Step 2: Run `cargo test -p squeezelite-config` to verify failure**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config`
Expected: Compilation failure (`analog_gain_code` and `loudness_enabled` missing on `AudioConfig`).

- [ ] **Step 3: Extend `AudioConfig` in `rust/crates/squeezelite-config/src/lib.rs` with `analog_gain_code` and `loudness_enabled`**

1. Add constants:
   - `const KEY_AGAIN: &str = "audio.again";`
   - `const KEY_LOUD: &str = "audio.loud";`
   - `pub const DEFAULT_ANALOG_GAIN_CODE: u8 = 0x08;` (`-4.0 dB` for 20V USB-PD headroom)
   - `pub const MAX_ANALOG_GAIN_CODE: u8 = 0x1F;` (`-15.5 dB`)
2. Extend `AudioConfig`:
   - `pub analog_gain_code: u8` (default `DEFAULT_ANALOG_GAIN_CODE = 8`)
   - `pub loudness_enabled: bool` (default `false`)
3. In `AudioConfig::validate()`, validate `self.analog_gain_code <= MAX_ANALOG_GAIN_CODE` (returning `Err(ConfigError::InvalidValue)` if `> 0x1F`).
4. In `AudioConfig::save_to(&self, store: &mut MemConfigStore<N>)`, write `KEY_AGAIN` (`u32::from(self.analog_gain_code)`) and `KEY_LOUD` (`self.loudness_enabled`).
5. In `AudioConfig::load_from(store: &MemConfigStore<N>)`, read `analog_gain_code` from `store.get_u32(KEY_AGAIN)` with fallback to `DEFAULT_ANALOG_GAIN_CODE` (`8`) when `Err(ConfigError::KeyNotFound)`, and read `loudness_enabled` from `store.get_bool(KEY_LOUD)` with fallback to `false` when `Err(ConfigError::KeyNotFound)`, preserving 100% backwards compatibility with existing flash sectors.

- [ ] **Step 4: Run `cargo test -p squeezelite-config` and `cargo clippy -p squeezelite-config`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config && cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p squeezelite-config --all-targets -- -D warnings`
Expected: All unit tests pass (`exit code 0`), `0` Clippy warnings.

---

## Wave 2: Parallel App Subsystem Upgrades in `louder-esp32-app` (Tasks 4–6)

### Task 4: Amplifier Manager Silicon Maximization & Live Telemetry (`src/amp.rs` & `src/hal.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/amp.rs`
- Modify: `rust/crates/louder-esp32-app/src/hal.rs`

- [ ] **Step 1: Write failing unit tests in `rust/crates/louder-esp32-app/src/amp.rs` for `configure_silicon_maximization` during `AmpManager::initialize`, `set_analog_gain`, and `read_telemetry`**

Add to `mod tests` in `rust/crates/louder-esp32-app/src/amp.rs`:

```rust
    #[test]
    fn test_amp_manager_silicon_maximization_and_telemetry() {
        let i2c = TargetI2cBus::new();
        let mut amp = AmpManager::with_config(i2c, AmpConfig::default());
        amp.initialize(32).expect("init ok");
        assert_eq!(amp.analog_gain_code(), 0x08);

        amp.start_playback_via_hiz().expect("play ok");
        amp.set_analog_gain(0x0C).expect("set_analog_gain ok");
        assert_eq!(amp.analog_gain_code(), 0x0C);

        let telem = amp.read_telemetry().expect("telemetry ok").expect("some telemetry");
        assert_eq!(telem.detected_sample_rate_hz(), 44_100);
        assert_eq!(telem.bck_ratio(), 64);
        assert!(telem.is_pll_locked());
    }
```

- [ ] **Step 2: Run `cargo test -p louder-esp32-app` to verify failure**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app`
Expected: Compilation failure (`analog_gain_code`, `set_analog_gain`, `read_telemetry` missing on `AmpManager`).

- [ ] **Step 3: Implement `analog_gain_code`, `configure_silicon_maximization`, `set_analog_gain`, and `read_telemetry` in `src/amp.rs` and update `TargetI2cBus` in `src/hal.rs`**

1. In `rust/crates/louder-esp32-app/src/hal.rs` (`TargetI2cBus::new`), initialize simulated TAS5805M telemetry registers in `tas5805_regs`:
   - `tas5805_regs[0x37] = 0x08;` (`44.1 kHz`)
   - `tas5805_regs[0x38] = 64;` (`64 FS`)
   - `tas5805_regs[0x39] = 0x08;` (`PLL locked`)
   - `tas5805_regs[0x54] = 0x08;` (`-4.0 dB` `AGAIN`)
2. In `rust/crates/louder-esp32-app/src/amp.rs`:
   - Add `pub initial_again_code: u8` to `AmpConfig` (default `0x08` = `-4.0 dB`).
   - Add `current_again_code: u8` to `AmpManager`.
   - In `AmpManager::initialize(&mut self, word_length_bits: u8)`: after `d.init()`, call `standby.configure_silicon_maximization(self.current_again_code)?;` before `standby.set_digital_volume(self.current_vol_reg)?;`.
   - Add `pub fn analog_gain_code(&self) -> u8`, `pub fn set_analog_gain(&mut self, again_code: u8) -> Result<(), DriverError<I2C::Error>>`, and `pub fn read_telemetry(&mut self) -> Result<Option<squeezelite_tas58xx::Tas5805mTelemetry>, DriverError<I2C::Error>>`.

- [ ] **Step 4: Run `cargo test -p louder-esp32-app` for `amp` and `hal` modules**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app amp`
Expected: All `amp` tests pass (`exit code 0`).

---

### Task 5: Stream Pipeline 10-Band Equalizer Preset Integration (`src/pipeline.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/pipeline.rs`

- [ ] **Step 1: Write a failing unit test in `rust/crates/louder-esp32-app/src/pipeline.rs` for `apply_eq_preset` and `eq_preset()`**

Add to `mod tests` in `rust/crates/louder-esp32-app/src/pipeline.rs`:

```rust
    #[test]
    fn test_pipeline_apply_eq_preset() {
        let mut stream_mem = [0u8; 4096];
        let mut out_mem = [0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            MockAudioSink::<64>::new(),
            44_100,
        );
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::Flat);

        pipeline.apply_eq_preset(squeezelite_audio_core::EqPreset::BassBoost);
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::BassBoost);
        assert!(pipeline.equalizer.is_enabled());

        pipeline.apply_eq_preset(squeezelite_audio_core::EqPreset::Flat);
        assert_eq!(pipeline.eq_preset(), squeezelite_audio_core::EqPreset::Flat);
        assert!(!pipeline.equalizer.is_enabled());
    }
```

- [ ] **Step 2: Run `cargo test -p louder-esp32-app pipeline` to verify failure**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app pipeline`
Expected: Compilation failure (`eq_preset` and `apply_eq_preset` not found).

- [ ] **Step 3: Add `current_eq_preset: EqPreset`, `apply_eq_preset`, and `eq_preset` to `StreamPipeline` in `rust/crates/louder-esp32-app/src/pipeline.rs`**

1. Add `current_eq_preset: squeezelite_audio_core::EqPreset` to `StreamPipeline<'a, S>` (initialized to `EqPreset::Flat` in `StreamPipeline::new`).
2. Add methods on `StreamPipeline<'a, S>`:
```rust
    /// Applies a 10-band biquad graphic equalizer preset to the audio pipeline.
    pub fn apply_eq_preset(&mut self, preset: squeezelite_audio_core::EqPreset) {
        self.current_eq_preset = preset;
        self.equalizer.apply_preset(preset);
    }

    /// Returns the currently active 10-band equalizer preset.
    #[must_use]
    pub const fn eq_preset(&self) -> squeezelite_audio_core::EqPreset {
        self.current_eq_preset
    }
```

- [ ] **Step 4: Run `cargo test -p louder-esp32-app pipeline`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app pipeline`
Expected: All `pipeline` tests pass (`exit code 0`).

---

### Task 6: Interactive Web UI & Extended Hardware Telemetry API (`src/http_control.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/http_control.rs`

- [ ] **Step 1: Write failing unit tests in `rust/crates/louder-esp32-app/src/http_control.rs` for the new `/cmd.json` EQ & `AGAIN` actions and extended `/status.json` telemetry fields**

Update and add tests in `mod tests` of `rust/crates/louder-esp32-app/src/http_control.rs`:

```rust
    #[test]
    fn test_cmd_json_eq_and_again_actions() {
        let mut scratch = [0u8; 1024];
        for (act, expected_action) in [
            ("eq_flat", HttpControlAction::SetEqPreset(0)),
            ("eq_bass", HttpControlAction::SetEqPreset(1)),
            ("eq_loudness", HttpControlAction::SetEqPreset(2)),
            ("eq_vocal", HttpControlAction::SetEqPreset(3)),
            ("eq_rock", HttpControlAction::SetEqPreset(4)),
            ("again0", HttpControlAction::SetAnalogGain(0x00)),
            ("again4", HttpControlAction::SetAnalogGain(0x08)),
            ("again6", HttpControlAction::SetAnalogGain(0x0C)),
            ("again8", HttpControlAction::SetAnalogGain(0x10)),
        ] {
            let req = alloc::format!("GET /cmd.json?act={act} HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n");
            let (len, action) =
                handle_http_control_request(req.as_bytes(), &sample_status(), &mut scratch)
                    .expect("valid response");
            assert_eq!(action, expected_action);
            let text = core::str::from_utf8(&scratch[..len]).expect("utf8");
            assert!(text.contains("HTTP/1.1 200 OK"));
        }
    }
```

- [ ] **Step 2: Run `cargo test -p louder-esp32-app http_control` to verify failure**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app http_control`
Expected: Compilation failure (`SetEqPreset` and `SetAnalogGain` variants missing on `HttpControlAction`).

- [ ] **Step 3: Extend `HttpControlAction`, `HttpStatusSnapshot`, `write_status_json`, and `DASHBOARD_HTML` in `rust/crates/louder-esp32-app/src/http_control.rs`**

1. Extend `HttpControlAction` with:
   - `SetEqPreset(u8)` (`0 = Flat`, `1 = Bass`, `2 = Loudness`, `3 = Vocal`, `4 = Rock`)
   - `SetAnalogGain(u8)` (`0x00 = 0 dB`, `0x08 = -4 dB`, `0x0C = -6 dB`, `0x10 = -8 dB`)
2. Extend `HttpStatusSnapshot<'a>` with:
   - `pub psram_free_bytes: u32`
   - `pub psram_used_bytes: u32`
   - `pub core1_active: bool`
   - `pub core1_loops: u32`
   - `pub tas_fs_hz: u32`
   - `pub tas_bck_ratio: u16`
   - `pub tas_pll_locked: bool`
   - `pub tas_automuted: bool`
   - `pub again_code: u8`
   - `pub eq_preset: &'a str`
   - `pub settings_seq: u32`
3. Update `write_status_json` to emit all new fields in `/status.json` (increasing stack buffer inside `handle_http_control_request` from `512` B to `768` B so the expanded JSON never truncates).
4. Update `DASHBOARD_HTML` to include:
   - Live telemetry badges for **PSRAM (`used / free KB`)**, **Dual-Core (`Core 1 loops`)**, **TAS5805M Silicon (`FS Hz`, `BCK ratio`, `PLL Lock`, `AGAIN dB`)**, **Active 10-Band EQ Preset**, and **NVS FlashJournal Sequence**.
   - Interactive **10-Band EQ Preset** buttons (`Flat`, `Bass Boost`, `Loudness`, `Vocal`, `Rock`) calling `cmd('eq_flat')`, `cmd('eq_bass')`, `cmd('eq_loudness')`, `cmd('eq_vocal')`, `cmd('eq_rock')`.
   - Interactive **Analog Gain (`AGAIN`)** buttons (`0 dB (29.5V)`, `-4 dB (20V PD)`, `-6 dB (15V)`, `-8 dB (12V)`) calling `cmd('again0')`, `cmd('again4')`, `cmd('again6')`, `cmd('again8')`.

- [ ] **Step 4: Run `cargo test -p louder-esp32-app` and `cargo clippy -p louder-esp32-app`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app && cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml -p louder-esp32-app --all-targets -- -D warnings`
Expected: All host unit tests compile and pass (once `main.rs` test helpers are updated in Task 7).

---

## Wave 3: Core Firmware Integration & `--stage-only` Verification (Tasks 7–8)

### Task 7: Wire 4 MB PSRAM, Dual-Core `APP_CPU`, `0x3F0000` `FlashJournal`, Hardware `Rng`, `PowerSaveMode::None`, and Live `UART2` Telemetry (`src/main.rs`)

**Files:**
- Modify: `rust/crates/louder-esp32-app/src/main.rs`

- [ ] **Step 1: Update `FirmwareBssBuffers`, `load_nvs_wifi_credentials`, and `Esp32SettingsFlashPartition` in `rust/crates/louder-esp32-app/src/main.rs`**

1. In `FirmwareBssBuffers` (`src/main.rs:473-509`):
   - Replace `ring_storage: [u8; 16384]` with `nvs_scratch: [u8; 12288]` (`12 KB` internal DRAM for SPI0-cache-safe NVS reads).
   - Add `static mut APP_CORE_STACK: esp_hal::system::Stack<4096> = esp_hal::system::Stack::new();` (`4 KB` internal DRAM) and `static CORE1_HEARTBEAT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);`.
   - Notice `12,288 + 4,096 = 16,384` bytes: total `.bss` and Core 0 `.stack` (`36,528` B) remain byte-for-byte identical to `0e2f4732`!
2. In `load_nvs_wifi_credentials` (`src/main.rs:514-527`):
   - Take `nvs_buf: &mut [u8; 12288]`.
   - First read `0x9000..0xC000` (`12,288` B) from `flash` and call `parse_nvs_wifi_credentials(nvs_buf, sta_mac)`. If that returns `Err`, read `0xA000..0xD000` (`12,288` B) and call `parse_nvs_wifi_credentials(nvs_buf, sta_mac)` so all 4 NVS pages (`16 KB`) are covered using only `12 KB` of internal DRAM.
3. Implement `Esp32SettingsFlashPartition<'a>` wrapping `&'a mut FlashStorage<'static>` at `SETTINGS_PARTITION_OFFSET = 0x3F0000` (`Sector A = 0x3F0000`, `Sector B = 0x3F1000`, `4,096` B each) implementing `crate::hal::FlashPartition` so `crate::hal::NvsFlashBackend` (`FlashJournal`) loads and saves `squeezelite_config::AudioConfig` directly to the dedicated `0x3F0000` `settings` partition using internal DRAM stack buffers (`256` B).

- [ ] **Step 2: Initialize 4 MB PSRAM (`512 KB` + `128 KB` buffers), Core 1 (`APP_CPU`), Hardware `Rng`, `PowerSaveMode::None`, and `UART2` Telemetry in `xtensa_main`**

1. Immediately after `esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);`, initialize the **4 MB External Quad-SPI PSRAM**:
   ```rust
   esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
   let mut psram_stream_storage: alloc::vec::Vec<u8> = alloc::vec![0u8; 524_288];
   let mut psram_output_storage: alloc::vec::Vec<u8> = alloc::vec![0u8; 131_072];
   ```
   (Because `524,288 > 65,536` and `131,072 > 65,536`, `esp_alloc` deterministically allocates both buffers in Region 1 `External` 4 MB PSRAM, expanding `StreamPipeline` from `8 KB + 8 KB` to **`512 KB + 128 KB` = `640 KB`**!).
2. Start **Core 1 (`APP_CPU` @ 240 MHz)** via `CpuControl`:
   ```rust
   let mut cpu_ctrl = esp_hal::system::CpuControl::new(peripherals.CPU_CTRL);
   let _app_core_guard = cpu_ctrl.start_app_core(
       unsafe { &mut *core::ptr::addr_of_mut!(APP_CORE_STACK) },
       || {
           loop {
               CORE1_HEARTBEAT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
               for _ in 0..10_000 {
                   core::hint::spin_loop();
               }
           }
       },
   );
   ```
3. Seed `smoltcp` `iface_cfg.random_seed` and `local_port_counter` from hardware TRNG (`esp_hal::rng::Rng::new()`):
   ```rust
   let hw_rng = esp_hal::rng::Rng::new();
   let rng_seed = (u64::from(hw_rng.random()) << 32) | u64::from(hw_rng.random());
   iface_cfg.random_seed = rng_seed;
   let mut local_port_counter: u16 = 49152 + ((hw_rng.random() as u16) & 0x3FFF);
   ```
4. Immediately after `WifiController::new(peripherals.WIFI, wifi_ctrl_cfg)`, disable Wi-Fi modem power save and boost TX power to `60` (`15 dBm`):
   ```rust
   let _ = wifi_controller.set_power_saving(esp_radio::wifi::PowerSaveMode::None);
   let _ = wifi_controller.set_max_tx_power(60);
   ```
5. Load `AudioConfig` from `0x3F0000` via `crate::hal::NvsFlashBackend::new(Esp32SettingsFlashPartition::new(&mut flash))` at boot:
   - Apply saved `volume` (`current_vol_pct`), `analog_gain_code` (`amp.set_analog_gain(...)`), and `equalizer_preset` (`pipeline.apply_eq_preset(EqPreset::from_u8(...))`).
   - Whenever `/cmd.json` changes volume (`PlayerIrCommand` with `vol_pct`), `SetEqPreset`, or `SetAnalogGain`, apply immediately to `amp` / `pipeline` and persist the updated `AudioConfig` to `0x3F0000` via `NvsFlashBackend`!
6. Poll `amp.read_telemetry()` once every `2,000 ms` alongside `check_faults()` so `/status.json` reports live `tas_fs_hz`, `tas_bck_ratio`, `tas_pll_locked`, `tas_automuted`, `again_code`, `psram_free_bytes`, `psram_used_bytes`, `core1_active`, `core1_loops`, `eq_preset`, and `settings_seq`.
7. Emit live `VOL=`, `STATE=`, and `TIME=` frames over `cyd_uart` (`UART2`, `GPIO4`/`GPIO36`) whenever volume changes or `1,000 ms` elapses during playback.

- [ ] **Step 3: Run host `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`**

Run: `cargo test --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace && cargo clippy --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --workspace --all-targets -- -D warnings`
Expected: 100% of workspace unit tests pass (`exit code 0`), `0` Clippy warnings.

---

### Task 8: `--stage-only` Xtensa Release Build, `.stack` Budget Verification, & Binary Header Audit

**Files:**
- Create/Update: `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

- [ ] **Step 1: Build the bare-metal `xtensa-esp32-none-elf` release ELF**

Run:
```bash
source /home/console-admin/export-esp.sh && cargo +esp build --manifest-path /home/console-admin/Github/squeezelite-esp32/rust/Cargo.toml --release -Zbuild-std=core,alloc --target xtensa-esp32-none-elf -p louder-esp32-app
```
Expected: `Finished release [optimized] target(s)` (`exit code 0`).

- [ ] **Step 2: Verify `.stack >= 36,000` bytes (`>= 0x008ca0`) via `readelf -S`**

Run:
```bash
/home/console-admin/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20240906/xtensa-esp-elf/bin/xtensa-esp32-elf-readelf -S /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app
```
Expected: `.stack` section size is `>= 0x008ca0` (`>= 36,000` bytes).

- [ ] **Step 3: Package the `--stage-only` OTA `.bin` image and run `verify_staged_ota_bin.py`**

Run:
```bash
espflash save-image --chip esp32 /home/console-admin/Github/squeezelite-esp32/rust/target/xtensa-esp32-none-elf/release/louder-esp32-app /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin && python3 /home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/verify_staged_ota_bin.py
```
Expected: `[STAGE-ONLY AUDIT PASS]` (`img_magic=0xE9`, `app_desc_magic=0xABCD5432`, `project_name='Squeezelite-ESP32'`, `version='I2S-4MFlash-Rust'`, `exit code 0`).

---

## Wave 4: Independent Final Code Review & Live Hardware/RF/DSP Certification (Tasks 9–10)

### Task 9: Final Whole-Branch Code & Anti-Brick Review (`Model: "pro"`)

**Files:**
- Read-only review of `git diff` across `rust/crates/*` and `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin`

- [ ] **Step 1: Dispatch Final Code Reviewer subagent (`TypeName: "research"`, `Model: "pro"`)**

Verify:
1. All 4 Anti-Brick gates in `src/ota_safety.rs` and `src/main.rs` are intact (`otadata_prearmed_factory: true`).
2. Zero PSRAM buffers are passed to `FlashStorage::read`/`write`/`erase`, and `FlashStorage` uses `.multicore_auto_park()`.
3. `.stack` is `>= 36,000` B and `512 KB + 128 KB` PSRAM allocations cannot starve the `64 KB` internal Wi-Fi heap.

---

### Task 10: Live OTA Flash to `172.22.1.15` (`"BASEMENT"`) & End-to-End Invariant 44 Multi-System Certification

**Target:** `172.22.1.15` (`"BASEMENT"`) and LMS `10.0.0.10:9000`

- [ ] **Step 1: Transition `172.22.1.15` to Factory recovery (`0x10000`) and flash the staged `.bin` to `ota_0` (`0x160000`)**

1. `curl -sS -X POST http://172.22.1.15/recovery.json` and wait ~6s until `GET http://172.22.1.15/status.json` reports `"recovery": 1`.
2. Stream `/home/console-admin/.gemini/antigravity/brain/31deac42-e161-443d-9205-80c67d7541ab/scratch/louder-esp32-app-basement-ota.bin` to `POST http://172.22.1.15/flash.json`.
3. Poll `GET http://172.22.1.15/status.json` until `"version": "I2S-4MFlash-Rust"` and `"otadata_prearmed_factory": true`.

- [ ] **Step 2: Verify 4 MB PSRAM, Dual-Core `APP_CPU`, TAS5805M Silicon Telemetry, Interactive EQ/AGAIN Controls, and NVS Flash Persistence (Invariant 44)**

Execute live end-to-end verification against `http://172.22.1.15`:
1. **4 MB PSRAM Verification**: Confirm `/status.json` reports `"psram_used_bytes" >= 655360` (`512 KB + 128 KB`) and `"psram_free_bytes" > 3000000` (`>3 MB` free PSRAM).
2. **Dual-Core `APP_CPU` Verification**: Confirm `/status.json` reports `"core1_active": true` and `"core1_loops"` strictly increasing across polls.
3. **TAS5805M Silicon Telemetry Verification**: Confirm `/status.json` reports `"tas_fs_hz": 44100`, `"tas_bck_ratio": 64`, `"tas_pll_locked": true`, `"again_code": 8`, `"i2c_nacks": 0`.
4. **Interactive EQ & Analog Gain Control Verification (Invariant 44)**:
   - Call `GET /cmd.json?act=eq_bass` -> verify `/status.json` reports `"eq_preset": "Bass"` and `"settings_seq"` increments.
   - Call `GET /cmd.json?act=eq_loudness` -> verify `/status.json` reports `"eq_preset": "Loudness"` and `"settings_seq"` increments.
   - Call `GET /cmd.json?act=again6` -> verify `/status.json` reports `"again_code": 12` (`-6.0 dB`), `i2c_acks` increments, and `"settings_seq"` increments.
   - Call `GET /cmd.json?act=again4` and `GET /cmd.json?act=eq_flat` -> verify read-back.
5. **Continuous Audio Soak & Connectivity Verification**:
   - Verify *Ibiza Global Radio* (`preset1`) streams at `~44,100 fps` (`0` I2C NACKs) with `512 KB` PSRAM buffering, and verify AirPlay (`_raop._tcp`) and UPnP (`MediaRenderer.xml`) remain active on `10.0.0.10`.
