//! Runtime configuration: the in-RAM [`AudioConfig`] behind a lock, debounced
//! persistence, the
//! RAM journal image, and boot-time resolution (spec §3.4–3.5, Task 10).
//!
//! Platform-free by design: no platform or cargo-feature gating here (that
//! lives in
//! `main.rs`), so every rule is exercised by host tests.
//!
//! # Pieces
//!
//! - [`ConfigHandle`]: the in-RAM [`AudioConfig`] + dirty flag, behind an
//!   embassy-sync blocking
//!   mutex. The coordinator (`setd` name), the HTTP server, the CYD bridge and
//!   the LMS volume path
//!   change it through [`ConfigHandle::update`].
//! - [`ConfigWriter`]: the debounce state machine of the device loop's config
//!   poll: a change is journaled once no further change has arrived for
//!   [`DEBOUNCE_MS`].
//! - [`RamJournal`]: the serialized journal image (a DRAM sector buffer) plus
//!   the flash
//!   bookkeeping (target sector per [`FlashTargetPolicy`], sequence) carried in
//!   RAM from the
//!   boot-time load, so a later flash write never has to read flash to find
//!   where to go.
//! - [`resolve_boot_audio`] / [`resolve_boot_wifi`]: the per-field boot
//!   resolution order
//!   (audio/identity: FlashJournal, then first-boot defaults; Wi-Fi in Phase
//!   0/1: the C
//!   firmware's NVS, then the first-boot default).
//!
//! # Why a snapshot can never be torn
//!
//! The whole [`AudioConfig`] lives inside ONE mutex. [`ConfigHandle::update`]
//! runs its closure on
//! a copy and publishes the result while holding the lock;
//! [`ConfigHandle::snapshot_if_dirty`]
//! clones the whole struct while holding the same lock. A snapshot is therefore
//! always the state
//! before or after an update, never a mix. Every published change bumps a
//! generation counter;
//! a snapshot remembers the generation it saw and [`ConfigHandle::mark_clean`]
//! only clears the
//! dirty flag if no update was published since, so a change racing a commit is
//! never lost.

use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use squeezelite_config::{AudioConfig, KeyValueStorage, SectorWriter};

use crate::ota_safety::WifiStaCredentials;

/// Quiet time after the last change before the `config_writer` journals it
/// (spec §3.5).
pub const DEBOUNCE_MS: u32 = 2000;

/// Size of one settings journal sector (one flash erase unit).
pub const JOURNAL_SECTOR_SIZE: usize = 4096;

/// Standard flash sector size alias for journal sector operations.
pub const FLASH_SECTOR_SIZE: usize = JOURNAL_SECTOR_SIZE;

/// Key for maximum sample rate in audio configuration.
pub const KEY_MAXRATE: &str = AudioConfig::KEY_MAXRATE;

/// Key for Wi-Fi passphrase in configuration journal.
pub const KEY_WIFI_PASS: &str = "wifi.pass";

/// Default maximum sample rate (0 = unlimited / native DAC limit).
pub const DEFAULT_MAX_SAMPLE_RATE: u32 = 0;

/// ESP-IDF NVS `U8` type identifier used by the C web UI (`config.json`).
pub const NVS_TYPE_U8: u8 = 1;

/// ESP-IDF NVS `U16` type identifier used by the C web UI (`config.json`).
pub const NVS_TYPE_U16: u8 = 2;

/// ESP-IDF NVS `U32` type identifier used by the C web UI (`config.json`).
pub const NVS_TYPE_U32: u8 = 4;

/// ESP-IDF NVS `STR` type identifier used by the C web UI (`config.json`).
pub const NVS_TYPE_STR: u8 = 33;

/// Returns `true` if `key` is a sensitive credential/passphrase key that must
/// be stripped from unauthenticated `GET /config.json` responses.
#[must_use]
pub fn is_secret_config_key(key: &str) -> bool {
    let k = key.trim();
    [
        "password",
        "pwd",
        "wifi.pass",
        "wifi_pass",
        "ap_pwd",
        "telnet_pwd",
        "a2dp_spin",
        "bt_sink_pin",
        "web_pwd",
        "secret",
    ]
    .iter()
    .any(|s| k.eq_ignore_ascii_case(s))
}

/// Parses a comma-separated list of up to 10 equalizer bands, clamping each to
/// `-15.0..=15.0`. Returns `Some([f32; 10])` if 1..=10 finite bands are parsed
/// (filling any omitted trailing bands with `0.0`), or `None` if `csv` is
/// empty, exceeds 10 bands, or contains non-finite/non-numeric tokens.
#[must_use]
pub fn parse_equalizer_bands(csv: &str) -> Option<[f32; 10]> {
    let trimmed = csv.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut bands = [0.0f32; 10];
    let mut count = 0;
    for part in trimmed.split(',') {
        if count >= 10 {
            return None;
        }
        let p = part.trim();
        if p.is_empty() {
            return None;
        }
        let val: f32 = p.parse().ok()?;
        if !val.is_finite() {
            return None;
        }
        bands[count] = val.clamp(-15.0, 15.0);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(bands)
}

/// Parses an IPv4 dotted-quad string (e.g. `"192.168.1.50"`) into `[u8; 4]`.
#[must_use]
pub fn parse_ipv4_dotted(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in s.trim().split('.') {
        if count >= 4 {
            return None;
        }
        if part.is_empty()
            || part.len() > 3
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        octets[count] = part.parse::<u8>().ok()?;
        count += 1;
    }
    if count == 4 {
        Some(octets)
    } else {
        None
    }
}

/// Maximum byte length of an NVS override value stored in [`NvsEntrySlot`].
pub const MAX_NVS_VALUE_LEN: usize = 128;

/// Maximum number of NVS override slots tracked by [`ConfigHandle`] and
/// [`RamJournal`].
pub const MAX_NVS_OVERRIDE_SLOTS: usize = 32;

/// Parses `<ipv4>[:<port>]`, rejecting unspecified `0.0.0.0`, non-digit
/// ports, and port `0` (valid ports are `1..=65535`).
#[must_use]
pub fn parse_lms_server_endpoint(arg: &str) -> Option<([u8; 4], Option<u16>)> {
    let trimmed = arg.trim();
    let (ip_str, port) = if let Some((ip_part, port_part)) = trimmed.split_once(':') {
        if port_part.is_empty() || !port_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let parsed_port: u16 = port_part.parse().ok()?;
        if parsed_port == 0 {
            return None;
        }
        (ip_part, Some(parsed_port))
    } else {
        (trimmed, None)
    };
    let ip = parse_ipv4_dotted(ip_str)?;
    if ip == [0, 0, 0, 0] {
        None
    } else {
        Some((ip, port))
    }
}

/// Maximum total UTF-8 value bytes held across all slots in
/// [`NvsOverrideTable`].
pub const NVS_OVERRIDE_POOL_BYTES: usize = 1024;

/// Something that durably records a configuration snapshot (device:
/// [`RamJournal`]).
pub trait ConfigJournal {
    /// Records `cfg`; `false` if not recorded (the snapshot stays dirty).
    fn commit(&mut self, cfg: &AudioConfig) -> bool;

    /// Records `cfg` together with `overrides`; defaults to [`Self::commit`].
    fn commit_with_overrides(&mut self, cfg: &AudioConfig, _overrides: &NvsOverrideTable) -> bool {
        self.commit(cfg)
    }
}

/// A slot storing an NVS key-value override.
#[derive(Clone, PartialEq, Eq)]
pub struct NvsEntrySlot {
    pub used: bool,
    pub key: heapless::String<24>,
    pub value: heapless::String<MAX_NVS_VALUE_LEN>,
}

impl core::fmt::Debug for NvsEntrySlot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let display_val = if is_secret_config_key(self.key.as_str()) {
            "<redacted>"
        } else {
            self.value.as_str()
        };
        f.debug_struct("NvsEntrySlot")
            .field("used", &self.used)
            .field("key", &self.key)
            .field("value", &display_val)
            .finish()
    }
}

impl Default for NvsEntrySlot {
    fn default() -> Self {
        Self {
            used: false,
            key: heapless::String::new(),
            value: heapless::String::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NvsCompactSlot {
    key: [u8; 24],
    key_len: u8,
    val_off: u16,
    val_len: u8,
}

impl NvsCompactSlot {
    const fn empty() -> Self {
        Self {
            key: [0u8; 24],
            key_len: 0,
            val_off: 0,
            val_len: 0,
        }
    }
}

/// Compact fixed-capacity NVS key-value override table backed by a shared
/// 1 KiB UTF-8 byte pool.
#[derive(Clone, PartialEq, Eq)]
pub struct NvsOverrideTable {
    slots: [NvsCompactSlot; MAX_NVS_OVERRIDE_SLOTS],
    pool: [u8; NVS_OVERRIDE_POOL_BYTES],
    pool_used: u16,
}

impl Default for NvsOverrideTable {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for NvsOverrideTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut list = f.debug_list();
        for (k, v) in self.iter() {
            let shown = if is_secret_config_key(k) {
                "<redacted>"
            } else {
                v
            };
            list.entry(&(k, shown));
        }
        list.finish()
    }
}

impl NvsOverrideTable {
    /// Creates an empty override table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [NvsCompactSlot::empty(); MAX_NVS_OVERRIDE_SLOTS],
            pool: [0u8; NVS_OVERRIDE_POOL_BYTES],
            pool_used: 0,
        }
    }

    /// Removes all entries and resets pool usage to zero.
    pub fn clear(&mut self) {
        self.slots = [NvsCompactSlot::empty(); MAX_NVS_OVERRIDE_SLOTS];
        self.pool_used = 0;
    }

