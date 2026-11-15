//! Wi-Fi Station Manager and Network Provisioning
//!
//! Provides network connection management backed by pure-Rust
//! key-value storage:
//! - Loads persisted credentials from `squeezelite_config::KeyValueStorage`
//!   on initialization.
//! - Persists credentials across reboots on successful connection.
//! - Simulated network status on host platforms.
//! - Active ESP-IDF Wi-Fi STA driver on ESP32 target hardware
//!   (`target_os = "espidf"`).

use core::fmt;
use heapless::{String, Vec};
use squeezelite_config::{KeyValueStorage, MemConfigStore, StorageError, WifiCredentials};

use crate::ota_safety::decrypt_nvs_wifi_password;

/// Default SoftAP fallback IPv4 address (`192.168.4.1`).
pub const DEFAULT_SOFTAP_IP: [u8; 4] = [192, 168, 4, 1];

/// Default SoftAP fallback IPv4 netmask (`255.255.255.0`).
pub const DEFAULT_SOFTAP_NETMASK: [u8; 4] = [255, 255, 255, 0];

/// Default SoftAP fallback SSID when station association fails.
pub const DEFAULT_SOFTAP_SSID: &str = "squeezelite-esp32";

/// Standard UDP port for Captive Portal DNS hijack responder.
pub const CAPTIVE_DNS_PORT: u16 = 53;

/// Maximum number of entries retained in a [`WifiScanTable`].
pub const MAX_SCAN_TABLE_ENTRIES: usize = 16;

/// Number of persistent AP credential slots in [`KnownApList`].
pub const KNOWN_AP_SLOTS: usize = 4;

/// Operational mode of the SoftAP fallback & captive portal subsystem.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftApMode {
    /// SoftAP interface is disabled; station mode only.
    Inactive,
    /// SoftAP automatically activated after station connection retries failed.
    ActiveFallback,
    /// SoftAP explicitly forced on for provisioning.
    ForcedProvisioning,
}

/// Configuration for the on-device SoftAP provisioning access point.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftApConfig {
    /// Broadcast SSID for the provisioning AP (`1..=32` bytes).
    pub ssid: String<32>,
    /// Optional WPA2 passphrase (`""` for open AP, or `8..=63` bytes).
    pub password: String<64>,
    /// 2.4 GHz 802.11 channel (`1..=14`).
    pub channel: u8,
    /// SoftAP gateway/portal IPv4 address.
    pub ap_ip: [u8; 4],
    /// SoftAP IPv4 subnet mask.
    pub netmask: [u8; 4],
    /// Optional binary WPA2 passphrase (`8..=64` bytes).
    pub ap_passphrase: Option<[u8; 64]>,
    /// Length of binary passphrase (`ap_passphrase`).
    pub ap_passphrase_len: u8,
}

impl Default for SoftApConfig {
    fn default() -> Self {
        let mut ssid = String::new();
        let _ = ssid.push_str(DEFAULT_SOFTAP_SSID);
        Self {
            ssid,
            password: String::new(),
            channel: 1,
            ap_ip: DEFAULT_SOFTAP_IP,
            netmask: DEFAULT_SOFTAP_NETMASK,
            ap_passphrase: None,
            ap_passphrase_len: 0,
        }
    }
}

impl SoftApConfig {
    /// Constructs a validated [`SoftApConfig`].
    pub fn new(ssid: &str, channel: u8, ap_ip: [u8; 4]) -> Result<Self, StorageError> {
        if ssid.is_empty() || ssid.len() > 32 || !(1..=14).contains(&channel) {
            return Err(StorageError::InvalidFormat);
        }
        if ap_ip == [0, 0, 0, 0] || ap_ip == [255, 255, 255, 255] {
            return Err(StorageError::InvalidFormat);
        }
        let mut s = String::new();
        s.push_str(ssid)
            .map_err(|()| StorageError::CapacityExceeded)?;
        Ok(Self {
            ssid: s,
            password: String::new(),
            channel,
            ap_ip,
            netmask: DEFAULT_SOFTAP_NETMASK,
            ap_passphrase: None,
            ap_passphrase_len: 0,
        })
    }

    /// Configures an optional binary WPA2 passphrase (`8..=64` bytes).
    #[must_use]
    pub const fn with_passphrase(mut self, pass: &[u8]) -> Self {
        if pass.len() >= 8 && pass.len() <= 64 {
            let mut buf = [0u8; 64];
            let mut i = 0;
            while i < pass.len() {
                buf[i] = pass[i];
                i += 1;
            }
            self.ap_passphrase = Some(buf);
            self.ap_passphrase_len = pass.len() as u8;
        }
        self
    }
}

/// OS captive-portal HTTP probe classification.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptivePortalProbe {
    /// Android / Chrome connectivity check (`/generate_204`, `/gen_204`).
    AndroidGenerate204,
    /// Apple iOS / macOS Captive Network Assistant (`/hotspot-detect.html`).
    AppleHotspotDetect,
    /// Windows NCSI probe (`/ncsi.txt`, `/connecttest.txt`, `/redirect`).
    WindowsNcsi,
}

