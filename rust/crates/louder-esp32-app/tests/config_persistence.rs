//! Task 10: debounced persistence (spec §3.5) and boot-time identity resolution (spec §3.4).
//!
//! Review Focus #5: three updates inside one debounce window produce exactly one journal write,
//! carrying the final state, and a snapshot racing an update is never a mix of two states.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use louder_esp32_app::board::first_boot_defaults;
use louder_esp32_app::config_handle::{
    resolve_boot_audio, resolve_boot_wifi, ConfigHandle, ConfigJournal, ConfigSource, ConfigWriter,
    JournalSector, JournalSlot, RamJournal, DEBOUNCE_MS, JOURNAL_SECTOR_SIZE,
};
use louder_esp32_app::ota_safety::WifiStaCredentials;
use squeezelite_config::{
    AudioConfig, FlashJournal, JournalView, KeyValueStorage, MemConfigStore, WifiCredentials,
};

/// Journal double: records every commit with the clock value it happened at.
#[derive(Default)]
struct RecordingJournal {
    now_ms: u64,
    commits: Vec<(u64, AudioConfig)>,
}

impl ConfigJournal for RecordingJournal {
    fn commit(&mut self, cfg: &AudioConfig) -> bool {
        self.commits.push((self.now_ms, cfg.clone()));
        true
    }
}

/// Every field derived from one `k`, so a snapshot mixing two updates is detectable.
fn state_for(k: u32) -> AudioConfig {
    let mut cfg = AudioConfig::default();
    cfg.player_name.clear();
    cfg.player_name.push_str(&format!("P{k}")).unwrap();
    cfg.lms_server_ip = Some([10, 1, (k >> 8) as u8, (k as u8) | 1]);
    cfg.lms_server_port = Some(1000 + (k % 1000) as u16);
    cfg.volume = (k % 101) as u8;
    cfg.equalizer_preset = (k % 7) as u8;
    cfg.analog_gain_code = (k % 32) as u8;
    cfg.loudness_enabled = k.is_multiple_of(2);
    cfg
}

fn assert_consistent(cfg: &AudioConfig) -> u32 {
    let k: u32 = cfg.player_name.as_str()[1..].parse().expect("name P<k>");
    assert_eq!(*cfg, state_for(k), "torn snapshot (name says k={k})");
    k
}

#[test]
fn test_config_writer_debounce_and_atomic_snapshot() {
    // --- 1. Debounce: updates at t=0/500/900 ms -> exactly one write, at t=2900, final state.
    let handle = ConfigHandle::new(first_boot_defaults::audio_config());
    let mut writer = ConfigWriter::new(&handle, 0);
    let mut journal = RecordingJournal::default();
    for now_ms in 0..=8_000u64 {
        match now_ms {
            0 => assert!(handle.update(|c| c.volume = 10)),
            500 => assert!(handle.update(|c| c.equalizer_preset = 3)),
            900 => assert!(handle.update(|c| {
                c.set_player_name("Den");
            })),
            _ => {}
        }
        journal.now_ms = now_ms;
        writer.poll(&handle, &mut journal, now_ms);
    }
    let mut expected = first_boot_defaults::audio_config();
    expected.volume = 10;
    expected.equalizer_preset = 3;
    expected.set_player_name("Den");
    assert_eq!(journal.commits.len(), 1, "one write per burst: {:?}", journal.commits);
    assert_eq!(journal.commits[0].0, 900 + u64::from(DEBOUNCE_MS));
    assert_eq!(journal.commits[0].0, 2900);
    assert_eq!(journal.commits[0].1, expected);
    assert!(!handle.is_dirty());

    // An update that changes nothing is not a change (no write, no wear).
    assert!(!handle.update(|c| c.volume = 10));
    assert!(!handle.is_dirty());
    // An invalid result is refused: the RAM config stays valid, so a commit can not fail on it.
    assert!(!handle.update(|c| c.volume = 101));
    assert_eq!(handle.read(|c| c.volume), 10);

    // --- 2. Deterministic interleave: an update between snapshot and mark_clean is not lost.
    let handle = ConfigHandle::new(state_for(1));
    assert!(handle.update(|c| *c = state_for(2)));
    let snap = handle.snapshot_if_dirty().expect("dirty");
    assert!(handle.update(|c| *c = state_for(3))); // races the commit of `snap`
    handle.mark_clean(); // marks `snap` journaled, not state 3
    assert_eq!(snap, state_for(2));
    assert!(handle.is_dirty(), "update published after the snapshot was dropped");
    assert_eq!(handle.snapshot_if_dirty(), Some(state_for(3)));
    handle.mark_clean();
    assert!(!handle.is_dirty());
    assert_eq!(handle.snapshot_if_dirty(), None);

    // --- 3. Threads: an updater rewriting every field races a snapshot/commit loop.
    const UPDATES: u32 = 20_000;
    let handle = Arc::new(ConfigHandle::new(state_for(0)));
    let done = Arc::new(AtomicBool::new(false));
    let updater = {
        let handle = Arc::clone(&handle);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            for k in 1..=UPDATES {
                assert!(handle.update(|c| *c = state_for(k)));
                if k.is_multiple_of(64) {
                    thread::yield_now();
                }
            }
            done.store(true, Ordering::Release);
        })
    };
    let mut journal = RecordingJournal::default();
    let mut last_k = 0;
    while !done.load(Ordering::Acquire) {
        if let Some(snap) = handle.snapshot_if_dirty() {
            let k = assert_consistent(&snap);
            assert!(k >= last_k, "snapshots go forward ({k} after {last_k})");
            last_k = k;
            assert!(journal.commit(&snap));
            handle.mark_clean();
        }
    }
    updater.join().unwrap();
    handle.flush_now(&mut journal); // the reboot-path flush catches whatever is left
    assert!(!handle.is_dirty());
    assert!(journal.commits.len() > 1, "the race actually interleaved");
    for (_, snap) in &journal.commits {
        assert_consistent(snap);
    }
    assert_eq!(journal.commits.last().unwrap().1, state_for(UPDATES), "final state journaled");
}