    /// Inserts or updates an override entry, compacting the pool in place
    /// whenever an existing value changes length.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let k = key.trim();
        let k_bytes = k.as_bytes();
        let v_bytes = value.as_bytes();
        if k_bytes.is_empty() || k_bytes.len() > 24 || v_bytes.len() > MAX_NVS_VALUE_LEN {
            return false;
        }
        let mut existing_idx = None;
        let mut free_idx = None;
        for (idx, slot) in self.slots.iter().enumerate() {
            let k_len = usize::from(slot.key_len);
            if k_len == 0 {
                if free_idx.is_none() {
                    free_idx = Some(idx);
                }
                continue;
            }
            if k_len <= slot.key.len() && &slot.key[..k_len] == k_bytes {
                existing_idx = Some(idx);
                break;
            }
        }
        let pool_used = usize::from(self.pool_used);
        if pool_used > NVS_OVERRIDE_POOL_BYTES {
            return false;
        }
        if let Some(idx) = existing_idx {
            let old_off = usize::from(self.slots[idx].val_off);
            let old_len = usize::from(self.slots[idx].val_len);
            if old_off.saturating_add(old_len) > pool_used {
                return false;
            }
            let new_len = v_bytes.len();
            if new_len == old_len {
                self.pool[old_off..old_off + new_len].copy_from_slice(v_bytes);
                return true;
            }
            let after_remove = pool_used - old_len;
            if after_remove + new_len > NVS_OVERRIDE_POOL_BYTES {
                return false;
            }
            if old_len > 0 {
                let tail = old_off + old_len;
                if tail < pool_used {
                    self.pool.copy_within(tail..pool_used, old_off);
                }
                for (j, other) in self.slots.iter_mut().enumerate() {
                    if j != idx && other.key_len > 0 && usize::from(other.val_off) >= tail {
                        other.val_off = (usize::from(other.val_off) - old_len) as u16;
                    }
                }
            }
            self.pool[after_remove..after_remove + new_len].copy_from_slice(v_bytes);
            self.slots[idx].val_off = after_remove as u16;
            self.slots[idx].val_len = new_len as u8;
            self.pool_used = (after_remove + new_len) as u16;
            return true;
        }
        let Some(idx) = free_idx else {
            return false;
        };
        if pool_used + v_bytes.len() > NVS_OVERRIDE_POOL_BYTES {
            return false;
        }
        self.slots[idx].key = [0u8; 24];
        self.slots[idx].key[..k_bytes.len()].copy_from_slice(k_bytes);
        self.slots[idx].key_len = k_bytes.len() as u8;
        self.slots[idx].val_off = pool_used as u16;
        self.slots[idx].val_len = v_bytes.len() as u8;
        self.pool[pool_used..pool_used + v_bytes.len()].copy_from_slice(v_bytes);
        self.pool_used = (pool_used + v_bytes.len()) as u16;
        true
    }

    fn slot_entry(&self, idx: usize) -> Option<(&str, &str)> {
        let slot = self.slots.get(idx)?;
        if slot.key_len == 0 {
            return None;
        }
        let k_len = usize::from(slot.key_len);
        let v_off = usize::from(slot.val_off);
        let v_len = usize::from(slot.val_len);
        let pool_used = usize::from(self.pool_used).min(NVS_OVERRIDE_POOL_BYTES);
        let v_end = v_off.checked_add(v_len)?;
        if v_end > pool_used {
            return None;
        }
        let k_bytes = slot.key.get(..k_len)?;
        let v_bytes = self.pool.get(v_off..v_end)?;
        let k = core::str::from_utf8(k_bytes).ok()?;
        let v = core::str::from_utf8(v_bytes).ok()?;
        Some((k, v))
    }

    /// Looks up an override value slice by trimmed key.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&str> {
        let target = key.trim();
        for (k, v) in self.iter() {
            if k == target {
                return Some(v);
            }
        }
        None
    }

    /// Iterates over active `(key, value)` slices in slot order.
    #[must_use]
    pub fn iter(&self) -> NvsOverrideIter<'_> {
        NvsOverrideIter {
            table: self,
            index: 0,
        }
    }

    /// Number of value pool bytes currently occupied.
    #[must_use]
    pub fn pool_used(&self) -> usize {
        usize::from(self.pool_used)
    }
}

/// Iterator over active `(key, value)` pairs in [`NvsOverrideTable`].
pub struct NvsOverrideIter<'a> {
    table: &'a NvsOverrideTable,
    index: usize,
}

impl<'a> Iterator for NvsOverrideIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.table.slots.len() {
            let idx = self.index;
            self.index += 1;
            if let Some(entry) = self.table.slot_entry(idx) {
                return Some(entry);
            }
        }
        None
    }
}

struct HandleState {
    cfg: AudioConfig,
    dirty: bool,
    /// Bumped by every published change.
    generation: u32,
    /// Generation seen by the last [`ConfigHandle::snapshot_if_dirty`].
    snapshot_generation: u32,
    nvs_overrides: NvsOverrideTable,
}

/// The runtime configuration: an [`AudioConfig`] plus a dirty flag behind one
/// mutex, with a cached `host_name` for zero-copy inspection.
pub struct ConfigHandle {
    state: Mutex<CriticalSectionRawMutex, RefCell<HandleState>>,
    host_name: heapless::String<32>,
}

impl Default for ConfigHandle {
    fn default() -> Self {
        Self::new(AudioConfig::default())
    }
}

impl ConfigHandle {
    /// A clean handle holding `cfg` (the boot-resolved configuration).
    pub fn new(cfg: AudioConfig) -> Self {
        let host_name = cfg.player_name.clone();
        Self {
            state: Mutex::new(RefCell::new(HandleState {
                cfg,
                dirty: false,
                generation: 0,
                snapshot_generation: 0,
                nvs_overrides: NvsOverrideTable::new(),
            })),
            host_name,
        }
    }

    /// Sets an NVS override key-value pair. Rejects empty keys, keys over 24
    /// bytes, values over [`MAX_NVS_VALUE_LEN`] bytes, or pool exhaustion
    /// without truncation.
    pub fn set_nvs_override(&self, key: &str, value: &str) -> bool {
        self.state.lock(|state| {
            let mut s = state.borrow_mut();
            if !s.nvs_overrides.set(key, value) {
                return false;
            }
            s.dirty = true;
            s.generation = s.generation.wrapping_add(1);
            true
        })
    }

    /// Invokes `f` with a shared reference to the compact NVS override table
    /// without copying it onto the stack.
    pub fn with_nvs_overrides<R>(&self, f: impl FnOnce(&NvsOverrideTable) -> R) -> R {
        self.state.lock(|state| f(&state.borrow().nvs_overrides))
    }

    /// Returns a compact clone of the active NVS override table.
    #[must_use]
    pub fn nvs_override_table(&self) -> NvsOverrideTable {
        self.state
            .lock(|state| state.borrow().nvs_overrides.clone())
    }

    /// Returns the active NVS override slots (host test helper).
    #[cfg(test)]
    #[must_use]
    pub fn nvs_overrides(&self) -> [NvsEntrySlot; MAX_NVS_OVERRIDE_SLOTS] {
        self.state.lock(|state| {
            let s = state.borrow();
            let mut out = <[NvsEntrySlot; MAX_NVS_OVERRIDE_SLOTS]>::default();
            for (idx, (k, v)) in s.nvs_overrides.iter().enumerate() {
                if idx < out.len() {
                    out[idx].used = true;
                    let _ = out[idx].key.push_str(k);
                    let _ = out[idx].value.push_str(v);
                }
            }
            out
        })
    }

    /// Returns the current host / player name string slice.
    #[must_use]
    pub fn host_name_str(&self) -> &str {
        self.host_name.as_str()
    }

    /// Updates both the cached `host_name` and the inner
    /// `AudioConfig::player_name`.
    pub fn set_host_name(&mut self, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        let max_len = trimmed.floor_char_boundary(trimmed.len().min(32));
        let clean = &trimmed[..max_len];
        let mut next_host = heapless::String::new();
        if next_host.push_str(clean).is_err() {
            return false;
        }
        self.host_name = next_host;
        self.update(|c| {
            c.set_player_name(clean);
        })
    }

    /// Looks up an NVS override value by key.
    #[must_use]
    pub fn nvs_override(&self, key: &str) -> Option<heapless::String<MAX_NVS_VALUE_LEN>> {
        self.state.lock(|state| {
            let s = state.borrow();
            let val = s.nvs_overrides.value(key)?;
            let mut out = heapless::String::new();
            let _ = out.push_str(val);
            Some(out)
        })
    }

    /// Looks up an NVS override value by key, returning `"<redacted>"` when
    /// the key or matched secret slot satisfies [`is_secret_config_key`].
    #[must_use]
    pub fn nvs_override_redacted(&self, key: &str) -> Option<heapless::String<MAX_NVS_VALUE_LEN>> {
        let should_redact = if let Some(val) = self.nvs_override(key) {
            if !is_secret_config_key(key) {
                return Some(val);
            }
            true
        } else {
            let k = key.trim();
            self.state.lock(|state| {
                let s = state.borrow();
                let mut matched = false;
                for (slot_key, _) in s.nvs_overrides.iter() {
                    if slot_key.trim().eq_ignore_ascii_case(k) && is_secret_config_key(slot_key) {
                        matched = true;
                        break;
                    }
                }
                matched
            })
        };
        if should_redact {
            let mut redacted = heapless::String::new();
            let _ = redacted.push_str("<redacted>");
            Some(redacted)
        } else {
            None
        }
    }

    /// Returns the active LMS server IPv4 address from typed configuration, if
    /// set.
    #[must_use]
    pub fn lms_server_ip(&self) -> Option<[u8; 4]> {
        self.read(|c| c.lms_server_ip)
    }

    /// Updates the LMS server IPv4 address in typed configuration.
    pub fn set_lms_server(&mut self, ip: [u8; 4]) -> bool {
        self.update(|c| c.lms_server_ip = Some(ip))
    }

    /// Returns the sequence / generation number of this configuration.
    #[must_use]
    pub fn sequence(&self) -> u32 {
        self.state.lock(|s| s.borrow().generation)
    }

    /// Sets the sequence / generation number of this configuration.
    pub fn set_sequence(&self, seq: u32) {
        self.state.lock(|s| {
            let mut state = s.borrow_mut();
            state.generation = seq;
            state.snapshot_generation = seq;
        });
    }

    /// Returns the device / player name.
    #[must_use]
    pub fn device_name(&self) -> heapless::String<32> {
        const _: () = assert!(
            crate::board::first_boot_defaults::PLAYER_NAME.len() <= 32,
            "first_boot_defaults::PLAYER_NAME must fit in heapless::String<32>"
        );
        self.read(|c| {
            if !c.player_name.is_empty() {
                return c.player_name.clone();
            }
            if !self.host_name.is_empty() {
                return self.host_name.clone();
            }
            let def_name = crate::board::first_boot_defaults::PLAYER_NAME;
            let max_len = def_name.floor_char_boundary(def_name.len().min(32));
            let mut fallback = heapless::String::new();
            let _ = fallback.push_str(&def_name[..max_len]);
            fallback
        })
    }

    /// Parses command-line flags from an `autoexec1` command string (such as
    /// `-s <ip[:port]>` or `-n <name>`) and updates `lms_server_ip`,
    /// `lms_server_port`, and `host_name`.
    pub fn apply_autoexec1_flags(&mut self, cmd: &str) {
        let mut iter = cmd.split_whitespace();
        while let Some(tok) = iter.next() {
            if tok == "-s" {
                if let Some(val) = iter.next() {
                    if let Some((ip, port)) = parse_lms_server_endpoint(val) {
                        let _ = self.update(|c| {
                            c.lms_server_ip = Some(ip);
                            if let Some(p) = port {
                                c.lms_server_port = Some(p);
                            }
                        });
                    }
                }
            } else if tok.starts_with("-s") && tok.len() > 2 {
                if let Some((ip, port)) = parse_lms_server_endpoint(&tok[2..]) {
                    let _ = self.update(|c| {
                        c.lms_server_ip = Some(ip);
                        if let Some(p) = port {
                            c.lms_server_port = Some(p);
                        }
                    });
                }
            } else if tok == "-n" {
                if let Some(val) = iter.next() {
                    let trimmed = val.trim_matches(|c| c == '"' || c == '\'');
                    if !trimmed.is_empty() {
                        let _ = self.set_host_name(trimmed);
                    }
                }
            } else if tok.starts_with("-n") && tok.len() > 2 {
                let val = &tok[2..];
                let trimmed = val.trim_matches(|c| c == '"' || c == '\'');
                if !trimmed.is_empty() {
                    let _ = self.set_host_name(trimmed);
                }
            }
        }
    }

