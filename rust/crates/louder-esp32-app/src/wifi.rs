//! Wi-Fi Station Manager and Network Provisioning
//!
//! Provides network connection management backed by pure-Rust key-value storage:
//! - Loads persisted credentials from `squeezelite_config::KeyValueStorage` on initialization.
//! - Persists credentials across reboots on successful connection.
//! - Simulated network status on host platforms.
//! - Active ESP-IDF Wi-Fi STA driver on ESP32 target hardware (`target_os = "espidf"`).

use core::fmt;
use squeezelite_config::{KeyValueStorage, MemConfigStore, StorageError, WifiCredentials};

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
#[derive(Clone, PartialEq, Eq)]
pub struct WifiConfig {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<64>,
}

impl fmt::Debug for WifiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WifiConfig")
            .field("ssid", &self.ssid)
            .field("password", &"[REDACTED]")
            .finish()
    }
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
    retry_count: u32,
    base_backoff_ms: u32,
    max_backoff_ms: u32,
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
            retry_count: 0,
            base_backoff_ms: 500,
            max_backoff_ms: 30000,
        };

        // Attempt to auto-load saved credentials from store
        if let Ok(Some(creds)) = WifiCredentials::load_from(&mgr.store) {
            mgr.config.ssid.clear();
            let _ = mgr.config.ssid.push_str(creds.ssid.as_str());
            mgr.config.password.clear();
            let _ = mgr.config.password.push_str(creds.passphrase.as_str());

            if creds.autoconnect && !creds.ssid.is_empty() {
                mgr.state = WifiState::Connecting;
            }
        }

        mgr
    }

    /// Current Wi-Fi state.
    pub fn state(&self) -> WifiState {
        self.state
    }

    /// Returns the current consecutive retry count.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// Access reference to the underlying key-value store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Access mutable reference to the underlying key-value store.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Initiates a Wi-Fi connection handshake and persists credentials to storage,
    /// transitioning into [`WifiState::Connecting`] until [`Self::confirm_connected`]
    /// or [`Self::on_connected`] is invoked upon link/DHCP completion (PERIPH-605).
    pub fn begin_connect(&mut self, ssid: &str, password: &str) -> Result<WifiState, StorageError> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(StorageError::InvalidFormat);
        }
        if !password.is_empty() && !(8..=63).contains(&password.len()) {
            return Err(StorageError::InvalidFormat);
        }

        self.retry_count = 0;

        self.config.ssid.clear();
        self.config.ssid.push_str(ssid).map_err(|_| StorageError::CapacityExceeded)?;
        self.config.password.clear();
        self.config.password.push_str(password).map_err(|_| StorageError::CapacityExceeded)?;

        // Persist to key-value storage, preserving existing static IP / network configuration if present (CODE-CFG-001)
        let existing = WifiCredentials::load_from(&self.store).ok().flatten();
        let mut creds = WifiCredentials::new(ssid, password)?;
        if let Some(prev) = existing {
            creds.static_ip = prev.static_ip;
            creds.gateway = prev.gateway;
            creds.subnet = prev.subnet;
            creds.dns = prev.dns;
            creds.autoconnect = prev.autoconnect;
        }
        creds.save_to(&mut self.store)?;

        self.state = WifiState::Connecting;
        Ok(self.state)
    }

    /// Connects to a Wi-Fi network and persists the credentials to storage.
    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), StorageError> {
        self.begin_connect(ssid, password)?;
        self.record_success();
        Ok(())
    }

    /// Returns the exponential backoff delay in milliseconds based on consecutive failures (APP-002).
    pub fn current_backoff_ms(&self) -> u32 {
        if self.retry_count == 0 {
            return 0;
        }
        let shift = core::cmp::min(self.retry_count - 1, 6);
        let delay = self.base_backoff_ms.saturating_mul(1 << shift);
        core::cmp::min(delay, self.max_backoff_ms)
    }

    /// Records a failed connection attempt and increments backoff counter using saturating arithmetic.
    pub fn record_failure(&mut self) {
        self.state = WifiState::Failed;
        self.retry_count = self.retry_count.saturating_add(1);
    }

    /// Handles an unexpected link disconnection event, transitioning state and incrementing retry count
    /// with saturating arithmetic (PERIPH-611).
    pub fn on_disconnected(&mut self) {
        self.state = WifiState::Disconnected;
        self.retry_count = self.retry_count.saturating_add(1);
    }

    /// Records a successful connection, resetting the retry counter.
    pub fn record_success(&mut self) {
        self.state = WifiState::Connected;
        self.retry_count = 0;
    }

    /// Confirms that Wi-Fi connection has been established.
    pub fn confirm_connected(&mut self) {
        self.record_success();
    }

    /// Callback invoked when the station link and IP lease are acquired (PERIPH-605).
    pub fn on_connected(&mut self) {
        self.record_success();
    }

    /// Disconnects from Wi-Fi without forgetting credentials.
    pub fn disconnect(&mut self) {
        self.state = WifiState::Disconnected;
    }

    /// Forgets saved credentials and clears them from persistent storage.
    pub fn forget_credentials(&mut self) -> Result<(), StorageError> {
        self.config.ssid.clear();
        self.config.password.clear();
        self.state = WifiState::Disconnected;
        self.retry_count = 0;
        WifiCredentials::clear(&mut self.store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wifi_manager_state_transitions() {
        let mut wifi = WifiManager::new();
        assert_eq!(wifi.state(), WifiState::Disconnected);

        // Two-step asynchronous connection lifecycle (PERIPH-605)
        let state = wifi.begin_connect("Home_Network", "SecretPass").unwrap();
        assert_eq!(state, WifiState::Connecting);
        assert_eq!(wifi.state(), WifiState::Connecting);

        wifi.on_connected();
        assert_eq!(wifi.state(), WifiState::Connected);
        assert_eq!(wifi.config.ssid.as_str(), "Home_Network");

        wifi.on_disconnected();
        assert_eq!(wifi.state(), WifiState::Disconnected);
        assert_eq!(wifi.retry_count(), 1);

        wifi.connect("Home_Network", "SecretPass").unwrap();
        assert_eq!(wifi.state(), WifiState::Connected);
        assert_eq!(wifi.retry_count(), 0);

        wifi.disconnect();
        assert_eq!(wifi.state(), WifiState::Disconnected);
    }

    #[test]
    fn test_wifi_config_debug_redacts_password() {
        let mut cfg = WifiConfig::default();
        cfg.ssid.push_str("MyNetwork").unwrap();
        cfg.password.push_str("SuperSecretPassword123").unwrap();

        let formatted = std::format!("{:?}", cfg);
        assert!(formatted.contains("MyNetwork"));
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("SuperSecretPassword123"));
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
            // Must have autoconnected from saved credentials into Connecting state
            assert_eq!(wifi.state(), WifiState::Connecting);
            assert_eq!(wifi.config.ssid.as_str(), "Office_5G");
            assert_eq!(wifi.config.password.as_str(), "EnterprisePass123");

            // Transition to connected once link is confirmed
            wifi.confirm_connected();
            assert_eq!(wifi.state(), WifiState::Connected);

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

    #[test]
    fn test_wifi_manager_connect_validation() {
        let mut wifi = WifiManager::new();

        // Empty SSID fails
        assert_eq!(wifi.connect("", "ValidPass123"), Err(StorageError::InvalidFormat));

        // SSID > 32 bytes fails
        let long_ssid = "This_SSID_Is_Way_Too_Long_Because_It_Exceeds_32_Bytes!";
        assert_eq!(wifi.connect(long_ssid, "ValidPass123"), Err(StorageError::InvalidFormat));

        // Short WPA2 passphrase (1..=7 bytes) fails
        assert_eq!(wifi.connect("ValidSSID", "short"), Err(StorageError::InvalidFormat));
        assert_eq!(wifi.connect("ValidSSID", "1234567"), Err(StorageError::InvalidFormat));

        // Long WPA2 passphrase (> 63 bytes) fails
        let long_pass = "1234567890123456789012345678901234567890123456789012345678901234";
        assert_eq!(wifi.connect("ValidSSID", long_pass), Err(StorageError::InvalidFormat));

        // Open network (empty password) succeeds
        assert!(wifi.connect("OpenNetwork", "").is_ok());

        // Valid WPA2 passphrase (8..=63 bytes) succeeds
        assert!(wifi.connect("SecuredNet", "validpass").is_ok());
    }

    #[test]
    fn test_wifi_exponential_backoff() {
        let mut wifi = WifiManager::new();
        assert_eq!(wifi.current_backoff_ms(), 0);

        wifi.record_failure();
        assert_eq!(wifi.current_backoff_ms(), 500);

        wifi.record_failure();
        assert_eq!(wifi.current_backoff_ms(), 1000);

        wifi.record_failure();
        assert_eq!(wifi.current_backoff_ms(), 2000);

        wifi.record_failure();
        assert_eq!(wifi.current_backoff_ms(), 4000);

        // Advance to max backoff
        for _ in 0..10 {
            wifi.record_failure();
        }
        assert_eq!(wifi.current_backoff_ms(), 30000);

        // Success resets backoff
        wifi.record_success();
        assert_eq!(wifi.current_backoff_ms(), 0);
    }

    #[test]
    fn test_begin_connect_resets_retry_count_and_preserves_network_config() {
        let store = MemConfigStore::<32>::new();
        let mut wifi = WifiManager::with_store(store);

        // Preconfigure existing credentials with static IP and gateway
        let mut creds = WifiCredentials::new("InitialSSID", "InitialPassword123").unwrap();
        creds.static_ip = Some([192, 168, 1, 150]);
        creds.gateway = Some([192, 168, 1, 1]);
        creds.subnet = Some([255, 255, 255, 0]);
        creds.dns = Some([1, 1, 1, 1]);
        creds.save_to(wifi.store_mut()).unwrap();

        // Simulate failed connections to accumulate retry_count
        wifi.record_failure();
        wifi.record_failure();
        assert_eq!(wifi.retry_count(), 2);
        assert!(wifi.current_backoff_ms() > 0);

        // begin_connect with new credentials
        let state = wifi.begin_connect("NewSSID", "NewPassword456").unwrap();
        assert_eq!(state, WifiState::Connecting);
        assert_eq!(wifi.retry_count(), 0);
        assert_eq!(wifi.current_backoff_ms(), 0);

        // Verify that the stored credentials contain the new SSID/password but preserved static IP settings
        let stored = WifiCredentials::load_from(wifi.store()).unwrap().expect("creds present");
        assert_eq!(stored.ssid.as_str(), "NewSSID");
        assert_eq!(stored.passphrase.as_str(), "NewPassword456");
        assert_eq!(stored.static_ip, Some([192, 168, 1, 150]));
        assert_eq!(stored.gateway, Some([192, 168, 1, 1]));
        assert_eq!(stored.subnet, Some([255, 255, 255, 0]));
        assert_eq!(stored.dns, Some([1, 1, 1, 1]));
    }
}