fn audio_journal(f: impl FnOnce(&mut MemConfigStore<16>)) -> MemConfigStore<16> {
    let mut store = MemConfigStore::<16>::new();
    f(&mut store);
    store
}

fn sta(ssid: &str, password: &str) -> WifiStaCredentials {
    let mut creds = WifiStaCredentials { ssid: Default::default(), password: Default::default() };
    creds.ssid.push_str(ssid).unwrap();
    creds.password.push_str(password).unwrap();
    creds
}

#[test]
fn test_boot_resolution_order() {
    let defaults = first_boot_defaults::audio_config();
    assert_eq!(defaults.player_name.as_str(), first_boot_defaults::PLAYER_NAME);
    assert_eq!(defaults.lms_server_ip, Some(first_boot_defaults::LMS_SERVER_IP));
    assert_eq!(defaults.lms_server_port, Some(first_boot_defaults::LMS_SERVER_PORT));

    // No journal: every field is the first-boot default.
    assert_eq!(resolve_boot_audio::<MemConfigStore<16>>(None, &defaults), defaults);

    // A journal holding only some keys: those fields come from it, the rest stay defaults.
    let partial = audio_journal(|s| {
        s.set_str(AudioConfig::KEY_NAME, "Den").unwrap();
        s.set_u8(AudioConfig::KEY_VOL, 33).unwrap();
    });
    let cfg = resolve_boot_audio(Some(&partial), &defaults);
    assert_eq!(cfg.player_name.as_str(), "Den");
    assert_eq!(cfg.volume, 33);
    assert_eq!(cfg.lms_server_ip, defaults.lms_server_ip);
    assert_eq!(cfg.lms_server_port, defaults.lms_server_port);
    assert_eq!(cfg.equalizer_preset, defaults.equalizer_preset);
    assert_eq!(cfg.analog_gain_code, defaults.analog_gain_code);
    assert_eq!(cfg.loudness_enabled, defaults.loudness_enabled);

    // A full journal beats every default.
    let mut full_cfg = AudioConfig::default();
    full_cfg.set_player_name("Kitchen");
    full_cfg.lms_server_ip = Some([192, 168, 7, 9]);
    full_cfg.lms_server_port = Some(3484);
    full_cfg.volume = 61;
    full_cfg.equalizer_preset = 4;
    full_cfg.analog_gain_code = 0x0C;
    full_cfg.loudness_enabled = true;
    let full = audio_journal(|s| full_cfg.save_to(s).unwrap());
    assert_eq!(resolve_boot_audio(Some(&full), &defaults), full_cfg);

    // Per field: an invalid journal value falls back to the default for that field only.
    let bad = audio_journal(|s| {
        s.set_str(AudioConfig::KEY_NAME, "Attic").unwrap();
        s.set_u8(AudioConfig::KEY_VOL, 200).unwrap(); // > 100
        s.set(AudioConfig::KEY_LMS_IP, &[0, 0, 0, 0]).unwrap(); // unroutable
        s.set_u32(AudioConfig::KEY_LMS_PORT, 0).unwrap(); // port 0
        s.set_u32(AudioConfig::KEY_AGAIN, 0x40).unwrap(); // > MAX_ANALOG_GAIN_CODE
        s.set_str(AudioConfig::KEY_LOUD, "x").unwrap(); // not a bool byte... ("x" = 0x78)
    });
    let cfg = resolve_boot_audio(Some(&bad), &defaults);
    assert_eq!(cfg.player_name.as_str(), "Attic");
    assert_eq!(cfg.volume, defaults.volume);
    assert_eq!(cfg.lms_server_ip, defaults.lms_server_ip);
    assert_eq!(cfg.lms_server_port, defaults.lms_server_port);
    assert_eq!(cfg.analog_gain_code, defaults.analog_gain_code);
    assert_eq!(cfg.loudness_enabled, defaults.loudness_enabled);
    // Legacy one-byte AGAIN is still accepted (AudioConfig::load_from compat).
    let legacy = audio_journal(|s| s.set_u8(AudioConfig::KEY_AGAIN, 0x10).unwrap());
    assert_eq!(resolve_boot_audio(Some(&legacy), &defaults).analog_gain_code, 0x10);

    // Wi-Fi: journal beats NVS beats default; a lower layer is not even consulted.
    let journal_wifi = WifiCredentials::new("JournalNet", "jpass").unwrap();
    let (creds, src) = resolve_boot_wifi(
        Some(&journal_wifi),
        || panic!("NVS read although the journal has credentials"),
        || panic!("default used although the journal has credentials"),
    )
    .unwrap();
    assert_eq!((creds, src), (sta("JournalNet", "jpass"), ConfigSource::Journal));

    let (creds, src) = resolve_boot_wifi(
        None,
        || Some(sta("NvsNet", "npass")),
        || panic!("default used although NVS has credentials"),
    )
    .unwrap();
    assert_eq!((creds, src), (sta("NvsNet", "npass"), ConfigSource::Nvs));

    let (creds, src) = resolve_boot_wifi(None, || None, || Some(sta("iot", "dpass"))).unwrap();
    assert_eq!((creds, src), (sta("iot", "dpass"), ConfigSource::Default));
    assert_eq!(resolve_boot_wifi(None, || None, || None), None);

    // Device flow: a committed RAM journal, written to its sector, resolves back at boot.
    let mut image = JournalSector::erased();
    let mut ram = RamJournal::new(&mut image, None, Some(journal_wifi.clone()));
    assert!(ram.commit(&full_cfg));
    let pending = ram.pending_image().unwrap();
    let mut flash = [[0xFFu8; JOURNAL_SECTOR_SIZE]; 2];
    flash[0] = *pending.sector;
    let (index, seq) = FlashJournal::select_dual_sector(&flash[0], &flash[1]).unwrap();
    assert_eq!((index, seq), (0, 1));
    let view = JournalView::new(&flash[index]).unwrap();
    assert_eq!(resolve_boot_audio(Some(&view), &defaults), full_cfg);
    let wifi = WifiCredentials::load_from(&view).unwrap();
    assert_eq!(wifi.as_ref(), Some(&journal_wifi));
}

