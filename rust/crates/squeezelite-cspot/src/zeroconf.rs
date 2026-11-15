//! ZeroConf mDNS announcement and HTTP receiver for Spotify Connect.
//!
//! Announces `_spotify-connect._tcp.local` on UDP 5353 and handles
//! `action=getInfo` and `action=addUser` HTTP endpoints.

extern crate alloc;

use crate::crypto::base64_decode;
use crate::crypto::base64_encode;
use crate::crypto::hmac_sha1;
use crate::crypto::pbkdf2_hmac_sha1;
use crate::crypto::Aes128Ctr;
use crate::crypto::Aes192Ecb;
use crate::crypto::CryptoError;
use crate::crypto::OakleyGroup1Dh;
use crate::crypto::Sha1;
use crate::spirc::decode_varint;
use crate::spirc::encode_varint;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

/// AP packet command for client authentication login request (`0xAB`).
pub const AP_LOGIN_REQUEST_CMD: u8 = 0xAB;

/// AP packet command for `APWelcome` authentication success (`0xAC`).
pub const AP_WELCOME_CMD: u8 = 0xAC;

/// AP packet command for `APLError` authentication failure (`0xAD`).
pub const AP_AUTH_DECLINED_CMD: u8 = 0xAD;

/// Spotify authentication type for stored reusable credentials (`0x01`).
pub const AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS: u32 = 0x01;

const REUSABLE_CRED_MAGIC: [u8; 4] = *b"CSRC";
const REUSABLE_CRED_VERSION: u8 = 1;

/// Spotify authentication credential type (`authentication.proto`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AuthenticationType {
    /// Standard username and password (`0x00`).
    UserPass = 0x00,
    /// Stored reusable Spotify credentials (`0x01`).
    StoredSpotifyCredentials = 0x01,
    /// Stored Facebook credentials (`0x02`).
    StoredFacebookCredentials = 0x02,
    /// Ephemeral Spotify OAuth token (`0x03`).
    SpotifyToken = 0x03,
    /// Facebook token (`0x04`).
    FacebookToken = 0x04,
}

impl From<u32> for AuthenticationType {
    fn from(val: u32) -> Self {
        match val {
            0x00 => Self::UserPass,
            0x01 => Self::StoredSpotifyCredentials,
            0x02 => Self::StoredFacebookCredentials,
            0x03 => Self::SpotifyToken,
            0x04 => Self::FacebookToken,
            _ => Self::StoredSpotifyCredentials,
        }
    }
}

/// Reusable Spotify credentials extracted from `APWelcome` (`0xAC`) and
/// persisted to `FlashJournal` with AES-128-CTR + HMAC-SHA1 authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReusableCredentials {
    /// Canonical Spotify username (`APWelcome.canonical_username`).
    pub canonical_username: String,
    /// Authentication type (`AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS = 1`).
    pub auth_type: u32,
    /// Reusable authentication blob (`APWelcome.reusable_auth_credentials`).
    pub auth_data: Vec<u8>,
}

impl ReusableCredentials {
    /// Creates reusable credentials from explicit fields.
    #[must_use]
    pub fn new(canonical_username: impl Into<String>, auth_type: u32, auth_data: Vec<u8>) -> Self {
        Self {
            canonical_username: canonical_username.into(),
            auth_type,
            auth_data,
        }
    }

    /// Creates reusable credentials with
    /// `AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS`.
    #[must_use]
    pub fn stored_spotify(canonical_username: impl Into<String>, auth_data: Vec<u8>) -> Self {
        Self::new(
            canonical_username,
            AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS,
            auth_data,
        )
    }

    /// Extracts reusable credentials from an AP packet (`cmd` must be `0xAC`).
    pub fn from_ap_welcome_packet(cmd: u8, payload: &[u8]) -> Result<Self, CryptoError> {
        if cmd != AP_WELCOME_CMD {
            return Err(CryptoError::DecryptionFailed);
        }
        Self::from_ap_welcome(payload)
    }

