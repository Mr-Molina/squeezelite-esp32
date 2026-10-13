//! Amplifier Lifecycle Manager
//!
//! Provides a managed interface over the type-state TAS58xx driver,
//! handling transitions between PowerDown, Standby, and Play modes
//! along with volume and fault monitoring.

use embedded_hal::i2c::I2c;
use squeezelite_tas58xx::{
    DriverError, FaultStatus, Play, PowerDown, Standby, Tas58xx,
    VOL_DEFAULT_MINUS_24DB, VOL_UNITY_0DB,
};

/// Current operational state of the amplifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpMode {
    PowerDown,
    Standby,
    Play,
}

/// Dynamic manager for the TAS58xx amplifier.
pub struct AmpManager<I2C> {
    inner: Option<AmpInner<I2C>>,
    current_volume: u8,
}

enum AmpInner<I2C> {
    PowerDown(Tas58xx<I2C, PowerDown>),
    Standby(Tas58xx<I2C, Standby>),
    Play(Tas58xx<I2C, Play>),
}

impl<I2C, E> AmpManager<I2C>
where
    I2C: I2c<Error = E>,
{
    /// Creates a new amplifier manager with the TAS58xx in PowerDown state for the active board.
    pub fn new(i2c: I2C) -> Self {
        Self::for_active_board(i2c)
    }

    /// Creates a new amplifier manager using the board's default active address (AMP-007).
    pub fn for_active_board(i2c: I2C) -> Self {
        Self::with_address(i2c, crate::board::ACTIVE_BOARD.tas5805m_i2c_addr())
    }

    /// Creates a new amplifier manager with a custom I2C address.
    pub fn with_address(i2c: I2C, addr: u8) -> Self {
        Self {
            inner: Some(AmpInner::PowerDown(Tas58xx::new(i2c, addr))),
            current_volume: VOL_DEFAULT_MINUS_24DB, // Default -24dB volume (0x60)
        }
    }

    /// Returns the current configured digital volume attenuation.
    pub fn current_volume(&self) -> u8 {
        self.current_volume
    }

    /// Consumes the AmpManager and extracts the underlying I2C peripheral.
    pub fn into_inner(self) -> Option<I2C> {
        match self.inner {
            Some(AmpInner::PowerDown(d)) => Some(d.into_inner()),
            Some(AmpInner::Standby(d)) => Some(d.into_inner()),
            Some(AmpInner::Play(d)) => Some(d.into_inner()),
            None => None,
        }
    }

    /// Returns the current amplifier operational mode.
    pub fn mode(&self) -> AmpMode {
        match self.inner.as_ref() {
            Some(AmpInner::PowerDown(_)) => AmpMode::PowerDown,
            Some(AmpInner::Standby(_)) => AmpMode::Standby,
            Some(AmpInner::Play(_)) => AmpMode::Play,
            None => AmpMode::PowerDown,
        }
    }

    /// Initializes the amplifier from PowerDown into Standby mode.
    pub fn initialize(&mut self, word_length_bits: u8) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::PowerDown(driver)) => {
                let mut standby = driver.init_into_standby()?;
                standby.reconfirm_book_page_and_clear_faults()?;
                standby.configure_audio_format(word_length_bits)?;
                self.inner = Some(AmpInner::Standby(standby));
                Ok(())
            }
            Some(AmpInner::Standby(mut standby)) => {
                standby.reconfirm_book_page_and_clear_faults()?;
                standby.configure_audio_format(word_length_bits)?;
                self.inner = Some(AmpInner::Standby(standby));
                Ok(())
            }
            Some(AmpInner::Play(play)) => {
                self.inner = Some(AmpInner::Play(play));
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Transitions the amplifier from Standby into active Play mode.
    pub fn start_playback(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::Standby(mut driver)) => {
                driver.set_digital_volume(self.current_volume)?;
                let play = driver.start_playback()?;
                self.inner = Some(AmpInner::Play(play));
                Ok(())
            }
            Some(AmpInner::Play(play)) => {
                self.inner = Some(AmpInner::Play(play));
                Ok(())
            }
            other => {
                self.inner = other;
                Ok(())
            }
        }
    }

    /// Transitions the amplifier from Play back to Standby mode (muting power stage).
    pub fn stop_playback(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::Play(driver)) => {
                let standby = driver.stop_playback()?;
                self.inner = Some(AmpInner::Standby(standby));
                Ok(())
            }
            Some(AmpInner::Standby(standby)) => {
                self.inner = Some(AmpInner::Standby(standby));
                Ok(())
            }
            other => {
                self.inner = other;
                Ok(())
            }
        }
    }

    /// Sets the digital volume attenuation (clamped to prevent hazardous overdrive < 0dB).
    pub fn set_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        let safe_vol = volume.max(VOL_UNITY_0DB);
        match self.inner {
            Some(AmpInner::Play(ref mut driver)) => {
                driver.set_digital_volume(safe_vol)?;
                self.current_volume = safe_vol;
                Ok(())
            }
            Some(AmpInner::Standby(ref mut driver)) => {
                driver.set_digital_volume(safe_vol)?;
                self.current_volume = safe_vol;
                Ok(())
            }
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Clears latched hardware faults in Standby or Play mode.
    pub fn clear_faults(&mut self) -> Result<(), DriverError<E>> {
        match self.inner {
            Some(AmpInner::Play(ref mut driver)) => driver.clear_faults(),
            Some(AmpInner::Standby(ref mut driver)) => driver.clear_faults(),
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Reads fault status from the amplifier in Standby or Play mode.
    pub fn check_faults(&mut self) -> Result<Option<FaultStatus>, DriverError<E>> {
        match self.inner {
            Some(AmpInner::Play(ref mut driver)) => Ok(Some(driver.check_faults()?)),
            Some(AmpInner::Standby(ref mut driver)) => Ok(Some(driver.check_faults()?)),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};
    use squeezelite_tas58xx::registers;

    #[test]
    fn test_amp_manager_full_lifecycle() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // start_playback (program volume in Standby BEFORE unmuting to Play)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x60]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
            // set_volume(0x40)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x40]),
            // stop_playback
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        assert_eq!(amp.mode(), AmpMode::PowerDown);
        assert_eq!(amp.current_volume(), VOL_DEFAULT_MINUS_24DB);

        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        amp.start_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);

        amp.set_volume(0x40).unwrap();
        assert_eq!(amp.current_volume(), 0x40);

        amp.stop_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        i2c.done();
    }

    #[test]
    fn test_amp_manager_set_volume_in_standby() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // set_volume(0x50) in Standby
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x50]),
            // start_playback (program volume in Standby BEFORE unmuting to Play)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x50]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        amp.set_volume(0x50).unwrap();
        assert_eq!(amp.current_volume(), 0x50);

        amp.start_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);

        i2c.done();
    }

    #[test]
    fn test_amp_manager_volume_clamping() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // set_volume(0x10) clamped to VOL_UNITY_0DB (0x30)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, VOL_UNITY_0DB]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        amp.initialize(32).unwrap();

        amp.set_volume(0x10).unwrap();
        assert_eq!(amp.current_volume(), VOL_UNITY_0DB);

        i2c.done();
    }

    #[test]
    fn test_amp_manager_clear_faults() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // clear_faults in Standby
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // start_playback (program volume in Standby BEFORE unmuting to Play)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x60]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
            // clear_faults in Play
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        // In PowerDown, set_volume and clear_faults return Err
        assert_eq!(amp.set_volume(0x40), Err(DriverError::InvalidParameter));
        assert_eq!(amp.clear_faults(), Err(DriverError::InvalidParameter));

        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);
        amp.clear_faults().unwrap();

        amp.start_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);
        amp.clear_faults().unwrap();

        i2c.done();
    }
}