#[test]
fn test_ram_journal_ping_pong_sequence_and_wifi_carry() {
    let mut image = JournalSector::erased();
    // Blank flash: first image goes to A with sequence 1.
    let mut ram = RamJournal::new(&mut image, None, None);
    assert_eq!((ram.sequence(), ram.target()), (0, JournalSlot::A));
    assert!(ram.pending_image().is_none());
    assert!(ram.commit(&state_for(1)));
    let p = ram.pending_image().unwrap();
    assert_eq!((p.slot, p.sequence), (JournalSlot::A, 1));
    assert_eq!(p.used_len % 4, 0);
    let (_, _, len) = FlashJournal::validate_sector(p.sector).unwrap();
    assert!(p.used_len >= len && p.used_len < len + 4);
    // Two commits before a flash write: same target, sequence keeps counting.
    assert!(ram.commit(&state_for(2)));
    assert_eq!(ram.pending_image().map(|p| (p.slot, p.sequence)), Some((JournalSlot::A, 2)));
    ram.mark_flash_written();
    assert!(ram.pending_image().is_none());
    assert_eq!(ram.target(), JournalSlot::B);
    assert!(ram.commit(&state_for(3)));
    assert_eq!(ram.pending_image().map(|p| (p.slot, p.sequence)), Some((JournalSlot::B, 3)));

    // Boot journal in A at sequence 5: the first write goes to B with 6 and carries the Wi-Fi.
    let wifi = WifiCredentials::new("Studio", "pw").unwrap();
    let mut image = JournalSector::erased();
    let mut ram = RamJournal::new(&mut image, Some((JournalSlot::A, 5)), Some(wifi.clone()));
    assert_eq!((ram.sequence(), ram.target()), (5, JournalSlot::B));
    assert!(ram.commit(&state_for(7)));
    let p = ram.pending_image().unwrap();
    assert_eq!((p.slot, p.sequence), (JournalSlot::B, 6));
    let view = JournalView::new(p.sector).unwrap();
    assert_eq!(AudioConfig::load_from(&view).unwrap(), state_for(7));
    assert_eq!(WifiCredentials::load_from(&view).unwrap(), Some(wifi));

    // An invalid config is refused without touching the image or the pending state.
    let before = *ram.pending_image().unwrap().sector;
    let mut invalid = state_for(8);
    invalid.volume = 150;
    assert!(!ram.commit(&invalid));
    let p = ram.pending_image().unwrap();
    assert_eq!((p.sequence, *p.sector), (6, before));
}

