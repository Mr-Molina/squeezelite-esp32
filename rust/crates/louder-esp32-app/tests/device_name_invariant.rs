//! The device name is a build-time choice (`SL_DEVICE_NAME`, default
//! `"BASEMENT"`) that lives in `board::first_boot_defaults::PLAYER_NAME`.
//! These files must take it from there, so a DEVBENCH image never announces
//! itself as the live device.

const FILES: [(&str, &str); 3] = [
    ("main.rs", include_str!("../src/main.rs")),
    ("bt_a2dp.rs", include_str!("../src/bt_a2dp.rs")),
    ("config_handle.rs", include_str!("../src/config_handle.rs")),
];

#[test]
fn device_name_literal_lives_only_in_board() {
    for (name, src) in FILES {
        let hits: Vec<_> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("\"BASEMENT\"") && !l.contains("assert"))
            .map(|(i, _)| i + 1)
            .collect();
        assert!(
            hits.is_empty(),
            "{name} hard-codes the device name at lines {hits:?}"
        );
    }
}
