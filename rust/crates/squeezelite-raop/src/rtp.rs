//! RTP audio receiver, control/timing packet codecs, bounded jitter/reorder
//! buffer, AES-128-CBC decryption, ALAC decoding, and sync drift tracker.
//!
//! Ingests UDP RTP audio (`0x60`), control (`0x54` sync, `0x55` resend,
//! `0x56` retransmit reply), and NTP timing (`0x52`/`0x53`) packets.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::crypto::Aes128Cbc;

/// RTP payload type for NTP timing request (`0x52` / `0xD2` with marker).
pub const RTP_PT_TIMING_REQUEST: u8 = 0x52;
/// RTP payload type for NTP timing response (`0x53` / `0xD3` with marker).
pub const RTP_PT_TIMING_RESPONSE: u8 = 0x53;
/// RTP payload type for control sync packet (`0x54` / `0xD4` with marker).
pub const RTP_PT_SYNC: u8 = 0x54;
/// RTP payload type for control resend request (`0x55` / `0xD5` with marker).
pub const RTP_PT_RESEND_REQUEST: u8 = 0x55;
/// RTP payload type for control retransmit reply (`0x56` / `0xD6` with marker).
pub const RTP_PT_RETRANSMIT_REPLY: u8 = 0x56;
pub const RTP_PT_RETRANSMIT_RESPONSE: u8 = RTP_PT_RETRANSMIT_REPLY;
/// RTP payload type for standard RAOP audio data (`0x60` / `96`).
pub const RTP_PT_AUDIO_DATA: u8 = 0x60;

/// Minimum allowed RAOP audio latency in 44.1 kHz frames (`11,025` = 250 ms).
pub const RAOP_MIN_LATENCY_FRAMES: u32 = 11_025;
/// Maximum allowed RAOP audio latency in 44.1 kHz frames (`105,840` = 2.4 s).
pub const RAOP_MAX_LATENCY_FRAMES: u32 = 105_840;
/// Standard RAOP audio sample rate in Hz.
pub const RAOP_SAMPLE_RATE_HZ: u32 = 44_100;
/// Default drift deadband window in milliseconds (`2` ms = 88 frames).
pub const DEFAULT_DRIFT_WINDOW_MS: u32 = 2;

/// Errors encountered while processing RTP audio, control, or timing packets.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RtpReceiverError {
    /// Packet size is under the minimum required for its RTP packet type.
    PacketTooShort,
    /// RTP version in header is not 2.
    InvalidRtpVersion,
    /// Unexpected or unsupported RTP payload type for the target parser.
    UnexpectedPayloadType(u8),
    /// ALAC codec initialization failed from SDP format parameters.
    AlacInitError,
    /// ALAC frame decoding failed.
    AlacDecodeError,
    /// Output buffer is smaller than maximum decoded frame samples.
    BufferTooSmall,
}

impl fmt::Display for RtpReceiverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PacketTooShort => {
                write!(f, "RTP packet too short for header layout")
            }
            Self::InvalidRtpVersion => {
                write!(f, "Invalid RTP version (expected v2)")
            }
            Self::UnexpectedPayloadType(pt) => {
                write!(f, "Unexpected RTP payload type: 0x{:02X}", pt)
            }
            Self::AlacInitError => {
                write!(f, "Failed to initialize ALAC decoder from SDP fmtp")
            }
            Self::AlacDecodeError => write!(f, "ALAC frame decode error"),
            Self::BufferTooSmall => write!(f, "PCM output buffer too small"),
        }
    }
}

/// Computes signed 16-bit wrapping sequence difference `a - b`.
///
/// Returns a positive value if `a` is newer than `b`, negative if `a` is older
/// than `b`, and `0` if equal.
#[must_use]
pub const fn seq_diff(a: u16, b: u16) -> i16 {
    a.wrapping_sub(b) as i16
}

/// Returns `true` if sequence number `a` is strictly after `b` (`a - b > 0`).
#[must_use]
pub const fn seq_after(a: u16, b: u16) -> bool {
    seq_diff(a, b) > 0
}

/// Returns `true` if sequence number `a` is strictly before `b` (`a - b < 0`).
#[must_use]
pub const fn seq_before(a: u16, b: u16) -> bool {
    seq_diff(a, b) < 0
}

/// Converts a 64-bit NTP timestamp (`seconds << 32 | fraction`) to
/// milliseconds.
#[must_use]
pub const fn ntp_to_ms(ntp: u64) -> u64 {
    ((ntp >> 10).wrapping_mul(1000)) >> 22
}

/// Converts milliseconds to a 64-bit NTP timestamp
/// (`seconds << 32 | fraction`).
#[must_use]
pub const fn ms_to_ntp(ms: u64) -> u64 {
    (((ms << 22).saturating_add(999)) / 1000) << 10
}

/// Parsed 12-byte RTP audio packet header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RtpHeader {
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
}

