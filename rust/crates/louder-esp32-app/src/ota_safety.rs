//! Pure-Rust Anti-Brick OTA Safety, NVS Wi-Fi Credential Decryptor, and `ota_header_check` Verifier.
//!
//! Enforces the 4MB flash invariants for target `172.22.1.15` (`"BASEMENT"`):
//! - `0x9000` (`0x4000`): `nvs` partition storing `"config"` -> `"ssid"` / `"password"` (`"enc:"` XOR)
//! - `0xD000` (`0x2000`): `otadata` partition (`0xFF` = boot `recovery` factory partition at `0x10000`)
//! - `0x10000` (`0x150000`): `recovery` (`factory` partition — NEVER modified by OTA)
//! - `0x160000` (`0x290000`): `ota_0` partition (target slot for `louder-esp32-app`)
//! - `0x3F0000` (`0x10000`): `settings` partition (strictly read-only from Rust)

use heapless::String;

/// Offset of the default `nvs` partition in 4MB flash (`partitions.csv`).
pub const NVS_PARTITION_OFFSET: u32 = 0x0000_9000;
/// Size of the default `nvs` partition in bytes (16 KB = 4 pages of 4096 bytes).
pub const NVS_PARTITION_SIZE: u32 = 0x0000_4000;

/// Offset of the `otadata` partition in 4MB flash (`partitions.csv`).
pub const OTADATA_PARTITION_OFFSET: u32 = 0x0000_D000;
/// Offset of the second 4 KB sector of `otadata`.
pub const OTADATA_SECTOR_1_OFFSET: u32 = 0x0000_E000;
/// End offset (exclusive) of the `otadata` partition (`0xD000 + 0x2000 = 0xF000`).
pub const OTADATA_PARTITION_END: u32 = 0x0000_F000;
/// Size of the `otadata` partition in bytes (8 KB = 2 sectors of 4096 bytes).
pub const OTADATA_PARTITION_SIZE: u32 = 0x0000_2000;

/// Offset of the untouched `recovery` (`factory`) app partition (`partitions.csv`).
pub const RECOVERY_PARTITION_OFFSET: u32 = 0x0001_0000;
/// Size of the `recovery` (`factory`) app partition in bytes (1,376 KB).
pub const RECOVERY_PARTITION_SIZE: u32 = 0x0015_0000;

/// Offset of the `ota_0` app partition (`partitions.csv`).
pub const OTA_0_PARTITION_OFFSET: u32 = 0x0016_0000;
/// Maximum size of the `ota_0` app partition in bytes (2,624 KB = 2,686,976 bytes).
pub const OTA_0_PARTITION_SIZE: u32 = 0x0029_0000;

/// Offset of the `settings` partition (`partitions.csv`, strictly read-only).
pub const SETTINGS_PARTITION_OFFSET: u32 = 0x003F_0000;
/// Size of the `settings` partition in bytes (64 KB).
pub const SETTINGS_PARTITION_SIZE: u32 = 0x0001_0000;

/// Fixed 16-byte domain salt from `components/wifi-manager/network_wifi.c:55-58`.
pub const WIFI_NVS_SALT: [u8; 16] = *b"WiFiNVSKey$Secur";

/// Prefix indicating an encrypted Wi-Fi password in NVS (`network_wifi.c:51`).
pub const WIFI_ENC_PREFIX: &str = "enc:";

/// Required `esp_image_header_t.magic` byte at offset `0x00` (`ESP_IMAGE_HEADER_MAGIC`).
pub const ESP_IMAGE_MAGIC_BYTE: u8 = 0xE9;

/// Required `esp_app_desc_t.magic_word` at offset `0x20` (`squeezelite-ota.c:650`).
pub const ESP_APP_DESC_MAGIC_WORD: u32 = 0xABCD_5432;

