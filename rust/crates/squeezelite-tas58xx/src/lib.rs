//! `squeezelite-tas58xx`
//!
//! A zero-cost, type-state `embedded-hal` driver for Texas Instruments TAS5805M
//! and TAS5825M Class-D digital amplifiers, as used on
//! the Louder-ESP32 / Louder-ESP32-Plus boards.
//!
//! Enforces hardware state transitions at compile time:
//! `PowerDown` -> `Standby` -> `Play`.

#![no_std]
#![forbid(unsafe_code)]

/// Register addresses from the TI TAS5805M / TAS5825M datasheets (SLASEH5B).
pub mod registers {
    pub const PAGE_SELECT: u8 = 0x00;
    pub const RESET_CTRL: u8 = 0x01;
    pub const DEVICE_CTRL_1: u8 = 0x02;
    pub const DEVICE_CTRL_2: u8 = 0x03;
    pub const SAP_CTRL_1: u8 = 0x33; // Audio Port Format & Word Length
    pub const SAP_CTRL_2: u8 = 0x34; // Audio Port Sample Rate
    /// `FS_MON`: Detected I2S frame sync (LRCLK) frequency monitor (`0x37`).
    pub const FS_MON: u8 = 0x37;
    /// `BCK_MON`: Detected BCK-to-FS clock ratio monitor (`0x38`).
    pub const BCK_MON: u8 = 0x38;
    /// `CLKDET_STATUS`: Clock detector & PLL lock status
    /// (`0x39`, bit 3 = PLL locked).
    pub const CLKDET_STATUS: u8 = 0x39;
    /// `DIG_VOL_LEFT` / `DIG_VOL_CTRL`: Master digital volume
    /// (`0x4C`, `0.5 dB`/step, `0x30` = `0.0 dB`).
    pub const DIG_VOL_CTRL: u8 = 0x4C; // Master Digital Volume Control
    pub const DIG_VOL_LEFT: u8 = 0x4C;
    /// `DIG_VOL_RIGHT`: Digital volume ramp / fine control (`0x4D`).
    pub const DIG_VOL_RIGHT: u8 = 0x4D;
    /// `DIG_VOL_CTRL2`: Digital volume ramp rate configuration (`0x4E`).
    pub const DIG_VOL_CTRL2: u8 = 0x4E;
    /// `AUTO_MUTE_CTRL`: Hardware automute enable
    /// (`0x50`, `0x07` = L/R independent + grouped automute).
    pub const AUTO_MUTE_CTRL: u8 = 0x50;
    /// `AUTO_MUTE_TIME`: Hardware automute threshold time (`0x51`).
    pub const AUTO_MUTE_TIME: u8 = 0x51;
    /// `ANA_CTRL`: Class-D analog modulation phase control
    /// (`0x53`, `0x01` = L/R out-of-phase PWM to halve PVDD ripple).
    pub const ANA_CTRL: u8 = 0x53;
    /// `AGAIN`: Analog gain attenuation (`0x54`, bits `4:0`,
    /// `0.5 dB`/step from `0x00` = `0.0 dB` / `29.5 V` to `0x1F` = `-15.5 dB`).
    pub const AGAIN: u8 = 0x54;
    /// `DSP_MISC`: DSP miscellaneous control (`0x66`).
    pub const DSP_MISC: u8 = 0x66;
    /// `AUTOMUTE_STATE`: Live L/R hardware automute status flags
    /// (`0x69`, bit 0 = Left muted, bit 1 = Right muted).
    pub const AUTOMUTE_STATE: u8 = 0x69;
    /// `SS_CTRL0`: Spread-spectrum PWM EMI reduction
    /// (`0x6B`, `0x01` = Triangle spread spectrum enabled).
    pub const SS_CTRL0: u8 = 0x6B;
    pub const CHAN_FAULT: u8 = 0x70;
    pub const GLOBAL_FAULT1: u8 = 0x71;
    pub const GLOBAL_FAULT2: u8 = 0x72;
    pub const OTW: u8 = 0x73;
    pub const FAULT_CLEAR: u8 = 0x78;
    pub const BOOK_SELECT: u8 = 0x7F;

    /// `DEVICE_CTRL_1` value: 1SPW (Hybrid-Pro) modulation,
    /// BTL stereo, 384 kHz FSW (`0x02`).
    pub const CTRL1_1SPW_384KHZ_BTL: u8 = 0x02;

    // GLOBAL_FAULT1 (0x71) bitmasks per TI TAS5805M datasheet
    // (SLASEH5B Section 7.6.1.29):
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
    /// `GLOBAL_FAULT2` (0x72) / `OT_WARNING` (0x73) Overtemperature warning
    /// (`OTW`, 135 C).
    pub const FAULT2_OTW: u8 = 0x08;
}

mod driver;

pub use driver::{
    AmpPowerState, DriverError, FaultStatus, Play, PowerDown, Standby, Tas5805mTelemetry, Tas58xx,
    Tas58xxTelemetry, TasFaultStatus, TryInitError, CHAN_FAULT_DC_MASK, CHAN_FAULT_OC_MASK,
    CTRL2_MODE_DEEP_SLEEP, CTRL2_MODE_HIZ, CTRL2_MODE_PLAY, CTRL2_MODE_PLAY_MUTE,
    CTRL2_MODE_STANDBY, DEFAULT_I2C_ADDR, FAULT1_BQ_WR_ERROR, FAULT1_CLOCK_FAULT, FAULT1_DC_FAULT,
    FAULT1_OTP_CRC_ERROR, FAULT1_PVDD_OV, FAULT1_PVDD_UV, FAULT2_OTSD, FAULT2_OTW, FAULT_CLEAR_ALL,
    FAULT_CLEAR_ANALOG, REG_AGAINST_FAULT, REG_CHAN_FAULT, REG_DEVICE_CTRL2, REG_GLOBAL_FAULT1,
    REG_GLOBAL_FAULT2, TAS5805M_I2C_ADDR, TAS5805M_I2C_ADDR_DEFAULT, VOL_DEFAULT_M20DB,
    VOL_DEFAULT_MINUS_24DB, VOL_MUTE, VOL_UNITY_0DB,
};