impl RtpHeader {
    /// Parses an RTP header from a byte buffer.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8]), RtpReceiverError> {
        if buf.len() < 12 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        let version = buf[0] >> 6;
        if version != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let payload_type = buf[1] & 0x7F;
        let sequence_number = u16::from_be_bytes([buf[2], buf[3]]);
        let timestamp = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);

        let header = Self {
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
        };
        Ok((header, &buf[12..]))
    }

    /// Serializes this 12-byte RTP header with `marker` bit if requested.
    #[must_use]
    pub fn to_bytes(&self, marker: bool) -> [u8; 12] {
        let mut out = [0u8; 12];
        out[0] = 0x80;
        out[1] = (self.payload_type & 0x7F) | if marker { 0x80 } else { 0x00 };
        out[2..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        out[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        out[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        out
    }
}

/// RTP Control Sync packet (`PT = 0x54` / `0xD4`, 20 bytes).
///
/// Sent periodically on the UDP control port to anchor the sender's RTP sample
/// clock (`rtp_timestamp_now` and `rtp_timestamp_latency`) to its NTP wall
/// clock (`ntp_seconds`, `ntp_fraction`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RtpSyncPacket {
    /// True if the extension bit (`0x10` in byte 0) indicates initial sync.
    pub is_first_sync: bool,
    /// Control flags in bytes `2..4` (e.g. `4` or `7` adds 11,025 sample delay).
    pub flags: u16,
    /// RTP timestamp currently playing at the receiver DAC (`rtp_now - latency`).
    pub rtp_timestamp_latency: u32,
    /// Upper 32 bits of the sender's 64-bit NTP clock (seconds).
    pub ntp_seconds: u32,
    /// Lower 32 bits of the sender's 64-bit NTP clock (fractional seconds).
    pub ntp_fraction: u32,
    /// Current RTP timestamp at the sender corresponding to the NTP time.
    pub rtp_timestamp_now: u32,
}

impl RtpSyncPacket {
    /// Creates a new `RtpSyncPacket`.
    #[must_use]
    pub fn new(
        is_first_sync: bool,
        flags: u16,
        rtp_timestamp_latency: u32,
        ntp_seconds: u32,
        ntp_fraction: u32,
        rtp_timestamp_now: u32,
    ) -> Self {
        Self {
            is_first_sync,
            flags,
            rtp_timestamp_latency,
            ntp_seconds,
            ntp_fraction,
            rtp_timestamp_now,
        }
    }

    /// Parses a 20-byte RTP control sync packet (`PT = 0x54` / `0xD4`).
    pub fn parse(buf: &[u8]) -> Result<Self, RtpReceiverError> {
        if buf.len() < 20 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        if (buf[0] >> 6) != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let pt = buf[1] & 0x7F;
        if pt != RTP_PT_SYNC {
            return Err(RtpReceiverError::UnexpectedPayloadType(pt));
        }
        let is_first_sync = (buf[0] & 0x10) != 0;
        let flags = u16::from_be_bytes([buf[2], buf[3]]);
        let rtp_timestamp_latency =
            u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let ntp_seconds =
            u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let ntp_fraction =
            u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let rtp_timestamp_now =
            u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);

        Ok(Self {
            is_first_sync,
            flags,
            rtp_timestamp_latency,
            ntp_seconds,
            ntp_fraction,
            rtp_timestamp_now,
        })
    }

    /// Encodes this sync packet into its 20-byte wire representation.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[0] = 0x80 | if self.is_first_sync { 0x10 } else { 0x00 };
        out[1] = RTP_PT_SYNC | 0x80; // 0xD4
        out[2..4].copy_from_slice(&self.flags.to_be_bytes());
        out[4..8].copy_from_slice(&self.rtp_timestamp_latency.to_be_bytes());
        out[8..12].copy_from_slice(&self.ntp_seconds.to_be_bytes());
        out[12..16].copy_from_slice(&self.ntp_fraction.to_be_bytes());
        out[16..20].copy_from_slice(&self.rtp_timestamp_now.to_be_bytes());
        out
    }

    /// Combines `ntp_seconds` and `ntp_fraction` into a 64-bit NTP timestamp.
    #[must_use]
    pub const fn remote_ntp(&self) -> u64 {
        ((self.ntp_seconds as u64) << 32) | (self.ntp_fraction as u64)
    }

    /// Computes effective latency in 44.1 kHz frames matching upstream `rtp.c`.
    #[must_use]
    pub fn effective_latency_frames(&self) -> u32 {
        let mut lat = self
            .rtp_timestamp_now
            .wrapping_sub(self.rtp_timestamp_latency);
        if self.flags == 4 || self.flags == 7 {
            lat = lat.saturating_add(RAOP_MIN_LATENCY_FRAMES);
        }
        lat.clamp(RAOP_MIN_LATENCY_FRAMES, RAOP_MAX_LATENCY_FRAMES)
    }
}

/// Unwraps a 4-byte RTP control retransmit reply (`PT = 0x56` / `0xD6`),
/// returning `(control_seq, embedded_rtp_audio_packet)` at offset `+4`.
pub fn unwrap_retransmit_packet(
    buf: &[u8],
) -> Result<(u16, &[u8]), RtpReceiverError> {
    if buf.len() < 16 {
        return Err(RtpReceiverError::PacketTooShort);
    }
    if (buf[0] >> 6) != 2 {
        return Err(RtpReceiverError::InvalidRtpVersion);
    }
    let pt = buf[1] & 0x7F;
    if pt != RTP_PT_RETRANSMIT_REPLY {
        return Err(RtpReceiverError::UnexpectedPayloadType(pt));
    }
    let control_seq = u16::from_be_bytes([buf[2], buf[3]]);
    let inner = &buf[4..];
    if (inner[0] >> 6) != 2 {
        return Err(RtpReceiverError::InvalidRtpVersion);
    }
    Ok((control_seq, inner))
}

/// Parsed packet arriving on the RAOP UDP control socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RtpControlPacket<'a> {
    /// Clock synchronization packet (`PT = 0x54` / `0xD4`).
    Sync(RtpSyncPacket),
    /// Retransmitted audio packet (`PT = 0x56` / `0xD6`) with inner RTP packet
    /// unwrapped from offset `+4`.
    Retransmit {
        /// Sequence number of the control reply header (`bytes 2..4`).
        control_seq: u16,
        /// Full embedded RTP audio packet (`&buf[4..]`).
        inner_packet: &'a [u8],
    },
}

