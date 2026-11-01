# Vendored Crate: `lewton` (0.10.2)

- **Upstream Crate**: `lewton` (version 0.10.2, crates.io)
- **Author**: est31 <MTest31@outlook.com> and contributors
- **License**: MIT OR Apache-2.0
- **Repository**: https://github.com/RustAudio/lewton
- **Vendored Date**: 2026-10-28 (Work Order WO-203 / Step S2)

## Purpose
Provides pure-Rust Vorbis decoding under `no_std + alloc` with `libm` for the ESP32 firmware target (`xtensa-esp32-none-elf`) and host platforms.

## Applied Patches for `no_std + alloc`
1. **Manifest (`Cargo.toml`)**:
   - Configured `default-features = false`.
   - Added `libm = "0.2"` dependency for `no_std` floating-point math (`sin`, `cos`, `atan`, `floor`, `sqrt`, `exp`, `exp2`).
   - Configured `byteorder = { version = "1.0", default-features = false }`.
   - Configured `tinyvec = { version = "1.0", default-features = false, features = ["alloc"] }`.
   - Gated `ogg` dependency and `inside_ogg` module behind `std` / `ogg` features.
2. **Core / Alloc Abstraction (`src/lib.rs`, `src/nostd_prelude.rs`)**:
   - Added `#![cfg_attr(not(feature = "std"), no_std)]` and `extern crate alloc;`.
   - Exported `nostd_prelude` with `Box`, `String`, `Vec`, `vec!`, and `F32Ext`.
   - Gated `std::error::Error` implementations under `#[cfg(feature = "std")]`.
   - Switched `Display` implementations to use `core::fmt::Formatter`.
   - Updated `try!` and `eno!` macros to use `::core::result::Result`.
3. **Floating-Point Math (`src/fmath.rs`)**:
   - Provided `F32Ext` trait dispatching to `libm` implementations in `no_std` mode (`sin`, `cos`, `atan`, `floor`, `sqrt`, `fabs`, `exp`, `exp2`).
   - Replaced `std::f32::consts::PI` and `std::f32::consts::FRAC_PI_4` with `core::f32::consts::PI` and `core::f32::consts::FRAC_PI_4`.
4. **Header In-Memory Byte Slicing (`src/header.rs`)**:
   - Implemented `SliceReader` to parse Vorbis comments directly from slice buffers without requiring `std::io::Cursor` or `std::io::Read`.
