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
}
