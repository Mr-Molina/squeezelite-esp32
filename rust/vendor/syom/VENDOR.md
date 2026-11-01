# Vendored Crate: `syom` (0.7.1)

- **Upstream Crate**: `syom` (version 0.7.1, crates.io)
- **Author**: Evgeny Khodzitsky
- **License**: MIT
- **Repository**: https://github.com/ekhodzitsky/syom
- **Vendored Date**: 2026-10-28 (Work Order WO-199 / Step S1)

## Purpose
Provides pure-Rust AAC decoding (ADTS, LC, HE-AAC, LATM) under `no_std + alloc` for the ESP32 firmware target (`xtensa-esp32-none-elf`) and host platforms.

## Applied Patches for `no_std + alloc`
1. **Manifest (`Cargo.toml`)**:
   - Added `std` feature flag (default enabled).
   - Added `libm = "0.2"` dependency for `no_std` floating point operations.
   - Added `once_cell = { version = "1.21", default-features = false, features = ["race", "alloc"] }` for heap-backed thread-safe lazy statics.
2. **Core / Alloc Abstraction (`src/lib.rs`, `src/nostd_prelude.rs`)**:
   - Added `#![cfg_attr(not(feature = "std"), no_std)]` and `extern crate alloc;`.
   - Gated std-dependent file I/O, streaming encoder, and profiling probes behind `#[cfg(feature = "std")]`.
   - Provided `x86_feature!` macro fallback using compile-time `cfg!(target_feature = ...)`.
3. **Lazy Statics Replacement (`src/lazy.rs`)**:
   - Replaced `std::sync::LazyLock` and `std::sync::OnceLock` with `LazyBox<T>` and `OnceBoxCell<T>` using `once_cell::race::OnceBox`.
   - Values allocate on heap (PSRAM on ESP32), avoiding static `.bss` / internal DRAM exhaustion.
4. **Floating-Point Math (`src/fmath.rs`)**:
   - Provided `F32Math` and `F64Math` traits dispatching to `libm` implementations in `no_std` mode.