impl<'a> RtpControlPacket<'a> {
    /// Parses a packet received on the RAOP UDP control port (`0x54` or `0x56`).
    pub fn parse(buf: &'a [u8]) -> Result<Self, RtpReceiverError> {
        if buf.len() < 4 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        if (buf[0] >> 6) != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let pt = buf[1] & 0x7F;
        match pt {
            RTP_PT_SYNC => Ok(Self::Sync(RtpSyncPacket::parse(buf)?)),
            RTP_PT_RETRANSMIT_REPLY => {
                let (control_seq, inner_packet) =
                    unwrap_retransmit_packet(buf)?;
                Ok(Self::Retransmit {
                    control_seq,
                    inner_packet,
                })
            }
            other => Err(RtpReceiverError::UnexpectedPayloadType(other)),
        }
    }
}

/// Apple RAOP RTP Resend Request packet (`0x80 0xD5`, 8 bytes).
///
/// Sent from the receiver to the sender's `control_port` when a sequence gap is
/// detected in the incoming audio stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct RtpResendRequest {
    /// Sequence number of this control resend request.
    pub request_seq: u16,
    /// First missing audio RTP sequence number.
    pub missed_seq_start: u16,
    /// Number of consecutive missing audio RTP packets.
    pub missed_count: u16,
}

impl RtpResendRequest {
    /// Creates a new `RtpResendRequest`.
    #[must_use]
    pub const fn new(
        request_seq: u16,
        missed_seq_start: u16,
        missed_count: u16,
    ) -> Self {
        Self {
            request_seq,
            missed_seq_start,
            missed_count,
        }
    }

    /// Encodes the 8-byte resend request packet (`[0x80, 0xD5, ...]`).
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; 8] {
        let req_b = self.request_seq.to_be_bytes();
        let start_b = self.missed_seq_start.to_be_bytes();
        let count_b = self.missed_count.to_be_bytes();
        [
            0x80,
            RTP_PT_RESEND_REQUEST | 0x80, // 0xD5
            req_b[0],
            req_b[1],
            start_b[0],
            start_b[1],
            count_b[0],
            count_b[1],
        ]
    }

    /// Parses an 8-byte `0x55` / `0xD5` resend request packet.
    pub fn parse(buf: &[u8]) -> Result<Self, RtpReceiverError> {
        if buf.len() < 8 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        if (buf[0] >> 6) != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let pt = buf[1] & 0x7F;
        if pt != RTP_PT_RESEND_REQUEST {
            return Err(RtpReceiverError::UnexpectedPayloadType(pt));
        }
        let request_seq = u16::from_be_bytes([buf[2], buf[3]]);
        let missed_seq_start = u16::from_be_bytes([buf[4], buf[5]]);
        let missed_count = u16::from_be_bytes([buf[6], buf[7]]);
        Ok(Self {
            request_seq,
            missed_seq_start,
            missed_count,
        })
    }
}

/// Builds a raw 8-byte RTP resend request (`0x80 0xD5`) packet.
#[must_use]
pub const fn build_resend_request(
    request_seq: u16,
    missed_seq_start: u16,
    missed_count: u16,
) -> [u8; 8] {
    RtpResendRequest::new(request_seq, missed_seq_start, missed_count).to_bytes()
}

/// 32-byte RAOP NTP Timing Request (`0x52`/`0xD2`) and Response
/// (`0x53`/`0xD3`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct NtpTimingPacket {
    /// Payload type (`0x52` for request, `0x53` for response).
    pub payload_type: u8,
    /// Reference (originate) 64-bit NTP timestamp (`bytes 8..16`).
    pub reference_ntp: u64,
    /// Receive 64-bit NTP timestamp (`bytes 16..24`).
    pub receive_ntp: u64,
    /// Transmit 64-bit NTP timestamp (`bytes 24..32`).
    pub transmit_ntp: u64,
}

impl NtpTimingPacket {
    /// Creates a 32-byte NTP timing request (`PT = 0x52`) with a 64-bit transmit
    /// timestamp.
    #[must_use]
    pub const fn new_request(transmit_ntp: u64) -> Self {
        Self {
            payload_type: RTP_PT_TIMING_REQUEST,
            reference_ntp: 0,
            receive_ntp: 0,
            transmit_ntp,
        }
    }

    /// Creates a 32-byte NTP timing request (`PT = 0x52`) placing a 32-bit local
    /// millisecond tick counter in the low 32 bits of `transmit_ntp` (matching
    /// upstream `rtp.c:rtp_request_timing`).
    #[must_use]
    pub const fn new_request_ms(local_now_ms: u32) -> Self {
        Self::new_request(local_now_ms as u64)
    }

    /// Creates a 32-byte NTP timing response (`PT = 0x53`).
    #[must_use]
    pub const fn new_response(
        reference_ntp: u64,
        receive_ntp: u64,
        transmit_ntp: u64,
    ) -> Self {
        Self {
            payload_type: RTP_PT_TIMING_RESPONSE,
            reference_ntp,
            receive_ntp,
            transmit_ntp,
        }
    }

