//! `squeezelite-config`
//!
//! A zero-allocation, `no_std` pure-Rust key-value configuration storage and
//! Flash journal engine for Squeezelite-ESP32 / Louder-ESP32-Plus.
//!
//! Provides:
//! - Strongly typed `KeyValueStorage` trait with type-safe accessors.
//! - In-memory bounded `MemConfigStore` backed by `heapless`.
//! - Dual-CRC32 validated `FlashJournal` serialization for power-loss
//!   resilient Flash storage.
//! - Domain configuration models: `WifiCredentials` and `AudioConfig`.

#![no_std]
#![forbid(unsafe_code)]

mod config_core;

pub use config_core::{
    crc32, crc32_update, AudioConfig, ConfigError, DualSectorFlashStore, FlashJournal, JournalView,
    KeyValueStorage, MemConfigStore, SectorWriter, StorageError, WifiCredentials,
};
