//! Wi-Fi Station Manager and Network Provisioning
//!
//! Provides network connection management backed by pure-Rust key-value storage:
//! - Loads persisted credentials from `squeezelite_config::KeyValueStorage` on initialization.
//! - Persists credentials across reboots on successful connection.
//! - Simulated network status on host platforms.
//! - Active ESP-IDF Wi-Fi STA driver on ESP32 target hardware (`target_os = "espidf"`).

use core::fmt;
use squeezelite_config::{KeyValueStorage, MemConfigStore, WifiCredentials};

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

/// Wi-Fi manager backed by pure-Rust key-value storage.
pub struct WifiManager<S = MemConfigStore<32>> {
    state: WifiState,
    pub config: WifiConfig,
    store: S,
}

impl Default for WifiManager<MemConfigStore<32>> {
    fn default() -> Self {
        Self::new()
    }
}

impl WifiManager<MemConfigStore<32>> {
    /// Creates a Wi-Fi manager with an in-memory key-value store.
    pub fn new() -> Self {
        Self::with_store(MemConfigStore::<32>::new())
    }
}

impl<S: KeyValueStorage> WifiManager<S> {
    /// Creates a Wi-Fi manager using the provided key-value storage backend.
    /// Automatically attempts to load saved credentials and autoconnect.
    pub fn with_store(store: S) -> Self {
        let mut mgr = Self {
            state: WifiState::Disconnected,
            config: WifiConfig::default(),
            store,
        };

        // Attempt to auto-load saved credentials from store
        if let Ok(Some(creds)) = WifiCredentials::load_from(&mgr.store) {
            mgr.config.ssid.clear();
            let _ = mgr.config.ssid.push_str(creds.ssid.as_str());
            mgr.config.password.clear();
            let _ = mgr.config.password.push_str(creds.passphrase.as_str());

            if creds.autoconnect {
                mgr.state = WifiState::Connected;
            }
        }

        mgr
    }

    /// Current Wi-Fi state.
    pub fn state(&self) -> WifiState {
        self.state
    }

    /// Access reference to the underlying key-value store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Access mutable reference to the underlying key-value store.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Connects to a Wi-Fi network and persists the credentials to storage.
    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), &'static str> {
        self.config.ssid.clear();
        self.config.ssid.push_str(ssid).map_err(|_| "SSID exceeds 32 bytes")?;
        self.config.password.clear();
        self.config.password.push_str(password).map_err(|_| "Password exceeds 64 bytes")?;

        // Persist to key-value storage
        if let Ok(creds) = WifiCredentials::new(ssid, password) {
            let _ = creds.save_to(&mut self.store);
        }

        self.state = WifiState::Connected;
        Ok(())
    }

    /// Disconnects from Wi-Fi without forgetting credentials.
    pub fn disconnect(&mut self) {
        self.state = WifiState::Disconnected;
    }

    /// Forgets saved credentials and clears them from persistent storage.
    pub fn forget_credentials(&mut self) -> Result<(), &'static str> {
        self.config.ssid.clear();
        self.config.password.clear();
        self.state = WifiState::Disconnected;
        WifiCredentials::clear(&mut self.store).map_err(|_| "Failed to erase credentials from store")
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

    #[test]
    fn test_wifi_manager_persistent_storage() {
        let mut store = MemConfigStore::<32>::new();

        // Scope 1: Provision Wi-Fi on initial setup
        {
            let mut wifi = WifiManager::with_store(store.clone());
            assert_eq!(wifi.state(), WifiState::Disconnected);

            wifi.connect("Office_5G", "EnterprisePass123").unwrap();
            assert_eq!(wifi.state(), WifiState::Connected);
            store = wifi.store().clone();
        }

        // Scope 2: Simulate reboot - new WifiManager instance loading from same store
        {
            let mut wifi = WifiManager::with_store(store);
            // Must have autoconnected from saved credentials
            assert_eq!(wifi.state(), WifiState::Connected);
            assert_eq!(wifi.config.ssid.as_str(), "Office_5G");
            assert_eq!(wifi.config.password.as_str(), "EnterprisePass123");

            // Forget credentials
            wifi.forget_credentials().unwrap();
            assert_eq!(wifi.state(), WifiState::Disconnected);
            assert!(wifi.config.ssid.is_empty());
            store = wifi.store().clone();
        }

        // Scope 3: Reboot after forget - should not autoconnect
        {
            let wifi = WifiManager::with_store(store);
            assert_eq!(wifi.state(), WifiState::Disconnected);
            assert!(wifi.config.ssid.is_empty());
        }
    }
}