/// Power-cycle model: two flash sectors, boot load -> commit -> flash write -> reboot, with one
/// write torn by power loss. Every boot resolves the newest journal that reached flash.
#[test]
fn test_ping_pong_survives_torn_write_across_reboots() {
    let mut flash = [[0xFFu8; JOURNAL_SECTOR_SIZE]; 2];
    let defaults = first_boot_defaults::audio_config();
    let boot = |flash: &[[u8; JOURNAL_SECTOR_SIZE]; 2]| -> (AudioConfig, Option<(JournalSlot, u32)>) {
        match FlashJournal::select_dual_sector(&flash[0], &flash[1]) {
            Some((i, seq)) => {
                let view = JournalView::new(&flash[i]).unwrap();
                (resolve_boot_audio(Some(&view), &defaults), Some((JournalSlot::from_index(i), seq)))
            }
            None => (defaults.clone(), None),
        }
    };
    let slot_index = |slot: JournalSlot| if slot == JournalSlot::A { 0 } else { 1 };

    for k in 1..=5u32 {
        let (cfg, sel) = boot(&flash);
        if k > 1 {
            assert_eq!(cfg, state_for(k - 1), "boot {k} sees the last completed write");
        }
        let mut image = JournalSector::erased();
        let mut ram = RamJournal::new(&mut image, sel, None);
        assert!(ram.commit(&state_for(k)));
        let p = ram.pending_image().unwrap();
        if let Some((active, _)) = sel {
            assert_ne!(p.slot, active, "never overwrite the newest valid journal");
        }
        flash[slot_index(p.slot)] = *p.sector;
    }
    // Torn write: the target sector is erased and half-written when power drops.
    let (_, sel) = boot(&flash);
    let mut image = JournalSector::erased();
    let mut ram = RamJournal::new(&mut image, sel, None);
    assert!(ram.commit(&state_for(6)));
    let p = ram.pending_image().unwrap();
    let torn = &mut flash[slot_index(p.slot)];
    *torn = [0xFF; JOURNAL_SECTOR_SIZE];
    torn[..p.used_len / 2].copy_from_slice(&p.sector[..p.used_len / 2]);
    let (cfg, _) = boot(&flash);
    assert_eq!(cfg, state_for(5), "the previous journal survives the torn write");
}

#[test]
fn test_first_boot_defaults_are_the_only_basement_identity() {
    let d = first_boot_defaults::audio_config();
    assert_eq!(d.player_name.as_str(), "BASEMENT");
    assert_eq!(first_boot_defaults::LMS_SERVER_IP, [10, 0, 0, 10]);
    assert_eq!(first_boot_defaults::LMS_SERVER_PORT, 3483);
    assert_eq!(first_boot_defaults::WIFI_SSID, "iot");
    assert!(d.validate().is_ok());
}
