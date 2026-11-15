//! DACP (Digital Audio Control Protocol) remote-control client for AirPlay 1.
//!
//! Extracts `DACP-ID` and `Active-Remote` session headers from RTSP requests
//! and formats HTTP `GET /ctrl-int/1/<command>` requests to control playback
//! on the AirPlay sender.

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::rtsp::RtspRequest;

/// Remote-control commands supported by DACP (`/ctrl-int/1/<command>`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DacpCommand {
    /// Toggle play/pause (`playpause`).
    PlayPause,
    /// Start playback (`play`).
    Play,
    /// Pause playback (`pause`).
    Pause,
    /// Resume paused playback (`playresume`).
    PlayResume,
    /// Stop playback (`stop`).
    Stop,
    /// Advance to next track (`nextitem`).
    NextItem,
    /// Return to previous track (`previtem`).
    PrevItem,
    /// Increase sender volume (`volumeup`).
    VolumeUp,
    /// Decrease sender volume (`volumedown`).
    VolumeDown,
    /// Toggle sender mute state (`mutetoggle`).
    MuteToggle,
    /// Begin fast-rewind (`beginrew`).
    BeginRew,
    /// Begin fast-forward (`beginff`).
    BeginFf,
}

impl DacpCommand {
    /// Returns the URL path action segment for `/ctrl-int/1/<command>`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlayPause => "playpause",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::PlayResume => "playresume",
            Self::Stop => "stop",
            Self::NextItem => "nextitem",
            Self::PrevItem => "previtem",
            Self::VolumeUp => "volumeup",
            Self::VolumeDown => "volumedown",
            Self::MuteToggle => "mutetoggle",
            Self::BeginRew => "beginrew",
            Self::BeginFf => "beginff",
        }
    }

    /// Parses a DACP command token string.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim() {
            "playpause" => Some(Self::PlayPause),
            "play" => Some(Self::Play),
            "pause" => Some(Self::Pause),
            "playresume" => Some(Self::PlayResume),
            "stop" => Some(Self::Stop),
            "nextitem" => Some(Self::NextItem),
            "previtem" => Some(Self::PrevItem),
            "volumeup" => Some(Self::VolumeUp),
            "volumedown" => Some(Self::VolumeDown),
            "mutetoggle" => Some(Self::MuteToggle),
            "beginrew" => Some(Self::BeginRew),
            "beginff" => Some(Self::BeginFf),
            _ => None,
        }
    }
}

/// DACP remote-control session state and HTTP request builder.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct DacpClient {
    /// 64-bit hex `DACP-ID` identifying the sender's `_dacp._tcp.local` service.
    pub dacp_id: String,
    /// `Active-Remote` authentication token required in DACP HTTP headers.
    pub active_remote: String,
}

impl DacpClient {
    /// Creates a new `DacpClient` from `DACP-ID` and `Active-Remote` strings.
    #[must_use]
    pub fn new(dacp_id: &str, active_remote: &str) -> Self {
        Self {
            dacp_id: dacp_id.trim().to_string(),
            active_remote: active_remote.trim().to_string(),
        }
    }

    /// Extracts a `DacpClient` from an `RtspRequest` if `Active-Remote` is
    /// present (with optional `DACP-ID`).
    #[must_use]
    pub fn from_rtsp_request(req: &RtspRequest<'_>) -> Option<Self> {
        let active_remote = req.active_remote?.trim();
        if active_remote.is_empty() {
            return None;
        }
        let dacp_id = req.dacp_id.unwrap_or("").trim();
        Some(Self::new(dacp_id, active_remote))
    }

    /// Returns the `DACP-ID` header value.
    #[must_use]
    pub fn dacp_id(&self) -> &str {
        &self.dacp_id
    }

    /// Returns the `Active-Remote` header value.
    #[must_use]
    pub fn active_remote(&self) -> &str {
        &self.active_remote
    }

    /// Returns the expected mDNS service instance prefix (`iTunes_Ctrl_<ID>`).
    #[must_use]
    pub fn mdns_instance_prefix(&self) -> String {
        format!("iTunes_Ctrl_{}", self.dacp_id)
    }

    /// Builds an HTTP/1.0 `GET /ctrl-int/1/<command>` request packet with the
    /// required `Active-Remote` header.
    #[must_use]
    pub fn build_command_request(&self, command: DacpCommand) -> Option<Vec<u8>> {
        self.build_raw_action_request(command.as_str())
    }

    /// Builds an HTTP/1.0 `GET /ctrl-int/1/setproperty?dmcp.device-volume=...`
    /// request from a linear volume (`0.0..=1.0`).
    #[must_use]
    pub fn build_volume_request(&self, volume_linear: f32) -> Option<Vec<u8>> {
        if volume_linear.is_nan() {
            return None;
        }
        let clamped = volume_linear.clamp(0.0, 1.0);
        let db = if clamped <= 0.0001 {
            -144.0_f32
        } else {
            (clamped - 1.0) * 30.0
        };
        let action = format!("setproperty?dmcp.device-volume={:.4}", db);
        self.build_raw_action_request(&action)
    }

    /// Builds an HTTP/1.0 `GET /ctrl-int/1/<action>` request for an arbitrary
    /// action string. Returns `None` if `action` or `active_remote` is empty,
    /// or if `action`, `dacp_id`, or `active_remote` contains control characters.
    #[must_use]
    pub fn build_raw_action_request(&self, action: &str) -> Option<Vec<u8>> {
        let trimmed = action.trim_start_matches('/');
        if trimmed.is_empty() || self.active_remote.is_empty() || trimmed.contains("..") {
            return None;
        }
        let has_invalid = |s: &str| s.bytes().any(|b| b == b' ' || !(0x20..0x7F).contains(&b));
        if has_invalid(trimmed) || has_invalid(&self.active_remote) || has_invalid(&self.dacp_id) {
            return None;
        }

        let mut req = format!(
            "GET /ctrl-int/1/{} HTTP/1.0\r\nActive-Remote: {}\r\n",
            trimmed, self.active_remote
        );
        if !self.dacp_id.is_empty() {
            let dacp_hdr = format!("DACP-ID: {}\r\n", self.dacp_id);
            req.push_str(&dacp_hdr);
        }
        req.push_str("Connection: close\r\n\r\n");
        Some(req.into_bytes())
    }
}
