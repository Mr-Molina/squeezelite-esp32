//! Property-based fuzz tests for `squeezelite-config`
//!
//! Verifies zero-panic guarantees, roundtrip fidelity, and corruption resistance
//! across arbitrary keys, values, and sector bit-flips.

use proptest::prelude::*;
use squeezelite_config::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Invariant: In-memory store preserves arbitrary byte payloads under valid keys.
    #[test]
    fn fuzz_mem_store_roundtrip(
        key in "[a-zA-Z0-9_\\.]{1,24}",
        val in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        let mut store = MemConfigStore::<16>::new();
        let set_res = store.set(&key, &val);
        prop_assert!(set_res.is_ok());

        let mut read_buf = [0u8; 128];
        let len = store.get(&key, &mut read_buf).unwrap();
        prop_assert_eq!(&read_buf[..len], &val[..]);
    }

    /// Invariant: Wi-Fi credentials roundtrip preserves arbitrary SSIDs and passphrases.
    #[test]
    fn fuzz_wifi_credentials_roundtrip(
        ssid in "[a-zA-Z0-9_\\- ]{1,32}",
        pass in "[a-zA-Z0-9!@#$%^&*()]{0,64}",
        auto in any::<bool>(),
        ip in proptest::option::of(any::<[u8; 4]>()),
        gw in proptest::option::of(any::<[u8; 4]>()),
    ) {
        let mut store = MemConfigStore::<16>::new();
        let mut creds = WifiCredentials::new(&ssid, &pass).unwrap();
        creds.autoconnect = auto;
        creds.static_ip = ip;
        creds.gateway = gw;

        prop_assert!(creds.save_to(&mut store).is_ok());

        let loaded = WifiCredentials::load_from(&store).unwrap();
        prop_assert!(loaded.is_some());
        let l = loaded.unwrap();
        prop_assert_eq!(l.ssid.as_str(), ssid.as_str());
        prop_assert_eq!(l.passphrase.as_str(), pass.as_str());
        prop_assert_eq!(l.autoconnect, auto);
        prop_assert_eq!(l.static_ip, ip);
        prop_assert_eq!(l.gateway, gw);
    }

    /// Invariant: FlashJournal serializes and deserializes arbitrary store states without data loss.
    #[test]
    fn fuzz_flash_journal_roundtrip(
        k1 in "[a-z]{1,8}",
        v1 in proptest::collection::vec(any::<u8>(), 0..64),
        k2 in "[A-Z]{1,8}",
        v2 in proptest::collection::vec(any::<u8>(), 0..64),
        vol in any::<u8>(),
    ) {
        // "vol" is the fixed key this property writes last; a generated k1 equal to it would be
        // overwritten by the volume byte and could never read back as v1.
        prop_assume!(k1 != "vol");
        let mut store = MemConfigStore::<16>::new();
        let _ = store.set(&k1, &v1);
        let _ = store.set(&k2, &v2);
        let _ = store.set_u8("vol", vol);

        let mut sector = [0u8; 2048];
        let written = FlashJournal::save_to_sector(&store, &mut sector).unwrap();

        let mut restored = MemConfigStore::<16>::new();
        let count = FlashJournal::load_from_sector(&sector[..written], &mut restored).unwrap();
        prop_assert_eq!(count, store.len());

        let mut buf = [0u8; 64];
        let l1 = restored.get(&k1, &mut buf).unwrap();
        prop_assert_eq!(&buf[..l1], &v1[..]);

        let l2 = restored.get(&k2, &mut buf).unwrap();
        prop_assert_eq!(&buf[..l2], &v2[..]);

        prop_assert_eq!(restored.get_u8("vol").unwrap(), Some(vol));
    }

    /// Invariant: FlashJournal must detect ANY single bit flip in the sector as corrupted.
    #[test]
    fn fuzz_flash_journal_bitflip_detection(
        val in proptest::collection::vec(any::<u8>(), 10..64),
        corrupt_offset_pct in 0..100usize,
        bit_idx in 0..8u8,
    ) {
        let mut store = MemConfigStore::<16>::new();
        let _ = store.set("key", &val);

        let mut sector = [0u8; 512];
        let written = FlashJournal::save_to_sector(&store, &mut sector).unwrap();

        // Corrupt one bit in the valid written range
        let offset = (corrupt_offset_pct * written) / 100;
        let clamped_offset = offset.min(written - 1);
        sector[clamped_offset] ^= 1 << bit_idx;

        let mut restored = MemConfigStore::<16>::new();
        let res = FlashJournal::load_from_sector(&sector[..written], &mut restored);

        // Must either fail with CorruptedData or InvalidFormat, but NEVER succeed or panic
        prop_assert!(res.is_err(), "Bitflip at offset {} was not detected!", clamped_offset);
    }

    /// Invariant: Dual-sector journal always recovers the older valid sector if the new sector is torn.
    #[test]
    fn fuzz_dual_sector_torn_write_recovery(
        v1 in proptest::collection::vec(any::<u8>(), 1..32),
        v2 in proptest::collection::vec(any::<u8>(), 1..32),
        tear_pct in 1..99usize,
    ) {
        let mut store = MemConfigStore::<16>::new();
        let _ = store.set("data", &v1);

        let mut sector_a = [0u8; 512];
        let mut sector_b = [0u8; 512];

        // Valid write to A
        let (wrote_b, _n_a) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
        prop_assert!(!wrote_b);

        // Update store with v2
        let _ = store.set("data", &v2);

        // Write to B
        let (wrote_b2, n_b) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
        prop_assert!(wrote_b2);

        // Simulate torn write to B by truncating or zeroing the tail after tear_pct
        let tear_point = (tear_pct * n_b) / 100;
        sector_b[tear_point..].fill(0xFF); // Flash unprogrammed state is 0xFF

        // Dual sector load MUST recover v1 from Sector A without error
        let mut recovered = MemConfigStore::<16>::new();
        let count = FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut recovered).unwrap();
        prop_assert_eq!(count, 1);

        let mut buf = [0u8; 32];
        let len = recovered.get("data", &mut buf).unwrap();
        prop_assert_eq!(&buf[..len], &v1[..]);
    }
}

