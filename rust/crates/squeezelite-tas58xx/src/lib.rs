//! `squeezelite-tas58xx`
//!
//! A zero-cost, type-state `embedded-hal` driver for Texas Instruments TAS5805M
//! and TAS5825M Class-D digital amplifiers, as used on the Louder-ESP32 / Louder-ESP32-Plus boards.
//!
//! Enforces hardware state transitions at compile time:
//! `PowerDown` -> `Standby` -> `Play`.

#![no_std]
#![forbid(unsafe_code)]

use core::fmt;
use core::marker::PhantomData;
use embedded_hal::i2c::I2c;

/// Standard 7-bit I2C address for TAS5825M (Louder-ESP32-Plus).
pub const DEFAULT_I2C_ADDR: u8 = 0x4C;
/// Standard 7-bit I2C address for TAS5805M (Louder-ESP32 alternate ADR pin high/float).
pub const TAS5805M_I2C_ADDR: u8 = 0x2D;
/// Standard 7-bit I2C address for TAS5805M (Sonocotta Louder-ESP32 default ADR pin low).
pub const TAS5805M_I2C_ADDR_DEFAULT: u8 = 0x2C;

/// Volume presets aligned with TI TAS5805M / TAS5825M register map (0x4C).
/// - 0x00 is +24dB overdrive (dangerous).
/// - 0x30 is 0.0dB (unity gain / maximum clean volume).
/// - 0x58 is -20.0dB (safe startup attenuation).
/// - 0x60 is -24.0dB (default moderate volume).
/// - 0xFF is hardware muted.
pub const VOL_UNITY_0DB: u8 = 0x30;
pub const VOL_DEFAULT_M20DB: u8 = 0x58;
pub const VOL_DEFAULT_MINUS_24DB: u8 = 0x60;
pub const VOL_MUTE: u8 = 0xFF;

/// Channel fault bitmasks (`CHAN_FAULT = 0x70`).
pub const CHAN_FAULT_OC_MASK: u8 = 0x03;
pub const CHAN_FAULT_DC_MASK: u8 = 0x0C;
pub const FAULT1_DC_FAULT: u8 = 0x03;

/// Global fault 1 bitmasks (`GLOBAL_FAULT1 = 0x71`, TI TAS5805M datasheet section 7.6.1.29).
pub const FAULT1_PVDD_UV: u8 = registers::FAULT1_PVDD_UV;
pub const FAULT1_PVDD_OV: u8 = registers::FAULT1_PVDD_OV;
pub const FAULT1_CLOCK_FAULT: u8 = registers::FAULT1_CLOCK_FAULT;
pub const FAULT1_BQ_WR_ERROR: u8 = registers::FAULT1_BQ_WR_ERROR;
pub const FAULT1_OTP_CRC_ERROR: u8 = registers::FAULT1_OTP_CRC_ERROR;

/// Global fault 2 and over-temperature warning bitmasks (`GLOBAL_FAULT2 = 0x72`, `OTW = 0x73`).
pub const FAULT2_OTSD: u8 = registers::FAULT2_OTSD;
pub const FAULT2_OTW: u8 = registers::FAULT2_OTW;

/// `DEVICE_CTRL_2 (0x03)` operating modes and fault clear constants.
pub const CTRL2_MODE_DEEP_SLEEP: u8 = 0x00;
pub const CTRL2_MODE_STANDBY: u8 = 0x01;
pub const CTRL2_MODE_HIZ: u8 = 0x02;
pub const CTRL2_MODE_PLAY: u8 = 0x03;
pub const CTRL2_MODE_PLAY_MUTE: u8 = 0x0B;
pub const FAULT_CLEAR_ANALOG: u8 = 0x80;

/// Register addresses from the TI TAS5805M / TAS5825M datasheets (SLASEH5B).
pub mod registers {
    pub const PAGE_SELECT: u8 = 0x00;
    pub const RESET_CTRL: u8 = 0x01;
    pub const DEVICE_CTRL_1: u8 = 0x02;
    pub const DEVICE_CTRL_2: u8 = 0x03;
    pub const SAP_CTRL_1: u8 = 0x33;   // Audio Port Format & Word Length
    pub const SAP_CTRL_2: u8 = 0x34;   // Audio Port Sample Rate
    /// `FS_MON`: Detected I2S frame sync (LRCLK) frequency monitor (`0x37`).
    pub const FS_MON: u8 = 0x37;
    /// `BCK_MON`: Detected BCK-to-FS clock ratio monitor (`0x38`).
    pub const BCK_MON: u8 = 0x38;
    /// `CLKDET_STATUS`: Clock detector & PLL lock status (`0x39`, bit 3 = PLL locked).
    pub const CLKDET_STATUS: u8 = 0x39;
    /// `DIG_VOL_LEFT` / `DIG_VOL_CTRL`: Master digital volume (`0x4C`, `0.5 dB`/step, `0x30` = `0.0 dB`).
    pub const DIG_VOL_CTRL: u8 = 0x4C; // Master Digital Volume Control
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
    pub const CHAN_FAULT: u8 = 0x70;
    pub const GLOBAL_FAULT1: u8 = 0x71;
    pub const GLOBAL_FAULT2: u8 = 0x72;
    pub const OTW: u8 = 0x73;
    pub const FAULT_CLEAR: u8 = 0x78;
    pub const BOOK_SELECT: u8 = 0x7F;

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
}

