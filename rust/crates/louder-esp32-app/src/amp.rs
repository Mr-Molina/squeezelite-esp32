//! Amplifier Lifecycle Manager
//!
//! Provides a managed interface over the type-state TAS58xx driver,
//! handling transitions between PowerDown, Standby, and Play modes
//! along with volume and fault monitoring.

use embedded_hal::i2c::I2c;
use squeezelite_tas58xx::{
    DriverError, FaultStatus, Play, PowerDown, Standby, Tas58xx,
    VOL_DEFAULT_M20DB, VOL_DEFAULT_MINUS_24DB, VOL_MUTE, VOL_UNITY_0DB,
};

/// Current operational state of the amplifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpMode {
    PowerDown,
    Standby,
    Play,
}

/// Configuration parameters for the TAS58xx amplifier manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmpConfig {
    /// Target 7-bit I2C slave address.
    pub i2c_addr: u8,
    /// Initial digital volume register value applied before entering Play mode.
    ///
    /// Defaults to [`VOL_DEFAULT_M20DB`] (`0x58`, `-20.0 dB` safe startup attenuation)
    /// to prevent unexpected high-SPL output bursts on boot (PERIPH-612).
    pub initial_volume: u8,
    /// I2S audio word length in bits (16, 20, 24, or 32).
    pub word_length_bits: u8,
}

impl Default for AmpConfig {
    /// Returns a safe default amplifier configuration:
    /// - `i2c_addr`: Active board TAS58xx I2C address.
    /// - `initial_volume`: [`VOL_DEFAULT_M20DB`] (`0x58` = `-20.0 dB` safe startup attenuation, PERIPH-612).
    /// - `word_length_bits`: `32`-bit I2S frames.
    fn default() -> Self {
        Self {
            i2c_addr: crate::board::ACTIVE_BOARD.tas5805m_i2c_addr(),
            initial_volume: VOL_DEFAULT_M20DB,
            word_length_bits: 32,
        }
    }
}