    /// Resolves the effective LMS server IPv4 address across typed config,
    /// `lms_server_ip`/`lms_ip` NVS overrides, and `-s` flags in `autoexec1`.
    #[must_use]
    pub fn effective_lms_server_ip(&self) -> Option<[u8; 4]> {
        if let Some(ip) = self.lms_server_ip() {
            if ip != [0, 0, 0, 0] {
                return Some(ip);
            }
        }
        if let Some(val) = self
            .nvs_override("lms_server_ip")
            .or_else(|| self.nvs_override("lms_ip"))
        {
            if let Some((ip, _)) = parse_lms_server_endpoint(val.as_str()) {
                return Some(ip);
            }
        }
        if let Some(cmd) = self.nvs_override("autoexec1") {
            let mut iter = cmd.as_str().split_whitespace();
            while let Some(tok) = iter.next() {
                if tok == "-s" {
                    if let Some(val) = iter.next() {
                        if let Some((ip, _)) = parse_lms_server_endpoint(val) {
                            return Some(ip);
                        }
                    }
                } else if tok.starts_with("-s") && tok.len() > 2 {
                    if let Some((ip, _)) = parse_lms_server_endpoint(&tok[2..]) {
                        return Some(ip);
                    }
                }
            }
        }
        None
    }

    /// Returns the effective Spotify Connect device name.
    #[must_use]
    pub fn effective_cspot_name(&self) -> heapless::String<32> {
        if let Some(val) = self.nvs_override("cspot_name") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                let mut s = heapless::String::new();
                let max_len = trimmed.floor_char_boundary(trimmed.len().min(32));
                let _ = s.push_str(&trimmed[..max_len]);
                return s;
            }
        }
        self.device_name()
    }

    /// Returns the effective Bluetooth device name.
    #[must_use]
    pub fn effective_bt_name(&self) -> heapless::String<32> {
        if let Some(val) = self.nvs_override("bt_name") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                let mut s = heapless::String::new();
                let max_len = trimmed.floor_char_boundary(trimmed.len().min(32));
                let _ = s.push_str(&trimmed[..max_len]);
                return s;
            }
        }
        self.device_name()
    }

    /// Returns whether AirPlay RAOP service is enabled (defaults to true).
    #[must_use]
    pub fn is_airplay_enabled(&self) -> bool {
        !matches!(
            self.nvs_override("enable_airplay").as_deref(),
            Some("0" | "false" | "off" | "no")
        )
    }

    /// Returns whether Bluetooth A2DP Sink service is enabled
    /// (defaults to true).
    #[must_use]
    pub fn is_bt_sink_enabled(&self) -> bool {
        !matches!(
            self.nvs_override("enable_bt_sink").as_deref(),
            Some("0" | "false" | "off" | "no")
        )
    }

    /// Returns whether Spotify Connect (`cspot`) service is enabled
    /// (defaults to true).
    #[must_use]
    pub fn is_cspot_enabled(&self) -> bool {
        !matches!(
            self.nvs_override("enable_cspot").as_deref(),
            Some("0" | "false" | "off" | "no")
        )
    }

    /// Returns whether `autoexec` script execution is enabled
    /// (defaults to true).
    #[must_use]
    pub fn is_autoexec_enabled(&self) -> bool {
        !matches!(
            self.nvs_override("autoexec").as_deref(),
            Some("0" | "false" | "off" | "no")
        )
    }

    /// Creates a mutable working copy of this handle's current state.
    #[must_use]
    pub fn clone_handle(&self) -> Self {
        self.state.lock(|state| {
            let s = state.borrow();
            let host_name = s.cfg.player_name.clone();
            Self {
                state: Mutex::new(RefCell::new(HandleState {
                    cfg: s.cfg.clone(),
                    dirty: s.dirty,
                    generation: s.generation,
                    snapshot_generation: s.snapshot_generation,
                    nvs_overrides: s.nvs_overrides.clone(),
                })),
                host_name,
            }
        })
    }

    /// Synchronizes changes from a working copy back into this shared handle.
    pub fn sync_from(&self, other: &Self) -> bool {
        let (next_cfg, next_overrides) = other.state.lock(|state| {
            let s = state.borrow();
            (s.cfg.clone(), s.nvs_overrides.clone())
        });
        self.state.lock(|state| {
            let mut s = state.borrow_mut();
            s.cfg = next_cfg;
            s.nvs_overrides = next_overrides;
            s.dirty = true;
            s.generation = s.generation.wrapping_add(1);
        });
        true
    }

    /// Applies a string key-value configuration mutation to the handle.
    pub fn apply_key_value(&mut self, key: &str, value: &str) -> bool {
        let k = key.trim();
        let v = value.trim();
        if k.is_empty() {
            return false;
        }
        match k {
            "host_name" | "player_name" | "name" => {
                if v.is_empty() || !self.set_nvs_override("host_name", v) {
                    return false;
                }
                self.set_host_name(v)
                    || self.host_name_str() == self.read(|c| c.player_name.clone()).as_str()
            }
            "lms_ip" | "lms_server_ip" => {
                let Some((octets, port)) = parse_lms_server_endpoint(v) else {
                    return false;
                };
                let mut ip_buf = heapless::String::<16>::new();
                let _ = core::fmt::write(
                    &mut ip_buf,
                    format_args!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]),
                );
                let preserve_server_ip_port = k == "lms_ip"
                    && port.is_none()
                    && matches!(
                        self.nvs_override("lms_server_ip")
                            .and_then(|cur| parse_lms_server_endpoint(cur.as_str())),
                        Some((cur_ip, Some(_))) if cur_ip == octets
                    );
                if (!preserve_server_ip_port && !self.set_nvs_override("lms_server_ip", v))
                    || !self.set_nvs_override("lms_ip", ip_buf.as_str())
                {
                    return false;
                }
                let _ = self.update(|c| {
                    c.lms_server_ip = Some(octets);
                    if let Some(p) = port {
                        c.lms_server_port = Some(p);
                    } else if k == "lms_server_ip" {
                        c.lms_server_port = None;
                    }
                });
                true
            }
            "autoexec1" => {
                if !self.set_nvs_override("autoexec1", v) {
                    return false;
                }
                self.apply_autoexec1_flags(v);
                true
            }
            "equalizer" => {
                if !self.set_nvs_override("equalizer", v) {
                    return false;
                }
                if let Some(bands) = parse_equalizer_bands(v) {
                    if bands.iter().all(|&b| b == 0.0) {
                        let _ = self.update(|c| c.equalizer_preset = 0);
                    }
                }
                true
            }
            "vol" | "vol_pct" | "volume" => {
                if let Ok(pct) = v.parse::<u8>() {
                    let clamped = pct.min(100);
                    let _ = self.update(|c| c.volume = clamped);
                    true
                } else {
                    false
                }
            }
            "eq" | "eq_preset" | "equalizer_preset" => {
                let preset = if let Ok(id) = v.parse::<u8>() {
                    Some(id.min(4))
                } else if v.eq_ignore_ascii_case("flat") {
                    Some(0)
                } else if v.eq_ignore_ascii_case("bass") {
                    Some(1)
                } else if v.eq_ignore_ascii_case("loudness") {
                    Some(2)
                } else if v.eq_ignore_ascii_case("vocal") {
                    Some(3)
                } else if v.eq_ignore_ascii_case("rock") {
                    Some(4)
                } else {
                    None
                };
                if let Some(p) = preset {
                    let _ = self.update(|c| c.equalizer_preset = p);
                    true
                } else {
                    false
                }
            }
            "again" | "again_code" | "analog_gain" => {
                let parsed =
                    if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                        u8::from_str_radix(hex, 16).ok()
                    } else {
                        v.parse::<u8>().ok()
                    };
                if let Some(code) = parsed {
                    let clamped = code.min(AudioConfig::MAX_ANALOG_GAIN_CODE);
                    let _ = self.update(|c| c.analog_gain_code = clamped);
                    true
                } else {
                    false
                }
            }
            "loud" | "loudness" | "loudness_enabled" => {
                let enabled = matches!(v, "1" | "true" | "TRUE" | "on" | "ON");
                let _ = self.update(|c| c.loudness_enabled = enabled);
                true
            }
            "maxrate" | "max_sample_rate" => {
                if let Ok(rate) = v.parse::<u32>() {
                    let _ = self.update(|c| c.set_max_sample_rate(rate));
                    true
                } else {
                    false
                }
            }
            "lms_port" => {
                if let Ok(port @ 1..) = v.parse::<u16>() {
                    let _ = self.update(|c| c.lms_server_port = Some(port));
                    true
                } else {
                    false
                }
            }
            _ => self.set_nvs_override(k, v),
        }
    }

    /// Flushes any pending changes and syncs NVS overrides to `journal`.
    pub fn force_flush(&self, journal: &mut impl ConfigJournal) -> bool {
        self.flush_now(journal)
    }

    /// Syncs NVS overrides and polls `writer` for debounced commit.
    pub fn flush_if_due(
        &self,
        writer: &mut ConfigWriter,
        journal: &mut impl ConfigJournal,
        now_ms: u64,
    ) -> bool {
        writer.poll(self, journal, now_ms)
    }

    /// Applies `f` to the configuration. The result is published only if it is
    /// valid
    /// ([`AudioConfig::validate`]) and differs from the current state; then the
    /// handle becomes
    /// dirty. Returns whether a change was published.
    pub fn update(&self, f: impl FnOnce(&mut AudioConfig)) -> bool {
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            let mut next = state.cfg.clone();
            f(&mut next);
            if next == state.cfg || next.validate().is_err() {
                return false;
            }
            state.cfg = next;
            state.dirty = true;
            state.generation = state.generation.wrapping_add(1);
            true
        })
    }

    /// A copy of the whole configuration if it changed since the last
    /// [`Self::mark_clean`].
    pub fn snapshot_if_dirty(&self) -> Option<AudioConfig> {
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            if !state.dirty {
                return None;
            }
            state.snapshot_generation = state.generation;
            Some(state.cfg.clone())
        })
    }

    /// A copy of the whole configuration and NVS overrides if changed since the
    /// last [`Self::mark_clean`].
    pub fn snapshot_if_dirty_with_overrides(&self) -> Option<(AudioConfig, NvsOverrideTable)> {
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            if !state.dirty {
                return None;
            }
            state.snapshot_generation = state.generation;
            Some((state.cfg.clone(), state.nvs_overrides.clone()))
        })
    }

    /// Marks the last [`Self::snapshot_if_dirty`] as durably recorded. A change
    /// published after
    /// that snapshot keeps the handle dirty.
    pub fn mark_clean(&self) {
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            if state.generation == state.snapshot_generation {
                state.dirty = false;
            }
        })
    }

    /// Journals any pending change now (called before every reboot action):
    /// snapshot, commit,
    /// mark clean. Returns whether a snapshot was committed.
    pub fn flush_now(&self, journal: &mut impl ConfigJournal) -> bool {
        // The commit runs OUTSIDE the lock: it may be slow (serialization;
        // with `runtime-flash-persist`, a flash write follows), and `update`
        // must never wait on it.
        let Some((snapshot, overrides)) = self.snapshot_if_dirty_with_overrides() else {
            return false;
        };
        if journal.commit_with_overrides(&snapshot, &overrides) {
            self.mark_clean();
            true
        } else {
            false
        }
    }

    /// `true` while a change awaits journaling.
    pub fn is_dirty(&self) -> bool {
        self.state.lock(|state| state.borrow().dirty)
    }

    /// Change counter (bumped by every published update).
    pub fn generation(&self) -> u32 {
        self.state.lock(|state| state.borrow().generation)
    }

    /// Reads the configuration under the lock without copying all of it.
    pub fn read<R>(&self, f: impl FnOnce(&AudioConfig) -> R) -> R {
        self.state.lock(|state| f(&state.borrow().cfg))
    }

    /// Exports the configuration as a compact JSON string.
    pub fn export_json(&self) -> Result<heapless::String<1024>, squeezelite_config::StorageError> {
        let mut json = heapless::String::<1024>::new();
        self.read(|cfg| {
            use core::fmt::Write;
            write!(
                &mut json,
                "{{\"player_name\":\"{}\",\"volume\":{},\"equalizer_preset\":{},\
\"analog_gain\":{},\"loudness\":{},\"max_sample_rate\":{}}}",
                cfg.player_name.as_str(),
                cfg.volume,
                cfg.equalizer_preset,
                cfg.analog_gain_code,
                cfg.loudness_enabled,
                cfg.max_sample_rate
            )
            .map_err(|_| squeezelite_config::StorageError::CapacityExceeded)
        })?;
        Ok(json)
    }

    /// Exports the configuration as JSON with `KEY_WIFI_PASS` redacted to
    /// `"***"`.
    pub fn export_json_redacted(
        &self,
    ) -> Result<heapless::String<1024>, squeezelite_config::StorageError> {
        let mut json = heapless::String::<1024>::new();
        self.read(|cfg| {
            use core::fmt::Write;
            write!(
                &mut json,
                "{{\"player_name\":\"{}\",\"volume\":{},\"equalizer_preset\":{},\
\"analog_gain\":{},\"loudness\":{},\"max_sample_rate\":{},\"{}\":\"***\"}}",
                cfg.player_name.as_str(),
                cfg.volume,
                cfg.equalizer_preset,
                cfg.analog_gain_code,
                cfg.loudness_enabled,
                cfg.max_sample_rate,
                KEY_WIFI_PASS
            )
            .map_err(|_| squeezelite_config::StorageError::CapacityExceeded)
        })?;
        Ok(json)
    }
}

