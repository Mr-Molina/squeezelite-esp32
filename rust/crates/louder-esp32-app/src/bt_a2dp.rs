//! Pure-Rust Dual-Mode Bluetooth Classic A2DP Sink & SBC Audio Decoder.
//!
//! Standards-compliant implementation providing:
//! - Dual-Mode HCI initialization state machine (Classic BR/EDR + BLE)
//!   advertising
//! - Classic connection management & SSP "Just Works" auto-pairing
//! - HCI ACL multi-fragment reassembly (PB = 0b10/0b00 + 0b01)
//! - L2CAP Signaling engine (CID 0x0001, Echo, Info, Conn, Config, Disconn)
//! - SDP Server (PSM 0x0001) exposing A2DP Audio Sink 0x110B Service Record
//! - AVDTP 1.3 Signaling & Media streaming (PSM 0x0019) + AVRCP (PSM 0x0017)
//! - High-bitrate Bluetooth A2DP SBC audio decoder with polyphase
//!   synthesis filterbank
//!
//! Zero heap allocations, fixed-size buffers, `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![allow(
    clippy::identity_op,
    clippy::needless_range_loop,
    clippy::new_without_default,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::too_many_arguments
)]

#[cfg(test)]
extern crate std;

use core::fmt;
use squeezelite_config::{FlashJournal, KeyValueStorage, MemConfigStore, StorageError};

/// Bluetooth Classic/LE local name (HCI name, EIR, LE advertising/scan
/// response, SDP ServiceName). The BT stack keeps its P0/P1 behaviour
/// (spec §3.1), so this is the player's first-boot name and does not follow a
/// later rename (`setd`); the fixed SDP record layout needs exactly 8 bytes
/// (checked at compile time where the record is built).
const BT_LOCAL_NAME: &[u8] =
    crate::board::first_boot_defaults::PLAYER_NAME.as_bytes();

// ============================================================================
// HCI Transport Trait
// ============================================================================

/// Clean, non-blocking packet transport for Bluetooth HCI frames.
pub trait HciTransport {
    /// Read an incoming HCI packet into `buf`. Returns the number of bytes
    /// read (0 if no packet).
    fn read_packet(&mut self, buf: &mut [u8]) -> usize;
    /// Write an outgoing HCI packet to the controller. Returns true if
    /// written successfully.
    fn write_packet(&mut self, data: &[u8]) -> bool;
    /// Returns true if an incoming packet is ready to be read.
    fn can_read(&self) -> bool {
        true
    }
}

// ============================================================================
// Constants
// ============================================================================

pub const HCI_PKT_COMMAND: u8 = 0x01;
pub const HCI_PKT_ACL: u8 = 0x02;
pub const HCI_PKT_EVENT: u8 = 0x04;

// HCI Opcodes
pub const OPCODE_RESET: u16 = 0x0C03;
pub const OPCODE_SET_EVENT_MASK: u16 = 0x0C01;
pub const OPCODE_WRITE_LOCAL_NAME: u16 = 0x0C13;
pub const OPCODE_WRITE_CLASS_OF_DEVICE: u16 = 0x0C24;
pub const OPCODE_WRITE_SIMPLE_PAIRING_MODE: u16 = 0x0C56;
pub const OPCODE_WRITE_EXTENDED_INQUIRY_RESPONSE: u16 = 0x0C52;
pub const OPCODE_WRITE_INQUIRY_MODE: u16 = 0x0C45;
pub const OPCODE_WRITE_PAGE_SCAN_ACTIVITY: u16 = 0x0C1C;
pub const OPCODE_WRITE_PAGE_SCAN_TYPE: u16 = 0x0C47;
pub const OPCODE_WRITE_INQUIRY_SCAN_ACTIVITY: u16 = 0x0C1E;
pub const OPCODE_WRITE_INQUIRY_SCAN_TYPE: u16 = 0x0C43;
pub const OPCODE_WRITE_DEFAULT_LINK_POLICY_SETTINGS: u16 = 0x080F;
pub const OPCODE_WRITE_SCAN_ENABLE: u16 = 0x0C1A;
pub const OPCODE_LE_SET_ADV_PARAMS: u16 = 0x2006;
pub const OPCODE_LE_SET_ADV_DATA: u16 = 0x2008;
pub const OPCODE_LE_SET_SCAN_RSP_DATA: u16 = 0x2009;
pub const OPCODE_LE_SET_ADV_ENABLE: u16 = 0x200A;

pub const OPCODE_INQUIRY: u16 = 0x0401;
pub const OPCODE_INQUIRY_CANCEL: u16 = 0x0402;
pub const OPCODE_CREATE_CONNECTION: u16 = 0x0405;
pub const OPCODE_ACCEPT_CONNECTION_REQUEST: u16 = 0x0409;
pub const OPCODE_PIN_CODE_REQUEST_REPLY: u16 = 0x040D;
pub const OPCODE_PIN_CODE_REQUEST_NEGATIVE_REPLY: u16 = 0x040E;
pub const OPCODE_LINK_KEY_REQUEST_REPLY: u16 = 0x040B;
pub const OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY: u16 = 0x040C;
pub const OPCODE_IO_CAPABILITY_REQUEST_REPLY: u16 = 0x042B;
pub const OPCODE_USER_CONFIRMATION_REQUEST_REPLY: u16 = 0x042C;
pub const OPCODE_USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY: u16 = 0x042D;

// HCI Event Codes
pub const EVT_INQUIRY_COMPLETE: u8 = 0x01;
pub const EVT_INQUIRY_RESULT: u8 = 0x02;
pub const EVT_CONNECTION_COMPLETE: u8 = 0x03;
pub const EVT_CONNECTION_REQUEST: u8 = 0x04;
pub const EVT_DISCONNECTION_COMPLETE: u8 = 0x05;
pub const EVT_NUM_COMPLETED_PACKETS: u8 = 0x13;
pub const EVT_COMMAND_COMPLETE: u8 = 0x0E;
pub const EVT_COMMAND_STATUS: u8 = 0x0F;
pub const EVT_PIN_CODE_REQUEST: u8 = 0x16;
pub const EVT_LINK_KEY_REQUEST: u8 = 0x17;
pub const EVT_LINK_KEY_NOTIFICATION: u8 = 0x18;
pub const EVT_INQUIRY_RESULT_WITH_RSSI: u8 = 0x22;
pub const EVT_EXTENDED_INQUIRY_RESULT: u8 = 0x2F;
pub const EVT_IO_CAPABILITY_REQUEST: u8 = 0x31;
pub const EVT_USER_CONFIRMATION_REQUEST: u8 = 0x33;

// AVRCP / AVCTP Constants (PSM 0x0017)
pub const AVRCP_PID: u16 = 0x110E;
pub const BT_SIG_COMPANY_ID: [u8; 3] = [0x00, 0x19, 0x58];
pub const AVRCP_CTYPE_CONTROL: u8 = 0x00;
pub const AVRCP_CTYPE_STATUS: u8 = 0x01;
pub const AVRCP_CTYPE_NOTIFY: u8 = 0x03;
pub const AVRCP_RSP_NOT_IMPLEMENTED: u8 = 0x08;
pub const AVRCP_RSP_ACCEPTED: u8 = 0x09;
pub const AVRCP_RSP_REJECTED: u8 = 0x0A;
pub const AVRCP_RSP_STABLE: u8 = 0x0C;
pub const AVRCP_RSP_CHANGED: u8 = 0x0D;
pub const AVRCP_RSP_INTERIM: u8 = 0x0F;
pub const AVRCP_SUBUNIT_PANEL: u8 = 0x48;
pub const AVRCP_OPCODE_VENDOR_DEPENDENT: u8 = 0x00;
pub const AVRCP_OPCODE_PASS_THROUGH: u8 = 0x7C;

pub const AVRCP_PDU_GET_CAPABILITIES: u8 = 0x10;
pub const AVRCP_PDU_GET_ELEMENT_ATTRIBUTES: u8 = 0x20;
pub const AVRCP_PDU_REGISTER_NOTIFICATION: u8 = 0x31;
pub const AVRCP_PDU_SET_ABSOLUTE_VOLUME: u8 = 0x50;

pub const AVRCP_EVENT_PLAYBACK_STATUS_CHANGED: u8 = 0x01;
pub const AVRCP_EVENT_TRACK_CHANGED: u8 = 0x02;
pub const AVRCP_EVENT_VOLUME_CHANGED: u8 = 0x0D;

pub const AVRCP_ATTR_TITLE: u32 = 0x01;
pub const AVRCP_ATTR_ARTIST: u32 = 0x02;
pub const AVRCP_ATTR_ALBUM: u32 = 0x03;
pub const AVRCP_ATTR_TRACK_NUMBER: u32 = 0x04;
pub const AVRCP_ATTR_TOTAL_TRACKS: u32 = 0x05;
pub const AVRCP_ATTR_GENRE: u32 = 0x06;
pub const AVRCP_ATTR_DURATION_MS: u32 = 0x07;
pub const AVRCP_CHARSET_UTF8: u16 = 0x006A;

/// Persistent FlashJournal configuration key for Bluetooth Classic link keys.
pub const BT_BONDS_CONFIG_KEY: &str = "bt.bonds";

// L2CAP
pub const L2CAP_CID_SIGNALING: u16 = 0x0001;
pub const PSM_SDP: u16 = 0x0001;
pub const PSM_AVDTP: u16 = 0x0019;
pub const PSM_AVRCP: u16 = 0x0017;

pub const L2CAP_CMD_CONN_REQ: u8 = 0x02;
pub const L2CAP_CMD_CONN_RSP: u8 = 0x03;
pub const L2CAP_CMD_CONFIG_REQ: u8 = 0x04;
pub const L2CAP_CMD_CONFIG_RSP: u8 = 0x05;
pub const L2CAP_CMD_DISCONN_REQ: u8 = 0x06;
pub const L2CAP_CMD_DISCONN_RSP: u8 = 0x07;
pub const L2CAP_CMD_ECHO_REQ: u8 = 0x08;
pub const L2CAP_CMD_ECHO_RSP: u8 = 0x09;
pub const L2CAP_CMD_INFO_REQ: u8 = 0x0A;
pub const L2CAP_CMD_INFO_RSP: u8 = 0x0B;

// AVDTP Signals
pub const AVDTP_DISCOVER: u8 = 0x01;
pub const AVDTP_GET_CAPABILITIES: u8 = 0x02;
pub const AVDTP_SET_CONFIGURATION: u8 = 0x03;
pub const AVDTP_GET_CONFIGURATION: u8 = 0x04;
pub const AVDTP_RECONFIGURE: u8 = 0x05;
pub const AVDTP_OPEN: u8 = 0x06;
pub const AVDTP_START: u8 = 0x07;
pub const AVDTP_CLOSE: u8 = 0x08;
pub const AVDTP_SUSPEND: u8 = 0x09;
pub const AVDTP_ABORT: u8 = 0x0A;
pub const AVDTP_GET_ALL_CAPABILITIES: u8 = 0x0C;
pub const AVDTP_DELAYREPORT: u8 = 0x0D;

pub const SBC_SYNCWORD: u8 = 0x9C;

// ============================================================================
// Enums and State Definitions
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InitStep {
    Reset,
    SetEventMask,
    WriteLocalName,
    WriteClassOfDevice,
    WriteSimplePairingMode,
    WriteExtendedInquiryResponse,
    WriteInquiryMode,
    WritePageScanActivity,
    WritePageScanType,
    WriteInquiryScanActivity,
    WriteInquiryScanType,
    WriteDefaultLinkPolicySettings,
    WriteScanEnable,
    LeSetAdvertisingParameters,
    LeSetAdvertisingData,
    LeSetScanResponseData,
    LeSetAdvertisingEnable,
    Completed,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChannelType {
    Sdp,
    AvdtpSignaling,
    AvdtpMedia,
    Avrcp,
    Unknown,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChannelState {
    Closed,
    Connecting,
    ConfigWait,
    Configured,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AvdtpStreamState {
    Idle,
    Configured,
    Open,
    Streaming,
}

#[derive(Copy, Clone, Debug)]
pub struct BondEntry {
    pub bd_addr: [u8; 6],
    pub link_key: [u8; 16],
    pub valid: bool,
}

impl BondEntry {
    pub const fn empty() -> Self {
        Self {
            bd_addr: [0; 6],
            link_key: [0; 16],
            valid: false,
        }
    }
}

// ============================================================================
// AVRCP 1.6 Controller & Target (PSM 0x0017 AVCTP)
// ============================================================================

/// AVRCP `PassThrough` (`0x7C`) operation IDs.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AvrcpOperationId {
    VolumeUp = 0x41,
    VolumeDown = 0x42,
    Mute = 0x43,
    Play = 0x44,
    Stop = 0x45,
    Pause = 0x46,
    Rewind = 0x48,
    FastForward = 0x49,
    Forward = 0x4B,
    Backward = 0x4C,
}

impl AvrcpOperationId {
    /// Alias for [`AvrcpOperationId::Forward`] (`0x4B`).
    pub const NEXT: Self = Self::Forward;
    /// Alias for [`AvrcpOperationId::Backward`] (`0x4C`).
    pub const PREV: Self = Self::Backward;

    #[must_use]
    pub const fn from_u8(code: u8) -> Option<Self> {
        match code & 0x7F {
            0x41 => Some(Self::VolumeUp),
            0x42 => Some(Self::VolumeDown),
            0x43 => Some(Self::Mute),
            0x44 => Some(Self::Play),
            0x45 => Some(Self::Stop),
            0x46 => Some(Self::Pause),
            0x48 => Some(Self::Rewind),
            0x49 => Some(Self::FastForward),
            0x4B => Some(Self::Forward),
            0x4C => Some(Self::Backward),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Button state flag inside an AVRCP `PassThrough` (`0x7C`) frame.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AvrcpButtonState {
    Pressed,
    Released,
}

/// Direction/classification of an AVCTP message on `PSM 0x0017`.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AvrcpMessageKind {
    Command,
    Response,
}

/// Track metadata decoded from or encoded into AVRCP `GetElementAttributes`
/// (`0x20` on Bluetooth SIG Company ID `0x001958`).
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AvrcpTrackMetadata {
    title_buf: [u8; 64],
    title_len: usize,
    artist_buf: [u8; 64],
    artist_len: usize,
    album_buf: [u8; 64],
    album_len: usize,
    pub duration_ms: u32,
}

impl AvrcpTrackMetadata {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            title_buf: [0u8; 64],
            title_len: 0,
            artist_buf: [0u8; 64],
            artist_len: 0,
            album_buf: [0u8; 64],
            album_len: 0,
            duration_ms: 0,
        }
    }

    #[must_use]
    pub fn with_metadata(
        title: &str,
        artist: &str,
        album: &str,
        duration_ms: u32,
    ) -> Self {
        let mut meta = Self::new();
        meta.update_title(title);
        meta.update_artist(artist);
        meta.update_album(album);
        meta.duration_ms = duration_ms;
        meta
    }

    pub fn clear(&mut self) {
        self.title_buf.fill(0);
        self.title_len = 0;
        self.artist_buf.fill(0);
        self.artist_len = 0;
        self.album_buf.fill(0);
        self.album_len = 0;
        self.duration_ms = 0;
    }

    pub fn update_title(&mut self, title: &str) {
        let bytes = copy_utf8_prefix(title, &mut self.title_buf);
        self.title_len = bytes;
    }

    pub fn update_artist(&mut self, artist: &str) {
        let bytes = copy_utf8_prefix(artist, &mut self.artist_buf);
        self.artist_len = bytes;
    }

    pub fn update_album(&mut self, album: &str) {
        let bytes = copy_utf8_prefix(album, &mut self.album_buf);
        self.album_len = bytes;
    }

    #[must_use]
    pub fn title(&self) -> &str {
        let len = self.title_len.min(self.title_buf.len());
        core::str::from_utf8(&self.title_buf[..len]).unwrap_or("")
    }

    #[must_use]
    pub fn artist(&self) -> &str {
        let len = self.artist_len.min(self.artist_buf.len());
        core::str::from_utf8(&self.artist_buf[..len]).unwrap_or("")
    }

    #[must_use]
    pub fn album(&self) -> &str {
        let len = self.album_len.min(self.album_buf.len());
        core::str::from_utf8(&self.album_buf[..len]).unwrap_or("")
    }

    #[must_use]
    pub const fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.title_len == 0
            && self.artist_len == 0
            && self.album_len == 0
            && self.duration_ms == 0
    }
}

fn copy_utf8_prefix(src: &str, dst: &mut [u8]) -> usize {
    dst.fill(0);
    let max_len = src.len().min(dst.len());
    let valid_len = src.floor_char_boundary(max_len);
    dst[..valid_len].copy_from_slice(&src.as_bytes()[..valid_len]);
    valid_len
}

fn parse_decimal_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut val = 0u32;
    let mut seen_digit = false;
    for &b in bytes {
        if b == 0 {
            break;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        seen_digit = true;
        val = val.saturating_mul(10).saturating_add(u32::from(b - b'0'));
    }
    if seen_digit {
        Some(val)
    } else {
        None
    }
}

fn format_decimal_u32(mut val: u32, out: &mut [u8; 10]) -> &[u8] {
    if val == 0 {
        out[9] = b'0';
        return &out[9..10];
    }
    let mut pos = 10usize;
    while val > 0 && pos > 0 {
        pos -= 1;
        out[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    &out[pos..10]
}

/// Converts a 7-bit AVRCP absolute volume (`0x00..=0x7F`) to percentage
/// (`0..=100%`).
#[must_use]
pub const fn avrcp_raw_to_volume_pct(raw: u8) -> u8 {
    let clamped = (raw & 0x7F) as u16;
    ((clamped * 100 + 63) / 127) as u8
}

/// Converts a percentage (`0..=100%`) to a 7-bit AVRCP absolute volume
/// (`0x00..=0x7F`).
#[must_use]
pub const fn volume_pct_to_avrcp_raw(pct: u8) -> u8 {
    let clamped = if pct > 100 { 100 } else { pct as u16 };
    ((clamped * 127 + 50) / 100) as u8
}

/// Builds an AVRCP `PassThrough` (`0x7C`) command frame over AVCTP.
pub fn build_avrcp_passthrough_cmd(
    tx_label: u8,
    op: AvrcpOperationId,
    state: AvrcpButtonState,
    out: &mut [u8],
) -> usize {
    if out.len() < 8 {
        return 0;
    }
    let state_bit = match state {
        AvrcpButtonState::Pressed => 0x00,
        AvrcpButtonState::Released => 0x80,
    };
    out[0] = (tx_label & 0x0F) << 4; // Single packet, Command (C/R=0), IPID=0
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_CTYPE_CONTROL;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_PASS_THROUGH;
    out[6] = state_bit | (op.as_u8() & 0x7F);
    out[7] = 0x00; // operation_data_field_length = 0
    8
}

/// Builds an AVRCP `PassThrough` (`0x7C`) response frame over AVCTP.
pub fn build_avrcp_passthrough_rsp(
    tx_label: u8,
    op: AvrcpOperationId,
    state: AvrcpButtonState,
    response_code: u8,
    out: &mut [u8],
) -> usize {
    if out.len() < 8 {
        return 0;
    }
    let state_bit = match state {
        AvrcpButtonState::Pressed => 0x00,
        AvrcpButtonState::Released => 0x80,
    };
    out[0] = ((tx_label & 0x0F) << 4) | 0x02; // Response (C/R=1)
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = response_code & 0x0F;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_PASS_THROUGH;
    out[6] = state_bit | (op.as_u8() & 0x7F);
    out[7] = 0x00;
    8
}

/// Parses an AVRCP `PassThrough` (`0x7C`) AVCTP frame, returning
/// `(tx_label, kind, op, button_state)`.
#[must_use]
pub fn parse_avrcp_passthrough(
    payload: &[u8],
) -> Option<(u8, AvrcpMessageKind, AvrcpOperationId, AvrcpButtonState)> {
    if payload.len() < 8 {
        return None;
    }
    let pid = u16::from_be_bytes([payload[1], payload[2]]);
    if pid != AVRCP_PID || payload[5] != AVRCP_OPCODE_PASS_THROUGH {
        return None;
    }
    let tx_label = (payload[0] >> 4) & 0x0F;
    let kind = if (payload[0] & 0x02) != 0 {
        AvrcpMessageKind::Response
    } else {
        AvrcpMessageKind::Command
    };
    let op = AvrcpOperationId::from_u8(payload[6] & 0x7F)?;
    let state = if (payload[6] & 0x80) != 0 {
        AvrcpButtonState::Released
    } else {
        AvrcpButtonState::Pressed
    };
    Some((tx_label, kind, op, state))
}

/// Builds an AVRCP `SetAbsoluteVolume` (`0x50`) command frame over AVCTP.
pub fn build_avrcp_set_absolute_volume_cmd(
    tx_label: u8,
    volume_raw: u8,
    out: &mut [u8],
) -> usize {
    if out.len() < 14 {
        return 0;
    }
    out[0] = (tx_label & 0x0F) << 4;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_CTYPE_CONTROL;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_SET_ABSOLUTE_VOLUME;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&1u16.to_be_bytes());
    out[13] = volume_raw & 0x7F;
    14
}

/// Builds an AVRCP `SetAbsoluteVolume` (`0x50`) `ACCEPTED` response frame.
pub fn build_avrcp_set_absolute_volume_rsp(
    tx_label: u8,
    volume_raw: u8,
    out: &mut [u8],
) -> usize {
    if out.len() < 14 {
        return 0;
    }
    out[0] = ((tx_label & 0x0F) << 4) | 0x02;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_RSP_ACCEPTED;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_SET_ABSOLUTE_VOLUME;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&1u16.to_be_bytes());
    out[13] = volume_raw & 0x7F;
    14
}

/// Builds an AVRCP `RegisterNotification` (`0x31`) command for
/// `EVENT_VOLUME_CHANGED` (`0x0D`).
pub fn build_avrcp_register_volume_notify_cmd(
    tx_label: u8,
    out: &mut [u8],
) -> usize {
    if out.len() < 18 {
        return 0;
    }
    out[0] = (tx_label & 0x0F) << 4;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_CTYPE_NOTIFY;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_REGISTER_NOTIFICATION;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&5u16.to_be_bytes());
    out[13] = AVRCP_EVENT_VOLUME_CHANGED;
    out[14..18].fill(0);
    18
}

/// Builds an AVRCP `RegisterNotification` (`0x31`) `INTERIM` (`0x0F`) or
/// `CHANGED` (`0x0D`) response for `EVENT_VOLUME_CHANGED` (`0x0D`).
pub fn build_avrcp_volume_notify_rsp(
    tx_label: u8,
    response_code: u8,
    volume_raw: u8,
    out: &mut [u8],
) -> usize {
    if out.len() < 15 {
        return 0;
    }
    out[0] = ((tx_label & 0x0F) << 4) | 0x02;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = response_code & 0x0F;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_REGISTER_NOTIFICATION;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&2u16.to_be_bytes());
    out[13] = AVRCP_EVENT_VOLUME_CHANGED;
    out[14] = volume_raw & 0x7F;
    15
}

/// Builds an AVRCP `GetElementAttributes` (`0x20`) command requesting
/// `Title` (`0x01`), `Artist` (`0x02`), `Album` (`0x03`), and
/// `DurationMs` (`0x07`).
pub fn build_avrcp_get_element_attributes_cmd(
    tx_label: u8,
    out: &mut [u8],
) -> usize {
    // 13 header + 8 identifier + 1 count + 4*4 attr_ids = 38 bytes
    if out.len() < 38 {
        return 0;
    }
    out[0] = (tx_label & 0x0F) << 4;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_CTYPE_STATUS;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_GET_ELEMENT_ATTRIBUTES;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&25u16.to_be_bytes());
    out[13..21].fill(0); // Identifier = PLAYING (0x0)
    out[21] = 4; // 4 attributes requested
    out[22..26].copy_from_slice(&AVRCP_ATTR_TITLE.to_be_bytes());
    out[26..30].copy_from_slice(&AVRCP_ATTR_ARTIST.to_be_bytes());
    out[30..34].copy_from_slice(&AVRCP_ATTR_ALBUM.to_be_bytes());
    out[34..38].copy_from_slice(&AVRCP_ATTR_DURATION_MS.to_be_bytes());
    38
}

/// Builds an AVRCP `GetElementAttributes` (`0x20`) `STABLE` (`0x0C`) response
/// encoding `Title`, `Artist`, `Album`, and `DurationMs`.
pub fn build_avrcp_get_element_attributes_rsp(
    tx_label: u8,
    meta: &AvrcpTrackMetadata,
    out: &mut [u8],
) -> usize {
    if out.len() < 14 {
        return 0;
    }
    let mut dur_scratch = [0u8; 10];
    let dur_bytes = format_decimal_u32(meta.duration_ms, &mut dur_scratch);
    let attrs: [(u32, &[u8]); 4] = [
        (AVRCP_ATTR_TITLE, meta.title().as_bytes()),
        (AVRCP_ATTR_ARTIST, meta.artist().as_bytes()),
        (AVRCP_ATTR_ALBUM, meta.album().as_bytes()),
        (AVRCP_ATTR_DURATION_MS, dur_bytes),
    ];

    let mut pos = 14usize;
    let mut count = 0u8;
    for (attr_id, val) in attrs {
        if pos + 8 + val.len() > out.len() {
            break;
        }
        out[pos..pos + 4].copy_from_slice(&attr_id.to_be_bytes());
        out[pos + 4..pos + 6].copy_from_slice(&AVRCP_CHARSET_UTF8.to_be_bytes());
        out[pos + 6..pos + 8].copy_from_slice(&(val.len() as u16).to_be_bytes());
        pos += 8;
        out[pos..pos + val.len()].copy_from_slice(val);
        pos += val.len();
        count += 1;
    }

    let param_len = (pos - 13) as u16;
    out[0] = ((tx_label & 0x0F) << 4) | 0x02;
    out[1..3].copy_from_slice(&AVRCP_PID.to_be_bytes());
    out[3] = AVRCP_RSP_STABLE;
    out[4] = AVRCP_SUBUNIT_PANEL;
    out[5] = AVRCP_OPCODE_VENDOR_DEPENDENT;
    out[6..9].copy_from_slice(&BT_SIG_COMPANY_ID);
    out[9] = AVRCP_PDU_GET_ELEMENT_ATTRIBUTES;
    out[10] = 0x00;
    out[11..13].copy_from_slice(&param_len.to_be_bytes());
    out[13] = count;
    pos
}

