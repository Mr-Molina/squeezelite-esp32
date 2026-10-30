//! Device/host parity invariant (spec §3.3, Task 7).
//!
//! The coordinator and the pipeline are the single engine shared by host and device. Platform
//! differences live in the I/O trait implementations and in bring-up, never in these two files,
//! so neither may mention `target_arch`. Codec-set differences use the `std-codecs` feature.

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
    assert!(hits.is_empty(), "src/coordinator.rs mentions target_arch: {hits:?}");
}

#[test]
fn pipeline_has_no_target_arch_cfg() {
    let hits = offending_lines(PIPELINE_SRC);
    assert!(hits.is_empty(), "src/pipeline.rs mentions target_arch: {hits:?}");
}

/// Task 10: the persistence logic is platform-free and fully host-tested; the device flash
/// commit and its `runtime-flash-persist` gate live only in `tasks.rs` / `main.rs`.
const CONFIG_HANDLE_SRC: &str = include_str!("../src/config_handle.rs");

#[test]
fn config_handle_has_no_platform_or_feature_cfg() {
    let hits: Vec<_> = CONFIG_HANDLE_SRC
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("target_arch") || line.contains("feature ="))
        .map(|(i, line)| (i + 1, line.trim()))
        .collect();
    assert!(hits.is_empty(), "src/config_handle.rs has platform/feature gating: {hits:?}");
}

#[test]
fn runtime_flash_persist_gate_only_in_tasks_and_main() {
    let libs = [
        ("config_handle.rs", CONFIG_HANDLE_SRC),
        ("coordinator.rs", COORDINATOR_SRC),
        ("pipeline.rs", PIPELINE_SRC),
        ("http_control.rs", include_str!("../src/http_control.rs")),
        ("board.rs", include_str!("../src/board.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
    ];
    for (name, src) in libs {
        assert!(
            !src.contains("feature = \"runtime-flash-persist\""),
            "{name} gates on runtime-flash-persist"
        );
    }
    assert!(include_str!("../src/tasks.rs").contains("feature = \"runtime-flash-persist\""));
}