/// Debounce state of the device loop's 100 ms config poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConfigWriter {
    seen_generation: u32,
    last_change_ms: u64,
}

impl ConfigWriter {
    /// A writer that has seen `handle`'s current generation at `now_ms`.
    pub fn new(handle: &ConfigHandle, now_ms: u64) -> Self {
        Self {
            seen_generation: handle.generation(),
            last_change_ms: now_ms,
        }
    }

    /// One `config_writer` step at `now_ms`: notes a new change, and once the
    /// handle has been
    /// dirty and quiet for [`DEBOUNCE_MS`], journals the snapshot
    /// ([`ConfigHandle::flush_now`]).
    /// Returns whether it committed.
    pub fn poll(
        &mut self,
        handle: &ConfigHandle,
        journal: &mut impl ConfigJournal,
        now_ms: u64,
    ) -> bool {
        let generation = handle.generation();
        if generation != self.seen_generation {
            // A new change restarts the quiet period.
            self.seen_generation = generation;
            self.last_change_ms = now_ms;
        }
        if !handle.is_dirty() || now_ms.saturating_sub(self.last_change_ms) < u64::from(DEBOUNCE_MS)
        {
            return false;
        }
        let committed = handle.flush_now(journal);
        if !committed {
            // Never retry a refused commit in a tight loop: wait another quiet
            // period.
            self.last_change_ms = now_ms;
        }
        committed
    }
}

/// Alias for [`ConfigWriter`].
pub type DebouncedConfigWriter = ConfigWriter;

/// Alias for [`RamJournal`].
pub type FlashNvsJournal<'a> = RamJournal<'a>;

/// Target for restoring configuration and NVS entries from a journal sector.
pub trait RestoreSectorTarget {
    /// Clears any existing entries before restoration.
    fn clear_entries(&mut self) {}

    /// Applies a restored key-value entry.
    fn apply_entry(&mut self, key: &str, val: &str);

    /// Finalizes restoration with the sector sequence counter.
    fn finalize_restore(&mut self, _sequence: u32) {}
}

impl RestoreSectorTarget for NvsOverrideTable {
    fn clear_entries(&mut self) {
        self.clear();
    }

    fn apply_entry(&mut self, key: &str, val: &str) {
        let _ = self.set(key, val);
    }
}

impl RestoreSectorTarget for [NvsEntrySlot; MAX_NVS_OVERRIDE_SLOTS] {
    fn clear_entries(&mut self) {
        for slot in self.iter_mut() {
            *slot = NvsEntrySlot::default();
        }
    }

    fn apply_entry(&mut self, key: &str, val: &str) {
        for slot in self.iter_mut() {
            if !slot.used {
                slot.used = true;
                slot.key.clear();
                let _ = slot.key.push_str(key);
                slot.value.clear();
                let _ = slot.value.push_str(val);
                break;
            }
        }
    }
}

impl RestoreSectorTarget for ConfigHandle {
    fn apply_entry(&mut self, key: &str, val: &str) {
        let _ = self.apply_key_value(key, val);
    }

    fn finalize_restore(&mut self, sequence: u32) {
        if let Some(cmd) = self.nvs_override("autoexec1") {
            self.apply_autoexec1_flags(cmd.as_str());
        }
        if let Some(ip_str) = self
            .nvs_override("lms_server_ip")
            .or_else(|| self.nvs_override("lms_ip"))
        {
            if let Some((ip, port)) = parse_lms_server_endpoint(ip_str.as_str()) {
                let _ = self.update(|c| {
                    c.lms_server_ip = Some(ip);
                    if let Some(p) = port {
                        c.lms_server_port = Some(p);
                    }
                });
            }
        }
        if let Some(host) = self.nvs_override("host_name") {
            let _ = self.set_host_name(host.as_str());
        }
        self.set_sequence(sequence);
    }
}

/// A word-aligned journal sector buffer (flash writes go straight from it, no
/// bounce copy).
#[repr(C, align(4))]
pub struct JournalSector(pub [u8; JOURNAL_SECTOR_SIZE]);

impl JournalSector {
    /// An erased (all `0xFF`) sector.
    pub const fn erased() -> Self {
        Self([0xFF; JOURNAL_SECTOR_SIZE])
    }
}

/// One of the two ping-pong journal sectors of the settings partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalSlot {
    /// First sector (`base`).
    A,
    /// Second sector (`base + 4096`).
    B,
}

impl JournalSlot {
    /// The other sector.
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// Flash offset of this sector for a journal starting at `base`.
    pub const fn offset(self, base: u32) -> u32 {
        match self {
            Self::A => base,
            Self::B => base + JOURNAL_SECTOR_SIZE as u32,
        }
    }

    /// `0` → [`Self::A`], anything else → [`Self::B`]
    /// (`FlashJournal::select_dual_sector` index).
    pub const fn from_index(index: usize) -> Self {
        if index == 0 {
            Self::A
        } else {
            Self::B
        }
    }
}

/// A journal image waiting for its flash write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingImage<'a> {
    /// Sector to erase and write (never the newest valid journal).
    pub slot: JournalSlot,
    /// The whole sector (image followed by `0xFF`).
    pub sector: &'a [u8; JOURNAL_SECTOR_SIZE],
    /// Image length rounded up to a 4-byte word (needed for NOR writes).
    pub used_len: usize,
    /// Sequence number of the image.
    pub sequence: u32,
}

/// Which journal sector a flash write targets. Chosen by the firmware build
/// (the cfg lives in
/// `main.rs`) and passed in at runtime, so this module stays platform-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashTargetPolicy {
    /// Always sector A (0x3FE000): byte-identical to the certified pre-reboot
    /// write. The default
    /// build: 0x3FF000 is page 15 of the C firmware's live `settings` NVS
    /// partition, which the
    /// certified build never touched (a failed C NVS init erases the whole
    /// partition,
    /// `components/platform_config/nvs_utilities.c:97-100`).
    FixedA,
    /// Dual-sector ping-pong (`runtime-flash-persist` builds only): the target
    /// is never the sector holding the newest valid journal, so power loss
    /// mid-write keeps the previous one.
    PingPong,
}

/// The serialized settings journal kept in RAM, plus the flash bookkeeping.
///
/// Built from the boot-time flash load ([`Self::new`]): the sector holding the
/// newest valid
/// journal (boot reads and validates BOTH sectors) and its sequence. Every
/// commit serializes the
/// audio/identity configuration into the RAM image with the next sequence
/// number; the flash
/// target follows the [`FlashTargetPolicy`]. Journal `wifi.*` keys are never
/// written (Phase 0/1:
/// Wi-Fi provisioning belongs to the C recovery firmware), so a commit drops
/// any the boot
/// journal held. Flash is never read after boot.
pub struct RamJournal<'a> {
    image: &'a mut JournalSector,
    image_len: usize,
    sequence: u32,
    target: JournalSlot,
    policy: FlashTargetPolicy,
    flash_pending: bool,
    boot_valid: bool,
    pending_wifi: Option<WifiStaCredentials>,
}

impl<'a> RamJournal<'a> {
    /// `boot`: the sector holding the newest valid journal and its sequence
    /// (`None` when flash held none). `policy`: where flash writes go.
    pub fn new(
        image: &'a mut JournalSector,
        boot: Option<(JournalSlot, u32)>,
        policy: FlashTargetPolicy,
    ) -> Self {
        let sequence = boot.map_or(0, |(_, sequence)| sequence);
        let target = match (policy, boot) {
            (FlashTargetPolicy::FixedA, _) | (FlashTargetPolicy::PingPong, None) => JournalSlot::A,
            (FlashTargetPolicy::PingPong, Some((active, _))) => active.other(),
        };
        let boot_valid = boot.is_some() && Self::validate_nvs2_header(&image.0).is_some();
        Self {
            image,
            image_len: 0,
            sequence,
            target,
            policy,
            flash_pending: false,
            boot_valid,
            pending_wifi: None,
        }
    }