/// Parses an AVRCP `GetElementAttributes` (`0x20`) response (either full AVCTP
/// frame or attribute parameter block) into [`AvrcpTrackMetadata`].
#[must_use]
pub fn parse_avrcp_element_attributes_rsp(
    payload: &[u8],
) -> Option<AvrcpTrackMetadata> {
    let params = if payload.len() >= 14
        && u16::from_be_bytes([payload[1], payload[2]]) == AVRCP_PID
        && payload[5] == AVRCP_OPCODE_VENDOR_DEPENDENT
        && payload[6..9] == BT_SIG_COMPANY_ID
        && payload[9] == AVRCP_PDU_GET_ELEMENT_ATTRIBUTES
    {
        let param_len = u16::from_be_bytes([payload[11], payload[12]]) as usize;
        let end = (13 + param_len).min(payload.len());
        &payload[13..end]
    } else if !payload.is_empty() {
        payload
    } else {
        return None;
    };

    if params.is_empty() {
        return None;
    }
    let num_attrs = params[0] as usize;
    let mut pos = 1usize;
    let mut meta = AvrcpTrackMetadata::new();

    for _ in 0..num_attrs {
        if pos + 8 > params.len() {
            break;
        }
        let attr_id = u32::from_be_bytes([
            params[pos],
            params[pos + 1],
            params[pos + 2],
            params[pos + 3],
        ]);
        let _charset = u16::from_be_bytes([params[pos + 4], params[pos + 5]]);
        let val_len = u16::from_be_bytes([params[pos + 6], params[pos + 7]]) as usize;
        pos += 8;
        if pos + val_len > params.len() {
            break;
        }
        let val_slice = &params[pos..pos + val_len];
        pos += val_len;

        match attr_id {
            AVRCP_ATTR_TITLE => {
                if let Ok(s) = core::str::from_utf8(val_slice) {
                    meta.update_title(s);
                }
            }
            AVRCP_ATTR_ARTIST => {
                if let Ok(s) = core::str::from_utf8(val_slice) {
                    meta.update_artist(s);
                }
            }
            AVRCP_ATTR_ALBUM => {
                if let Ok(s) = core::str::from_utf8(val_slice) {
                    meta.update_album(s);
                }
            }
            AVRCP_ATTR_DURATION_MS => {
                if let Some(ms) = parse_decimal_u32(val_slice) {
                    meta.duration_ms = ms;
                }
            }
            _ => {}
        }
    }

    Some(meta)
}

// ============================================================================
// Persistent Link-Key Bonding Store & Security Policy
// ============================================================================

/// A single bonded Bluetooth Classic peer stored in [`BondingStore`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BondedPeer {
    pub bd_addr: [u8; 6],
    pub link_key: [u8; 16],
    pub key_type: u8,
    pub valid: bool,
    pub lru_tick: u32,
}

impl BondedPeer {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            bd_addr: [0u8; 6],
            link_key: [0u8; 16],
            key_type: 0,
            valid: false,
            lru_tick: 0,
        }
    }
}

/// 4-slot LRU persistent link-key bonding store with binary and hex
/// serialization compatible with [`FlashJournal`] under key `"bt.bonds"`.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BondingStore {
    peers: [BondedPeer; 4],
    clock: u32,
}

impl BondingStore {
    pub const MAGIC: [u8; 4] = *b"BTB1";
    pub const RECORD_BYTES: usize = 23; // 6 bd_addr + 16 link_key + 1 key_type
    pub const BINARY_MAX_BYTES: usize = 5 + 4 * Self::RECORD_BYTES; // 97 bytes

    #[must_use]
    pub const fn new() -> Self {
        Self {
            peers: [BondedPeer::empty(); 4],
            clock: 0,
        }
    }

    pub fn clear(&mut self) {
        self.peers = [BondedPeer::empty(); 4];
        self.clock = 0;
    }

    #[must_use]
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        for peer in &self.peers {
            if peer.valid {
                n += 1;
            }
        }
        n
    }

    #[must_use]
    pub const fn peers(&self) -> &[BondedPeer; 4] {
        &self.peers
    }

    /// Looks up a bonded peer without updating LRU recency.
    #[must_use]
    pub fn peek(&self, bd_addr: &[u8; 6]) -> Option<BondedPeer> {
        for peer in &self.peers {
            if peer.valid && &peer.bd_addr == bd_addr {
                return Some(*peer);
            }
        }
        None
    }

    /// Looks up a bonded peer and touches its LRU timestamp on hit.
    pub fn lookup(&mut self, bd_addr: &[u8; 6]) -> Option<BondedPeer> {
        self.clock = self.clock.wrapping_add(1);
        let tick = self.clock;
        for peer in &mut self.peers {
            if peer.valid && &peer.bd_addr == bd_addr {
                peer.lru_tick = tick;
                return Some(*peer);
            }
        }
        None
    }

    /// Inserts or updates a bonded peer, evicting the least-recently-used slot
    /// when all 4 slots are occupied.
    pub fn upsert(&mut self, bd_addr: [u8; 6], link_key: [u8; 16], key_type: u8) {
        self.clock = self.clock.wrapping_add(1);
        let tick = self.clock;

        for peer in &mut self.peers {
            if peer.valid && peer.bd_addr == bd_addr {
                peer.link_key = link_key;
                peer.key_type = key_type;
                peer.lru_tick = tick;
                return;
            }
        }

        for peer in &mut self.peers {
            if !peer.valid {
                *peer = BondedPeer {
                    bd_addr,
                    link_key,
                    key_type,
                    valid: true,
                    lru_tick: tick,
                };
                return;
            }
        }

        let mut lru_idx = 0usize;
        let mut min_tick = self.peers[0].lru_tick;
        for idx in 1..4 {
            if self.peers[idx].lru_tick < min_tick {
                min_tick = self.peers[idx].lru_tick;
                lru_idx = idx;
            }
        }
        self.peers[lru_idx] = BondedPeer {
            bd_addr,
            link_key,
            key_type,
            valid: true,
            lru_tick: tick,
        };
    }

    /// Removes a peer from the bonding store.
    pub fn remove(&mut self, bd_addr: &[u8; 6]) -> bool {
        for peer in &mut self.peers {
            if peer.valid && &peer.bd_addr == bd_addr {
                *peer = BondedPeer::empty();
                return true;
            }
        }
        false
    }

    /// Encodes the valid peers in LRU order (oldest to newest) into a compact
    /// binary record (`<= 97` bytes) suitable for `FlashJournal` key
    /// `"bt.bonds"`.
    pub fn encode_binary(&self, out: &mut [u8]) -> usize {
        let valid_count = self.count();
        let needed = 5 + valid_count * Self::RECORD_BYTES;
        if out.len() < needed {
            return 0;
        }
        out[0..4].copy_from_slice(&Self::MAGIC);
        out[4] = valid_count as u8;

        // Sort valid indices by lru_tick ascending so decode preserves LRU order
        let mut indices = [0usize; 4];
        let mut n = 0usize;
        for idx in 0..4 {
            if self.peers[idx].valid {
                indices[n] = idx;
                n += 1;
            }
        }
        for i in 0..n {
            for j in (i + 1)..n {
                if self.peers[indices[j]].lru_tick < self.peers[indices[i]].lru_tick {
                    indices.swap(i, j);
                }
            }
        }

        let mut pos = 5usize;
        for k in 0..n {
            let p = &self.peers[indices[k]];
            out[pos..pos + 6].copy_from_slice(&p.bd_addr);
            out[pos + 6..pos + 22].copy_from_slice(&p.link_key);
            out[pos + 22] = p.key_type;
            pos += Self::RECORD_BYTES;
        }
        pos
    }

    /// Decodes a [`BondingStore`] from a binary slice produced by
    /// [`BondingStore::encode_binary`].
    pub fn decode_binary(data: &[u8]) -> Result<Self, StorageError> {
        if data.len() < 5 || data[0..4] != Self::MAGIC {
            return Err(StorageError::InvalidFormat);
        }
        let count = data[4] as usize;
        if count > 4 || data.len() < 5 + count * Self::RECORD_BYTES {
            return Err(StorageError::InvalidFormat);
        }
        let mut store = Self::new();
        let mut pos = 5usize;
        for _ in 0..count {
            let mut bd_addr = [0u8; 6];
            bd_addr.copy_from_slice(&data[pos..pos + 6]);
            let mut link_key = [0u8; 16];
            link_key.copy_from_slice(&data[pos + 6..pos + 22]);
            let key_type = data[pos + 22];
            store.upsert(bd_addr, link_key, key_type);
            pos += Self::RECORD_BYTES;
        }
        Ok(store)
    }

    /// Encodes the bonding store as uppercase ASCII hex into `out`.
    pub fn encode_hex(&self, out: &mut [u8]) -> usize {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut raw = [0u8; Self::BINARY_MAX_BYTES];
        let raw_len = self.encode_binary(&mut raw);
        if raw_len == 0 || out.len() < raw_len * 2 {
            return 0;
        }
        for i in 0..raw_len {
            out[i * 2] = HEX[(raw[i] >> 4) as usize];
            out[i * 2 + 1] = HEX[(raw[i] & 0x0F) as usize];
        }
        raw_len * 2
    }

    /// Decodes a [`BondingStore`] from an ASCII hex string produced by
    /// [`BondingStore::encode_hex`].
    pub fn decode_hex(hex_str: &str) -> Result<Self, StorageError> {
        let bytes = hex_str.as_bytes();
        if !bytes.len().is_multiple_of(2)
            || bytes.len() / 2 > Self::BINARY_MAX_BYTES
        {
            return Err(StorageError::InvalidFormat);
        }
        let mut raw = [0u8; Self::BINARY_MAX_BYTES];
        let raw_len = bytes.len() / 2;
        for i in 0..raw_len {
            let hi = decode_hex_nibble(bytes[i * 2])
                .ok_or(StorageError::InvalidFormat)?;
            let lo = decode_hex_nibble(bytes[i * 2 + 1])
                .ok_or(StorageError::InvalidFormat)?;
            raw[i] = (hi << 4) | lo;
        }
        Self::decode_binary(&raw[..raw_len])
    }

    /// Saves the bonding store under key [`BT_BONDS_CONFIG_KEY`] (`"bt.bonds"`)
    /// in any [`KeyValueStorage`] implementation.
    pub fn save_to_storage<S: KeyValueStorage>(
        &self,
        storage: &mut S,
    ) -> Result<(), StorageError> {
        let mut buf = [0u8; Self::BINARY_MAX_BYTES];
        let n = self.encode_binary(&mut buf);
        if n == 0 {
            return Err(StorageError::BufferTooSmall);
        }
        storage.set(BT_BONDS_CONFIG_KEY, &buf[..n])
    }

    /// Loads the bonding store from key [`BT_BONDS_CONFIG_KEY`] (`"bt.bonds"`)
    /// in any [`KeyValueStorage`] implementation.
    pub fn load_from_storage<S: KeyValueStorage>(
        storage: &S,
    ) -> Result<Self, StorageError> {
        let mut buf = [0u8; 128];
        let n = storage.get(BT_BONDS_CONFIG_KEY, &mut buf)?;
        Self::decode_binary(&buf[..n])
    }

    /// Persists the bonding store into `store` under `"bt.bonds"` and commits
    /// the store to a [`FlashJournal`] sector image.
    pub fn save_to_journal_sector<const N: usize>(
        &self,
        store: &mut MemConfigStore<N>,
        sector: &mut [u8],
        sequence: u32,
    ) -> Result<usize, StorageError> {
        self.save_to_storage(store)?;
        FlashJournal::save_to_sector_with_seq(store, sector, sequence)
    }

    /// Loads a [`FlashJournal`] sector image into `store` and deserializes
    /// the [`BondingStore`] from `"bt.bonds"`.
    pub fn load_from_journal_sector<const N: usize>(
        sector: &[u8],
        store: &mut MemConfigStore<N>,
    ) -> Result<Self, StorageError> {
        let _ = FlashJournal::load_from_sector(sector, store)?;
        Self::load_from_storage(store)
    }
}

const fn decode_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Bluetooth Classic Secure Simple Pairing (SSP) / PIN security policy.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BtPairingPolicy {
    /// SSP "Just Works" (`NoInputNoOutput = 0x03`, general bonding) + PIN fallback.
    SspJustWorks,
    /// SSP Numeric Comparison (`DisplayYesNo = 0x01`, MITM required).
    SspNumericComparison,
    /// Legacy fixed PIN code pairing only (SSP disabled).
    LegacyPinOnly,
    /// Reject new unbonded pairings; allow only peers in [`BondingStore`].
    BondedOnly,
}

/// Configurable Bluetooth local device name and SSP/PIN pairing policy.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BtSecurityPolicy {
    device_name_buf: [u8; 32],
    device_name_len: usize,
    pin_code_buf: [u8; 16],
    pin_code_len: usize,
    pub pairing_policy: BtPairingPolicy,
    pub discoverable: bool,
}

impl BtSecurityPolicy {
    #[must_use]
    pub const fn new() -> Self {
        let mut name_buf = [0u8; 32];
        let mut i = 0;
        while i < BT_LOCAL_NAME.len() && i < 32 {
            name_buf[i] = BT_LOCAL_NAME[i];
            i += 1;
        }
        let mut pin_buf = [0u8; 16];
        pin_buf[0] = b'0';
        pin_buf[1] = b'0';
        pin_buf[2] = b'0';
        pin_buf[3] = b'0';
        Self {
            device_name_buf: name_buf,
            device_name_len: i,
            pin_code_buf: pin_buf,
            pin_code_len: 4,
            pairing_policy: BtPairingPolicy::SspJustWorks,
            discoverable: true,
        }
    }

    pub fn update_device_name(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return;
        }
        let len = copy_utf8_prefix(trimmed, &mut self.device_name_buf);
        self.device_name_len = len;
    }

    #[must_use]
    pub fn device_name(&self) -> &str {
        let bytes = self.device_name_bytes();
        core::str::from_utf8(bytes).unwrap_or("BASEMENT")
    }

    #[must_use]
    pub fn device_name_bytes(&self) -> &[u8] {
        if self.device_name_len == 0 {
            BT_LOCAL_NAME
        } else {
            let len = self.device_name_len.min(self.device_name_buf.len());
            &self.device_name_buf[..len]
        }
    }

    pub fn update_pin_code(&mut self, pin: &[u8]) {
        if pin.is_empty() || pin.len() > 16 {
            return;
        }
        self.pin_code_buf.fill(0);
        self.pin_code_buf[..pin.len()].copy_from_slice(pin);
        self.pin_code_len = pin.len();
    }

    #[must_use]
    pub fn pin_code(&self) -> &[u8] {
        if self.pin_code_len == 0 {
            b"0000"
        } else {
            let len = self.pin_code_len.min(self.pin_code_buf.len());
            &self.pin_code_buf[..len]
        }
    }

    #[must_use]
    pub const fn ssp_enabled(&self) -> bool {
        !matches!(self.pairing_policy, BtPairingPolicy::LegacyPinOnly)
    }

    #[must_use]
    pub const fn io_capability_byte(&self) -> u8 {
        match self.pairing_policy {
            BtPairingPolicy::SspNumericComparison => 0x01, // DisplayYesNo
            BtPairingPolicy::SspJustWorks
            | BtPairingPolicy::LegacyPinOnly
            | BtPairingPolicy::BondedOnly => 0x03, // NoInputNoOutput
        }
    }

    #[must_use]
    pub const fn auth_requirements_byte(&self) -> u8 {
        match self.pairing_policy {
            BtPairingPolicy::SspNumericComparison => 0x05, // MITM Required - General Bonding
            BtPairingPolicy::SspJustWorks
            | BtPairingPolicy::LegacyPinOnly
            | BtPairingPolicy::BondedOnly => 0x04, // MITM Not Required - General Bonding
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct L2capChannel {
    pub local_cid: u16,
    pub remote_cid: u16,
    pub psm: u16,
    pub state: ChannelState,
    pub channel_type: ChannelType,
    pub handle: u16,
    pub outbound_config_sent: bool,
}

impl L2capChannel {
    pub const fn empty() -> Self {
        Self {
            local_cid: 0,
            remote_cid: 0,
            psm: 0,
            state: ChannelState::Closed,
            channel_type: ChannelType::Unknown,
            handle: 0,
            outbound_config_sent: false,
        }
    }
}

pub struct AclReassembly {
    pub active: bool,
    pub handle: u16,
    pub expected_len: usize,
    pub current_len: usize,
    pub buf: [u8; 1024],
}

impl AclReassembly {
    pub const fn new() -> Self {
        Self {
            active: false,
            handle: 0,
            expected_len: 0,
            current_len: 0,
            buf: [0; 1024],
        }
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.handle = 0;
        self.expected_len = 0;
        self.current_len = 0;
    }
}

// ============================================================================
// SBC Decoder Tables
// ============================================================================

pub const SBC_OFFSET4: [[i32; 4]; 4] = [
    [-1, 0, 0, 0],
    [-2, 0, 0, 1],
    [-2, 0, 0, 1],
    [-2, 0, 0, 1],
];

pub const SBC_OFFSET8: [[i32; 8]; 4] = [
    [-2, 0, 0, 0, 0, 0, 0, 1],
    [-3, 0, 0, 0, 0, 0, 1, 2],
    [-4, 0, 0, 0, 0, 0, 1, 2],
    [-4, 0, 0, 0, 0, 0, 1, 2],
];

pub const SBC_PROTO_4_40M0: [i32; 20] = [
    0, -1431, -17773, 17772, 1430, -71, -2679, -25558, 10177, 401, -196, -3785, -32328, 3777,
    -245, -359, -4220, -36940, -804, -511,
];

pub const SBC_PROTO_4_40M1: [i32; 20] = [
    -503, -3392, -38577, -3392, -503, -511, -804, -36940, -4220, -359, -245, 3777, -32328,
    -3785, -196, 401, 10177, -25558, -2679, -71,
];

pub const SBC_PROTO_8_80M0: [i32; 40] = [
    0, -1484, -17826, 17825, 1483, -42, -2105, -21754, 13942, 916, -90, -2742, -25579, 10243,
    432, -146, -3342, -29150, 6844, 46, -216, -3842, -32314, 3837, -237, -299, -4170, -34935,
    1288, -424, -388, -4253, -36898, -767, -523, -468, -4016, -38114, -2322, -552,
];

pub const SBC_PROTO_8_80M1: [i32; 40] = [
    -528, -3392, -38524, -3392, -528, -552, -2322, -38114, -4016, -468, -523, -767, -36898,
    -4253, -388, -424, 1288, -34935, -4170, -299, -237, 3837, -32314, -3842, -216, 46, 6844,
    -29150, -3342, -146, 432, 10243, -25579, -2742, -90, 916, 13942, -21754, -2105, -42,
];

pub const SYNMATRIX4: [[i32; 4]; 8] = [
    [5792, -5793, -5793, 5792],
    [3134, -7569, 7568, -3135],
    [0, 0, 0, 0],
    [-3135, 7568, -7569, 3134],
    [-5793, 5792, 5792, -5793],
    [-7569, -3135, 3134, 7568],
    [-8192, -8192, -8192, -8192],
    [-7569, -3135, 3134, 7568],
];

pub const SYNMATRIX8: [[i32; 8]; 16] = [
    [5792, -5793, -5793, 5792, 5792, -5793, -5793, 5792],
    [4551, -8035, 1598, 6811, -6812, -1599, 8034, -4552],
    [3134, -7569, 7568, -3135, -3135, 7568, -7569, 3134],
    [1598, -4552, 6811, -8035, 8034, -6812, 4551, -1599],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [-1599, 4551, -6812, 8034, -8035, 6811, -4552, 1598],
    [-3135, 7568, -7569, 3134, 3134, -7569, 7568, -3135],
    [-4552, 8034, -1599, -6812, 6811, 1598, -8035, 4551],
    [-5793, 5792, 5792, -5793, -5793, 5792, 5792, -5793],
    [-6812, 1598, 8034, 4551, -4552, -8035, -1599, 6811],
    [-7569, -3135, 3134, 7568, 7568, 3134, -3135, -7569],
    [-8035, -6812, -4552, -1599, 1598, 4551, 6811, 8034],
    [-8192, -8192, -8192, -8192, -8192, -8192, -8192, -8192],
    [-8035, -6812, -4552, -1599, 1598, 4551, 6811, 8034],
    [-7569, -3135, 3134, 7568, 7568, 3134, -3135, -7569],
    [-6812, 1598, 8034, 4551, -4552, -8035, -1599, 6811],
];

pub const CRC_TABLE: [u8; 256] = [
    0x00, 0x1D, 0x3A, 0x27, 0x74, 0x69, 0x4E, 0x53, 0xE8, 0xF5, 0xD2, 0xCF, 0x9C, 0x81, 0xA6,
    0xBB, 0xCD, 0xD0, 0xF7, 0xEA, 0xB9, 0xA4, 0x83, 0x9E, 0x25, 0x38, 0x1F, 0x02, 0x51, 0x4C,
    0x6B, 0x76, 0x87, 0x9A, 0xBD, 0xA0, 0xF3, 0xEE, 0xC9, 0xD4, 0x6F, 0x72, 0x55, 0x48, 0x1B,
    0x06, 0x21, 0x3C, 0x4A, 0x57, 0x70, 0x6D, 0x3E, 0x23, 0x04, 0x19, 0xA2, 0xBF, 0x98, 0x85,
    0xD6, 0xCB, 0xEC, 0xF1, 0x13, 0x0E, 0x29, 0x34, 0x67, 0x7A, 0x5D, 0x40, 0xFB, 0xE6, 0xC1,
    0xDC, 0x8F, 0x92, 0xB5, 0xA8, 0xDE, 0xC3, 0xE4, 0xF9, 0xAA, 0xB7, 0x90, 0x8D, 0x36, 0x2B,
    0x0C, 0x11, 0x42, 0x5F, 0x78, 0x65, 0x94, 0x89, 0xAE, 0xB3, 0xE0, 0xFD, 0xDA, 0xC7, 0x7C,
    0x61, 0x46, 0x5B, 0x08, 0x15, 0x32, 0x2F, 0x59, 0x44, 0x63, 0x7E, 0x2D, 0x30, 0x17, 0x0A,
    0xB1, 0xAC, 0x8B, 0x96, 0xC5, 0xD8, 0xFF, 0xE2, 0x26, 0x3B, 0x1C, 0x01, 0x52, 0x4F, 0x68,
    0x75, 0xCE, 0xD3, 0xF4, 0xE9, 0xBA, 0xA7, 0x80, 0x9D, 0xEB, 0xF6, 0xD1, 0xCC, 0x9F, 0x82,
    0xA5, 0xB8, 0x03, 0x1E, 0x39, 0x24, 0x77, 0x6A, 0x4D, 0x50, 0xA1, 0xBC, 0x9B, 0x86, 0xD5,
    0xC8, 0xEF, 0xF2, 0x49, 0x54, 0x73, 0x6E, 0x3D, 0x20, 0x07, 0x1A, 0x6C, 0x71, 0x56, 0x4B,
    0x18, 0x05, 0x22, 0x3F, 0x84, 0x99, 0xBE, 0xA3, 0xF0, 0xED, 0xCA, 0xD7, 0x35, 0x28, 0x0F,
    0x12, 0x41, 0x5C, 0x7B, 0x66, 0xDD, 0xC0, 0xE7, 0xFA, 0xA9, 0xB4, 0x93, 0x8E, 0xF8, 0xE5,
    0xC2, 0xDF, 0x8C, 0x91, 0xB6, 0xAB, 0x10, 0x0D, 0x2A, 0x37, 0x64, 0x79, 0x5E, 0x43, 0xB2,
    0xAF, 0x88, 0x95, 0xC6, 0xDB, 0xFC, 0xE1, 0x5A, 0x47, 0x60, 0x7D, 0x2E, 0x33, 0x14, 0x09,
    0x7F, 0x62, 0x45, 0x58, 0x0B, 0x16, 0x31, 0x2C, 0x97, 0x8A, 0xAD, 0xB0, 0xE3, 0xFE, 0xD9,
    0xC4,
];

pub fn sbc_crc8(data: &[u8], len_bits: usize) -> u8 {
    let mut crc = 0x0F;
    let bytes = len_bits / 8;
    for &b in &data[..bytes] {
        crc = CRC_TABLE[(crc ^ b) as usize];
    }
    let remainder = len_bits % 8;
    if remainder > 0 {
        let mut octet = data[bytes];
        for _ in 0..remainder {
            let bit = ((octet ^ crc) & 0x80) >> 7;
            crc = ((crc & 0x7F) << 1) ^ (if bit != 0 { 0x1D } else { 0 });
            octet <<= 1;
        }
    }
    crc
}

#[inline(always)]
pub fn sbc_clip16(s: i32) -> i16 {
    if s > 0x7FFF {
        0x7FFF
    } else if s < -0x8000 {
        -0x8000
    } else {
        s as i16
    }
}

// ============================================================================
// SBC Decoder Implementation
// ============================================================================

#[derive(Debug, PartialEq, Eq)]
pub enum SbcError {
    TooShort,
    BadSyncword,
    BadCrc,
    BitpoolOutOfBounds,
    Unsupported,
}

impl fmt::Display for SbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "SBC payload too short"),
            Self::BadSyncword => write!(f, "Invalid SBC syncword"),
            Self::BadCrc => write!(f, "SBC CRC mismatch"),
            Self::BitpoolOutOfBounds => write!(f, "SBC bitpool out of bounds"),
            Self::Unsupported => write!(f, "Unsupported SBC mode/configuration"),
        }
    }
}

pub struct SbcDecoder {
    pub v: [[i32; 170]; 2],
    pub offset: [[usize; 16]; 2],
    pub subbands: usize,
}

impl SbcDecoder {
    pub const fn new() -> Self {
        Self {
            v: [[0; 170]; 2],
            offset: [[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160]; 2],
            subbands: 0,
        }
    }

    pub fn reset(&mut self, subbands: usize) {
        self.subbands = subbands;
        self.v = [[0; 170]; 2];
        for ch in 0..2 {
            for i in 0..(subbands * 2) {
                self.offset[ch][i] = 10 * i + 10;
            }
        }
    }