    /// Parses an `APWelcome` (`0xAC`) protobuf payload (`authentication.proto`).
    pub fn from_ap_welcome(payload: &[u8]) -> Result<Self, CryptoError> {
        let mut pos = 0usize;
        let mut canonical_username = String::new();
        let mut auth_type = AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS;
        let mut auth_data = Vec::new();

        while pos < payload.len() {
            let key =
                decode_varint(payload, &mut pos).map_err(|_| CryptoError::InvalidCiphertext)?;
            let field_num = (key >> 3) as u32;
            let wire_type = (key & 0x07) as u8;

            match wire_type {
                0 => {
                    let val = decode_varint(payload, &mut pos)
                        .map_err(|_| CryptoError::InvalidCiphertext)?;
                    if field_num == 0x1E || field_num == 2 {
                        auth_type = val as u32;
                    }
                }
                1 => {
                    if pos + 8 > payload.len() {
                        return Err(CryptoError::InvalidCiphertext);
                    }
                    pos += 8;
                }
                2 => {
                    let len = decode_varint(payload, &mut pos)
                        .map_err(|_| CryptoError::InvalidCiphertext)?
                        as usize;
                    if pos + len > payload.len() {
                        return Err(CryptoError::InvalidCiphertext);
                    }
                    let slice = &payload[pos..pos + len];
                    pos += len;

                    if field_num == 0x0A || field_num == 1 {
                        let s = core::str::from_utf8(slice)
                            .map_err(|_| CryptoError::InvalidCiphertext)?;
                        canonical_username = s.to_string();
                    } else if field_num == 0x28 || field_num == 3 {
                        auth_data = slice.to_vec();
                    }
                }
                5 => {
                    if pos + 4 > payload.len() {
                        return Err(CryptoError::InvalidCiphertext);
                    }
                    pos += 4;
                }
                _ => return Err(CryptoError::InvalidCiphertext),
            }
        }

        if canonical_username.is_empty() || auth_data.is_empty() {
            return Err(CryptoError::InvalidCiphertext);
        }

        Ok(Self {
            canonical_username,
            auth_type,
            auth_data,
        })
    }

    /// Encodes `APWelcome` (`0xAC`) protobuf bytes matching `authentication.proto`.
    #[must_use]
    pub fn encode_ap_welcome(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.canonical_username.len() + self.auth_data.len() + 24);

        // Field 10 (0x0A): canonical_username (string, wire type 2)
        encode_varint(&mut buf, (0x0A << 3) | 2);
        let user_bytes = self.canonical_username.as_bytes();
        encode_varint(&mut buf, user_bytes.len() as u64);
        buf.extend_from_slice(user_bytes);

        // Field 20 (0x14): account_type_logged_in = 0 (Spotify)
        encode_varint(&mut buf, 0x14 << 3);
        encode_varint(&mut buf, 0);

        // Field 25 (0x19): credentials_type_logged_in = 0 (Spotify)
        encode_varint(&mut buf, 0x19 << 3);
        encode_varint(&mut buf, 0);

        // Field 30 (0x1E): reusable_auth_credentials_type (varint)
        encode_varint(&mut buf, 0x1E << 3);
        encode_varint(&mut buf, self.auth_type as u64);

        // Field 40 (0x28): reusable_auth_credentials (bytes, wire type 2)
        encode_varint(&mut buf, (0x28 << 3) | 2);
        encode_varint(&mut buf, self.auth_data.len() as u64);
        buf.extend_from_slice(&self.auth_data);