/// Detects whether an HTTP request path matches a known OS captive-portal
/// connectivity probe endpoint.
#[must_use]
pub fn detect_captive_portal_probe(raw_path: &str) -> Option<CaptivePortalProbe> {
    let path = raw_path.split('?').next().unwrap_or(raw_path);
    match path {
        "/generate_204" | "/gen_204" => Some(CaptivePortalProbe::AndroidGenerate204),
        "/hotspot-detect.html" | "/library/test/success.html" => {
            Some(CaptivePortalProbe::AppleHotspotDetect)
        }
        "/ncsi.txt" | "/connecttest.txt" | "/redirect" => Some(CaptivePortalProbe::WindowsNcsi),
        _ => None,
    }
}

/// Formats an `HTTP/1.1 302 Found` redirect pointing client captive-portal
/// probes to `http://<portal_ip>/`.
pub fn build_captive_portal_redirect(portal_ip: [u8; 4], out_buf: &mut [u8]) -> Option<usize> {
    use core::fmt::Write as _;
    let mut header: String<192> = String::new();
    write!(
        header,
        "HTTP/1.1 302 Found\r\n\
         Location: http://{}.{}.{}.{}/\r\n\
         Cache-Control: no-store\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\r\n",
        portal_ip[0], portal_ip[1], portal_ip[2], portal_ip[3]
    )
    .ok()?;
    let bytes = header.as_bytes();
    if bytes.len() > out_buf.len() {
        return None;
    }
    out_buf[..bytes.len()].copy_from_slice(bytes);
    Some(bytes.len())
}

/// Stateless UDP Port 53 Captive Portal DNS A-record hijack responder.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptivePortalDns {
    /// IPv4 address returned for all hijacked DNS `A` queries.
    pub portal_ip: [u8; 4],
}

impl Default for CaptivePortalDns {
    fn default() -> Self {
        Self {
            portal_ip: DEFAULT_SOFTAP_IP,
        }
    }
}

impl CaptivePortalDns {
    /// Creates a new DNS hijack responder targeting `portal_ip`.
    #[must_use]
    pub const fn new(portal_ip: [u8; 4]) -> Self {
        Self { portal_ip }
    }

    /// Parses a raw UDP DNS query and builds a wildcard A-record response.
    ///
    /// Validates that the DNS question name contains no compression pointers
    /// before formatting the response.
    #[must_use]
    pub fn handle_query(&self, query: &[u8], out_buf: &mut [u8]) -> Option<usize> {
        self.build_response(query, out_buf)
    }

    /// Parses a raw UDP DNS query packet and writes a wildcard `A`-record
    /// response pointing to `self.portal_ip` into `out_buf`.
    ///
    /// Returns the response length in bytes, or `None` if `query` is malformed
    /// or `out_buf` is too small.
    pub fn build_response(&self, query: &[u8], out_buf: &mut [u8]) -> Option<usize> {
        // Minimum DNS header (12 bytes) + 1-byte root label + QTYPE (2) + QCLASS (2).
        if query.len() < 17 {
            return None;
        }
        let flags = u16::from_be_bytes([query[2], query[3]]);
        // Reject packets that are already responses (QR bit 15 == 1).
        if (flags & 0x8000) != 0 {
            return None;
        }
        // Verify standard query opcode (bits 11-14: (flags & 0x7800) == 0).
        if (flags & 0x7800) != 0 {
            return None;
        }
        let qdcount = u16::from_be_bytes([query[4], query[5]]);
        if qdcount == 0 {
            return None;
        }

        // Walk the first question QNAME starting at byte 12.
        let mut pos = 12usize;
        while pos < query.len() {
            let label_len = usize::from(query[pos]);
            if label_len == 0 {
                pos += 1;
                break;
            }
            // Reject DNS compression pointer (0xC0) in question name (AUD03-SEC-001).
            if (query[pos] & 0xC0) == 0xC0 {
                return None;
            }
            // Reject invalid label lengths (> 63).
            if label_len > 63 || pos + 1 + label_len >= query.len() {
                return None;
            }
            pos += 1 + label_len;
        }

        if pos + 4 > query.len() {
            return None;
        }
        let qtype = u16::from_be_bytes([query[pos], query[pos + 1]]);
        let qclass = u16::from_be_bytes([query[pos + 2], query[pos + 3]]);
        let question_end = pos + 4;
        let question_slice = &query[12..question_end];

        // Answer with an A record for QTYPE A (1) or ANY (255) on IN class (1).
        // For other query types (e.g. AAAA = 28), return NOERROR with ANCOUNT=0.
        let include_answer = (qtype == 1 || qtype == 255) && (qclass == 1 || qclass == 255);
        let answer_len = if include_answer { 16 } else { 0 };
        let total_len = 12 + question_slice.len() + answer_len;
        if total_len > out_buf.len() {
            return None;
        }

        // Transaction ID
        out_buf[0] = query[0];
        out_buf[1] = query[1];
        // Flags: QR=1, Opcode=0, AA=1, RD=(copied), RA=1, RCODE=0
        let rd = flags & 0x0100;
        let resp_flags: u16 = 0x8480 | rd;
        out_buf[2..4].copy_from_slice(&resp_flags.to_be_bytes());
        // QDCOUNT = 1
        out_buf[4..6].copy_from_slice(&1u16.to_be_bytes());
        // ANCOUNT = 1 or 0
        let ancount: u16 = u16::from(include_answer);
        out_buf[6..8].copy_from_slice(&ancount.to_be_bytes());
        // NSCOUNT = 0, ARCOUNT = 0
        out_buf[8..12].fill(0);

        // Copy single question section
        let q_end_out = 12 + question_slice.len();
        out_buf[12..q_end_out].copy_from_slice(question_slice);

        if include_answer {
            let ans = &mut out_buf[q_end_out..total_len];
            // Name pointer to offset 12 (0xC00C)
            ans[0] = 0xC0;
            ans[1] = 0x0C;
            // TYPE = A (0x0001)
            ans[2..4].copy_from_slice(&1u16.to_be_bytes());
            // CLASS = IN (0x0001)
            ans[4..6].copy_from_slice(&1u16.to_be_bytes());
            // TTL = 60 seconds
            ans[6..10].copy_from_slice(&60u32.to_be_bytes());
            // RDLENGTH = 4
            ans[10..12].copy_from_slice(&4u16.to_be_bytes());
            // RDATA = portal IPv4
            ans[12..16].copy_from_slice(&self.portal_ip);
        }

        Some(total_len)
    }
}

