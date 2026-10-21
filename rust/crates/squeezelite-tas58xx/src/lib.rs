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

/// Global fault 1 bitmasks (`GLOBAL_FAULT1 = 0x71`, TI TAS5805M datasheet section 7.6.1.24).
pub const FAULT1_CLOCK_FAULT: u8 = 0x04;
pub const FAULT1_PVDD_UV: u8 = 0x20;
pub const FAULT1_PVDD_OV: u8 = 0x40;
pub const FAULT1_BQ_WR_ERROR: u8 = 0x80;

/// Global fault 2 and over-temperature warning bitmasks (`GLOBAL_FAULT2 = 0x72`, `OTW = 0x73`).
pub const FAULT2_OTSD: u8 = 0x01;
pub const FAULT2_OTW: u8 = 0x04;

/// `DEVICE_CTRL_2 (0x03)` operating modes and fault clear constants.
pub const CTRL2_MODE_DEEP_SLEEP: u8 = 0x00;
pub const CTRL2_MODE_STANDBY: u8 = 0x01;
pub const CTRL2_MODE_HIZ: u8 = 0x02;
pub const CTRL2_MODE_PLAY: u8 = 0x03;
pub const CTRL2_MODE_PLAY_MUTE: u8 = 0x0B;
pub const FAULT_CLEAR_ANALOG: u8 = 0x80;

/// Register addresses from the TI TAS5805M / TAS5825M datasheets.
pub mod registers {
    pub const PAGE_SELECT: u8 = 0x00;
    pub const RESET_CTRL: u8 = 0x01;
    pub const DEVICE_CTRL_1: u8 = 0x02;
    pub const DEVICE_CTRL_2: u8 = 0x03;
    pub const SAP_CTRL_1: u8 = 0x33;   // Audio Port Format & Word Length
    pub const SAP_CTRL_2: u8 = 0x34;   // Audio Port Sample Rate
    pub const DIG_VOL_CTRL: u8 = 0x4C; // Master Digital Volume Control
    pub const DIG_VOL_LEFT: u8 = 0x4D;
    pub const DIG_VOL_RIGHT: u8 = 0x4E;
    pub const CHAN_FAULT: u8 = 0x70;
    pub const GLOBAL_FAULT1: u8 = 0x71;
    pub const GLOBAL_FAULT2: u8 = 0x72;
    pub const OTW: u8 = 0x73;
    pub const FAULT_CLEAR: u8 = 0x78;
    pub const BOOK_SELECT: u8 = 0x7F;
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
        (self.global_fault2 & FAULT2_OTSD) != 0 || self.has_ot_warning()
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
        (self.otw & (FAULT2_OTW | 0x01)) != 0 || (self.global_fault2 & FAULT2_OTW) != 0
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
}

impl<I2C, E> Tas58xx<I2C, Standby>
where
    I2C: I2c<Error = E>,
{
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
}
