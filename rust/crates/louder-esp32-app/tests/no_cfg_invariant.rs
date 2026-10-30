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