/// 802.11 authentication / security mode reported by AP scans.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiAuthMode {
    /// Open (unencrypted) network.
    Open,
    /// Legacy WEP network.
    Wep,
    /// WPA-Personal (TKIP).
    WpaPsk,
    /// WPA2-Personal (CCMP).
    Wpa2Psk,
    /// Mixed WPA/WPA2-Personal.
    WpaWpa2Psk,
    /// WPA3-Personal (SAE).
    Wpa3Psk,
    /// WPA2/WPA3-Enterprise (802.1X).
    Enterprise,
}

impl WifiAuthMode {
    /// Returns the short JSON/UI string label for this authentication mode.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Wep => "WEP",
            Self::WpaPsk => "WPA_PSK",
            Self::Wpa2Psk => "WPA2_PSK",
            Self::WpaWpa2Psk => "WPA_WPA2_PSK",
            Self::Wpa3Psk => "WPA3_PSK",
            Self::Enterprise => "ENTERPRISE",
        }
    }
}

/// Single Wi-Fi access point discovered during an 802.11 scan.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedAp {
    /// Broadcast SSID (`1..=32` bytes).
    pub ssid: String<32>,
    /// 6-byte BSSID MAC address.
    pub bssid: [u8; 6],
    /// Received Signal Strength Indicator in dBm (e.g. `-45`).
    pub rssi: i8,
    /// 2.4 GHz channel number (`1..=14`).
    pub channel: u8,
    /// Security / authentication mode.
    pub auth: WifiAuthMode,
}

impl ScannedAp {
    /// Creates a new [`ScannedAp`] descriptor.
    pub fn new(
        ssid: &str,
        bssid: [u8; 6],
        rssi: i8,
        channel: u8,
        auth: WifiAuthMode,
    ) -> Result<Self, StorageError> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(StorageError::InvalidFormat);
        }
        let mut s = String::new();
        s.push_str(ssid)
            .map_err(|()| StorageError::CapacityExceeded)?;
        Ok(Self {
            ssid: s,
            bssid,
            rssi,
            channel,
            auth,
        })
    }
}

/// Bounded table of scanned Wi-Fi access points, deduplicated by SSID and
/// ordered by descending RSSI (strongest signal first).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiScanTable {
    entries: Vec<ScannedAp, MAX_SCAN_TABLE_ENTRIES>,
}

impl WifiScanTable {
    /// Creates an empty [`WifiScanTable`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Clears all scanned AP entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of unique SSIDs currently in the scan table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no APs are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Slice of scanned APs sorted from strongest RSSI to weakest.
    #[must_use]
    pub fn entries(&self) -> &[ScannedAp] {
        self.entries.as_slice()
    }

    /// Inserts or updates a scanned AP, keeping the strongest RSSI per SSID
    /// and maintaining descending RSSI order.
    pub fn upsert(&mut self, ap: ScannedAp) {
        let mut updated = false;
        for existing in &mut self.entries {
            if existing.ssid == ap.ssid {
                if ap.rssi >= existing.rssi {
                    *existing = ap.clone();
                }
                updated = true;
                break;
            }
        }
        if !updated {
            if self.entries.len() < MAX_SCAN_TABLE_ENTRIES {
                let _ = self.entries.push(ap);
            } else if let Some(last) = self.entries.last_mut() {
                if ap.rssi > last.rssi {
                    *last = ap;
                }
            }
        }
        // Insertion sort by descending RSSI.
        let n = self.entries.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 && self.entries[j].rssi > self.entries[j - 1].rssi {
                self.entries.swap(j, j - 1);
                j -= 1;
            }
        }
    }

    /// Looks up a scanned AP entry by exact SSID.
    #[must_use]
    pub fn best_for_ssid(&self, ssid: &str) -> Option<&ScannedAp> {
        self.entries.iter().find(|e| e.ssid.as_str() == ssid)
    }
}