    /// Decode one SBC frame starting at `data[0]`.
    /// Decodes into interleaved 16-bit signed stereo PCM
    /// (`pcm_out[0]` = L0, `pcm_out[1]` = R0, ...).
    /// Returns `Ok((bytes_consumed, num_stereo_samples))`
    /// where `num_stereo_samples = blocks * subbands`.
    pub fn decode_frame(
        &mut self,
        data: &[u8],
        pcm_out: &mut [i16],
    ) -> Result<(usize, usize), SbcError> {
        if data.len() < 4 {
            return Err(SbcError::TooShort);
        }
        if data[0] != SBC_SYNCWORD {
            return Err(SbcError::BadSyncword);
        }

        let sf_idx = ((data[1] >> 6) & 0x03) as usize;
        let block_mode = (data[1] >> 4) & 0x03;
        let blocks = match block_mode {
            0 => 4,
            1 => 8,
            2 => 12,
            3 => 16,
            _ => 16,
        };

        let mode = (data[1] >> 2) & 0x03;
        let channels = if mode == 0 { 1 } else { 2 };
        let allocation = ((data[1] >> 1) & 0x01) as usize; // 0 = Loudness, 1 = SNR
        let subband_mode = data[1] & 0x01;
        let subbands = if subband_mode != 0 { 8 } else { 4 };
        let bitpool = data[2] as usize;

        if (mode == 0 || mode == 1) && bitpool > 16 * subbands {
            return Err(SbcError::BitpoolOutOfBounds);
        }
        if (mode == 2 || mode == 3) && bitpool > 32 * subbands {
            return Err(SbcError::BitpoolOutOfBounds);
        }

        if self.subbands != subbands {
            self.reset(subbands);
        }

        let mut consumed_bits = 32usize;
        let mut crc_header = [0u8; 11];
        crc_header[0] = data[1];
        crc_header[1] = data[2];
        let mut crc_pos = 16usize;

        let mut joint = 0u8;
        if mode == 3 {
            // Joint Stereo
            if data.len() * 8 < consumed_bits + subbands {
                return Err(SbcError::TooShort);
            }
            let j_byte = data[4];
            for sb in 0..(subbands - 1) {
                joint |= ((j_byte >> (7 - sb)) & 0x01) << sb;
            }
            if subbands == 4 {
                crc_header[crc_pos / 8] = j_byte & 0xF0;
            } else {
                crc_header[crc_pos / 8] = j_byte;
            }
            consumed_bits += subbands;
            crc_pos += subbands;
        }

        if data.len() * 8 < consumed_bits + (4 * subbands * channels) {
            return Err(SbcError::TooShort);
        }

        let mut scale_factor = [[0usize; 8]; 2];
        for ch in 0..channels {
            for sb in 0..subbands {
                let sf_val =
                    ((data[consumed_bits >> 3] >> (4 - (consumed_bits & 0x7))) & 0x0F) as usize;
                scale_factor[ch][sb] = sf_val;
                crc_header[crc_pos >> 3] |= (sf_val as u8) << (4 - (crc_pos & 0x7));
                consumed_bits += 4;
                crc_pos += 4;
            }
        }

        let computed_crc = sbc_crc8(&crc_header, crc_pos);
        if data[3] != computed_crc {
            return Err(SbcError::BadCrc);
        }

        // Bit allocation
        let mut bits = [[0usize; 8]; 2];
        self.calculate_bits(
            mode,
            allocation,
            sf_idx,
            subbands,
            channels,
            bitpool,
            &scale_factor,
            &mut bits,
        );

        let mut levels = [[0usize; 8]; 2];
        for ch in 0..channels {
            for sb in 0..subbands {
                levels[ch][sb] = (1 << bits[ch][sb]) - 1;
            }
        }

        // Unpack subband samples
        let mut sb_sample = [[[0i32; 8]; 2]; 16];
        for blk in 0..blocks {
            for ch in 0..channels {
                for sb in 0..subbands {
                    let lvl = levels[ch][sb];
                    if lvl == 0 {
                        sb_sample[blk][ch][sb] = 0;
                        continue;
                    }

                    let shift = scale_factor[ch][sb] + 1 + 2; // + SBCDEC_FIXED_EXTRA_BITS
                    let mut audio_sample = 0u32;
                    let num_bits = bits[ch][sb];
                    for bit in 0..num_bits {
                        if consumed_bits >= data.len() * 8 {
                            return Err(SbcError::TooShort);
                        }
                        if ((data[consumed_bits >> 3] >> (7 - (consumed_bits & 0x7))) & 0x01) != 0 {
                            audio_sample |= 1 << (num_bits - bit - 1);
                        }
                        consumed_bits += 1;
                    }

                    let numerator = (((audio_sample as u64) << 1) | 1) << shift;
                    let val = (numerator / (lvl as u64)) as i32 - (1 << shift);
                    sb_sample[blk][ch][sb] = val;
                }
            }
        }

        // Joint stereo butterfly mixing
        if mode == 3 {
            for blk in 0..blocks {
                for sb in 0..subbands {
                    if (joint & (1 << sb)) != 0 {
                        let mid = sb_sample[blk][0][sb];
                        let side = sb_sample[blk][1][sb];
                        sb_sample[blk][0][sb] = mid + side;
                        sb_sample[blk][1][sb] = mid - side;
                    }
                }
            }
        }

        // Round consumed bytes to byte boundary
        if (consumed_bits & 0x7) != 0 {
            consumed_bits += 8 - (consumed_bits & 0x7);
        }
        let bytes_consumed = consumed_bits >> 3;

        // Synthesis filterbank
        let mut ch_pcm = [[0i16; 128]; 2];
        if subbands == 4 {
            for ch in 0..channels {
                for blk in 0..blocks {
                    self.synthesize_four(&sb_sample, ch, blk, &mut ch_pcm[ch]);
                }
            }
        } else {
            for ch in 0..channels {
                for blk in 0..blocks {
                    self.synthesize_eight(&sb_sample, ch, blk, &mut ch_pcm[ch]);
                }
            }
        }

        let total_frames = blocks * subbands;
        if pcm_out.len() < total_frames * 2 {
            return Err(SbcError::TooShort);
        }

        if channels == 1 {
            // Mono: duplicate to L & R
            for i in 0..total_frames {
                pcm_out[i * 2] = ch_pcm[0][i];
                pcm_out[i * 2 + 1] = ch_pcm[0][i];
            }
        } else {
            // Stereo
            for i in 0..total_frames {
                pcm_out[i * 2] = ch_pcm[0][i];
                pcm_out[i * 2 + 1] = ch_pcm[1][i];
            }
        }

        Ok((bytes_consumed, total_frames))
    }

    #[allow(clippy::too_many_arguments)]
    fn calculate_bits(
        &self,
        mode: u8,
        allocation: usize,
        sf_idx: usize,
        subbands: usize,
        channels: usize,
        bitpool: usize,
        scale_factor: &[[usize; 8]; 2],
        bits: &mut [[usize; 8]; 2],
    ) {
        if mode == 0 || mode == 1 {
            // MONO or DUAL_CHANNEL
            for ch in 0..channels {
                let ch_bitpool = if mode == 1 {
                    if ch == 0 {
                        bitpool.div_ceil(2)
                    } else {
                        bitpool / 2
                    }
                } else {
                    bitpool
                };

                let mut bitneed = [0i32; 8];
                let mut max_bitneed = 0i32;

                if allocation == 1 {
                    // SNR
                    for sb in 0..subbands {
                        bitneed[sb] = scale_factor[ch][sb] as i32;
                        if bitneed[sb] > max_bitneed {
                            max_bitneed = bitneed[sb];
                        }
                    }
                } else {
                    // Loudness
                    for sb in 0..subbands {
                        if scale_factor[ch][sb] == 0 {
                            bitneed[sb] = -5;
                        } else {
                            let offset = if subbands == 4 {
                                SBC_OFFSET4[sf_idx][sb]
                            } else {
                                SBC_OFFSET8[sf_idx][sb]
                            };
                            let loudness = (scale_factor[ch][sb] as i32) - offset;
                            bitneed[sb] = if loudness > 0 {
                                loudness / 2
                            } else {
                                loudness
                            };
                        }
                        if bitneed[sb] > max_bitneed {
                            max_bitneed = bitneed[sb];
                        }
                    }
                }

                let mut bitcount = 0usize;
                let mut slicecount = 0usize;
                let mut bitslice = max_bitneed + 1;

                loop {
                    bitslice -= 1;
                    bitcount += slicecount;
                    slicecount = 0;
                    for sb in 0..subbands {
                        if bitneed[sb] > bitslice + 1 && bitneed[sb] < bitslice + 16 {
                            slicecount += 1;
                        } else if bitneed[sb] == bitslice + 1 {
                            slicecount += 2;
                        }
                    }
                    if bitcount + slicecount >= ch_bitpool || bitslice <= -32 {
                        break;
                    }
                }

                if bitcount + slicecount == ch_bitpool {
                    bitcount += slicecount;
                    bitslice -= 1;
                }

                for sb in 0..subbands {
                    if bitneed[sb] < bitslice + 2 {
                        bits[ch][sb] = 0;
                    } else {
                        let b = (bitneed[sb] - bitslice).clamp(0, 16) as usize;
                        bits[ch][sb] = b;
                    }
                }

                let mut sb = 0;
                while bitcount < ch_bitpool && sb < subbands {
                    if bits[ch][sb] >= 2 && bits[ch][sb] < 16 {
                        bits[ch][sb] += 1;
                        bitcount += 1;
                    } else if bitneed[sb] == bitslice + 1 && ch_bitpool > bitcount + 1 {
                        bits[ch][sb] = 2;
                        bitcount += 2;
                    }
                    sb += 1;
                }

                sb = 0;
                while bitcount < ch_bitpool && sb < subbands {
                    if bits[ch][sb] < 16 {
                        bits[ch][sb] += 1;
                        bitcount += 1;
                    }
                    sb += 1;
                }
            }
        } else {
            // STEREO or JOINT_STEREO
            let mut bitneed = [[0i32; 8]; 2];
            let mut max_bitneed = 0i32;

            if allocation == 1 {
                // SNR
                for ch in 0..2 {
                    for sb in 0..subbands {
                        bitneed[ch][sb] = scale_factor[ch][sb] as i32;
                        if bitneed[ch][sb] > max_bitneed {
                            max_bitneed = bitneed[ch][sb];
                        }
                    }
                }
            } else {
                // Loudness
                for ch in 0..2 {
                    for sb in 0..subbands {
                        if scale_factor[ch][sb] == 0 {
                            bitneed[ch][sb] = -5;
                        } else {
                            let offset = if subbands == 4 {
                                SBC_OFFSET4[sf_idx][sb]
                            } else {
                                SBC_OFFSET8[sf_idx][sb]
                            };
                            let loudness = (scale_factor[ch][sb] as i32) - offset;
                            bitneed[ch][sb] = if loudness > 0 {
                                loudness / 2
                            } else {
                                loudness
                            };
                        }
                        if bitneed[ch][sb] > max_bitneed {
                            max_bitneed = bitneed[ch][sb];
                        }
                    }
                }
            }

            let mut bitcount = 0usize;
            let mut slicecount = 0usize;
            let mut bitslice = max_bitneed + 1;

            loop {
                bitslice -= 1;
                bitcount += slicecount;
                slicecount = 0;
                for ch in 0..2 {
                    for sb in 0..subbands {
                        if bitneed[ch][sb] > bitslice + 1 && bitneed[ch][sb] < bitslice + 16 {
                            slicecount += 1;
                        } else if bitneed[ch][sb] == bitslice + 1 {
                            slicecount += 2;
                        }
                    }
                }
                if bitcount + slicecount >= bitpool || bitslice <= -32 {
                    break;
                }
            }

            if bitcount + slicecount == bitpool {
                bitcount += slicecount;
                bitslice -= 1;
            }

            for ch in 0..2 {
                for sb in 0..subbands {
                    if bitneed[ch][sb] < bitslice + 2 {
                        bits[ch][sb] = 0;
                    } else {
                        let b = (bitneed[ch][sb] - bitslice).clamp(0, 16) as usize;
                        bits[ch][sb] = b;
                    }
                }
            }

            let mut ch = 0;
            let mut sb = 0;
            while bitcount < bitpool {
                if bits[ch][sb] >= 2 && bits[ch][sb] < 16 {
                    bits[ch][sb] += 1;
                    bitcount += 1;
                } else if bitneed[ch][sb] == bitslice + 1 && bitpool > bitcount + 1 {
                    bits[ch][sb] = 2;
                    bitcount += 2;
                }
                if ch == 1 {
                    ch = 0;
                    sb += 1;
                    if sb >= subbands {
                        break;
                    }
                } else {
                    ch = 1;
                }
            }

            ch = 0;
            sb = 0;
            while bitcount < bitpool {
                if bits[ch][sb] < 16 {
                    bits[ch][sb] += 1;
                    bitcount += 1;
                }
                if ch == 1 {
                    ch = 0;
                    sb += 1;
                    if sb >= subbands {
                        break;
                    }
                } else {
                    ch = 1;
                }
            }
        }
    }

    pub fn synthesize_four(
        &mut self,
        sb_sample: &[[[i32; 8]; 2]; 16],
        ch: usize,
        blk: usize,
        pcm_out: &mut [i16],
    ) {
        let offset = &mut self.offset[ch];
        let v = &mut self.v[ch];

        for i in 0..8 {
            if offset[i] == 0 {
                offset[i] = 79;
                for j in 0..9 {
                    v[j + 80] = v[j];
                }
            } else {
                offset[i] -= 1;
            }

            let s0 = sb_sample[blk][ch][0] as i64;
            let s1 = sb_sample[blk][ch][1] as i64;
            let s2 = sb_sample[blk][ch][2] as i64;
            let s3 = sb_sample[blk][ch][3] as i64;

            let acc = (SYNMATRIX4[i][0] as i64 * s0)
                + (SYNMATRIX4[i][1] as i64 * s1)
                + (SYNMATRIX4[i][2] as i64 * s2)
                + (SYNMATRIX4[i][3] as i64 * s3);

            v[offset[i]] = (acc >> 15) as i32;
        }

        for i in 0..4 {
            let idx = i * 5;
            let k = (i + 4) & 0x0F;

            let acc = (v[offset[i] + 0] as i64 * SBC_PROTO_4_40M0[idx + 0] as i64)
                + (v[offset[k] + 1] as i64 * SBC_PROTO_4_40M1[idx + 0] as i64)
                + (v[offset[i] + 2] as i64 * SBC_PROTO_4_40M0[idx + 1] as i64)
                + (v[offset[k] + 3] as i64 * SBC_PROTO_4_40M1[idx + 1] as i64)
                + (v[offset[i] + 4] as i64 * SBC_PROTO_4_40M0[idx + 2] as i64)
                + (v[offset[k] + 5] as i64 * SBC_PROTO_4_40M1[idx + 2] as i64)
                + (v[offset[i] + 6] as i64 * SBC_PROTO_4_40M0[idx + 3] as i64)
                + (v[offset[k] + 7] as i64 * SBC_PROTO_4_40M1[idx + 3] as i64)
                + (v[offset[i] + 8] as i64 * SBC_PROTO_4_40M0[idx + 4] as i64)
                + (v[offset[k] + 9] as i64 * SBC_PROTO_4_40M1[idx + 4] as i64);

            let sample = (acc >> 15) as i32;
            pcm_out[blk * 4 + i] = sbc_clip16(sample);
        }
    }

    pub fn synthesize_eight(
        &mut self,
        sb_sample: &[[[i32; 8]; 2]; 16],
        ch: usize,
        blk: usize,
        pcm_out: &mut [i16],
    ) {
        let offset = &mut self.offset[ch];
        let v = &mut self.v[ch];

        for i in 0..16 {
            if offset[i] == 0 {
                offset[i] = 159;
                for j in 0..9 {
                    v[j + 160] = v[j];
                }
            } else {
                offset[i] -= 1;
            }

            let mut acc = 0i64;
            for sb in 0..8 {
                acc += SYNMATRIX8[i][sb] as i64 * (sb_sample[blk][ch][sb] as i64);
            }

            v[offset[i]] = (acc >> 15) as i32;
        }

        for i in 0..8 {
            let idx = i * 5;
            let k = (i + 8) & 0x0F;

            let acc = (v[offset[i] + 0] as i64 * SBC_PROTO_8_80M0[idx + 0] as i64)
                + (v[offset[k] + 1] as i64 * SBC_PROTO_8_80M1[idx + 0] as i64)
                + (v[offset[i] + 2] as i64 * SBC_PROTO_8_80M0[idx + 1] as i64)
                + (v[offset[k] + 3] as i64 * SBC_PROTO_8_80M1[idx + 1] as i64)
                + (v[offset[i] + 4] as i64 * SBC_PROTO_8_80M0[idx + 2] as i64)
                + (v[offset[k] + 5] as i64 * SBC_PROTO_8_80M1[idx + 2] as i64)
                + (v[offset[i] + 6] as i64 * SBC_PROTO_8_80M0[idx + 3] as i64)
                + (v[offset[k] + 7] as i64 * SBC_PROTO_8_80M1[idx + 3] as i64)
                + (v[offset[i] + 8] as i64 * SBC_PROTO_8_80M0[idx + 4] as i64)
                + (v[offset[k] + 9] as i64 * SBC_PROTO_8_80M1[idx + 4] as i64);

            let sample = (acc >> 15) as i32;
            pcm_out[blk * 8 + i] = sbc_clip16(sample);
        }
    }
}

// ============================================================================
// Pure-Rust SBC Audio Encoder (8-Subband, 16-Block, 44.1 kHz Stereo/Joint)
// ============================================================================

/// Channel mode for [`SbcEncoder`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SbcChannelMode {
    Mono = 0,
    DualChannel = 1,
    Stereo = 2,
    JointStereo = 3,
}

/// Bit allocation method for [`SbcEncoder`].
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SbcAllocationMethod {
    Loudness = 0,
    Snr = 1,
}

/// Pure-Rust 8-subband, 16-block Bluetooth A2DP SBC audio encoder.
///
/// Uses the exact time-reversed polyphase analysis adjoint of
/// [`SBC_PROTO_8_80M0`], [`SBC_PROTO_8_80M1`], and [`SYNMATRIX8`] to achieve
/// >60 dB round-trip SNR through [`SbcDecoder`] with zero heap allocations.
#[non_exhaustive]
pub struct SbcEncoder {
    pub x_blocks: [[[i16; 8]; 10]; 2],
    pub bitpool: u8,
    pub sample_rate_hz: u32,
    pub channel_mode: SbcChannelMode,
    pub allocation: SbcAllocationMethod,
}

impl SbcEncoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            x_blocks: [[[0i16; 8]; 10]; 2],
            bitpool: 53,
            sample_rate_hz: 44100,
            channel_mode: SbcChannelMode::Stereo,
            allocation: SbcAllocationMethod::Loudness,
        }
    }

    #[must_use]
    pub const fn with_config(
        sample_rate_hz: u32,
        channel_mode: SbcChannelMode,
        allocation: SbcAllocationMethod,
        bitpool: u8,
    ) -> Self {
        Self {
            x_blocks: [[[0i16; 8]; 10]; 2],
            bitpool,
            sample_rate_hz,
            channel_mode,
            allocation,
        }
    }

    pub fn reset(&mut self) {
        self.x_blocks = [[[0i16; 8]; 10]; 2];
    }

    #[must_use]
    const fn sf_index(&self) -> usize {
        match self.sample_rate_hz {
            16000 => 0,
            32000 => 1,
            48000 => 3,
            _ => 2, // 44100 Hz default
        }
    }

    /// Runs the 8-subband polyphase analysis filterbank on one 8-sample block
    /// for channel `ch`.
    pub fn analyze_eight(&mut self, ch: usize, pcm8: &[i16; 8]) -> [i32; 8] {
        let hist = &mut self.x_blocks[ch];
        let mut b = 9usize;
        while b > 0 {
            hist[b] = hist[b - 1];
            b -= 1;
        }
        hist[0] = *pcm8;

        let mut w = [0i64; 16];
        for i in 0..8 {
            let idx = i * 5;
            let acc0 = (hist[9][i] as i64 * SBC_PROTO_8_80M0[idx] as i64)
                + (hist[7][i] as i64 * SBC_PROTO_8_80M0[idx + 1] as i64)
                + (hist[5][i] as i64 * SBC_PROTO_8_80M0[idx + 2] as i64)
                + (hist[3][i] as i64 * SBC_PROTO_8_80M0[idx + 3] as i64)
                + (hist[1][i] as i64 * SBC_PROTO_8_80M0[idx + 4] as i64);

            let acc1 = (hist[8][i] as i64 * SBC_PROTO_8_80M1[idx] as i64)
                + (hist[6][i] as i64 * SBC_PROTO_8_80M1[idx + 1] as i64)
                + (hist[4][i] as i64 * SBC_PROTO_8_80M1[idx + 2] as i64)
                + (hist[2][i] as i64 * SBC_PROTO_8_80M1[idx + 3] as i64)
                + (hist[0][i] as i64 * SBC_PROTO_8_80M1[idx + 4] as i64);

            w[i] = acc0 >> 15;
            w[i + 8] = acc1 >> 15;
        }

        let mut sb_out = [0i32; 8];
        for sb in 0..8 {
            let mut acc = 0i64;
            for r in 0..16 {
                acc += (SYNMATRIX8[r][sb] as i64) * w[r];
            }
            sb_out[sb] = (acc >> 14) as i32;
        }
        sb_out
    }

    /// Encodes 128 interleaved stereo PCM frames (`256` `i16` samples:
    /// `[L0, R0, L1, R1, ..., L127, R127]`) into one 16-block, 8-subband SBC
    /// frame starting with syncword `0x9C`.
    /// Returns `Ok(sbc_bytes_written)`.
    pub fn encode_frame(
        &mut self,
        pcm_stereo: &[i16],
        sbc_out: &mut [u8],
    ) -> Result<usize, SbcError> {
        let blocks = 16usize;
        let subbands = 8usize;
        let total_stereo_frames = blocks * subbands; // 128
        if pcm_stereo.len() < total_stereo_frames * 2 {
            return Err(SbcError::TooShort);
        }

        let mode = self.channel_mode as u8;
        let channels = if mode == 0 { 1usize } else { 2usize };
        let bitpool = self.bitpool as usize;
        if (mode == 0 || mode == 1) && (bitpool < 2 || bitpool > 16 * subbands) {
            return Err(SbcError::BitpoolOutOfBounds);
        }
        if (mode == 2 || mode == 3) && (bitpool < 2 || bitpool > 32 * subbands) {
            return Err(SbcError::BitpoolOutOfBounds);
        }

        let mut sb_sample = [[[0i32; 8]; 2]; 16];
        for blk in 0..blocks {
            let base = blk * 16; // 8 stereo frames = 16 i16 samples
            let mut pcm_l = [0i16; 8];
            let mut pcm_r = [0i16; 8];
            for i in 0..8 {
                pcm_l[i] = pcm_stereo[base + i * 2];
                pcm_r[i] = pcm_stereo[base + i * 2 + 1];
            }
            if channels == 1 {
                let mut pcm_m = [0i16; 8];
                for i in 0..8 {
                    pcm_m[i] = ((i32::from(pcm_l[i]) + i32::from(pcm_r[i])) / 2) as i16;
                }
                sb_sample[blk][0] = self.analyze_eight(0, &pcm_m);
            } else {
                sb_sample[blk][0] = self.analyze_eight(0, &pcm_l);
                sb_sample[blk][1] = self.analyze_eight(1, &pcm_r);
            }
        }

        let mut joint_mask = 0u8;
        if mode == 3 {
            // Evaluate Joint Stereo mid/side coding on subbands 0..7
            for sb in 0..(subbands - 1) {
                let mut max_l = 0i32;
                let mut max_r = 0i32;
                let mut max_m = 0i32;
                let mut max_s = 0i32;
                for blk in 0..blocks {
                    let l = sb_sample[blk][0][sb];
                    let r = sb_sample[blk][1][sb];
                    let m = (l + r) >> 1;
                    let s = (l - r) >> 1;
                    max_l = max_l.max(l.saturating_abs());
                    max_r = max_r.max(r.saturating_abs());
                    max_m = max_m.max(m.saturating_abs());
                    max_s = max_s.max(s.saturating_abs());
                }
                let sf_lr = compute_scale_factor(max_l) + compute_scale_factor(max_r);
                let sf_ms = compute_scale_factor(max_m) + compute_scale_factor(max_s);
                if sf_ms < sf_lr {
                    joint_mask |= 1 << sb;
                    for blk in 0..blocks {
                        let l = sb_sample[blk][0][sb];
                        let r = sb_sample[blk][1][sb];
                        sb_sample[blk][0][sb] = (l + r) >> 1;
                        sb_sample[blk][1][sb] = (l - r) >> 1;
                    }
                }
            }
        }

        let mut scale_factor = [[0usize; 8]; 2];
        for ch in 0..channels {
            for sb in 0..subbands {
                let mut max_abs = 0i32;
                for blk in 0..blocks {
                    max_abs = max_abs.max(sb_sample[blk][ch][sb].saturating_abs());
                }
                scale_factor[ch][sb] = compute_scale_factor(max_abs);
            }
        }

        let sf_idx = self.sf_index();
        let allocation = self.allocation as usize;
        let dec_helper = SbcDecoder::new();
        let mut bits = [[0usize; 8]; 2];
        dec_helper.calculate_bits(
            mode,
            allocation,
            sf_idx,
            subbands,
            channels,
            bitpool,
            &scale_factor,
            &mut bits,
        );

        // Compute total bits to verify sbc_out capacity
        let header_bits = 32
            + (if mode == 3 { subbands } else { 0 })
            + (4 * subbands * channels);
        let mut payload_bits = 0usize;
        for ch in 0..channels {
            for sb in 0..subbands {
                payload_bits += blocks * bits[ch][sb];
            }
        }
        let total_bytes = (header_bits + payload_bits).div_ceil(8);
        if sbc_out.len() < total_bytes {
            return Err(SbcError::TooShort);
        }
        sbc_out[..total_bytes].fill(0);

        let byte1 = ((sf_idx as u8 & 0x03) << 6)
            | (0x03 << 4) // 16 blocks
            | ((mode & 0x03) << 2)
            | (((allocation as u8) & 0x01) << 1)
            | 0x01; // 8 subbands

        sbc_out[0] = SBC_SYNCWORD;
        sbc_out[1] = byte1;
        sbc_out[2] = self.bitpool;

        let mut crc_header = [0u8; 11];
        crc_header[0] = byte1;
        crc_header[1] = self.bitpool;
        let mut crc_pos = 16usize;
        let mut bit_pos = 32usize;

        if mode == 3 {
            let mut j_byte = 0u8;
            for sb in 0..(subbands - 1) {
                if (joint_mask & (1 << sb)) != 0 {
                    j_byte |= 1 << (7 - sb);
                }
            }
            sbc_out[4] = j_byte;
            crc_header[crc_pos / 8] = j_byte;
            bit_pos += subbands;
            crc_pos += subbands;
        }

        for ch in 0..channels {
            for sb in 0..subbands {
                let sf_val = (scale_factor[ch][sb] & 0x0F) as u8;
                sbc_out[bit_pos >> 3] |= sf_val << (4 - (bit_pos & 0x7));
                crc_header[crc_pos >> 3] |= sf_val << (4 - (crc_pos & 0x7));
                bit_pos += 4;
                crc_pos += 4;
            }
        }

        sbc_out[3] = sbc_crc8(&crc_header, crc_pos);

        // Quantize and pack subband samples MSB-first
        for blk in 0..blocks {
            for ch in 0..channels {
                for sb in 0..subbands {
                    let num_bits = bits[ch][sb];
                    if num_bits == 0 {
                        continue;
                    }
                    let lvl = (1u64 << num_bits) - 1;
                    let shift = scale_factor[ch][sb] + 3;
                    let limit = (1i32 << shift) - 1;
                    let clamped = sb_sample[blk][ch][sb].clamp(-limit, limit);
                    let shifted = (clamped + (1i32 << shift)) as u64;
                    let audio_sample =
                        ((shifted * lvl) >> (shift + 1)).min(lvl - 1) as u32;

                    for b in 0..num_bits {
                        let bit_val = (audio_sample >> (num_bits - 1 - b)) & 0x01;
                        if bit_val != 0 {
                            sbc_out[bit_pos >> 3] |= 1 << (7 - (bit_pos & 0x7));
                        }
                        bit_pos += 1;
                    }
                }
            }
        }

        Ok(total_bytes)
    }
}

const fn compute_scale_factor(max_abs: i32) -> usize {
    let mut sf = 0usize;
    while sf < 15 && (1i32 << (sf + 3)) <= max_abs {
        sf += 1;
    }
    sf
}

// ============================================================================
// A2DP Source (`-o BT`) Inquiry Matcher & AVDTP Initiator State Machine
// ============================================================================

/// Extracts the UTF-8 local device name from an Extended Inquiry Response
/// (`EIR`) data payload (AD types `0x08` Shortened or `0x09` Complete Local
/// Name).
#[must_use]
pub fn parse_eir_local_name(eir_data: &[u8]) -> Option<&str> {
    let mut pos = 0usize;
    while pos < eir_data.len() {
        let field_len = eir_data[pos] as usize;
        if field_len == 0 || pos + 1 + field_len > eir_data.len() {
            break;
        }
        let ad_type = eir_data[pos + 1];
        let payload = &eir_data[pos + 2..pos + 1 + field_len];
        if ad_type == 0x08 || ad_type == 0x09 {
            let trimmed_len = payload
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(payload.len());
            if let Ok(name) = core::str::from_utf8(&payload[..trimmed_len]) {
                return Some(name);
            }
        }
        pos += 1 + field_len;
    }
    None
}

