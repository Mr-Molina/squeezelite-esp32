# Vendored Crate: `opus-rs` (0.1.40)

- **Upstream Crate**: `opus-rs` (version 0.1.40, crates.io)
- **Author**: jinti <shenjindi@fourz.cn>
- **License**: BSD-3-Clause
- **Repository**: https://github.com/restsend/opus-rs
- **Vendored Date**: 2026-10-28 (Work Order WO-207 / Step S4)

## Purpose
Provides pure-Rust Opus audio codec decoding under `no_std + alloc` for the ESP32 firmware target (`xtensa-esp32-none-elf`) and host platforms.

## Applied Patches for `no_std + alloc`
1. **Manifest (`Cargo.toml`)**:
   - Configured `default-features = false`, dropping unused `std` dependencies and dev dependencies.
   - Declared `libm = "0.2"` as core dependency for `no_std` floating point math operations.
   - Added `alloc` and `heap` features.
2. **Core / Alloc Abstraction (`src/lib.rs`)**:
   - Added `#![no_std]` and `extern crate alloc;`.
   - Imported `alloc::boxed::Box`.
3. **OpusDecoder Stack Footprint Reduction**:
   - Converted `OpusDecoder` internal state (`celt_dec: Box<CeltDecoder>`, `silk_dec: Box<SilkDecoder>`, and scratch `FixedVec` buffers) to use `alloc::boxed::Box`.
   - Keeps `OpusDecoder` struct size under 500 bytes to prevent stack overflows on embedded MCU runtimes while allocating heavy codec working buffers in PSRAM heap.
   - Streamlined `state_ref` and `state_mut` helpers to uniformly dereference boxed codec structures.