    /// Encodes the timing packet into its 32-byte wire format.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[0] = 0x80;
        out[1] = (self.payload_type & 0x7F) | 0x80;
        out[2..4].copy_from_slice(&7u16.to_be_bytes());
        out[8..16].copy_from_slice(&self.reference_ntp.to_be_bytes());
        out[16..24].copy_from_slice(&self.receive_ntp.to_be_bytes());
        out[24..32].copy_from_slice(&self.transmit_ntp.to_be_bytes());
        out
    }

    /// Parses a 32-byte NTP timing packet (`PT = 0x52` or `0x53`).
    pub fn parse(buf: &[u8]) -> Result<Self, RtpReceiverError> {
        if buf.len() < 32 {
            return Err(RtpReceiverError::PacketTooShort);
        }
        if (buf[0] >> 6) != 2 {
            return Err(RtpReceiverError::InvalidRtpVersion);
        }
        let pt = buf[1] & 0x7F;
        if pt != RTP_PT_TIMING_REQUEST && pt != RTP_PT_TIMING_RESPONSE {
            return Err(RtpReceiverError::UnexpectedPayloadType(pt));
        }
        let reference_ntp = u64::from_be_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14],
            buf[15],
        ]);
        let receive_ntp = u64::from_be_bytes([
            buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22],
            buf[23],
        ]);
        let transmit_ntp = u64::from_be_bytes([
            buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30],
            buf[31],
        ]);
        Ok(Self {
            payload_type: pt,
            reference_ntp,
            receive_ntp,
            transmit_ntp,
        })
    }

    /// Returns the low 32 bits of `reference_ntp` (`bytes 12..16`), used by
    /// `rtp.c` to store the local millisecond timestamp.
    #[must_use]
    pub const fn reference_low_u32(&self) -> u32 {
        self.reference_ntp as u32
    }

    /// Computes round-trip time in milliseconds against a 32-bit local ms
    /// clock.
    #[must_use]
    pub const fn roundtrip_ms(&self, local_now_ms: u32) -> u32 {
        local_now_ms.wrapping_sub(self.reference_low_u32())
    }
}

/// Parses an RTSP `RTP-Info` header string (e.g. `seq=1234;rtptime=567890`)
/// into `(Option<u16>, Option<u32>)`.
#[must_use]
pub fn parse_rtp_info(rtp_info: &str) -> (Option<u16>, Option<u32>) {
    let mut seq = None;
    let mut rtptime = None;
    for part in rtp_info.split(';') {
        let trimmed = part.trim();
        if let Some(eq_idx) = trimmed.find('=') {
            let k = trimmed[..eq_idx].trim();
            let v = trimmed[eq_idx + 1..].trim();
            if k.eq_ignore_ascii_case("seq") {
                seq = v.parse::<u16>().ok();
            } else if k.eq_ignore_ascii_case("rtptime") {
                rtptime = v.parse::<u32>().ok();
            }
        }
    }
    (seq, rtptime)
}

/// A single reordered audio frame or packet entry inside `RtpJitterBuffer`.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct JitterEntry {
    /// Parsed 12-byte RTP header (`sequence_number`, `timestamp`, `ssrc`).
    pub header: RtpHeader,
    /// Raw RTP payload bytes (populated by `push_packet`).
    pub payload: Vec<u8>,
    /// Decoded stereo `i16` PCM samples (populated by `push_pcm` or
    /// `push_and_decode`).
    pub pcm: Vec<i16>,
}

impl JitterEntry {
    /// Creates a new `JitterEntry`.
    #[must_use]
    pub fn new(header: RtpHeader, payload: Vec<u8>, pcm: Vec<i16>) -> Self {
        Self {
            header,
            payload,
            pcm,
        }
    }
}

/// Bounded RTP jitter and packet reorder ring buffer.
///
/// Orders incoming audio (`0x60`) and retransmitted (`0x56`/`0xD6`) packets by
/// 16-bit wrapping sequence number (`seq_diff`), emits `RtpResendRequest`
/// (`0x80 0xD5`) on sequence gaps, and flushes cleanly on RTSP `FLUSH`.
pub struct RtpJitterBuffer {
    capacity: usize,
    slots: Vec<Option<JitterEntry>>,
    read_seq: Option<u16>,
    write_seq: Option<u16>,
    min_allowed_seq: Option<u16>,
    last_flush_rtptime: Option<u32>,
    resend_seq_counter: u16,
    resent_requests: u32,
    recovered_packets: u32,
    late_packets: u32,
}

impl Default for RtpJitterBuffer {
    fn default() -> Self {
        Self::new(128)
    }
}