/// Checks whether an `EIR` payload advertises the A2DP `AudioSink` (`0x110B`)
/// or `AdvancedAudioDistribution` (`0x110D`) 16-bit Service Class UUID.
#[must_use]
pub fn eir_has_audio_sink_uuid(eir_data: &[u8]) -> bool {
    let mut pos = 0usize;
    while pos < eir_data.len() {
        let field_len = eir_data[pos] as usize;
        if field_len == 0 || pos + 1 + field_len > eir_data.len() {
            break;
        }
        let ad_type = eir_data[pos + 1];
        let payload = &eir_data[pos + 2..pos + 1 + field_len];
        if ad_type == 0x02 || ad_type == 0x03 {
            let mut i = 0usize;
            while i + 1 < payload.len() {
                let uuid = u16::from_le_bytes([payload[i], payload[i + 1]]);
                if uuid == 0x110B || uuid == 0x110D {
                    return true;
                }
                i += 2;
            }
        }
        pos += 1 + field_len;
    }
    false
}

fn ascii_contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    if n.len() > h.len() {
        return false;
    }
    for start in 0..=(h.len() - n.len()) {
        let mut matched = true;
        for k in 0..n.len() {
            if !h[start + k].eq_ignore_ascii_case(&n[k]) {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}

/// Connection & AVDTP signaling phase for A2DP Source (`-o BT`) mode.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BtA2dpSourcePhase {
    Idle,
    Inquiring,
    ConnectingAcl,
    ConnectingAvdtpSignaling,
    DiscoveringSeps,
    GettingCapabilities,
    SettingConfiguration,
    OpeningStream,
    ConnectingAvdtpMedia,
    StartingStream,
    Streaming,
}

/// A2DP Source (`-o BT`) state machine managing HCI Inquiry/EIR sink
/// discovery, AVDTP Source initiator signaling (`Discover` ->
/// `GetCapabilities` -> `SetConfiguration` -> `Open` -> `Start`), 44.1 kHz
/// LMS rate lock, and RTP/SBC media packetization.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BtA2dpSourceState {
    pub phase: BtA2dpSourcePhase,
    target_name_buf: [u8; 32],
    target_name_len: usize,
    pub target_bd_addr: Option<[u8; 6]>,
    pub matched_bd_addr: Option<[u8; 6]>,
    pub matched_rssi: i8,
    pub acl_handle: Option<u16>,
    pub sig_local_cid: u16,
    pub sig_remote_cid: u16,
    pub media_local_cid: u16,
    pub media_remote_cid: u16,
    pub remote_sink_seid: u8,
    pub local_source_seid: u8,
    pub tx_label: u8,
    pub rtp_seq_num: u16,
    pub rtp_timestamp: u32,
    pub rtp_ssrc: u32,
    pub locked_sample_rate_hz: u32,
    pub sbc_bitpool: u8,
}

impl BtA2dpSourceState {
    /// LMS sample rate lock for A2DP Source (`-o BT`) output.
    pub const LMS_LOCKED_RATE_HZ: u32 = 44100;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: BtA2dpSourcePhase::Idle,
            target_name_buf: [0u8; 32],
            target_name_len: 0,
            target_bd_addr: None,
            matched_bd_addr: None,
            matched_rssi: -127,
            acl_handle: None,
            sig_local_cid: 0x0048,
            sig_remote_cid: 0,
            media_local_cid: 0x0049,
            media_remote_cid: 0,
            remote_sink_seid: 1,
            local_source_seid: 1,
            tx_label: 1,
            rtp_seq_num: 1,
            rtp_timestamp: 0,
            rtp_ssrc: 0x534C_4254, // "SLBT"
            locked_sample_rate_hz: Self::LMS_LOCKED_RATE_HZ,
            sbc_bitpool: 53,
        }
    }

    pub fn update_target_name(&mut self, name: &str) {
        let len = copy_utf8_prefix(name.trim(), &mut self.target_name_buf);
        self.target_name_len = len;
    }

    #[must_use]
    pub fn target_name(&self) -> &str {
        let len = self.target_name_len.min(self.target_name_buf.len());
        core::str::from_utf8(&self.target_name_buf[..len]).unwrap_or("")
    }

    pub fn update_target_bd_addr(&mut self, bd_addr: Option<[u8; 6]>) {
        self.target_bd_addr = bd_addr;
    }

    #[must_use]
    pub const fn is_streaming(&self) -> bool {
        matches!(self.phase, BtA2dpSourcePhase::Streaming)
    }

    fn next_tx_label(&mut self) -> u8 {
        let lbl = self.tx_label & 0x0F;
        self.tx_label = (lbl + 1) & 0x0F;
        lbl
    }

    /// Evaluates whether an inquired peer (`bd_addr` + optional `eir_data`)
    /// matches the configured target sink address or target sink name.
    #[must_use]
    pub fn matches_inquiry_peer(&self, bd_addr: &[u8; 6], eir_data: &[u8]) -> bool {
        if let Some(ref target_addr) = self.target_bd_addr {
            if target_addr == bd_addr {
                return true;
            }
        }
        let target_name = self.target_name();
        if !target_name.is_empty() {
            if let Some(eir_name) = parse_eir_local_name(eir_data) {
                if ascii_contains_ignore_case(eir_name, target_name) {
                    return true;
                }
            }
        }
        false
    }

    /// Starts Bluetooth Classic GIAC Inquiry (`0x9E8B33`) to discover the
    /// target A2DP Sink.
    pub fn start_inquiry<T: HciTransport>(
        &mut self,
        transport: &mut T,
        inquiry_length_units: u8,
    ) -> bool {
        let len = inquiry_length_units.clamp(1, 0x30);
        // LAP = 0x9E8B33 (GIAC), Inquiry_Length, Num_Responses = 0
        let params: [u8; 5] = [0x33, 0x8B, 0x9E, len, 0x00];
        if BtA2dpSink::send_cmd(transport, OPCODE_INQUIRY, &params) {
            self.phase = BtA2dpSourcePhase::Inquiring;
            true
        } else {
            false
        }
    }

    /// Processes an HCI `ExtendedInquiryResult` (`0x2F`) event; if the peer
    /// matches the configured sink name/MAC, cancels inquiry and initiates
    /// `HCI_Create_Connection` (`0x0405`).
    pub fn on_extended_inquiry_result<T: HciTransport>(
        &mut self,
        transport: &mut T,
        params: &[u8],
    ) -> bool {
        if params.len() < 15 {
            return false;
        }
        let mut bd_addr = [0u8; 6];
        bd_addr.copy_from_slice(&params[1..7]);
        let page_scan_rep = params[7];
        let clock_offset = u16::from_le_bytes([params[12], params[13]]);
        let rssi = params[14] as i8;
        let eir_data = &params[15..];

        if !self.matches_inquiry_peer(&bd_addr, eir_data) {
            return false;
        }

        self.matched_bd_addr = Some(bd_addr);
        self.matched_rssi = rssi;
        let _ = BtA2dpSink::send_cmd(transport, OPCODE_INQUIRY_CANCEL, &[]);

        // HCI_Create_Connection (0x0405): BD_ADDR(6), Packet_Type(0xCC18),
        // Page_Scan_Rep_Mode(1), Reserved(0), Clock_Offset(2), Role_Switch(1)
        let mut conn_params = [0u8; 13];
        conn_params[0..6].copy_from_slice(&bd_addr);
        conn_params[6..8].copy_from_slice(&0xCC18u16.to_le_bytes());
        conn_params[8] = page_scan_rep;
        conn_params[9] = 0x00;
        conn_params[10..12].copy_from_slice(&(clock_offset | 0x8000).to_le_bytes());
        conn_params[12] = 0x01;
        if BtA2dpSink::send_cmd(transport, OPCODE_CREATE_CONNECTION, &conn_params) {
            self.phase = BtA2dpSourcePhase::ConnectingAcl;
            true
        } else {
            false
        }
    }

    /// Transitions from ACL connection completion to opening the L2CAP AVDTP
    /// Signaling channel (`PSM 0x0019`).
    pub fn on_acl_connected<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        ident: u8,
    ) -> bool {
        self.acl_handle = Some(handle);
        self.phase = BtA2dpSourcePhase::ConnectingAvdtpSignaling;
        let mut req = [0u8; 8];
        req[0] = L2CAP_CMD_CONN_REQ;
        req[1] = ident;
        req[2..4].copy_from_slice(&4u16.to_le_bytes());
        req[4..6].copy_from_slice(&PSM_AVDTP.to_le_bytes());
        req[6..8].copy_from_slice(&self.sig_local_cid.to_le_bytes());
        BtA2dpSink::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &req)
    }

    /// Initiates the AVDTP Source `DISCOVER` (`0x01`) request once the
    /// signaling L2CAP channel is configured.
    pub fn start_avdtp_discovery<T: HciTransport>(
        &mut self,
        transport: &mut T,
        sig_remote_cid: u16,
    ) -> bool {
        let Some(handle) = self.acl_handle else {
            return false;
        };
        self.sig_remote_cid = sig_remote_cid;
        self.phase = BtA2dpSourcePhase::DiscoveringSeps;
        let lbl = self.next_tx_label();
        let cmd = [lbl << 4, AVDTP_DISCOVER];
        BtA2dpSink::send_l2cap(transport, handle, sig_remote_cid, &cmd)
    }

    /// Advances the AVDTP Source initiator state machine upon receiving an
    /// AVDTP signaling response from the remote A2DP Sink:
    /// `Discover` -> `GetCapabilities` -> `SetConfiguration` -> `Open` ->
    /// (L2CAP Media Channel) -> `Start` -> `Streaming`.
    pub fn handle_avdtp_response<T: HciTransport>(
        &mut self,
        transport: &mut T,
        payload: &[u8],
    ) -> bool {
        let Some(handle) = self.acl_handle else {
            return false;
        };
        if payload.len() < 2 {
            return false;
        }
        let msg_type = payload[0] & 0x03;
        let sig_id = payload[1] & 0x3F;
        if msg_type != 0x02 {
            // Only advance on Response Accept (0x02)
            return false;
        }

        match sig_id {
            AVDTP_DISCOVER => {
                // Parse discovered SEPs (2 bytes each): find Audio Sink (TSEP = 1)
                let mut offset = 2usize;
                let mut found_seid = None;
                while offset + 1 < payload.len() {
                    let seid = (payload[offset] >> 2) & 0x3F;
                    let in_use = (payload[offset] & 0x02) != 0;
                    let media_type = (payload[offset + 1] >> 4) & 0x0F;
                    let tsep = (payload[offset + 1] >> 3) & 0x01;
                    if media_type == 0x00 && tsep == 0x01 && !in_use && seid > 0 {
                        found_seid = Some(seid);
                        break;
                    }
                    offset += 2;
                }
                let seid = found_seid.unwrap_or(1);
                self.remote_sink_seid = seid;
                self.phase = BtA2dpSourcePhase::GettingCapabilities;
                let lbl = self.next_tx_label();
                let cmd = [lbl << 4, AVDTP_GET_CAPABILITIES, seid << 2];
                BtA2dpSink::send_l2cap(transport, handle, self.sig_remote_cid, &cmd)
            }
            AVDTP_GET_CAPABILITIES | AVDTP_GET_ALL_CAPABILITIES => {
                // Inspect SBC Media Codec capability (0x07, 0x06, 0x00, 0x00,
                // oct0, oct1, min_bp, max_bp)
                for i in 2..payload.len().saturating_sub(7) {
                    if payload[i] == 0x07
                        && payload[i + 1] == 0x06
                        && payload[i + 2] == 0x00
                        && payload[i + 3] == 0x00
                    {
                        let peer_max_bp = payload[i + 7];
                        if peer_max_bp >= 2 {
                            self.sbc_bitpool = peer_max_bp.min(53);
                        }
                        break;
                    }
                }
                self.locked_sample_rate_hz = Self::LMS_LOCKED_RATE_HZ;
                self.phase = BtA2dpSourcePhase::SettingConfiguration;
                let lbl = self.next_tx_label();
                // 44.1 kHz (0x20) + Joint Stereo (0x01) = 0x21;
                // 16 blocks (0x10) + 8 subbands (0x04) + Loudness (0x01) = 0x15
                let cmd: [u8; 14] = [
                    lbl << 4,
                    AVDTP_SET_CONFIGURATION,
                    self.remote_sink_seid << 2,
                    self.local_source_seid << 2,
                    0x01,
                    0x00, // Media Transport
                    0x07,
                    0x06,
                    0x00,
                    0x00,
                    0x21,
                    0x15,
                    2,
                    self.sbc_bitpool,
                ];
                BtA2dpSink::send_l2cap(transport, handle, self.sig_remote_cid, &cmd)
            }
            AVDTP_SET_CONFIGURATION => {
                self.phase = BtA2dpSourcePhase::OpeningStream;
                let lbl = self.next_tx_label();
                let cmd = [lbl << 4, AVDTP_OPEN, self.remote_sink_seid << 2];
                BtA2dpSink::send_l2cap(transport, handle, self.sig_remote_cid, &cmd)
            }
            AVDTP_OPEN => {
                self.phase = BtA2dpSourcePhase::ConnectingAvdtpMedia;
                let mut req = [0u8; 8];
                req[0] = L2CAP_CMD_CONN_REQ;
                req[1] = self.next_tx_label().max(1);
                req[2..4].copy_from_slice(&4u16.to_le_bytes());
                req[4..6].copy_from_slice(&PSM_AVDTP.to_le_bytes());
                req[6..8].copy_from_slice(&self.media_local_cid.to_le_bytes());
                BtA2dpSink::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &req)
            }
            AVDTP_START => {
                self.phase = BtA2dpSourcePhase::Streaming;
                true
            }
            _ => false,
        }
    }

    /// Sends `AVDTP_START` (`0x07`) once the L2CAP media transport channel is
    /// configured.
    pub fn on_media_channel_configured<T: HciTransport>(
        &mut self,
        transport: &mut T,
        media_remote_cid: u16,
    ) -> bool {
        let Some(handle) = self.acl_handle else {
            return false;
        };
        self.media_remote_cid = media_remote_cid;
        self.phase = BtA2dpSourcePhase::StartingStream;
        let lbl = self.next_tx_label();
        let cmd = [lbl << 4, AVDTP_START, self.remote_sink_seid << 2];
        BtA2dpSink::send_l2cap(transport, handle, self.sig_remote_cid, &cmd)
    }

    /// Wraps one or more encoded SBC frames in an A2DP RTP media packet
    /// (12-byte RTP header + 1-byte SBC payload header + `sbc_frames`).
    pub fn build_rtp_media_packet(
        &mut self,
        sbc_frames: &[u8],
        frame_count: u8,
        samples_per_frame: u32,
        out: &mut [u8],
    ) -> usize {
        let total_len = 13 + sbc_frames.len();
        if out.len() < total_len || frame_count == 0 {
            return 0;
        }
        out[0] = 0x80; // RTP v2
        out[1] = 0x60; // Payload Type 96 (dynamic A2DP SBC)
        out[2..4].copy_from_slice(&self.rtp_seq_num.to_be_bytes());
        out[4..8].copy_from_slice(&self.rtp_timestamp.to_be_bytes());
        out[8..12].copy_from_slice(&self.rtp_ssrc.to_be_bytes());
        out[12] = frame_count & 0x0F;
        out[13..total_len].copy_from_slice(sbc_frames);

        self.rtp_seq_num = self.rtp_seq_num.wrapping_add(1);
        self.rtp_timestamp = self
            .rtp_timestamp
            .wrapping_add(u32::from(frame_count) * samples_per_frame);
        total_len
    }
}

// ============================================================================
// Dual-Mode Bluetooth A2DP Sink Engine
// ============================================================================

pub struct BtA2dpSink {
    pub init_step: InitStep,
    pub last_init_step_sent: Option<InitStep>,
    pub last_init_sent_ms: u64,
    pub rx_packets: u32,
    pub decoded_frames: u64,
    pub bond_table: [BondEntry; 8],
    pub bond_count: usize,
    pub bonding_store: BondingStore,
    pub security_policy: BtSecurityPolicy,
    pub active_handle: Option<u16>,
    pub peer_bd_addr: [u8; 6],
    pub acl_reassembly: AclReassembly,
    pub channels: [L2capChannel; 8],
    pub next_local_cid: u16,
    pub next_ident: u8,
    pub avdtp_state: AvdtpStreamState,
    pub sample_rate_hz: u32,
    pub in_use: bool,
    pub avrcp_volume_raw: u8,
    pub avrcp_volume_pct: u8,
    pub avrcp_vol_notify_tx_label: Option<u8>,
    pub last_passthrough_op: Option<AvrcpOperationId>,
    pub last_passthrough_state: Option<AvrcpButtonState>,
    pub track_metadata: AvrcpTrackMetadata,
    pub sbc_decoder: SbcDecoder,
    pub pcm_staging: [i16; 512],
    pub pcm_bytes: [u8; 1024],
}

impl BtA2dpSink {
    pub const fn new() -> Self {
        Self {
            init_step: InitStep::Reset,
            last_init_step_sent: None,
            last_init_sent_ms: 0,
            rx_packets: 0,
            decoded_frames: 0,
            bond_table: [BondEntry::empty(); 8],
            bond_count: 0,
            bonding_store: BondingStore::new(),
            security_policy: BtSecurityPolicy::new(),
            active_handle: None,
            peer_bd_addr: [0; 6],
            acl_reassembly: AclReassembly::new(),
            channels: [L2capChannel::empty(); 8],
            next_local_cid: 0x0040,
            next_ident: 1,
            avdtp_state: AvdtpStreamState::Idle,
            sample_rate_hz: 44100,
            in_use: false,
            avrcp_volume_raw: 0x40,
            avrcp_volume_pct: 50,
            avrcp_vol_notify_tx_label: None,
            last_passthrough_op: None,
            last_passthrough_state: None,
            track_metadata: AvrcpTrackMetadata::new(),
            sbc_decoder: SbcDecoder::new(),
            pcm_staging: [0; 512],
            pcm_bytes: [0; 1024],
        }
    }

    pub fn reset_in_place(&mut self) {
        self.init_step = InitStep::Reset;
        self.last_init_step_sent = None;
        self.last_init_sent_ms = 0;
        self.rx_packets = 0;
        self.decoded_frames = 0;
        self.bond_table = [BondEntry::empty(); 8];
        self.bond_count = 0;
        self.bonding_store.clear();
        self.security_policy = BtSecurityPolicy::new();
        self.active_handle = None;
        self.peer_bd_addr = [0; 6];
        self.acl_reassembly.reset();
        self.channels = [L2capChannel::empty(); 8];
        self.next_local_cid = 0x0040;
        self.next_ident = 1;
        self.avdtp_state = AvdtpStreamState::Idle;
        self.sample_rate_hz = 44100;
        self.in_use = false;
        self.avrcp_volume_raw = 0x40;
        self.avrcp_volume_pct = 50;
        self.avrcp_vol_notify_tx_label = None;
        self.last_passthrough_op = None;
        self.last_passthrough_state = None;
        self.track_metadata.clear();
        self.sbc_decoder.reset(8);
        self.pcm_staging.fill(0);
        self.pcm_bytes.fill(0);
    }

    pub fn is_initialized(&self) -> bool {
        self.init_step == InitStep::Completed
    }

    pub fn is_connected(&self) -> bool {
        self.active_handle.is_some()
    }

    pub fn is_streaming(&self) -> bool {
        self.avdtp_state == AvdtpStreamState::Streaming
    }

    pub fn next_ident(&mut self) -> u8 {
        let id = self.next_ident;
        self.next_ident = if id == 255 { 1 } else { id + 1 };
        id
    }

    /// Reset L2CAP and AVDTP state for a disconnected handle.
    pub fn handle_disconnection(&mut self, handle: u16) {
        if self.active_handle == Some(handle) {
            self.active_handle = None;
        }
        for ch in &mut self.channels {
            if ch.handle == handle {
                *ch = L2capChannel::empty();
            }
        }
        self.acl_reassembly.reset();
        self.avdtp_state = AvdtpStreamState::Idle;
        self.in_use = false;
    }

    /// Step the HCI controller initialization sequence forward.
    /// Emits the current command if pending and returns true if a command
    /// was transmitted.
    pub fn step_init<T: HciTransport>(&mut self, transport: &mut T) -> bool {
        match self.init_step {
            InitStep::Reset => {
                // HCI_Reset (0x0C03)
                Self::send_cmd(transport, OPCODE_RESET, &[])
            }
            InitStep::SetEventMask => {
                // HCI_Set_Event_Mask (0x0C01) -> 0x3DBFF807FFFBFFFF
                let mask: [u8; 8] = [0xFF, 0xFF, 0xFB, 0xFF, 0x07, 0xF8, 0xBF, 0x3D];
                Self::send_cmd(transport, OPCODE_SET_EVENT_MASK, &mask)
            }
            InitStep::WriteLocalName => {
                // HCI_Write_Local_Name (0x0C13) -> configured device name + 0
                let mut name_param = [0u8; 248];
                let name = self.security_policy.device_name_bytes();
                let copy_len = name.len().min(247);
                name_param[..copy_len].copy_from_slice(&name[..copy_len]);
                Self::send_cmd(transport, OPCODE_WRITE_LOCAL_NAME, &name_param)
            }
            InitStep::WriteClassOfDevice => {
                // HCI_Write_Class_Of_Device (0x0C24) -> 0x240414
                let cod = [0x14, 0x04, 0x24];
                Self::send_cmd(transport, OPCODE_WRITE_CLASS_OF_DEVICE, &cod)
            }
            InitStep::WriteSimplePairingMode => {
                // HCI_Write_Simple_Pairing_Mode (0x0C56) -> 0x01 or 0x00
                let ssp_byte = [u8::from(self.security_policy.ssp_enabled())];
                Self::send_cmd(transport, OPCODE_WRITE_SIMPLE_PAIRING_MODE, &ssp_byte)
            }
            InitStep::WriteExtendedInquiryResponse => {
                // HCI_Write_Extended_Inquiry_Response (0x0C52): FEC_Req (1) + 240 bytes EIR
                let mut eir_param = [0u8; 241];
                eir_param[0] = 0x00; // FEC Not Required
                let mut offset = 1;

                // AD 1: Complete Local Name (0x09)
                let name = self.security_policy.device_name_bytes();
                let name_len = name.len().min(64);
                eir_param[offset] = (1 + name_len) as u8;
                eir_param[offset + 1] = 0x09;
                eir_param[offset + 2..offset + 2 + name_len].copy_from_slice(&name[..name_len]);
                offset += 2 + name_len;

                // AD 2: Complete List of 16-bit Service Class UUIDs (0x03)
                // 0x110B (Audio Sink), 0x110D (A2DP), 0x110E (AVRCP), 0x110C (AVRCP Target)
                let uuids: [u8; 8] = [0x0B, 0x11, 0x0D, 0x11, 0x0E, 0x11, 0x0C, 0x11];
                eir_param[offset] = (1 + uuids.len()) as u8;
                eir_param[offset + 1] = 0x03;
                eir_param[offset + 2..offset + 2 + uuids.len()].copy_from_slice(&uuids);
                offset += 2 + uuids.len();

                // AD 3: Tx Power Level (0x0A): +9 dBm
                eir_param[offset] = 0x02;
                eir_param[offset + 1] = 0x0A;
                eir_param[offset + 2] = 0x09;

                Self::send_cmd(transport, OPCODE_WRITE_EXTENDED_INQUIRY_RESPONSE, &eir_param)
            }
            InitStep::WriteInquiryMode => {
                // HCI_Write_Inquiry_Mode (0x0C45) -> 0x02
                Self::send_cmd(transport, OPCODE_WRITE_INQUIRY_MODE, &[0x02])
            }
            InitStep::WritePageScanActivity => {
                // Interval = 0x0800, Window = 0x0012
                let params = [0x00, 0x08, 0x12, 0x00];
                Self::send_cmd(transport, OPCODE_WRITE_PAGE_SCAN_ACTIVITY, &params)
            }
            InitStep::WritePageScanType => {
                // 0x01 = Interlaced Scan
                Self::send_cmd(transport, OPCODE_WRITE_PAGE_SCAN_TYPE, &[0x01])
            }
            InitStep::WriteInquiryScanActivity => {
                // Interval = 0x0800, Window = 0x0012
                let params = [0x00, 0x08, 0x12, 0x00];
                Self::send_cmd(transport, OPCODE_WRITE_INQUIRY_SCAN_ACTIVITY, &params)
            }
            InitStep::WriteInquiryScanType => {
                // 0x01 = Interlaced Scan
                Self::send_cmd(transport, OPCODE_WRITE_INQUIRY_SCAN_TYPE, &[0x01])
            }
            InitStep::WriteDefaultLinkPolicySettings => {
                // 0x0007 = Role Switch + Hold + Sniff
                let params = [0x07, 0x00];
                Self::send_cmd(transport, OPCODE_WRITE_DEFAULT_LINK_POLICY_SETTINGS, &params)
            }
            InitStep::WriteScanEnable => {
                // 0x03 = Inquiry Scan + Page Scan
                Self::send_cmd(transport, OPCODE_WRITE_SCAN_ENABLE, &[0x03])
            }
            InitStep::LeSetAdvertisingParameters => {
                // Min=0x0100, Max=0x0200, Type=0x00 (ADV_IND), Public,
                // Peer=0 (6 bytes), ChannelMap=0x07, Filter=0
                let params: [u8; 15] = [
                    0x00, 0x01, // Advertising_Interval_Min = 0x0100
                    0x00, 0x02, // Advertising_Interval_Max = 0x0200
                    0x00,       // Advertising_Type = ADV_IND
                    0x00,       // Own_Address_Type = Public
                    0x00,       // Peer_Address_Type = Public
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Peer_Address (6 bytes, indices 7..13)
                    0x07,       // Advertising_Channel_Map = 0x07 (index 13: ch 37, 38, 39)
                    0x00,       // Advertising_Filter_Policy = 0x00 (index 14)
                ];
                Self::send_cmd(transport, OPCODE_LE_SET_ADV_PARAMS, &params)
            }
            InitStep::LeSetAdvertisingData => {
                let mut param = [0u8; 32];
                // Length byte + data
                // Flags: len 2, type 0x01, value 0x1A
                // UUID: len 3, type 0x03, 0x110B
                // Name: len 1 + name, type 0x09, configured device name
                let full_name = self.security_policy.device_name_bytes();
                let name = &full_name[..full_name.len().min(22)];
                let len = 3 + 4 + (2 + name.len());
                param[0] = len as u8;
                let mut off = 1;

                param[off] = 0x02;
                param[off + 1] = 0x01;
                param[off + 2] = 0x1A;
                off += 3;

                param[off] = 0x03;
                param[off + 1] = 0x03;
                param[off + 2] = 0x0B;
                param[off + 3] = 0x11;
                off += 4;

                param[off] = (1 + name.len()) as u8;
                param[off + 1] = 0x09;
                param[off + 2..off + 2 + name.len()].copy_from_slice(name);

                Self::send_cmd(transport, OPCODE_LE_SET_ADV_DATA, &param)
            }
            InitStep::LeSetScanResponseData => {
                let mut param = [0u8; 32];
                let full_name = self.security_policy.device_name_bytes();
                let name = &full_name[..full_name.len().min(29)];
                param[0] = (2 + name.len()) as u8;
                param[1] = (1 + name.len()) as u8;
                param[2] = 0x09;
                param[3..3 + name.len()].copy_from_slice(name);
                Self::send_cmd(transport, OPCODE_LE_SET_SCAN_RSP_DATA, &param)
            }
            InitStep::LeSetAdvertisingEnable => {
                Self::send_cmd(transport, OPCODE_LE_SET_ADV_ENABLE, &[0x01])
            }
            InitStep::Completed => false,
        }
    }

