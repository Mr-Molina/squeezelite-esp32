//! Pure-Rust `#![no_std]` Spotify Connect (`cspot`) engine
//! for squeezelite-esp32.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod cdn;
pub mod crypto;
pub mod mercury;
pub mod shannon;
pub mod spirc;
pub mod zeroconf;

pub use cdn::AudioKeyDecryptor;
pub use cdn::CdnAudioDecryptor;
pub use cdn::CdnError;
pub use cdn::CdnStreamFeeder;
pub use cdn::AUDIO_AES_IV;
pub use cdn::SPOTIFY_HEADER_SIZE;
pub use crypto::base64_decode;
pub use crypto::base64_encode;
pub use crypto::big_num_add;
pub use crypto::hmac_sha1;
pub use crypto::montgomery_mod_pow;
pub use crypto::pbkdf2_hmac_sha1;
pub use crypto::Aes128Ctr;
pub use crypto::Aes192Ecb;
pub use crypto::BigUint;
pub use crypto::CryptoError;
pub use crypto::OakleyGroup1Dh;
pub use crypto::Sha1;
pub use crypto::DH_PRIME;
pub use mercury::AudioKeyPacket;
pub use mercury::MercuryError;
pub use mercury::MercuryHeader;
pub use mercury::MercuryPacket;
pub use mercury::MercuryPacketBuilder;
pub use mercury::MercuryType;
pub use shannon::Shannon;
pub use shannon::SHANNON_N;
pub use spirc::decode_varint;
pub use spirc::encode_varint;
pub use spirc::PlayStatus;
pub use spirc::SpircError;
pub use spirc::SpircEvent;
pub use spirc::SpircFrame;
pub use spirc::SpircMessageType;
pub use spirc::SpircStateMachine;
pub use spirc::TrackMetadata;
pub use spirc::SPOTIFY_IMAGE_CDN_BASE;
pub use spirc::SPOTIFY_MAX_VOLUME;
pub use spirc::SPOTIFY_VOLUME_STEP;
pub use zeroconf::get_param;
pub use zeroconf::AuthenticationType;
pub use zeroconf::ReusableCredentials;
pub use zeroconf::SpotifyMdnsBuilder;
pub use zeroconf::SpotifyMdnsConfig;
pub use zeroconf::ZeroConfError;
pub use zeroconf::ZeroConfResponse;
pub use zeroconf::ZeroConfServer;
pub use zeroconf::ZeroConfUser;
pub use zeroconf::AP_AUTH_DECLINED_CMD;
pub use zeroconf::AP_LOGIN_REQUEST_CMD;
pub use zeroconf::AP_WELCOME_CMD;
pub use zeroconf::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS;
