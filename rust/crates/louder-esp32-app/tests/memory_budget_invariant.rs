//! Memory re-budget invariants (work order F3).
//!
//! The device bring-up in `main.rs` is Xtensa-only and cannot run on the host,
//! so these tests read the source: they fail when internal RAM that F3
//! reclaimed is spent again by the same construct.

const MAIN_SRC: &str = include_str!("../src/main.rs");

/// The board always has PSRAM. The internal-RAM stand-ins for the stream and
/// output rings and for the two large TCP receive buffers are gone; boot
/// resets into recovery when PSRAM is missing.
#[test]
fn no_psram_absent_fallback_buffers() {
    for field in ["ring_storage", "bss_stream_storage", "bss_output_storage"] {
        assert!(
            !MAIN_SRC.contains(field),
            "main.rs still has the PSRAM-absent fallback `{field}`"
        );
    }
    assert!(
        MAIN_SRC.contains("[ANTI-BRICK] PSRAM missing"),
        "main.rs must reset into recovery when PSRAM is missing"
    );
}

/// Every TCP socket buffer is allocated from the PSRAM heap region at
/// bring-up; no static socket buffer lives in internal RAM.
#[test]
fn socket_buffers_are_not_static() {
    for name in ["FirmwareBssBuffers", "BSS_BUFFERS"] {
        assert!(
            !MAIN_SRC.contains(name),
            "main.rs still has the static socket buffers `{name}`"
        );
    }
}

/// The line before a `static` declaration, i.e. its attribute if it has one.
fn line_before_static(name: &str) -> &'static str {
    let needle = format!("static mut {name}:");
    let at = MAIN_SRC
        .find(&needle)
        .unwrap_or_else(|| panic!("main.rs has no `{needle}`"));
    MAIN_SRC[..at]
        .trim_end()
        .lines()
        .last()
        .unwrap_or("")
        .trim()
}

/// The settings journal state and the config handle sit in `.dram2_uninit`,
/// not in `.bss`, where every byte comes out of the main stack.
#[test]
fn config_statics_are_in_dram2() {
    for name in ["JOURNAL_IMAGE", "RAM_JOURNAL", "CONFIG_HANDLE"] {
        assert_eq!(
            line_before_static(name),
            "#[unsafe(link_section = \".dram2_uninit\")]",
            "`{name}` must be placed in .dram2_uninit"
        );
    }
}

/// The AES decryption tables are read-only data in flash; they are not copied
/// into internal RAM (`.dram1`) at boot.
#[test]
fn aes_tables_are_not_in_dram() {
    const RAOP_CRYPTO_SRC: &str = include_str!("../../squeezelite-raop/src/crypto.rs");
    assert!(
        !RAOP_CRYPTO_SRC.contains("link_section"),
        "squeezelite-raop/src/crypto.rs places a table in a RAM section"
    );
}
