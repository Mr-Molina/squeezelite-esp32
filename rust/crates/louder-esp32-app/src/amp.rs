//! Amplifier Lifecycle Manager
//!
//! Provides a managed interface over the type-state TAS58xx driver,
//! handling transitions between PowerDown, Standby, and Play modes
//! along with volume and fault monitoring.

use embedded_hal::i2c::I2c;
pub use squeezelite_tas58xx::Tas5805mTelemetry;
use squeezelite_tas58xx::{
    DriverError, FaultStatus, Play, PowerDown, Standby, Tas58xx,
    VOL_DEFAULT_M20DB, VOL_MUTE, VOL_UNITY_0DB,
};

/// Maps LMS logarithmic mixer gain (`0..=65536`) to perceptual hardware volume percentage (`0..=100%`).
///
/// LMS sends gain on a 50 dB curve where 51% slider is `gain = 1842` (-31 dB) and 100% is `gain = 65536` (0 dB).
/// This fixed-point Q8 integer log2 curve maps `gain = 0 -> 0%`, `gain = 1842 -> 55%` (-22.5 dB on TAS5805M `0x4C`),
/// and `gain = 65536 -> 100%` (0 dB `0x30` on TAS5805M `0x4C`).
pub fn lms_gain_to_hardware_volume_percent(gain: u32) -> u8 {
    if gain == 0 {
        return 0;
    }
    if gain >= 65536 {
        return 100;
    }
    let shift = 31 - gain.leading_zeros();
    let frac = ((gain - (1 << shift)) << 8) >> shift;
    let log2_q8 = (shift << 8) | frac;
    let diff = 4096_i32 - log2_q8 as i32;
    let reduction = ((diff * 2202) + 32768) >> 16;
    let p = 100_i32 - reduction;
    p.clamp(1, 100) as u8
}

/// Maps LMS logarithmic mixer gain (`0..=65536`) to exact TAS5805M `0x4C` hardware volume register code.
///
/// TAS5805M `DIG_VOL_CTRL` (register `0x4C`) is calibrated at 0.5 dB per step:
/// - `0x30` = 0.0 dB (100% volume)
/// - `0x5D` = -22.5 dB (55% perceptual volume)
/// - `0xFF` = Mute (0% volume)
pub fn lms_gain_to_tas5805m_vol_reg(gain: u32) -> u8 {
    let pct = lms_gain_to_hardware_volume_percent(gain);
    if pct == 0 {
        0xFF // VOL_MUTE
    } else {
        0x30 + (100 - pct.min(100)) // 0x30 (0.0 dB) at 100%, +1 (+0.5 dB attenuation) per 1% below 100%
    }
}

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
    /// Initial analog gain attenuation code (`0x00..=0x1F`, 0.5 dB/step, default `0x08` = `-4.0 dB`).
    pub initial_again_code: u8,
}

impl Default for AmpConfig {
    /// Returns a safe default amplifier configuration:
    /// - `i2c_addr`: Active board TAS58xx I2C address.
    /// - `initial_volume`: [`VOL_DEFAULT_M20DB`] (`0x58` = `-20.0 dB` safe startup attenuation, PERIPH-612).
    /// - `word_length_bits`: `32`-bit I2S frames.
    /// - `initial_again_code`: `0x08` (`-4.0 dB` analog gain attenuation).
    fn default() -> Self {
        Self {
            i2c_addr: crate::board::ACTIVE_BOARD.tas5805m_i2c_addr(),
            initial_volume: VOL_DEFAULT_M20DB,
            word_length_bits: 32,
            initial_again_code: 0x08,
        }
    }
}