pub const REG_DEVICE_CTRL2: u8 = registers::DEVICE_CTRL_2;
pub const REG_CHAN_FAULT: u8 = registers::CHAN_FAULT;
pub const REG_GLOBAL_FAULT1: u8 = registers::GLOBAL_FAULT1;
pub const REG_GLOBAL_FAULT2: u8 = registers::GLOBAL_FAULT2;
pub const REG_AGAINST_FAULT: u8 = registers::FAULT_CLEAR;

// Compile-time state markers
pub struct PowerDown;
pub struct Standby;
pub struct Play;

/// Runtime power state representation for amplifier lifecycle tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpPowerState {
    Uninitialized,
    DeepSleep,
    Standby,
    HiZ,
    Playing,
}

/// Decoded hardware fault indicators across all 4 TI fault registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaultStatus {
    pub chan_fault: u8,
    pub global_fault1: u8,
    pub global_fault2: u8,
    pub otw: u8,
}

impl FaultStatus {
    pub fn is_clean(&self) -> bool {
        self.chan_fault == 0 && self.global_fault1 == 0 && self.global_fault2 == 0
    }

    pub fn has_overtemperature(&self) -> bool {
        (self.global_fault2 & (FAULT2_OTSD | 0x01)) != 0 || self.has_ot_warning()
    }

    pub fn has_overcurrent(&self) -> bool {
        (self.chan_fault & CHAN_FAULT_OC_MASK) != 0
    }

    pub fn has_dc_fault(&self) -> bool {
        (self.chan_fault & CHAN_FAULT_DC_MASK) != 0
    }

    pub fn has_clock_fault(&self) -> bool {
        (self.global_fault1 & FAULT1_CLOCK_FAULT) != 0
    }

    pub fn has_pvdd_uvlo(&self) -> bool {
        (self.global_fault1 & FAULT1_PVDD_UV) != 0
    }

    pub fn has_pvdd_ovlo(&self) -> bool {
        (self.global_fault1 & FAULT1_PVDD_OV) != 0
    }

    pub fn has_bq_wr_error(&self) -> bool {
        (self.global_fault1 & FAULT1_BQ_WR_ERROR) != 0
    }

    pub fn has_ot_warning(&self) -> bool {
        (self.otw & (FAULT2_OTW | 0x02)) != 0 || (self.global_fault2 & FAULT2_OTW) != 0
    }
}

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

    /// Returns `true` if `CLKDET_STATUS` (`0x39`) bit 3 indicates internal PLL lock (`1`) or
    /// when `CLKDET_STATUS` bits `2:0` (`0x07`) report zero clock faults while `FS_MON` (`0x37`)
    /// reports a valid sample rate (`> 0`) and `BCK_MON` (`0x38`) reports a valid BCK ratio (`>= 32`).
    #[must_use]
    pub const fn is_pll_locked(&self) -> bool {
        (self.clkdet_status & 0x08) != 0
            || ((self.clkdet_status & 0x07) == 0
                && self.detected_sample_rate_hz() > 0
                && self.bck_mon >= 32)
    }

    /// Returns `true` if both Left and Right channels are currently hardware-automuted (`0x69` bits `1:0` == `0b11`).
    #[must_use]
    pub const fn is_automuted(&self) -> bool {
        (self.automute_state & 0x03) == 0x03
    }
}

/// Errors returned by the TAS58xx driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError<E> {
    I2cError(E),
    FaultDetected(FaultStatus),
    InvalidParameter,
}

impl<E: fmt::Display> fmt::Display for DriverError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I2cError(e) => write!(f, "I2C communication error: {}", e),
            Self::FaultDetected(status) => write!(
                f,
                "Hardware fault detected: chan=0x{:02X}, gf1=0x{:02X}, gf2=0x{:02X}, otw=0x{:02X}",
                status.chan_fault, status.global_fault1, status.global_fault2, status.otw
            ),
            Self::InvalidParameter => write!(f, "Invalid parameter supplied"),
        }
    }
}

/// Texas Instruments TAS58xx Type-State Amplifier Driver.
pub struct Tas58xx<I2C, STATE> {
    i2c: I2C,
    addr: u8,
    _state: PhantomData<STATE>,
}

impl<I2C, STATE> Tas58xx<I2C, STATE> {
    /// Extracts the underlying I2C bus peripheral (e.g. for bus recovery).
    pub fn into_inner(self) -> I2C {
        self.i2c
    }