        buf
    }

    /// Encodes `LoginCredentials` protobuf bytes (`authentication.proto`)
    /// for reconnect.
    #[must_use]
    pub fn encode_login_credentials(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.canonical_username.len() + self.auth_data.len() + 16);

        // Field 10 (0x0A): username
        encode_varint(&mut buf, (0x0A << 3) | 2);
        let user_bytes = self.canonical_username.as_bytes();
        encode_varint(&mut buf, user_bytes.len() as u64);
        buf.extend_from_slice(user_bytes);

        // Field 20 (0x14): typ
        encode_varint(&mut buf, 0x14 << 3);
        encode_varint(&mut buf, self.auth_type as u64);

        // Field 30 (0x1E): auth_data
        encode_varint(&mut buf, (0x1E << 3) | 2);
        encode_varint(&mut buf, self.auth_data.len() as u64);
        buf.extend_from_slice(&self.auth_data);

        buf
    }

    /// Serializes credentials into deterministic binary frame before encryption.
    #[must_use]
    pub fn serialize_plain(&self) -> Vec<u8> {
        let user_bytes = self.canonical_username.as_bytes();
        let user_len = core::cmp::min(user_bytes.len(), u16::MAX as usize);
        let auth_len = core::cmp::min(self.auth_data.len(), u16::MAX as usize);

        let mut out = Vec::with_capacity(13 + user_len + auth_len);
        out.extend_from_slice(&REUSABLE_CRED_MAGIC);
        out.push(REUSABLE_CRED_VERSION);
        out.extend_from_slice(&self.auth_type.to_be_bytes());
        out.extend_from_slice(&(user_len as u16).to_be_bytes());
        out.extend_from_slice(&user_bytes[..user_len]);
        out.extend_from_slice(&(auth_len as u16).to_be_bytes());
        out.extend_from_slice(&self.auth_data[..auth_len]);
        out
    }

    /// Deserializes credentials from deterministic plaintext binary frame.
    pub fn deserialize_plain(data: &[u8]) -> Result<Self, CryptoError> {
        if data.len() < 13 {
            return Err(CryptoError::DecryptionFailed);
        }
        if data[0..4] != REUSABLE_CRED_MAGIC || data[4] != REUSABLE_CRED_VERSION {
            return Err(CryptoError::DecryptionFailed);
        }
        let auth_type = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
        let user_len = u16::from_be_bytes([data[9], data[10]]) as usize;
        if 11 + user_len + 2 > data.len() {
            return Err(CryptoError::DecryptionFailed);
        }
        let username_str = core::str::from_utf8(&data[11..11 + user_len])
            .map_err(|_| CryptoError::DecryptionFailed)?;
        let auth_pos = 11 + user_len;
        let auth_len = u16::from_be_bytes([data[auth_pos], data[auth_pos + 1]]) as usize;
        if auth_pos + 2 + auth_len != data.len() {
            return Err(CryptoError::DecryptionFailed);
        }
        let auth_data = data[auth_pos + 2..auth_pos + 2 + auth_len].to_vec();
        if username_str.is_empty() || auth_data.is_empty() {
            return Err(CryptoError::DecryptionFailed);
        }

        Ok(Self {
            canonical_username: username_str.to_string(),
            auth_type,
            auth_data,
        })
    }

    fn derive_keys(secret: &[u8]) -> ([u8; 16], [u8; 20]) {
        let base_hash = Sha1::digest(secret);
        let base_key = &base_hash[0..16];
        let enc_full = hmac_sha1(base_key, b"encryption");
        let mac_key = hmac_sha1(base_key, b"checksum");
        let mut enc_key = [0u8; 16];
        enc_key.copy_from_slice(&enc_full[0..16]);
        (enc_key, mac_key)
    }

    /// Encrypts and authenticates credentials with AES-128-CTR + HMAC-SHA1
    /// for `FlashJournal` persistence (`[16B IV || ciphertext || 20B HMAC]`).
    #[must_use]
    pub fn encrypt(&self, secret: &[u8], iv: [u8; 16]) -> Vec<u8> {
        let (enc_key, mac_key) = Self::derive_keys(secret);
        let mut ciphertext = self.serialize_plain();
        let mut ctr = Aes128Ctr::new(&enc_key, iv);
        ctr.process(&mut ciphertext);

        let mut payload = Vec::with_capacity(16 + ciphertext.len() + 20);
        payload.extend_from_slice(&iv);
        payload.extend_from_slice(&ciphertext);

        let mac = hmac_sha1(&mac_key, &payload);
        payload.extend_from_slice(&mac);
        payload
    }

    /// Verifies HMAC-SHA1 and decrypts AES-128-CTR credentials from `FlashJournal`.
    pub fn decrypt(blob: &[u8], secret: &[u8]) -> Result<Self, CryptoError> {
        if blob.len() < 16 + 13 + 20 {
            return Err(CryptoError::InvalidCiphertext);
        }

        let (enc_key, mac_key) = Self::derive_keys(secret);
        let mac_offset = blob.len() - 20;
        let authenticated_part = &blob[..mac_offset];
        let expected_mac = &blob[mac_offset..];
        let computed_mac = hmac_sha1(&mac_key, authenticated_part);

        let mut diff = 0u8;
        for (&a, &b) in computed_mac.iter().zip(expected_mac.iter()) {
            diff |= a ^ b;
        }
        if diff != 0 {
            return Err(CryptoError::DecryptionFailed);
        }

        let mut iv = [0u8; 16];
        iv.copy_from_slice(&blob[0..16]);
        let mut decrypted = blob[16..mac_offset].to_vec();

        let mut ctr = Aes128Ctr::new(&enc_key, iv);
        ctr.process(&mut decrypted);

        Self::deserialize_plain(&decrypted)
    }

    /// Encodes encrypted credentials as Base64 for string-valued
    /// `FlashJournal` storage.
    #[must_use]
    pub fn encrypt_base64(&self, secret: &[u8], iv: [u8; 16]) -> String {
        base64_encode(&self.encrypt(secret, iv))
    }

    /// Decodes and decrypts Base64-encoded credentials from `FlashJournal`.
    pub fn decrypt_base64(encoded: &str, secret: &[u8]) -> Result<Self, CryptoError> {
        let raw = base64_decode(encoded)?;
        Self::decrypt(&raw, secret)
    }
}

