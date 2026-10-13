//! `squeezelite-tas58xx`
//!
//! A zero-cost, type-state `embedded-hal` driver for Texas Instruments TAS5805M
//! and TAS5825M Class-D digital amplifiers, as used on the Louder-ESP32-Plus board.
//!
//! Enforces hardware state transitions at compile time:
//! `PowerDown` -> `Standby` -> `Play`.

#![no_std]

use core::fmt;
use core::marker::PhantomData;
use embedded_hal::i2c::I2c;

/// Standard 7-bit I2C address for TAS5825M / TAS5805M (ADR pin pulled to GND).
pub const DEFAULT_I2C_ADDR: u8 = 0x4C;

/// Register addresses from the TI TAS5825M datasheet.
pub mod registers {
    pub const RESET_CTRL: u8 = 0x01;
    pub const DEVICE_CTRL_1: u8 = 0x02;
    pub const DEVICE_CTRL_2: u8 = 0x03;
    pub const SAP_CTRL_1: u8 = 0x33;   // Audio Port Format & Word Length
    pub const SAP_CTRL_2: u8 = 0x34;   // Audio Port Sample Rate
    pub const DIG_VOL_CTRL: u8 = 0x4C; // Master Digital Volume Control
    pub const DIG_VOL_LEFT: u8 = 0x4D;
    pub const DIG_VOL_RIGHT: u8 = 0x4E;
    pub const FAULT_CLEAR: u8 = 0x78;
    pub const FAULT_STATUS: u8 = 0x70;
}

// Compile-time state markers
pub struct PowerDown;
pub struct Standby;
pub struct Play;

/// Errors returned by the TAS58xx driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError<E> {
    I2cError(E),
    FaultDetected(u8),
    InvalidParameter,
}

impl<E: fmt::Display> fmt::Display for DriverError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I2cError(e) => write!(f, "I2C communication error: {}", e),
            Self::FaultDetected(status) => write!(f, "Hardware fault detected: 0x{:02X}", status),
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

    /// Resets the TAS58xx and transitions it into `Standby` mode.
    pub fn init_into_standby(mut self) -> Result<Tas58xx<I2C, Standby>, DriverError<E>> {
        // Full software reset (write 0x11 to RESET_CTRL)
        self.i2c
            .write(self.addr, &[registers::RESET_CTRL, 0x11])
            .map_err(DriverError::I2cError)?;

        // Place into Standby mode (write 0x01 to DEVICE_CTRL_2)
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, 0x01])
            .map_err(DriverError::I2cError)?;

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
        })
    }
}

impl<I2C, E> Tas58xx<I2C, Standby>
where
    I2C: I2c<Error = E>,
{
    /// Configures the I2S audio interface bit depth (16, 24, or 32 bits).
    pub fn configure_audio_format(&mut self, word_length_bits: u8) -> Result<(), DriverError<E>> {
        let val = match word_length_bits {
            16 => 0x00,
            24 => 0x04,
            32 => 0x08,
            _ => return Err(DriverError::InvalidParameter),
        };
        self.i2c
            .write(self.addr, &[registers::SAP_CTRL_1, val])
            .map_err(DriverError::I2cError)
    }

    /// Transitions into `Play` mode, activating the Class-D PWM power stage.
    pub fn start_playback(mut self) -> Result<Tas58xx<I2C, Play>, DriverError<E>> {
        // Clear any power-up fault flags
        self.i2c
            .write(self.addr, &[registers::FAULT_CLEAR, 0x80])
            .map_err(DriverError::I2cError)?;

        // Set Play mode (write 0x03 to DEVICE_CTRL_2)
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, 0x03])
            .map_err(DriverError::I2cError)?;

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
    /// Sets master digital volume.
    /// - `0x00`: 0 dB (Maximum volume)
    /// - `0xFE`: -103 dB
    /// - `0xFF`: Muted
    pub fn set_digital_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::DIG_VOL_CTRL, volume])
            .map_err(DriverError::I2cError)
    }

    /// Reads hardware fault register.
    pub fn check_faults(&mut self) -> Result<u8, DriverError<E>> {
        let mut status = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[registers::FAULT_STATUS], &mut status)
            .map_err(DriverError::I2cError)?;
        Ok(status[0])
    }

    /// Transitions back to `Standby` mode, deactivating the PWM output cleanly.
    pub fn stop_playback(mut self) -> Result<Tas58xx<I2C, Standby>, DriverError<E>> {
        self.i2c
            .write(self.addr, &[registers::DEVICE_CTRL_2, 0x01])
            .map_err(DriverError::I2cError)?;

        Ok(Tas58xx {
            i2c: self.i2c,
            addr: self.addr,
            _state: PhantomData,
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
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::SAP_CTRL_1, 0x08]), // 32-bit I2S
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x03]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, 0x30]), // Set volume
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
}