    /// Returns the active I2C slave address.
    pub fn address(&self) -> u8 {
        self.addr
    }
}

impl<I2C, E> Tas58xx<I2C, PowerDown>
where
    I2C: I2c<Error = E>,
{
    /// Creates a driver instance in the uninitialized `PowerDown` state.
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            _state: PhantomData,
        }
    }

    /// Resets the TAS58xx and transitions it into `Standby` mode, returning `Self` on I2C error
    /// so the caller never loses ownership of the I2C peripheral (PERIPH-604).
    pub fn try_init_into_standby(mut self) -> Result<Tas58xx<I2C, Standby>, (Self, DriverError<E>)> {
        if let Err(e) = self.i2c.write(self.addr, &[registers::BOOK_SELECT, 0x00]) {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self.i2c.write(self.addr, &[registers::PAGE_SELECT, 0x00]) {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self.i2c.write(self.addr, &[registers::RESET_CTRL, 0x11]) {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_STANDBY])
        {
            return Err((self, DriverError::I2cError(e)));
        }

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }

    /// Resets the TAS58xx and transitions it into `Standby` mode.
    /// Selects Book 0 / Page 0, asserts digital/register reset, and enters Standby.
    pub fn init_into_standby(self) -> Result<Tas58xx<I2C, Standby>, DriverError<E>> {
        self.try_init_into_standby().map_err(|(_, e)| e)
    }

    /// Convenience alias for `init_into_standby`.
    pub fn init(self) -> Result<Tas58xx<I2C, Standby>, DriverError<E>> {
        self.init_into_standby()
    }
}