/// Configuration for Spotify Connect ZeroConf mDNS announcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyMdnsConfig {
    /// Human-readable player name (e.g. "BASEMENT").
    pub player_name: String,
    /// Device 6-byte Ethernet / Wi-Fi MAC address.
    pub mac_address: [u8; 6],
    /// Device 4-byte IPv4 address.
    pub ip_address: [u8; 4],
    /// HTTP ZeroConf listening port (default 80 or 8080).
    pub port: u16,
}

impl Default for SpotifyMdnsConfig {
    fn default() -> Self {
        Self {
            player_name: "BASEMENT".to_string(),
            mac_address: [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34],
            ip_address: [172, 22, 1, 15],
            port: 80,
        }
    }
}

impl SpotifyMdnsConfig {
    /// Service type for Spotify Connect mDNS.
    pub const SERVICE_TYPE: &'static str = "_spotify-connect._tcp.local";

    /// Instance name: `<player_name>._spotify-connect._tcp.local`.
    pub fn instance_name(&self) -> String {
        format!("{}.{}", self.player_name, Self::SERVICE_TYPE)
    }

    /// Host name: `<player_name>.local`.
    pub fn host_name(&self) -> String {
        format!("{}.local", self.player_name)
    }
}

/// Binary mDNS response packet encoder for Spotify Connect.
pub struct SpotifyMdnsBuilder;

impl SpotifyMdnsBuilder {
    /// Encodes a dotted domain name into length-prefixed DNS wire labels.
    pub fn encode_dns_name(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for label in name.split('.') {
            if label.is_empty() {
                continue;
            }
            let bytes = label.as_bytes();
            let len = core::cmp::min(bytes.len(), 63) as u8;
            out.push(len);
            out.extend_from_slice(&bytes[..len as usize]);
        }
        out.push(0);
        out
    }

