//! `squeezelite-config`
//!
//! A zero-allocation, `no_std` pure-Rust key-value configuration storage and
//! Flash journal engine for Squeezelite-ESP32 / Louder-ESP32-Plus.
//!
//! Provides:
//! - Strongly typed `KeyValueStorage` trait with type-safe accessors.
//! - In-memory bounded `MemConfigStore` backed by `heapless`.
//! - Dual-CRC32 validated `FlashJournal` serialization for power-loss resilient Flash storage.
//! - Domain configuration models: `WifiCredentials` and `AudioConfig`.

#![no_std]

use core::fmt;
use heapless::{String, Vec};

/// Errors encountered in key-value configuration operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    /// Requested key does not exist in the store.
    KeyNotFound,
    /// Destination buffer is too small to receive the value.
    BufferTooSmall,
    /// Key-value storage capacity exceeded maximum allowable entries.
    CapacityExceeded,
    /// Stored data failed CRC integrity checks or sector magic mismatch.
    CorruptedData,
    /// Data layout or field lengths violate protocol invariants.
    InvalidFormat,
    /// String payload is not valid UTF-8.
    InvalidUtf8,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyNotFound => write!(f, "Configuration key not found"),
            Self::BufferTooSmall => write!(f, "Destination buffer too small"),
            Self::CapacityExceeded => write!(f, "Storage entry capacity exceeded"),
            Self::CorruptedData => write!(f, "CRC or data corruption detected"),
            Self::InvalidFormat => write!(f, "Invalid storage format or layout"),
            Self::InvalidUtf8 => write!(f, "Stored string is invalid UTF-8"),
        }
    }
}

/// Updates a running IEEE 802.3 CRC32 checksum accumulator (polynomial 0xEDB88320).
pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

/// Standard IEEE 802.3 CRC32 checksum calculator (polynomial 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(0xFFFF_FFFF, data)
}

/// Generic trait for key-value storage backends.
pub trait KeyValueStorage {
    /// Retrieves raw bytes associated with `key`.
    /// Returns the number of bytes written to `out`.
    fn get(&self, key: &str, out: &mut [u8]) -> Result<usize, StorageError>;

    /// Stores raw bytes under `key`.
    fn set(&mut self, key: &str, val: &[u8]) -> Result<(), StorageError>;

    /// Removes `key` and its associated value.
    fn remove(&mut self, key: &str) -> Result<(), StorageError>;

    /// Checks if `key` exists in the store.
    fn contains(&self, key: &str) -> bool;

