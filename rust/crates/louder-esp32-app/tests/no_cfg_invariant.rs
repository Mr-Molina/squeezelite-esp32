//! Device/host parity invariant (spec §3.3, Task 7).
//!
//! The coordinator and the pipeline are the single engine shared by host and
//! device. Platform differences live in the I/O trait implementations and in
//! bring-up, never in these two files, so neither may mention `target_arch`.
//! Codec-set differences use the `std-codecs` feature.

const COORDINATOR_SRC: &str = include_str!("../src/coordinator.rs");
const PIPELINE_SRC: &str = include_str!("../src/pipeline.rs");

fn offending_lines(src: &str) -> Vec<(usize, &str)> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| line.contains("target_arch"))
        .map(|(i, line)| (i + 1, line.trim()))
        .collect()
}

#[test]
fn coordinator_has_no_target_arch_cfg() {
    let hits = offending_lines(COORDINATOR_SRC);
    assert!(
        hits.is_empty(),
        "src/coordinator.rs mentions target_arch: {hits:?}"
    );
}

#[test]
fn pipeline_has_no_target_arch_cfg() {
    let hits = offending_lines(PIPELINE_SRC);
    assert!(
        hits.is_empty(),
        "src/pipeline.rs mentions target_arch: {hits:?}"
    );
}

/// Task 10: the persistence logic is platform-free and fully host-tested; the
/// device flash commit and its `runtime-flash-persist` gate live only in
/// `main.rs`.
const CONFIG_HANDLE_SRC: &str = include_str!("../src/config_handle.rs");

#[test]
fn config_handle_has_no_platform_or_feature_cfg() {
    let hits: Vec<_> = CONFIG_HANDLE_SRC
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("target_arch") || line.contains("feature ="))
        .map(|(i, line)| (i + 1, line.trim()))
        .collect();
    assert!(
        hits.is_empty(),
        "src/config_handle.rs has platform/feature gating: {hits:?}"
    );
}

#[test]
fn runtime_flash_persist_gate_only_in_main() {
    let libs = [
        ("config_handle.rs", CONFIG_HANDLE_SRC),
        ("coordinator.rs", COORDINATOR_SRC),
        ("pipeline.rs", PIPELINE_SRC),
        ("http_control.rs", include_str!("../src/http_control.rs")),
        ("board.rs", include_str!("../src/board.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
        ("tasks.rs", include_str!("../src/tasks.rs")),
    ];
    for (name, src) in libs {
        assert!(
            !src.contains("feature = \"runtime-flash-persist\""),
            "{name} gates on runtime-flash-persist"
        );
    }
    assert!(include_str!("../src/main.rs").contains("feature = \"runtime-flash-persist\""));
}

/// F2 (single loop): `main.rs` is the only device path. `tasks.rs` keeps the
/// core-1 executor and the `CTRL`/`EVT` channel types for D1a, nothing of the
/// former core-0 task ring.
#[test]
fn tasks_has_no_core0_executor() {
    let src = include_str!("../src/tasks.rs");
    for gone in [
        "run_core0_executor",
        "fn run_ring",
        "enum RingTask",
        "enum Phase",
        "fn http_serve",
    ] {
        assert!(!src.contains(gone), "tasks.rs still contains `{gone}`");
    }
    assert!(src.contains("fn run_core1_executor"));
}
