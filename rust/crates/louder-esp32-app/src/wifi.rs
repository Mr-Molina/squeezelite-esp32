//! Wi-Fi Station Manager and Network Provisioning
//!
//! Provides network connection management:
//! - Simulated network status on host platforms.
//! - Active ESP-IDF Wi-Fi STA driver on ESP32 target hardware (`target_os = "espidf"`).

use core::fmt;

/// Wi-Fi connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

impl fmt::Display for WifiState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Connecting => write!(f, "Connecting"),
            Self::Connected => write!(f, "Connected"),
            Self::Failed => write!(f, "Connection Failed"),
        }
    }
}

/// Wi-Fi configuration credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiConfig {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<64>,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            ssid: heapless::String::new(),
            password: heapless::String::new(),
        }
    }
}

/// Simulated Wi-Fi manager for host-side verification.
#[cfg(not(target_os = "espidf"))]
pub struct WifiManager {
    state: WifiState,
    config: WifiConfig,
}

#[cfg(not(target_os = "espidf"))]
impl Default for WifiManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "espidf"))]
impl WifiManager {
    pub fn new() -> Self {
        Self {
            state: WifiState::Disconnected,
            config: WifiConfig::default(),
        }
    }

    pub fn state(&self) -> WifiState {
        self.state
    }

    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), &'static str> {
        self.config.ssid.clear();
        let _ = self.config.ssid.push_str(ssid);
        self.config.password.clear();
        let _ = self.config.password.push_str(password);
        self.state = WifiState::Connected;
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.state = WifiState::Disconnected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wifi_manager_state_transitions() {
        let mut wifi = WifiManager::new();
        assert_eq!(wifi.state(), WifiState::Disconnected);

        wifi.connect("Home_Network", "SecretPass").unwrap();
        assert_eq!(wifi.state(), WifiState::Connected);
        assert_eq!(wifi.config.ssid.as_str(), "Home_Network");

        wifi.disconnect();
        assert_eq!(wifi.state(), WifiState::Disconnected);
    }
}