    /// Advance the initialization state machine after an opcode finishes.
    pub fn on_command_complete(&mut self, opcode: u16) {
        match self.init_step {
            InitStep::Reset if opcode == OPCODE_RESET => {
                self.init_step = InitStep::SetEventMask;
            }
            InitStep::SetEventMask if opcode == OPCODE_SET_EVENT_MASK => {
                self.init_step = InitStep::WriteLocalName;
            }
            InitStep::WriteLocalName if opcode == OPCODE_WRITE_LOCAL_NAME => {
                self.init_step = InitStep::WriteClassOfDevice;
            }
            InitStep::WriteClassOfDevice if opcode == OPCODE_WRITE_CLASS_OF_DEVICE => {
                self.init_step = InitStep::WriteSimplePairingMode;
            }
            InitStep::WriteSimplePairingMode if opcode == OPCODE_WRITE_SIMPLE_PAIRING_MODE => {
                self.init_step = InitStep::WriteExtendedInquiryResponse;
            }
            InitStep::WriteExtendedInquiryResponse
                if opcode == OPCODE_WRITE_EXTENDED_INQUIRY_RESPONSE =>
            {
                self.init_step = InitStep::WriteInquiryMode;
            }
            InitStep::WriteInquiryMode if opcode == OPCODE_WRITE_INQUIRY_MODE => {
                self.init_step = InitStep::WritePageScanActivity;
            }
            InitStep::WritePageScanActivity if opcode == OPCODE_WRITE_PAGE_SCAN_ACTIVITY => {
                self.init_step = InitStep::WritePageScanType;
            }
            InitStep::WritePageScanType if opcode == OPCODE_WRITE_PAGE_SCAN_TYPE => {
                self.init_step = InitStep::WriteInquiryScanActivity;
            }
            InitStep::WriteInquiryScanActivity
                if opcode == OPCODE_WRITE_INQUIRY_SCAN_ACTIVITY =>
            {
                self.init_step = InitStep::WriteInquiryScanType;
            }
            InitStep::WriteInquiryScanType if opcode == OPCODE_WRITE_INQUIRY_SCAN_TYPE => {
                self.init_step = InitStep::WriteDefaultLinkPolicySettings;
            }
            InitStep::WriteDefaultLinkPolicySettings
                if opcode == OPCODE_WRITE_DEFAULT_LINK_POLICY_SETTINGS =>
            {
                self.init_step = InitStep::WriteScanEnable;
            }
            InitStep::WriteScanEnable if opcode == OPCODE_WRITE_SCAN_ENABLE => {
                self.init_step = InitStep::LeSetAdvertisingParameters;
            }
            InitStep::LeSetAdvertisingParameters if opcode == OPCODE_LE_SET_ADV_PARAMS => {
                self.init_step = InitStep::LeSetAdvertisingData;
            }
            InitStep::LeSetAdvertisingData if opcode == OPCODE_LE_SET_ADV_DATA => {
                self.init_step = InitStep::LeSetScanResponseData;
            }
            InitStep::LeSetScanResponseData if opcode == OPCODE_LE_SET_SCAN_RSP_DATA => {
                self.init_step = InitStep::LeSetAdvertisingEnable;
            }
            InitStep::LeSetAdvertisingEnable if opcode == OPCODE_LE_SET_ADV_ENABLE => {
                self.init_step = InitStep::Completed;
            }
            _ => {}
        }
    }

    /// Helper to send an HCI Command packet over the transport.
    pub fn send_cmd<T: HciTransport>(transport: &mut T, opcode: u16, params: &[u8]) -> bool {
        if params.len() > 255 {
            return false;
        }
        let mut buf = [0u8; 260];
        buf[0] = HCI_PKT_COMMAND;
        buf[1] = opcode as u8;
        buf[2] = (opcode >> 8) as u8;
        buf[3] = params.len() as u8;
        buf[4..4 + params.len()].copy_from_slice(params);
        transport.write_packet(&buf[..4 + params.len()])
    }

    /// Helper to send an L2CAP packet inside an HCI ACL packet.
    pub fn send_l2cap<T: HciTransport>(
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) -> bool {
        if payload.len() > 1024 - 9 {
            return false;
        }
        let mut buf = [0u8; 1024];
        let l2cap_len = payload.len();
        let acl_data_len = 4 + l2cap_len;
        let total_len = 5 + acl_data_len;

        buf[0] = HCI_PKT_ACL;
        // Handle + PB=2 (0b10 start automatically flushable) + BC=0
        let handle_flags = (handle & 0x0FFF) | (0b10 << 12);
        buf[1] = handle_flags as u8;
        buf[2] = (handle_flags >> 8) as u8;
        buf[3] = acl_data_len as u8;
        buf[4] = (acl_data_len >> 8) as u8;

        // L2CAP header
        buf[5] = l2cap_len as u8;
        buf[6] = (l2cap_len >> 8) as u8;
        buf[7] = cid as u8;
        buf[8] = (cid >> 8) as u8;

        buf[9..9 + l2cap_len].copy_from_slice(payload);
        transport.write_packet(&buf[..total_len])
    }

    /// Process a received HCI packet (Event or ACL).
    pub fn process_packet<T: HciTransport, F: FnMut(&[u8], u32)>(
        &mut self,
        transport: &mut T,
        pkt: &[u8],
        mut on_pcm: F,
    ) {
        if pkt.is_empty() {
            return;
        }

        match pkt[0] {
            HCI_PKT_EVENT => self.handle_hci_event(transport, &pkt[1..]),
            HCI_PKT_ACL => self.handle_hci_acl(transport, &pkt[1..], &mut on_pcm),
            _ => {}
        }
    }