/// Required `esp_app_desc_t.project_name` at offset `0x50` (`application_name.h:5`).
pub const REQUIRED_OTA_PROJECT_NAME: &str = "Squeezelite-ESP32";

/// Rust OTA firmware version string embedded in `esp_app_desc_t.version` at offset `0x30`.
pub const RUST_OTA_FIRMWARE_VERSION: &str = "I2S-4MFlash-Rust";

const NVS_PAGE_SIZE: usize = 4096;
const NVS_ENTRY_OFFSET: usize = 64;
const NVS_ENTRY_SIZE: usize = 32;
const NVS_ENTRIES_PER_PAGE: usize = 126;

const NVS_PAGE_STATE_ACTIVE: u32 = 0xFFFF_FFFE;
const NVS_PAGE_STATE_FULL: u32 = 0xFFFF_FFFC;

const NVS_TYPE_U8: u8 = 0x01;
const NVS_TYPE_STR: u8 = 0x21;
const NVS_TYPE_BLOB: u8 = 0x41;
const NVS_TYPE_BLOB_DATA: u8 = 0x42;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Errors returned by NVS credential extraction or OTA safety checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtaSafetyError {
    /// Ciphertext after `"enc:"` has an odd number of hex digits or exceeds buffer capacity.
    InvalidHexLength(usize),
    /// Ciphertext contains a non-hexadecimal character.
    InvalidHexCharacter,
    /// Decrypted or plaintext credential is not valid UTF-8 or exceeds `heapless::String` capacity.
    CredentialStringError,
    /// NVS partition did not contain a valid `"config"` or `"aplist"` Wi-Fi credential entry.
    CredentialsNotFound,
    /// `otadata` sector read-back after erase did not return all `0xFF` bytes.
    OtadataNotErased(usize),
    /// Image failed `squeezelite-ota.c` `ota_header_check()` validation.
    InvalidBinaryHeader(&'static str),
}

/// Decrypted Wi-Fi Station credentials loaded from NVS (`0x9000`) or compile-time fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiStaCredentials {
    /// Wi-Fi SSID (up to 32 bytes).
    pub ssid: String<32>,
    /// Decrypted Wi-Fi WPA/WPA2 passphrase (up to 64 bytes).
    pub password: String<64>,
}

/// Summary of a validated ESP32 OTA `.bin` image header (`esp_image_header_t` + `esp_app_desc_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaBinarySummary {
    /// Total image length in bytes.
    pub image_size: usize,
    /// `esp_app_desc_t.magic_word` (`0xABCD5432`).
    pub app_desc_magic: u32,
    /// `esp_app_desc_t.version` string (`0x30..0x50`).
    pub version: String<32>,
    /// `esp_app_desc_t.project_name` string (`0x50..0x70`).
    pub project_name: String<32>,
}

/// ESP-IDF `esp_ota_select_entry_t` (`32` bytes) stored at `0xD000` and `0xE000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaSelectEntry {
    /// Monotonic sequence number (`1` selects `ota_0` when `ota_count == 1`).
    pub ota_seq: u32,
    /// Label / reserved 20 bytes (`0xFF` in standard ESP-IDF).
    pub seq_label: [u8; 20],
    /// `esp_ota_img_states_t` (`0x0000_0002` = `ESP_OTA_IMG_VALID`).
    pub ota_state: u32,
    /// CRC32 of `ota_seq` (4 bytes little-endian) with initial value `0xFFFF_FFFF`.
    pub crc: u32,
}

impl OtaSelectEntry {
    /// `ESP_OTA_IMG_VALID` state constant.
    pub const OTA_IMG_VALID: u32 = 0x0000_0002;

    /// Constructs a valid `OtaSelectEntry` selecting `ota_0` (`ota_seq = 1`).
    #[must_use]
    pub fn new_ota_0() -> Self {
        let ota_seq = 1u32;
        let seq_bytes = ota_seq.to_le_bytes();
        let crc = esp_crc32_le(0xFFFF_FFFF, &seq_bytes);
        Self {
            ota_seq,
            seq_label: [0xFF; 20],
            ota_state: Self::OTA_IMG_VALID,
            crc,
        }
    }

