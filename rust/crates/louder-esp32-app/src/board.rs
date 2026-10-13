//! Louder-ESP32-Plus Hardware Pinout Definitions
//!
//! Conditionally compiled for either:
//! - `board-louder-esp32` (ESP32-WROVER-N8R8)
//! - `board-louder-esp32-s3` (ESP32-S3-WROOM-N8R8)

#[cfg(all(feature = "board-louder-esp32", feature = "board-louder-esp32-s3"))]
compile_error!("Features 'board-louder-esp32' and 'board-louder-esp32-s3' are mutually exclusive.");

#[cfg(not(any(feature = "board-louder-esp32", feature = "board-louder-esp32-s3")))]
compile_error!("You must select either 'board-louder-esp32' or 'board-louder-esp32-s3'.");

#[cfg(feature = "board-louder-esp32")]
pub mod pins {
    //! Louder-ESP32 (ESP32-WROVER-N8R8) Pinout
    pub const I2S_BCLK: i32 = 26;
    pub const I2S_LRCLK: i32 = 25;
    pub const I2S_DOUT: i32 = 22;
    pub const I2C_SDA: i32 = 21;
    pub const I2C_SCL: i32 = 27;
    pub const TAS58XX_PWDN: i32 = 33;
    pub const TAS58XX_FAULT: i32 = 34; // Input only
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
}

/// Board identity descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardModel {
    LouderEsp32Wrover,
    LouderEsp32PlusS3,
}

impl BoardModel {
    pub const fn active() -> Self {
        #[cfg(feature = "board-louder-esp32")]
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
}