impl RtpJitterBuffer {
    /// Creates a new bounded `RtpJitterBuffer` with `capacity` slots (clamped
    /// to at least 4 slots).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1).next_power_of_two();
        Self {
            capacity: cap,
            slots: vec![None; cap],
            read_seq: None,
            write_seq: None,
            min_allowed_seq: None,
            last_flush_rtptime: None,
            resend_seq_counter: 1,
            resent_requests: 0,
            recovered_packets: 0,
            late_packets: 0,
        }
    }

    /// Ring buffer slot capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Next sequence number expected to be popped by the audio consumer.
    #[must_use]
    pub fn read_seq(&self) -> Option<u16> {
        self.read_seq
    }

    /// Highest sequence number written into the buffer.
    #[must_use]
    pub fn write_seq(&self) -> Option<u16> {
        self.write_seq
    }

    /// Minimum allowed sequence number set by RTSP `FLUSH` or `RECORD`.
    #[must_use]
    pub fn min_allowed_seq(&self) -> Option<u16> {
        self.min_allowed_seq
    }

    /// Last RTP timestamp recorded from RTSP `FLUSH`.
    #[must_use]
    pub fn last_flush_rtptime(&self) -> Option<u32> {
        self.last_flush_rtptime
    }

    /// Total missing sequence count requested via `0xD5` resend packets.
    #[must_use]
    pub fn resent_requests(&self) -> u32 {
        self.resent_requests
    }

    /// Total out-of-order or retransmitted packets recovered before playback
    /// deadline.
    #[must_use]
    pub fn recovered_packets(&self) -> u32 {
        self.recovered_packets
    }

    /// Total packets discarded because they arrived after `read_seq` advanced.
    #[must_use]
    pub fn late_packets(&self) -> u32 {
        self.late_packets
    }

    /// Number of populated slots currently waiting in the ring buffer.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        let mut count = 0usize;
        for slot in &self.slots {
            if slot.is_some() {
                count += 1;
            }
        }
        count
    }

    fn slot_index(&self, seq: u16) -> usize {
        (seq as usize) % self.capacity
    }

    fn clear_slots(&mut self) {
        self.slots.fill(None);
    }

    /// Pushes a raw RTP audio packet (`0x60`) or retransmit reply (`0x56`/`0xD6`)
    /// into the reorder ring.
    ///
    /// Automatically unwraps the 4-byte `0x56`/`0xD6` header if present.
    /// Returns `Ok(Some(RtpResendRequest))` if a sequence gap was detected.
    pub fn push_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<Option<RtpResendRequest>, RtpReceiverError> {
        let inner = if packet.len() >= 16
            && (packet[1] & 0x7F) == RTP_PT_RETRANSMIT_REPLY
        {
            let (_ctrl_seq, unwrapped) = unwrap_retransmit_packet(packet)?;
            unwrapped
        } else {
            packet
        };
        let (header, payload) = RtpHeader::parse(inner)?;
        if header.payload_type != RTP_PT_AUDIO_DATA
            && header.payload_type != RTP_PT_RETRANSMIT_RESPONSE
        {
            return Err(RtpReceiverError::UnexpectedPayloadType(header.payload_type));
        }
        let entry = JitterEntry::new(header, payload.to_vec(), Vec::new());
        Ok(self.insert_entry(entry))
    }

    /// Pushes pre-decoded stereo PCM samples and their `RtpHeader` into the
    /// reorder ring.
    pub fn push_pcm(
        &mut self,
        header: RtpHeader,
        pcm: &[i16],
    ) -> Option<RtpResendRequest> {
        let entry = JitterEntry::new(header, Vec::new(), pcm.to_vec());
        self.insert_entry(entry)
    }

    /// Decrypts and decodes an RTP packet (`0x60` or `0x56`/`0xD6` retransmit)
    /// using `receiver` and inserts the decoded PCM frame into the reorder ring.
    pub fn push_and_decode(
        &mut self,
        receiver: &mut RtpAudioReceiver,
        packet: &[u8],
    ) -> Result<Option<RtpResendRequest>, RtpReceiverError> {
        let (pcm, header) = receiver.process_packet(packet)?;
        Ok(self.push_pcm(header, pcm))
    }

    fn insert_entry(&mut self, entry: JitterEntry) -> Option<RtpResendRequest> {
        let seq = entry.header.sequence_number;

        if let Some(min_seq) = self.min_allowed_seq {
            if seq_before(seq, min_seq) {
                return None;
            }
            if self.write_seq.is_none() {
                let idx = self.slot_index(seq);
                self.slots[idx] = Some(entry);
                self.read_seq = Some(seq);
                self.write_seq = Some(seq);
                self.min_allowed_seq = None;
                return None;
            }
            self.min_allowed_seq = None;
        }

        let (Some(last_w), Some(cur_r)) = (self.write_seq, self.read_seq)
        else {
            let idx = self.slot_index(seq);
            self.slots[idx] = Some(entry);
            self.read_seq = Some(seq);
            self.write_seq = Some(seq);
            return None;
        };

        let diff_w = seq_diff(seq, last_w);
        if diff_w == 1 {
            // Expected next packet in sequence
            let idx = self.slot_index(seq);
            self.slots[idx] = Some(entry);
            self.write_seq = Some(seq);
            let d = seq_diff(seq, cur_r);
            if d > 0 && (d as usize) >= self.capacity {
                let new_r =
                    seq.wrapping_sub((self.capacity.saturating_sub(1)) as u16);
                self.read_seq = Some(new_r);
            }
            None
        } else if diff_w > 1 {
            // Sequence gap detected: packets `(last_w + 1)..seq` are missing
            let gap = (diff_w - 1) as u16;
            if (gap as usize) >= self.capacity {
                // Gap exceeds ring capacity: reset ring to new anchor
                self.clear_slots();
                let idx = self.slot_index(seq);
                self.slots[idx] = Some(entry);
                self.read_seq = Some(seq);
                self.write_seq = Some(seq);
                return None;
            }

            // Clear skipped slots so stale entries from previous wrap cannot
            // masquerade as ready frames
            let mut s = last_w.wrapping_add(1);
            while s != seq {
                let s_idx = self.slot_index(s);
                self.slots[s_idx] = None;
                s = s.wrapping_add(1);
            }

            let idx = self.slot_index(seq);
            self.slots[idx] = Some(entry);
            self.write_seq = Some(seq);

            let d = seq_diff(seq, cur_r);
            if d > 0 && (d as usize) >= self.capacity {
                let new_r =
                    seq.wrapping_sub((self.capacity.saturating_sub(1)) as u16);
                self.read_seq = Some(new_r);
            }

            let req_seq = self.resend_seq_counter;
            self.resend_seq_counter = self.resend_seq_counter.wrapping_add(1);
            let missed_start = last_w.wrapping_add(1);
            self.resent_requests =
                self.resent_requests.saturating_add(u32::from(gap));
            Some(RtpResendRequest::new(req_seq, missed_start, gap))
        } else {
            // Out-of-order or retransmitted packet (`seq <= last_w`)
            if seq_diff(seq, cur_r) >= 0 {
                let idx = self.slot_index(seq);
                let already_present = self.slots[idx]
                    .as_ref()
                    .is_some_and(|e| e.header.sequence_number == seq);
                if !already_present {
                    self.recovered_packets =
                        self.recovered_packets.saturating_add(1);
                }
                self.slots[idx] = Some(entry);
            } else {
                self.late_packets = self.late_packets.saturating_add(1);
            }
            None
        }
    }

    /// Pops the next in-order entry at `read_seq` if it is present and ready.
    pub fn pop_ready(&mut self) -> Option<JitterEntry> {
        let r_seq = self.read_seq?;
        let idx = self.slot_index(r_seq);
        let is_match = self.slots[idx]
            .as_ref()
            .is_some_and(|e| e.header.sequence_number == r_seq);
        if !is_match {
            return None;
        }
        let entry = self.slots[idx].take();
        if self.write_seq.is_some() {
            self.read_seq = Some(r_seq.wrapping_add(1));
        }
        entry
    }

    /// Drains all contiguous ready entries in sequence order starting from
    /// `read_seq`.
    pub fn drain_ready(&mut self) -> Vec<JitterEntry> {
        let mut out = Vec::new();
        while let Some(entry) = self.pop_ready() {
            out.push(entry);
        }
        out
    }

    /// Skips a missing slot at `read_seq` when playback deadline forces
    /// advancement. Returns `true` if `read_seq` was advanced past a gap.
    pub fn advance_missing(&mut self) -> bool {
        let (Some(r_seq), Some(w_seq)) = (self.read_seq, self.write_seq) else {
            return false;
        };
        if seq_diff(w_seq, r_seq) < 0 {
            return false;
        }
        let idx = self.slot_index(r_seq);
        let is_ready = self.slots[idx]
            .as_ref()
            .is_some_and(|e| e.header.sequence_number == r_seq);
        if is_ready {
            return false;
        }
        self.read_seq = Some(r_seq.wrapping_add(1));
        true
    }

    /// Returns all currently missing sequence numbers between `read_seq` and
    /// `write_seq`.
    #[must_use]
    pub fn missing_sequences(&self) -> Vec<u16> {
        let mut missing = Vec::new();
        let (Some(r_seq), Some(w_seq)) = (self.read_seq, self.write_seq) else {
            return missing;
        };
        if seq_diff(w_seq, r_seq) <= 0 {
            return missing;
        }
        let mut s = r_seq;
        while seq_before(s, w_seq) {
            let idx = self.slot_index(s);
            let present = self.slots[idx]
                .as_ref()
                .is_some_and(|e| e.header.sequence_number == s);
            if !present {
                missing.push(s);
            }
            s = s.wrapping_add(1);
        }
        missing
    }

    /// Flushes all buffered packets with sequence numbers strictly before
    /// `flush_seq` (handling RTSP `FLUSH` `RTP-Info: seq=...;rtptime=...`).
    pub fn flush(&mut self, flush_seq: u16, flush_rtptime: u32) {
        self.min_allowed_seq = Some(flush_seq);
        self.last_flush_rtptime = Some(flush_rtptime);

        for slot in &mut self.slots {
            let should_drop = slot
                .as_ref()
                .is_some_and(|e| seq_before(e.header.sequence_number, flush_seq));
            if should_drop {
                *slot = None;
            }
        }

        if let Some(w_seq) = self.write_seq {
            if seq_before(w_seq, flush_seq) {
                self.clear_slots();
                self.read_seq = None;
                self.write_seq = None;
                return;
            }
        }

        if let Some(r_seq) = self.read_seq {
            if seq_before(r_seq, flush_seq) {
                self.read_seq = Some(flush_seq);
            }
        }
    }
}