    /// Serializes the entry into its exact 32-byte little-endian flash representation.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0xFFu8; 32];
        out[0..4].copy_from_slice(&self.ota_seq.to_le_bytes());
        out[4..24].copy_from_slice(&self.seq_label);
        out[24..28].copy_from_slice(&self.ota_state.to_le_bytes());
        out[28..32].copy_from_slice(&self.crc.to_le_bytes());
        out
    }
}

/// Computes the ESP-IDF ROM `esp_crc32_le(crc, buf)` used by `esp-bootloader-esp-idf` for `otadata`.
#[must_use]
pub fn esp_crc32_le(initial_crc: u32, buf: &[u8]) -> u32 {
    let mut crc = !initial_crc;
    for &byte in buf {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Pure-Rust FIPS 180-4 SHA-256 implementation for single-block messages (`len <= 55` bytes).
#[must_use]
pub fn sha256_short(data: &[u8]) -> [u8; 32] {
    assert!(data.len() <= 55, "sha256_short only supports messages up to 55 bytes");
    let mut block = [0u8; 64];
    block[..data.len()].copy_from_slice(data);
    block[data.len()] = 0x80;
    let bit_len = (data.len() as u64) * 8;
    block[56..64].copy_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Verifies that a read-back slice from `otadata` (`0xD000` and `0xE000`) is completely `0xFF`,
/// proving the ESP-IDF 2nd-stage bootloader will boot `Factory` (`recovery` at `0x10000`) on reset.
pub fn verify_otadata_erased(bytes: &[u8]) -> Result<(), OtaSafetyError> {
    for (idx, &b) in bytes.iter().enumerate() {
        if b != 0xFF {
            return Err(OtaSafetyError::OtadataNotErased(idx));
        }
    }
    Ok(())
}

/// Derives the 16-byte XOR keystream from `WIFI_NVS_SALT` and the 6-byte Wi-Fi STA MAC address,
/// matching `derive_key_from_mac()` in `components/wifi-manager/network_wifi.c:53-70`.
#[must_use]
pub fn derive_wifi_nvs_key(sta_mac: &[u8; 6]) -> [u8; 16] {
    let mut combined = [0u8; 22];
    combined[0..16].copy_from_slice(&WIFI_NVS_SALT);
    combined[16..22].copy_from_slice(sta_mac);

    let digest = sha256_short(&combined);
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest[0..16]);
    key
}

fn decode_hex_nibble(b: u8) -> Result<u8, OtaSafetyError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(OtaSafetyError::InvalidHexCharacter),
    }
}

fn heapless_string_from_str<const N: usize>(s: &str) -> Result<String<N>, OtaSafetyError> {
    let mut out = String::<N>::new();
    out.push_str(s)
        .map_err(|()| OtaSafetyError::CredentialStringError)?;
    Ok(out)
}

