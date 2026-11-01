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
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

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

    fn append_a_record(
        records: &mut Vec<u8>,
        host_name: &str,
        ip: [u8; 4],
        ttl: u32,
    ) {
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

        Self::append_ptr_record(
            &mut packet,
            SpotifyMdnsConfig::SERVICE_TYPE,
            &instance,
            ttl,
        );

        Self::append_srv_record(
            &mut packet,
            &instance,
            config.port,
            &host,
            ttl,
        );

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
    Ok(((lo & 0x7f) as u32) | ((hi as u32) << 7))
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            let hex_str = format!("{}{}", h1, h2);
            if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                out.push(byte as char);
            }
        } else if ch == '+' {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
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
pub struct ZeroConfServer;

impl ZeroConfServer {
    /// Builds JSON response for `action=getInfo`.
    pub fn build_get_info_json(
        device_id: &str,
        remote_name: &str,
        public_key_b64: &str,
    ) -> String {
        format!(
            "{{\"status\":101,\"statusString\":\"OK\",\"spotifyError\":0,\
\"version\":\"2.7.1\",\"libraryVersion\":\"1.0.0\",\"accountReq\":\"PREMIUM\",\
\"brandDisplayName\":\"cspot\",\"modelDisplayName\":\"{}\",\"voiceSupport\":\"NO\",\
\"availability\":\"\",\"productID\":0,\"tokenType\":\"default\",\"groupStatus\":\"NONE\",\
\"resolverVersion\":\"0\",\"scope\":\"streaming,client-authorization-universal\",\
\"activeUser\":\"\",\"deviceID\":\"{}\",\"remoteName\":\"{}\",\
\"publicKey\":\"{}\",\"deviceType\":\"AudioDongle\"}}",
            remote_name, device_id, remote_name, public_key_b64
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

        let iv: [u8; 16] = blob[0..16].try_into().map_err(|_| CryptoError::InvalidCiphertext)?;
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
        if mac != checksum {
            return Err(CryptoError::DecryptionFailed);
        }

        let mut ctr = Aes128Ctr::new(&enc_key, iv);
        ctr.process(&mut encrypted);

        // Secondary blob decode
        let enc_str = core::str::from_utf8(&encrypted).map_err(|_| CryptoError::DecryptionFailed)?;
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
        pos += skip_len;
        pos += 1;
        let auth_type = read_blob_int(&blob_data, &mut pos)?;
        pos += 1;
        let auth_size = read_blob_int(&blob_data, &mut pos)? as usize;

        if pos + auth_size > blob_data.len() {
            return Err(CryptoError::DecryptionFailed);
        }

        let auth_data = blob_data[pos..pos + auth_size].to_vec();
        Ok((auth_type, auth_data))
    }

    /// Handles an incoming ZeroConf HTTP request (`getInfo` or `addUser`).
    pub fn handle_request(
        uri: &str,
        body: &str,
        dh: &OakleyGroup1Dh,
        device_id: &str,
        remote_name: &str,
    ) -> Result<ZeroConfResponse, ZeroConfError> {
        let combined_params = if let Some(idx) = uri.find('?') {
            format!("{}&{}", &uri[idx + 1..], body)
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
                let username = get_param(&combined_params, "userName")
                    .ok_or(ZeroConfError::MissingParameter)?;
                let blob_b64 = get_param(&combined_params, "blob")
                    .ok_or(ZeroConfError::MissingParameter)?;
                let client_key_b64 = get_param(&combined_params, "clientKey")
                    .ok_or(ZeroConfError::MissingParameter)?;
                let dev_name = get_param(&combined_params, "deviceName")
                    .unwrap_or_else(|| "Spotify App".to_string());

                let blob_bytes = base64_decode(&blob_b64)?;
                let client_key_bytes = base64_decode(&client_key_b64)?;

                let shared_key = dh.calculate_shared(&client_key_bytes)?;
                let (_auth_type, _auth_data) = Self::decode_blob(
                    &blob_bytes,
                    &shared_key,
                    device_id,
                    &username,
                )?;

                let resp_json = format!(
                    "{{\"status\":101,\"statusString\":\"OK\",\"spotifyError\":0,\
\"userName\":\"{}\",\"deviceName\":\"{}\"}}",
                    username, dev_name
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