/// RTP/NTP clock drift tracker and single-frame stuff/drop calculator.
///
/// Compares RTP timestamps and NTP sync anchors against local DAC consumption
/// and computes a `-1`, `0`, or `+1` stereo frame adjustment when drift exceeds
/// the configured window (default `2` ms = `88` frames at 44.1 kHz).
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SyncDriftTracker {
    sample_rate_hz: u32,
    drift_window_ms: u32,
    drift_window_frames: i32,
    ntp_local_anchor_ms: Option<u64>,
    ntp_remote_anchor: Option<u64>,
    sync_rtp_anchor: Option<u32>,
    sync_local_anchor_ms: Option<u64>,
    latency_frames: u32,
    frames_stuffed: u64,
    frames_dropped: u64,
}

impl Default for SyncDriftTracker {
    fn default() -> Self {
        Self::new(RAOP_SAMPLE_RATE_HZ, DEFAULT_DRIFT_WINDOW_MS)
    }
}

impl SyncDriftTracker {
    /// Creates a new `SyncDriftTracker` for the given sample rate and drift
    /// deadband window in milliseconds.
    #[must_use]
    pub fn new(sample_rate_hz: u32, drift_window_ms: u32) -> Self {
        let rate = sample_rate_hz.max(1);
        let win_frames = ((u64::from(rate) * u64::from(drift_window_ms)) / 1000)
            .max(1) as i32;
        Self {
            sample_rate_hz: rate,
            drift_window_ms,
            drift_window_frames: win_frames,
            ntp_local_anchor_ms: None,
            ntp_remote_anchor: None,
            sync_rtp_anchor: None,
            sync_local_anchor_ms: None,
            latency_frames: RAOP_MIN_LATENCY_FRAMES,
            frames_stuffed: 0,
            frames_dropped: 0,
        }
    }

    /// Configured drift window in stereo frames (e.g. `88` frames for 2 ms at
    /// 44.1 kHz).
    #[must_use]
    pub const fn window_frames(&self) -> u32 {
        self.drift_window_frames as u32
    }

    /// Configured drift window in milliseconds.
    #[must_use]
    pub const fn window_ms(&self) -> u32 {
        self.drift_window_ms
    }

    /// Current effective latency in frames.
    #[must_use]
    pub const fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    /// Total count of `+1` stuffed frames emitted.
    #[must_use]
    pub const fn frames_stuffed(&self) -> u64 {
        self.frames_stuffed
    }

