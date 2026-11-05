//! Runtime configuration: the in-RAM [`AudioConfig`] behind a lock, debounced
//! persistence, the
//! RAM journal image, and boot-time resolution (spec §3.4–3.5, Task 10).
//!
//! Platform-free by design: no platform or cargo-feature gating here (that
//! lives in
//! `main.rs`/`tasks.rs`), so every rule is exercised by host tests.
//!
//! # Pieces
//!
//! - [`ConfigHandle`]: the in-RAM [`AudioConfig`] + dirty flag, behind an
//!   embassy-sync blocking
//!   mutex. The coordinator (`setd` name), the HTTP server, the CYD bridge and
//!   the LMS volume path
//!   change it through [`ConfigHandle::update`].
//! - [`ConfigWriter`]: the debounce state machine of the `config_writer` task:
//!   a change is
//!   journaled once no further change has arrived for [`DEBOUNCE_MS`].
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

/// Something that durably records a configuration snapshot (device:
/// [`RamJournal`]).
pub trait ConfigJournal {
    /// Records `cfg`; `false` if it was not recorded (the snapshot stays dirty).
    fn commit(&mut self, cfg: &AudioConfig) -> bool;
}

struct HandleState {
    cfg: AudioConfig,
    dirty: bool,
    /// Bumped by every published change.
    generation: u32,
    /// Generation seen by the last [`ConfigHandle::snapshot_if_dirty`].
    snapshot_generation: u32,
}

/// The runtime configuration: an [`AudioConfig`] plus a dirty flag behind one
/// mutex.
pub struct ConfigHandle {
    state: Mutex<CriticalSectionRawMutex, RefCell<HandleState>>,
}

impl ConfigHandle {
    /// A clean handle holding `cfg` (the boot-resolved configuration).
    pub const fn new(cfg: AudioConfig) -> Self {
        Self {
            state: Mutex::new(RefCell::new(HandleState {
                cfg,
                dirty: false,
                generation: 0,
                snapshot_generation: 0,
            })),
        }
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
    pub fn flush_now<J: ConfigJournal>(&self, journal: &mut J) -> bool {
        // The commit runs OUTSIDE the lock: it may be slow (serialization; on the
        // device with
        // `runtime-flash-persist`, a flash write follows), and `update` must never wait
        // on it.
        let Some(snapshot) = self.snapshot_if_dirty() else {
            return false;
        };
        if journal.commit(&snapshot) {
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

    /// Exports the configuration as JSON with `KEY_WIFI_PASS` redacted to `"***"`.
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

/// Debounce state of the `config_writer` task.
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
    pub fn poll<J: ConfigJournal>(
        &mut self,
        handle: &ConfigHandle,
        journal: &mut J,
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
            // Never retry a refused commit in a tight loop: wait another quiet period.
            self.last_change_ms = now_ms;
        }
        committed
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
    /// Sector to erase and write (never the one holding the newest valid journal).
    pub slot: JournalSlot,
    /// The whole sector (image followed by `0xFF`).
    pub sector: &'a [u8; JOURNAL_SECTOR_SIZE],
    /// Image length rounded up to a 4-byte word (what a NOR write needs to cover).
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
    /// Dual-sector ping-pong (`runtime-flash-persist` builds only): the target is
    /// never the
    /// sector holding the newest valid journal, so power loss mid-write keeps the
    /// previous one.
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
    pending_wifi: Option<WifiStaCredentials>,
}

impl<'a> RamJournal<'a> {
    /// `boot`: the sector holding the newest valid journal and its sequence (`None`
    /// when flash
    /// held none). `policy`: where flash writes go.
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
        Self {
            image,
            image_len: 0,
            sequence,
            target,
            policy,
            flash_pending: false,
            pending_wifi: None,
        }
    }

    /// Sets pending Wi-Fi credentials to be written on the next commit.
    pub fn set_pending_wifi(&mut self, creds: Option<WifiStaCredentials>) {
        self.pending_wifi = creds;
    }

    /// Pending Wi-Fi credentials, if explicitly scheduled.
    pub fn pending_wifi(&self) -> Option<&WifiStaCredentials> {
        self.pending_wifi.as_ref()
    }

    /// Sequence of the newest journal (RAM image, or the boot journal before any
    /// commit).
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// Sector the next flash write goes to.
    pub fn target(&self) -> JournalSlot {
        self.target
    }

    /// The image awaiting a flash write, if the RAM journal is newer than flash.
    pub fn pending_image(&self) -> Option<PendingImage<'_>> {
        self.flash_pending.then(|| PendingImage {
            slot: self.target,
            sector: &self.image.0,
            used_len: (self.image_len + 3) & !3,
            sequence: self.sequence,
        })
    }

    /// Records that [`Self::pending_image`] reached flash: that sector now holds
    /// the newest
    /// journal. With [`FlashTargetPolicy::PingPong`] the next write targets the
    /// other one.
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
        // Reuse old_buf for read-back verification without a second 4 KiB buffer
        if !read_fn(self.target, old_buf) || old_buf != &self.image.0 {
            return false;
        }
        self.mark_flash_written();
        true
    }
}

impl ConfigJournal for RamJournal<'_> {
    fn commit(&mut self, cfg: &AudioConfig) -> bool {
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

        match serialize_image(&mut self.image.0, cfg, next, wifi_ref) {
            Ok(len) => {
                self.image_len = len;
                self.sequence = next;
                self.flash_pending = true;
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
    // Analog gain: u32 since the AGAIN key was widened; legacy journals hold one
    // byte.
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
    if len < JOURNAL_SECTOR_SIZE {
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

        // FixedA never leaves sector A, whatever the boot journal and however many
        // writes.
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

        // 1. Journal present with non-empty SSID takes precedence over NVS and default
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

        // Verify that wifi.ssid and wifi.pass were preserved in the serialized sector
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
}
