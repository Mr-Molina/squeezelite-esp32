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
    pub const TAS5805M_PRIMARY_I2C_ADDR: u8 = 0x2C;
    pub const CYD_UART_TX: i32 = 4;
    pub const CYD_UART_RX: i32 = 36;
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
    pub const TAS5805M_PRIMARY_I2C_ADDR: u8 = 0x2C;
    pub const CYD_UART_TX: i32 = 43;
    pub const CYD_UART_RX: i32 = 44;
}

/// Default Lyrion Music Server (LMS) IPv4 address from target build context (`10.0.0.10`).
pub const DEFAULT_LMS_SERVER_IP: [u8; 4] = [10, 0, 0, 10];
/// Default Lyrion Music Server (LMS) SlimProto TCP/UDP port (`3483`).
pub const DEFAULT_LMS_SERVER_PORT: u16 = 3483;
/// Default deployed player node name (`"BASEMENT"` on `172.22.1.15`).
pub const DEFAULT_PLAYER_NAME: &str = "BASEMENT";
/// Default deployed Wi-Fi SSID (`"iot"` on `172.22.1.15`).
pub const DEFAULT_WIFI_SSID: &str = "iot";
/// Hardware MAC address of the `172.22.1.15` `"BASEMENT"` ESP32 unit (`c8:2e:18:be:b8:34`).
pub const BASEMENT_PLAYER_MAC: [u8; 6] = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34];

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

    pub const fn cyd_uart_pins(self) -> (u8, u8) {
        match self {
            Self::LouderEsp32Wrover => (4, 36),
            Self::LouderEsp32PlusS3 => (43, 44),
        }
    }

    pub const fn i2s_pins(self) -> I2sPinConfig {
        match self {
            Self::LouderEsp32Wrover => I2sPinConfig {
                bclk_pin: 26,
                lrclk_pin: 25,
                dout_pin: 22,
                mclk_pin: None,
            },
            Self::LouderEsp32PlusS3 => I2sPinConfig {
                bclk_pin: 14,
                lrclk_pin: 15,
                dout_pin: 16,
                mclk_pin: None,
            },
        }
    }
}

/// Hardware I2S pin assignment configuration (`HALNET-412`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I2sPinConfig {
    pub bclk_pin: u8,
    pub lrclk_pin: u8,
    pub dout_pin: u8,
    /// Optional master clock (MCLK) pin (`None` on Louder-ESP32 3-wire BCLK PLL mode).
    pub mclk_pin: Option<u8>,
}

impl I2sPinConfig {
    /// Returns the I2S pin configuration for the active board target.
    pub const fn for_active_board() -> Self {
        Self {
            bclk_pin: pins::I2S_BCLK as u8,
            lrclk_pin: pins::I2S_LRCLK as u8,
            dout_pin: pins::I2S_DOUT as u8,
            mclk_pin: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_cyd_uart_pins_and_addresses() {
        assert_eq!(BoardModel::LouderEsp32Wrover.cyd_uart_pins(), (4, 36));
        assert_eq!(BoardModel::LouderEsp32PlusS3.cyd_uart_pins(), (43, 44));
        assert_eq!(pins::TAS5805M_PRIMARY_I2C_ADDR, 0x2C);
    }

    #[test]
    fn test_basement_deployment_constants() {
        assert_eq!(DEFAULT_LMS_SERVER_IP, [10, 0, 0, 10]);
        assert_eq!(DEFAULT_LMS_SERVER_PORT, 3483);
        assert_eq!(DEFAULT_PLAYER_NAME, "BASEMENT");
        assert_eq!(DEFAULT_WIFI_SSID, "iot");
        assert_eq!(BASEMENT_PLAYER_MAC, [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34]);
    }
}