    fn append_ptr_record(
        records: &mut Vec<u8>,
        service_type: &str,
        instance_target: &str,
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(service_type));
        records.extend_from_slice(&12u16.to_be_bytes()); // Type: PTR
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let target_bytes = Self::encode_dns_name(instance_target);
        records.extend_from_slice(&(target_bytes.len() as u16).to_be_bytes());
        records.extend_from_slice(&target_bytes);
    }

    fn append_srv_record(
        records: &mut Vec<u8>,
        instance_name: &str,
        port: u16,
        target_host: &str,
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(instance_name));
        records.extend_from_slice(&33u16.to_be_bytes()); // Type: SRV
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let host_bytes = Self::encode_dns_name(target_host);
        let rdlength = 6 + host_bytes.len() as u16;
        records.extend_from_slice(&rdlength.to_be_bytes());
        records.extend_from_slice(&0u16.to_be_bytes()); // Priority
        records.extend_from_slice(&0u16.to_be_bytes()); // Weight
        records.extend_from_slice(&port.to_be_bytes());
        records.extend_from_slice(&host_bytes);
    }

    fn append_txt_record(
        records: &mut Vec<u8>,
        instance_name: &str,
        attributes: &[&[u8]],
        ttl: u32,
    ) {
        records.extend_from_slice(&Self::encode_dns_name(instance_name));
        records.extend_from_slice(&16u16.to_be_bytes()); // Type: TXT
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (flush)
        records.extend_from_slice(&ttl.to_be_bytes());

        let mut rdata = Vec::new();
        for attr in attributes {
            let len = core::cmp::min(attr.len(), 255) as u8;
            rdata.push(len);
            rdata.extend_from_slice(&attr[..len as usize]);
        }

        records.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        records.extend_from_slice(&rdata);
    }

    fn append_a_record(records: &mut Vec<u8>, host_name: &str, ip: [u8; 4], ttl: u32) {
        records.extend_from_slice(&Self::encode_dns_name(host_name));
        records.extend_from_slice(&1u16.to_be_bytes()); // Type: A
        records.extend_from_slice(&0x8001u16.to_be_bytes()); // Class: IN (flush)
        records.extend_from_slice(&ttl.to_be_bytes());
        records.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH = 4
        records.extend_from_slice(&ip);
    }

    /// Builds a full binary mDNS response packet announcing Spotify Connect.
    pub fn build_announcement(config: &SpotifyMdnsConfig) -> Vec<u8> {
        let mut packet = Vec::with_capacity(512);

        // Header: ID=0, Flags=0x8400 (Authoritative Answer), 4 Answers
        packet.extend_from_slice(&0u16.to_be_bytes());
        packet.extend_from_slice(&0x8400u16.to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes()); // QDCOUNT
        packet.extend_from_slice(&4u16.to_be_bytes()); // ANCOUNT = 4
        packet.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
        packet.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

        let ttl = 120u32;
        let instance = config.instance_name();
        let host = config.host_name();

        Self::append_ptr_record(&mut packet, SpotifyMdnsConfig::SERVICE_TYPE, &instance, ttl);

        Self::append_srv_record(&mut packet, &instance, config.port, &host, ttl);

        let txt_records: [&[u8]; 3] = [b"CPath=/", b"VERSION=1.0", b"Stack=SP"];
        Self::append_txt_record(&mut packet, &instance, &txt_records, ttl);

        Self::append_a_record(&mut packet, &host, config.ip_address, ttl);

        packet
    }
}

/// ZeroConf extracted user credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZeroConfUser {
    /// Spotify account username.
    pub username: String,
    /// Spotify authentication type (e.g. 1 for login blob).
    pub auth_type: u32,
    /// Extracted authentication token/blob.
    pub auth_data: Vec<u8>,
    /// Initiating Spotify client device name.
    pub device_name: String,
}