    /// Retrieves a UTF-8 string value for `key`.
    fn get_str<const N: usize>(&self, key: &str) -> Result<Option<String<N>>, StorageError> {
        let mut buf = [0u8; N];
        match self.get(key, &mut buf) {
            Ok(len) => {
                let s = core::str::from_utf8(&buf[..len]).map_err(|_| StorageError::InvalidUtf8)?;
                let mut res = String::new();
                res.push_str(s).map_err(|_| StorageError::CapacityExceeded)?;
                Ok(Some(res))
            }
            Err(StorageError::KeyNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Stores a UTF-8 string under `key`.
    fn set_str(&mut self, key: &str, val: &str) -> Result<(), StorageError> {
        self.set(key, val.as_bytes())
    }

    /// Retrieves a 32-bit unsigned integer value for `key`.
    fn get_u32(&self, key: &str) -> Result<Option<u32>, StorageError> {
        let mut buf = [0u8; 4];
        match self.get(key, &mut buf) {
            Ok(4) => Ok(Some(u32::from_be_bytes(buf))),
            Ok(_) | Err(StorageError::BufferTooSmall) => Err(StorageError::InvalidFormat),
            Err(StorageError::KeyNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Stores a 32-bit unsigned integer in big-endian byte order under `key`.
    fn set_u32(&mut self, key: &str, val: u32) -> Result<(), StorageError> {
        self.set(key, &val.to_be_bytes())
    }

    /// Retrieves an 8-bit unsigned integer value for `key`.
    fn get_u8(&self, key: &str) -> Result<Option<u8>, StorageError> {
        let mut buf = [0u8; 1];
        match self.get(key, &mut buf) {
            Ok(1) => Ok(Some(buf[0])),
            Ok(_) | Err(StorageError::BufferTooSmall) => Err(StorageError::InvalidFormat),
            Err(StorageError::KeyNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Stores an 8-bit unsigned integer under `key`.
    fn set_u8(&mut self, key: &str, val: u8) -> Result<(), StorageError> {
        self.set(key, &[val])
    }

    /// Retrieves a boolean flag for `key`.
    fn get_bool(&self, key: &str) -> Result<Option<bool>, StorageError> {
        match self.get_u8(key)? {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(_) => Err(StorageError::InvalidFormat),
            None => Ok(None),
        }
    }

    /// Stores a boolean flag (0 = false, 1 = true) under `key`.
    fn set_bool(&mut self, key: &str, val: bool) -> Result<(), StorageError> {
        self.set_u8(key, if val { 1 } else { 0 })
    }
}

/// In-memory, zero-heap key-value store backed by `heapless::Vec`.
#[derive(Debug, Clone)]
pub struct MemConfigStore<const MAX_ENTRIES: usize = 32> {
    entries: Vec<(String<32>, Vec<u8, 128>), MAX_ENTRIES>,
}

impl<const MAX_ENTRIES: usize> Default for MemConfigStore<MAX_ENTRIES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_ENTRIES: usize> MemConfigStore<MAX_ENTRIES> {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn iter(&self) -> core::slice::Iter<'_, (String<32>, Vec<u8, 128>)> {
        self.entries.iter()
    }
}

impl<const MAX_ENTRIES: usize> KeyValueStorage for MemConfigStore<MAX_ENTRIES> {
    fn get(&self, key: &str, out: &mut [u8]) -> Result<usize, StorageError> {
        for (k, v) in &self.entries {
            if k.as_str() == key {
                if out.len() < v.len() {
                    return Err(StorageError::BufferTooSmall);
                }
                out[..v.len()].copy_from_slice(v.as_slice());
                return Ok(v.len());
            }
        }
        Err(StorageError::KeyNotFound)
    }

    fn set(&mut self, key: &str, val: &[u8]) -> Result<(), StorageError> {
        if val.len() > 128 {
            return Err(StorageError::CapacityExceeded);
        }

        // Update existing entry if present
        for (k, v) in &mut self.entries {
            if k.as_str() == key {
                v.clear();
                v.extend_from_slice(val).map_err(|_| StorageError::CapacityExceeded)?;
                return Ok(());
            }
        }

        // Insert new entry
        let mut key_str = String::new();
        key_str.push_str(key).map_err(|_| StorageError::CapacityExceeded)?;

        let mut val_vec = Vec::new();
        val_vec.extend_from_slice(val).map_err(|_| StorageError::CapacityExceeded)?;

        self.entries
            .push((key_str, val_vec))
            .map_err(|_| StorageError::CapacityExceeded)?;

        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k.as_str() == key) {
            self.entries.swap_remove(pos);
            Ok(())
        } else {
            Err(StorageError::KeyNotFound)
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k.as_str() == key)
    }
}

/// Dual-CRC32 validated Flash journal serializer for non-volatile storage.
pub struct FlashJournal;

impl FlashJournal {
    pub const MAGIC: [u8; 4] = *b"SLNV"; // Squeezelite NVS
    pub const VERSION: u8 = 1;
    pub const HEADER_SIZE: usize = 16; // 4 magic + 1 ver + 2 count + 1 rsvd + 4 seq + 4 crc

    /// Returns true if sequence number `a` is newer than or equal to `b`,
    /// accounting for modular serial number wraparound.
    #[inline]
    pub fn is_seq_newer_or_equal(a: u32, b: u32) -> bool {
        (a.wrapping_sub(b) as i32) >= 0
    }

    /// Serializes all entries from `store` into `sector` with sector and per-entry CRC32 using `sequence`.
    /// Returns total bytes written.
    pub fn save_to_sector_with_seq<const MAX_ENTRIES: usize>(
        store: &MemConfigStore<MAX_ENTRIES>,
        sector: &mut [u8],
        sequence: u32,
    ) -> Result<usize, StorageError> {
        if sector.len() < Self::HEADER_SIZE {
            return Err(StorageError::BufferTooSmall);
        }

        // Header placeholder
        sector[0..4].copy_from_slice(&Self::MAGIC);
        sector[4] = Self::VERSION;
        sector[5..7].copy_from_slice(&(store.len() as u16).to_be_bytes());
        sector[7] = 0; // Reserved
        sector[8..12].copy_from_slice(&sequence.to_be_bytes());

        let mut offset = Self::HEADER_SIZE;

        for (k, v) in store.iter() {
            let key_bytes = k.as_bytes();
            let val_bytes = v.as_slice();
            let entry_len = 1 + 2 + 4 + key_bytes.len() + val_bytes.len();

            if offset + entry_len > sector.len() {
                return Err(StorageError::BufferTooSmall);
            }

            // Entry header: key_len (1) + val_len (2) + entry_crc (4)
            sector[offset] = key_bytes.len() as u8;
            sector[offset + 1..offset + 3].copy_from_slice(&(val_bytes.len() as u16).to_be_bytes());

            // Compute robust, non-commutative entry CRC over lengths and payload
            let mut crc = 0xFFFF_FFFF;
            crc = crc32_update(crc, &[key_bytes.len() as u8]);
            crc = crc32_update(crc, &(val_bytes.len() as u16).to_be_bytes());
            crc = crc32_update(crc, key_bytes);
            crc = crc32_update(crc, val_bytes);
            let entry_crc = !crc;
            sector[offset + 3..offset + 7].copy_from_slice(&entry_crc.to_be_bytes());

            offset += 7;
            sector[offset..offset + key_bytes.len()].copy_from_slice(key_bytes);
            offset += key_bytes.len();
            sector[offset..offset + val_bytes.len()].copy_from_slice(val_bytes);
            offset += val_bytes.len();
        }

        // Sector CRC over header (0..12) combined with payload bytes (HEADER_SIZE..offset)
        let mut crc = 0xFFFF_FFFF;
        crc = crc32_update(crc, &sector[0..12]);
        crc = crc32_update(crc, &sector[Self::HEADER_SIZE..offset]);
        let sector_crc = !crc;
        sector[12..16].copy_from_slice(&sector_crc.to_be_bytes());

        Ok(offset)
    }

    /// Serializes all entries from `store` into a single `sector` with default sequence 1.
    pub fn save_to_sector<const MAX_ENTRIES: usize>(
        store: &MemConfigStore<MAX_ENTRIES>,
        sector: &mut [u8],
    ) -> Result<usize, StorageError> {
        Self::save_to_sector_with_seq(store, sector, 1)
    }

    /// Reads and validates sector metadata (sequence, entry_count, data_len) without mutating store.
    pub fn read_sector_meta(sector: &[u8]) -> Result<(u32, usize, usize), StorageError> {
        if sector.len() < Self::HEADER_SIZE {
            return Err(StorageError::BufferTooSmall);
        }

        if sector[0..4] != Self::MAGIC || sector[4] != Self::VERSION || sector[7] != 0 {
            return Err(StorageError::InvalidFormat);
        }

        let entry_count = u16::from_be_bytes([sector[5], sector[6]]) as usize;
        let sequence = u32::from_be_bytes([sector[8], sector[9], sector[10], sector[11]]);
        let expected_sector_crc = u32::from_be_bytes([sector[12], sector[13], sector[14], sector[15]]);

        // First pass: validate entry structure and find total data length
        let mut offset = Self::HEADER_SIZE;
        for _ in 0..entry_count {
            if offset + 7 > sector.len() {
                return Err(StorageError::CorruptedData);
            }
            let key_len = sector[offset] as usize;
            let val_len = u16::from_be_bytes([sector[offset + 1], sector[offset + 2]]) as usize;
            if key_len == 0 || key_len > 32 || val_len > 128 {
                return Err(StorageError::InvalidFormat);
            }
            offset += 7;

            if offset + key_len + val_len > sector.len() {
                return Err(StorageError::CorruptedData);
            }
            offset += key_len + val_len;
        }

        // Verify sector CRC over header and payload
        let mut crc = 0xFFFF_FFFF;
        crc = crc32_update(crc, &sector[0..12]);
        crc = crc32_update(crc, &sector[Self::HEADER_SIZE..offset]);
        if !crc != expected_sector_crc {
            return Err(StorageError::CorruptedData);
        }

        Ok((sequence, entry_count, offset))
    }

    /// Deserializes entries from `sector` into `store` after validating CRCs.
    /// Uses temporary staging to avoid wiping `store` on partial failure (non-destructive).
    pub fn load_from_sector<const MAX_ENTRIES: usize>(
        sector: &[u8],
        store: &mut MemConfigStore<MAX_ENTRIES>,
    ) -> Result<usize, StorageError> {
        let (_sequence, entry_count, data_len) = Self::read_sector_meta(sector)?;

        if entry_count > MAX_ENTRIES {
            return Err(StorageError::CapacityExceeded);
        }

        let mut staging = MemConfigStore::<MAX_ENTRIES>::new();
        let mut read_offset = Self::HEADER_SIZE;
        for _ in 0..entry_count {
            let key_len = sector[read_offset] as usize;
            let val_len = u16::from_be_bytes([sector[read_offset + 1], sector[read_offset + 2]]) as usize;
            let expected_entry_crc = u32::from_be_bytes([
                sector[read_offset + 3],
                sector[read_offset + 4],
                sector[read_offset + 5],
                sector[read_offset + 6],
            ]);

            read_offset += 7;
            let key_slice = &sector[read_offset..read_offset + key_len];
            read_offset += key_len;
            let val_slice = &sector[read_offset..read_offset + val_len];
            read_offset += val_len;

            let mut crc = 0xFFFF_FFFF;
            crc = crc32_update(crc, &[key_len as u8]);
            crc = crc32_update(crc, &(val_len as u16).to_be_bytes());
            crc = crc32_update(crc, key_slice);
            crc = crc32_update(crc, val_slice);

            if !crc != expected_entry_crc {
                return Err(StorageError::CorruptedData);
            }

            let key_str = core::str::from_utf8(key_slice).map_err(|_| StorageError::InvalidUtf8)?;
            if staging.contains(key_str) {
                return Err(StorageError::InvalidFormat);
            }
            staging.set(key_str, val_slice)?;
        }

        if read_offset != data_len {
            return Err(StorageError::InvalidFormat);
        }

        // Commit all entries atomically into destination store
        *store = staging;
        Ok(entry_count)
    }

    /// Loads configuration from dual ping-pong sectors (A/B), selecting the valid sector with
    /// the newest sequence counter. Provides power-loss resilience.
    pub fn load_from_dual_sectors<const MAX_ENTRIES: usize>(
        sector_a: &[u8],
        sector_b: &[u8],
        store: &mut MemConfigStore<MAX_ENTRIES>,
    ) -> Result<usize, StorageError> {
        let meta_a = Self::read_sector_meta(sector_a);
        let meta_b = Self::read_sector_meta(sector_b);

        match (meta_a, meta_b) {
            (Ok((seq_a, _, _)), Ok((seq_b, _, _))) => {
                let (primary, fallback) = if Self::is_seq_newer_or_equal(seq_a, seq_b) {
                    (sector_a, sector_b)
                } else {
                    (sector_b, sector_a)
                };
                match Self::load_from_sector(primary, store) {
                    Ok(count) => Ok(count),
                    Err(_) => Self::load_from_sector(fallback, store),
                }
            }
            (Ok(_), Err(_)) => Self::load_from_sector(sector_a, store),
            (Err(_), Ok(_)) => Self::load_from_sector(sector_b, store),
            (Err(e), Err(_)) => Err(e),
        }
    }

    /// Saves configuration to the alternate ping-pong sector with an incremented sequence counter.
    /// Returns a tuple of `(wrote_to_b: bool, bytes_written: usize)`.
    pub fn save_to_dual_sectors<const MAX_ENTRIES: usize>(
        store: &MemConfigStore<MAX_ENTRIES>,
        sector_a: &mut [u8],
        sector_b: &mut [u8],
    ) -> Result<(bool, usize), StorageError> {
        let meta_a = Self::read_sector_meta(sector_a);
        let meta_b = Self::read_sector_meta(sector_b);

        let (write_to_b, next_seq) = match (meta_a, meta_b) {
            (Ok((seq_a, _, _)), Ok((seq_b, _, _))) => {
                if Self::is_seq_newer_or_equal(seq_a, seq_b) {
                    (true, seq_a.wrapping_add(1))
                } else {
                    (false, seq_b.wrapping_add(1))
                }
            }
            (Ok((seq_a, _, _)), Err(_)) => (true, seq_a.wrapping_add(1)),
            (Err(_), Ok((seq_b, _, _))) => (false, seq_b.wrapping_add(1)),
            (Err(_), Err(_)) => (false, 1),
        };

        if write_to_b {
            let n = Self::save_to_sector_with_seq(store, sector_b, next_seq)?;
            Ok((true, n))
        } else {
            let n = Self::save_to_sector_with_seq(store, sector_a, next_seq)?;
            Ok((false, n))
        }
    }
}

/// Strongly typed Wi-Fi station credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct WifiCredentials {
    pub ssid: String<32>,
    pub passphrase: String<64>,
    pub autoconnect: bool,
    pub static_ip: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub subnet: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
}

impl core::fmt::Debug for WifiCredentials {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WifiCredentials")
            .field("ssid", &self.ssid)
            .field("passphrase", &"[REDACTED]")
            .field("autoconnect", &self.autoconnect)
            .field("static_ip", &self.static_ip)
            .field("gateway", &self.gateway)
            .field("subnet", &self.subnet)
            .field("dns", &self.dns)
            .finish()
    }
}

impl WifiCredentials {
    pub const KEY_SSID: &'static str = "wifi.ssid";
    pub const KEY_PASS: &'static str = "wifi.pass";
    pub const KEY_AUTO: &'static str = "wifi.auto";
    pub const KEY_IP: &'static str = "wifi.ip";
    pub const KEY_GW: &'static str = "wifi.gw";
    pub const KEY_MASK: &'static str = "wifi.mask";
    pub const KEY_DNS: &'static str = "wifi.dns";

    pub fn new(ssid: &str, passphrase: &str) -> Result<Self, StorageError> {
        let mut s = String::new();
        s.push_str(ssid).map_err(|_| StorageError::CapacityExceeded)?;
        let mut p = String::new();
        p.push_str(passphrase).map_err(|_| StorageError::CapacityExceeded)?;

        Ok(Self {
            ssid: s,
            passphrase: p,
            autoconnect: true,
            static_ip: None,
            gateway: None,
            subnet: None,
            dns: None,
        })
    }

    /// Validates Wi-Fi configuration invariants.
    pub fn validate(&self) -> Result<(), StorageError> {
        if let Some(ip) = self.static_ip {
            if ip == [0, 0, 0, 0] || ip == [255, 255, 255, 255] {
                return Err(StorageError::InvalidFormat);
            }
        }
        if let Some(gw) = self.gateway {
            if gw == [0, 0, 0, 0] || gw == [255, 255, 255, 255] {
                return Err(StorageError::InvalidFormat);
            }
        }
        if let Some(mask) = self.subnet {
            if mask == [0, 0, 0, 0] {
                return Err(StorageError::InvalidFormat);
            }
        }
        Ok(())
    }

    /// Saves Wi-Fi credentials into `store`.
    pub fn save_to<S: KeyValueStorage>(&self, store: &mut S) -> Result<(), StorageError> {
        store.set_str(Self::KEY_SSID, self.ssid.as_str())?;
        store.set_str(Self::KEY_PASS, self.passphrase.as_str())?;
        store.set_bool(Self::KEY_AUTO, self.autoconnect)?;

        if let Some(ip) = self.static_ip {
            store.set(Self::KEY_IP, &ip)?;
        } else {
            let _ = store.remove(Self::KEY_IP);
        }

        if let Some(gw) = self.gateway {
            store.set(Self::KEY_GW, &gw)?;
        } else {
            let _ = store.remove(Self::KEY_GW);
        }

        if let Some(mask) = self.subnet {
            store.set(Self::KEY_MASK, &mask)?;
        } else {
            let _ = store.remove(Self::KEY_MASK);
        }

        if let Some(dns) = self.dns {
            store.set(Self::KEY_DNS, &dns)?;
        } else {
            let _ = store.remove(Self::KEY_DNS);
        }

        Ok(())
    }

    /// Loads Wi-Fi credentials from `store` if present.
    pub fn load_from<S: KeyValueStorage>(store: &S) -> Result<Option<Self>, StorageError> {
        let ssid = match store.get_str::<32>(Self::KEY_SSID)? {
            Some(s) => s,
            None => return Ok(None),
        };

        let passphrase = store.get_str::<64>(Self::KEY_PASS)?.unwrap_or_default();
        let autoconnect = store.get_bool(Self::KEY_AUTO)?.unwrap_or(true);

        let static_ip = read_ip_opt(store, Self::KEY_IP)?;
        let gateway = read_ip_opt(store, Self::KEY_GW)?;
        let subnet = read_ip_opt(store, Self::KEY_MASK)?;
        let dns = read_ip_opt(store, Self::KEY_DNS)?;

        Ok(Some(Self {
            ssid,
            passphrase,
            autoconnect,
            static_ip,
            gateway,
            subnet,
            dns,
        }))
    }

    /// Removes Wi-Fi credentials from `store`.
    pub fn clear<S: KeyValueStorage>(store: &mut S) -> Result<(), StorageError> {
        let _ = store.remove(Self::KEY_SSID);
        let _ = store.remove(Self::KEY_PASS);
        let _ = store.remove(Self::KEY_AUTO);
        let _ = store.remove(Self::KEY_IP);
        let _ = store.remove(Self::KEY_GW);
        let _ = store.remove(Self::KEY_MASK);
        let _ = store.remove(Self::KEY_DNS);
        Ok(())
    }
}

/// Strongly typed player audio & LMS server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioConfig {
    pub player_name: String<32>,
    pub lms_server_ip: Option<[u8; 4]>,
    pub lms_server_port: Option<u16>,
    pub volume: u8,
    pub equalizer_preset: u8,
}

impl Default for AudioConfig {
    fn default() -> Self {
        let mut name = String::new();
        let _ = name.push_str("LouderESP32");
        Self {
            player_name: name,
            lms_server_ip: None,
            lms_server_port: None,
            volume: 80,
            equalizer_preset: 0,
        }
    }
}

impl AudioConfig {
    pub const KEY_NAME: &'static str = "audio.name";
    pub const KEY_LMS_IP: &'static str = "audio.lms_ip";
    pub const KEY_LMS_PORT: &'static str = "audio.lms_port";
    pub const KEY_VOL: &'static str = "audio.vol";
    pub const KEY_EQ: &'static str = "audio.eq";

    /// Validates player audio configuration invariants.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.player_name.is_empty() {
            return Err(StorageError::InvalidFormat);
        }
        if self.volume > 100 {
            return Err(StorageError::InvalidFormat);
        }
        if self.lms_server_port == Some(0) {
            return Err(StorageError::InvalidFormat);
        }
        if let Some(ip) = self.lms_server_ip {
            if ip == [0, 0, 0, 0] || ip == [255, 255, 255, 255] {
                return Err(StorageError::InvalidFormat);
            }
        }
        Ok(())
    }

    pub fn save_to<S: KeyValueStorage>(&self, store: &mut S) -> Result<(), StorageError> {
        self.validate()?;
        store.set_str(Self::KEY_NAME, self.player_name.as_str())?;
        store.set_u8(Self::KEY_VOL, self.volume)?;
        store.set_u8(Self::KEY_EQ, self.equalizer_preset)?;

        if let Some(ip) = self.lms_server_ip {
            store.set(Self::KEY_LMS_IP, &ip)?;
        } else {
            let _ = store.remove(Self::KEY_LMS_IP);
        }

        if let Some(port) = self.lms_server_port {
            store.set_u32(Self::KEY_LMS_PORT, port as u32)?;
        } else {
            let _ = store.remove(Self::KEY_LMS_PORT);
        }

        Ok(())
    }

    pub fn load_from<S: KeyValueStorage>(store: &S) -> Result<Self, StorageError> {
        let mut config = Self::default();

        if let Some(name) = store.get_str::<32>(Self::KEY_NAME)? {
            config.player_name = name;
        }

        if let Some(vol) = store.get_u8(Self::KEY_VOL)? {
            config.volume = vol;
        }

        if let Some(eq) = store.get_u8(Self::KEY_EQ)? {
            config.equalizer_preset = eq;
        }

        config.lms_server_ip = read_ip_opt(store, Self::KEY_LMS_IP)?;

        if let Some(port) = store.get_u32(Self::KEY_LMS_PORT)? {
            let port_u16 = u16::try_from(port).map_err(|_| StorageError::InvalidFormat)?;
            config.lms_server_port = Some(port_u16);
        }

        config.validate()?;
        Ok(config)
    }
}

fn read_ip_opt<S: KeyValueStorage>(store: &S, key: &str) -> Result<Option<[u8; 4]>, StorageError> {
    let mut buf = [0u8; 4];
    match store.get(key, &mut buf) {
        Ok(4) => Ok(Some(buf)),
        Ok(_) | Err(StorageError::BufferTooSmall) => Err(StorageError::InvalidFormat),
        Err(StorageError::KeyNotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::format;
    use super::*;

    #[test]
    fn test_mem_store_basic_operations() {
        let mut store = MemConfigStore::<16>::new();
        assert_eq!(store.len(), 0);
        assert!(!store.contains("wifi.ssid"));

        store.set_str("wifi.ssid", "Studio_Network").unwrap();
        assert_eq!(store.len(), 1);
        assert!(store.contains("wifi.ssid"));

        let retrieved = store.get_str::<32>("wifi.ssid").unwrap().unwrap();
        assert_eq!(retrieved.as_str(), "Studio_Network");

        store.set_u8("audio.vol", 95).unwrap();
        assert_eq!(store.get_u8("audio.vol").unwrap(), Some(95));

        store.remove("wifi.ssid").unwrap();
        assert!(!store.contains("wifi.ssid"));
        assert_eq!(store.get_str::<32>("wifi.ssid").unwrap(), None);
    }

    #[test]
    fn test_wifi_credentials_save_and_load() {
        let mut store = MemConfigStore::<16>::new();

        let mut creds = WifiCredentials::new("LouderNetwork", "SuperSecretKey99").unwrap();
        creds.static_ip = Some([192, 168, 1, 150]);
        creds.gateway = Some([192, 168, 1, 1]);
        creds.subnet = Some([255, 255, 255, 0]);
        creds.dns = Some([1, 1, 1, 1]);

        creds.save_to(&mut store).unwrap();

        let loaded = WifiCredentials::load_from(&store).unwrap().unwrap();
        assert_eq!(loaded, creds);

        WifiCredentials::clear(&mut store).unwrap();
        assert_eq!(WifiCredentials::load_from(&store).unwrap(), None);
    }

    #[test]
    fn test_flash_journal_save_and_load() {
        let mut store = MemConfigStore::<16>::new();
        store.set_str("wifi.ssid", "Main_AP").unwrap();
        store.set_str("wifi.pass", "Pass1234").unwrap();
        store.set_u8("audio.vol", 85).unwrap();
        store.set_u32("audio.baud", 115200).unwrap();

        let mut sector = [0u8; 1024];
        let bytes_written = FlashJournal::save_to_sector(&store, &mut sector).unwrap();
        assert!(bytes_written > FlashJournal::HEADER_SIZE);

        let mut restored_store = MemConfigStore::<16>::new();
        let entries = FlashJournal::load_from_sector(&sector[..bytes_written], &mut restored_store).unwrap();
        assert_eq!(entries, 4);

        assert_eq!(restored_store.get_str::<32>("wifi.ssid").unwrap().unwrap().as_str(), "Main_AP");
        assert_eq!(restored_store.get_str::<32>("wifi.pass").unwrap().unwrap().as_str(), "Pass1234");
        assert_eq!(restored_store.get_u8("audio.vol").unwrap(), Some(85));
        assert_eq!(restored_store.get_u32("audio.baud").unwrap(), Some(115200));
    }

    #[test]
    fn test_flash_journal_detects_corruption() {
        let mut store = MemConfigStore::<16>::new();
        store.set_str("wifi.ssid", "SecureAP").unwrap();

        let mut sector = [0u8; 512];
        let bytes_written = FlashJournal::save_to_sector(&store, &mut sector).unwrap();

        // Corrupt a byte in the payload
        sector[FlashJournal::HEADER_SIZE + 5] ^= 0xFF;

        let mut restored_store = MemConfigStore::<16>::new();
        let res = FlashJournal::load_from_sector(&sector[..bytes_written], &mut restored_store);
        assert_eq!(res, Err(StorageError::CorruptedData));
    }

    #[test]
    fn test_wifi_credentials_debug_masks_passphrase() {
        let creds = WifiCredentials::new("GuestWiFi", "SuperSecretPass").unwrap();
        let formatted = format!("{:?}", creds);
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("SuperSecretPass"));
    }

    #[test]
    fn test_dual_sector_ping_pong_power_cut_resilience() {
        let mut store = MemConfigStore::<16>::new();
        store.set_str("setting.k1", "v1").unwrap();

        let mut sector_a = [0u8; 512];
        let mut sector_b = [0u8; 512];

        // First write -> goes to sector A (sequence 1)
        let (wrote_b, n1) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
        assert!(!wrote_b);
        assert!(n1 > FlashJournal::HEADER_SIZE);

        // Second write -> goes to sector B (sequence 2)
        store.set_str("setting.k1", "v2").unwrap();
        let (wrote_b2, n2) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
        assert!(wrote_b2);
        assert!(n2 > FlashJournal::HEADER_SIZE);

        // Load selects sector B (seq 2 > seq 1)
        let mut loaded = MemConfigStore::<16>::new();
        FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut loaded).unwrap();
        assert_eq!(loaded.get_str::<32>("setting.k1").unwrap().unwrap().as_str(), "v2");

        // Simulate power cut during write to B by corrupting sector B
        sector_b[FlashJournal::HEADER_SIZE + 2] ^= 0xFF;

        // Fallback loads cleanly from intact sector A (v1)
        let mut fallback = MemConfigStore::<16>::new();
        FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut fallback).unwrap();
        assert_eq!(fallback.get_str::<32>("setting.k1").unwrap().unwrap().as_str(), "v1");
    }

    #[test]
    fn test_non_destructive_load_on_corruption() {
        let mut store = MemConfigStore::<16>::new();
        store.set_str("existing", "preserved").unwrap();

        let mut bad_sector = [0u8; 256];
        bad_sector[0..4].copy_from_slice(b"SLNV");
        bad_sector[4] = 1;
        bad_sector[5..7].copy_from_slice(&1u16.to_be_bytes()); // 1 entry
        bad_sector[8..12].copy_from_slice(&1u32.to_be_bytes()); // seq 1
        // Bad CRC
        bad_sector[12..16].copy_from_slice(&0xDEADBEEFu32.to_be_bytes());

        let res = FlashJournal::load_from_sector(&bad_sector, &mut store);
        assert!(res.is_err());
        // Store must NOT have been cleared!
        assert_eq!(store.get_str::<32>("existing").unwrap().unwrap().as_str(), "preserved");
    }

    #[test]
    fn test_sequence_number_wraparound_logic() {
        assert!(FlashJournal::is_seq_newer_or_equal(0, u32::MAX));
        assert!(!FlashJournal::is_seq_newer_or_equal(u32::MAX, 0));
        assert!(FlashJournal::is_seq_newer_or_equal(5, 5));
        assert!(FlashJournal::is_seq_newer_or_equal(10, 5));
        assert!(!FlashJournal::is_seq_newer_or_equal(5, 10));
        assert!(FlashJournal::is_seq_newer_or_equal(1, 0));
    }

    #[test]
    fn test_scalar_accessor_error_mapping() {
        let mut store = MemConfigStore::<16>::new();
        // Store 5 bytes under u32 key -> get_u32 must return InvalidFormat (BufferTooSmall mapped)
        store.set("bad.u32", &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(store.get_u32("bad.u32"), Err(StorageError::InvalidFormat));

        // Store 3 bytes under u32 key -> get_u32 must return InvalidFormat
        store.set("short.u32", &[1, 2, 3]).unwrap();
        assert_eq!(store.get_u32("short.u32"), Err(StorageError::InvalidFormat));

        // Store 2 bytes under u8 key -> get_u8 must return InvalidFormat
        store.set("bad.u8", &[1, 2]).unwrap();
        assert_eq!(store.get_u8("bad.u8"), Err(StorageError::InvalidFormat));

        // Store 5 bytes under IP key -> read_ip_opt must return InvalidFormat
        store.set("bad.ip", &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(read_ip_opt(&store, "bad.ip"), Err(StorageError::InvalidFormat));

        // Store 3 bytes under IP key -> read_ip_opt must return InvalidFormat
        store.set("short.ip", &[1, 2, 3]).unwrap();
        assert_eq!(read_ip_opt(&store, "short.ip"), Err(StorageError::InvalidFormat));
    }

    #[test]
    fn test_audio_config_validation() {
        let mut valid_config = AudioConfig::default();
        assert!(valid_config.validate().is_ok());

        // Volume > 100 invalid
        valid_config.volume = 101;
        assert_eq!(valid_config.validate(), Err(StorageError::InvalidFormat));
        valid_config.volume = 100;
        assert!(valid_config.validate().is_ok());

        // Empty player name invalid
        valid_config.player_name.clear();
        assert_eq!(valid_config.validate(), Err(StorageError::InvalidFormat));
        valid_config.player_name.push_str("ValidPlayer").unwrap();
        assert!(valid_config.validate().is_ok());

        // Port == 0 invalid
        valid_config.lms_server_port = Some(0);
        assert_eq!(valid_config.validate(), Err(StorageError::InvalidFormat));
        valid_config.lms_server_port = Some(9000);
        assert!(valid_config.validate().is_ok());

        // IP [0,0,0,0] or [255,255,255,255] invalid
        valid_config.lms_server_ip = Some([0, 0, 0, 0]);
        assert_eq!(valid_config.validate(), Err(StorageError::InvalidFormat));
        valid_config.lms_server_ip = Some([255, 255, 255, 255]);
        assert_eq!(valid_config.validate(), Err(StorageError::InvalidFormat));
        valid_config.lms_server_ip = Some([192, 168, 1, 50]);
        assert!(valid_config.validate().is_ok());

        // AudioConfig save_to validates config
        let mut store = MemConfigStore::<16>::new();
        valid_config.volume = 105;
        assert_eq!(valid_config.save_to(&mut store), Err(StorageError::InvalidFormat));
    }

    #[test]
    fn test_wifi_credentials_validation() {
        let mut creds = WifiCredentials::new("SSID", "PASS").unwrap();
        assert!(creds.validate().is_ok());

        creds.static_ip = Some([0, 0, 0, 0]);
        assert_eq!(creds.validate(), Err(StorageError::InvalidFormat));
        creds.static_ip = Some([255, 255, 255, 255]);
        assert_eq!(creds.validate(), Err(StorageError::InvalidFormat));
        creds.static_ip = Some([192, 168, 1, 10]);
        assert!(creds.validate().is_ok());

        creds.gateway = Some([0, 0, 0, 0]);
        assert_eq!(creds.validate(), Err(StorageError::InvalidFormat));
        creds.gateway = Some([255, 255, 255, 255]);
        assert_eq!(creds.validate(), Err(StorageError::InvalidFormat));
        creds.gateway = Some([192, 168, 1, 1]);
        assert!(creds.validate().is_ok());

        creds.subnet = Some([0, 0, 0, 0]);
        assert_eq!(creds.validate(), Err(StorageError::InvalidFormat));
        creds.subnet = Some([255, 255, 255, 0]);
        assert!(creds.validate().is_ok());
    }
}