/// Maps a logical `0..=100%` volume percentage to a TAS58xx hardware digital volume
/// attenuation register value (`0x30..=0xFF`) (PERIPH-601):
/// - `0%` returns [`VOL_MUTE`] (`0xFF`, hardware mute).
/// - `1..=100%` (clamped at `100%`) maps linearly from `0xCF` (`-79.5 dB`) to [`VOL_UNITY_0DB`] (`0x30`, `0.0 dB` unity gain).
pub const fn percent_to_attenuation(percent: u8) -> u8 {
    if percent == 0 {
        VOL_MUTE
    } else {
        let clamped = if percent > 100 { 100 } else { percent };
        0xCF - (((clamped as u16 - 1) * (0xCF - VOL_UNITY_0DB) as u16) / 99) as u8
    }
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

    /// Creates a new amplifier manager from an explicit [`AmpConfig`].
    pub fn with_config(i2c: I2C, config: AmpConfig) -> Self {
        Self {
            inner: Some(AmpInner::PowerDown(Tas58xx::new(i2c, config.i2c_addr))),
            current_volume: config.initial_volume.max(VOL_UNITY_0DB),
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
    /// Preserves `self.inner` if a transient I2C error occurs (PERIPH-604).
    pub fn initialize(&mut self, word_length_bits: u8) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::PowerDown(driver)) => match driver.try_init_into_standby() {
                Ok(mut standby) => {
                    if let Err(e) = standby.reconfirm_book_page_and_clear_faults() {
                        self.inner = Some(AmpInner::Standby(standby));
                        return Err(e);
                    }
                    if let Err(e) = standby.configure_audio_format(word_length_bits) {
                        self.inner = Some(AmpInner::Standby(standby));
                        return Err(e);
                    }
                    self.inner = Some(AmpInner::Standby(standby));
                    Ok(())
                }
                Err((power_down, e)) => {
                    self.inner = Some(AmpInner::PowerDown(power_down));
                    Err(e)
                }
            },
            Some(AmpInner::Standby(mut standby)) => {
                if let Err(e) = standby.reconfirm_book_page_and_clear_faults() {
                    self.inner = Some(AmpInner::Standby(standby));
                    return Err(e);
                }
                if let Err(e) = standby.configure_audio_format(word_length_bits) {
                    self.inner = Some(AmpInner::Standby(standby));
                    return Err(e);
                }
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
    /// Preserves `self.inner` if a transient I2C error occurs (PERIPH-604).
    pub fn start_playback(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::Standby(mut driver)) => {
                if let Err(e) = driver.set_digital_volume(self.current_volume) {
                    self.inner = Some(AmpInner::Standby(driver));
                    return Err(e);
                }
                match driver.try_start_playback() {
                    Ok(play) => {
                        self.inner = Some(AmpInner::Play(play));
                        Ok(())
                    }
                    Err((standby, e)) => {
                        self.inner = Some(AmpInner::Standby(standby));
                        Err(e)
                    }
                }
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
    /// Preserves `self.inner` if a transient I2C error occurs (PERIPH-604).
    pub fn stop_playback(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::Play(driver)) => match driver.try_stop_playback() {
                Ok(standby) => {
                    self.inner = Some(AmpInner::Standby(standby));
                    Ok(())
                }
                Err((play, e)) => {
                    self.inner = Some(AmpInner::Play(play));
                    Err(e)
                }
            },
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

    /// Sets the raw digital volume attenuation (`0x30..=0xFF`, clamped to prevent hazardous overdrive < 0dB).
    /// Borrows `self.inner` via `as_mut()` so a transient I2C error never drops the driver (PERIPH-604).
    pub fn set_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        let safe_vol = volume.max(VOL_UNITY_0DB);
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => {
                driver.set_digital_volume(safe_vol)?;
                self.current_volume = safe_vol;
                Ok(())
            }
            Some(AmpInner::Standby(driver)) => {
                driver.set_digital_volume(safe_vol)?;
                self.current_volume = safe_vol;
                Ok(())
            }
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Sets the amplifier volume from a logical percentage (`0..=100%`) by converting it to
    /// a TAS58xx register attenuation code before writing to `DIG_VOL_CTRL` (PERIPH-601).
    pub fn set_volume_percent(&mut self, percent: u8) -> Result<(), DriverError<E>> {
        let attenuation = percent_to_attenuation(percent);
        self.set_volume(attenuation)
    }

    /// Clears latched hardware faults in Standby or Play mode.
    pub fn clear_faults(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => driver.clear_faults(),
            Some(AmpInner::Standby(driver)) => driver.clear_faults(),
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Executes the 3-step TI datasheet hardware fault recovery sequence in Play mode (PERIPH-603).
    pub fn recover_from_fault(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => driver.recover_from_fault(),
            Some(AmpInner::Standby(driver)) => driver.clear_faults(),
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Reads fault status from the amplifier in Standby or Play mode.
    pub fn check_faults(&mut self) -> Result<Option<FaultStatus>, DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => Ok(Some(driver.check_faults()?)),
            Some(AmpInner::Standby(driver)) => Ok(Some(driver.check_faults()?)),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::i2c::ErrorKind;
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

    #[test]
    fn test_percent_to_attenuation_and_set_volume_percent() {
        assert_eq!(percent_to_attenuation(0), VOL_MUTE);
        assert_eq!(percent_to_attenuation(1), 0xCF);
        assert_eq!(percent_to_attenuation(100), VOL_UNITY_0DB);
        assert_eq!(percent_to_attenuation(150), VOL_UNITY_0DB);

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
            // set_volume_percent(0) -> 0xFF
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, VOL_MUTE]),
            // set_volume_percent(100) -> 0x30
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, VOL_UNITY_0DB]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::with_config(&mut i2c, AmpConfig::default());
        assert_eq!(amp.current_volume(), VOL_DEFAULT_M20DB);
        amp.initialize(32).unwrap();

        amp.set_volume_percent(0).unwrap();
        assert_eq!(amp.current_volume(), VOL_MUTE);

        amp.set_volume_percent(100).unwrap();
        assert_eq!(amp.current_volume(), VOL_UNITY_0DB);

        i2c.done();
    }

    #[test]
    fn test_transient_i2c_error_does_not_drop_driver() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // First initialize attempt fails on first I2C write
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00])
                .with_error(ErrorKind::Bus),
            // Second initialize attempt succeeds because inner was preserved
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        assert!(amp.initialize(32).is_err());
        assert_eq!(amp.mode(), AmpMode::PowerDown);

        // Retry after transient error succeeds because self.inner was not dropped
        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);
        assert!(amp.into_inner().is_some());

        i2c.done();
    }
}