/// ZeroConf HTTP error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZeroConfError {
    /// Missing required HTTP parameter.
    MissingParameter,
    /// Cryptographic decode failure.
    Crypto(CryptoError),
    /// UTF-8 decode error.
    InvalidUtf8,
    /// HTTP method or URL not matched.
    NotFound,
}

impl From<CryptoError> for ZeroConfError {
    fn from(err: CryptoError) -> Self {
        Self::Crypto(err)
    }
}

impl fmt::Display for ZeroConfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingParameter => write!(f, "missing required HTTP parameter"),
            Self::Crypto(e) => write!(f, "cryptographic error: {}", e),
            Self::InvalidUtf8 => write!(f, "invalid utf-8 encoding"),
            Self::NotFound => write!(f, "route not found"),
        }
    }
}

/// ZeroConf HTTP response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZeroConfResponse {
    /// HTTP status code (e.g. 200).
    pub status_code: u16,
    /// Content-Type header.
    pub content_type: &'static str,
    /// Body content bytes.
    pub body: Vec<u8>,
}

fn read_blob_int(data: &[u8], pos: &mut usize) -> Result<u32, CryptoError> {
    if *pos >= data.len() {
        return Err(CryptoError::DecryptionFailed);
    }
    let lo = data[*pos];
    *pos += 1;
    if (lo & 0x80) == 0 {
        return Ok(lo as u32);
    }
    if *pos >= data.len() {
        return Err(CryptoError::DecryptionFailed);
    }
    let hi = data[*pos];
    *pos += 1;
    Ok(((lo & 0x7f) as u32) | (((hi & 0x7f) as u32) << 7))
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let s_bytes = s.as_bytes();
    let mut i = 0;
    while i < s_bytes.len() {
        match s_bytes[i] {
            b'%' => {
                if i + 2 < s_bytes.len() {
                    if let Ok(hex_str) = core::str::from_utf8(&s_bytes[i + 1..i + 3]) {
                        if let Ok(byte) = u8::from_str_radix(hex_str, 16) {
                            bytes.push(byte);
                            i += 3;
                            continue;
                        }
                    }
                }
                bytes.push(b'%');
                i += 1;
            }
            b'+' => {
                bytes.push(b' ');
                i += 1;
            }
            b => {
                bytes.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Extracts a query parameter from URL or form body.
pub fn get_param(query: &str, key: &str) -> Option<String> {
    for part in query.split('&') {
        let mut kv = part.splitn(2, '=');
        let k = kv.next()?.trim();
        let v = kv.next().unwrap_or("").trim();
        if k == key {
            return Some(url_decode(v));
        }
    }
    None
}

/// Implements Spotify ZeroConf HTTP endpoint protocol.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZeroConfServer {
    last_user: Option<ZeroConfUser>,
}

impl ZeroConfServer {
    /// Creates a new `ZeroConfServer` instance with no authenticated user.
    #[must_use]
    pub fn new() -> Self {
        Self { last_user: None }
    }

    /// Returns the last authenticated user, if any.
    #[must_use]
    pub fn last_user(&self) -> Option<&ZeroConfUser> {
        self.last_user.as_ref()
    }

    /// Takes the last authenticated user out of the server, leaving `None`.
    pub fn take_last_user(&mut self) -> Option<ZeroConfUser> {
        self.last_user.take()
    }

    /// Escapes quotes and backslashes and strips control characters (< 0x20)
    /// for safe JSON string interpolation.
    pub fn escape_json_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if (c as u32) < 0x20 {
                continue;
            }
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                _ => out.push(c),
            }
        }
        out
    }

    /// Builds JSON response for `action=getInfo`.
    pub fn build_get_info_json(device_id: &str, remote_name: &str, public_key_b64: &str) -> String {
        let esc_device_id = Self::escape_json_string(device_id);
        let esc_remote_name = Self::escape_json_string(remote_name);
        let esc_pub_key = Self::escape_json_string(public_key_b64);
        format!(
            "{{\"status\":101,\"statusString\":\"OK\",\"spotifyError\":0,\
\"version\":\"2.7.1\",\"libraryVersion\":\"1.0.0\",\"accountReq\":\"PREMIUM\",\
\"brandDisplayName\":\"cspot\",\"modelDisplayName\":\"{}\",\"voiceSupport\":\"NO\",\
\"availability\":\"\",\"productID\":0,\"tokenType\":\"default\",\"groupStatus\":\"NONE\",\
\"resolverVersion\":\"0\",\"scope\":\"streaming,client-authorization-universal\",\
\"activeUser\":\"\",\"deviceID\":\"{}\",\"remoteName\":\"{}\",\
\"publicKey\":\"{}\",\"deviceType\":\"AudioDongle\"}}",
            esc_remote_name, esc_device_id, esc_remote_name, esc_pub_key
        )
    }

    /// Decrypts Spotify login blob from `action=addUser`.
    pub fn decode_blob(
        blob: &[u8],
        shared_key: &[u8],
        device_id: &str,
        username: &str,
    ) -> Result<(u32, Vec<u8>), CryptoError> {
        if blob.len() < 36 {
            return Err(CryptoError::InvalidCiphertext);
        }

        let iv: [u8; 16] = blob[0..16]
            .try_into()
            .map_err(|_| CryptoError::InvalidCiphertext)?;
        let mut encrypted = blob[16..blob.len() - 20].to_vec();
        let checksum = &blob[blob.len() - 20..];

        let base_key_full = Sha1::digest(shared_key);
        let base_key = &base_key_full[0..16];

        let checksum_key = hmac_sha1(base_key, b"checksum");
        let enc_key_full = hmac_sha1(base_key, b"encryption");
        let enc_key: [u8; 16] = enc_key_full[0..16]
            .try_into()
            .map_err(|_| CryptoError::InvalidCiphertext)?;

        let mac = hmac_sha1(&checksum_key, &encrypted);
        let diff = (0..20).fold(0u8, |acc, i| acc | (mac[i] ^ checksum[i]));
        if diff != 0 {
            return Err(CryptoError::DecryptionFailed);
        }

        let mut ctr = Aes128Ctr::new(&enc_key, iv);
        ctr.process(&mut encrypted);

        // Secondary blob decode
        let enc_str =
            core::str::from_utf8(&encrypted).map_err(|_| CryptoError::DecryptionFailed)?;
        let mut blob_data = base64_decode(enc_str)?;

        let secret = Sha1::digest(device_id.as_bytes());
        let pk_base_key = pbkdf2_hmac_sha1(&secret, username.as_bytes(), 256, 20);

        let base_key_hashed = Sha1::digest(&pk_base_key);
        let mut ecb_key = [0u8; 24];
        ecb_key[..20].copy_from_slice(&base_key_hashed);
        ecb_key[20..24].copy_from_slice(&[0x00, 0x00, 0x00, 0x14]);

        let ecb = Aes192Ecb::new(&ecb_key);
        ecb.decrypt(&mut blob_data);

        let l = blob_data.len();
        if l < 17 {
            return Err(CryptoError::DecryptionFailed);
        }

        for i in 0..(l - 16) {
            blob_data[l - i - 1] ^= blob_data[l - i - 17];
        }

        // Parse extracted loginData
        let mut pos = 1;
        let skip_len = read_blob_int(&blob_data, &mut pos)? as usize;
        pos = pos
            .checked_add(skip_len)
            .ok_or(CryptoError::DecryptionFailed)?;
        pos = pos.checked_add(1).ok_or(CryptoError::DecryptionFailed)?;
        let auth_type = read_blob_int(&blob_data, &mut pos)?;
        pos = pos.checked_add(1).ok_or(CryptoError::DecryptionFailed)?;
        let auth_size = read_blob_int(&blob_data, &mut pos)? as usize;

        let end_pos = pos
            .checked_add(auth_size)
            .ok_or(CryptoError::DecryptionFailed)?;
        if end_pos > blob_data.len() {
            return Err(CryptoError::DecryptionFailed);
        }

        let auth_data = blob_data[pos..end_pos].to_vec();
        Ok((auth_type, auth_data))
    }

    /// Handles an incoming ZeroConf HTTP request (`getInfo` or `addUser`), storing
    /// the authenticated user in `self.last_user`.
    pub fn handle_request(
        &mut self,
        uri: &str,
        body: &str,
        dh: &OakleyGroup1Dh,
        device_id: &str,
        remote_name: &str,
    ) -> Result<ZeroConfResponse, ZeroConfError> {
        let combined_params = if let Some(idx) = uri.find('?') {
            format!("{}&{}", body, &uri[idx + 1..])
        } else {
            body.to_string()
        };

        let action = get_param(&combined_params, "action")
            .or_else(|| {
                if uri.contains("spotify_info") {
                    Some("getInfo".to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        match action.as_str() {
            "getInfo" => {
                let pub_b64 = base64_encode(dh.public_key());
                let json = Self::build_get_info_json(device_id, remote_name, &pub_b64);
                Ok(ZeroConfResponse {
                    status_code: 200,
                    content_type: "application/json",
                    body: json.into_bytes(),
                })
            }
            "addUser" => {
                if body.trim().is_empty() {
                    // Reject GET requests attempting addUser
                    return Err(ZeroConfError::NotFound);
                }
                let username = get_param(&combined_params, "userName")
                    .ok_or(ZeroConfError::MissingParameter)?;
                let blob_b64 =
                    get_param(&combined_params, "blob").ok_or(ZeroConfError::MissingParameter)?;
                let client_key_b64 = get_param(&combined_params, "clientKey")
                    .ok_or(ZeroConfError::MissingParameter)?;
                let dev_name = get_param(&combined_params, "deviceName")
                    .unwrap_or_else(|| "Spotify App".to_string());

                let blob_bytes = base64_decode(&blob_b64)?;
                let client_key_bytes = base64_decode(&client_key_b64)?;

                let shared_key = dh.calculate_shared(&client_key_bytes)?;
                let (auth_type, auth_data) =
                    Self::decode_blob(&blob_bytes, &shared_key, device_id, &username)?;

                self.last_user = Some(ZeroConfUser {
                    username: username.clone(),
                    auth_type,
                    auth_data,
                    device_name: dev_name.clone(),
                });

                let esc_username = Self::escape_json_string(&username);
                let esc_dev_name = Self::escape_json_string(&dev_name);
                let resp_json = format!(
                    "{{\"status\":101,\"statusString\":\"OK\",\"spotifyError\":0,\
\"userName\":\"{}\",\"deviceName\":\"{}\"}}",
                    esc_username, esc_dev_name
                );

                Ok(ZeroConfResponse {
                    status_code: 200,
                    content_type: "application/json",
                    body: resp_json.into_bytes(),
                })
            }
            _ => Err(ZeroConfError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json_string() {
        let raw = "foo\"bar\\baz\u{0007}\ntest";
        let escaped = ZeroConfServer::escape_json_string(raw);
        assert_eq!(escaped, "foo\\\"bar\\\\baztest");
    }

    #[test]
    fn test_reject_add_user_get_request() {
        let dh = OakleyGroup1Dh::from_private_key([0x01u8; 96]).expect("dh init");
        let mut server = ZeroConfServer::new();
        let uri = "/?action=addUser&userName=alice";
        let res = server.handle_request(uri, "", &dh, "dev1", "living");
        assert_eq!(res.err(), Some(ZeroConfError::NotFound));
    }
}
