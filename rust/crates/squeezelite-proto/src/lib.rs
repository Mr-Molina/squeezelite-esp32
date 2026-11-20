//! `squeezelite-proto`
//!
//! A zero-allocation, `no_std` SlimProto network protocol engine for connecting
//! to Logitech Media Server (LMS) / Lyrion Music Server.
//!
//! Handles decoding of server packets (`strm`, `audg`, `setd`, `flsh`, `body`)
//! and encoding of client telemetry packets (`HELO`, `STAT`, `RESP`).

#![no_std]
#![forbid(unsafe_code)]

mod wire;

pub use wire::{
    build_capabilities, next_backoff_ms, sample_rate_from_index, AudioFormat, AutostartMode,
    BuildError, DeviceSetting, DisconnectReason, DiscoveredServer, DiscoveryBuilder,
    DiscoveryParser, Endianness, ParseError, ServerCommand, SlimprotoBuilder, SlimprotoParser,
    StatBuilder, StatEvent, StatReport, StreamCommandType, TransitionType, OPCODE_AUDE,
    OPCODE_AUDG, OPCODE_BODY, OPCODE_CONT, OPCODE_DSCO, OPCODE_FLSH, OPCODE_SERV, OPCODE_SETD,
    OPCODE_STRM, OPCODE_VERS, SQUEEZENETWORK_MARKER_IP, STAT_FRAME_SIZE, STAT_PAYLOAD_SIZE,
    STRM_FLAG_CONT_WAIT, STRM_FLAG_SERVER_REBOOT, SYNCGROUP_ID_LEN,
};
