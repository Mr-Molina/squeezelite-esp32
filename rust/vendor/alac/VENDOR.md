# Vendored Crate: `alac` (0.5.0)

- **Upstream Crate**: `alac` (version 0.5.0, crates.io)
- **Author**: Edward Barnard <eabarnard@gmail.com>
- **License**: MIT OR Apache-2.0
- **Repository**: https://github.com/ebarnard/alac.rs
- **Vendored Date**: 2026-10-28 (Work Order WO-205 / Step S3)

## Purpose
Provides pure-Rust Apple Lossless Audio Codec (ALAC) decoding under `no_std + alloc` for the ESP32 firmware target (`xtensa-esp32-none-elf`) and host platforms.

## Applied Patches for `no_std + alloc`
1. **Manifest (`Cargo.toml`)**:
   - Configured `default-features = false`, dropping unused `mp4parse` and `caf` dependencies.
   - Removed unused modules `caf.rs`, `mp4.rs`, and `reader.rs`.
   - Zero external dependencies.
2. **Core / Alloc Abstraction (`src/lib.rs`)**:
   - Added `#![no_std]` and `extern crate alloc;`.
   - Switched from `std::fmt` and `std::str::FromStr` to `core::fmt` and `core::str::FromStr`.
3. **BitCursor (`src/bitcursor.rs`)**:
   - Replaced `std::cmp` with `core::cmp`.
4. **Decoder (`src/dec.rs`)**:
   - Replaced `std::cmp::min` with `core::cmp::min`.
   - Imported `alloc::boxed::Box`, `alloc::vec`, and `alloc::vec::Vec`.
