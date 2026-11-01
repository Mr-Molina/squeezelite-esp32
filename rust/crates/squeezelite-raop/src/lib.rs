//! Pure-Rust `#![no_std]` AirPlay 1 (RAOP) receiver engine
//! for squeezelite-esp32.

#![no_std]

extern crate alloc;

pub mod crypto;
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
pub use mdns::MdnsBuilder;
pub use mdns::MdnsConfig;
pub use rtp::RtpAudioReceiver;
pub use rtp::RtpHeader;
pub use rtp::RtpReceiverError;
pub use rtsp::RtspError;
pub use rtsp::RtspMethod;
pub use rtsp::RtspRequest;
pub use rtsp::RtspResponse;
pub use rtsp::RtspServer;
pub use rtsp::SdpSession;
pub use rtsp::ServerState;
