//! Amplifier Lifecycle Manager
//!
//! Provides a managed interface over the type-state TAS58xx driver,
//! handling transitions between PowerDown, Standby, and Play modes
//! along with volume and fault monitoring.

use embedded_hal::i2c::I2c;
use squeezelite_tas58xx::{DriverError, Play, PowerDown, Standby, Tas58xx, DEFAULT_I2C_ADDR};

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
    /// Creates a new amplifier manager with the TAS58xx in PowerDown state.
    pub fn new(i2c: I2C) -> Self {
        Self::with_address(i2c, DEFAULT_I2C_ADDR)
    }

    /// Creates a new amplifier manager with a custom I2C address.
    pub fn with_address(i2c: I2C, addr: u8) -> Self {
        Self {
            inner: Some(AmpInner::PowerDown(Tas58xx::new(i2c, addr))),
            current_volume: 0x30, // Default -24dB volume
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
        if let Some(AmpInner::PowerDown(driver)) = self.inner.take() {
            let mut standby = driver.init_into_standby()?;
            standby.configure_audio_format(word_length_bits)?;
            self.inner = Some(AmpInner::Standby(standby));
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Transitions the amplifier from Standby into active Play mode.
    pub fn start_playback(&mut self) -> Result<(), DriverError<E>> {
        if let Some(AmpInner::Standby(driver)) = self.inner.take() {
            let mut play = driver.start_playback()?;
            play.set_digital_volume(self.current_volume)?;
            self.inner = Some(AmpInner::Play(play));
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Transitions the amplifier from Play back to Standby mode (muting power stage).
    pub fn stop_playback(&mut self) -> Result<(), DriverError<E>> {
        if let Some(AmpInner::Play(driver)) = self.inner.take() {
            let standby = driver.stop_playback()?;
            self.inner = Some(AmpInner::Standby(standby));
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Sets the digital volume attenuation (0 = max, 255 = mute).
    pub fn set_volume(&mut self, volume: u8) -> Result<(), DriverError<E>> {
        self.current_volume = volume;
        if let Some(AmpInner::Play(ref mut driver)) = self.inner {
            driver.set_digital_volume(volume)?;
        }
        Ok(())
    }

    /// Reads fault status from the amplifier.
    pub fn check_faults(&mut self) -> Result<Option<u8>, DriverError<E>> {
        if let Some(AmpInner::Play(ref mut driver)) = self.inner {
            let faults = driver.check_faults()?;
            Ok(Some(faults))
        } else {
            Ok(None)
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
        let expectations = [
            // init_into_standby
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]),
            // configure_audio_format(32)
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::SAP_CTRL_1, 0x08]),
            // start_playback
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x03]),
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, 0x30]),
            // set_volume(0x20)
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DIG_VOL_CTRL, 0x20]),
            // stop_playback
            I2cTrans::write(DEFAULT_I2C_ADDR, vec![registers::DEVICE_CTRL_2, 0x01]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        assert_eq!(amp.mode(), AmpMode::PowerDown);

        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        amp.start_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);

        amp.set_volume(0x20).unwrap();

        amp.stop_playback().unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        i2c.done();
    }
}