    /// Total count of `-1` dropped frames emitted.
    #[must_use]
    pub const fn frames_dropped(&self) -> u64 {
        self.frames_dropped
    }

    /// Returns `true` if both NTP timing and RTP sync anchors have been
    /// established.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        self.sync_rtp_anchor.is_some() && self.sync_local_anchor_ms.is_some()
    }

    /// Processes an incoming `NtpTimingPacket` (`0x53`), updating the NTP clock
    /// anchor if the round-trip time is `<= 100` ms.
    pub fn record_ntp_timing(
        &mut self,
        timing: &NtpTimingPacket,
        local_now_ms: u32,
    ) -> bool {
        let rtt = timing.roundtrip_ms(local_now_ms);
        if rtt > 100 {
            return false;
        }
        self.ntp_local_anchor_ms = Some(u64::from(timing.reference_low_u32()));
        self.ntp_remote_anchor = Some(timing.receive_ntp);
        true
    }

    /// Processes an incoming `RtpSyncPacket` (`0x54`/`0xD4`), updating the RTP
    /// playback anchor.
    pub fn record_rtp_sync(
        &mut self,
        sync: &RtpSyncPacket,
        fallback_local_ms: u64,
    ) -> bool {
        let local_anchor = if let (Some(loc_ms), Some(rem_ntp)) =
            (self.ntp_local_anchor_ms, self.ntp_remote_anchor)
        {
            let ntp_timestamp = sync.remote_ntp();
            let dt_ntp = (ntp_timestamp as i64).wrapping_sub(rem_ntp as i64);
            if dt_ntp <= 0 {
                return false;
            }
            let remote_gap_ms = ntp_to_ms(dt_ntp as u64);
            if remote_gap_ms > 10_000 {
                return false;
            }
            loc_ms.wrapping_add(remote_gap_ms)
        } else {
            fallback_local_ms
        };

        if let Some(prev_rtp) = self.sync_rtp_anchor {
            let rtp_timestamp = sync.rtp_timestamp_now;
            let drtp = i64::from(rtp_timestamp.wrapping_sub(prev_rtp) as i32);
            let _ = drtp;
        }

        let lat = sync.effective_latency_frames();
        self.latency_frames = lat;
        self.sync_rtp_anchor = Some(sync.rtp_timestamp_now.wrapping_sub(lat));
        self.sync_local_anchor_ms = Some(local_anchor);
        true
    }

    /// Computes expected local playback time in milliseconds for a given RTP
    /// timestamp.
    #[must_use]
    pub fn expected_playtime_ms(&self, rtp_timestamp: u32) -> Option<u64> {
        let rtp_anchor = self.sync_rtp_anchor?;
        let local_anchor_ms = self.sync_local_anchor_ms?;
        let delta_frames = rtp_timestamp.wrapping_sub(rtp_anchor) as i32;
        let delta_ms = (i64::from(delta_frames) * 1000)
            / i64::from(self.sample_rate_hz);
        if delta_ms >= 0 {
            Some(local_anchor_ms.wrapping_add(delta_ms as u64))
        } else {
            Some(local_anchor_ms.saturating_sub((-delta_ms) as u64))
        }
    }

    /// Compares expected RTP timestamp against actual DAC RTP consumption and
    /// returns `-1` (drop 1 stereo frame), `0` (in sync), or `+1` (stuff 1
    /// stereo frame).
    ///
    /// - When `expected_rtp_ts > actual_dac_rtp_ts` by more than `window_frames`
    ///   (DAC playback is lagging behind the sender clock), returns `-1` to drop
    ///   one frame and catch up.
    /// - When `actual_dac_rtp_ts > expected_rtp_ts` by more than `window_frames`
    ///   (DAC playback is ahead of the sender clock), returns `+1` to duplicate
    ///   one frame and slow down.
    pub fn compute_frame_adjustment(
        &mut self,
        expected_rtp_ts: u32,
        actual_dac_rtp_ts: u32,
    ) -> i8 {
        let drift_frames =
            expected_rtp_ts.wrapping_sub(actual_dac_rtp_ts) as i32;
        if drift_frames > self.drift_window_frames {
            self.frames_dropped = self.frames_dropped.saturating_add(1);
            -1
        } else if drift_frames < -self.drift_window_frames {
            self.frames_stuffed = self.frames_stuffed.saturating_add(1);
            1
        } else {
            0
        }
    }

    /// Computes `-1`, `0`, or `+1` stereo frame adjustment by comparing a
    /// packet's scheduled playtime against `local_now_ms`.
    pub fn compute_time_adjustment(
        &mut self,
        packet_rtp_ts: u32,
        local_now_ms: u64,
    ) -> i8 {
        let Some(expected_ms) = self.expected_playtime_ms(packet_rtp_ts) else {
            return 0;
        };
        let drift_ms = (local_now_ms as i64) - (expected_ms as i64);
        let win = i64::from(self.drift_window_ms);
        if drift_ms > win {
            self.frames_dropped = self.frames_dropped.saturating_add(1);
            -1
        } else if drift_ms < -win {
            self.frames_stuffed = self.frames_stuffed.saturating_add(1);
            1
        } else {
            0
        }
    }

    /// Applies a `-1` (drop), `0` (pass-through), or `+1` (stuff) adjustment to
    /// an interleaved stereo `i16` PCM slice (`[L, R, L, R, ...]`).
    pub fn apply_stereo_adjustment(
        adjustment: i8,
        input_pcm: &[i16],
        out_pcm: &mut Vec<i16>,
    ) {
        let input_pcm = &input_pcm[..input_pcm.len() & !1];
        out_pcm.clear();
        if input_pcm.len() < 2 {
            out_pcm.extend_from_slice(input_pcm);
            return;
        }
        match adjustment.cmp(&0) {
            core::cmp::Ordering::Less => {
                // Drop 1 stereo frame (2 i16 samples) if at least 2 frames exist
                if input_pcm.len() >= 4 {
                    out_pcm
                        .extend_from_slice(&input_pcm[..input_pcm.len() - 2]);
                } else {
                    out_pcm.extend_from_slice(input_pcm);
                }
            }
            core::cmp::Ordering::Equal => {
                out_pcm.extend_from_slice(input_pcm);
            }
            core::cmp::Ordering::Greater => {
                // Stuff 1 stereo frame by duplicating the final [L, R] pair
                out_pcm.reserve(input_pcm.len() + 2);
                out_pcm.extend_from_slice(input_pcm);
                let last_l = input_pcm[input_pcm.len() - 2];
                let last_r = input_pcm[input_pcm.len() - 1];
                out_pcm.push(last_l);
                out_pcm.push(last_r);
            }
        }
    }
}