/// Entry in the 4-slot [`KnownApList`].
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct KnownApEntry {
    /// Target Wi-Fi SSID (`1..=32` bytes).
    pub ssid: String<32>,
    /// Decrypted WPA/WPA2 passphrase (`0` or `8..=63` bytes).
    pub password: String<64>,
    /// User/provisioning priority weight (`0..=255`; higher is preferred).
    pub priority: u8,
}

impl fmt::Debug for KnownApEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KnownApEntry")
            .field("ssid", &self.ssid)
            .field("password", &"[REDACTED]")
            .field("priority", &self.priority)
            .finish()
    }
}

impl KnownApEntry {
    /// Creates a validated [`KnownApEntry`].
    pub fn new(ssid: &str, password: &str, priority: u8) -> Result<Self, StorageError> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(StorageError::InvalidFormat);
        }
        let valid_len = (8..=63).contains(&password.len())
            || (password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()));
        if !password.is_empty() && !valid_len {
            return Err(StorageError::InvalidFormat);
        }
        let mut s = String::new();
        s.push_str(ssid)
            .map_err(|()| StorageError::CapacityExceeded)?;
        let mut p = String::new();
        p.push_str(password)
            .map_err(|()| StorageError::CapacityExceeded)?;
        Ok(Self {
            ssid: s,
            password: p,
            priority,
        })
    }
}

/// 4-slot known-AP credential table with RSSI + priority auto-selection,
/// matching the C `squeezelite-esp32` `aplist` multi-AP design.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KnownApList {
    slots: [Option<KnownApEntry>; KNOWN_AP_SLOTS],
}

impl KnownApList {
    /// Creates an empty 4-slot [`KnownApList`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None, None, None, None],
        }
    }

    /// Number of populated AP slots (`0..=4`).
    #[must_use]
    pub fn len(&self) -> usize {
        let mut count = 0usize;
        for slot in &self.slots {
            if slot.is_some() {
                count += 1;
            }
        }
        count
    }

    /// Returns `true` if all 4 slots are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a reference to slot `index` (`0..4`).
    #[must_use]
    pub fn slot(&self, index: usize) -> Option<&KnownApEntry> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Inserts or updates an AP in the 4-slot list. If all 4 slots are full,
    /// replaces the slot with the lowest `priority`.
    pub fn upsert(
        &mut self,
        ssid: &str,
        password: &str,
        priority: u8,
    ) -> Result<usize, StorageError> {
        let entry = KnownApEntry::new(ssid, password, priority)?;

        // 1. Update in place if SSID already exists.
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if let Some(existing) = slot {
                if existing.ssid.as_str() == ssid {
                    *existing = entry;
                    return Ok(idx);
                }
            }
        }

        // 2. Otherwise use the first free slot.
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                return Ok(idx);
            }
        }

        // 3. Evict the lowest-priority slot.
        let mut evict_idx = 0usize;
        let mut lowest_prio = u8::MAX;
        for (idx, slot) in self.slots.iter().enumerate() {
            if let Some(existing) = slot {
                if existing.priority <= lowest_prio {
                    lowest_prio = existing.priority;
                    evict_idx = idx;
                }
            }
        }
        self.slots[evict_idx] = Some(entry);
        Ok(evict_idx)
    }

    /// Removes a known AP by SSID, returning `true` if an entry was removed.
    pub fn remove(&mut self, ssid: &str) -> bool {
        for slot in &mut self.slots {
            let matches = match slot {
                Some(entry) => entry.ssid.as_str() == ssid,
                None => false,
            };
            if matches {
                *slot = None;
                return true;
            }
        }
        false
    }

    /// Automatically selects the best known AP visible in `scan` by combining
    /// signal strength (`rssi`) and `priority` (`score = rssi + priority / 4`),
    /// returning the winning [`KnownApEntry`] and its measured RSSI.
    #[must_use]
    pub fn auto_pick<'a>(&'a self, scan: &WifiScanTable) -> Option<(&'a KnownApEntry, i8)> {
        let mut best: Option<(&'a KnownApEntry, i8, i32)> = None;
        for slot in &self.slots {
            let Some(known) = slot.as_ref() else {
                continue;
            };
            let Some(scanned) = scan.best_for_ssid(known.ssid.as_str()) else {
                continue;
            };
            let score = i32::from(scanned.rssi) + (i32::from(known.priority) / 4);
            let replace = match best {
                None => true,
                Some((prev_entry, prev_rssi, prev_score)) => {
                    score > prev_score
                        || (score == prev_score && known.priority > prev_entry.priority)
                        || (score == prev_score
                            && known.priority == prev_entry.priority
                            && scanned.rssi > prev_rssi)
                }
            };
            if replace {
                best = Some((known, scanned.rssi, score));
            }
        }
        best.map(|(entry, rssi, _)| (entry, rssi))
    }
}