    fn validate_nvs2_header(sector: &[u8; JOURNAL_SECTOR_SIZE]) -> Option<u32> {
        let sequence =
            if sector.len() >= 20 && sector[16..20] == squeezelite_config::FlashJournal::MAGIC {
                match squeezelite_config::FlashJournal::validate_sector(sector) {
                    Ok((seq, _, _)) => seq,
                    Err(_) => return None,
                }
            } else if sector.len() >= 4 && sector[0..4] == squeezelite_config::FlashJournal::MAGIC {
                if sector[12..16] != [0xFF; 4] && sector[12..16] != [0; 4] {
                    match squeezelite_config::FlashJournal::validate_sector(sector) {
                        Ok((seq, _, _)) => seq,
                        Err(_) => return None,
                    }
                } else {
                    0u32
                }
            } else {
                return None;
            };
        if sector.len() < 2058 || &sector[2048..2052] != b"NVS2" {
            return None;
        }
        Some(sequence)
    }

    fn parse_nvs2_entries<T: RestoreSectorTarget>(
        sector: &[u8; JOURNAL_SECTOR_SIZE],
        target: &mut T,
    ) {
        let count = sector[2057] as usize;
        let mut offset = 2058;
        for _ in 0..count {
            if offset >= 4096 {
                break;
            }
            let k_len = sector[offset] as usize;
            offset += 1;
            if offset + k_len > 4096 {
                break;
            }
            let k_str = core::str::from_utf8(&sector[offset..offset + k_len]).unwrap_or("");
            offset += k_len;
            if offset >= 4096 {
                break;
            }
            let v_len = sector[offset] as usize;
            offset += 1;
            if offset + v_len > 4096 {
                break;
            }
            let v_str = core::str::from_utf8(&sector[offset..offset + v_len]).unwrap_or("");
            offset += v_len;

            if !k_str.is_empty() {
                target.apply_entry(k_str, v_str);
            }
        }
    }

    /// Restores NVS overrides from a sector into a target implementing
    /// [`RestoreSectorTarget`].
    pub fn restore_sector_into<T: RestoreSectorTarget>(
        sector: &[u8; JOURNAL_SECTOR_SIZE],
        target: &mut T,
    ) -> bool {
        let Some(sequence) = Self::validate_nvs2_header(sector) else {
            return false;
        };
        target.clear_entries();
        Self::parse_nvs2_entries(sector, target);
        target.finalize_restore(sequence);
        true
    }

    /// Restores NVS overrides and LMS IP from a journal sector if magic
    /// `b"NVS2"` is present.
    pub fn restore_from_sector(&mut self, sector: &[u8; JOURNAL_SECTOR_SIZE]) -> bool {
        if Self::validate_nvs2_header(sector).is_none() {
            return false;
        }
        self.image.0[2048..4096].copy_from_slice(&sector[2048..4096]);
        self.boot_valid = true;
        true
    }

    /// Restores journal's NVS overrides into `handle`, synchronizing
    /// key-values, autoexec flags, LMS IP, host name, and sequence counter.
    pub fn restore_into_handle(&self, handle: &mut ConfigHandle) {
        if self.boot_valid && self.image.0.len() >= 2058 && &self.image.0[2048..2052] == b"NVS2" {
            Self::parse_nvs2_entries(&self.image.0, handle);
        }
        RestoreSectorTarget::finalize_restore(handle, self.sequence);
    }

    /// Parses NVS overrides currently stored in the RAM sector image on demand.
    #[cfg(test)]
    #[must_use]
    pub fn parsed_nvs_overrides(&self) -> NvsOverrideTable {
        let mut table = NvsOverrideTable::new();
        if self.image.0.len() >= 2058 && &self.image.0[2048..2052] == b"NVS2" {
            Self::parse_nvs2_entries(&self.image.0, &mut table);
        }
        table
    }

    /// Serializes `cfg` and writes `nvs_overrides` at `sector[2048..4096]`
    /// with magic `b"NVS2"`.
    pub fn serialize_into(
        nvs_overrides: &NvsOverrideTable,
        sector: &mut [u8; JOURNAL_SECTOR_SIZE],
        cfg: &AudioConfig,
        sequence: u32,
        wifi: Option<(&str, &str)>,
    ) -> Result<usize, squeezelite_config::StorageError> {
        Self::serialize_into_opt(Some(nvs_overrides), sector, cfg, sequence, wifi)
    }

    fn serialize_into_opt(
        nvs_overrides: Option<&NvsOverrideTable>,
        sector: &mut [u8; JOURNAL_SECTOR_SIZE],
        cfg: &AudioConfig,
        sequence: u32,
        wifi: Option<(&str, &str)>,
    ) -> Result<usize, squeezelite_config::StorageError> {
        let len = serialize_image(sector, cfg, sequence, wifi)?;
        let nvs_slice = &mut sector[2048..4096];
        nvs_slice.fill(0xFF);
        nvs_slice[0..4].copy_from_slice(b"NVS2");
        if let Some(ip) = cfg.lms_server_ip {
            nvs_slice[4] = 1;
            nvs_slice[5..9].copy_from_slice(&ip);
        } else {
            nvs_slice[4] = 0;
            nvs_slice[5..9].copy_from_slice(&[0, 0, 0, 0]);
        }
        let mut written_count: u8 = 0;
        let mut offset = 10;
        if let Some(table) = nvs_overrides {
            for (key, val) in table.iter() {
                let k_bytes = key.as_bytes();
                let v_bytes = val.as_bytes();
                if offset + 2 + k_bytes.len() + v_bytes.len() <= 2048 {
                    nvs_slice[offset] = k_bytes.len() as u8;
                    offset += 1;
                    nvs_slice[offset..offset + k_bytes.len()].copy_from_slice(k_bytes);
                    offset += k_bytes.len();
                    nvs_slice[offset] = v_bytes.len() as u8;
                    offset += 1;
                    nvs_slice[offset..offset + v_bytes.len()].copy_from_slice(v_bytes);
                    offset += v_bytes.len();
                    written_count = written_count.saturating_add(1);
                }
            }
        }
        nvs_slice[9] = written_count;
        Ok(len)
    }

    /// Serializes `cfg` and writes parsed overrides from the RAM sector at
    /// `sector[2048..4096]` with magic `b"NVS2"`.
    pub fn serialize_image(
        &self,
        sector: &mut [u8; JOURNAL_SECTOR_SIZE],
        cfg: &AudioConfig,
        sequence: u32,
        wifi: Option<(&str, &str)>,
    ) -> Result<usize, squeezelite_config::StorageError> {
        Self::serialize_into_opt(None, sector, cfg, sequence, wifi)
    }

    /// Sets pending Wi-Fi credentials to be written on the next commit.
    pub fn set_pending_wifi(&mut self, creds: Option<WifiStaCredentials>) {
        self.pending_wifi = creds;
    }

    /// Pending Wi-Fi credentials, if explicitly scheduled.
    pub fn pending_wifi(&self) -> Option<&WifiStaCredentials> {
        self.pending_wifi.as_ref()
    }

    /// Sequence of the newest journal (RAM image, or boot journal before any
    /// commit).
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// Sector the next flash write goes to.
    pub fn target(&self) -> JournalSlot {
        self.target
    }

    /// Image awaiting a flash write, if the RAM journal is newer than flash.
    pub fn pending_image(&self) -> Option<PendingImage<'_>> {
        self.flash_pending.then(|| PendingImage {
            slot: self.target,
            sector: &self.image.0,
            used_len: (self.image_len + 3) & !3,
            sequence: self.sequence,
        })
    }

    /// Records that [`Self::pending_image`] reached flash: that sector now
    /// holds the newest journal. With [`FlashTargetPolicy::PingPong`] the next
    /// write targets the other one.
    pub fn mark_flash_written(&mut self) {
        if self.flash_pending {
            self.flash_pending = false;
            if self.policy == FlashTargetPolicy::PingPong {
                self.target = self.target.other();
            }
        }
    }

    /// Commits pending journal image to flash storage, reusing `old_buf`
    /// for both the pre-read skip check and post-write verification.
    ///
    /// Eliminates a second 4 KiB stack scratch buffer.
    pub fn commit_flash_verified(
        &mut self,
        mut read_fn: impl FnMut(JournalSlot, &mut [u8; JOURNAL_SECTOR_SIZE]) -> bool,
        mut write_fn: impl FnMut(JournalSlot, &[u8; JOURNAL_SECTOR_SIZE]) -> bool,
        old_buf: &mut [u8; JOURNAL_SECTOR_SIZE],
    ) -> bool {
        if !self.flash_pending {
            return true;
        }
        // Pre-read skip check using old_buf
        if read_fn(self.target, old_buf) && old_buf == &self.image.0 {
            self.mark_flash_written();
            return true;
        }
        if !write_fn(self.target, &self.image.0) {
            return false;
        }
        // Reuse old_buf for read-back verification without a 2nd 4 KiB buffer.
        if !read_fn(self.target, old_buf) || old_buf != &self.image.0 {
            return false;
        }
        self.mark_flash_written();
        true
    }

    fn commit_internal(&mut self, cfg: &AudioConfig, overrides: Option<&NvsOverrideTable>) -> bool {
        let next = self.sequence.wrapping_add(1);
        let mut preserved_ssid = heapless::String::<32>::new();
        let mut preserved_pass = heapless::String::<64>::new();
        let mut has_wifi = false;

        if let Some(ref creds) = self.pending_wifi {
            let _ = preserved_ssid.push_str(creds.ssid.as_str());
            let _ = preserved_pass.push_str(creds.password.as_str());
            has_wifi = !preserved_ssid.is_empty();
        } else if squeezelite_config::FlashJournal::validate_sector(&self.image.0).is_ok() {
            let mut store = squeezelite_config::MemConfigStore::<16>::new();
            if squeezelite_config::FlashJournal::load_from_sector(&self.image.0, &mut store).is_ok()
            {
                if let Ok(Some(s)) = store.get_str::<32>("wifi.ssid") {
                    if !s.is_empty() {
                        let _ = preserved_ssid.push_str(s.as_str());
                        if let Ok(Some(p)) = store.get_str::<64>("wifi.pass") {
                            let _ = preserved_pass.push_str(p.as_str());
                        }
                        has_wifi = true;
                    }
                }
            }
        }

        let wifi_ref = if has_wifi {
            Some((preserved_ssid.as_str(), preserved_pass.as_str()))
        } else {
            None
        };

        let result =
            if overrides.is_none() && self.boot_valid && &self.image.0[2048..2052] == b"NVS2" {
                match serialize_image(&mut self.image.0, cfg, next, wifi_ref) {
                    Ok(len) => {
                        let nvs_slice = &mut self.image.0[2048..4096];
                        if let Some(ip) = cfg.lms_server_ip {
                            nvs_slice[4] = 1;
                            nvs_slice[5..9].copy_from_slice(&ip);
                        } else {
                            nvs_slice[4] = 0;
                            nvs_slice[5..9].copy_from_slice(&[0, 0, 0, 0]);
                        }
                        Ok(len)
                    }
                    Err(e) => Err(e),
                }
            } else {
                Self::serialize_into_opt(overrides, &mut self.image.0, cfg, next, wifi_ref)
            };

        match result {
            Ok(len) => {
                self.image_len = len;
                self.sequence = next;
                self.flash_pending = true;
                if overrides.is_some() {
                    self.boot_valid = false;
                }
                true
            }
            Err(_) => {
                // Staged write protects sector if serialization failed.
                if squeezelite_config::FlashJournal::validate_sector(&self.image.0).is_err() {
                    self.flash_pending = false;
                }
                false
            }
        }
    }
}