/// RTP Audio Receiver and Decryption Engine.
pub struct RtpAudioReceiver {
    aes_cipher: Option<Aes128Cbc>,
    aes_iv: [u8; 16],
    alac_decoder: Option<alac::Decoder>,
    pcm_out: Vec<i16>,
    decrypt_scratch: Vec<u8>,
    last_seq: Option<u16>,
    last_timestamp: Option<u32>,
    packets_received: u64,
}

impl Default for RtpAudioReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl RtpAudioReceiver {
    /// Creates a new RTP audio receiver.
    pub fn new() -> Self {
        Self {
            aes_cipher: None,
            aes_iv: [0u8; 16],
            alac_decoder: None,
            // Typical max AirPlay ALAC frame is 352 or 4096 samples stereo (8192 i16)
            pcm_out: vec![0i16; 8192],
            decrypt_scratch: Vec::with_capacity(2048),
            last_seq: None,
            last_timestamp: None,
            packets_received: 0,
        }
    }

    /// Configures the AES-128-CBC decryption session key and base IV.
    pub fn configure_crypto(&mut self, key: &[u8; 16], iv: &[u8; 16]) {
        self.aes_cipher = Some(Aes128Cbc::new(key));
        self.aes_iv = *iv;
    }

    /// Disables encryption (cleartext RTP streaming).
    pub fn disable_crypto(&mut self) {
        self.aes_cipher = None;
    }

    /// Configures the ALAC decoder using the SDP `fmtp` attribute.
    ///
    /// e.g. `96 352 0 16 40 10 14 2 255 0 0 44100` or
    /// `352 0 16 40 10 14 2 255 0 0 44100`
    pub fn configure_alac(&mut self, fmtp: &str) -> Result<(), RtpReceiverError> {
        let trimmed = fmtp.trim();
        // If fmtp begins with payload type token ("96 "), strip it
        let params = if let Some(stripped) = trimmed.strip_prefix("96 ") {
            stripped.trim()
        } else {
            trimmed
        };

        let stream_info = alac::StreamInfo::from_sdp_format_parameters(params)
            .map_err(|_| RtpReceiverError::AlacInitError)?;

        let max_samples = stream_info.max_samples_per_packet() as usize;
        if self.pcm_out.len() < max_samples {
            self.pcm_out.resize(max_samples, 0);
        }

        self.alac_decoder = Some(alac::Decoder::new(stream_info));
        Ok(())
    }

    /// Total packets successfully received.
    pub fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Last received sequence number.
    pub fn last_sequence_number(&self) -> Option<u16> {
        self.last_seq
    }

    /// Last received RTP timestamp.
    pub fn last_timestamp(&self) -> Option<u32> {
        self.last_timestamp
    }

    /// Ingests, decrypts, and decodes an RTP audio packet (`0x60` or `0x56`/`0xD6`
    /// retransmit reply) into stereo PCM.
    ///
    /// Returns a slice of decoded `i16` samples `[L, R, L, R, ...]`.
    pub fn process_packet<'a>(
        &'a mut self,
        packet: &[u8],
    ) -> Result<(&'a [i16], RtpHeader), RtpReceiverError> {
        let raw_rtp = if packet.len() >= 16
            && (packet[1] & 0x7F) == RTP_PT_RETRANSMIT_REPLY
        {
            let (_ctrl_seq, inner) = unwrap_retransmit_packet(packet)?;
            inner
        } else {
            packet
        };
        let (header, payload) = RtpHeader::parse(raw_rtp)?;

        self.last_seq = Some(header.sequence_number);
        self.last_timestamp = Some(header.timestamp);
        self.packets_received += 1;

        // In-place decryption if encrypted
        let payload_to_decode = if let Some(ref cipher) = self.aes_cipher {
            self.decrypt_scratch.clear();
            self.decrypt_scratch.extend_from_slice(payload);
            cipher.decrypt_in_place(&self.aes_iv, &mut self.decrypt_scratch);
            &self.decrypt_scratch[..]
        } else {
            payload
        };

        let decoder = self
            .alac_decoder
            .as_mut()
            .ok_or(RtpReceiverError::AlacInitError)?;
        let max_samples = decoder.stream_info().max_samples_per_packet() as usize;

        if self.pcm_out.len() < max_samples {
            self.pcm_out.resize(max_samples, 0);
        }

        let decoded_len = {
            let decoded_slice = decoder
                .decode_packet(payload_to_decode, &mut self.pcm_out)
                .map_err(|_| RtpReceiverError::AlacDecodeError)?;
            decoded_slice.len()
        };

        Ok((&self.pcm_out[..decoded_len], header))
    }
}