impl<I2C, E> Tas58xx<I2C, Standby>
where
    I2C: I2c<Error = E>,
{
    /// Transitions into `Play` mode (convenience alias for `start_playback`).
    pub fn enable_play(self) -> Result<Tas58xx<I2C, Play>, DriverError<E>> {
        self.start_playback()
    }

    /// Selects Book 0 and Page 0, ensuring Page 0 is selected first (RUST-18).
    pub fn select_page0_book0(&mut self) -> Result<(), DriverError<E>> {
        self.select_book_page(0x00, 0x00)
    }

    /// Helper to write a single register on active page/book.
    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[reg, val])
            .map_err(DriverError::I2cError)
    }

    /// Configures Texas Instruments TAS5805M silicon maximization registers:
    /// 1. Selects Page 0 / Book 0.
    /// 2. Sets Hybrid-Pro 1SPW modulation, BTL stereo, 384 kHz FSW (`0x02 = 0x02`).
    /// 3. Sets Class-D L/R out-of-phase PWM to halve PVDD ripple (`0x53 = 0x01`).
    /// 4. Sets analog gain attenuation (`0x54 = again_code.min(0x1F)`).
    /// 5. Configures smooth 0.5 dB ramp rate (`0x4E = 0x00`).
    /// 6. Enables hardware automute (`0x50 = 0x07`).
    /// 7. Enables triangle spread spectrum EMI reduction (`0x6B = 0x01`).
    pub fn configure_silicon_maximization(&mut self, again_code: u8) -> Result<(), DriverError<E>> {
        self.select_page0_book0()?;
        self.write_reg(registers::DEVICE_CTRL_1, registers::CTRL1_1SPW_384KHZ_BTL)?;
        self.write_reg(registers::ANA_CTRL, 0x01)?;
        self.write_reg(registers::AGAIN, again_code.min(0x1F))?;
        self.write_reg(registers::DIG_VOL_CTRL2, 0x00)?;
        self.write_reg(registers::AUTO_MUTE_CTRL, 0x07)?;
        self.write_reg(registers::SS_CTRL0, 0x01)?;
        Ok(())
    }

    /// Sets the analog gain attenuation register (`0x54`, bits 4:0).
    pub fn set_analog_gain(&mut self, again_code: u8) -> Result<(), DriverError<E>> {
        self.select_page0_book0()?;
        self.write_reg(registers::AGAIN, again_code.min(0x1F))
    }

    /// Reads live silicon telemetry from registers 0x37..=0x39 and 0x69.
    pub fn read_telemetry(&mut self) -> Result<Tas5805mTelemetry, DriverError<E>> {
        self.select_page0_book0()?;
        let mut fs_mon = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::FS_MON], &mut fs_mon)
            .map_err(DriverError::I2cError)?;
        let mut bck_mon = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::BCK_MON], &mut bck_mon)
            .map_err(DriverError::I2cError)?;
        let mut clkdet = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::CLKDET_STATUS], &mut clkdet)
            .map_err(DriverError::I2cError)?;
        let mut automute = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::AUTOMUTE_STATE], &mut automute)
            .map_err(DriverError::I2cError)?;

        Ok(Tas5805mTelemetry {
            fs_mon: fs_mon[0],
            bck_mon: bck_mon[0],
            clkdet_status: clkdet[0],
            automute_state: automute[0],
        })
    }
    /// Configures the I2S audio interface bit depth (16, 20, 24, or 32 bits).
    /// Bits [1:0] WORD_LENGTH: 00 = 16-bit, 01 = 20-bit, 10 = 24-bit, 11 = 32-bit.
    pub fn configure_audio_format(&mut self, word_length_bits: u8) -> Result<(), DriverError<E>> {
        let val = match word_length_bits {
            16 => 0x00,
            20 => 0x01,
            24 => 0x02,
            32 => 0x03,
            _ => return Err(DriverError::InvalidParameter),
        };
        self.i2c
            .write(self.addr, &[registers::SAP_CTRL_1, val])
            .map_err(DriverError::I2cError)
    }

    /// Reads hardware fault status registers (0x70..=0x73) during Standby.
    pub fn check_faults(&mut self) -> Result<FaultStatus, DriverError<E>> {
        let mut status = [0u8; 4];
        self.i2c
            .write_read(self.addr, &[registers::CHAN_FAULT], &mut status)
            .map_err(DriverError::I2cError)?;
        Ok(FaultStatus {
            chan_fault: status[0],
            global_fault1: status[1],
            global_fault2: status[2],
            otw: status[3],
        })
    }

    /// Clears any latched hardware faults.
    pub fn clear_faults(&mut self) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
            .map_err(DriverError::I2cError)
    }

    /// Sets master digital volume while in Standby mode with automatic upper-bound clamping.
    /// Clamps values below `0x30` to `VOL_UNITY_0DB` (0.0 dB) to prevent hazardous overdrive
    /// prior to unmuting into Play.
    pub fn set_digital_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        let safe_vol = if volume < VOL_UNITY_0DB {
            VOL_UNITY_0DB // Clamp to 0.0dB unity gain
        } else {
            volume
        };
        self.i2c
            .write(self.addr, &[registers::DIG_VOL_CTRL, safe_vol])
            .map_err(DriverError::I2cError)
    }

    /// Re-confirms Book 0 and Page 0 selection and clears hardware faults after reset.
    pub fn reconfirm_book_page_and_clear_faults(&mut self) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::BOOK_SELECT, 0x00])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::PAGE_SELECT, 0x00])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
            .map_err(DriverError::I2cError)
    }

    /// Selects a register Book and Page, ensuring Page 0 is selected first (RUST-18).
    ///
    /// Texas Instruments TAS5805M/TAS5825M book selection (`BOOK_SELECT` register 0x7F)
    /// is only accessible from Page 0. If the device was left on a non-zero page,
    /// writing to 0x7F would modify that page's register instead of the book register.
    /// Therefore, this method strictly performs:
    /// 1. Write `PAGE_SELECT (0x00)` = `0x00`
    /// 2. Write `BOOK_SELECT (0x7F)` = `book`
    /// 3. Write `PAGE_SELECT (0x00)` = `page`
    pub fn select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::PAGE_SELECT, 0x00])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::BOOK_SELECT, book])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::PAGE_SELECT, page])
            .map_err(DriverError::I2cError)
    }

    /// Stages the amplifier into Hi-Z mode and clears latched faults (RUST-17).
    ///
    /// Writes:
    /// 1. `DEVICE_CTRL_2 (0x03)` = `CTRL2_MODE_HIZ (0x02)`
    /// 2. `FAULT_CLEAR (0x78)` = `FAULT_CLEAR_ANALOG (0x80)`
    pub fn stage_hiz_and_clear_faults(&mut self) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
            .map_err(DriverError::I2cError)
    }

    /// Transitions into `Play` mode, staging through Hi-Z and clearing faults first (RUST-17).
    ///
    /// Writes:
    /// 1. `DEVICE_CTRL_2 (0x03)` = `CTRL2_MODE_HIZ (0x02)`
    /// 2. `FAULT_CLEAR (0x78)` = `FAULT_CLEAR_ANALOG (0x80)`
    /// 3. `DEVICE_CTRL_2 (0x03)` = `CTRL2_MODE_PLAY_MUTE (0x0B)`
    /// 4. `DEVICE_CTRL_2 (0x03)` = `CTRL2_MODE_PLAY (0x03)`
    ///
    /// Returns `(Self, DriverError<E>)` on I2C error so peripheral ownership is retained (PERIPH-604).
    pub fn try_start_playback_via_hiz(
        mut self,
    ) -> Result<Tas58xx<I2C, Play>, (Self, DriverError<E>)> {
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY_MUTE])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY])
        {
            return Err((self, DriverError::I2cError(e)));
        }

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }

    /// Transitions into `Play` mode staging through Hi-Z and clearing faults first (RUST-17).
    pub fn start_playback_via_hiz(self) -> Result<Tas58xx<I2C, Play>, DriverError<E>> {
        self.try_start_playback_via_hiz().map_err(|(_, e)| e)
    }

    /// Transitions into `Play` mode, returning `Self` on I2C error (PERIPH-604).
    pub fn try_start_playback(mut self) -> Result<Tas58xx<I2C, Play>, (Self, DriverError<E>)> {
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY_MUTE])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY])
        {
            return Err((self, DriverError::I2cError(e)));
        }

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }

    /// Transitions into `Play` mode, activating the Class-D PWM power stage with
    /// soft-mute unmuting ramp to prevent acoustic pop/thump transients.
    pub fn start_playback(self) -> Result<Tas58xx<I2C, Play>, DriverError<E>> {
        self.try_start_playback().map_err(|(_, e)| e)
    }

    /// Places the amplifier into Deep Sleep (`PowerDown`), returning `Self` on I2C error.
    pub fn try_enter_deep_sleep(
        mut self,
    ) -> Result<Tas58xx<I2C, PowerDown>, (Self, DriverError<E>)> {
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_DEEP_SLEEP])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }
}