#[test]
fn test_dual_sector_sequence_wraparound() {
    let mut store = MemConfigStore::<16>::new();
    store.set_str("k", "v_max").unwrap();

    let mut sector_a = [0u8; 512];
    let mut sector_b = [0u8; 512];

    // Sector A at u32::MAX
    let bytes_a = FlashJournal::save_to_sector_with_seq(&store, &mut sector_a, u32::MAX).unwrap();
    assert!(bytes_a > FlashJournal::HEADER_SIZE);

    // Save to dual sectors: since sector A is u32::MAX and sector B is unwritten (0s),
    // save_to_dual_sectors writes to sector B with u32::MAX.wrapping_add(1) = 0
    store.set_str("k", "v_zero").unwrap();
    let (wrote_b, bytes_b) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
    assert!(wrote_b);
    assert!(bytes_b > FlashJournal::HEADER_SIZE);

    let (seq_b, _, _) = FlashJournal::read_sector_meta(&sector_b).unwrap();
    assert_eq!(seq_b, 0);

    // Load from dual sectors selects seq 0 over u32::MAX
    let mut loaded = MemConfigStore::<16>::new();
    FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut loaded).unwrap();
    let mut buf = [0u8; 16];
    let len = loaded.get("k", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"v_zero");

    // Next save writes to sector A with seq 0.wrapping_add(1) = 1
    store.set_str("k", "v_one").unwrap();
    let (wrote_b2, _) = FlashJournal::save_to_dual_sectors(&store, &mut sector_a, &mut sector_b).unwrap();
    assert!(!wrote_b2);
    let (seq_a2, _, _) = FlashJournal::read_sector_meta(&sector_a).unwrap();
    assert_eq!(seq_a2, 1);

    let mut loaded2 = MemConfigStore::<16>::new();
    FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut loaded2).unwrap();
    let len2 = loaded2.get("k", &mut buf).unwrap();
    assert_eq!(&buf[..len2], b"v_one");
}

#[test]
fn test_dual_sector_fallback_on_entry_corruption_in_newer_sector() {
    let mut store_a = MemConfigStore::<16>::new();
    store_a.set_str("k", "v_old").unwrap();

    let mut store_b = MemConfigStore::<16>::new();
    store_b.set_str("k", "v_new").unwrap();

    let mut sector_a = [0u8; 512];
    let mut sector_b = [0u8; 512];

    FlashJournal::save_to_sector_with_seq(&store_a, &mut sector_a, 1).unwrap();
    let written_b = FlashJournal::save_to_sector_with_seq(&store_b, &mut sector_b, 2).unwrap();

    // In newer sector B: corrupt the entry CRC (offset 16 + 3)
    sector_b[FlashJournal::HEADER_SIZE + 3] ^= 0xA5;
    // Recompute the sector CRC over header[0..12] and payload[16..written_b] so sector metadata is Ok
    let mut crc = 0xFFFF_FFFF;
    crc = crc32_update(crc, &sector_b[0..12]);
    crc = crc32_update(crc, &sector_b[FlashJournal::HEADER_SIZE..written_b]);
    let sector_crc = !crc;
    sector_b[12..16].copy_from_slice(&sector_crc.to_be_bytes());

    // Both sectors pass read_sector_meta
    assert!(FlashJournal::read_sector_meta(&sector_a).is_ok());
    assert!(FlashJournal::read_sector_meta(&sector_b).is_ok());

    // Loading from sector B directly fails with CorruptedData
    let mut tmp = MemConfigStore::<16>::new();
    assert_eq!(FlashJournal::load_from_sector(&sector_b, &mut tmp), Err(StorageError::CorruptedData));

    // Dual sector load must automatically fall back to intact sector A
    let mut recovered = MemConfigStore::<16>::new();
    let count = FlashJournal::load_from_dual_sectors(&sector_a, &sector_b, &mut recovered).unwrap();
    assert_eq!(count, 1);
    let mut buf = [0u8; 16];
    let len = recovered.get("k", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"v_old");
}

