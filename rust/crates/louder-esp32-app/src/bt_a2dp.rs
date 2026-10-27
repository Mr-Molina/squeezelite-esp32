//! Pure-Rust Dual-Mode Bluetooth Classic A2DP Sink & SBC Audio Decoder.
//!
//! Standards-compliant implementation providing:
//! - Dual-Mode HCI initialization state machine (Classic BR/EDR + BLE advertising)
//! - Classic connection management & SSP "Just Works" auto-pairing
//! - HCI ACL multi-fragment reassembly (PB = 0b10/0b00 + 0b01)
//! - L2CAP Signaling engine (CID 0x0001, Echo, Info, Conn, Config, Disconn)
//! - SDP Server (PSM 0x0001) exposing A2DP Audio Sink 0x110B Service Record
//! - AVDTP 1.3 Signaling & Media streaming (PSM 0x0019) + AVRCP (PSM 0x0017)
//! - High-bitrate Bluetooth A2DP SBC audio decoder with polyphase synthesis filterbank
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

// ============================================================================
// HCI Transport Trait
// ============================================================================

/// Clean, non-blocking packet transport for Bluetooth HCI frames.
pub trait HciTransport {
    /// Read an incoming HCI packet into `buf`. Returns the number of bytes read (0 if no packet).
    fn read_packet(&mut self, buf: &mut [u8]) -> usize;
    /// Write an outgoing HCI packet to the controller. Returns true if written successfully.
    fn write_packet(&mut self, data: &[u8]) -> bool;
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

pub const OPCODE_ACCEPT_CONNECTION_REQUEST: u16 = 0x0409;
pub const OPCODE_PIN_CODE_REQUEST_REPLY: u16 = 0x040D;
pub const OPCODE_LINK_KEY_REQUEST_REPLY: u16 = 0x040B;
pub const OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY: u16 = 0x040C;
pub const OPCODE_IO_CAPABILITY_REQUEST_REPLY: u16 = 0x042B;
pub const OPCODE_USER_CONFIRMATION_REQUEST_REPLY: u16 = 0x042C;

// HCI Event Codes
pub const EVT_CONNECTION_COMPLETE: u8 = 0x03;
pub const EVT_CONNECTION_REQUEST: u8 = 0x04;
pub const EVT_DISCONNECTION_COMPLETE: u8 = 0x05;
pub const EVT_NUM_COMPLETED_PACKETS: u8 = 0x13;
pub const EVT_COMMAND_COMPLETE: u8 = 0x0E;
pub const EVT_COMMAND_STATUS: u8 = 0x0F;
pub const EVT_PIN_CODE_REQUEST: u8 = 0x16;
pub const EVT_LINK_KEY_REQUEST: u8 = 0x17;
pub const EVT_LINK_KEY_NOTIFICATION: u8 = 0x18;
pub const EVT_IO_CAPABILITY_REQUEST: u8 = 0x31;
pub const EVT_USER_CONFIRMATION_REQUEST: u8 = 0x33;

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
            offset: [[0; 16]; 2],
            subbands: 8,
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
    /// Decodes into interleaved 16-bit signed stereo PCM (`pcm_out[0]` = L0, `pcm_out[1]` = R0, ...).
    /// Returns `Ok((bytes_consumed, num_stereo_samples))` where `num_stereo_samples = blocks * subbands`.
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
                    if bitcount + slicecount >= bitpool {
                        break;
                    }
                }

                if bitcount + slicecount == bitpool {
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
                while bitcount < bitpool && sb < subbands {
                    if bits[ch][sb] >= 2 && bits[ch][sb] < 16 {
                        bits[ch][sb] += 1;
                        bitcount += 1;
                    } else if bitneed[sb] == bitslice + 1 && bitpool > bitcount + 1 {
                        bits[ch][sb] = 2;
                        bitcount += 2;
                    }
                    sb += 1;
                }

                sb = 0;
                while bitcount < bitpool && sb < subbands {
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
                if bitcount + slicecount >= bitpool {
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
    pub active_handle: Option<u16>,
    pub peer_bd_addr: [u8; 6],
    pub acl_reassembly: AclReassembly,
    pub channels: [L2capChannel; 8],
    pub next_local_cid: u16,
    pub next_ident: u8,
    pub avdtp_state: AvdtpStreamState,
    pub sample_rate_hz: u32,
    pub in_use: bool,
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
            active_handle: None,
            peer_bd_addr: [0; 6],
            acl_reassembly: AclReassembly::new(),
            channels: [L2capChannel::empty(); 8],
            next_local_cid: 0x0040,
            next_ident: 1,
            avdtp_state: AvdtpStreamState::Idle,
            sample_rate_hz: 44100,
            in_use: false,
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
        self.active_handle = None;
        self.peer_bd_addr = [0; 6];
        self.acl_reassembly.reset();
        self.channels = [L2capChannel::empty(); 8];
        self.next_local_cid = 0x0040;
        self.next_ident = 1;
        self.avdtp_state = AvdtpStreamState::Idle;
        self.sample_rate_hz = 44100;
        self.in_use = false;
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
    /// Emits the current command if pending and returns true if a command was transmitted.
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
                // HCI_Write_Local_Name (0x0C13) -> "BASEMENT" + 0
                let mut name_param = [0u8; 248];
                let name = b"BASEMENT";
                name_param[..name.len()].copy_from_slice(name);
                Self::send_cmd(transport, OPCODE_WRITE_LOCAL_NAME, &name_param)
            }
            InitStep::WriteClassOfDevice => {
                // HCI_Write_Class_Of_Device (0x0C24) -> 0x240414
                let cod = [0x14, 0x04, 0x24];
                Self::send_cmd(transport, OPCODE_WRITE_CLASS_OF_DEVICE, &cod)
            }
            InitStep::WriteSimplePairingMode => {
                // HCI_Write_Simple_Pairing_Mode (0x0C56) -> 0x01
                Self::send_cmd(transport, OPCODE_WRITE_SIMPLE_PAIRING_MODE, &[0x01])
            }
            InitStep::WriteExtendedInquiryResponse => {
                // HCI_Write_Extended_Inquiry_Response (0x0C52): FEC_Req (1) + 240 bytes EIR
                let mut eir_param = [0u8; 241];
                eir_param[0] = 0x00; // FEC Not Required
                let mut offset = 1;

                // AD 1: Complete Local Name (0x09) "BASEMENT"
                let name = b"BASEMENT";
                eir_param[offset] = (1 + name.len()) as u8;
                eir_param[offset + 1] = 0x09;
                eir_param[offset + 2..offset + 2 + name.len()].copy_from_slice(name);
                offset += 2 + name.len();

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
                // Min=0x0100, Max=0x0200, Type=0x00 (ADV_IND), Public, Peer=0 (6 bytes), ChannelMap=0x07, Filter=0
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
                // Name: len 9, type 0x09, "BASEMENT"
                let name = b"BASEMENT";
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
                let name = b"BASEMENT";
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
                if params.len() >= 3 {
                    let opcode = u16::from_le_bytes([params[1], params[2]]);
                    self.on_command_complete(opcode);
                }
            }
            EVT_COMMAND_STATUS => {
                if params.len() >= 4 {
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
                    let mut pin_reply = [0u8; 23];
                    pin_reply[..6].copy_from_slice(&params[..6]);
                    pin_reply[6] = 4; // PIN len
                    pin_reply[7..11].copy_from_slice(b"0000");
                    Self::send_cmd(transport, OPCODE_PIN_CODE_REQUEST_REPLY, &pin_reply);
                }
            }
            EVT_LINK_KEY_REQUEST => {
                if params.len() >= 6 {
                    let mut bd_addr = [0u8; 6];
                    bd_addr.copy_from_slice(&params[..6]);
                    let found_key = self.find_bond_key(&bd_addr);
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
                    self.save_bond_key(&bd_addr, &key);
                }
            }
            EVT_IO_CAPABILITY_REQUEST => {
                if params.len() >= 6 {
                    let mut reply = [0u8; 9];
                    reply[..6].copy_from_slice(&params[..6]);
                    reply[6] = 0x03; // NoInputNoOutput
                    reply[7] = 0x00; // OOB data not present
                    reply[8] = 0x02; // MITM not required - General Bonding
                    Self::send_cmd(transport, OPCODE_IO_CAPABILITY_REQUEST_REPLY, &reply);
                }
            }
            EVT_USER_CONFIRMATION_REQUEST => {
                if params.len() >= 6 {
                    Self::send_cmd(transport, OPCODE_USER_CONFIRMATION_REQUEST_REPLY, &params[..6]);
                }
            }
            _ => {}
        }
    }

    fn find_bond_key(&self, addr: &[u8; 6]) -> Option<[u8; 16]> {
        for bond in &self.bond_table {
            if bond.valid && &bond.bd_addr == addr {
                return Some(bond.link_key);
            }
        }
        None
    }

    fn save_bond_key(&mut self, addr: &[u8; 6], key: &[u8; 16]) {
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
                self.handle_l2cap_frame(transport, handle, l2cap_cid, &payload[4..4 + l2cap_len], on_pcm);
            } else {
                // Partial PDU: begin reassembly
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
                    let l2cap_cid =
                        u16::from_le_bytes([self.acl_reassembly.buf[2], self.acl_reassembly.buf[3]]);
                    self.acl_reassembly.active = false;
                    let mut reassembled = [0u8; 1024];
                    let pdu_data_len = total_pdu_len.saturating_sub(4);
                    reassembled[..pdu_data_len].copy_from_slice(&self.acl_reassembly.buf[4..total_pdu_len]);
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
            let cmd_data = &payload[offset + 4..offset + 4 + cmd_len.min(payload.len() - (offset + 4))];
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
        self.next_local_cid += 1;

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

        // Send Connection Response: Success (0x0000) followed by outbound Configuration Request (MTU = 895 = 0x037F)
        // Coalesced into a single L2CAP signaling packet (CID 0x0001) to respect VHCI 1-packet-in-flight limit
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
        // 0x0100: Text("BASEMENT") [13 bytes]
        // 0x0311: Uint16(0x0001) [6 bytes]
        const RECORD_ATTRS: [u8; 77] = [
            0x09, 0x00, 0x00, 0x0A, 0x00, 0x01, 0x00, 0x01,
            0x09, 0x00, 0x01, 0x35, 0x03, 0x19, 0x11, 0x0B,
            0x09, 0x00, 0x04, 0x35, 0x10, 0x35, 0x06, 0x19, 0x01, 0x00, 0x09, 0x00, 0x19, 0x35,
            0x06, 0x19, 0x00, 0x19, 0x09, 0x01, 0x03,
            0x09, 0x00, 0x05, 0x35, 0x03, 0x19, 0x10, 0x02,
            0x09, 0x00, 0x09, 0x35, 0x08, 0x35, 0x06, 0x19, 0x11, 0x0D, 0x09, 0x01, 0x03,
            0x09, 0x01, 0x00, 0x25, 0x08, b'B', b'A', b'S', b'E', b'M', b'E', b'N', b'T',
            0x09, 0x03, 0x11, 0x09, 0x00, 0x01,
        ];

        match pdu_id {
            0x02 => {
                // ServiceSearchRequest -> 0x03 ServiceSearchResponse
                let matches = Self::sdp_matches_audio_sink(params);
                let mut rsp = [0u8; 32];
                rsp[0] = 0x03;
                rsp[1..3].copy_from_slice(&tid.to_be_bytes());
                if matches {
                    rsp[3..5].copy_from_slice(&0x0009u16.to_be_bytes()); // Param len
                    rsp[5..7].copy_from_slice(&0x0001u16.to_be_bytes()); // TotalServiceRecordCount = 1
                    rsp[7..9].copy_from_slice(&0x0001u16.to_be_bytes()); // CurrentServiceRecordCount = 1
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
                    let req_handle = u32::from_be_bytes([params[0], params[1], params[2], params[3]]);
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
        // Search through pattern bytes for 16-bit UUID matching:
        // 0x110B (AudioSink), 0x110D (A2DP), 0x0019 (AVDTP), 0x0100 (L2CAP), or 0x1002 (BrowseRoot)
        for i in 0..params.len().saturating_sub(2) {
            if params[i] == 0x19 {
                let uuid = u16::from_be_bytes([params[i + 1], params[i + 2]]);
                if uuid == 0x110B || uuid == 0x110D || uuid == 0x0019 || uuid == 0x0100 || uuid == 0x1002 {
                    return true;
                }
            }
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
                let in_use_flag = if self.in_use { 0x02 } else { 0x00 };
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
                // Scan for SBC codec parameters: 0x07, 0x06, 0x00, 0x00, oct0, oct1, min_bp, max_bp
                for i in 0..payload.len().saturating_sub(5) {
                    if payload[i] == 0x07 && payload[i + 1] == 0x06 && payload[i + 2] == 0x00 && payload[i + 3] == 0x00 {
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
                let rsp = [tx_label | 0x02, sig_id];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
            AVDTP_OPEN => {
                self.avdtp_state = AvdtpStreamState::Open;
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
                self.in_use = false;
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
                // General Accept
                let rsp = [tx_label | 0x02, sig_id];
                Self::send_l2cap(transport, handle, cid, &rsp);
            }
        }
    }

    fn handle_avdtp_media<F: FnMut(&[u8], u32)>(&mut self, payload: &[u8], on_pcm: &mut F) {
        // RTP Header is minimum 12 bytes
        if payload.len() < 13 {
            return;
        }

        let cc = (payload[0] & 0x0F) as usize;
        let rtp_header_len = 12 + cc * 4;
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
                    self.decoded_frames = self.decoded_frames.wrapping_add(num_stereo_samples as u64);
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

    fn handle_avrcp<T: HciTransport>(
        &mut self,
        transport: &mut T,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) {
        if payload.len() < 3 {
            return;
        }

        let cr = (payload[0] >> 1) & 0x01;
        if cr == 0 {
            // Command from peer: reply with Response (cr = 1)
            let mut rsp = [0u8; 64];
            let len = payload.len().min(64);
            rsp[..len].copy_from_slice(&payload[..len]);
            rsp[0] = (payload[0] & 0xF0) | 0x02; // Response (cr = 1)
            if len >= 4 {
                rsp[3] = 0x09; // Accepted
            }
            Self::send_l2cap(transport, handle, cid, &rsp[..len]);
        }
    }

    /// Polls the Bluetooth transport: drains up to 16 packets and steps the HCI init sequence.
    pub fn poll<T: HciTransport, F: FnMut(&[u8], u32)>(
        &mut self,
        transport: &mut T,
        now_ms: u64,
        mut on_pcm: F,
    ) -> bool {
        let mut had_activity = false;
        let mut rx = [0u8; 1024];

        for _ in 0..16 {
            let n = transport.read_packet(&mut rx);
            if n == 0 {
                break;
            }
            self.rx_packets = self.rx_packets.wrapping_add(1);
            self.process_packet(transport, &rx[..n], &mut on_pcm);
            had_activity = true;
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

    // 119-byte valid SBC frame (16 blocks, 8 subbands, Joint Stereo, Loudness, Bitpool 53)
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
            let cmd_complete = [HCI_PKT_EVENT, EVT_COMMAND_COMPLETE, 0x04, 0x01, last_cmd[1], last_cmd[2], 0x00];
            sink.process_packet(&mut transport, &cmd_complete, |_, _| {});
        }

        assert_eq!(sink.init_step, InitStep::Completed);
        assert_eq!(transport.tx_packets.len(), 17);

        // Verify key parameters in sent commands:
        // 1. Reset (0x0C03)
        assert_eq!(u16::from_le_bytes([transport.tx_packets[0][1], transport.tx_packets[0][2]]), OPCODE_RESET);
        // 3. Write Local Name contains "BASEMENT"
        assert_eq!(&transport.tx_packets[2][4..12], b"BASEMENT");
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
        assert_eq!(u16::from_le_bytes([last_cmd[1], last_cmd[2]]), OPCODE_ACCEPT_CONNECTION_REQUEST);
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
        let link_key_req = [HCI_PKT_EVENT, EVT_LINK_KEY_REQUEST, 6, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        sink.process_packet(&mut transport, &link_key_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(u16::from_le_bytes([last_cmd[1], last_cmd[2]]), OPCODE_LINK_KEY_REQUEST_NEGATIVE_REPLY);
        assert_eq!(&last_cmd[4..10], &peer_addr);

        // 4. IO Capability Request -> NoInputNoOutput (0x03)
        let io_cap_req = [HCI_PKT_EVENT, EVT_IO_CAPABILITY_REQUEST, 6, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        sink.process_packet(&mut transport, &io_cap_req, |_, _| {});
        let last_cmd = transport.tx_packets.last().unwrap();
        assert_eq!(u16::from_le_bytes([last_cmd[1], last_cmd[2]]), OPCODE_IO_CAPABILITY_REQUEST_REPLY);
        assert_eq!(last_cmd[10], 0x03); // NoInputNoOutput

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
        assert_eq!(u16::from_le_bytes([last_cmd[1], last_cmd[2]]), OPCODE_USER_CONFIRMATION_REQUEST_REPLY);
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

        // 1. Search for AudioSink (0x110B) via SDP ServiceSearchAttributeRequest (PDU 0x06)
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
        // Contains "BASEMENT" in service name
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
        assert!(!sink.in_use);

        // 7. CLOSE
        let close_cmd = [
            HCI_PKT_ACL, 0x40, 0x20, 7, 0, 3, 0, sig_cid as u8, (sig_cid >> 8) as u8,
            0x60, AVDTP_CLOSE, 0x04,
        ];
        sink.process_packet(&mut transport, &close_cmd, |_, _| {});
        assert_eq!(sink.avdtp_state, AvdtpStreamState::Idle);
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
    }
}