/// Maps a logical `0..=100%` volume percentage to a TAS58xx hardware digital volume
/// attenuation register value (`0x30..=0xFF`) (PERIPH-601, CODE-TAS-001):
/// - `0%` returns [`VOL_MUTE`] (`0xFF`, hardware mute).
/// - `1..=100%` (clamped at `100%`) maps linearly with 0.5 dB/step matching `lms_gain_to_tas5805m_vol_reg`:
///   `100% => 0x30` (0.0 dB), `50% => 0x62` (-25.0 dB), `1% => 0x93` (-49.5 dB).
pub const fn percent_to_attenuation(percent: u8) -> u8 {
    if percent == 0 {
        VOL_MUTE
    } else {
        let clamped = if percent > 100 { 100 } else { percent };
        0x30 + (100 - clamped)
    }
}

/// Dynamic manager for the TAS58xx amplifier.
pub struct AmpManager<I2C> {
    inner: Option<AmpInner<I2C>>,
    current_volume: u8,
    current_again_code: u8,
    config: AmpConfig,
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
        let cfg = AmpConfig {
            i2c_addr: addr,
            ..AmpConfig::default()
        };
        Self {
            inner: Some(AmpInner::PowerDown(Tas58xx::new(i2c, addr))),
            current_volume: cfg.initial_volume,
            current_again_code: cfg.initial_again_code,
            config: cfg,
        }
    }

    /// Creates a new amplifier manager from an explicit [`AmpConfig`].
    pub fn with_config(i2c: I2C, config: AmpConfig) -> Self {
        Self {
            inner: Some(AmpInner::PowerDown(Tas58xx::new(i2c, config.i2c_addr))),
            current_volume: config.initial_volume.max(VOL_UNITY_0DB),
            current_again_code: config.initial_again_code,
            config,
        }
    }

    /// Returns the configured amplifier parameters.
    pub fn config(&self) -> &AmpConfig {
        &self.config
    }

    /// Returns the currently applied analog gain attenuation code (`0x00..=0x1F`).
    pub fn analog_gain_code(&self) -> u8 {
        self.current_again_code
    }

    /// Sets the analog gain attenuation register (`0x54`, bits 4:0, clamped to 0x1F).
    pub fn set_analog_gain(&mut self, again_code: u8) -> Result<(), DriverError<E>> {
        let clamped = again_code.min(0x1F);
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => {
                driver.set_analog_gain(clamped)?;
                self.current_again_code = clamped;
                Ok(())
            }
            Some(AmpInner::Standby(driver)) => {
                driver.set_analog_gain(clamped)?;
                self.current_again_code = clamped;
                Ok(())
            }
            Some(AmpInner::PowerDown(_)) | None => Err(DriverError::InvalidParameter),
        }
    }

    /// Convenience alias for [`set_analog_gain`].
    pub fn set_analog_gain_code(&mut self, code: u8) -> Result<(), DriverError<E>> {
        self.set_analog_gain(code)
    }

    /// Reads live silicon telemetry from registers 0x37..=0x39 and 0x69 in Standby or Play mode.
    pub fn read_telemetry(&mut self) -> Result<Option<Tas5805mTelemetry>, DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => Ok(Some(driver.read_telemetry()?)),
            Some(AmpInner::Standby(driver)) => Ok(Some(driver.read_telemetry()?)),
            _ => Ok(None),
        }
    }

    /// Convenience alias for [`read_telemetry`].
    pub fn poll_telemetry(&mut self) -> Result<Option<Tas5805mTelemetry>, DriverError<E>> {
        self.read_telemetry()
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
                    if let Err(e) = standby.configure_silicon_maximization(self.current_again_code) {
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
                if let Err(e) = standby.configure_silicon_maximization(self.current_again_code) {
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

    /// Transitions the amplifier from Standby into active Play mode via Hi-Z staging (`HW-PREP-05`).
    ///
    /// Preserves `self.inner` if a transient I2C error occurs (PERIPH-604).
    pub fn start_playback_via_hiz(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.take() {
            Some(AmpInner::Standby(mut driver)) => {
                if let Err(e) = driver.set_digital_volume(self.current_volume) {
                    self.inner = Some(AmpInner::Standby(driver));
                    return Err(e);
                }
                match driver.try_start_playback_via_hiz() {
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

    /// Stages the amplifier into Hi-Z mode and clears faults (`RUST-17`).
    pub fn stage_hiz(&mut self) -> Result<(), DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Standby(driver)) => driver.stage_hiz_and_clear_faults(),
            _ => Err(DriverError::InvalidParameter),
        }
    }

    /// Selects book and page registers on the TAS58xx amplifier in Standby or Play mode (`RUST-18`).
    pub fn select_book_page(&mut self, book: u8, page: u8) -> Result<(), DriverError<E>> {
        match self.inner.as_mut() {
            Some(AmpInner::Play(driver)) => driver.select_book_page(book, page),
            Some(AmpInner::Standby(driver)) => driver.select_book_page(book, page),
            _ => Err(DriverError::InvalidParameter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::TargetI2cBus;
    use embedded_hal::i2c::ErrorKind;
    use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTrans};
    use squeezelite_tas58xx::registers;

    #[test]
    fn test_amp_manager_full_lifecycle() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
            // start_playback (program volume in Standby BEFORE unmuting to Play)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
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
        assert_eq!(amp.current_volume(), VOL_DEFAULT_M20DB);

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
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
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
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
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
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
            // clear_faults in Standby
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // start_playback (program volume in Standby BEFORE unmuting to Play)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
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
        assert_eq!(percent_to_attenuation(1), 0x93);
        assert_eq!(percent_to_attenuation(50), 0x62);
        assert_eq!(percent_to_attenuation(100), VOL_UNITY_0DB);
        assert_eq!(percent_to_attenuation(150), VOL_UNITY_0DB);

        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
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
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00])
                .with_error(ErrorKind::Bus),
            // Second initialize attempt succeeds because inner was preserved
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
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

    #[test]
    fn test_amp_manager_select_book_page_and_stage_hiz() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
            // stage_hiz
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x02]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // select_book_page(0x01, 0x02)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x01]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x02]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        amp.initialize(32).unwrap();
        amp.stage_hiz().unwrap();
        amp.select_book_page(0x01, 0x02).unwrap();

        assert_eq!(crate::board::BoardModel::LouderEsp32Wrover.cyd_uart_pins(), (4, 36));

        i2c.done();
    }

    #[test]
    fn test_amp_manager_start_playback_via_hiz() {
        let addr = crate::board::pins::TAS58XX_I2C_ADDR;
        let expectations = [
            // init_into_standby
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::RESET_CTRL, 0x11]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x01]),
            // reconfirm_book_page_and_clear_faults
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            // configure_audio_format(32)
            I2cTrans::write(addr, vec![registers::SAP_CTRL_1, 0x03]),
            // configure_silicon_maximization(0x08)
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::BOOK_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::PAGE_SELECT, 0x00]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_1, 0x02]),
            I2cTrans::write(addr, vec![registers::ANA_CTRL, 0x01]),
            I2cTrans::write(addr, vec![registers::AGAIN, 0x08]),
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL2, 0x00]),
            I2cTrans::write(addr, vec![registers::AUTO_MUTE_CTRL, 0x07]),
            I2cTrans::write(addr, vec![registers::SS_CTRL0, 0x01]),
            // start_playback_via_hiz:
            // 1) set_digital_volume(0x58)
            I2cTrans::write(addr, vec![registers::DIG_VOL_CTRL, 0x58]),
            // 2) try_start_playback_via_hiz sequence:
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x02]),
            I2cTrans::write(addr, vec![registers::FAULT_CLEAR, 0x80]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x0B]),
            I2cTrans::write(addr, vec![registers::DEVICE_CTRL_2, 0x03]),
        ];
        let mut i2c = I2cMock::new(&expectations);

        let mut amp = AmpManager::new(&mut i2c);
        amp.initialize(32).unwrap();
        assert_eq!(amp.mode(), AmpMode::Standby);

        amp.start_playback_via_hiz().unwrap();
        assert_eq!(amp.mode(), AmpMode::Play);

        i2c.done();
    }

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
}