    fn handle_hci_event<T: HciTransport>(&mut self, transport: &mut T, event_pkt: &[u8]) {
        if event_pkt.len() < 2 {
            return;
        }
        let evt_code = event_pkt[0];
        let param_len = event_pkt[1] as usize;
        let params = &event_pkt[2..2 + param_len.min(event_pkt.len().saturating_sub(2))];

        match evt_code {
            EVT_COMMAND_COMPLETE => {
                if params.len() >= 4 && params[3] == 0x00 {
                    let opcode = u16::from_le_bytes([params[1], params[2]]);
                    self.on_command_complete(opcode);
                }
            }
            EVT_COMMAND_STATUS => {
                if params.len() >= 4 && params[0] == 0x00 {
                    let opcode = u16::from_le_bytes([params[2], params[3]]);
                    self.on_command_complete(opcode);
                }
            }
            EVT_CONNECTION_REQUEST => {
                if params.len() >= 10 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[0..6]);
                    let link_type = params[9];
                    if link_type == 0x01 {
                        // Accept ACL connection request as slave (0x01)
                        let mut accept_param = [0u8; 7];
                        accept_param[..6].copy_from_slice(&bd_addr);
                        accept_param[6] = 0x01; // Role: Slave
                        Self::send_cmd(transport, OPCODE_ACCEPT_CONNECTION_REQUEST, &accept_param);
                    }
                }
            }
            EVT_CONNECTION_COMPLETE => {
                if params.len() >= 11 && params[0] == 0x00 {
                    let handle = u16::from_le_bytes([params[1], params[2]]) & 0x0FFF;
                    self.active_handle = Some(handle);
                    self.peer_bd_addr.copy_from_slice(&params[3..9]);
                }
            }
            EVT_DISCONNECTION_COMPLETE => {
                if params.len() >= 4 {
                    let handle = u16::from_le_bytes([params[1], params[2]]) & 0x0FFF;
                    self.handle_disconnection(handle);
                    // Re-enable Scan Enable (0x03) immediately
                    Self::send_cmd(transport, OPCODE_WRITE_SCAN_ENABLE, &[0x03]);
                }
            }
            EVT_PIN_CODE_REQUEST => {
                if params.len() >= 6 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[..6]);
                    if self.security_policy.pairing_policy == BtPairingPolicy::BondedOnly
                        && self.find_bond_key(&bd_addr).is_none()
                    {
                        Self::send_cmd(
                            transport,
                            OPCODE_PIN_CODE_REQUEST_NEGATIVE_REPLY,
                            &bd_addr,
                        );
                    } else {
                        let pin = self.security_policy.pin_code();
                        let pin_len = pin.len().min(16);
                        let mut pin_reply = [0u8; 23];
                        pin_reply[..6].copy_from_slice(&bd_addr);
                        pin_reply[6] = pin_len as u8;
                        pin_reply[7..7 + pin_len].copy_from_slice(&pin[..pin_len]);
                        Self::send_cmd(transport, OPCODE_PIN_CODE_REQUEST_REPLY, &pin_reply);
                    }
                }
            }
            EVT_LINK_KEY_REQUEST => {
                if params.len() >= 6 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[..6]);
                    let found_key = self
                        .bonding_store
                        .lookup(&bd_addr)
                        .map(|peer| peer.link_key)
                        .or_else(|| self.find_bond_key(&bd_addr));
                    if let Some(key) = found_key {
                        let mut reply = [0u8; 22];
                        reply[..6].copy_from_slice(&bd_addr);
                        reply[6..22].copy_from_slice(&key);
                        Self::send_cmd(transport, OPCODE_LINK_KEY_REQUEST_REPLY, &reply);
                    } else {
                        Self::send_cmd(transport, OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY, &bd_addr);
                    }
                }
            }
            EVT_LINK_KEY_NOTIFICATION => {
                if params.len() >= 22 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[..6]);
                    let mut key = [0u8; 16];
                    key.copy_from_slice(&params[6..22]);
                    let key_type = if params.len() >= 23 { params[22] } else { 0x04 };
                    self.save_bond_key_with_type(&bd_addr, &key, key_type);
                }
            }
            EVT_IO_CAPABILITY_REQUEST => {
                if params.len() >= 6 {
                    let mut reply = [0u8; 9];
                    reply[..6].copy_from_slice(&params[..6]);
                    reply[6] = self.security_policy.io_capability_byte();
                    reply[7] = 0x00; // OOB data not present
                    reply[8] = self.security_policy.auth_requirements_byte();
                    Self::send_cmd(transport, OPCODE_IO_CAPABILITY_REQUEST_REPLY, &reply);
                }
            }
            EVT_USER_CONFIRMATION_REQUEST => {
                if params.len() >= 6 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[..6]);
                    if self.security_policy.pairing_policy == BtPairingPolicy::BondedOnly
                        && self.find_bond_key(&bd_addr).is_none()
                    {
                        Self::send_cmd(
                            transport,
                            OPCODE_USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY,
                            &params[..6],
                        );
                        return;
                    }
                    Self::send_cmd(transport, OPCODE_USER_CONFIRMATION_REQUEST_REPLY, &params[..6]);
                }
            }
            _ => {}
        }
    }

    pub fn find_bond_key(&self, addr: &[u8; 6]) -> Option<[u8; 16]> {
        if let Some(peer) = self.bonding_store.peek(addr) {
            return Some(peer.link_key);
        }
        for bond in &self.bond_table {
            if bond.valid && &bond.bd_addr == addr {
                return Some(bond.link_key);
            }
        }
        None
    }

    pub fn save_bond_key(&mut self, addr: &[u8; 6], key: &[u8; 16]) {
        self.save_bond_key_with_type(addr, key, 0x04);
    }

    pub fn save_bond_key_with_type(&mut self, addr: &[u8; 6], key: &[u8; 16], key_type: u8) {
        self.bonding_store.upsert(*addr, *key, key_type);
        for bond in &mut self.bond_table {
            if bond.valid && &bond.bd_addr == addr {
                bond.link_key = *key;
                return;
            }
        }
        let slot = self.bond_count % 8;
        self.bond_table[slot] = BondEntry {
            bd_addr: *addr,
            link_key: *key,
            valid: true,
        };
        self.bond_count += 1;
    }

    fn handle_hci_acl<T: HciTransport, F: FnMut(&[u8], u32)>(
        &mut self,
        transport: &mut T,
        acl_pkt: &[u8],
        on_pcm: &mut F,
    ) {
        if acl_pkt.len() < 4 {
            return;
        }

        let raw_handle = u16::from_le_bytes([acl_pkt[0], acl_pkt[1]]);
        let handle = raw_handle & 0x0FFF;
        let pb_flag = ((raw_handle >> 12) & 0x03) as u8;
        let acl_data_len = u16::from_le_bytes([acl_pkt[2], acl_pkt[3]]) as usize;
        let payload = &acl_pkt[4..4 + acl_data_len.min(acl_pkt.len().saturating_sub(4))];

        if pb_flag == 0b10 || pb_flag == 0b00 {
            // First fragment / Start of L2CAP PDU
            if payload.len() < 4 {
                return;
            }
            let l2cap_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
            let l2cap_cid = u16::from_le_bytes([payload[2], payload[3]]);

            if payload.len() >= 4 + l2cap_len {
                // Complete PDU in single ACL packet
                self.acl_reassembly.reset();
                self.handle_l2cap_frame(
                    transport,
                    handle,
                    l2cap_cid,
                    &payload[4..4 + l2cap_len],
                    on_pcm,
                );
            } else {
                // Partial PDU: begin reassembly
                if 4 + l2cap_len > 1024 {
                    self.acl_reassembly.reset();
                    return;
                }
                self.acl_reassembly.active = true;
                self.acl_reassembly.handle = handle;
                self.acl_reassembly.expected_len = 4 + l2cap_len;
                let copy_len = payload.len().min(1024);
                self.acl_reassembly.buf[..copy_len].copy_from_slice(&payload[..copy_len]);
                self.acl_reassembly.current_len = copy_len;
            }
        } else if pb_flag == 0b01 {
            // Continuing fragment
            if self.acl_reassembly.active && self.acl_reassembly.handle == handle {
                let remaining = 1024 - self.acl_reassembly.current_len;
                let to_copy = payload.len().min(remaining);
                let cur = self.acl_reassembly.current_len;
                self.acl_reassembly.buf[cur..cur + to_copy].copy_from_slice(&payload[..to_copy]);
                self.acl_reassembly.current_len += to_copy;

                if self.acl_reassembly.current_len >= self.acl_reassembly.expected_len {
                    let total_pdu_len = self.acl_reassembly.expected_len.min(1024);
                    let l2cap_cid = u16::from_le_bytes([
                        self.acl_reassembly.buf[2],
                        self.acl_reassembly.buf[3],
                    ]);
                    self.acl_reassembly.active = false;
                    let mut reassembled = [0u8; 1024];
                    let pdu_data_len = total_pdu_len.saturating_sub(4);
                    reassembled[..pdu_data_len]
                        .copy_from_slice(&self.acl_reassembly.buf[4..total_pdu_len]);
                    self.handle_l2cap_frame(
                        transport,
                        handle,
                        l2cap_cid,
                        &reassembled[..pdu_data_len],
                        on_pcm,
                    );
                }
            }
        }
    }

    fn handle_l2cap_frame<T: HciTransport, F: FnMut(&[u8], u32)>(
        &mut self,
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
        on_pcm: &mut F,
    ) {
        if cid == L2CAP_CID_SIGNALING {
            self.handle_l2cap_signaling(transport, handle, payload);
        } else {
            // Channel payload
            let mut matched = None;
            for ch in &self.channels {
                if ch.state != ChannelState::Closed && ch.handle == handle && ch.local_cid == cid {
                    matched = Some((ch.channel_type, ch.remote_cid));
                    break;
                }
            }

            match matched {
                Some((ChannelType::Sdp, remote_cid)) => {
                    self.handle_sdp_request(transport, handle, remote_cid, payload);
                }
                Some((ChannelType::AvdtpSignaling, remote_cid)) => {
                    self.handle_avdtp_signaling(transport, handle, remote_cid, payload);
                }
                Some((ChannelType::AvdtpMedia, _)) => {
                    self.handle_avdtp_media(payload, on_pcm);
                }
                Some((ChannelType::Avrcp, remote_cid)) => {
                    self.handle_avrcp(transport, handle, remote_cid, payload);
                }
                _ => {}
            }
        }
    }

    fn handle_l2cap_signaling<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        payload: &[u8],
    ) {
        let mut offset = 0;
        while offset + 4 <= payload.len() {
            let code = payload[offset];
            let ident = payload[offset + 1];
            let cmd_len = u16::from_le_bytes([payload[offset + 2], payload[offset + 3]]) as usize;
            let rem_len = payload.len() - (offset + 4);
            let cmd_data = &payload[offset + 4..offset + 4 + cmd_len.min(rem_len)];
            offset += 4 + cmd_len;

            match code {
                L2CAP_CMD_ECHO_REQ => {
                    // Echo response: code 0x09, same ident, same data
                    let mut rsp = [0u8; 128];
                    rsp[0] = L2CAP_CMD_ECHO_RSP;
                    rsp[1] = ident;
                    rsp[2] = cmd_data.len() as u8;
                    rsp[3] = (cmd_data.len() >> 8) as u8;
                    let copy_len = cmd_data.len().min(124);
                    rsp[4..4 + copy_len].copy_from_slice(&cmd_data[..copy_len]);
                    Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp[..4 + copy_len]);
                }
                L2CAP_CMD_INFO_REQ => {
                    if cmd_data.len() >= 2 {
                        let info_type = u16::from_le_bytes([cmd_data[0], cmd_data[1]]);
                        match info_type {
                            0x0002 => {
                                // Extended Features Mask: Fixed Channels Supported (0x00000080)
                                let rsp: [u8; 12] = [
                                    L2CAP_CMD_INFO_RSP,
                                    ident,
                                    0x08,
                                    0x00,
                                    0x02,
                                    0x00, // info_type = 0x0002
                                    0x00,
                                    0x00, // result = Success
                                    0x80,
                                    0x00,
                                    0x00,
                                    0x00, // mask: Fixed Channels
                                ];
                                Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
                            }
                            0x0003 => {
                                // Fixed Channels Supported: CID 0x0001 (bit 1)
                                let rsp: [u8; 16] = [
                                    L2CAP_CMD_INFO_RSP,
                                    ident,
                                    0x0C,
                                    0x00,
                                    0x03,
                                    0x00, // info_type = 0x0003
                                    0x00,
                                    0x00, // result = Success
                                    0x02,
                                    0x00,
                                    0x00,
                                    0x00,
                                    0x00,
                                    0x00,
                                    0x00,
                                    0x00,
                                ];
                                Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
                            }
                            _ => {
                                // Not supported
                                let rsp: [u8; 8] = [
                                    L2CAP_CMD_INFO_RSP,
                                    ident,
                                    0x04,
                                    0x00,
                                    cmd_data[0],
                                    cmd_data[1],
                                    0x01,
                                    0x00, // result = 0x0001 Not supported
                                ];
                                Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
                            }
                        }
                    }
                }
                L2CAP_CMD_CONN_REQ => {
                    if cmd_data.len() >= 4 {
                        let psm = u16::from_le_bytes([cmd_data[0], cmd_data[1]]);
                        let scid = u16::from_le_bytes([cmd_data[2], cmd_data[3]]);
                        self.handle_connection_request(transport, handle, ident, psm, scid);
                    }
                }
                L2CAP_CMD_CONFIG_REQ => {
                    if cmd_data.len() >= 4 {
                        let dcid = u16::from_le_bytes([cmd_data[0], cmd_data[1]]);
                        let flags = u16::from_le_bytes([cmd_data[2], cmd_data[3]]);
                        self.handle_configuration_request(transport, handle, ident, dcid, flags);
                    }
                }
                L2CAP_CMD_CONFIG_RSP => {
                    if cmd_data.len() >= 6 {
                        let scid = u16::from_le_bytes([cmd_data[0], cmd_data[1]]);
                        for ch in &mut self.channels {
                            if ch.state != ChannelState::Closed
                                && ch.handle == handle
                                && ch.local_cid == scid
                            {
                                ch.state = ChannelState::Configured;
                                break;
                            }
                        }
                    }
                }
                L2CAP_CMD_DISCONN_REQ => {
                    if cmd_data.len() >= 4 {
                        let dcid = u16::from_le_bytes([cmd_data[0], cmd_data[1]]);
                        let scid = u16::from_le_bytes([cmd_data[2], cmd_data[3]]);
                        self.handle_disconnection_request(transport, handle, ident, dcid, scid);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_connection_request<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        ident: u8,
        psm: u16,
        scid: u16,
    ) {
        let channel_type = match psm {
            PSM_SDP => ChannelType::Sdp,
            PSM_AVDTP => {
                let mut has_sig = false;
                for ch in &self.channels {
                    if ch.state != ChannelState::Closed
                        && ch.handle == handle
                        && ch.channel_type == ChannelType::AvdtpSignaling
                    {
                        has_sig = true;
                        break;
                    }
                }
                if has_sig {
                    ChannelType::AvdtpMedia
                } else {
                    ChannelType::AvdtpSignaling
                }
            }
            PSM_AVRCP => ChannelType::Avrcp,
            _ => ChannelType::Unknown,
        };

        if channel_type == ChannelType::Unknown {
            // PSM Not Supported: result = 0x0002
            let mut rsp = [0u8; 12];
            rsp[0] = L2CAP_CMD_CONN_RSP;
            rsp[1] = ident;
            rsp[2] = 0x08;
            rsp[3] = 0x00;
            rsp[4..6].copy_from_slice(&0u16.to_le_bytes()); // dcid 0
            rsp[6..8].copy_from_slice(&scid.to_le_bytes());
            rsp[8..10].copy_from_slice(&0x0002u16.to_le_bytes()); // PSM not supported
            rsp[10..12].copy_from_slice(&0x0000u16.to_le_bytes());
            Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
            return;
        }

        // Allocate slot
        let local_cid = self.next_local_cid;
        self.next_local_cid = if self.next_local_cid >= 0xFF00 {
            0x0040
        } else {
            self.next_local_cid + 1
        };

        let mut allocated = false;
        for ch in &mut self.channels {
            if ch.state == ChannelState::Closed {
                *ch = L2capChannel {
                    local_cid,
                    remote_cid: scid,
                    psm,
                    state: ChannelState::ConfigWait,
                    channel_type,
                    handle,
                    outbound_config_sent: true,
                };
                allocated = true;
                break;
            }
        }

        if !allocated {
            // No resources
            let mut rsp = [0u8; 12];
            rsp[0] = L2CAP_CMD_CONN_RSP;
            rsp[1] = ident;
            rsp[2] = 0x08;
            rsp[3] = 0x00;
            rsp[4..6].copy_from_slice(&0u16.to_le_bytes());
            rsp[6..8].copy_from_slice(&scid.to_le_bytes());
            rsp[8..10].copy_from_slice(&0x0004u16.to_le_bytes()); // No resources
            rsp[10..12].copy_from_slice(&0x0000u16.to_le_bytes());
            Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
            return;
        }

        // Send Connection Response: Success (0x0000) followed by outbound
        // Configuration Request (MTU = 895 = 0x037F)
        // Coalesced into a single L2CAP signaling packet (CID 0x0001) to
        // respect VHCI 1-packet-in-flight limit
        let out_id = self.next_ident();
        let mut combined = [0u8; 24];
        // 1. L2CAP_CMD_CONN_RSP (12 bytes)
        combined[0] = L2CAP_CMD_CONN_RSP;
        combined[1] = ident;
        combined[2] = 0x08;
        combined[3] = 0x00;
        combined[4..6].copy_from_slice(&local_cid.to_le_bytes());
        combined[6..8].copy_from_slice(&scid.to_le_bytes());
        combined[8..10].copy_from_slice(&0x0000u16.to_le_bytes()); // Success
        combined[10..12].copy_from_slice(&0x0000u16.to_le_bytes());

        // 2. L2CAP_CMD_CONFIG_REQ (12 bytes)
        combined[12] = L2CAP_CMD_CONFIG_REQ;
        combined[13] = out_id;
        combined[14] = 0x08;
        combined[15] = 0x00;
        combined[16..18].copy_from_slice(&scid.to_le_bytes()); // dcid = peer's scid
        combined[18..20].copy_from_slice(&0x0000u16.to_le_bytes()); // flags = 0
        combined[20] = 0x01; // Option: MTU
        combined[21] = 0x02; // Length: 2
        combined[22..24].copy_from_slice(&0x037Fu16.to_le_bytes()); // MTU: 895

        Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &combined);
    }

    fn handle_configuration_request<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        ident: u8,
        dcid: u16,
        flags: u16,
    ) {
        let mut remote_cid = 0;
        let mut need_outbound_cfg = false;

        for ch in &mut self.channels {
            if ch.state != ChannelState::Closed && ch.handle == handle && ch.local_cid == dcid {
                remote_cid = ch.remote_cid;
                if !ch.outbound_config_sent {
                    ch.outbound_config_sent = true;
                    need_outbound_cfg = true;
                }
                break;
            }
        }

        if remote_cid == 0 {
            return;
        }

        if need_outbound_cfg {
            let out_id = self.next_ident();
            let mut combined = [0u8; 22];
            // 1. L2CAP_CMD_CONFIG_RSP (10 bytes)
            combined[0] = L2CAP_CMD_CONFIG_RSP;
            combined[1] = ident;
            combined[2] = 0x06;
            combined[3] = 0x00;
            combined[4..6].copy_from_slice(&remote_cid.to_le_bytes()); // scid
            combined[6..8].copy_from_slice(&(flags & 0x0001).to_le_bytes());
            combined[8..10].copy_from_slice(&0x0000u16.to_le_bytes()); // Result: Success

            // 2. L2CAP_CMD_CONFIG_REQ (12 bytes)
            combined[10] = L2CAP_CMD_CONFIG_REQ;
            combined[11] = out_id;
            combined[12] = 0x08;
            combined[13] = 0x00;
            combined[14..16].copy_from_slice(&remote_cid.to_le_bytes());
            combined[16..18].copy_from_slice(&0x0000u16.to_le_bytes());
            combined[18] = 0x01;
            combined[19] = 0x02;
            combined[20..22].copy_from_slice(&0x037Fu16.to_le_bytes());

            Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &combined);
        } else {
            let mut rsp = [0u8; 10];
            rsp[0] = L2CAP_CMD_CONFIG_RSP;
            rsp[1] = ident;
            rsp[2] = 0x06;
            rsp[3] = 0x00;
            rsp[4..6].copy_from_slice(&remote_cid.to_le_bytes()); // scid
            rsp[6..8].copy_from_slice(&(flags & 0x0001).to_le_bytes());
            rsp[8..10].copy_from_slice(&0x0000u16.to_le_bytes()); // Result: Success
            Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
        }
    }

    fn handle_disconnection_request<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        ident: u8,
        dcid: u16,
        scid: u16,
    ) {
        let mut was_avdtp = false;
        for ch in &mut self.channels {
            if ch.state != ChannelState::Closed && ch.handle == handle && ch.local_cid == dcid {
                if ch.channel_type == ChannelType::AvdtpSignaling
                    || ch.channel_type == ChannelType::AvdtpMedia
                {
                    was_avdtp = true;
                }
                *ch = L2capChannel::empty();
                break;
            }
        }

        if was_avdtp {
            self.avdtp_state = AvdtpStreamState::Idle;
            self.in_use = false;
        }

        let mut rsp = [0u8; 8];
        rsp[0] = L2CAP_CMD_DISCONN_RSP;
        rsp[1] = ident;
        rsp[2] = 0x04;
        rsp[3] = 0x00;
        rsp[4..6].copy_from_slice(&dcid.to_le_bytes());
        rsp[6..8].copy_from_slice(&scid.to_le_bytes());
        Self::send_l2cap(transport, handle, L2CAP_CID_SIGNALING, &rsp);
    }

    fn handle_sdp_request<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) {
        if payload.len() < 5 {
            return;
        }

        let pdu_id = payload[0];
        let tid = u16::from_be_bytes([payload[1], payload[2]]);
        let params = &payload[5..];

        // Service Record 0x00010001 (A2DP Audio Sink 0x110B)
        // Attribute list elements (77 bytes):
        // 0x0000: Uint32(0x00010001) [8 bytes]
        // 0x0001: DES { UUID16(0x110B) } [8 bytes]
        // 0x0004: ProtocolDescriptorList [21 bytes]
        // 0x0005: DES { UUID16(0x1002 PublicBrowseRoot) } [8 bytes]
        // 0x0009: BluetoothProfileDescriptorList [13 bytes]
        // 0x0100: Text(BT_LOCAL_NAME, 8 bytes) [13 bytes]
        // 0x0311: Uint16(0x0001) [6 bytes]
        const RECORD_ATTRS: [u8; 77] = {
            let mut attrs = [
                0x09, 0x00, 0x00, 0x0A, 0x00, 0x01, 0x00, 0x01,
                0x09, 0x00, 0x01, 0x35, 0x03, 0x19, 0x11, 0x0B,
                0x09, 0x00, 0x04, 0x35, 0x10, 0x35, 0x06, 0x19, 0x01, 0x00, 0x09, 0x00, 0x19, 0x35,
                0x06, 0x19, 0x00, 0x19, 0x09, 0x01, 0x03,
                0x09, 0x00, 0x05, 0x35, 0x03, 0x19, 0x10, 0x02,
                0x09, 0x00, 0x09, 0x35, 0x08, 0x35, 0x06, 0x19, 0x11, 0x0D, 0x09, 0x01, 0x03,
                0x09, 0x01, 0x00, 0x25, 0x08, 0, 0, 0, 0, 0, 0, 0, 0,
                0x09, 0x03, 0x11, 0x09, 0x00, 0x01,
            ];
            // ServiceName (0x0100) is a Text(8) element at bytes 63..71; this
            // fixed record layout only fits an 8-byte name.
            const NAME_AT: usize = 63;
            assert!(
                BT_LOCAL_NAME.len() == 8
                    && attrs[NAME_AT - 2] == 0x25
                    && attrs[NAME_AT - 1] == 0x08
            );
            let mut i = 0;
            while i < 8 {
                attrs[NAME_AT + i] = BT_LOCAL_NAME[i];
                i += 1;
            }
            attrs
        };

        match pdu_id {
            0x02 => {
                // ServiceSearchRequest -> 0x03 ServiceSearchResponse
                let matches = Self::sdp_matches_audio_sink(params);
                let mut rsp = [0u8; 32];
                rsp[0] = 0x03;
                rsp[1..3].copy_from_slice(&tid.to_be_bytes());
                if matches {
                    rsp[3..5].copy_from_slice(&0x0009u16.to_be_bytes()); // Param len
                    // TotalServiceRecordCount = 1
                    rsp[5..7].copy_from_slice(&0x0001u16.to_be_bytes());
                    // CurrentServiceRecordCount = 1
                    rsp[7..9].copy_from_slice(&0x0001u16.to_be_bytes());
                    rsp[9..13].copy_from_slice(&0x00010001u32.to_be_bytes()); // Record handle
                    rsp[13] = 0x00; // ContinuationState
                    Self::send_l2cap(transport, handle, cid, &rsp[..14]);
                } else {
                    rsp[3..5].copy_from_slice(&0x0005u16.to_be_bytes()); // Param len
                    rsp[5..7].copy_from_slice(&0x0000u16.to_be_bytes()); // Total = 0
                    rsp[7..9].copy_from_slice(&0x0000u16.to_be_bytes()); // Current = 0
                    rsp[9] = 0x00; // ContinuationState
                    Self::send_l2cap(transport, handle, cid, &rsp[..10]);
                }
            }
            0x04 => {
                // ServiceAttributeRequest -> 0x05 ServiceAttributeResponse
                if params.len() >= 4 {
                    let req_handle = u32::from_be_bytes([
                        params[0], params[1], params[2], params[3],
                    ]);
                    let mut rsp = [0u8; 128];
                    rsp[0] = 0x05;
                    rsp[1..3].copy_from_slice(&tid.to_be_bytes());

                    if req_handle == 0x00010001 {
                        let attr_list_len = 2 + RECORD_ATTRS.len();
                        let param_len = (2 + attr_list_len + 1) as u16;
                        rsp[3..5].copy_from_slice(&param_len.to_be_bytes());
                        rsp[5..7].copy_from_slice(&(attr_list_len as u16).to_be_bytes());
                        rsp[7] = 0x35; // DES
                        rsp[8] = RECORD_ATTRS.len() as u8;
                        rsp[9..9 + RECORD_ATTRS.len()].copy_from_slice(&RECORD_ATTRS);
                        rsp[9 + RECORD_ATTRS.len()] = 0x00; // ContinuationState
                        Self::send_l2cap(transport, handle, cid, &rsp[..10 + RECORD_ATTRS.len()]);
                    } else {
                        // Empty match
                        rsp[3..5].copy_from_slice(&0x0005u16.to_be_bytes());
                        rsp[5..7].copy_from_slice(&0x0002u16.to_be_bytes());
                        rsp[7] = 0x35;
                        rsp[8] = 0x00;
                        rsp[9] = 0x00;
                        Self::send_l2cap(transport, handle, cid, &rsp[..10]);
                    }
                }
            }
            0x06 => {
                // ServiceSearchAttributeRequest -> 0x07 ServiceSearchAttributeResponse
                let matches = Self::sdp_matches_audio_sink(params);
                let mut rsp = [0u8; 128];
                rsp[0] = 0x07;
                rsp[1..3].copy_from_slice(&tid.to_be_bytes());

                if matches {
                    let inner_des_len = 2 + RECORD_ATTRS.len();
                    let outer_des_len = 2 + inner_des_len;
                    let param_len = (2 + outer_des_len + 1) as u16;
                    rsp[3..5].copy_from_slice(&param_len.to_be_bytes());
                    rsp[5..7].copy_from_slice(&(outer_des_len as u16).to_be_bytes());
                    rsp[7] = 0x35;
                    rsp[8] = inner_des_len as u8;
                    rsp[9] = 0x35;
                    rsp[10] = RECORD_ATTRS.len() as u8;
                    rsp[11..11 + RECORD_ATTRS.len()].copy_from_slice(&RECORD_ATTRS);
                    rsp[11 + RECORD_ATTRS.len()] = 0x00; // ContinuationState
                    Self::send_l2cap(transport, handle, cid, &rsp[..12 + RECORD_ATTRS.len()]);
                } else {
                    // Empty list DES {}
                    rsp[3..5].copy_from_slice(&0x0005u16.to_be_bytes());
                    rsp[5..7].copy_from_slice(&0x0002u16.to_be_bytes());
                    rsp[7] = 0x35;
                    rsp[8] = 0x00;
                    rsp[9] = 0x00;
                    Self::send_l2cap(transport, handle, cid, &rsp[..10]);
                }
            }
            _ => {}
        }
    }

    fn sdp_matches_audio_sink(params: &[u8]) -> bool {
        // Match 16-bit, 32-bit, or 128-bit UUID against known A2DP/AVDTP UUIDs:
        // 0x110B (AudioSink), 0x110D (A2DP), 0x0019 (AVDTP),
        // 0x0100 (L2CAP), 0x1002 (BrowseRoot)
        let is_target = |uuid: u16| -> bool {
            matches!(
                uuid,
                0x110B | 0x110D | 0x0019 | 0x0100 | 0x1002
            )
        };
        const BT_BASE_UUID_SUFFIX: [u8; 12] = [
            0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B, 0x34, 0xFB,
        ];

        let mut i = 0;
        while i < params.len() {
            match params[i] {
                // 16-bit UUID (0x19)
                0x19 if i + 2 < params.len() => {
                    let uuid = u16::from_be_bytes([params[i + 1], params[i + 2]]);
                    if is_target(uuid) {
                        return true;
                    }
                }
                // 32-bit UUID (0x1A)
                0x1A if i + 4 < params.len() => {
                    if params[i + 1] == 0 && params[i + 2] == 0 {
                        let uuid = u16::from_be_bytes([params[i + 3], params[i + 4]]);
                        if is_target(uuid) {
                            return true;
                        }
                    }
                }
                // 128-bit UUID (0x1C) matching Bluetooth Base UUID
                0x1C if i + 16 < params.len() => {
                    if params[i + 1] == 0
                        && params[i + 2] == 0
                        && params[i + 5..i + 17] == BT_BASE_UUID_SUFFIX
                    {
                        let uuid = u16::from_be_bytes([params[i + 3], params[i + 4]]);
                        if is_target(uuid) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn handle_avdtp_signaling<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) {
        if payload.len() < 2 {
            return;
        }

        let tx_label = payload[0] & 0xF0;
        let msg_type = payload[0] & 0x03;
        let sig_id = payload[1] & 0x3F;

        if msg_type != 0x00 {
            // Only respond to commands
            return;
        }

        match sig_id {
            AVDTP_DISCOVER => {
                // Reply: SEID=1, InUse=(in_use), MediaType=Audio(0), TSEP=Sink(1) -> 0x08
                let in_use_flag = if self.avdtp_state != AvdtpStreamState::Idle {
                    0x02
                } else {
                    0x00
                };
                let rsp = [tx_label | 0x02, AVDTP_DISCOVER, 0x04 | in_use_flag, 0x08];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_GET_CAPABILITIES => {
                let rsp = [
                    tx_label | 0x02,
                    AVDTP_GET_CAPABILITIES,
                    0x01,
                    0x00, // Media Transport
                    0x07,
                    0x06, // Media Codec (len 6)
                    0x00, // Audio
                    0x00, // SBC
                    0x3F, // 44.1k + 48k + Mono/Dual/Stereo/Joint
                    0xFF, // 4/8/12/16 blocks, 4/8 subbands, SNR + Loudness
                    2,    // min bitpool
                    53,   // max bitpool
                ];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_GET_ALL_CAPABILITIES => {
                let rsp = [
                    tx_label | 0x02,
                    AVDTP_GET_ALL_CAPABILITIES,
                    0x01,
                    0x00, // Media Transport
                    0x07,
                    0x06, // Media Codec (len 6)
                    0x00, // Audio
                    0x00, // SBC
                    0x3F,
                    0xFF,
                    2,
                    53,
                    0x08,
                    0x00, // Delay Reporting
                ];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_SET_CONFIGURATION | AVDTP_RECONFIGURE => {
                // Scan for SBC codec parameters:
                // 0x07, 0x06, 0x00, 0x00, oct0, oct1, min_bp, max_bp
                for i in 0..payload.len().saturating_sub(5) {
                    if payload[i] == 0x07
                        && payload[i + 1] == 0x06
                        && payload[i + 2] == 0x00
                        && payload[i + 3] == 0x00
                    {
                        let oct0 = payload[i + 4];
                        if (oct0 & 0x10) != 0 {
                            self.sample_rate_hz = 48000;
                        } else {
                            self.sample_rate_hz = 44100;
                        }
                        break;
                    }
                }
                self.avdtp_state = AvdtpStreamState::Configured;
                self.in_use = true;
                let rsp = [tx_label | 0x02, sig_id];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_OPEN => {
                self.avdtp_state = AvdtpStreamState::Open;
                self.in_use = true;
                let rsp = [tx_label | 0x02, AVDTP_OPEN];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_START => {
                self.avdtp_state = AvdtpStreamState::Streaming;
                self.in_use = true;
                let rsp = [tx_label | 0x02, AVDTP_START];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_CLOSE => {
                self.avdtp_state = AvdtpStreamState::Idle;
                self.in_use = false;
                let rsp = [tx_label | 0x02, AVDTP_CLOSE];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_SUSPEND => {
                self.avdtp_state = AvdtpStreamState::Open;
                self.in_use = true;
                let rsp = [tx_label | 0x02, AVDTP_SUSPEND];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_ABORT => {
                self.avdtp_state = AvdtpStreamState::Idle;
                self.in_use = false;
                let rsp = [tx_label | 0x02, AVDTP_ABORT];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_DELAYREPORT => {
                let rsp = [tx_label | 0x02, AVDTP_DELAYREPORT];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            _ => {
                // General Reject: msg_type = 0x01
                let rsp = [tx_label | 0x01, sig_id];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
        }
    }

    fn handle_avdtp_media<F: FnMut(&[u8], u32)>(&mut self, payload: &[u8], on_pcm: &mut F) {
        if self.avdtp_state != AvdtpStreamState::Streaming {
            return;
        }

        // RTP Header is minimum 12 bytes
        if payload.len() < 12 {
            return;
        }

        let cc = (payload[0] & 0x0F) as usize;
        let mut rtp_header_len = 12 + cc * 4;
        if (payload[0] & 0x10) != 0 {
            // RTP header extension present: 4-byte header + ext_words * 4
            if payload.len() < rtp_header_len + 4 {
                return;
            }
            let ext_words = u16::from_be_bytes([
                payload[rtp_header_len + 2],
                payload[rtp_header_len + 3],
            ]) as usize;
            rtp_header_len += 4 + ext_words * 4;
        }

        if payload.len() <= rtp_header_len {
            return;
        }

        // A2DP SBC Payload Header (1 byte)
        // bit 3..0: nrof_frames
        let mut offset = rtp_header_len + 1;

        while offset < payload.len() {
            if payload[offset] != SBC_SYNCWORD {
                offset += 1;
                continue;
            }

            match self.sbc_decoder.decode_frame(&payload[offset..], &mut self.pcm_staging) {
                Ok((bytes_consumed, num_stereo_samples)) => {
                    self.decoded_frames =
                        self.decoded_frames.wrapping_add(num_stereo_samples as u64);
                    let total_samples = (num_stereo_samples * 2).min(512); // 2 channels
                    for i in 0..total_samples {
                        let b = self.pcm_staging[i].to_le_bytes();
                        self.pcm_bytes[i * 2] = b[0];
                        self.pcm_bytes[i * 2 + 1] = b[1];
                    }
                    let num_pcm_bytes = total_samples * 2;
                    on_pcm(&self.pcm_bytes[..num_pcm_bytes], self.sample_rate_hz);
                    offset += bytes_consumed;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    /// Locate the active AVRCP L2CAP channel `(handle, remote_cid)`.
    pub fn avrcp_remote_cid(&self) -> Option<(u16, u16)> {
        let handle = self.active_handle?;
        for ch in &self.channels {
            if ch.state != ChannelState::Closed
                && ch.handle == handle
                && ch.channel_type == ChannelType::Avrcp
            {
                return Some((handle, ch.remote_cid));
            }
        }
        None
    }

    /// Transmit an AVRCP `PASS THROUGH` command (`0x7C`) to the connected peer.
    pub fn send_avrcp_passthrough<T: HciTransport>(
        &self,
        transport: &mut T,
        tx_label: u8,
        op: AvrcpOperationId,
        state: AvrcpButtonState,
    ) -> bool {
        let Some((handle, remote_cid)) = self.avrcp_remote_cid() else {
            return false;
        };
        let mut pkt = [0u8; 8];
        let n = build_avrcp_passthrough_cmd(tx_label, op, state, &mut pkt);
        Self::send_l2cap(transport, handle, remote_cid, &pkt[..n])
    }

    /// Transmit an AVRCP `SetAbsoluteVolume` (`0x50`) command for `0..=100%`.
    pub fn send_avrcp_absolute_volume<T: HciTransport>(
        &mut self,
        transport: &mut T,
        tx_label: u8,
        volume_pct: u8,
    ) -> bool {
        let Some((handle, remote_cid)) = self.avrcp_remote_cid() else {
            return false;
        };
        let vol_raw = volume_pct_to_avrcp_raw(volume_pct);
        self.avrcp_volume_raw = vol_raw;
        self.avrcp_volume_pct = volume_pct.min(100);
        let mut pkt = [0u8; 14];
        let n = build_avrcp_set_absolute_volume_cmd(tx_label, vol_raw, &mut pkt);
        Self::send_l2cap(transport, handle, remote_cid, &pkt[..n])
    }

    /// Emit an AVRCP `EVENT_VOLUME_CHANGED` (`0x0D`) `CHANGED` (`0x0D`)
    /// notification response if the peer has registered for volume updates.
    pub fn notify_avrcp_volume_changed<T: HciTransport>(
        &mut self,
        transport: &mut T,
        volume_pct: u8,
    ) -> bool {
        let vol_raw = volume_pct_to_avrcp_raw(volume_pct);
        self.avrcp_volume_raw = vol_raw;
        self.avrcp_volume_pct = volume_pct.min(100);
        let Some(tx_label) = self.avrcp_vol_notify_tx_label.take() else {
            return false;
        };
        let Some((handle, remote_cid)) = self.avrcp_remote_cid() else {
            return false;
        };
        let mut pkt = [0u8; 15];
        let n = build_avrcp_volume_notify_rsp(tx_label, AVRCP_RSP_CHANGED, vol_raw, &mut pkt);
        Self::send_l2cap(transport, handle, remote_cid, &pkt[..n])
    }

    /// Request track metadata (`GetElementAttributes` `0x20`) from the peer.
    pub fn request_avrcp_track_metadata<T: HciTransport>(
        &self,
        transport: &mut T,
        tx_label: u8,
    ) -> bool {
        let Some((handle, remote_cid)) = self.avrcp_remote_cid() else {
            return false;
        };
        let mut pkt = [0u8; 38];
        let n = build_avrcp_get_element_attributes_cmd(tx_label, &mut pkt);
        Self::send_l2cap(transport, handle, remote_cid, &pkt[..n])
    }

    fn handle_avrcp<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) {
        if payload.len() < 6 {
            return;
        }

        let tx_label = (payload[0] >> 4) & 0x0F;
        let cr = (payload[0] >> 1) & 0x01;
        let pid = u16::from_be_bytes([payload[1], payload[2]]);
        if pid != AVRCP_PID {
            return;
        }
        let ctype_or_rsp = payload[3] & 0x0F;
        let opcode = payload[5];

        if cr == 0 {
            // Command from peer (Controller -> Target)
            if opcode == AVRCP_OPCODE_PASS_THROUGH {
                if let Some((_, _, op, state)) = parse_avrcp_passthrough(payload) {
                    self.last_passthrough_op = Some(op);
                    self.last_passthrough_state = Some(state);
                    let mut rsp = [0u8; 8];
                    let n = build_avrcp_passthrough_rsp(
                        tx_label,
                        op,
                        state,
                        AVRCP_RSP_ACCEPTED,
                        &mut rsp,
                    );
                    Self::send_l2cap(transport, handle, cid, &rsp[..n]);
                    return;
                }
            } else if opcode == AVRCP_OPCODE_VENDOR_DEPENDENT && payload.len() >= 13 {
                if payload[6..9] == BT_SIG_COMPANY_ID {
                    let pdu_id = payload[9];
                    let param_len =
                        u16::from_be_bytes([payload[11], payload[12]]) as usize;
                    let params = &payload[13..payload.len().min(13 + param_len)];

                    match pdu_id {
                        AVRCP_PDU_SET_ABSOLUTE_VOLUME if !params.is_empty() => {
                            let vol_raw = params[0] & 0x7F;
                            self.avrcp_volume_raw = vol_raw;
                            self.avrcp_volume_pct = avrcp_raw_to_volume_pct(vol_raw);
                            let mut rsp = [0u8; 14];
                            let n = build_avrcp_set_absolute_volume_rsp(
                                tx_label,
                                vol_raw,
                                &mut rsp,
                            );
                            Self::send_l2cap(transport, handle, cid, &rsp[..n]);
                            return;
                        }
                        AVRCP_PDU_REGISTER_NOTIFICATION if !params.is_empty() => {
                            let event_id = params[0];
                            if event_id == AVRCP_EVENT_VOLUME_CHANGED {
                                self.avrcp_vol_notify_tx_label = Some(tx_label);
                                let mut rsp = [0u8; 15];
                                let n = build_avrcp_volume_notify_rsp(
                                    tx_label,
                                    AVRCP_RSP_INTERIM,
                                    self.avrcp_volume_raw,
                                    &mut rsp,
                                );
                                Self::send_l2cap(transport, handle, cid, &rsp[..n]);
                                return;
                            } else if event_id == AVRCP_EVENT_PLAYBACK_STATUS_CHANGED {
                                let status_byte = if self.avdtp_state == AvdtpStreamState::Streaming
                                {
                                    0x01 // PLAYING
                                } else {
                                    0x00 // STOPPED
                                };
                                let rsp = [
                                    (tx_label << 4) | 0x02,
                                    (AVRCP_PID >> 8) as u8,
                                    AVRCP_PID as u8,
                                    AVRCP_RSP_INTERIM,
                                    AVRCP_SUBUNIT_PANEL,
                                    AVRCP_OPCODE_VENDOR_DEPENDENT,
                                    BT_SIG_COMPANY_ID[0],
                                    BT_SIG_COMPANY_ID[1],
                                    BT_SIG_COMPANY_ID[2],
                                    AVRCP_PDU_REGISTER_NOTIFICATION,
                                    0x00,
                                    0x00,
                                    0x02,
                                    AVRCP_EVENT_PLAYBACK_STATUS_CHANGED,
                                    status_byte,
                                ];
                                Self::send_l2cap(transport, handle, cid, &rsp);
                                return;
                            }
                        }
                        AVRCP_PDU_GET_ELEMENT_ATTRIBUTES => {
                            let mut rsp = [0u8; 320];
                            let n = build_avrcp_get_element_attributes_rsp(
                                tx_label,
                                &self.track_metadata,
                                &mut rsp,
                            );
                            if n > 0 {
                                Self::send_l2cap(transport, handle, cid, &rsp[..n]);
                                return;
                            }
                        }
                        AVRCP_PDU_GET_CAPABILITIES if !params.is_empty() => {
                            let cap_id = params[0];
                            if cap_id == 0x03 {
                                let rsp = [
                                    (tx_label << 4) | 0x02,
                                    (AVRCP_PID >> 8) as u8,
                                    AVRCP_PID as u8,
                                    AVRCP_RSP_STABLE,
                                    AVRCP_SUBUNIT_PANEL,
                                    AVRCP_OPCODE_VENDOR_DEPENDENT,
                                    BT_SIG_COMPANY_ID[0],
                                    BT_SIG_COMPANY_ID[1],
                                    BT_SIG_COMPANY_ID[2],
                                    AVRCP_PDU_GET_CAPABILITIES,
                                    0x00,
                                    0x00,
                                    0x05,
                                    0x03,
                                    0x03,
                                    AVRCP_EVENT_PLAYBACK_STATUS_CHANGED,
                                    AVRCP_EVENT_TRACK_CHANGED,
                                    AVRCP_EVENT_VOLUME_CHANGED,
                                ];
                                Self::send_l2cap(transport, handle, cid, &rsp);
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Fallback generic response
            let mut rsp = [0u8; 64];
            let len = payload.len().min(64);
            rsp[..len].copy_from_slice(&payload[..len]);
            rsp[0] = (payload[0] & 0xF0) | 0x02;
            rsp[3] = AVRCP_RSP_NOT_IMPLEMENTED;
            Self::send_l2cap(transport, handle, cid, &rsp[..len]);
        } else {
            // Response from peer (Target -> Controller)
            if opcode == AVRCP_OPCODE_VENDOR_DEPENDENT
                && payload.len() >= 13
                && payload[6..9] == BT_SIG_COMPANY_ID
            {
                let pdu_id = payload[9];
                let param_len = u16::from_be_bytes([payload[11], payload[12]]) as usize;
                let params = &payload[13..payload.len().min(13 + param_len)];

                match pdu_id {
                    AVRCP_PDU_SET_ABSOLUTE_VOLUME if !params.is_empty() => {
                        let vol_raw = params[0] & 0x7F;
                        self.avrcp_volume_raw = vol_raw;
                        self.avrcp_volume_pct = avrcp_raw_to_volume_pct(vol_raw);
                    }
                    AVRCP_PDU_REGISTER_NOTIFICATION if params.len() >= 2 => {
                        if params[0] == AVRCP_EVENT_VOLUME_CHANGED
                            && (ctype_or_rsp == AVRCP_RSP_INTERIM
                                || ctype_or_rsp == AVRCP_RSP_CHANGED)
                        {
                            let vol_raw = params[1] & 0x7F;
                            self.avrcp_volume_raw = vol_raw;
                            self.avrcp_volume_pct = avrcp_raw_to_volume_pct(vol_raw);
                        }
                    }
                    AVRCP_PDU_GET_ELEMENT_ATTRIBUTES => {
                        if let Some(meta) = parse_avrcp_element_attributes_rsp(payload) {
                            self.track_metadata = meta;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Polls the Bluetooth transport: drains up to 16 packets and steps
    /// the HCI init sequence.
    pub fn poll<T: HciTransport, F: FnMut(&[u8], u32)>(
        &mut self,
        transport: &mut T,
        now_ms: u64,
        mut on_pcm: F,
    ) -> bool {
        let mut had_activity = false;

        if transport.can_read() {
            let mut rx = [0u8; 1024];

            for _ in 0..16 {
                let n = transport.read_packet(&mut rx);
                if n == 0 {
                    break;
                }
                self.rx_packets = self.rx_packets.wrapping_add(1);
                self.process_packet(transport, &rx[..n], &mut on_pcm);
                had_activity = true;
                if !transport.can_read() {
                    break;
                }
            }
        }

        if self.init_step != InitStep::Completed
            && (self.last_init_step_sent != Some(self.init_step)
                || now_ms.saturating_sub(self.last_init_sent_ms) >= 250)
        {
            if self.step_init(transport) {
                self.last_init_step_sent = Some(self.init_step);
                self.last_init_sent_ms = now_ms;
                had_activity = true;
            }
        }

        had_activity
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct MockHciTransport {
        pub tx_packets: Vec<Vec<u8>>,
        pub rx_queue: Vec<Vec<u8>>,
    }

    impl MockHciTransport {
        pub fn new() -> Self {
            Self {
                tx_packets: Vec::new(),
                rx_queue: Vec::new(),
            }
        }

        #[allow(dead_code)]
        pub fn queue_rx(&mut self, pkt: &[u8]) {
            self.rx_queue.push(pkt.to_vec());
        }
    }

    impl HciTransport for MockHciTransport {
        fn can_read(&self) -> bool {
            !self.rx_queue.is_empty()
        }

        fn read_packet(&mut self, buf: &mut [u8]) -> usize {
            if self.rx_queue.is_empty() {
                0
            } else {
                let pkt = self.rx_queue.remove(0);
                let len = pkt.len().min(buf.len());
                buf[..len].copy_from_slice(&pkt[..len]);
                len
            }
        }

        fn write_packet(&mut self, data: &[u8]) -> bool {
            self.tx_packets.push(data.to_vec());
            true
        }
    }

    // 119-byte valid SBC frame (16 blocks, 8 subbands, Joint Stereo,
    // Loudness, Bitpool 53)
    pub const SAMPLE_SBC_FRAME_8SB: [u8; 119] = [
        0x9C, 0xBD, 0x35, 0xA0, 0xFE, 0xD9, 0x86, 0x66, 0x56, 0x00, 0x00, 0x00, 0x06, 0x80, 0x10,
        0x20, 0x3E, 0xF7, 0xBB, 0xBC, 0x07, 0x7B, 0x01, 0xD7, 0xBD, 0xDD, 0xE1, 0x54, 0x48, 0xB1,
        0xB9, 0xCE, 0xEF, 0x19, 0x25, 0x45, 0x32, 0x71, 0x38, 0x8B, 0x27, 0x5B, 0x8F, 0x65, 0x12,
        0xDD, 0xFA, 0xE2, 0x93, 0xB4, 0x33, 0xB5, 0x56, 0x7C, 0xC6, 0x8A, 0xFB, 0x9C, 0xEE, 0xE3,
        0xFD, 0xEC, 0x18, 0x1C, 0xF8, 0x77, 0x0F, 0xAF, 0xA0, 0x40, 0xF7, 0xBB, 0xBC, 0x16, 0x7E,
        0xFD, 0xF7, 0xBD, 0xDD, 0xFC, 0xA3, 0xF7, 0xEF, 0xBD, 0xEE, 0xEF, 0xB9, 0xDF, 0xBF, 0x7D,
        0xEF, 0x77, 0x75, 0xAE, 0xFD, 0xFB, 0xEF, 0x7B, 0xBB, 0x82, 0x07, 0xEF, 0xDF, 0x7B, 0xDD,
        0xDC, 0xF5, 0xBF, 0x7E, 0xFB, 0xDE, 0xEE, 0xF8, 0x85, 0xFB, 0xF7, 0xDE, 0xF7, 0x77,
    ];

    // 68-byte valid SBC frame (16 blocks, 4 subbands, Stereo, SNR, Bitpool 30)
    pub const SAMPLE_SBC_FRAME_4SB: [u8; 68] = [
        0x9C, 0xBA, 0x1E, 0x3A, 0xD8, 0x77, 0xD8, 0x77, 0x7F, 0x6A, 0xFE, 0xD6, 0x01, 0xAC, 0x03,
        0x58, 0x26, 0xB0, 0x4D, 0x62, 0x22, 0xC4, 0x45, 0x9E, 0x55, 0x3C, 0xAB, 0x78, 0xCE, 0xF1,
        0x9F, 0xB6, 0xBF, 0x6D, 0x7B, 0x5A, 0xF6, 0xB5, 0xBC, 0x6B, 0x78, 0xD5, 0xDD, 0xAB, 0xBB,
        0x53, 0x66, 0xA6, 0xCD, 0x42, 0xDA, 0x85, 0xB5, 0x05, 0x6A, 0x0A, 0xD4, 0x99, 0xA9, 0x33,
        0x56, 0x26, 0xAC, 0x4D, 0x6A, 0x1A, 0xD4, 0x35,
    ];

    #[test]
    fn test_hci_initialization_sequence() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();

        assert_eq!(sink.init_step, InitStep::Reset);

        // Step through the entire sequence, acknowledging each command
        while sink.init_step != InitStep::Completed {
            assert!(sink.step_init(&mut transport));
            let last_cmd = transport.tx_packets.last().expect("command sent");
            assert_eq!(last_cmd[0], HCI_PKT_COMMAND);
            let _opcode = u16::from_le_bytes([last_cmd[1], last_cmd[2]]);

            // Reply with Command Complete Event (0x0E)
            let cmd_complete = [
                HCI_PKT_EVENT,
                EVT_COMMAND_COMPLETE,
                0x04,
                0x01,
                last_cmd[1],
                last_cmd[2],
                0x00,
            ];
            sink.process_packet(&mut transport, &cmd_complete, |_, _| {});
        }

        assert_eq!(sink.init_step, InitStep::Completed);
        assert_eq!(transport.tx_packets.len(), 17);

        // Verify key parameters in sent commands:
        // 1. Reset (0x0C03)
        assert_eq!(
            u16::from_le_bytes([transport.tx_packets[0][1], transport.tx_packets[0][2]]),
            OPCODE_RESET
        );
        // 3. Write Local Name contains the BT local name
        assert_eq!(&transport.tx_packets[2][4..12], BT_LOCAL_NAME);
        // 4. Class Of Device: 0x240414
        assert_eq!(&transport.tx_packets[3][4..7], &[0x14, 0x04, 0x24]);
        // 5. Simple Pairing Mode: 0x01
        assert_eq!(transport.tx_packets[4][4], 0x01);
        // 13. Write Scan Enable: 0x03
        assert_eq!(transport.tx_packets[12][4], 0x03);
        // 17. LE Advertising Enable: 0x01
        assert_eq!(transport.tx_packets[16][4], 0x01);
    }

    #[test]
    fn test_poll_drains_packets_and_steps_init() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();

        assert!(!sink.is_initialized());
        assert!(!sink.is_connected());
        assert!(!sink.is_streaming());

        // Initial poll sends HCI_Reset
        let activity = sink.poll(&mut transport, 0, |_, _| {});
        assert!(activity);
        assert_eq!(transport.tx_packets.len(), 1);
        assert_eq!(sink.last_init_step_sent, Some(InitStep::Reset));

        // Polling before 250ms with no rx returns false
        assert!(!sink.poll(&mut transport, 100, |_, _| {}));

        // Queue command complete for Reset
        transport.queue_rx(&[
            HCI_PKT_EVENT,
            EVT_COMMAND_COMPLETE,
            0x04,
            0x01,
            0x03, 0x0C, // OPCODE_RESET
            0x00,
        ]);

        let activity = sink.poll(&mut transport, 150, |_, _| {});
        assert!(activity);
        assert_eq!(sink.rx_packets, 1);
        assert_eq!(sink.init_step, InitStep::SetEventMask);
        assert_eq!(sink.last_init_step_sent, Some(InitStep::SetEventMask));
        assert_eq!(transport.tx_packets.len(), 2);
    }

    #[test]
    fn test_connection_and_ssp_just_works() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.init_step = InitStep::Completed;

        let peer_addr = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

        // 1. Connection Request from peer
        let conn_req = [
            HCI_PKT_EVENT,
            EVT_CONNECTION_REQUEST,
            10,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, // bd_addr
            0x04, 0x04, 0x20, // cod
            0x01, // link_type: ACL
        ];
        sink.process_packet(&mut transport, &conn_req, |_, _| {});

        // Expect Accept_Connection_Request sent as slave
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_ACCEPT_CONNECTION_REQUEST
        );
        assert_eq!(&last_cmd[4..10], &peer_addr);
        assert_eq!(last_cmd[10], 0x01); // Role: Slave

        // 2. Connection Complete Event (handle = 0x0040)
        let conn_complete = [
            HCI_PKT_EVENT,
            EVT_CONNECTION_COMPLETE,
            11,
            0x00, // Status: Success
            0x40, 0x00, // Handle = 0x0040
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x01, // ACL
            0x00, // Encryption disabled
        ];
        sink.process_packet(&mut transport, &conn_complete, |_, _| {});
        assert_eq!(sink.active_handle, Some(0x0040));
        assert_eq!(sink.peer_bd_addr, peer_addr);

        // 3. Link Key Request (not yet paired) -> Negative Reply
        let link_key_req = [
            HCI_PKT_EVENT, EVT_LINK_KEY_REQUEST, 6, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        ];
        sink.process_packet(&mut transport, &link_key_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY
        );
        assert_eq!(&last_cmd[4..10], &peer_addr);

        // 4. IO Capability Request -> NoInputNoOutput (0x03)
        let io_cap_req = [
            HCI_PKT_EVENT, EVT_IO_CAPABILITY_REQUEST, 6, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        ];
        sink.process_packet(&mut transport, &io_cap_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_IO_CAPABILITY_REQUEST_REPLY
        );
        assert_eq!(last_cmd[10], 0x03); // NoInputNoOutput
        assert_eq!(last_cmd[12], 0x04); // General Bonding

        // 5. User Confirmation Request -> User Confirmation Request Reply
        let user_confirm_req = [
            HCI_PKT_EVENT,
            EVT_USER_CONFIRMATION_REQUEST,
            10,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x12, 0x34, 0x56, 0x78, // numeric value
        ];
        sink.process_packet(&mut transport, &user_confirm_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_USER_CONFIRMATION_REQUEST_REPLY
        );
        assert_eq!(&last_cmd[4..10], &peer_addr);

        // 6. Link Key Notification -> Save to bond table
        let dummy_key = [0xAA; 16];
        let mut key_notif = [0u8; 26];
        key_notif[0] = HCI_PKT_EVENT;
        key_notif[1] = EVT_LINK_KEY_NOTIFICATION;
        key_notif[2] = 23;
        key_notif[3..9].copy_from_slice(&peer_addr);
        key_notif[9..25].copy_from_slice(&dummy_key);
        key_notif[25] = 0x04; // Key type
        sink.process_packet(&mut transport, &key_notif, |_, _| {});
        assert_eq!(sink.bond_count, 1);
        assert_eq!(sink.find_bond_key(&peer_addr), Some(dummy_key));

        // 7. Subsequent Link Key Request -> Now responds with Link_Key_Request_Reply
        sink.process_packet(&mut transport, &link_key_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(u16::from_le_bytes([last_cmd[1], last_cmd[2]]), OPCODE_LINK_KEY_REQUEST_REPLY);
        assert_eq!(&last_cmd[10..26], &dummy_key);
    }

    #[test]
    fn test_ssp_bonded_only_negative_reply() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.security_policy.pairing_policy = BtPairingPolicy::BondedOnly;
        let peer_addr = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

        let user_confirm_req = [
            HCI_PKT_EVENT,
            EVT_USER_CONFIRMATION_REQUEST,
            10,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x12, 0x34, 0x56, 0x78,
        ];
        sink.process_packet(&mut transport, &user_confirm_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY
        );
        assert_eq!(&last_cmd[4..10], &peer_addr);
    }

    #[test]
    fn test_avrcp_fallback_not_implemented() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        let avrcp_cid = 0x0048;
        sink.active_handle = Some(0x0040);
        sink.channels[1] = L2capChannel {
            local_cid: avrcp_cid,
            remote_cid: 0x0044,
            psm: PSM_AVRCP,
            state: ChannelState::Configured,
            channel_type: ChannelType::Avrcp,
            handle: 0x0040,
            outbound_config_sent: true,
        };

        // Unknown vendor-dependent PDU (e.g. PDU ID 0xFE with Company ID 0x001958)
        let unknown_vendor_cmd = [
            0x00, // AVCTP: tx_label 0, cr 0 (command)
            (AVRCP_PID >> 8) as u8,
            AVRCP_PID as u8,
            AVRCP_CTYPE_CONTROL,
            0x48, // Subunit: Panel
            AVRCP_OPCODE_VENDOR_DEPENDENT,
            0x00, 0x19, 0x58, // BT_SIG_COMPANY_ID
            0xFE, // Unknown PDU
            0x00, // Packet type: Single
            0x00, 0x00, // Parameter length: 0
        ];
        let mut acl = Vec::new();
        acl.push(HCI_PKT_ACL);
        acl.extend_from_slice(&0x2040u16.to_le_bytes());
        let l2cap_len = unknown_vendor_cmd.len() as u16;
        acl.extend_from_slice(&(4 + l2cap_len).to_le_bytes());
        acl.extend_from_slice(&l2cap_len.to_le_bytes());
        acl.extend_from_slice(&avrcp_cid.to_le_bytes());
        acl.extend_from_slice(&unknown_vendor_cmd);

        sink.process_packet(&mut transport, &acl, |_, _| {});
        let last_tx = transport.tx_packets.last().expect("fallback rsp");
        assert_eq!(last_tx[9 + 3], AVRCP_RSP_NOT_IMPLEMENTED);
    }

    #[test]
    fn test_l2cap_echo_and_info_request() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);

        // Send L2CAP Echo Request (ping) on CID 0x0001
        let ping_payload = b"l2ping-test-payload";
        let mut echo_req = Vec::new();
        echo_req.push(HCI_PKT_ACL);
        echo_req.extend_from_slice(&0x2040u16.to_le_bytes()); // Handle 0x0040, PB=2
        let l2cap_info_len = (4 + ping_payload.len()) as u16;
        let acl_data_len = 4 + l2cap_info_len;
        echo_req.extend_from_slice(&acl_data_len.to_le_bytes()); // ACL data len
        echo_req.extend_from_slice(&l2cap_info_len.to_le_bytes()); // L2CAP len
        echo_req.extend_from_slice(&L2CAP_CID_SIGNALING.to_le_bytes()); // CID 0x0001
        // Signaling command: code=0x08 (Echo Req), ident=0x42, len
        echo_req.push(L2CAP_CMD_ECHO_REQ);
        echo_req.push(0x42); // Ident
        echo_req.extend_from_slice(&(ping_payload.len() as u16).to_le_bytes());
        echo_req.extend_from_slice(ping_payload);

        sink.process_packet(&mut transport, &echo_req, |_, _| {});

        // Expect Echo Response on CID 0x0001
        let last_tx = transport.tx_packets.last().expect("echo response sent");
        assert_eq!(last_tx[0], HCI_PKT_ACL);
        assert_eq!(u16::from_le_bytes([last_tx[7], last_tx[8]]), L2CAP_CID_SIGNALING);
        assert_eq!(last_tx[9], L2CAP_CMD_ECHO_RSP);
        assert_eq!(last_tx[10], 0x42); // Matching ident
        assert_eq!(&last_tx[13..13 + ping_payload.len()], ping_payload);

        // Test Information Request for Extended Features Mask (0x0002)
        let info_req = [
            HCI_PKT_ACL,
            0x40, 0x20, // Handle 0x0040, PB=2
            10, 0, // ACL len
            6, 0, // L2CAP len
            0x01, 0x00, // CID 0x0001
            L2CAP_CMD_INFO_REQ,
            0x43, // Ident
            0x02, 0x00, // Cmd len
            0x02, 0x00, // info_type: Extended Features Mask
        ];
        sink.process_packet(&mut transport, &info_req, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], L2CAP_CMD_INFO_RSP);
        assert_eq!(last_tx[10], 0x43);
        assert_eq!(&last_tx[17..21], &[0x80, 0x00, 0x00, 0x00]); // Fixed Channels supported
    }

    #[test]
    fn test_acl_multi_fragment_reassembly() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);

        // Ping payload of 60 bytes split across two ACL packets
        let ping_payload = [0x5A; 60];
        let total_l2cap_len = 4 + ping_payload.len(); // 64 bytes

        // Fragment 1: PB = 0b10 (First packet), contains L2CAP header + first 30 bytes
        let mut frag1 = Vec::new();
        frag1.push(HCI_PKT_ACL);
        frag1.extend_from_slice(&0x2040u16.to_le_bytes()); // Handle 0x0040, PB=2
        let frag1_data_len = (4 + 4 + 30) as u16; // 38 bytes
        frag1.extend_from_slice(&frag1_data_len.to_le_bytes());
        frag1.extend_from_slice(&(total_l2cap_len as u16).to_le_bytes()); // Total L2CAP len
        frag1.extend_from_slice(&L2CAP_CID_SIGNALING.to_le_bytes());
        frag1.push(L2CAP_CMD_ECHO_REQ);
        frag1.push(0x77); // Ident
        frag1.extend_from_slice(&(ping_payload.len() as u16).to_le_bytes());
        frag1.extend_from_slice(&ping_payload[..30]);

        // Fragment 2: PB = 0b01 (Continuing packet), contains remaining 30 bytes
        let mut frag2 = Vec::new();
        frag2.push(HCI_PKT_ACL);
        frag2.extend_from_slice(&0x1040u16.to_le_bytes()); // Handle 0x0040, PB=1 (continuing)
        frag2.extend_from_slice(&30u16.to_le_bytes());
        frag2.extend_from_slice(&ping_payload[30..]);

        // Process frag1: Should not dispatch yet
        sink.process_packet(&mut transport, &frag1, |_, _| {});
        assert!(sink.acl_reassembly.active);
        assert_eq!(transport.tx_packets.len(), 0);

        // Process frag2: Should reassemble and dispatch Echo Response!
        sink.process_packet(&mut transport, &frag2, |_, _| {});
        assert!(!sink.acl_reassembly.active);
        assert_eq!(transport.tx_packets.len(), 1);

        let tx = &transport.tx_packets[0];
        assert_eq!(tx[9], L2CAP_CMD_ECHO_RSP);
        assert_eq!(tx[10], 0x77);
        assert_eq!(&tx[13..13 + ping_payload.len()], &ping_payload);
    }

    #[test]
    fn test_sdp_service_search_matching() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);

        // Establish SDP channel (PSM 0x0001, peer SCID 0x0050)
        let sdp_conn_req = [
            HCI_PKT_ACL, 0x40, 0x20, 12, 0, 8, 0, 0x01, 0x00,
            L2CAP_CMD_CONN_REQ, 0x10, 4, 0, 0x01, 0x00, 0x50, 0x00,
        ];
        sink.process_packet(&mut transport, &sdp_conn_req, |_, _| {});
        let sdp_cid = sink.channels[0].local_cid;
        assert_eq!(sink.channels[0].channel_type, ChannelType::Sdp);

        // 1. Search for AudioSink (0x110B) via SDP
        // ServiceSearchAttributeRequest (PDU 0x06)
        let sdp_req_audio = [
            HCI_PKT_ACL, 0x40, 0x20, 21, 0, 17, 0, sdp_cid as u8, (sdp_cid >> 8) as u8,
            0x06, // PDU 0x06
            0x00, 0x01, // TID
            0x00, 0x0C, // Param len
            // ServiceSearchPattern: DES { UUID16(0x110B) }
            0x35, 0x03, 0x19, 0x11, 0x0B,
            0x03, 0xFF, // Max attr byte count
            0x35, 0x03, 0x09, 0x00, 0x00, // Attr list
        ];
        sink.process_packet(&mut transport, &sdp_req_audio, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(u16::from_le_bytes([last_tx[7], last_tx[8]]), 0x0050); // Peer SCID
        assert_eq!(last_tx[9], 0x07); // SDP ServiceSearchAttributeResponse
        // Contains the BT local name in the service name
        assert!(last_tx.windows(8).any(|w| w == BT_LOCAL_NAME));

        // 1b. Search for AudioSink via 32-bit UUID (0x1A)
        let sdp_req_audio32 = [
            HCI_PKT_ACL, 0x40, 0x20, 23, 0, 19, 0, sdp_cid as u8, (sdp_cid >> 8) as u8,
            0x06, 0x00, 0x03, 0x00, 0x0E,
            0x35, 0x05, 0x1A, 0x00, 0x00, 0x11, 0x0B, // 32-bit UUID
            0x03, 0xFF, 0x35, 0x03, 0x09, 0x00, 0x00,
        ];
        sink.process_packet(&mut transport, &sdp_req_audio32, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], 0x07);
        assert!(last_tx.windows(8).any(|w| w == b"BASEMENT"));

        // 1c. Search for AudioSink via 128-bit Bluetooth Base UUID (0x1C)
        let sdp_req_audio128 = [
            HCI_PKT_ACL, 0x40, 0x20, 35, 0, 31, 0, sdp_cid as u8, (sdp_cid >> 8) as u8,
            0x06, 0x00, 0x04, 0x00, 0x1A,
            0x35, 0x11, 0x1C,
            0x00, 0x00, 0x11, 0x0B, // 16-bit short in 128-bit base
            0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B, 0x34, 0xFB,
            0x03, 0xFF, 0x35, 0x03, 0x09, 0x00, 0x00,
        ];
        sink.process_packet(&mut transport, &sdp_req_audio128, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], 0x07);
        assert!(last_tx.windows(8).any(|w| w == b"BASEMENT"));

        // 2. Search for HFP (0x111E) -> Must reject with empty match [0x35, 0x00]
        let sdp_req_hfp = [
            HCI_PKT_ACL, 0x40, 0x20, 21, 0, 17, 0, sdp_cid as u8, (sdp_cid >> 8) as u8,
            0x06, // PDU 0x06
            0x00, 0x02, // TID
            0x00, 0x0C, // Param len
            0x35, 0x03, 0x19, 0x11, 0x1E, // UUID16 0x111E (HFP)
            0x03, 0xFF,
            0x35, 0x03, 0x09, 0x00, 0x00,
        ];
        sink.process_packet(&mut transport, &sdp_req_hfp, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], 0x07);
        // Param length is 5 (byte count 2 + DES len 0 [0x35, 0x00] + cont 0x00)
        assert_eq!(&last_tx[14..18], &[0x00, 0x02, 0x35, 0x00]);
    }

    #[test]
    fn test_avdtp_lifecycle_and_state_machine() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);

        // Open AVDTP Signaling channel (PSM 0x0019, peer SCID 0x0060)
        let avdtp_sig_conn = [
            HCI_PKT_ACL, 0x40, 0x20, 12, 0, 8, 0, 0x01, 0x00,
            L2CAP_CMD_CONN_REQ, 0x20, 4, 0, 0x19, 0x00, 0x60, 0x00,
        ];
        sink.process_packet(&mut transport, &avdtp_sig_conn, |_, _| {});
        let sig_cid = sink.channels[0].local_cid;
        assert_eq!(sink.channels[0].channel_type, ChannelType::AvdtpSignaling);

        // 1. DISCOVER
        let discover_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 6, 0, 2, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x00, // TxLabel=0, MsgType=Cmd (0)
            AVDTP_DISCOVER,
        ];
        sink.process_packet(&mut transport, &discover_cmd, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(u16::from_le_bytes([last_tx[7], last_tx[8]]), 0x0060); // Peer SCID
        assert_eq!(last_tx[9], 0x02); // Accept
        assert_eq!(last_tx[10], AVDTP_DISCOVER);
        assert_eq!(last_tx[11], 0x04); // SEID=1, in_use=0
        assert_eq!(last_tx[12], 0x08); // Audio Sink

        // 2. GET_ALL_CAPABILITIES
        let get_caps_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x10, AVDTP_GET_ALL_CAPABILITIES, 0x04,
        ];
        sink.process_packet(&mut transport, &get_caps_cmd, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], 0x12);
        assert_eq!(last_tx[10], AVDTP_GET_ALL_CAPABILITIES);

        // 3. SET_CONFIGURATION (Configuring 48kHz SBC)
        let set_cfg_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 18, 0, 14, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x20, AVDTP_SET_CONFIGURATION, 0x04, 0x04,
            0x01, 0x00, // Media Transport
            0x07, 0x06, 0x00, 0x00, 0x11, 0xFF, 2, 53, // 48kHz (0x10) + Joint Stereo (0x01)
        ];
        sink.process_packet(&mut transport, &set_cfg_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Configured);
        assert_eq!(sink.sample_rate_hz, 48000);

        // 4. OPEN
        let open_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x30, AVDTP_OPEN, 0x04,
        ];
        sink.process_packet(&mut transport, &open_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Open);
        assert!(sink.in_use);

        // 4b. DISCOVER while Open -> InUse must be 1 (0x02)
        sink.process_packet(&mut transport, &discover_cmd, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[11], 0x06); // SEID=1, in_use=1 (0x04 | 0x02)

        // 5. START
        let start_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x40, AVDTP_START, 0x04,
        ];
        sink.process_packet(&mut transport, &start_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Streaming);
        assert!(sink.in_use);

        // 6. SUSPEND
        let suspend_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x50, AVDTP_SUSPEND, 0x04,
        ];
        sink.process_packet(&mut transport, &suspend_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Open);
        assert!(sink.in_use);

        // 7. CLOSE
        let close_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x60, AVDTP_CLOSE, 0x04,
        ];
        sink.process_packet(&mut transport, &close_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Idle);
        assert!(!sink.in_use);

        // 8. Unknown AVDTP command -> General Reject (msg_type 0x01)
        let unknown_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 6, 0, 2, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x70, // TxLabel=7, MsgType=Cmd (0)
            0x3E, // Unknown sig_id
        ];
        sink.process_packet(&mut transport, &unknown_cmd, |_, _| {});
        let last_tx = transport.tx_packets.last().unwrap();
        assert_eq!(last_tx[9], 0x71); // TxLabel=7, MsgType=General Reject (0x01)
        assert_eq!(last_tx[10], 0x3E);
    }

    #[test]
    fn test_sbc_decoder_polyphase_synthesis() {
        let mut decoder = SbcDecoder::new();
        let mut pcm_buf = [0i16; 512];

        // 1. Decode 8-subband frame (16 blocks, 8 subbands, Joint Stereo, Bitpool 53)
        let res8 = decoder.decode_frame(&SAMPLE_SBC_FRAME_8SB, &mut pcm_buf);
        assert!(res8.is_ok());
        let (consumed8, frames8) = res8.unwrap();
        assert_eq!(consumed8, SAMPLE_SBC_FRAME_8SB.len());
        assert_eq!(frames8, 128); // 128 stereo frames = 256 samples of 16-bit PCM = 512 bytes

        // Check waveform samples non-zero and within valid dynamic range
        let mut non_zero_count = 0;
        for &s in &pcm_buf[..256] {
            if s != 0 {
                non_zero_count += 1;
            }
        }
        assert!(non_zero_count > 0, "Synthesized PCM samples must be non-zero");

        // 2. Decode 4-subband frame (16 blocks, 4 subbands, Stereo, SNR, Bitpool 30)
        let mut pcm_buf4 = [0i16; 512];
        let res4 = decoder.decode_frame(&SAMPLE_SBC_FRAME_4SB, &mut pcm_buf4);
        assert!(res4.is_ok());
        let (consumed4, frames4) = res4.unwrap();
        assert_eq!(consumed4, SAMPLE_SBC_FRAME_4SB.len());
        assert_eq!(frames4, 64); // 64 stereo frames = 128 samples of 16-bit PCM = 256 bytes

        let mut non_zero_count4 = 0;
        for &s in &pcm_buf4[..128] {
            if s != 0 {
                non_zero_count4 += 1;
            }
        }
        assert!(non_zero_count4 > 0, "4-subband synthesized PCM samples must be non-zero");
    }

    #[test]
    fn test_rtp_a2dp_media_streaming_end_to_end() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);
        sink.sample_rate_hz = 44100;
        sink.avdtp_state = AvdtpStreamState::Streaming;

        // Open first AVDTP channel (Signaling)
        let sig_conn = [
            HCI_PKT_ACL, 0x40, 0x20, 12, 0, 8, 0, 0x01, 0x00,
            L2CAP_CMD_CONN_REQ, 0x01, 4, 0, 0x19, 0x00, 0x60, 0x00,
        ];
        sink.process_packet(&mut transport, &sig_conn, |_, _| {});

        // Open second AVDTP channel (Media)
        let media_conn = [
            HCI_PKT_ACL, 0x40, 0x20, 12, 0, 8, 0, 0x01, 0x00,
            L2CAP_CMD_CONN_REQ, 0x02, 4, 0, 0x19, 0x00, 0x61, 0x00,
        ];
        sink.process_packet(&mut transport, &media_conn, |_, _| {});
        let media_cid = sink.channels[1].local_cid;
        assert_eq!(sink.channels[1].channel_type, ChannelType::AvdtpMedia);

        // Construct RTP Media Packet wrapping SAMPLE_SBC_FRAME_8SB
        let mut media_pkt = Vec::new();
        media_pkt.push(HCI_PKT_ACL);
        media_pkt.extend_from_slice(&0x2040u16.to_le_bytes()); // Handle 0x0040, PB=2
        let rtp_payload_len = 12 + 1 + SAMPLE_SBC_FRAME_8SB.len();
        let l2cap_total_len = (4 + rtp_payload_len) as u16;
        media_pkt.extend_from_slice(&l2cap_total_len.to_le_bytes()); // ACL len
        media_pkt.extend_from_slice(&(rtp_payload_len as u16).to_le_bytes()); // L2CAP len
        media_pkt.extend_from_slice(&media_cid.to_le_bytes());

        // RTP Header (12 bytes)
        media_pkt.extend_from_slice(&[
            0x80, // V=2, P=0, X=0, CC=0
            0x60, // PT=96
            0x00, 0x01, // Seq 1
            0x00, 0x00, 0x00, 0x80, // Timestamp 128
            0x12, 0x34, 0x56, 0x78, // SSRC
        ]);

        // A2DP SBC Header (1 byte): 1 frame
        media_pkt.push(0x01);
        media_pkt.extend_from_slice(&SAMPLE_SBC_FRAME_8SB);

        let mut pcm_received_bytes = 0;
        let mut pcm_sample_rate = 0;

        sink.process_packet(&mut transport, &media_pkt, |pcm, rate| {
            pcm_received_bytes += pcm.len();
            pcm_sample_rate = rate;
        });

        // 128 stereo PCM samples * 2 channels * 2 bytes = 512 bytes
        assert_eq!(pcm_received_bytes, 512);
        assert_eq!(pcm_sample_rate, 44100);

        // Verify non-streaming state drops media frames
        sink.avdtp_state = AvdtpStreamState::Open;
        let mut dropped_pcm = false;
        sink.process_packet(&mut transport, &media_pkt, |_, _| {
            dropped_pcm = true;
        });
        assert!(!dropped_pcm, "Media packet must be dropped when not in Streaming state");

        // Verify RTP header extension (X bit set) is properly parsed and skipped
        sink.avdtp_state = AvdtpStreamState::Streaming;
        let mut ext_media_pkt = Vec::new();
        ext_media_pkt.push(HCI_PKT_ACL);
        ext_media_pkt.extend_from_slice(&0x2040u16.to_le_bytes());
        // Extension: 4-byte header + 1 word (4 bytes) = 8 bytes extension
        let ext_rtp_len = 12 + 8 + 1 + SAMPLE_SBC_FRAME_8SB.len();
        ext_media_pkt.extend_from_slice(&((4 + ext_rtp_len) as u16).to_le_bytes());
        ext_media_pkt.extend_from_slice(&(ext_rtp_len as u16).to_le_bytes());
        ext_media_pkt.extend_from_slice(&media_cid.to_le_bytes());

        // RTP Header with X=1 (0x90): V=2, X=1, CC=0
        ext_media_pkt.extend_from_slice(&[
            0x90, 0x60, 0x00, 0x02, 0x00, 0x00, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
        ]);
        // Extension header: defined by profile = 0xBEDE, length = 1 word (4 bytes)
        ext_media_pkt.extend_from_slice(&[0xBE, 0xDE, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD]);
        // SBC payload header + frame
        ext_media_pkt.push(0x01);
        ext_media_pkt.extend_from_slice(&SAMPLE_SBC_FRAME_8SB);

        let mut ext_pcm_bytes = 0;
        sink.process_packet(&mut transport, &ext_media_pkt, |pcm, _| {
            ext_pcm_bytes += pcm.len();
        });
        assert_eq!(ext_pcm_bytes, 512, "RTP extension header must be skipped to decode PCM");
    }

    #[test]
    fn test_bounds_and_oversize_protection() {
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();

        // send_cmd > 255 bytes returns false
        let large_params = [0u8; 256];
        assert!(!BtA2dpSink::send_cmd(&mut transport, OPCODE_RESET, &large_params));

        // send_l2cap > 1015 bytes returns false
        let large_l2cap = [0u8; 1016];
        assert!(!BtA2dpSink::send_l2cap(&mut transport, 0x0040, 0x0040, &large_l2cap));

        // ACL packet with l2cap_len > 1020 triggers reassembly abort
        let mut oversized_acl = Vec::new();
        oversized_acl.push(HCI_PKT_ACL);
        oversized_acl.extend_from_slice(&0x2040u16.to_le_bytes()); // PB=2
        oversized_acl.extend_from_slice(&34u16.to_le_bytes()); // ACL len 34
        oversized_acl.extend_from_slice(&1021u16.to_le_bytes()); // L2CAP len 1021 (4+1021 > 1024)
        oversized_acl.extend_from_slice(&0x0040u16.to_le_bytes());
        oversized_acl.extend_from_slice(&[0u8; 30]);

        sink.process_packet(&mut transport, &oversized_acl, |_, _| {});
        assert!(!sink.acl_reassembly.active, "Oversized L2CAP PDU must abort reassembly");
    }

    #[test]
    fn test_avrcp_passthrough_and_absolute_volume_and_notifications() {
        assert_eq!(avrcp_raw_to_volume_pct(0x00), 0);
        assert_eq!(avrcp_raw_to_volume_pct(0x7F), 100);
        assert_eq!(volume_pct_to_avrcp_raw(0), 0x00);
        assert_eq!(volume_pct_to_avrcp_raw(100), 0x7F);
        for pct in [0u8, 25, 50, 75, 100] {
            let raw = volume_pct_to_avrcp_raw(pct);
            assert_eq!(avrcp_raw_to_volume_pct(raw), pct);
        }

        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);

        // Open AVRCP L2CAP channel (PSM 0x0017, peer SCID 0x0070)
        let avrcp_conn = [
            HCI_PKT_ACL,
            0x40,
            0x20,
            12,
            0,
            8,
            0,
            0x01,
            0x00,
            L2CAP_CMD_CONN_REQ,
            0x30,
            4,
            0,
            0x17,
            0x00,
            0x70,
            0x00,
        ];
        sink.process_packet(&mut transport, &avrcp_conn, |_, _| {});
        let avrcp_cid = sink.channels[0].local_cid;
        assert_eq!(sink.channels[0].channel_type, ChannelType::Avrcp);

        // Helper to wrap an AVCTP/AVRCP payload in an L2CAP ACL packet
        let wrap_avrcp = |cid: u16, pdu: &[u8]| -> Vec<u8> {
            let mut pkt = Vec::new();
            pkt.push(HCI_PKT_ACL);
            pkt.extend_from_slice(&0x2040u16.to_le_bytes());
            let l2cap_len = pdu.len() as u16;
            pkt.extend_from_slice(&(4 + l2cap_len).to_le_bytes());
            pkt.extend_from_slice(&l2cap_len.to_le_bytes());
            pkt.extend_from_slice(&cid.to_le_bytes());
            pkt.extend_from_slice(pdu);
            pkt
        };

        // 1. Test PassThrough commands for Play, Pause, Stop, Next, Prev
        let ops = [
            AvrcpOperationId::Play,
            AvrcpOperationId::Pause,
            AvrcpOperationId::Stop,
            AvrcpOperationId::NEXT,
            AvrcpOperationId::PREV,
        ];
        for (idx, &op) in ops.iter().enumerate() {
            let mut cmd = [0u8; 8];
            let cmd_len = build_avrcp_passthrough_cmd(
                idx as u8,
                op,
                AvrcpButtonState::Pressed,
                &mut cmd,
            );
            let acl = wrap_avrcp(avrcp_cid, &cmd[..cmd_len]);
            sink.process_packet(&mut transport, &acl, |_, _| {});
            assert_eq!(sink.last_passthrough_op, Some(op));
            assert_eq!(sink.last_passthrough_state, Some(AvrcpButtonState::Pressed));

            let last_tx = transport.tx_packets.last().expect("passthrough rsp");
            let parsed = parse_avrcp_passthrough(&last_tx[9..]).expect("valid passthrough rsp");
            assert_eq!(parsed.0, (idx as u8) & 0x0F);
            assert_eq!(parsed.1, AvrcpMessageKind::Response);
            assert_eq!(last_tx[9 + 3], AVRCP_RSP_ACCEPTED);
            assert_eq!(parsed.2, op);
            assert_eq!(parsed.3, AvrcpButtonState::Pressed);
        }

        // 2. Test SetAbsoluteVolume (0x50) command from peer
        let mut vol_cmd = [0u8; 14];
        let vol_len = build_avrcp_set_absolute_volume_cmd(
            5,
            volume_pct_to_avrcp_raw(80),
            &mut vol_cmd,
        );
        let acl_vol = wrap_avrcp(avrcp_cid, &vol_cmd[..vol_len]);
        sink.process_packet(&mut transport, &acl_vol, |_, _| {});
        assert_eq!(sink.avrcp_volume_pct, 80);
        let last_tx = transport.tx_packets.last().expect("set_abs_vol rsp");
        assert_eq!(last_tx[9 + 3], AVRCP_RSP_ACCEPTED);
        assert_eq!(last_tx[9 + 9], AVRCP_PDU_SET_ABSOLUTE_VOLUME);
        assert_eq!(avrcp_raw_to_volume_pct(last_tx[9 + 13]), 80);

        // 3. Test RegisterNotification (0x31) EVENT_VOLUME_CHANGED (0x0D)
        let mut reg_cmd = [0u8; 18];
        let reg_len = build_avrcp_register_volume_notify_cmd(7, &mut reg_cmd);
        let acl_reg = wrap_avrcp(avrcp_cid, &reg_cmd[..reg_len]);
        sink.process_packet(&mut transport, &acl_reg, |_, _| {});
        assert_eq!(sink.avrcp_vol_notify_tx_label, Some(7));
        let interim_tx = transport.tx_packets.last().expect("interim rsp");
        assert_eq!(interim_tx[9 + 3], AVRCP_RSP_INTERIM);
        assert_eq!(interim_tx[9 + 13], AVRCP_EVENT_VOLUME_CHANGED);
        assert_eq!(avrcp_raw_to_volume_pct(interim_tx[9 + 14]), 80);

        // Trigger local volume change -> emits CHANGED (0x0D) response
        assert!(sink.notify_avrcp_volume_changed(&mut transport, 40));
        assert_eq!(sink.avrcp_vol_notify_tx_label, None);
        let changed_tx = transport.tx_packets.last().expect("changed rsp");
        assert_eq!(changed_tx[9 + 3], AVRCP_RSP_CHANGED);
        assert_eq!(changed_tx[9 + 13], AVRCP_EVENT_VOLUME_CHANGED);
        assert_eq!(avrcp_raw_to_volume_pct(changed_tx[9 + 14]), 40);
    }

    #[test]
    fn test_avrcp_get_element_attributes_track_metadata() {
        let meta = AvrcpTrackMetadata::with_metadata(
            "Strobe",
            "deadmau5",
            "For Lack of a Better Name",
            637_000,
        );

        let mut rsp_buf = [0u8; 320];
        let rsp_len = build_avrcp_get_element_attributes_rsp(3, &meta, &mut rsp_buf);
        assert!(rsp_len > 20);

        let parsed_meta = parse_avrcp_element_attributes_rsp(&rsp_buf[..rsp_len])
            .expect("parse element attributes rsp");
        assert_eq!(parsed_meta.title(), "Strobe");
        assert_eq!(parsed_meta.artist(), "deadmau5");
        assert_eq!(parsed_meta.album(), "For Lack of a Better Name");
        assert_eq!(parsed_meta.duration_ms(), 637_000);

        // Test end-to-end over BtA2dpSink AVRCP channel (Target & Controller roles)
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);
        sink.track_metadata = meta;

        let avrcp_conn = [
            HCI_PKT_ACL,
            0x40,
            0x20,
            12,
            0,
            8,
            0,
            0x01,
            0x00,
            L2CAP_CMD_CONN_REQ,
            0x31,
            4,
            0,
            0x17,
            0x00,
            0x71,
            0x00,
        ];
        sink.process_packet(&mut transport, &avrcp_conn, |_, _| {});
        let avrcp_cid = sink.channels[0].local_cid;

        let mut req_pdu = [0u8; 38];
        let req_len = build_avrcp_get_element_attributes_cmd(9, &mut req_pdu);
        let mut acl_req = Vec::new();
        acl_req.push(HCI_PKT_ACL);
        acl_req.extend_from_slice(&0x2040u16.to_le_bytes());
        acl_req.extend_from_slice(&((4 + req_len) as u16).to_le_bytes());
        acl_req.extend_from_slice(&(req_len as u16).to_le_bytes());
        acl_req.extend_from_slice(&avrcp_cid.to_le_bytes());
        acl_req.extend_from_slice(&req_pdu[..req_len]);

        sink.process_packet(&mut transport, &acl_req, |_, _| {});
        let target_rsp = transport.tx_packets.last().expect("element attributes rsp").clone();

        // Feed target response into a second sink acting as Controller
        let mut controller_sink = BtA2dpSink::new();
        controller_sink.active_handle = Some(0x0040);
        controller_sink.process_packet(&mut transport, &avrcp_conn, |_, _| {});
        let ctrl_cid = controller_sink.channels[0].local_cid;
        let mut rx_rsp = target_rsp;
        rx_rsp[7..9].copy_from_slice(&ctrl_cid.to_le_bytes());
        controller_sink.process_packet(&mut transport, &rx_rsp, |_, _| {});
        assert_eq!(controller_sink.track_metadata.title(), "Strobe");
        assert_eq!(controller_sink.track_metadata.artist(), "deadmau5");
        assert_eq!(
            controller_sink.track_metadata.album(),
            "For Lack of a Better Name"
        );
        assert_eq!(controller_sink.track_metadata.duration_ms(), 637_000);
    }

    #[test]
    fn test_bonding_store_lru_and_flash_journal_persistence_and_security_policy() {
        let mut store = BondingStore::new();
        let addr1 = [0x01, 0x00, 0x00, 0x00, 0x00, 0x01];
        let addr2 = [0x02, 0x00, 0x00, 0x00, 0x00, 0x02];
        let addr3 = [0x03, 0x00, 0x00, 0x00, 0x00, 0x03];
        let addr4 = [0x04, 0x00, 0x00, 0x00, 0x00, 0x04];
        let addr5 = [0x05, 0x00, 0x00, 0x00, 0x00, 0x05];

        store.upsert(addr1, [0x11; 16], 0x04);
        store.upsert(addr2, [0x22; 16], 0x04);
        store.upsert(addr3, [0x33; 16], 0x04);
        store.upsert(addr4, [0x44; 16], 0x04);
        assert_eq!(store.count(), 4);

        // Touch addr1 so addr2 becomes the least-recently-used entry
        assert_eq!(store.lookup(&addr1).map(|p| p.link_key), Some([0x11; 16]));

        // Insert addr5 -> must evict addr2
        store.upsert(addr5, [0x55; 16], 0x05);
        assert_eq!(store.count(), 4);
        assert_eq!(store.peek(&addr2), None);
        assert_eq!(store.peek(&addr1).map(|p| p.link_key), Some([0x11; 16]));
        assert_eq!(store.peek(&addr3).map(|p| p.link_key), Some([0x33; 16]));
        assert_eq!(store.peek(&addr4).map(|p| p.link_key), Some([0x44; 16]));
        assert_eq!(store.peek(&addr5).map(|p| p.link_key), Some([0x55; 16]));

        // Hex round-trip
        let mut hex_buf = [0u8; 200];
        let hex_len = store.encode_hex(&mut hex_buf);
        let hex_str = core::str::from_utf8(&hex_buf[..hex_len]).expect("utf8 hex");
        let from_hex = BondingStore::decode_hex(hex_str).expect("hex decode");
        assert_eq!(from_hex.count(), 4);
        assert_eq!(from_hex.peek(&addr1).map(|p| p.link_key), Some([0x11; 16]));
        assert_eq!(from_hex.peek(&addr5).map(|p| p.link_key), Some([0x55; 16]));

        // FlashJournal MemConfigStore round-trip under key "bt.bonds"
        let mut kv: MemConfigStore<8> = MemConfigStore::new();
        store.save_to_storage(&mut kv).expect("save to MemConfigStore");
        let loaded_kv = BondingStore::load_from_storage(&kv).expect("load from MemConfigStore");
        assert_eq!(loaded_kv.count(), 4);
        assert_eq!(loaded_kv.peek(&addr1).map(|p| p.link_key), Some([0x11; 16]));
        assert_eq!(loaded_kv.peek(&addr5).map(|p| p.link_key), Some([0x55; 16]));

        // FlashJournal raw 4 KB sector round-trip
        let mut sector = [0xFFu8; 4096];
        let mut kv_out: MemConfigStore<8> = MemConfigStore::new();
        store
            .save_to_journal_sector(&mut kv_out, &mut sector, 1)
            .expect("save to journal sector");
        let mut kv_in: MemConfigStore<8> = MemConfigStore::new();
        let loaded_sector = BondingStore::load_from_journal_sector(&sector, &mut kv_in)
            .expect("load from journal sector");
        assert_eq!(loaded_sector.count(), 4);
        assert_eq!(
            loaded_sector.peek(&addr1).map(|p| p.link_key),
            Some([0x11; 16])
        );
        assert_eq!(
            loaded_sector.peek(&addr5).map(|p| p.link_key),
            Some([0x55; 16])
        );

        // Test BtSecurityPolicy BondedOnly rejection vs bonded acceptance
        let mut transport = MockHciTransport::new();
        let mut sink = BtA2dpSink::new();
        sink.bonding_store = loaded_sector;
        sink.security_policy.update_device_name("LOUDER-BT");
        sink.security_policy.update_pin_code(b"1234");
        sink.security_policy.pairing_policy = BtPairingPolicy::BondedOnly;

        // Unknown peer PIN request -> Rejected
        let pin_req_unbonded = [
            HCI_PKT_EVENT,
            EVT_PIN_CODE_REQUEST,
            6,
            0x99,
            0x88,
            0x77,
            0x66,
            0x55,
            0x44,
        ];
        sink.process_packet(&mut transport, &pin_req_unbonded, |_, _| {});
        let last_cmd = transport.tx_packets.last().expect("pin neg reply");
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_PIN_CODE_REQUEST_NEGATIVE_REPLY
        );

        // Bonded peer (addr1) PIN request -> Accepted with configured PIN "1234"
        let pin_req_bonded = [
            HCI_PKT_EVENT,
            EVT_PIN_CODE_REQUEST,
            6,
            addr1[0],
            addr1[1],
            addr1[2],
            addr1[3],
            addr1[4],
            addr1[5],
        ];
        sink.process_packet(&mut transport, &pin_req_bonded, |_, _| {});
        let last_cmd = transport.tx_packets.last().expect("pin reply");
        assert_eq!(
            u16::from_le_bytes([last_cmd[1], last_cmd[2]]),
            OPCODE_PIN_CODE_REQUEST_REPLY
        );
        assert_eq!(&last_cmd[11..15], b"1234");
    }

    #[test]
    fn test_sbc_encoder_decoder_roundtrip_stereo_and_joint_stereo() {
        for mode in [SbcChannelMode::Stereo, SbcChannelMode::JointStereo] {
            let mut encoder = SbcEncoder::new();
            encoder.channel_mode = mode;
            encoder.bitpool = 53;
            let mut decoder = SbcDecoder::new();

            let num_frames = 8;
            let total_samples = num_frames * 128;
            let mut pcm_in = vec![0i16; total_samples * 2];
            for i in 0..total_samples {
                let t = i as f64 / 44100.0;
                let l = (16000.0 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as i16;
                let r = (16000.0 * (2.0 * std::f64::consts::PI * 2000.0 * t).cos()) as i16;
                pcm_in[i * 2] = l;
                pcm_in[i * 2 + 1] = r;
            }

            let mut pcm_out = Vec::with_capacity(total_samples * 2);
            let mut sbc_buf = [0u8; 256];
            let mut dec_buf = [0i16; 512];

            for f in 0..num_frames {
                let slice = &pcm_in[f * 256..(f + 1) * 256];
                let encoded_len = encoder
                    .encode_frame(slice, &mut sbc_buf)
                    .expect("encode_frame succeeds");
                assert_eq!(sbc_buf[0], SBC_SYNCWORD);
                let expected_len = if mode == SbcChannelMode::JointStereo {
                    119
                } else {
                    118
                };
                assert_eq!(encoded_len, expected_len);

                let (consumed, stereo_frames) = decoder
                    .decode_frame(&sbc_buf[..encoded_len], &mut dec_buf)
                    .expect("decode_frame succeeds");
                assert_eq!(consumed, encoded_len);
                assert_eq!(stereo_frames, 128);
                pcm_out.extend_from_slice(&dec_buf[..256]);
            }

            // Combined analysis + synthesis polyphase filterbank group delay is
            // 72 stereo samples (9 blocks * 8 subbands). Verify SNR after warm-up.
            let lag = 72;
            let start = 256;
            let end = total_samples - lag;
            let mut sig_power = 0.0f64;
            let mut err_power = 0.0f64;
            for i in start..end {
                let ref_l = pcm_in[(i - lag) * 2] as f64;
                let ref_r = pcm_in[(i - lag) * 2 + 1] as f64;
                let out_l = pcm_out[i * 2] as f64;
                let out_r = pcm_out[i * 2 + 1] as f64;
                sig_power += ref_l * ref_l + ref_r * ref_r;
                err_power += (ref_l - out_l).powi(2) + (ref_r - out_r).powi(2);
            }
            let snr_db = 10.0 * (sig_power / err_power.max(1.0)).log10();
            assert!(
                snr_db > 45.0,
                "Expected SBC round-trip SNR > 45 dB for {:?}, got {:.2} dB",
                mode,
                snr_db
            );
        }
    }

    #[test]
    fn test_a2dp_source_eir_discovery_and_avdtp_initiator_and_rtp_media() {
        let mut transport = MockHciTransport::new();
        let mut source = BtA2dpSourceState::new();
        source.update_target_name("JBL Flip 6");

        // 1. Start Inquiry
        assert!(source.start_inquiry(&mut transport, 5));
        assert_eq!(source.phase, BtA2dpSourcePhase::Inquiring);
        let inq_cmd = transport.tx_packets.last().expect("inquiry cmd");
        assert_eq!(u16::from_le_bytes([inq_cmd[1], inq_cmd[2]]), OPCODE_INQUIRY);

        // 2. Simulate Extended Inquiry Result (0x2F) for "JBL Flip 6"
        let peer_mac = [0xAA, 0xBB, 0xCC, 0x11, 0x22, 0x33];
        let mut eir_params = [0u8; 40];
        eir_params[0] = 1; // Num_Responses
        eir_params[1..7].copy_from_slice(&peer_mac);
        eir_params[7] = 0x01; // Page_Scan_Repetition_Mode = R1
        eir_params[12] = 0x34;
        eir_params[13] = 0x12; // Clock_Offset = 0x1234
        eir_params[14] = (-55i8) as u8; // RSSI
        // EIR data starts at eir_params[15..]
        // AD1: Complete Local Name "JBL Flip 6" (10 bytes)
        eir_params[15] = 11;
        eir_params[16] = 0x09;
        eir_params[17..27].copy_from_slice(b"JBL Flip 6");
        // AD2: 16-bit Service Class UUIDs: 0x110B (Audio Sink)
        eir_params[27] = 3;
        eir_params[28] = 0x03;
        eir_params[29] = 0x0B;
        eir_params[30] = 0x11;

        assert!(eir_has_audio_sink_uuid(&eir_params[15..]));
        assert!(source.on_extended_inquiry_result(&mut transport, &eir_params));
        assert_eq!(source.phase, BtA2dpSourcePhase::ConnectingAcl);
        assert_eq!(source.matched_bd_addr, Some(peer_mac));
        let create_conn = transport.tx_packets.last().expect("create conn");
        assert_eq!(
            u16::from_le_bytes([create_conn[1], create_conn[2]]),
            OPCODE_CREATE_CONNECTION
        );

        // 3. Transition to ACL connected -> opens L2CAP AVDTP Signaling (PSM 0x0019)
        assert!(source.on_acl_connected(&mut transport, 0x0040, 1));
        assert_eq!(source.phase, BtA2dpSourcePhase::ConnectingAvdtpSignaling);

        // 4. Start AVDTP discovery on remote signaling CID 0x0080
        assert!(source.start_avdtp_discovery(&mut transport, 0x0080));
        assert_eq!(source.phase, BtA2dpSourcePhase::DiscoveringSeps);

        // 5. Step through AVDTP Discover -> GetCapabilities -> SetConfiguration -> Open
        let disc_rsp = [0x12, AVDTP_DISCOVER, 0x08, 0x08]; // SEID 2, Audio Sink
        assert!(source.handle_avdtp_response(&mut transport, &disc_rsp));
        assert_eq!(source.phase, BtA2dpSourcePhase::GettingCapabilities);
        assert_eq!(source.remote_sink_seid, 2);

        let caps_rsp = [
            0x22,
            AVDTP_GET_CAPABILITIES,
            0x01,
            0x00,
            0x07,
            0x06,
            0x00,
            0x00,
            0x21,
            0x15,
            2,
            53,
        ];
        assert!(source.handle_avdtp_response(&mut transport, &caps_rsp));
        assert_eq!(source.phase, BtA2dpSourcePhase::SettingConfiguration);

        let set_cfg_rsp = [0x32, AVDTP_SET_CONFIGURATION];
        assert!(source.handle_avdtp_response(&mut transport, &set_cfg_rsp));
        assert_eq!(source.phase, BtA2dpSourcePhase::OpeningStream);

        let open_rsp = [0x42, AVDTP_OPEN];
        assert!(source.handle_avdtp_response(&mut transport, &open_rsp));
        assert_eq!(source.phase, BtA2dpSourcePhase::ConnectingAvdtpMedia);

        // 6. Media L2CAP channel configured (remote CID 0x0081) -> sends AVDTP Start
        assert!(source.on_media_channel_configured(&mut transport, 0x0081));
        assert_eq!(source.phase, BtA2dpSourcePhase::StartingStream);

        // 7. AVDTP Start Response -> Streaming!
        let start_rsp = [0x52, AVDTP_START];
        assert!(source.handle_avdtp_response(&mut transport, &start_rsp));
        assert!(source.is_streaming());

        // 8. Encode 128 stereo PCM frames and wrap in an A2DP RTP media packet
        let mut pcm_frame = [0i16; 256];
        for i in 0..128 {
            pcm_frame[i * 2] = ((i as i16) * 200) - 12000;
            pcm_frame[i * 2 + 1] = 12000 - ((i as i16) * 200);
        }
        let mut encoder = SbcEncoder::new();
        let mut sbc_frame = [0u8; 128];
        let sbc_len = encoder
            .encode_frame(&pcm_frame, &mut sbc_frame)
            .expect("SBC encode");
        let mut media_payload = [0u8; 256];
        let media_len = source.build_rtp_media_packet(
            &sbc_frame[..sbc_len],
            1,
            128,
            &mut media_payload,
        );
        assert!(media_len > 13);

        // Feed the generated RTP/SBC payload into BtA2dpSink
        let mut sink = BtA2dpSink::new();
        sink.active_handle = Some(0x0040);
        sink.avdtp_state = AvdtpStreamState::Streaming;
        sink.channels[0] = L2capChannel {
            local_cid: 0x0081,
            remote_cid: 0x0049,
            psm: PSM_AVDTP,
            state: ChannelState::Configured,
            channel_type: ChannelType::AvdtpMedia,
            handle: 0x0040,
            outbound_config_sent: true,
        };
        let mut acl_media = Vec::new();
        acl_media.push(HCI_PKT_ACL);
        acl_media.extend_from_slice(&0x2040u16.to_le_bytes());
        acl_media.extend_from_slice(&((4 + media_len) as u16).to_le_bytes());
        acl_media.extend_from_slice(&(media_len as u16).to_le_bytes());
        acl_media.extend_from_slice(&0x0081u16.to_le_bytes());
        acl_media.extend_from_slice(&media_payload[..media_len]);

        let mut decoded_bytes = 0;
        sink.process_packet(&mut transport, &acl_media, |pcm, rate| {
            decoded_bytes += pcm.len();
            assert_eq!(rate, 44100);
        });
        assert_eq!(decoded_bytes, 512);
    }
}