impl ConfigJournal for RamJournal<'_> {
    fn commit(&mut self, cfg: &AudioConfig) -> bool {
        self.commit_internal(cfg, None)
    }

    fn commit_with_overrides(&mut self, cfg: &AudioConfig, overrides: &NvsOverrideTable) -> bool {
        self.commit_internal(cfg, Some(overrides))
    }
}

/// Where a boot-resolved value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// The settings FlashJournal.
    Journal,
    /// The C firmware's NVS partition (Wi-Fi only, read-only).
    Nvs,
    /// Compile-time first-boot default.
    Default,
}

/// Boot resolution of the audio/identity fields, each one independently: the
/// journal's value
/// when the journal holds a valid one, else `defaults`'.
pub fn resolve_boot_audio<S: KeyValueStorage>(
    journal: Option<&S>,
    defaults: &AudioConfig,
) -> AudioConfig {
    let mut cfg = defaults.clone();
    let Some(j) = journal else {
        return cfg;
    };
    if let Ok(Some(name)) = j.get_str::<32>(AudioConfig::KEY_NAME) {
        cfg.set_player_name(name.as_str());
    }
    let mut ip = [0u8; 4];
    if j.get(AudioConfig::KEY_LMS_IP, &mut ip) == Ok(4)
        && ip != [0, 0, 0, 0]
        && ip != [255, 255, 255, 255]
    {
        cfg.lms_server_ip = Some(ip);
    }
    if let Ok(Some(port)) = j.get_u32(AudioConfig::KEY_LMS_PORT) {
        if let Ok(port @ 1..) = u16::try_from(port) {
            cfg.lms_server_port = Some(port);
        }
    }
    if let Ok(Some(vol @ 0..=100)) = j.get_u8(AudioConfig::KEY_VOL) {
        cfg.volume = vol;
    }
    if let Ok(Some(eq)) = j.get_u8(AudioConfig::KEY_EQ) {
        cfg.equalizer_preset = eq;
    }
    // Analog gain: u32 since AGAIN was widened; legacy journals hold 1 byte.
    let again = match j.get_u32(AudioConfig::KEY_AGAIN) {
        Ok(Some(v)) => u8::try_from(v).ok(),
        Ok(None) => None,
        Err(_) => j.get_u8(AudioConfig::KEY_AGAIN).ok().flatten(),
    };
    if let Some(code @ 0..=AudioConfig::MAX_ANALOG_GAIN_CODE) = again {
        cfg.analog_gain_code = code;
    }
    if let Ok(Some(loud)) = j.get_bool(AudioConfig::KEY_LOUD) {
        cfg.loudness_enabled = loud;
    }
    if let Ok(Some(rate)) = j.get_u32(AudioConfig::KEY_MAXRATE) {
        cfg.max_sample_rate = rate;
    } else {
        cfg.max_sample_rate = DEFAULT_MAX_SAMPLE_RATE;
    }
    cfg
}

/// Boot resolution of the Wi-Fi station credentials: the C firmware's NVS ones,
/// else the
/// first-boot default (`default` is only called when NVS has none).
pub fn resolve_boot_wifi(
    nvs: impl FnOnce() -> Option<WifiStaCredentials>,
    default: impl FnOnce() -> Option<WifiStaCredentials>,
) -> Option<(WifiStaCredentials, ConfigSource)> {
    if let Some(creds) = nvs() {
        return Some((creds, ConfigSource::Nvs));
    }
    default().map(|creds| (creds, ConfigSource::Default))
}

/// Boot resolution of Wi-Fi station credentials with FlashJournal precedence:
/// 1. Journal (`wifi.ssid`, `wifi.pass`), if present and non-empty ->
///    `ConfigSource::Journal`.
/// 2. NVS partition, if present -> `ConfigSource::Nvs`.
/// 3. Compile-time defaults -> `ConfigSource::Default`.
pub fn resolve_boot_wifi_with_journal<S: KeyValueStorage>(
    journal: Option<&S>,
    nvs: impl FnOnce() -> Option<WifiStaCredentials>,
    default: impl FnOnce() -> Option<WifiStaCredentials>,
) -> Option<(WifiStaCredentials, ConfigSource)> {
    if let Some(j) = journal {
        if let Ok(Some(ssid)) = j.get_str::<32>("wifi.ssid") {
            if !ssid.is_empty() {
                let password = j
                    .get_str::<64>("wifi.pass")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let creds = WifiStaCredentials { ssid, password };
                return Some((creds, ConfigSource::Journal));
            }
        }
    }
    resolve_boot_wifi(nvs, default)
}

