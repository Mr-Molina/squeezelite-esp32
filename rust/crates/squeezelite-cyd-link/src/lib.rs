//! `squeezelite-cyd-link`
//!
//! A zero-allocation, `no_std` UART framing and telemetry link protocol
//! between Squeezelite-ESP32 / Louder-ESP32-Plus audio engine and the
//! CYD (Cheap Yellow Display) front-panel touch controller.
//!
//! Text-based, newline-delimited (`\n` or `\r\n`) protocol for easy debugging,
//! human inspection, and serial terminal compatibility.

#![no_std]
#![forbid(unsafe_code)]

mod protocol;

pub use protocol::{
    truncate_utf8, CydCommand, CydLinkError, CydLinkProtocol, CydMessage, LinkError, PlayState,
};