/// Static IPv4 configuration (`ip`, `gateway`, `netmask`, `dns`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticIpConfig {
    /// Static IPv4 address assigned to the interface.
    pub ip: [u8; 4],
    /// Default IPv4 gateway address.
    pub gateway: [u8; 4],
    /// IPv4 subnet mask.
    pub netmask: [u8; 4],
    /// Primary DNS resolver IPv4 address.
    pub dns: [u8; 4],
}

impl StaticIpConfig {
    /// Validates and constructs a [`StaticIpConfig`].
    pub fn new(
        ip: [u8; 4],
        gateway: [u8; 4],
        netmask: [u8; 4],
        dns: [u8; 4],
    ) -> Result<Self, StorageError> {
        if ip == [0, 0, 0, 0] || ip[0] == 127 || ip[0] >= 224 {
            return Err(StorageError::InvalidFormat);
        }
        let mask_u32 = u32::from_be_bytes(netmask);
        if mask_u32 == 0 || mask_u32 == u32::MAX || (!mask_u32 & (!mask_u32 + 1)) != 0 {
            return Err(StorageError::InvalidFormat);
        }
        Ok(Self {
            ip,
            gateway,
            netmask,
            dns,
        })
    }

    /// Checks whether `target` lies within the configured IPv4 subnet.
    #[must_use]
    pub const fn is_same_subnet(&self, target: [u8; 4]) -> bool {
        let ip_u32 = u32::from_be_bytes(self.ip);
        let target_u32 = u32::from_be_bytes(target);
        let mask_u32 = u32::from_be_bytes(self.netmask);
        (ip_u32 & mask_u32) == (target_u32 & mask_u32)
    }
}

/// Validated IEEE 802.11d regulatory domain / country code configuration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WifiCountryCode {
    /// Two-character uppercase ISO 3166-1 alpha-2 country code.
    pub code: [u8; 2],
    /// First allowed 2.4 GHz channel (`1`).
    pub start_channel: u8,
    /// Number of allowed 2.4 GHz channels (`11` FCC, `13` ETSI, `14` JP).
    pub num_channels: u8,
}

impl WifiCountryCode {
    /// Parses and validates a 2-character country code string (`"US"`, `"EU"`,
    /// `"JP"`, `"GB"`, `"DE"`, `"FR"`, `"CA"`, `"AU"`, `"01"`, etc.).
    pub fn parse(raw: &str) -> Result<Self, StorageError> {
        let trimmed = raw.trim();
        let bytes = trimmed.as_bytes();
        if bytes.len() != 2 {
            return Err(StorageError::InvalidFormat);
        }
        let c0 = bytes[0].to_ascii_uppercase();
        let c1 = bytes[1].to_ascii_uppercase();
        let valid_chars =
            (c0.is_ascii_uppercase() && c1.is_ascii_uppercase()) || (c0 == b'0' && c1 == b'1');
        if !valid_chars {
            return Err(StorageError::InvalidFormat);
        }
        let code = [c0, c1];
        let num_channels = match &code {
            b"US" | b"CA" | b"TW" => 11,
            b"JP" => 14,
            _ => 13,
        };
        Ok(Self {
            code,
            start_channel: 1,
            num_channels,
        })
    }

    /// Returns the 2-character country code as a `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.code).unwrap_or("US")
    }

    /// Returns `true` if `channel` is permitted in this regulatory domain.
    #[must_use]
    pub const fn is_channel_allowed(&self, channel: u8) -> bool {
        channel >= self.start_channel
            && channel < self.start_channel.saturating_add(self.num_channels)
    }
}

