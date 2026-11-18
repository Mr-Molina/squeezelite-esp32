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