/// Decrypts a Wi-Fi password value from NVS (`network_wifi.c:94-122`).
///
/// If `raw_value` starts with `"enc:"`, hex-decodes the payload and XORs each byte with
/// `derive_wifi_nvs_key(sta_mac)[i % 16]`. Otherwise, returns `raw_value` as plaintext.
pub fn decrypt_nvs_wifi_password(
    raw_value: &str,
    sta_mac: &[u8; 6],
) -> Result<String<64>, OtaSafetyError> {
    let trimmed = raw_value.trim_end_matches('\0');
    if let Some(hex_part) = trimmed.strip_prefix(WIFI_ENC_PREFIX) {
        let hex_bytes = hex_part.as_bytes();
        if hex_bytes.is_empty() || (hex_bytes.len() % 2 != 0) || (hex_bytes.len() / 2 > 64) {
            return Err(OtaSafetyError::InvalidHexLength(hex_bytes.len()));
        }
        let key = derive_wifi_nvs_key(sta_mac);
        let mut out_bytes = [0u8; 64];
        let out_len = hex_bytes.len() / 2;
        for i in 0..out_len {
            let hi = decode_hex_nibble(hex_bytes[i * 2])?;
            let lo = decode_hex_nibble(hex_bytes[i * 2 + 1])?;
            let cipher_byte = (hi << 4) | lo;
            out_bytes[i] = cipher_byte ^ key[i % 16];
        }
        let plain_slice = &out_bytes[..out_len];
        let plain_trimmed = match plain_slice.iter().position(|&b| b == 0) {
            Some(nul) => &plain_slice[..nul],
            None => plain_slice,
        };
        let plain_str = core::str::from_utf8(plain_trimmed)
            .map_err(|_| OtaSafetyError::CredentialStringError)?;
        heapless_string_from_str(plain_str)
    } else {
        heapless_string_from_str(trimmed)
    }
}

/// Checks whether entry `entry_idx` (`0..126`) in an NVS page is in the `WRITTEN` state (`2b10`).
fn is_nvs_entry_written(bitmap: &[u8], entry_idx: usize) -> bool {
    let byte_idx = entry_idx / 4;
    let bit_shift = (entry_idx % 4) * 2;
    if byte_idx >= bitmap.len() {
        return false;
    }
    let state = (bitmap[byte_idx] >> bit_shift) & 0b11;
    state == 0b10
}

fn parse_nvs_entry_key(entry: &[u8]) -> &str {
    let key_raw = &entry[8..24];
    let key_len = key_raw.iter().position(|&b| b == 0).unwrap_or(16);
    core::str::from_utf8(&key_raw[..key_len]).unwrap_or("")
}

/// Extracts a string or blob payload from a written NVS entry starting at `entry_idx`.
fn extract_nvs_entry_value<'a>(
    page: &'a [u8],
    entry_idx: usize,
    span: usize,
    data_type: u8,
) -> Option<&'a str> {
    if span < 2 || entry_idx + span > NVS_ENTRIES_PER_PAGE {
        return None;
    }
    let header_offset = NVS_ENTRY_OFFSET + entry_idx * NVS_ENTRY_SIZE;
    let size = u16::from_le_bytes([page[header_offset + 24], page[header_offset + 25]]) as usize;
    let data_start = NVS_ENTRY_OFFSET + (entry_idx + 1) * NVS_ENTRY_SIZE;
    let max_data_bytes = (span - 1) * NVS_ENTRY_SIZE;
    if size > max_data_bytes || data_start + size > page.len() {
        return None;
    }
    if data_type != NVS_TYPE_STR && data_type != NVS_TYPE_BLOB && data_type != NVS_TYPE_BLOB_DATA {
        return None;
    }
    let raw = &page[data_start..data_start + size];
    let trimmed_len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    core::str::from_utf8(&raw[..trimmed_len]).ok()
}