/// Decodes a legacy C `squeezelite-esp32` Wi-Fi password (either plaintext or
/// `"enc:"` hex-encoded SHA-256 MAC-salted XOR ciphertext) into a plaintext
/// `heapless::String<64>`.
pub fn decode_legacy_c_password(
    raw_password: &str,
    sta_mac: &[u8; 6],
) -> Result<String<64>, StorageError> {
    decrypt_nvs_wifi_password(raw_password, sta_mac).map_err(|_| StorageError::InvalidFormat)
}

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
    pub credentials: Option<WifiCredentials>,
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
            credentials: None,
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
            mgr.credentials = Some(creds.clone());

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

    /// Initiates a Wi-Fi connection handshake and persists credentials,
    /// transitioning into [`WifiState::Connecting`] until
    /// [`Self::confirm_connected`] or [`Self::on_connected`] is invoked upon
    /// link/DHCP completion (PERIPH-605).
    pub fn begin_connect(&mut self, ssid: &str, password: &str) -> Result<WifiState, StorageError> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(StorageError::InvalidFormat);
        }
        let valid_len = (8..=63).contains(&password.len())
            || (password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()));
        if !password.is_empty() && !valid_len {
            return Err(StorageError::InvalidFormat);
        }

        self.retry_count = 0;

        self.config.ssid.clear();
        self.config
            .ssid
            .push_str(ssid)
            .map_err(|_| StorageError::CapacityExceeded)?;
        self.config.password.clear();
        self.config
            .password
            .push_str(password)
            .map_err(|_| StorageError::CapacityExceeded)?;

        // Persist to storage, preserving static IP settings if present
        // (CODE-CFG-001)
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

        self.credentials = Some(creds);
        self.state = WifiState::Connecting;
        Ok(self.state)
    }

    /// Connects to a Wi-Fi network and persists the credentials to storage.
    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<(), StorageError> {
        self.begin_connect(ssid, password)?;
        self.record_success();
        Ok(())
    }

    /// Returns the exponential backoff delay in milliseconds based on
    /// consecutive failures (APP-002).
    pub fn current_backoff_ms(&self) -> u32 {
        if self.retry_count == 0 {
            return 0;
        }
        let shift = core::cmp::min(self.retry_count - 1, 6);
        let delay = self.base_backoff_ms.saturating_mul(1 << shift);
        core::cmp::min(delay, self.max_backoff_ms)
    }

    /// Returns the reconnect delay in milliseconds (alias for
    /// [`Self::current_backoff_ms`]).
    #[must_use]
    pub fn reconnect_delay_ms(&self) -> u32 {
        self.current_backoff_ms()
    }

    /// Adds deterministic jitter (`jitter_seed % 500`) to reconnect delay.
    #[must_use]
    pub fn reconnect_delay_with_jitter_ms(&self, jitter_seed: u32) -> u32 {
        self.reconnect_delay_ms().saturating_add(jitter_seed % 500)
    }

    /// Deterministic jitter helper for current backoff delay.
    #[must_use]
    pub fn current_backoff_with_jitter_ms(&self, jitter_seed: u32) -> u32 {
        self.reconnect_delay_with_jitter_ms(jitter_seed)
    }

    /// Records a failed connection attempt and increments backoff counter
    /// using saturating arithmetic.
    pub fn record_failure(&mut self) {
        self.state = WifiState::Failed;
        self.retry_count = self.retry_count.saturating_add(1);
    }

    /// Handles an unexpected link disconnection event, transitioning state and
    /// incrementing retry count with saturating arithmetic (PERIPH-611).
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

    /// Callback invoked when station link and IP lease are acquired (PERIPH-605).
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
        for _ in 0..64 {
            let _ = self.config.password.push('\0');
        }
        self.config.password.clear();

        if let Some(mut creds) = self.credentials.take() {
            creds.passphrase.clear();
            for _ in 0..64 {
                let _ = creds.passphrase.push('\0');
            }
            creds.passphrase.clear();
        }
        self.credentials = None;

        self.state = WifiState::Disconnected;
        self.retry_count = 0;
        WifiCredentials::clear(&mut self.store)
    }

    /// Evaluates whether the station should fall back to SoftAP provisioning
    /// mode (either no SSID is configured or `retry_count >= max_retries`).
    #[must_use]
    pub fn should_enter_softap_fallback(&self, max_retries: u32) -> bool {
        if self.state == WifiState::Connected {
            return false;
        }
        self.config.ssid.is_empty() || self.retry_count >= max_retries
    }

    /// Returns the active [`SoftApMode`] based on current connection state and
    /// `max_retries` threshold.
    #[must_use]
    pub fn evaluate_softap_mode(&self, max_retries: u32) -> SoftApMode {
        if self.should_enter_softap_fallback(max_retries) {
            SoftApMode::ActiveFallback
        } else {
            SoftApMode::Inactive
        }
    }

    /// Initiates a Wi-Fi connection accepting either a plaintext passphrase or
    /// a legacy C `"enc:"` hex-encoded XOR passphrase decrypted with `sta_mac`.
    pub fn begin_connect_maybe_encrypted(
        &mut self,
        ssid: &str,
        raw_password: &str,
        sta_mac: &[u8; 6],
    ) -> Result<WifiState, StorageError> {
        let decrypted = decode_legacy_c_password(raw_password, sta_mac)?;
        self.begin_connect(ssid, decrypted.as_str())
    }

    /// Persists a validated [`StaticIpConfig`] into the underlying credential
    /// store alongside the active Wi-Fi credentials.
    pub fn apply_static_ip(&mut self, static_cfg: StaticIpConfig) -> Result<(), StorageError> {
        let mut creds = match WifiCredentials::load_from(&self.store)? {
            Some(existing) => existing,
            None => WifiCredentials::new(self.config.ssid.as_str(), self.config.password.as_str())?,
        };
        creds.static_ip = Some(static_cfg.ip);
        creds.gateway = Some(static_cfg.gateway);
        creds.subnet = Some(static_cfg.netmask);
        creds.dns = Some(static_cfg.dns);
        creds.save_to(&mut self.store)
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
        assert_eq!(
            wifi.connect("", "ValidPass123"),
            Err(StorageError::InvalidFormat)
        );

        // SSID > 32 bytes fails
        let long_ssid = "This_SSID_Is_Way_Too_Long_Because_It_Exceeds_32_Bytes!";
        assert_eq!(
            wifi.connect(long_ssid, "ValidPass123"),
            Err(StorageError::InvalidFormat)
        );

        // Short WPA2 passphrase (1..=7 bytes) fails
        assert_eq!(
            wifi.connect("ValidSSID", "short"),
            Err(StorageError::InvalidFormat)
        );
        assert_eq!(
            wifi.connect("ValidSSID", "1234567"),
            Err(StorageError::InvalidFormat)
        );

        // Long WPA2 passphrase (> 63 bytes and not 64-hex raw PSK) fails
        let long_pass = "123456789012345678901234567890123456789012345678901234567890123z";
        assert_eq!(
            wifi.connect("ValidSSID", long_pass),
            Err(StorageError::InvalidFormat)
        );

        // 64-byte valid ASCII hex raw PSK succeeds
        let hex_psk = "1234567890123456789012345678901234567890123456789012345678901234";
        assert!(wifi.connect("ValidSSID", hex_psk).is_ok());

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
        assert_eq!(wifi.reconnect_delay_with_jitter_ms(250), 250);
        assert_eq!(wifi.reconnect_delay_with_jitter_ms(750), 250);

        let ap_cfg = SoftApConfig::default().with_passphrase(b"12345678");
        assert_eq!(ap_cfg.ap_passphrase_len, 8);
        assert!(ap_cfg.ap_passphrase.is_some());
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

        // Verify stored credentials contain new SSID/pass but preserved IP
        let stored = WifiCredentials::load_from(wifi.store())
            .unwrap()
            .expect("creds present");
        assert_eq!(stored.ssid.as_str(), "NewSSID");
        assert_eq!(stored.passphrase.as_str(), "NewPassword456");
        assert_eq!(stored.static_ip, Some([192, 168, 1, 150]));
        assert_eq!(stored.gateway, Some([192, 168, 1, 1]));
        assert_eq!(stored.subnet, Some([255, 255, 255, 0]));
        assert_eq!(stored.dns, Some([1, 1, 1, 1]));
    }

    #[test]
    fn test_captive_portal_dns_a_record_hijack_and_probe_detection() {
        let dns = CaptivePortalDns::new([192, 168, 4, 1]);
        // Query for "connectivitycheck.gstatic.com" (QTYPE=A=1, QCLASS=IN=1)
        let query: [u8; 47] = [
            0x12, 0x34, // ID
            0x01, 0x00, // Flags: standard query, RD=1
            0x00, 0x01, // QDCOUNT = 1
            0x00, 0x00, // ANCOUNT = 0
            0x00, 0x00, // NSCOUNT = 0
            0x00, 0x00, // ARCOUNT = 0
            // QNAME: 17 "connectivitycheck" 7 "gstatic" 3 "com" 0
            17, b'c', b'o', b'n', b'n', b'e', b'c', b't', b'i', b'v', b'i', b't', b'y', b'c', b'h',
            b'e', b'c', b'k', 7, b'g', b's', b't', b'a', b't', b'i', b'c', 3, b'c', b'o', b'm',
            0, // root label
            0x00, 0x01, // QTYPE = A
            0x00, 0x01, // QCLASS = IN
        ];
        let mut resp = [0u8; 128];
        let len = dns
            .build_response(&query, &mut resp)
            .expect("valid dns hijack");
        assert_eq!(len, query.len() + 16);
        assert_eq!(&resp[0..2], &[0x12, 0x34]);
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1); // ANCOUNT = 1
        assert_eq!(&resp[len - 4..len], &[192, 168, 4, 1]);

        // OS captive-portal HTTP probe detection
        assert_eq!(
            detect_captive_portal_probe("/generate_204"),
            Some(CaptivePortalProbe::AndroidGenerate204)
        );
        assert_eq!(
            detect_captive_portal_probe("/hotspot-detect.html"),
            Some(CaptivePortalProbe::AppleHotspotDetect)
        );
        assert_eq!(
            detect_captive_portal_probe("/ncsi.txt"),
            Some(CaptivePortalProbe::WindowsNcsi)
        );
        assert_eq!(
            detect_captive_portal_probe("/connecttest.txt"),
            Some(CaptivePortalProbe::WindowsNcsi)
        );
        assert_eq!(detect_captive_portal_probe("/status.json"), None);

        let mut redir_buf = [0u8; 256];
        let rlen =
            build_captive_portal_redirect([192, 168, 4, 1], &mut redir_buf).expect("redirect");
        let rstr = core::str::from_utf8(&redir_buf[..rlen]).expect("utf8");
        assert!(rstr.starts_with("HTTP/1.1 302 Found\r\n"));
        assert!(rstr.contains("Location: http://192.168.4.1/\r\n"));
    }

    #[test]
    fn test_net_502_dns_opcode_and_compression_pointer() {
        let dns = CaptivePortalDns::new([10, 0, 0, 1]);
        // 1. Non-standard opcode (opcode = 1 (IQUERY) -> flags = 0x0800) must
        // be rejected.
        let invalid_opcode_query: [u8; 17] = [
            0xAB, 0xCD, // ID
            0x08, 0x00, // Flags: Opcode = 1
            0x00, 0x01, // QDCOUNT = 1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // AN/NS/AR
            0x00, // root label
            0x00, 0x01, // QTYPE = A
            0x00, 0x01, // QCLASS = IN
        ];
        let mut resp = [0u8; 128];
        assert_eq!(dns.build_response(&invalid_opcode_query, &mut resp), None);

        // 2. Query with 2-byte DNS compression pointer (0xC00C) in QNAME
        // must be rejected (AUD03-SEC-001).
        let compressed_query: [u8; 18] = [
            0x11, 0x22, // ID
            0x01, 0x00, // Flags: standard query, RD=1
            0x00, 0x01, // QDCOUNT = 1
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // AN/NS/AR
            0xC0, 0x0C, // 2-byte compression pointer
            0x00, 0x01, // QTYPE = A
            0x00, 0x01, // QCLASS = IN
        ];
        assert_eq!(dns.build_response(&compressed_query, &mut resp), None);
        assert_eq!(dns.handle_query(&compressed_query, &mut resp), None);
    }

    #[test]
    fn test_scan_table_and_4slot_known_ap_auto_pick() {
        let mut scan = WifiScanTable::new();
        scan.upsert(ScannedAp::new("Guest", [1; 6], -75, 1, WifiAuthMode::Open).expect("valid ap"));
        scan.upsert(
            ScannedAp::new("IoT_Basement", [2; 6], -52, 6, WifiAuthMode::Wpa2Psk)
                .expect("valid ap"),
        );
        scan.upsert(
            ScannedAp::new("Attic_Mesh", [3; 6], -60, 11, WifiAuthMode::Wpa2Psk).expect("valid ap"),
        );
        // Stronger duplicate updates existing SSID
        scan.upsert(ScannedAp::new("Guest", [1; 6], -68, 1, WifiAuthMode::Open).expect("valid ap"));

        assert_eq!(scan.len(), 3);
        assert_eq!(scan.entries()[0].ssid.as_str(), "IoT_Basement");
        assert_eq!(scan.entries()[1].ssid.as_str(), "Attic_Mesh");
        assert_eq!(scan.entries()[2].ssid.as_str(), "Guest");
        assert_eq!(scan.entries()[2].rssi, -68);

        let mut known = KnownApList::new();
        known
            .upsert("Attic_Mesh", "AtticPass123", 10)
            .expect("slot 0");
        known
            .upsert("IoT_Basement", "BasementPass123", 20)
            .expect("slot 1");
        known
            .upsert("Office_Net", "OfficePass123", 5)
            .expect("slot 2");
        known
            .upsert("Backup_Net", "BackupPass123", 1)
            .expect("slot 3");
        assert_eq!(known.len(), 4);

        // 5th insert evicts lowest-priority slot ("Backup_Net" with priority 1)
        known
            .upsert("HighPrio_Net", "HighPrioPass123", 50)
            .expect("evict lowest");
        assert_eq!(known.len(), 4);
        assert!((0..4).all(|i| known.slot(i).unwrap().ssid.as_str() != "Backup_Net"));

        let (picked, rssi) = known.auto_pick(&scan).expect("auto_pick found match");
        assert_eq!(picked.ssid.as_str(), "IoT_Basement");
        assert_eq!(rssi, -52);
    }

    #[test]
    fn test_static_ip_country_code_and_legacy_c_enc_password() {
        let static_ip = StaticIpConfig::new(
            [172, 22, 1, 15],
            [172, 22, 1, 1],
            [255, 255, 255, 0],
            [1, 1, 1, 1],
        )
        .expect("valid static ip");
        assert!(static_ip.is_same_subnet([172, 22, 1, 200]));
        assert!(!static_ip.is_same_subnet([172, 22, 2, 1]));
        assert!(
            StaticIpConfig::new([0, 0, 0, 0], [1, 1, 1, 1], [255, 255, 255, 0], [8, 8, 8, 8])
                .is_err()
        );

        let us = WifiCountryCode::parse("US").expect("US");
        assert_eq!(us.as_str(), "US");
        assert!(us.is_channel_allowed(11));
        assert!(!us.is_channel_allowed(12));

        let eu = WifiCountryCode::parse("eu").expect("EU");
        assert_eq!(eu.as_str(), "EU");
        assert!(eu.is_channel_allowed(13));
        assert!(!eu.is_channel_allowed(14));

        let jp = WifiCountryCode::parse("JP").expect("JP");
        assert!(jp.is_channel_allowed(14));

        // Legacy C "enc:" XOR credential compatibility
        let mac = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34];
        let key = crate::ota_safety::derive_wifi_nvs_key(&mac);
        let plain = "LegacyPass2026";
        let mut enc: String<64> = String::new();
        let _ = enc.push_str("enc:");
        for (i, &b) in plain.as_bytes().iter().enumerate() {
            use core::fmt::Write as _;
            let _ = write!(enc, "{:02x}", b ^ key[i % 16]);
        }

        let mut wifi = WifiManager::new();
        assert!(wifi.should_enter_softap_fallback(3));
        assert_eq!(
            wifi.begin_connect_maybe_encrypted("iot", enc.as_str(), &mac)
                .expect("decrypt & connect"),
            WifiState::Connecting
        );
        assert_eq!(wifi.config.password.as_str(), plain);
        wifi.apply_static_ip(static_ip).expect("apply static ip");
    }
}
