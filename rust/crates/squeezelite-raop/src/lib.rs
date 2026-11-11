//! Pure-Rust `#![no_std]` AirPlay 1 (RAOP) receiver engine
//! for squeezelite-esp32.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod crypto;
pub mod dacp;
pub mod dmap;
pub mod mdns;
pub mod rtp;
pub mod rtsp;

pub use crypto::airport_express_key;
pub use crypto::base64_decode;
pub use crypto::base64_encode;
pub use crypto::montgomery_mod_pow;
pub use crypto::pad_pkcs1_v15_type1;
pub use crypto::unpad_oaep_sha1;
pub use crypto::unpad_pkcs1_v15_type2;
pub use crypto::Aes128Cbc;
pub use crypto::BigUint;
pub use crypto::CryptoError;
pub use crypto::RsaPrivateKey;
pub use crypto::Sha1;
pub use crypto::AIRPORT_EXPRESS_D_BYTES;
pub use crypto::AIRPORT_EXPRESS_N_BYTES;
pub use dacp::DacpClient;
pub use dacp::DacpCommand;
pub use dmap::ArtworkMimeType;
pub use dmap::CoverArtwork;
pub use dmap::DmapError;
pub use dmap::DmapMetadata;
pub use dmap::TrackProgress;
pub use mdns::MdnsBuilder;
pub use mdns::MdnsConfig;
pub use rtp::build_resend_request;
pub use rtp::ms_to_ntp;
pub use rtp::ntp_to_ms;
pub use rtp::parse_rtp_info;
pub use rtp::seq_after;
pub use rtp::seq_before;
pub use rtp::seq_diff;
pub use rtp::unwrap_retransmit_packet;
pub use rtp::JitterEntry;
pub use rtp::NtpTimingPacket;
pub use rtp::RtpAudioReceiver;
pub use rtp::RtpControlPacket;
pub use rtp::RtpHeader;
pub use rtp::RtpJitterBuffer;
pub use rtp::RtpReceiverError;
pub use rtp::RtpResendRequest;
pub use rtp::RtpSyncPacket;
pub use rtp::SyncDriftTracker;
pub use rtp::DEFAULT_DRIFT_WINDOW_MS;
pub use rtp::RAOP_MAX_LATENCY_FRAMES;
pub use rtp::RAOP_MIN_LATENCY_FRAMES;
pub use rtp::RAOP_SAMPLE_RATE_HZ;
pub use rtp::RTP_PT_AUDIO_DATA;
pub use rtp::RTP_PT_RESEND_REQUEST;
pub use rtp::RTP_PT_RETRANSMIT_REPLY;
pub use rtp::RTP_PT_SYNC;
pub use rtp::RTP_PT_TIMING_REQUEST;
pub use rtp::RTP_PT_TIMING_RESPONSE;
pub use rtsp::RtspError;
pub use rtsp::RtspMethod;
pub use rtsp::RtspRequest;
pub use rtsp::RtspResponse;
pub use rtsp::RtspServer;
pub use rtsp::SdpSession;
pub use rtsp::ServerState;