impl<I2C, E> Tas58xx<I2C, Play>
where
    I2C: I2c<Error = E>,
{
    /// Sets master digital volume with automatic upper-bound clamping to prevent hazardous overdrive.
    /// Values above `0x30` attenuate down to `0xFE` (-103 dB).
    /// `0xFF` is hardware muted.
    pub fn set_digital_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        let safe_vol = if volume < VOL_UNITY_0DB {
            VOL_UNITY_0DB // Clamp to 0.0dB unity gain
        } else {
            volume
        };
        self.i2c
            .write(self.addr, &[registers::DIG_VOL_CTRL, safe_vol])
            .map_err(DriverError::I2cError)
    }

    /// Reads hardware fault status registers (0x70..=0x73) during Play.
    pub fn check_faults(&mut self) -> Result<FaultStatus, DriverError<E>> {
        let mut status = [0u8; 4];
        self.i2c
            .write_read(self.addr, &[registers::CHAN_FAULT], &mut status)
            .map_err(DriverError::I2cError)?;
        Ok(FaultStatus {
            chan_fault: status[0],
            global_fault1: status[1],
            global_fault2: status[2],
            otw: status[3],
        })
    }

    /// Clears any latched hardware faults during Play.
    pub fn clear_faults(&mut self) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
            .map_err(DriverError::I2cError)
    }

    /// Executes the 3-step TI datasheet recovery sequence (PERIPH-603):
    /// 1. Transition `DEVICE_CTRL_2 (0x03)` to `CTRL2_MODE_HIZ (0x02)`.
    /// 2. Clear analog/digital fault latches (`FAULT_CLEAR (0x78) = 0x80`).
    /// 3. Return `DEVICE_CTRL_2 (0x03)` to `CTRL2_MODE_PLAY (0x03)`.
    pub fn recover_from_fault(&mut self) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY])
            .map_err(DriverError::I2cError)
    }

    /// Selects a register Book and Page during Play mode, ensuring Page 0 is selected first (RUST-18).
    ///
    /// Texas Instruments TAS5805M/TAS5825M book selection (`BOOK_SELECT` register 0x7F)
    /// is only accessible from Page 0. If the device was left on a non-zero page,
    /// writing to 0x7F would modify that page's register instead of the book register.
    /// Therefore, this method strictly performs:
    /// 1. Write `PAGE_SELECT (0x00)` = `0x00`
    /// 2. Write `BOOK_SELECT (0x7F)` = `book`
    /// 3. Write `PAGE_SELECT (0x00)` = `page`
    pub fn select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::PAGE_SELECT, 0x00])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::BOOK_SELECT, book])
            .map_err(DriverError::I2cError)?;
        self.i2c
            .write(self.addr, &[registers::PAGE_SELECT, page])
            .map_err(DriverError::I2cError)
    }

    /// Transitions back to `Standby` mode, returning `Self` on I2C error (PERIPH-604).
    pub fn try_stop_playback(mut self) -> Result<Tas58xx<I2C, Standby>, (Self, DriverError<E>)> {
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY_MUTE])
        {
            return Err((self, DriverError::I2cError(e)));
        }
        if let Err(e) = self
            .i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, CTRL2_MODE_STANDBY])
        {
            return Err((self, DriverError::I2cError(e)));
        }

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }

    /// Transitions back to `Standby` mode with soft-mute ramp down to prevent click/pop.
    pub fn stop_playback(self) -> Result<Tas58xx<I2C, Standby>, DriverError<E>> {
        self.try_stop_playback().map_err(|(_, e)| e)
    }

    /// Selects Book 0 and Page 0, ensuring Page 0 is selected first (RUST-18).
    pub fn select_page0_book0(&mut self) -> Result<(), DriverError<E>> {
        self.select_book_page(0x00, 0x00)
    }

    /// Helper to write a single register on active page/book.
    fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[reg, val])
            .map_err(DriverError::I2cError)
    }

    /// Configures Texas Instruments TAS5805M silicon maximization registers:
    /// 1. Selects Page 0 / Book 0.
    /// 2. Sets Hybrid-Pro 1SPW modulation, BTL stereo, 384 kHz FSW (`0x02 = 0x02`).
    /// 3. Sets Class-D L/R out-of-phase PWM to halve PVDD ripple (`0x53 = 0x01`).
    /// 4. Sets analog gain attenuation (`0x54 = again_code.min(0x1F)`).
    /// 5. Configures smooth 0.5 dB ramp rate (`0x4E = 0x00`).
    /// 6. Enables hardware automute (`0x50 = 0x07`).
    /// 7. Enables triangle spread spectrum EMI reduction (`0x6B = 0x01`).
    pub fn configure_silicon_maximization(&mut self, again_code: u8) -> Result<(), DriverError<E>> {
        self.select_page0_book0()?;
        self.write_reg(registers::DEVICE_CTRL_1, registers::CTRL1_1SPW_384KHZ_BTL)?;
        self.write_reg(registers::ANA_CTRL, 0x01)?;
        self.write_reg(registers::AGAIN, again_code.min(0x1F))?;
        self.write_reg(registers::DIG_VOL_CTRL2, 0x00)?;
        self.write_reg(registers::AUTO_MUTE_CTRL, 0x07)?;
        self.write_reg(registers::SS_CTRL0, 0x01)?;
        Ok(())
    }

    /// Sets the analog gain attenuation register (`0x54`, bits 4:0).
    pub fn set_analog_gain(&mut self, again_code: u8) -> Result<(), DriverError<E>> {
        self.select_page0_book0()?;
        self.write_reg(registers::AGAIN, again_code.min(0x1F))
    }

    /// Reads live silicon telemetry from registers 0x37..=0x39 and 0x69.
    pub fn read_telemetry(&mut self) -> Result<Tas5805mTelemetry, DriverError<E>> {
        self.select_page0_book0()?;
        let mut fs_mon = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::FS_MON], &mut fs_mon)
            .map_err(DriverError::I2cError)?;
        let mut bck_mon = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::BCK_MON], &mut bck_mon)
            .map_err(DriverError::I2cError)?;
        let mut clkdet = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::CLKDET_STATUS], &mut clkdet)
            .map_err(DriverError::I2cError)?;
        let mut automute = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::AUTOMUTE_STATE], &mut automute)
            .map_err(DriverError::I2cError)?;

        Ok(Tas5805mTelemetry {
            fs_mon: fs_mon[0],
            bck_mon: bck_mon[0],
            clkdet_status: clkdet[0],
            automute_state: automute[0],
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use super::*;
    use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};

    #[test]
    fn test_power_up_to_standby_transition() {
        let expectations = [
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let driver = Tas58xx::new(&mut i2c, DEFAULT_I2C_ADDR);
        let _standby = driver.init_into_standby().unwrap();

        i2c.done();
    }

    #[test]
    fn test_standby_to_play_and_volume_change() {
        let expectations = [
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::SAP_CTRL_1, 0x03]), // 32-bit I2S (bits [1:0] = 11)
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x0B]), // Soft mute asserted
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x03]), // Unmute with soft ramp
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, 0x30]), // Set volume (0dB unity gain)
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x0B]), // Soft mute before Standby
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]), // Stop back to Standby
        ];
        let mut i2c = I2cMock::new(&expectations);

        let driver = Tas58xx::new(&mut i2c, DEFAULT_I2C_ADDR);
        let mut standby = driver.init_into_standby().unwrap();
        standby.configure_audio_format(32).unwrap();
        let mut playing = standby.start_playback().unwrap();
        playing.set_digital_volume(0x30).unwrap();
        let _standby_again = playing.stop_playback().unwrap();

        i2c.done();
    }

    #[test]
    fn test_volume_clamping_prevents_overdrive() {
        let expectations = [
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, VOL_UNITY_0DB]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut playing = Tas58xx {
            i2c: &mut i2c,
            addr: DEFAULT_I2C_ADDR,
            _state: PhantomData::<Play>,
        };

        // Attempting to set 0x00 (+24dB boost) is clamped to 0x30 (0dB unity)
        playing.set_digital_volume(0x00).unwrap();

        i2c.done();
    }

    #[test]
    fn test_multi_byte_fault_status_polling() {
        let expectations = [
            I2cTrans::write_read(
                DEFAULT_I2C_ADDR,
                vec![registers::CHAN_FAULT],
                vec![0x00, 0x00, 0x01, 0x00], // global_fault2 bit 0 = OTSD (overtemperature)
            ),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut standby = Tas58xx {
            i2c: &mut i2c,
            addr: DEFAULT_I2C_ADDR,
            _state: PhantomData::<Standby>,
        };

        let fault = standby.check_faults().unwrap();
        assert!(!fault.is_clean());
        assert!(fault.has_overtemperature());

        i2c.done();
    }

    #[test]
    fn test_standby_set_digital_volume_with_clamping() {
        let expectations = [
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, VOL_UNITY_0DB]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, 0x48]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut standby = Tas58xx {
            i2c: &mut i2c,
            addr: DEFAULT_I2C_ADDR,
            _state: PhantomData::<Standby>,
        };

        // Clamped from overdrive 0x10 to 0x30
        standby.set_digital_volume(0x10).unwrap();
        // Valid attenuated volume 0x48 unchanged
        standby.set_digital_volume(0x48).unwrap();

        i2c.done();
    }

    #[test]
    fn test_standby_reconfirm_book_page_and_clear_faults() {
        let expectations = [
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::FAULT_CLEAR, 0x80]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut standby = Tas58xx {
            i2c: &mut i2c,
            addr: DEFAULT_I2C_ADDR,
            _state: PhantomData::<Standby>,
        };

        standby.reconfirm_book_page_and_clear_faults().unwrap();

        i2c.done();
    }

    #[test]
    fn test_fault_status_diagnostic_helpers() {
        let clean = FaultStatus::default();
        assert!(clean.is_clean());
        assert!(!clean.has_overcurrent());
        assert!(!clean.has_dc_fault());
        assert!(!clean.has_clock_fault());
        assert!(!clean.has_pvdd_uvlo());
        assert!(!clean.has_pvdd_ovlo());
        assert!(!clean.has_bq_wr_error());
        assert!(!clean.has_ot_warning());
        assert!(!clean.has_overtemperature());

        let oc1 = FaultStatus {
            chan_fault: 0x01,
            ..Default::default()
        };
        assert!(oc1.has_overcurrent());
        assert!(!oc1.has_dc_fault());

        let oc2 = FaultStatus {
            chan_fault: 0x02,
            ..Default::default()
        };
        assert!(oc2.has_overcurrent());

        let dc1 = FaultStatus {
            chan_fault: 0x04,
            ..Default::default()
        };
        assert!(!dc1.has_overcurrent());
        assert!(dc1.has_dc_fault());

        let dc2 = FaultStatus {
            chan_fault: 0x08,
            ..Default::default()
        };
        assert!(dc2.has_dc_fault());

        let clk = FaultStatus {
            global_fault1: FAULT1_CLOCK_FAULT,
            ..Default::default()
        };
        assert!(clk.has_clock_fault());
        assert!(!clk.has_pvdd_uvlo());

        let uvlo = FaultStatus {
            global_fault1: FAULT1_PVDD_UV,
            ..Default::default()
        };
        assert!(!uvlo.has_clock_fault());
        assert!(uvlo.has_pvdd_uvlo());

        let ovlo = FaultStatus {
            global_fault1: FAULT1_PVDD_OV,
            ..Default::default()
        };
        assert!(!ovlo.has_pvdd_uvlo());
        assert!(ovlo.has_pvdd_ovlo());

        let bq = FaultStatus {
            global_fault1: FAULT1_BQ_WR_ERROR,
            ..Default::default()
        };
        assert!(bq.has_bq_wr_error());

        let otw = FaultStatus {
            otw: FAULT2_OTW,
            ..Default::default()
        };
        assert!(otw.has_ot_warning());
        assert!(otw.has_overtemperature());
    }

    #[test]
    fn test_recover_from_fault_three_step_sequence() {
        let expectations = [
            I2cTrans::write(
                DEFAULT_I2C_ADDR,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ],
            ),
            I2cTrans::write(
                DEFAULT_I2C_ADDR,
                vec![registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG],
            ),
            I2cTrans::write(
                DEFAULT_I2C_ADDR,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY],
            ),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut playing = Tas58xx {
            i2c: &mut i2c,
            addr: DEFAULT_I2C_ADDR,
            _state: PhantomData::<Play>,
        };

        playing.recover_from_fault().unwrap();

        i2c.done();
    }

    #[test]
    fn test_select_book_page_writes_page_zero_before_book() {
        let expectations = [
            // Standby call: PAGE_SELECT 0x00 -> BOOK_SELECT 0x8C -> PAGE_SELECT 0x2A
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::PAGE_SELECT, 0x00],
            ),
            I2cTrans::write(TAS5805M_I2C_ADDR_DEFAULT, vec![registers::BOOK_SELECT, 0x8C]),
            I2cTrans::write(TAS5805M_I2C_ADDR_DEFAULT, vec![registers::PAGE_SELECT, 0x2A]),
            // Play call: PAGE_SELECT 0x00 -> BOOK_SELECT 0x00 -> PAGE_SELECT 0x00
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::PAGE_SELECT, 0x00],
            ),
            I2cTrans::write(TAS5805M_I2C_ADDR_DEFAULT, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(TAS5805M_I2C_ADDR_DEFAULT, vec![registers::PAGE_SELECT, 0x00]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut standby = Tas58xx {
            i2c: &mut i2c,
            addr: TAS5805M_I2C_ADDR_DEFAULT,
            _state: PhantomData::<Standby>,
        };
        standby.select_book_page(0x8C, 0x2A).unwrap();

        let mut playing = Tas58xx {
            i2c: &mut i2c,
            addr: TAS5805M_I2C_ADDR_DEFAULT,
            _state: PhantomData::<Play>,
        };
        playing.select_book_page(0x00, 0x00).unwrap();

        i2c.done();
    }

    #[test]
    fn test_stage_hiz_and_clear_faults() {
        let expectations = [
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ],
            ),
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG],
            ),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut standby = Tas58xx {
            i2c: &mut i2c,
            addr: TAS5805M_I2C_ADDR_DEFAULT,
            _state: PhantomData::<Standby>,
        };

        standby.stage_hiz_and_clear_faults().unwrap();

        i2c.done();
    }

    #[test]
    fn test_try_start_playback_via_hiz_sequence() {
        let expectations = [
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_HIZ],
            ),
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::FAULT_CLEAR, FAULT_CLEAR_ANALOG],
            ),
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY_MUTE],
            ),
            I2cTrans::write(
                TAS5805M_I2C_ADDR_DEFAULT,
                vec![registers::DEVICE_CTRL_2, CTRL2_MODE_PLAY],
            ),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let standby = Tas58xx {
            i2c: &mut i2c,
            addr: TAS5805M_I2C_ADDR_DEFAULT,
            _state: PhantomData::<Standby>,
        };

        let _playing = standby
            .try_start_playback_via_hiz()
            .map_err(|(_, e)| e)
            .unwrap();

        i2c.done();
    }

    #[derive(Debug, Clone)]
    pub struct MockI2c {
        pub die_id: u8,
        pub registers: [u8; 256],
        current_reg: u8,
    }

    impl Default for MockI2c {
        fn default() -> Self {
            let mut regs = [0u8; 256];
            regs[0x37] = 0x09; // FS_MON: 48 kHz
            regs[0x38] = 64;   // BCK_MON: 64
            regs[0x39] = 0x08; // CLKDET_STATUS: PLL locked
            regs[0x69] = 0x00; // AUTOMUTE_STATE: unmuted
            Self {
                die_id: 0,
                registers: regs,
                current_reg: 0,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MockI2cError;

    impl embedded_hal::i2c::Error for MockI2cError {
        fn kind(&self) -> embedded_hal::i2c::ErrorKind {
            embedded_hal::i2c::ErrorKind::Other
        }
    }

    impl embedded_hal::i2c::ErrorType for MockI2c {
        type Error = MockI2cError;
    }

    impl embedded_hal::i2c::I2c for MockI2c {
        fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            for byte in read.iter_mut() {
                *byte = self.registers[self.current_reg as usize];
                self.current_reg = self.current_reg.wrapping_add(1);
            }
            Ok(())
        }

        fn write(&mut self, _address: u8, write: &[u8]) -> Result<(), Self::Error> {
            if write.is_empty() {
                return Ok(());
            }
            self.current_reg = write[0];
            let mut reg = write[0] as usize;
            for &val in &write[1..] {
                if reg < 256 {
                    self.registers[reg] = val;
                    reg += 1;
                }
            }
            Ok(())
        }

        fn write_read(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error> {
            self.write(address, write)?;
            self.read(address, read)
        }

        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [embedded_hal::i2c::Operation<'_>],
        ) -> Result<(), Self::Error> {
            for op in operations {
                match op {
                    embedded_hal::i2c::Operation::Read(buf) => self.read(address, buf)?,
                    embedded_hal::i2c::Operation::Write(buf) => self.write(address, buf)?,
                }
            }
            Ok(())
        }
    }

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
    #[allow(clippy::field_reassign_with_default)]
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

    #[test]
    fn test_telemetry_pll_locked_zero_fault_and_error_bits() {
        assert!(!Tas5805mTelemetry::default().is_pll_locked());
        let valid_44k1 = Tas5805mTelemetry {
            fs_mon: 0x08,
            bck_mon: 64,
            clkdet_status: 0x00,
            automute_state: 0x00,
        };
        assert_eq!(valid_44k1.detected_sample_rate_hz(), 44_100);
        assert!(valid_44k1.is_pll_locked());

        let faulted_44k1 = Tas5805mTelemetry {
            fs_mon: 0x08,
            bck_mon: 64,
            clkdet_status: 0x01,
            automute_state: 0x00,
        };
        assert!(!faulted_44k1.is_pll_locked());
    }
}
