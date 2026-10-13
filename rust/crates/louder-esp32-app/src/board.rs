//! Louder-ESP32-Plus Hardware Pinout Definitions
//!
//! Conditionally compiled for either:
//! - `board-louder-esp32` (ESP32-WROVER-N8R8)
//! - `board-louder-esp32-s3` (ESP32-S3-WROOM-N8R8)

// When both features are enabled (e.g. during --all-features), board-louder-esp32-s3 takes precedence.

#[cfg(not(any(feature = "board-louder-esp32", feature = "board-louder-esp32-s3")))]
compile_error!("You must select either 'board-louder-esp32' or 'board-louder-esp32-s3'.");

#[cfg(all(feature = "board-louder-esp32", not(feature = "board-louder-esp32-s3")))]
pub mod pins {
    //! Louder-ESP32 (ESP32-WROVER-N8R8) Pinout
    pub const I2S_BCLK: i32 = 26;
    pub const I2S_LRCLK: i32 = 25;
    pub const I2S_DOUT: i32 = 22;
    pub const I2C_SDA: i32 = 21;
    pub const I2C_SCL: i32 = 27;
    pub const TAS58XX_PWDN: i32 = 33;
    pub const TAS58XX_FAULT: i32 = 34; // Input only
    pub const TAS58XX_I2C_ADDR: u8 = 0x2D; // Louder-ESP32 TAS5825M ADR=GND/float
}

#[cfg(feature = "board-louder-esp32-s3")]
pub mod pins {
    //! Louder-ESP32-Plus (ESP32-S3-WROOM-N8R8) Pinout
    pub const I2S_BCLK: i32 = 14;
    pub const I2S_LRCLK: i32 = 15;
    pub const I2S_DOUT: i32 = 16;
    pub const I2C_SDA: i32 = 8;
    pub const I2C_SCL: i32 = 9;
    pub const TAS58XX_PWDN: i32 = 17;
    pub const TAS58XX_FAULT: i32 = 18;
    pub const TAS58XX_I2C_ADDR: u8 = 0x4C; // Louder-ESP32-Plus TAS5805M/TAS5825M ADR=DVDD
}

/// Board identity descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardModel {
    LouderEsp32Wrover,
    LouderEsp32PlusS3,
}

/// Active board configuration descriptor for the compilation target.
pub const ACTIVE_BOARD: BoardModel = BoardModel::active();

impl BoardModel {
    pub const fn active() -> Self {
        #[cfg(all(feature = "board-louder-esp32", not(feature = "board-louder-esp32-s3")))]
        {
            Self::LouderEsp32Wrover
        }
        #[cfg(feature = "board-louder-esp32-s3")]
        {
            Self::LouderEsp32PlusS3
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::LouderEsp32Wrover => "Louder-ESP32 (WROVER-N8R8)",
            Self::LouderEsp32PlusS3 => "Louder-ESP32-Plus (S3-WROOM-N8R8)",
        }
    }

    pub const fn default_amp_address(self) -> u8 {
        match self {
            Self::LouderEsp32Wrover => 0x2D,
            Self::LouderEsp32PlusS3 => 0x4C,
        }
    }

    pub const fn tas5805m_i2c_addr(self) -> u8 {
        self.default_amp_address()
    }
}