#[test]
fn test_duplicate_key_rejection() {
    let mut sector = [0u8; 512];
    sector[0..4].copy_from_slice(&FlashJournal::MAGIC);
    sector[4] = FlashJournal::VERSION;
    sector[5..7].copy_from_slice(&2u16.to_be_bytes()); // 2 entries
    sector[7] = 0;
    sector[8..12].copy_from_slice(&1u32.to_be_bytes()); // seq 1

    let mut offset = FlashJournal::HEADER_SIZE;
    let k = b"dup_key";
    let v1 = b"val1";
    sector[offset] = k.len() as u8;
    sector[offset + 1..offset + 3].copy_from_slice(&(v1.len() as u16).to_be_bytes());
    let mut c1 = 0xFFFF_FFFF;
    c1 = crc32_update(c1, &[k.len() as u8]);
    c1 = crc32_update(c1, &(v1.len() as u16).to_be_bytes());
    c1 = crc32_update(c1, k);
    c1 = crc32_update(c1, v1);
    sector[offset + 3..offset + 7].copy_from_slice(&(!c1).to_be_bytes());
    offset += 7;
    sector[offset..offset + k.len()].copy_from_slice(k);
    offset += k.len();
    sector[offset..offset + v1.len()].copy_from_slice(v1);
    offset += v1.len();

    // Duplicate entry with exact same key
    let v2 = b"val2";
    sector[offset] = k.len() as u8;
    sector[offset + 1..offset + 3].copy_from_slice(&(v2.len() as u16).to_be_bytes());
    let mut c2 = 0xFFFF_FFFF;
    c2 = crc32_update(c2, &[k.len() as u8]);
    c2 = crc32_update(c2, &(v2.len() as u16).to_be_bytes());
    c2 = crc32_update(c2, k);
    c2 = crc32_update(c2, v2);
    sector[offset + 3..offset + 7].copy_from_slice(&(!c2).to_be_bytes());
    offset += 7;
    sector[offset..offset + k.len()].copy_from_slice(k);
    offset += k.len();
    sector[offset..offset + v2.len()].copy_from_slice(v2);
    offset += v2.len();

    // Recompute valid sector CRC
    let mut crc = 0xFFFF_FFFF;
    crc = crc32_update(crc, &sector[0..12]);
    crc = crc32_update(crc, &sector[FlashJournal::HEADER_SIZE..offset]);
    sector[12..16].copy_from_slice(&(!crc).to_be_bytes());

    // read_sector_meta passes because lengths and sector CRC are valid
    assert!(FlashJournal::read_sector_meta(&sector[..offset]).is_ok());

    // load_from_sector must reject the duplicate key with InvalidFormat
    let mut store = MemConfigStore::<16>::new();
    let res = FlashJournal::load_from_sector(&sector[..offset], &mut store);
    assert_eq!(res, Err(StorageError::InvalidFormat));
}

#[test]
fn test_wifi_credentials_optional_ip_key_removal_when_none() {
    let mut store = MemConfigStore::<16>::new();
    let mut creds = WifiCredentials::new("HomeAP", "SecretPass").unwrap();
    creds.static_ip = Some([192, 168, 1, 100]);
    creds.gateway = Some([192, 168, 1, 1]);
    creds.subnet = Some([255, 255, 255, 0]);
    creds.dns = Some([1, 1, 1, 1]);

    creds.save_to(&mut store).unwrap();
    assert!(store.contains(WifiCredentials::KEY_IP));
    assert!(store.contains(WifiCredentials::KEY_GW));
    assert!(store.contains(WifiCredentials::KEY_MASK));
    assert!(store.contains(WifiCredentials::KEY_DNS));

    // Update credentials with None for static IP and gateway/subnet/dns
    creds.static_ip = None;
    creds.gateway = None;
    creds.subnet = None;
    creds.dns = None;

    creds.save_to(&mut store).unwrap();
    assert!(!store.contains(WifiCredentials::KEY_IP));
    assert!(!store.contains(WifiCredentials::KEY_GW));
    assert!(!store.contains(WifiCredentials::KEY_MASK));
    assert!(!store.contains(WifiCredentials::KEY_DNS));

    let loaded = WifiCredentials::load_from(&store).unwrap().unwrap();
    assert_eq!(loaded.static_ip, None);
    assert_eq!(loaded.gateway, None);
    assert_eq!(loaded.subnet, None);
    assert_eq!(loaded.dns, None);
}