/// Serializes `cfg` (and optional Wi-Fi credentials) as a complete
/// journal image into `sector` with `sequence`; returns image length.
///
/// Serializes directly into `sector` after `cfg.validate()` to avoid
/// secondary 4 KiB stack allocations.
fn serialize_image(
    sector: &mut [u8; JOURNAL_SECTOR_SIZE],
    cfg: &AudioConfig,
    sequence: u32,
    wifi: Option<(&str, &str)>,
) -> Result<usize, squeezelite_config::StorageError> {
    cfg.validate()?;
    let mut writer = SectorWriter::new(&mut sector[..])?;
    cfg.save_to(&mut writer)?;
    if let Some((ssid, pass)) = wifi {
        if !ssid.is_empty() {
            writer.set_str("wifi.ssid", ssid)?;
            if !pass.is_empty() {
                writer.set_str("wifi.pass", pass)?;
            }
        }
    }
    let len = writer.finish(sequence)?;
    if len < 2048 {
        sector[len..2048].fill(0xFF);
    } else if len < JOURNAL_SECTOR_SIZE {
        sector[len..].fill(0xFF);
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Journal that refuses the first `refuse` commits.
    struct FlakyJournal {
        refuse: u32,
        commits: u32,
    }

    impl ConfigJournal for FlakyJournal {
        fn commit(&mut self, _cfg: &AudioConfig) -> bool {
            if self.refuse > 0 {
                self.refuse -= 1;
                false
            } else {
                self.commits += 1;
                true
            }
        }
    }

    #[test]
    fn test_journal_slot_offsets_and_ping_pong() {
        const BASE: u32 = 0x003F_E000;
        assert_eq!(JournalSlot::A.offset(BASE), 0x003F_E000);
        assert_eq!(JournalSlot::B.offset(BASE), 0x003F_F000);
        assert_eq!(JournalSlot::A.other(), JournalSlot::B);
        assert_eq!(JournalSlot::B.other(), JournalSlot::A);
        assert_eq!(JournalSlot::from_index(0), JournalSlot::A);
        assert_eq!(JournalSlot::from_index(1), JournalSlot::B);
        assert_eq!(core::mem::align_of::<JournalSector>(), 4);
    }

    #[test]
    fn test_refused_commit_stays_dirty_and_retries_after_a_quiet_period() {
        let handle = ConfigHandle::new(AudioConfig::default());
        let mut writer = ConfigWriter::new(&handle, 0);
        let mut journal = FlakyJournal {
            refuse: 1,
            commits: 0,
        };
        assert!(handle.update(|c| c.volume = 5));
        assert!(!writer.poll(&handle, &mut journal, 0));
        assert!(!writer.poll(&handle, &mut journal, 1_999));
        assert!(!writer.poll(&handle, &mut journal, 2_000)); // refused
        assert!(handle.is_dirty());
        assert!(!writer.poll(&handle, &mut journal, 2_001)); // no tight retry
        assert!(!writer.poll(&handle, &mut journal, 3_999));
        assert!(writer.poll(&handle, &mut journal, 4_000));
        assert_eq!(journal.commits, 1);
        assert!(!handle.is_dirty());
        assert!(!writer.poll(&handle, &mut journal, 10_000));
    }

    #[test]
    fn test_flush_now_commits_immediately_and_only_when_dirty() {
        let handle = ConfigHandle::new(AudioConfig::default());
        let mut journal = FlakyJournal {
            refuse: 0,
            commits: 0,
        };
        assert!(!handle.flush_now(&mut journal));
        assert!(handle.update(|c| c.equalizer_preset = 2));
        assert!(handle.flush_now(&mut journal)); // no debounce on the reboot path
        assert_eq!(journal.commits, 1);
        assert!(!handle.flush_now(&mut journal));
        assert_eq!(handle.read(|c| c.equalizer_preset), 2);
    }

    #[test]
    fn test_ram_journal_refused_commit_keeps_pending_image() {
        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(
            &mut image,
            Some((JournalSlot::B, 9)),
            FlashTargetPolicy::PingPong,
        );
        assert_eq!(ram.target(), JournalSlot::A);
        assert!(ram.commit(&AudioConfig::default()));
        let mut bad = AudioConfig::default();
        bad.player_name.clear(); // empty name: invalid
        assert!(!ram.commit(&bad));
        let pending = ram.pending_image().expect("previous image still pending");
        assert_eq!((pending.slot, pending.sequence), (JournalSlot::A, 10));
        assert!(squeezelite_config::FlashJournal::validate_sector(pending.sector).is_ok());
        ram.mark_flash_written();
        ram.mark_flash_written(); // idempotent without a pending image
        assert_eq!(ram.target(), JournalSlot::B);

        // FixedA never leaves sector A regardless of boot journal or writes.
        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(
            &mut image,
            Some((JournalSlot::B, 9)),
            FlashTargetPolicy::FixedA,
        );
        for seq in 10..13 {
            assert!(ram.commit(&AudioConfig::default()));
            let p = ram.pending_image().unwrap();
            assert_eq!((p.slot, p.sequence), (JournalSlot::A, seq));
            ram.mark_flash_written();
            assert_eq!(ram.target(), JournalSlot::A);
        }
    }

    #[test]
    fn test_logic_505_resolve_boot_wifi_with_journal() {
        use squeezelite_config::MemConfigStore;

        let mut store = MemConfigStore::<16>::new();
        store.set_str("wifi.ssid", "JournalAP").expect("set ssid");
        store
            .set_str("wifi.pass", "JournalSecret")
            .expect("set pass");

        let nvs_creds = WifiStaCredentials {
            ssid: "NvsAP".try_into().unwrap(),
            password: "NvsPass".try_into().unwrap(),
        };
        let def_creds = WifiStaCredentials {
            ssid: "DefAP".try_into().unwrap(),
            password: "DefPass".try_into().unwrap(),
        };

        // 1. Journal present with non-empty SSID beats NVS and default.
        let (res_creds, src) = resolve_boot_wifi_with_journal(
            Some(&store),
            || Some(nvs_creds.clone()),
            || Some(def_creds.clone()),
        )
        .expect("resolved with journal");
        assert_eq!(src, ConfigSource::Journal);
        assert_eq!(res_creds.ssid.as_str(), "JournalAP");
        assert_eq!(res_creds.password.as_str(), "JournalSecret");

        // 2. Journal absent falls back to NVS
        let (res_creds, src) = resolve_boot_wifi_with_journal(
            None::<&MemConfigStore<16>>,
            || Some(nvs_creds.clone()),
            || Some(def_creds.clone()),
        )
        .expect("resolved with nvs");
        assert_eq!(src, ConfigSource::Nvs);
        assert_eq!(res_creds.ssid.as_str(), "NvsAP");

        // 3. Journal and NVS absent falls back to default
        let (res_creds, src) = resolve_boot_wifi_with_journal(
            None::<&MemConfigStore<16>>,
            || None,
            || Some(def_creds.clone()),
        )
        .expect("resolved with default");
        assert_eq!(src, ConfigSource::Default);
        assert_eq!(res_creds.ssid.as_str(), "DefAP");

        // 4. Backward compatibility check for resolve_boot_wifi
        let (res_creds, src) =
            resolve_boot_wifi(|| Some(nvs_creds.clone()), || Some(def_creds.clone()))
                .expect("resolved with resolve_boot_wifi");
        assert_eq!(src, ConfigSource::Nvs);
        assert_eq!(res_creds.ssid.as_str(), "NvsAP");
    }

    #[test]
    fn test_ram_journal_commit_preserves_wifi_credentials() {
        use squeezelite_config::MemConfigStore;

        let mut image = JournalSector::erased();
        let mut initial_store = MemConfigStore::<16>::new();
        initial_store
            .set_str("wifi.ssid", "StationAP")
            .expect("ssid");
        initial_store
            .set_str("wifi.pass", "StationKey")
            .expect("pass");
        initial_store
            .set_str(AudioConfig::KEY_NAME, "MyPlayer")
            .expect("name");
        squeezelite_config::FlashJournal::save_to_sector_with_seq(&initial_store, &mut image.0, 1)
            .expect("initial save");

        let mut ram = RamJournal::new(
            &mut image,
            Some((JournalSlot::A, 1)),
            FlashTargetPolicy::FixedA,
        );

        // Commit an audio config update with pending_wifi == None
        let updated_audio = AudioConfig {
            volume: 77,
            ..AudioConfig::default()
        };
        assert!(ram.commit(&updated_audio));

        // Verify that wifi.ssid and wifi.pass were preserved in the sector.
        let mut verified_store = MemConfigStore::<16>::new();
        squeezelite_config::FlashJournal::load_from_sector(&ram.image.0, &mut verified_store)
            .expect("load sector");
        assert_eq!(
            verified_store
                .get_str::<32>("wifi.ssid")
                .unwrap()
                .as_deref(),
            Some("StationAP")
        );
        assert_eq!(
            verified_store
                .get_str::<64>("wifi.pass")
                .unwrap()
                .as_deref(),
            Some("StationKey")
        );
        assert_eq!(
            verified_store.get_u8(AudioConfig::KEY_VOL).unwrap(),
            Some(77)
        );

        // Verify staging: invalid config does not corrupt the sector
        let mut bad_cfg = AudioConfig::default();
        bad_cfg.player_name.clear();
        let snapshot = ram.image.0;
        assert!(!ram.commit(&bad_cfg));
        assert_eq!(ram.image.0, snapshot);
    }

    #[test]
    fn test_export_json_redacted_and_maxrate_resolution() {
        let mut cfg = AudioConfig::default();
        cfg.player_name.clear();
        cfg.player_name.push_str("TestBox").unwrap();
        cfg.volume = 65;
        cfg.set_max_sample_rate(96_000);

        let handle = ConfigHandle::new(cfg);
        let redacted = handle.export_json_redacted().expect("json export");
        assert!(redacted.contains("\"player_name\":\"TestBox\""));
        assert!(redacted.contains("\"volume\":65"));
        assert!(redacted.contains("\"max_sample_rate\":96000"));
        assert!(redacted.contains("\"wifi.pass\":\"***\""));

        let unredacted = handle.export_json().expect("unredacted json export");
        assert!(!unredacted.contains("\"wifi.pass\""));

        let mut store = squeezelite_config::MemConfigStore::<16>::new();
        store.set_u32(KEY_MAXRATE, 192_000).expect("store maxrate");
        let resolved = resolve_boot_audio(Some(&store), &AudioConfig::default());
        assert_eq!(resolved.max_sample_rate(), 192_000);

        let empty_store = squeezelite_config::MemConfigStore::<16>::new();
        let fallback = resolve_boot_audio(Some(&empty_store), &AudioConfig::default());
        assert_eq!(fallback.max_sample_rate(), DEFAULT_MAX_SAMPLE_RATE);
    }

    #[test]
    fn test_ram_journal_commit_flash_verified_reusing_buffer() {
        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(ram.commit(&AudioConfig::default()));

        let simulated_flash = core::cell::RefCell::new([0xFFu8; JOURNAL_SECTOR_SIZE]);
        let mut old_buf = [0u8; JOURNAL_SECTOR_SIZE];

        let committed = ram.commit_flash_verified(
            |_slot, buf| {
                buf.copy_from_slice(&*simulated_flash.borrow());
                true
            },
            |_slot, data| {
                simulated_flash.borrow_mut().copy_from_slice(data);
                true
            },
            &mut old_buf,
        );
        assert!(committed);
        assert_eq!(*simulated_flash.borrow(), ram.image.0);
    }

    #[test]
    fn test_nvs2_overrides_serialization_and_restore() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.apply_key_value("host_name", "KitchenPlayer"));
        assert!(handle.apply_key_value("lms_ip", "192.168.1.50"));
        assert!(handle.apply_key_value("custom_k", "custom_v"));

        assert_eq!(handle.host_name_str(), "KitchenPlayer");
        let overrides = handle.nvs_overrides();
        assert!(overrides.iter().any(|s| {
            s.used && s.key.as_str() == "host_name" && s.value.as_str() == "KitchenPlayer"
        }));
        assert!(overrides.iter().any(|s| {
            s.used && s.key.as_str() == "lms_ip" && s.value.as_str() == "192.168.1.50"
        }));
        assert!(overrides
            .iter()
            .any(|s| { s.used && s.key.as_str() == "custom_k" && s.value.as_str() == "custom_v" }));

        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(handle.force_flush(&mut ram));

        let pending = ram.pending_image().expect("pending image");
        assert_eq!(&pending.sector[2048..2052], b"NVS2");

        let mut image2 = JournalSector::erased();
        let mut restored_ram = RamJournal::new(&mut image2, None, FlashTargetPolicy::FixedA);
        assert!(restored_ram.restore_from_sector(pending.sector));
        let mut restored_handle = ConfigHandle::new(AudioConfig::default());
        restored_ram.restore_into_handle(&mut restored_handle);

        let restored_overrides = restored_handle.nvs_overrides();
        assert!(restored_overrides.iter().any(|s| {
            s.used && s.key.as_str() == "host_name" && s.value.as_str() == "KitchenPlayer"
        }));
        assert!(restored_overrides.iter().any(|s| {
            s.used && s.key.as_str() == "lms_ip" && s.value.as_str() == "192.168.1.50"
        }));
        assert!(restored_overrides
            .iter()
            .any(|s| { s.used && s.key.as_str() == "custom_k" && s.value.as_str() == "custom_v" }));
    }

    #[test]
    fn test_autoexec1_flags_and_effective_getters() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.apply_key_value(
            "autoexec1",
            "squeezelite -o I2S -n BASEMENT -s 10.0.0.50 -b 500:2000 -c flc,p"
        ));
        assert_eq!(handle.effective_lms_server_ip(), Some([10, 0, 0, 50]));
        assert_eq!(handle.device_name().as_str(), "BASEMENT");

        // Service enables default to true
        assert!(handle.is_airplay_enabled());
        assert!(handle.is_bt_sink_enabled());
        assert!(handle.is_cspot_enabled());
        assert!(handle.is_autoexec_enabled());

        // Can disable services
        handle.apply_key_value("enable_airplay", "0");
        assert!(!handle.is_airplay_enabled());
        handle.apply_key_value("enable_bt_sink", "no");
        assert!(!handle.is_bt_sink_enabled());
        handle.apply_key_value("enable_cspot", "false");
        assert!(!handle.is_cspot_enabled());
        handle.apply_key_value("autoexec", "off");
        assert!(!handle.is_autoexec_enabled());

        // Parse equalizer bands
        assert_eq!(parse_equalizer_bands(""), None);
        let bands = parse_equalizer_bands("2,1,0,-1,-2,0,1,2,1,0").expect("valid bands");
        assert_eq!(bands, [2.0, 1.0, 0.0, -1.0, -2.0, 0.0, 1.0, 2.0, 1.0, 0.0]);

        // Trailing fill with 0.0
        let partial = parse_equalizer_bands("3.5,-4.0").expect("partial bands");
        assert_eq!(partial[0], 3.5);
        assert_eq!(partial[1], -4.0);
        assert_eq!(partial[2], 0.0);
    }

    #[test]
    fn test_restore_sector_into_config_handle() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        handle.apply_key_value(
            "autoexec1",
            "squeezelite -o I2S -n BASEMENT -s 10.0.0.50 -b 500:2000 -c flc,p",
        );
        handle.set_sequence(42);

        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(handle.force_flush(&mut ram));

        let pending = ram.pending_image().expect("pending image");

        let mut restored_handle = ConfigHandle::new(AudioConfig::default());
        assert!(FlashNvsJournal::restore_sector_into(
            pending.sector,
            &mut restored_handle
        ));

        assert_eq!(
            restored_handle.effective_lms_server_ip(),
            Some([10, 0, 0, 50])
        );
        assert_eq!(restored_handle.device_name().as_str(), "BASEMENT");
        assert_eq!(restored_handle.sequence(), 1);
    }

    #[test]
    fn test_parse_equalizer_bands_rejects_nan_inf_and_over_10_bands() {
        assert_eq!(parse_equalizer_bands("NaN,0,0"), None);
        assert_eq!(parse_equalizer_bands("0,nan,0"), None);
        assert_eq!(parse_equalizer_bands("inf,0,0"), None);
        assert_eq!(parse_equalizer_bands("-inf,0,0"), None);
        assert_eq!(parse_equalizer_bands("infinity,0,0"), None);
        assert_eq!(parse_equalizer_bands("1,2,3,4,5,6,7,8,9,10,11"), None);
        assert_eq!(parse_equalizer_bands("1,2,3,4,5,6,7,8,9,10,bad"), None);
        assert_eq!(parse_equalizer_bands("1,2,3,4,5,6,7,8,9,10,"), None);
        let clamped = parse_equalizer_bands("20.0,-20.0").expect("finite clamp");
        assert_eq!(clamped[0], 15.0);
        assert_eq!(clamped[1], -15.0);
    }

    #[test]
    fn test_parse_ipv4_dotted_and_autoexec1_port_validation() {
        assert_eq!(parse_ipv4_dotted("192.168.1.50"), Some([192, 168, 1, 50]));
        assert_eq!(parse_ipv4_dotted("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4_dotted("+192.168.1.1"), None);
        assert_eq!(parse_ipv4_dotted("192.+168.1.1"), None);
        assert_eq!(parse_ipv4_dotted("010.001.000.001"), None);
        assert_eq!(parse_ipv4_dotted("192.168.01.1"), None);
        assert_eq!(parse_ipv4_dotted("256.1.1.1"), None);

        let mut handle = ConfigHandle::new(AudioConfig::default());
        handle.apply_autoexec1_flags("squeezelite -s 192.168.1.50:invalid");
        assert_eq!(handle.lms_server_ip(), None);
        handle.apply_autoexec1_flags("squeezelite -s 192.168.1.50:+3483");
        assert_eq!(handle.lms_server_ip(), None);
        handle.apply_autoexec1_flags("squeezelite -s 192.168.1.50:99999");
        assert_eq!(handle.lms_server_ip(), None);
        handle.apply_autoexec1_flags("squeezelite -s 192.168.1.50:3483");
        assert_eq!(handle.lms_server_ip(), Some([192, 168, 1, 50]));
    }

    #[test]
    fn test_set_nvs_override_rejects_over_128_bytes() {
        let handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.set_nvs_override("k1", "short"));
        let too_long = "a".repeat(129);
        assert!(!handle.set_nvs_override("k1", &too_long));
        assert_eq!(handle.nvs_override("k1").as_deref(), Some("short"));
        assert!(!handle.set_nvs_override("k2", &too_long));
        assert_eq!(handle.nvs_override("k2"), None);
        let exact_128 = "b".repeat(128);
        assert!(handle.set_nvs_override("k2", &exact_128));
        assert_eq!(
            handle.nvs_override("k2").as_deref(),
            Some(exact_128.as_str())
        );
    }

    #[test]
    fn test_nvs_entry_slot_debug_and_secret_key_redaction() {
        assert!(is_secret_config_key("password"));
        assert!(is_secret_config_key("  WIFI.PASS "));
        assert!(is_secret_config_key("Web_Pwd"));
        assert!(is_secret_config_key("TELNET_PWD"));
        assert!(!is_secret_config_key("host_name"));

        let mut secret_slot = NvsEntrySlot {
            used: true,
            ..Default::default()
        };
        secret_slot.key.push_str("WIFI.PASS").unwrap();
        secret_slot.value.push_str("TopSecretPass123").unwrap();
        let dbg = format!("{secret_slot:?}");
        assert!(!dbg.contains("TopSecretPass123"));
        assert!(dbg.contains("<redacted>"));

        let mut plain_slot = NvsEntrySlot {
            used: true,
            ..Default::default()
        };
        plain_slot.key.push_str("host_name").unwrap();
        plain_slot.value.push_str("LivingRoom").unwrap();
        let plain_dbg = format!("{plain_slot:?}");
        assert!(plain_dbg.contains("LivingRoom"));

        let handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.set_nvs_override("wifi.pass", "SuperSecret"));
        assert_eq!(
            handle.nvs_override_redacted("  WIFI.PASS ").as_deref(),
            Some("<redacted>")
        );
        assert_eq!(
            handle.nvs_override("wifi.pass").as_deref(),
            Some("SuperSecret")
        );
    }

    #[test]
    fn test_apply_lms_server_ip_with_port_and_effective_ip() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.apply_key_value("lms_server_ip", "172.25.8.40:9000"));
        assert_eq!(handle.lms_server_ip(), Some([172, 25, 8, 40]));
        assert_eq!(handle.read(|c| c.lms_server_port), Some(9000));
        assert_eq!(
            handle.nvs_override("lms_server_ip").as_deref(),
            Some("172.25.8.40:9000")
        );
        assert_eq!(
            handle.nvs_override("lms_ip").as_deref(),
            Some("172.25.8.40")
        );
        assert_eq!(handle.effective_lms_server_ip(), Some([172, 25, 8, 40]));

        assert!(handle.update(|c| c.lms_server_ip = None));
        assert_eq!(handle.effective_lms_server_ip(), Some([172, 25, 8, 40]));

        assert!(!handle.apply_key_value("lms_server_ip", "172.25.8.40:0"));
        assert!(!handle.apply_key_value("lms_server_ip", "172.25.8.40:99999"));
        assert!(!handle.apply_key_value("lms_server_ip", "172.25.8.40:abc"));
    }

    #[test]
    fn test_apply_long_autoexec1_and_propagate_override_failure() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        let cmd =
            "squeezelite -o I2S -n BASEMENT -s 172.25.8.40:9000 -b 500:2000 -C 30 -d all=info -W";
        assert!(cmd.len() > 64 && cmd.len() <= 128);
        assert!(handle.apply_key_value("autoexec1", cmd));
        assert_eq!(handle.nvs_override("autoexec1").as_deref(), Some(cmd));
        assert_eq!(handle.lms_server_ip(), Some([172, 25, 8, 40]));
        assert_eq!(handle.read(|c| c.lms_server_port), Some(9000));

        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(handle.force_flush(&mut ram));
        let pending = ram.pending_image().expect("pending image");

        let mut image2 = JournalSector::erased();
        let mut restored_ram = RamJournal::new(&mut image2, None, FlashTargetPolicy::FixedA);
        assert!(restored_ram.restore_from_sector(pending.sector));
        let mut restored = ConfigHandle::new(AudioConfig::default());
        restored_ram.restore_into_handle(&mut restored);
        assert_eq!(restored.nvs_override("autoexec1").as_deref(), Some(cmd));
        assert_eq!(restored.lms_server_ip(), Some([172, 25, 8, 40]));
        assert_eq!(restored.read(|c| c.lms_server_port), Some(9000));

        let too_long = "x".repeat(129);
        assert!(!handle.apply_key_value("autoexec1", &too_long));
        assert!(!handle.apply_key_value("custom_key", &too_long));
        assert!(!handle.apply_key_value("host_name", &too_long));
        assert!(!handle.apply_key_value("equalizer", &too_long));
        assert_eq!(handle.nvs_override("autoexec1").as_deref(), Some(cmd));
        assert_eq!(handle.nvs_override("custom_key"), None);

        let mut slot_handle = ConfigHandle::new(AudioConfig::default());
        for i in 0..25 {
            let key = format!("k_{i}");
            let val = format!("v_{i}");
            assert!(slot_handle.apply_key_value(&key, &val));
        }
    }

    #[test]
    fn test_sync_from_updates_device_and_host_name() {
        let shared = ConfigHandle::new(AudioConfig::default());
        let mut local = shared.clone_handle();
        assert!(local.apply_key_value("host_name", "DEVBENCH"));
        assert_eq!(local.host_name_str(), "DEVBENCH");
        assert!(shared.sync_from(&local));
        assert_eq!(shared.device_name().as_str(), "DEVBENCH");
        assert_eq!(shared.clone_handle().host_name_str(), "DEVBENCH");
    }

    #[test]
    fn test_compact_nvs_override_table_32_slots_and_in_place_compaction() {
        let mut table = NvsOverrideTable::new();
        let long_val = "x".repeat(MAX_NVS_VALUE_LEN);
        assert!(table.set("autoexec1", &long_val));
        for i in 1..MAX_NVS_OVERRIDE_SLOTS {
            let k = format!("k_{i:02}");
            let v = format!("v_{i:02}");
            assert!(table.set(&k, &v), "slot {i} should fit");
        }
        assert!(!table.set("slot_33", "nope"), "33rd key must fail");

        // Repeated length-changing updates on autoexec1 must compact in place
        // and never exhaust the 1024-byte pool.
        for iter in 0..50 {
            let next = if iter % 2 == 0 {
                "short".to_string()
            } else {
                long_val.clone()
            };
            assert!(table.set("autoexec1", &next), "iter {iter} failed");
            assert_eq!(table.value("autoexec1"), Some(next.as_str()));
            assert_eq!(table.value("k_01"), Some("v_01"));
            assert_eq!(table.value("k_31"), Some("v_31"));
        }
        assert!(table.pool_used() <= NVS_OVERRIDE_POOL_BYTES);

        // Pool overflow on a new entry must fail without corrupting existing
        // keys; failed update on an existing key must preserve its prior value.
        let mut full = NvsOverrideTable::new();
        for i in 0..8 {
            assert!(full.set(&format!("big_{i}"), &long_val));
        }
        assert_eq!(full.pool_used(), 1024);
        assert!(!full.set("overflow_new", "a"));
        assert!(full.set("big_0", "tiny"));
        assert_eq!(full.value("big_0"), Some("tiny"));
        assert!(full.set("extra", &"z".repeat(124)));
        assert_eq!(full.pool_used(), 1024);
        assert!(!full.set("big_0", &long_val));
        assert_eq!(full.value("big_0"), Some("tiny"));
        assert_eq!(full.value("big_7"), Some(long_val.as_str()));

        // RamJournal::parsed_nvs_overrides parses directly from sector bytes.
        let mut handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.apply_key_value("autoexec1", &long_val));
        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(handle.flush_now(&mut ram));
        let parsed = ram.parsed_nvs_overrides();
        assert_eq!(parsed.value("autoexec1"), Some(long_val.as_str()));
    }

    #[test]
    fn test_cold_boot_restore_preserves_lms_server_ip_port() {
        let mut handle = ConfigHandle::new(AudioConfig::default());
        assert!(handle.apply_key_value("lms_server_ip", "172.25.8.40:9000"));
        assert!(handle.apply_key_value(
            "autoexec1",
            "squeezelite -o I2S -b 500:2000 -d all=info -C 30 -W -s 172.25.8.40:9000 -n BASEMENT",
        ));

        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, None, FlashTargetPolicy::FixedA);
        assert!(handle.flush_now(&mut ram));
        let pending = ram.pending_image().expect("pending image");

        let mut image2 = JournalSector::erased();
        let mut restored_ram = RamJournal::new(&mut image2, None, FlashTargetPolicy::FixedA);
        assert!(restored_ram.restore_from_sector(pending.sector));
        let mut restored = ConfigHandle::new(AudioConfig::default());
        restored_ram.restore_into_handle(&mut restored);

        assert_eq!(
            restored.nvs_override("lms_server_ip").as_deref(),
            Some("172.25.8.40:9000"),
            "cold-boot replay must preserve :port on lms_server_ip"
        );
        assert_eq!(
            restored.nvs_override("lms_ip").as_deref(),
            Some("172.25.8.40")
        );
        assert_eq!(
            restored.read(|c| (c.lms_server_ip, c.lms_server_port)),
            (Some([172, 25, 8, 40]), Some(9000))
        );

        assert!(restored.apply_key_value("lms_server_ip", "172.25.8.40"));
        assert_eq!(
            restored.nvs_override("lms_server_ip").as_deref(),
            Some("172.25.8.40")
        );
        assert_eq!(restored.read(|c| c.lms_server_port), None);
    }
}