/// Scans a raw `nvs` partition buffer (`0x9000..0xD000`, multiple of 4096 bytes) for the
/// `"config"` namespace (`"ssid"` and `"password"` keys) or `"aplist"` fallback, and decrypts
/// the password using `sta_mac`.
pub fn parse_nvs_wifi_credentials(
    nvs_bytes: &[u8],
    sta_mac: &[u8; 6],
) -> Result<WifiStaCredentials, OtaSafetyError> {
    let mut config_ns_id: Option<u8> = None;
    let mut aplist_ns_id: Option<u8> = None;

    for page in nvs_bytes.chunks_exact(NVS_PAGE_SIZE) {
        let state = u32::from_le_bytes([page[0], page[1], page[2], page[3]]);
        if state != NVS_PAGE_STATE_ACTIVE && state != NVS_PAGE_STATE_FULL {
            continue;
        }
        let bitmap = &page[32..64];
        let mut idx = 0usize;
        while idx < NVS_ENTRIES_PER_PAGE {
            let entry_off = NVS_ENTRY_OFFSET + idx * NVS_ENTRY_SIZE;
            let entry = &page[entry_off..entry_off + NVS_ENTRY_SIZE];
            let span = usize::from(entry[2]).max(1);
            if is_nvs_entry_written(bitmap, idx) && entry[0] == 0 && entry[1] == NVS_TYPE_U8 {
                let key = parse_nvs_entry_key(entry);
                let ns_id = entry[24];
                if key == "config" {
                    config_ns_id = Some(ns_id);
                } else if key == "aplist" {
                    aplist_ns_id = Some(ns_id);
                }
            }
            idx += span;
        }
    }

    let mut latest_ssid: Option<&str> = None;
    let mut latest_password: Option<&str> = None;
    let mut latest_aplist_json: Option<&str> = None;

    for page in nvs_bytes.chunks_exact(NVS_PAGE_SIZE) {
        let state = u32::from_le_bytes([page[0], page[1], page[2], page[3]]);
        if state != NVS_PAGE_STATE_ACTIVE && state != NVS_PAGE_STATE_FULL {
            continue;
        }
        let bitmap = &page[32..64];
        let mut idx = 0usize;
        while idx < NVS_ENTRIES_PER_PAGE {
            let entry_off = NVS_ENTRY_OFFSET + idx * NVS_ENTRY_SIZE;
            let entry = &page[entry_off..entry_off + NVS_ENTRY_SIZE];
            let span = usize::from(entry[2]).max(1);
            if is_nvs_entry_written(bitmap, idx) {
                let ns = entry[0];
                let dtype = entry[1];
                let key = parse_nvs_entry_key(entry);
                if Some(ns) == config_ns_id {
                    if key == "ssid" {
                        if let Some(val) = extract_nvs_entry_value(page, idx, span, dtype) {
                            if !val.is_empty() {
                                latest_ssid = Some(val);
                            }
                        }
                    } else if key == "password" {
                        if let Some(val) = extract_nvs_entry_value(page, idx, span, dtype) {
                            latest_password = Some(val);
                        }
                    }
                } else if Some(ns) == aplist_ns_id {
                    if let Some(val) = extract_nvs_entry_value(page, idx, span, dtype) {
                        if val.contains("\"ssid\"") && val.contains("\"pass\"") {
                            latest_aplist_json = Some(val);
                        }
                    }
                }
            }
            idx += span;
        }
    }

    if let (Some(raw_ssid), Some(raw_pass)) = (latest_ssid, latest_password) {
        let ssid = heapless_string_from_str(raw_ssid.trim_end_matches('\0'))?;
        let password = decrypt_nvs_wifi_password(raw_pass, sta_mac)?;
        return Ok(WifiStaCredentials { ssid, password });
    }

    if let Some(json) = latest_aplist_json {
        if let (Some(raw_ssid), Some(raw_pass)) = (
            extract_json_string_field(json, "\"ssid\":\""),
            extract_json_string_field(json, "\"pass\":\""),
        ) {
            let ssid = heapless_string_from_str(raw_ssid)?;
            let password = decrypt_nvs_wifi_password(raw_pass, sta_mac)?;
            return Ok(WifiStaCredentials { ssid, password });
        }
    }

    Err(OtaSafetyError::CredentialsNotFound)
}

fn extract_json_string_field<'a>(json: &'a str, marker: &str) -> Option<&'a str> {
    let start = json.find(marker)? + marker.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Resolves Wi-Fi STA credentials using NVS partition (`0x9000`) extraction or compile-time
/// override (`SL_WIFI_PASS` if non-empty).
pub fn resolve_wifi_credentials(
    nvs_bytes: &[u8],
    sta_mac: &[u8; 6],
    compile_time_ssid: Option<&str>,
    compile_time_pass: Option<&str>,
) -> Result<WifiStaCredentials, OtaSafetyError> {
    if let Ok(creds) = parse_nvs_wifi_credentials(nvs_bytes, sta_mac) {
        return Ok(creds);
    }
    if let (Some(ssid_str), Some(pass_str)) = (compile_time_ssid, compile_time_pass) {
        if !ssid_str.is_empty() && !pass_str.is_empty() {
            let ssid = heapless_string_from_str(ssid_str)?;
            let password = decrypt_nvs_wifi_password(pass_str, sta_mac)?;
            return Ok(WifiStaCredentials { ssid, password });
        }
    }
    Err(OtaSafetyError::CredentialsNotFound)
}

/// Validates an ESP32 firmware `.bin` image against the exact checks performed by
/// `ota_header_check()` in `components/squeezelite-ota/squeezelite-ota.c:633-669`.
pub fn verify_ota_binary_header(image: &[u8]) -> Result<OtaBinarySummary, OtaSafetyError> {
    if image.len() < 0x120 {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "image shorter than 288 bytes (esp_image_header + segment + esp_app_desc_t)",
        ));
    }
    if image.len() > OTA_0_PARTITION_SIZE as usize {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "image exceeds ota_0 partition capacity (0x290000 bytes)",
        ));
    }
    if image[0] != ESP_IMAGE_MAGIC_BYTE {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "byte 0x00 is not ESP_IMAGE_HEADER_MAGIC (0xE9)",
        ));
    }
    let app_desc_magic = u32::from_le_bytes([image[0x20], image[0x21], image[0x22], image[0x23]]);
    if app_desc_magic != ESP_APP_DESC_MAGIC_WORD {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "offset 0x20 does not contain esp_app_desc_t magic 0xABCD5432",
        ));
    }

    let version_raw = &image[0x30..0x50];
    let version_len = version_raw.iter().position(|&b| b == 0).unwrap_or(32);
    let version_str = core::str::from_utf8(&version_raw[..version_len])
        .map_err(|_| OtaSafetyError::InvalidBinaryHeader("version field is not valid UTF-8"))?;
    if version_str.is_empty() {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "esp_app_desc_t.version is empty",
        ));
    }

    let proj_raw = &image[0x50..0x70];
    let proj_len = proj_raw.iter().position(|&b| b == 0).unwrap_or(32);
    let proj_str = core::str::from_utf8(&proj_raw[..proj_len]).map_err(|_| {
        OtaSafetyError::InvalidBinaryHeader("project_name field is not valid UTF-8")
    })?;
    if proj_str != REQUIRED_OTA_PROJECT_NAME {
        return Err(OtaSafetyError::InvalidBinaryHeader(
            "project_name mismatch: must be 'Squeezelite-ESP32' for recovery ota_header_check()",
        ));
    }

    let version = heapless_string_from_str(version_str)
        .map_err(|_| OtaSafetyError::InvalidBinaryHeader("version exceeds 32 chars"))?;
    let project_name = heapless_string_from_str(proj_str)
        .map_err(|_| OtaSafetyError::InvalidBinaryHeader("project_name exceeds 32 chars"))?;

    Ok(OtaBinarySummary {
        image_size: image.len(),
        app_desc_magic,
        version,
        project_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    const BASEMENT_MAC: [u8; 6] = [0xC8, 0x2E, 0x18, 0xBE, 0xB8, 0x34];

    fn encrypt_for_test(plaintext: &str, sta_mac: &[u8; 6]) -> String<128> {
        let key = derive_wifi_nvs_key(sta_mac);
        let mut out: String<128> = String::new();
        let _ = out.push_str("enc:");
        for (i, &b) in plaintext.as_bytes().iter().enumerate() {
            let cipher = b ^ key[i % 16];
            let _ = write!(out, "{:02x}", cipher);
        }
        out
    }

    fn write_nvs_entry_state(page: &mut [u8], entry_idx: usize, state: u8) {
        let byte_idx = 32 + (entry_idx / 4);
        let shift = (entry_idx % 4) * 2;
        page[byte_idx] &= !(0b11 << shift);
        page[byte_idx] |= (state & 0b11) << shift;
    }

    fn write_nvs_namespace_entry(page: &mut [u8], entry_idx: usize, name: &str, ns_id: u8) {
        write_nvs_entry_state(page, entry_idx, 0b10);
        let off = NVS_ENTRY_OFFSET + entry_idx * NVS_ENTRY_SIZE;
        page[off..off + NVS_ENTRY_SIZE].fill(0);
        page[off] = 0;
        page[off + 1] = NVS_TYPE_U8;
        page[off + 2] = 1;
        page[off + 3] = 0xFF;
        let name_bytes = name.as_bytes();
        page[off + 8..off + 8 + name_bytes.len()].copy_from_slice(name_bytes);
        page[off + 24] = ns_id;
    }

    fn write_nvs_blob_entry(
        page: &mut [u8],
        entry_idx: usize,
        ns_id: u8,
        data_type: u8,
        key: &str,
        payload: &[u8],
    ) -> usize {
        let data_slots = payload.len().div_ceil(NVS_ENTRY_SIZE).max(1);
        let span = 1 + data_slots;
        for s in 0..span {
            write_nvs_entry_state(page, entry_idx + s, 0b10);
        }
        let off = NVS_ENTRY_OFFSET + entry_idx * NVS_ENTRY_SIZE;
        page[off..off + span * NVS_ENTRY_SIZE].fill(0);
        page[off] = ns_id;
        page[off + 1] = data_type;
        page[off + 2] = span as u8;
        page[off + 3] = 0xFF;
        let key_bytes = key.as_bytes();
        page[off + 8..off + 8 + key_bytes.len()].copy_from_slice(key_bytes);
        let size_le = (payload.len() as u16).to_le_bytes();
        page[off + 24] = size_le[0];
        page[off + 25] = size_le[1];
        let data_off = NVS_ENTRY_OFFSET + (entry_idx + 1) * NVS_ENTRY_SIZE;
        page[data_off..data_off + payload.len()].copy_from_slice(payload);
        span
    }

    #[test]
    fn sha256_short_matches_fips_180_4_test_vectors() {
        let empty = sha256_short(b"");
        assert_eq!(
            empty,
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
                0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
                0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
                0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55
            ]
        );
        let abc = sha256_short(b"abc");
        assert_eq!(
            abc,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
                0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
                0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
                0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad
            ]
        );
    }

    #[test]
    fn decrypts_encrypted_and_plaintext_nvs_passwords_with_basement_mac() {
        let secret = "MyBasementIoTPass!2026";
        let encrypted = encrypt_for_test(secret, &BASEMENT_MAC);
        assert!(encrypted.as_str().starts_with("enc:"));

        let decrypted = decrypt_nvs_wifi_password(encrypted.as_str(), &BASEMENT_MAC).unwrap();
        assert_eq!(decrypted.as_str(), secret);

        let plain = decrypt_nvs_wifi_password("PlainPass123\0", &BASEMENT_MAC).unwrap();
        assert_eq!(plain.as_str(), "PlainPass123");
    }

    #[test]
    fn parses_wifi_credentials_from_synthetic_nvs_config_namespace() {
        let mut nvs = [0xFFu8; NVS_PAGE_SIZE * 2];
        nvs[0..4].copy_from_slice(&NVS_PAGE_STATE_ACTIVE.to_le_bytes());

        write_nvs_namespace_entry(&mut nvs[0..NVS_PAGE_SIZE], 0, "config", 1);
        let span1 = write_nvs_blob_entry(
            &mut nvs[0..NVS_PAGE_SIZE],
            1,
            1,
            NVS_TYPE_BLOB,
            "ssid",
            b"iot\0",
        );

        let enc_pass = encrypt_for_test("SqueezeRustWiFi99", &BASEMENT_MAC);
        let _span2 = write_nvs_blob_entry(
            &mut nvs[0..NVS_PAGE_SIZE],
            1 + span1,
            1,
            NVS_TYPE_BLOB,
            "password",
            enc_pass.as_bytes(),
        );

        let creds = parse_nvs_wifi_credentials(&nvs, &BASEMENT_MAC).unwrap();
        assert_eq!(creds.ssid.as_str(), "iot");
        assert_eq!(creds.password.as_str(), "SqueezeRustWiFi99");
    }

    #[test]
    fn parses_wifi_credentials_from_aplist_json_fallback() {
        let mut nvs = [0xFFu8; NVS_PAGE_SIZE];
        nvs[0..4].copy_from_slice(&NVS_PAGE_STATE_FULL.to_le_bytes());

        write_nvs_namespace_entry(&mut nvs, 0, "aplist", 2);
        let enc_pass = encrypt_for_test("FallbackPass42", &BASEMENT_MAC);
        let mut json: String<192> = String::new();
        let _ = write!(
            json,
            "{{\"ssid\":\"iot\",\"pass\":\"{}\",\"chan\":6}}",
            enc_pass.as_str()
        );
        write_nvs_blob_entry(&mut nvs, 1, 2, NVS_TYPE_BLOB, "iot", json.as_bytes());

        let creds = parse_nvs_wifi_credentials(&nvs, &BASEMENT_MAC).unwrap();
        assert_eq!(creds.ssid.as_str(), "iot");
        assert_eq!(creds.password.as_str(), "FallbackPass42");
    }

    #[test]
    fn verifies_otadata_erase_and_ota_0_select_entry_crc() {
        let erased = [0xFFu8; 64];
        assert!(verify_otadata_erased(&erased).is_ok());

        let mut dirty = [0xFFu8; 64];
        dirty[17] = 0x01;
        assert_eq!(
            verify_otadata_erased(&dirty),
            Err(OtaSafetyError::OtadataNotErased(17))
        );

        let entry = OtaSelectEntry::new_ota_0();
        let raw = entry.to_bytes();
        assert_eq!(&raw[0..4], &1u32.to_le_bytes());
        assert_eq!(&raw[24..28], &OtaSelectEntry::OTA_IMG_VALID.to_le_bytes());
        let expected_crc = esp_crc32_le(0xFFFF_FFFF, &1u32.to_le_bytes());
        assert_eq!(&raw[28..32], &expected_crc.to_le_bytes());
    }

    #[test]
    fn validates_esp_app_desc_header_for_squeezelite_ota_check() {
        let mut bin = [0u8; 0x140];
        bin[0x00] = ESP_IMAGE_MAGIC_BYTE;
        bin[0x20..0x24].copy_from_slice(&ESP_APP_DESC_MAGIC_WORD.to_le_bytes());
        bin[0x30..0x30 + RUST_OTA_FIRMWARE_VERSION.len()]
            .copy_from_slice(RUST_OTA_FIRMWARE_VERSION.as_bytes());
        bin[0x50..0x50 + REQUIRED_OTA_PROJECT_NAME.len()]
            .copy_from_slice(REQUIRED_OTA_PROJECT_NAME.as_bytes());

        let summary = verify_ota_binary_header(&bin).unwrap();
        assert_eq!(summary.project_name.as_str(), "Squeezelite-ESP32");
        assert_eq!(summary.version.as_str(), "I2S-4MFlash-Rust");

        bin[0x50..0x70].fill(0);
        bin[0x50..0x50 + b"louder-esp32-app".len()].copy_from_slice(b"louder-esp32-app");
        assert!(matches!(
            verify_ota_binary_header(&bin),
            Err(OtaSafetyError::InvalidBinaryHeader(_))
        ));
    }
}
