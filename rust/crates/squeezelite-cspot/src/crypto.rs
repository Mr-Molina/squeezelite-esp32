//! Cryptographic primitives for Spotify Connect (cspot).
//!
//! Includes Oakley Group 1 Diffie-Hellman (RFC 2409), pure-Rust SHA-1,
//! HMAC-SHA1, PBKDF2-HMAC-SHA1, AES-128-CTR, AES-192-ECB, and Base64.

#![allow(
    unknown_lints,
    clippy::chunks_exact_to_as_chunks,
    clippy::manual_div_ceil
)]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

/// Cryptographic processing error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoError {
    /// Provided key length is invalid.
    InvalidKeyLength,
    /// Ciphertext or message format is invalid.
    InvalidCiphertext,
    /// Base64 decoding failed.
    InvalidBase64,
    /// Authentication checksum mismatch or corrupted blob.
    DecryptionFailed,
    /// Modulus for modular exponentiation is even or zero.
    InvalidModulus,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKeyLength => write!(f, "invalid key length"),
            Self::InvalidCiphertext => write!(f, "invalid ciphertext format"),
            Self::InvalidBase64 => write!(f, "invalid base64 encoding"),
            Self::DecryptionFailed => write!(f, "decryption or MAC verification failed"),
            Self::InvalidModulus => write!(f, "invalid modulus for exponentiation"),
        }
    }
}

/// Unsigned arbitrary-precision integer for DH arithmetic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    /// Creates a BigUint with value zero.
    pub fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    /// Creates a BigUint with value one.
    pub fn one() -> Self {
        Self { limbs: vec![1] }
    }

    /// Creates a BigUint from a single `u32` value.
    pub fn from_u32(val: u32) -> Self {
        if val == 0 {
            Self::zero()
        } else {
            Self { limbs: vec![val] }
        }
    }

    fn normalize(&mut self) {
        while let Some(&0) = self.limbs.last() {
            self.limbs.pop();
        }
    }

    /// Returns true if value is zero.
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Returns true if value is odd.
    pub fn is_odd(&self) -> bool {
        self.limbs.first().is_some_and(|&l| (l & 1) != 0)
    }

    /// Creates a BigUint from big-endian bytes.
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::zero();
        }
        let mut limbs = Vec::new();
        let mut chunk = 0u32;
        let mut shift = 0;
        for &b in bytes.iter().rev() {
            chunk |= (b as u32) << shift;
            shift += 8;
            if shift == 32 {
                limbs.push(chunk);
                chunk = 0;
                shift = 0;
            }
        }
        if shift > 0 {
            limbs.push(chunk);
        }
        let mut res = Self { limbs };
        res.normalize();
        res
    }

    /// Exports BigUint to big-endian bytes.
    pub fn to_be_bytes(&self) -> Vec<u8> {
        if self.is_zero() {
            return vec![0];
        }
        let mut bytes = Vec::new();
        for &limb in self.limbs.iter().rev() {
            bytes.extend_from_slice(&limb.to_be_bytes());
        }
        let first_non_zero = bytes
            .iter()
            .position(|&b| b != 0)
            .unwrap_or(bytes.len() - 1);
        bytes[first_non_zero..].to_vec()
    }

    /// Exports BigUint to fixed-length big-endian bytes with leading zeros.
    pub fn to_be_bytes_padded(&self, len: usize) -> Result<Vec<u8>, CryptoError> {
        let raw = self.to_be_bytes();
        if raw.len() > len {
            return Err(CryptoError::InvalidCiphertext);
        }
        let mut padded = vec![0u8; len];
        let offset = len - raw.len();
        padded[offset..].copy_from_slice(&raw);
        Ok(padded)
    }

    /// Returns bit length of BigUint.
    pub fn bit_len(&self) -> usize {
        if self.is_zero() {
            return 0;
        }
        let last_idx = self.limbs.len() - 1;
        let last_limb = self.limbs[last_idx];
        let bits_in_last = 32 - last_limb.leading_zeros() as usize;
        last_idx * 32 + bits_in_last
    }

    /// Adds another BigUint.
    pub fn add(&self, other: &Self) -> Self {
        let max_len = core::cmp::max(self.limbs.len(), other.limbs.len());
        let mut result = Vec::with_capacity(max_len + 1);
        let mut carry = 0u64;

        for i in 0..max_len {
            let a = self.limbs.get(i).copied().unwrap_or(0) as u64;
            let b = other.limbs.get(i).copied().unwrap_or(0) as u64;
            let sum = a + b + carry;
            result.push(sum as u32);
            carry = sum >> 32;
        }
        if carry > 0 {
            result.push(carry as u32);
        }
        let mut res = Self { limbs: result };
        res.normalize();
        res
    }

    /// Subtracts another BigUint, clamping to zero on underflow.
    pub fn sub(&self, other: &Self) -> Self {
        let mut result = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0i64;

        for i in 0..self.limbs.len() {
            let a = self.limbs[i] as i64;
            let b = other.limbs.get(i).copied().unwrap_or(0) as i64;
            let diff = a - b - borrow;
            if diff < 0 {
                result.push((diff + (1i64 << 32)) as u32);
                borrow = 1;
            } else {
                result.push(diff as u32);
                borrow = 0;
            }
        }
        if borrow != 0 {
            return Self::zero();
        }
        let mut res = Self { limbs: result };
        res.normalize();
        res
    }

    /// Multiplies with another BigUint.
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let mut result = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let cur = result[i + j] as u64 + (a as u64) * (b as u64) + carry;
                result[i + j] = cur as u32;
                carry = cur >> 32;
            }
            if carry > 0 {
                result[i + other.limbs.len()] += carry as u32;
            }
        }
        let mut res = Self { limbs: result };
        res.normalize();
        res
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            if a != b {
                return a.cmp(b);
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn inv32(n: u32) -> u32 {
    let mut x = 2u32.wrapping_sub(n);
    x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x)));
    x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x)));
    x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x)));
    x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x)));
    x.wrapping_neg()
}

fn mont_reduce(t: &mut [u32], n: &[u32], n_prime: u32) -> Vec<u32> {
    let k = n.len();
    for i in 0..k {
        let m = t[i].wrapping_mul(n_prime);
        let mut carry = 0u64;
        for j in 0..k {
            let prod = (m as u64) * (n[j] as u64) + (t[i + j] as u64) + carry;
            t[i + j] = prod as u32;
            carry = prod >> 32;
        }
        let mut p = i + k;
        while carry > 0 && p < t.len() {
            let sum = (t[p] as u64) + carry;
            t[p] = sum as u32;
            carry = sum >> 32;
            p += 1;
        }
    }

    let mut res = BigUint {
        limbs: t[k..].to_vec(),
    };
    res.normalize();

    let n_big = BigUint { limbs: n.to_vec() };
    while res >= n_big {
        res = res.sub(&n_big);
    }
    let mut out = res.limbs;
    out.resize(k, 0);
    out
}

fn mont_mul(a: &[u32], b: &[u32], n: &[u32], n_prime: u32) -> Vec<u32> {
    let k = n.len();
    let mut t = vec![0u32; 2 * k + 2];
    for i in 0..k {
        let a_i = if i < a.len() { a[i] as u64 } else { 0 };
        let mut carry = 0u64;
        for j in 0..k {
            let b_j = if j < b.len() { b[j] as u64 } else { 0 };
            let sum = (t[i + j] as u64) + a_i * b_j + carry;
            t[i + j] = sum as u32;
            carry = sum >> 32;
        }
        t[i + k] += carry as u32;
    }
    mont_reduce(&mut t, n, n_prime)
}

/// Montgomery modular exponentiation: computes (base^exp) mod modulus.
pub fn montgomery_mod_pow(
    base: &BigUint,
    exp: &BigUint,
    modulus: &BigUint,
) -> Result<BigUint, CryptoError> {
    if !modulus.is_odd() || modulus.is_zero() {
        return Err(CryptoError::InvalidModulus);
    }
    if modulus.limbs.len() >= 127 {
        return Err(CryptoError::InvalidModulus);
    }
    if exp.is_zero() {
        return Ok(BigUint::one());
    }

    let k = modulus.limbs.len();
    let n_limbs = modulus.limbs.clone();
    let n_prime = inv32(n_limbs[0]);

    // Precompute R^2 mod N where R = 2^(32 * k) using fixed stack buffers
    const MAX_LIMBS: usize = 128;
    let mut r_limbs = [0u32; MAX_LIMBS];
    let mut r_len = 1usize;
    r_limbs[0] = 1;

    let mod_limbs = &modulus.limbs;
    for _ in 0..(2 * 32 * k) {
        let mut carry = 0u32;
        for limb in &mut r_limbs[..r_len] {
            let val = *limb;
            *limb = (val << 1) | carry;
            carry = val >> 31;
        }
        if carry > 0 && r_len < MAX_LIMBS {
            r_limbs[r_len] = carry;
            r_len += 1;
        }

        while r_len > 0 && r_limbs[r_len - 1] == 0 {
            r_len -= 1;
        }
        let is_ge = if r_len > mod_limbs.len() {
            true
        } else if r_len < mod_limbs.len() {
            false
        } else {
            let mut ge = true;
            for i in (0..r_len).rev() {
                if r_limbs[i] > mod_limbs[i] {
                    ge = true;
                    break;
                } else if r_limbs[i] < mod_limbs[i] {
                    ge = false;
                    break;
                }
            }
            ge
        };

        if is_ge {
            let mut borrow = 0u64;
            for i in 0..r_len {
                let m = if i < mod_limbs.len() {
                    mod_limbs[i] as u64
                } else {
                    0
                };
                let diff = (r_limbs[i] as u64).wrapping_sub(m).wrapping_sub(borrow);
                r_limbs[i] = diff as u32;
                borrow = (diff >> 63) & 1;
            }
            while r_len > 0 && r_limbs[r_len - 1] == 0 {
                r_len -= 1;
            }
        }
    }

    let r = BigUint {
        limbs: r_limbs[..r_len].to_vec(),
    };

    let mut base_padded = base.limbs.clone();
    base_padded.resize(k, 0);
    let mut r_squared_padded = r.limbs.clone();
    r_squared_padded.resize(k, 0);

    let x_bar_initial = mont_mul(&base_padded, &r_squared_padded, &n_limbs, n_prime);

    // Initial result is 1 in Montgomery form: 1 * R^2 mod N -> R mod N
    let mut one_padded = vec![0u32; k];
    one_padded[0] = 1;
    let mut res_bar = mont_mul(&one_padded, &r_squared_padded, &n_limbs, n_prime);

    let total_bits = k * 32;
    let mut current_base_bar = x_bar_initial;

    for bit_idx in 0..total_bits {
        let limb_idx = bit_idx / 32;
        let bit_in_limb = bit_idx % 32;
        let bit = if limb_idx < exp.limbs.len() {
            (exp.limbs[limb_idx] >> bit_in_limb) & 1
        } else {
            0
        };

        let next = mont_mul(&res_bar, &current_base_bar, &n_limbs, n_prime);
        let mask = 0u32.wrapping_sub(bit);
        for i in 0..res_bar.len() {
            res_bar[i] = (next[i] & mask) | (res_bar[i] & !mask);
        }
        current_base_bar = mont_mul(&current_base_bar, &current_base_bar, &n_limbs, n_prime);
    }

    let mut final_prod = vec![0u32; 2 * k + 4];
    final_prod[..k].copy_from_slice(&res_bar);
    let final_limbs = mont_reduce(&mut final_prod, &n_limbs, n_prime);

    let mut res = BigUint { limbs: final_limbs };
    res.normalize();
    Ok(res)
}

/// Oakley Group 1 (RFC 2409) 768-bit MODP Diffie-Hellman prime.
///
/// Note: Oakley Group 1 uses a 768-bit modulus, which is cryptographically
/// legacy/weak by modern standards, but is strictly protocol-mandated by
/// Spotify's proprietary Shannon authentication and key exchange wire format.
pub const DH_PRIME: [u8; 96] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc9, 0x0f, 0xda, 0xa2, 0x21, 0x68, 0xc2, 0x34,
    0xc4, 0xc6, 0x62, 0x8b, 0x80, 0xdc, 0x1c, 0xd1, 0x29, 0x02, 0x4e, 0x08, 0x8a, 0x67, 0xcc, 0x74,
    0x02, 0x0b, 0xbe, 0xa6, 0x3b, 0x13, 0x9b, 0x22, 0x51, 0x4a, 0x08, 0x79, 0x8e, 0x34, 0x04, 0xdd,
    0xef, 0x95, 0x19, 0xb3, 0xcd, 0x3a, 0x43, 0x1b, 0x30, 0x2b, 0x0a, 0x6d, 0xf2, 0x5f, 0x14, 0x37,
    0x4f, 0xe1, 0x35, 0x6d, 0x6d, 0x51, 0xc2, 0x45, 0xe4, 0x85, 0xb5, 0x76, 0x62, 0x5e, 0x7e, 0xc6,
    0xf4, 0x4c, 0x42, 0xe9, 0xa6, 0x3a, 0x36, 0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// Oakley Group 1 Diffie-Hellman implementation (768-bit MODP, generator 2).
#[derive(Clone, Debug)]
pub struct OakleyGroup1Dh {
    private_key: [u8; 96],
    public_key: [u8; 96],
}

impl OakleyGroup1Dh {
    /// Constructs a DH context from a given 96-byte private key.
    pub fn from_private_key(priv_key: [u8; 96]) -> Result<Self, CryptoError> {
        let p = BigUint::from_be_bytes(&DH_PRIME);
        let g = BigUint::from_u32(2);
        let x = BigUint::from_be_bytes(&priv_key);
        let y = montgomery_mod_pow(&g, &x, &p)?;
        let pub_vec = y.to_be_bytes_padded(96)?;
        let mut public_key = [0u8; 96];
        public_key.copy_from_slice(&pub_vec);

        Ok(Self {
            private_key: priv_key,
            public_key,
        })
    }

    /// Returns the 96-byte public key.
    pub fn public_key(&self) -> &[u8; 96] {
        &self.public_key
    }

    /// Computes the shared secret using peer's public key.
    pub fn calculate_shared(&self, remote_key: &[u8]) -> Result<[u8; 96], CryptoError> {
        if remote_key.len() > 96 {
            return Err(CryptoError::InvalidKeyLength);
        }
        let p = BigUint::from_be_bytes(&DH_PRIME);
        let y = BigUint::from_be_bytes(remote_key);
        let two = BigUint::from_u32(2);
        let p_minus_two = p.sub(&two);
        if y < two || y > p_minus_two {
            return Err(CryptoError::InvalidCiphertext);
        }
        let x = BigUint::from_be_bytes(&self.private_key);
        let shared = montgomery_mod_pow(&y, &x, &p)?;
        let shared_vec = shared.to_be_bytes_padded(96)?;
        let mut out = [0u8; 96];
        out.copy_from_slice(&shared_vec);
        Ok(out)
    }
}

impl Drop for OakleyGroup1Dh {
    fn drop(&mut self) {
        self.private_key.fill(0);
        core::hint::black_box(&mut self.private_key);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

/// Pure-Rust SHA-1 message digest implementation.
#[derive(Clone, Debug)]
pub struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    /// Creates a new SHA-1 instance.
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            buffer: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    /// Feeds data into the SHA-1 digest.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len += data.len() as u64;
        while !data.is_empty() {
            let needed = 64 - self.buf_len;
            let take = core::cmp::min(needed, data.len());
            self.buffer[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                self.process_block();
                self.buf_len = 0;
            }
        }
    }

    /// Finalizes and returns the 20-byte SHA-1 digest.
    pub fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.total_len * 8;
        self.buffer[self.buf_len] = 0x80;
        self.buf_len += 1;

        if self.buf_len > 56 {
            for i in self.buf_len..64 {
                self.buffer[i] = 0;
            }
            self.process_block();
            self.buf_len = 0;
        }

        for i in self.buf_len..56 {
            self.buffer[i] = 0;
        }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        self.process_block();

        let mut digest = [0u8; 20];
        for (i, &s) in self.state.iter().enumerate() {
            digest[i * 4..(i + 1) * 4].copy_from_slice(&s.to_be_bytes());
        }
        digest
    }

    /// One-shot SHA-1 digest helper.
    pub fn digest(data: &[u8]) -> [u8; 20] {
        let mut ctx = Self::new();
        ctx.update(data);
        ctx.finalize()
    }

    fn process_block(&mut self) {
        let mut w = [0u32; 80];
        for (i, item) in w.iter_mut().enumerate().take(16) {
            *item = u32::from_be_bytes([
                self.buffer[i * 4],
                self.buffer[i * 4 + 1],
                self.buffer[i * 4 + 2],
                self.buffer[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for (i, &w_val) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w_val);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

/// Computes HMAC-SHA1.
pub fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = Sha1::digest(key);
        key_block[..20].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = key_block[i] ^ 0x36;
        opad[i] = key_block[i] ^ 0x5c;
    }

    let mut inner = Sha1::new();
    inner.update(&ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha1::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize()
}

/// PBKDF2 with PRF HMAC-SHA1.
pub fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut block_idx = 1u32;

    while out.len() < out_len {
        let mut salt_extended = Vec::with_capacity(salt.len() + 4);
        salt_extended.extend_from_slice(salt);
        salt_extended.extend_from_slice(&block_idx.to_be_bytes());

        let mut u = hmac_sha1(password, &salt_extended);
        let mut t = u;

        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for j in 0..20 {
                t[j] ^= u[j];
            }
        }

        let needed = out_len - out.len();
        let take = core::cmp::min(needed, 20);
        out.extend_from_slice(&t[..take]);
        block_idx += 1;
    }

    out
}

const S_BOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const INV_S_BOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

const RCON: [u32; 10] = [
    0x01000000, 0x02000000, 0x04000000, 0x08000000, 0x10000000, 0x20000000, 0x40000000, 0x80000000,
    0x1b000000, 0x36000000,
];

fn sub_word(w: u32) -> u32 {
    let b0 = S_BOX[(w >> 24) as usize] as u32;
    let b1 = S_BOX[((w >> 16) & 0xFF) as usize] as u32;
    let b2 = S_BOX[((w >> 8) & 0xFF) as usize] as u32;
    let b3 = S_BOX[(w & 0xFF) as usize] as u32;
    (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
}

fn rot_word(w: u32) -> u32 {
    w.rotate_left(8)
}

#[inline(always)]
const fn xtime(x: u8) -> u8 {
    (x << 1) ^ (((x >> 7) & 1).wrapping_neg() & 0x1b)
}

#[inline(always)]
const fn inv_mix_muls(x: u8) -> (u8, u8, u8, u8) {
    let x2 = xtime(x);
    let x4 = xtime(x2);
    let x8 = xtime(x4);
    (x8 ^ x, x8 ^ x2 ^ x, x8 ^ x4 ^ x, x8 ^ x4 ^ x2)
}

/// AES-128 CTR mode stream cipher.
#[derive(Clone, Debug)]
pub struct Aes128Ctr {
    round_keys: [u32; 44],
    iv: [u8; 16],
}

impl Aes128Ctr {
    /// Creates a new AES-128-CTR instance with 16-byte key and 16-byte initial IV.
    pub fn new(key: &[u8; 16], iv: [u8; 16]) -> Self {
        let mut rk = [0u32; 44];
        for i in 0..4 {
            rk[i] =
                u32::from_be_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }
        for i in 4..44 {
            let mut temp = rk[i - 1];
            if i % 4 == 0 {
                temp = sub_word(rot_word(temp)) ^ RCON[(i / 4) - 1];
            }
            rk[i] = rk[i - 4] ^ temp;
        }
        Self { round_keys: rk, iv }
    }

    fn encrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut state = *block;

        // Round 0: AddRoundKey
        let rk_first = &self.round_keys[0..4];
        for c in 0..4 {
            let k = rk_first[c].to_be_bytes();
            for r in 0..4 {
                state[r + 4 * c] ^= k[r];
            }
        }

        // Rounds 1..9
        for round in 1..10 {
            // SubBytes
            for b in &mut state {
                *b = S_BOX[*b as usize];
            }

            // ShiftRows
            let mut temp = [0u8; 16];
            temp[0] = state[0];
            temp[4] = state[4];
            temp[8] = state[8];
            temp[12] = state[12];

            temp[1] = state[5];
            temp[5] = state[9];
            temp[9] = state[13];
            temp[13] = state[1];

            temp[2] = state[10];
            temp[6] = state[14];
            temp[10] = state[2];
            temp[14] = state[6];

            temp[3] = state[15];
            temp[7] = state[3];
            temp[11] = state[7];
            temp[15] = state[11];

            // MixColumns
            for c in 0..4 {
                let s0 = temp[4 * c];
                let s1 = temp[4 * c + 1];
                let s2 = temp[4 * c + 2];
                let s3 = temp[4 * c + 3];

                state[4 * c] = xtime(s0) ^ (xtime(s1) ^ s1) ^ s2 ^ s3;
                state[4 * c + 1] = s0 ^ xtime(s1) ^ (xtime(s2) ^ s2) ^ s3;
                state[4 * c + 2] = s0 ^ s1 ^ xtime(s2) ^ (xtime(s3) ^ s3);
                state[4 * c + 3] = (xtime(s0) ^ s0) ^ s1 ^ s2 ^ xtime(s3);
            }

            // AddRoundKey
            let rk_cur = &self.round_keys[round * 4..(round + 1) * 4];
            for c in 0..4 {
                let k = rk_cur[c].to_be_bytes();
                for r in 0..4 {
                    state[r + 4 * c] ^= k[r];
                }
            }
        }

        // Round 10: SubBytes, ShiftRows, AddRoundKey (no MixColumns)
        for b in &mut state {
            *b = S_BOX[*b as usize];
        }
        let mut temp = [0u8; 16];
        temp[0] = state[0];
        temp[4] = state[4];
        temp[8] = state[8];
        temp[12] = state[12];

        temp[1] = state[5];
        temp[5] = state[9];
        temp[9] = state[13];
        temp[13] = state[1];

        temp[2] = state[10];
        temp[6] = state[14];
        temp[10] = state[2];
        temp[14] = state[6];

        temp[3] = state[15];
        temp[7] = state[3];
        temp[11] = state[7];
        temp[15] = state[11];

        let rk_last = &self.round_keys[40..44];
        for c in 0..4 {
            let k = rk_last[c].to_be_bytes();
            for r in 0..4 {
                temp[r + 4 * c] ^= k[r];
            }
        }

        temp
    }

    fn increment_iv(iv: &mut [u8; 16]) {
        let low =
            u64::from_be_bytes([iv[8], iv[9], iv[10], iv[11], iv[12], iv[13], iv[14], iv[15]]);
        let (new_low, carry) = low.overflowing_add(1);
        iv[8..16].copy_from_slice(&new_low.to_be_bytes());
        if carry {
            let high = u64::from_be_bytes([iv[0], iv[1], iv[2], iv[3], iv[4], iv[5], iv[6], iv[7]]);
            iv[0..8].copy_from_slice(&high.wrapping_add(1).to_be_bytes());
        }
    }

    /// Encrypts or decrypts bytes in-place using CTR keystream.
    pub fn process(&mut self, data: &mut [u8]) {
        let mut pos = 0;
        let full_blocks_len = data.len() / 16 * 16;
        while pos < full_blocks_len {
            let keystream = self.encrypt_block(&self.iv);
            Self::increment_iv(&mut self.iv);
            for j in 0..4 {
                let offset = pos + j * 4;
                let d_word = u32::from_ne_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                let k_word = u32::from_ne_bytes([
                    keystream[j * 4],
                    keystream[j * 4 + 1],
                    keystream[j * 4 + 2],
                    keystream[j * 4 + 3],
                ]);
                data[offset..offset + 4].copy_from_slice(&(d_word ^ k_word).to_ne_bytes());
            }
            pos += 16;
        }

        if pos < data.len() {
            let keystream = self.encrypt_block(&self.iv);
            Self::increment_iv(&mut self.iv);
            let take = data.len() - pos;
            for i in 0..take {
                data[pos + i] ^= keystream[i];
            }
        }
    }
}

/// AES-192 ECB mode block cipher (used in Spotify secondary blob decryption).
#[derive(Clone, Debug)]
pub struct Aes192Ecb {
    round_keys: [u32; 52],
}

impl Aes192Ecb {
    /// Creates a new AES-192-ECB instance with 24-byte key.
    pub fn new(key: &[u8; 24]) -> Self {
        let mut rk = [0u32; 52];
        for i in 0..6 {
            rk[i] =
                u32::from_be_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }
        for i in 6..52 {
            let mut temp = rk[i - 1];
            if i % 6 == 0 {
                temp = sub_word(rot_word(temp)) ^ RCON[(i / 6) - 1];
            }
            rk[i] = rk[i - 6] ^ temp;
        }
        Self { round_keys: rk }
    }

    /// Decrypts a single 16-byte block in place.
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut state = *block;

        // Round 12: AddRoundKey
        let rk_last = &self.round_keys[48..52];
        for c in 0..4 {
            let k = rk_last[c].to_be_bytes();
            for r in 0..4 {
                state[r + 4 * c] ^= k[r];
            }
        }

        // Rounds 11 down to 1
        for round in (1..12).rev() {
            let mut temp = [0u8; 16];
            temp[0] = state[0];
            temp[4] = state[4];
            temp[8] = state[8];
            temp[12] = state[12];

            temp[1] = state[13];
            temp[5] = state[1];
            temp[9] = state[5];
            temp[13] = state[9];

            temp[2] = state[10];
            temp[6] = state[14];
            temp[10] = state[2];
            temp[14] = state[6];

            temp[3] = state[7];
            temp[7] = state[11];
            temp[11] = state[15];
            temp[15] = state[3];

            for b in &mut temp {
                *b = INV_S_BOX[*b as usize];
            }

            let rk_cur = &self.round_keys[round * 4..(round + 1) * 4];
            for c in 0..4 {
                let k = rk_cur[c].to_be_bytes();
                for r in 0..4 {
                    temp[r + 4 * c] ^= k[r];
                }
            }

            for c in 0..4 {
                let (s0_9, s0_11, s0_13, s0_14) = inv_mix_muls(temp[4 * c]);
                let (s1_9, s1_11, s1_13, s1_14) = inv_mix_muls(temp[4 * c + 1]);
                let (s2_9, s2_11, s2_13, s2_14) = inv_mix_muls(temp[4 * c + 2]);
                let (s3_9, s3_11, s3_13, s3_14) = inv_mix_muls(temp[4 * c + 3]);

                state[4 * c] = s0_14 ^ s1_11 ^ s2_13 ^ s3_9;
                state[4 * c + 1] = s0_9 ^ s1_14 ^ s2_11 ^ s3_13;
                state[4 * c + 2] = s0_13 ^ s1_9 ^ s2_14 ^ s3_11;
                state[4 * c + 3] = s0_11 ^ s1_13 ^ s2_9 ^ s3_14;
            }
        }

        // Round 0: InvShiftRows, InvSubBytes, AddRoundKey (no InvMixColumns)
        let mut temp = [0u8; 16];
        temp[0] = state[0];
        temp[4] = state[4];
        temp[8] = state[8];
        temp[12] = state[12];

        temp[1] = state[13];
        temp[5] = state[1];
        temp[9] = state[5];
        temp[13] = state[9];

        temp[2] = state[10];
        temp[6] = state[14];
        temp[10] = state[2];
        temp[14] = state[6];

        temp[3] = state[7];
        temp[7] = state[11];
        temp[11] = state[15];
        temp[15] = state[3];

        for b in &mut temp {
            *b = INV_S_BOX[*b as usize];
        }

        let rk_first = &self.round_keys[0..4];
        for c in 0..4 {
            let k = rk_first[c].to_be_bytes();
            for r in 0..4 {
                temp[r + 4 * c] ^= k[r];
            }
        }

        *block = temp;
    }

    /// Decrypts slice of 16-byte blocks in place.
    ///
    /// If `data` is not aligned to 16-byte block boundaries, any partial
    /// trailing remainder bytes are zeroized to avoid leaking unauthenticated
    /// or partial plaintext.
    pub fn decrypt(&self, data: &mut [u8]) {
        let rem_start = data.len() - (data.len() % 16);
        for chunk in data[..rem_start].chunks_exact_mut(16) {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            self.decrypt_block(&mut block);
            chunk.copy_from_slice(&block);
        }
        for b in &mut data[rem_start..] {
            *b = 0;
        }
    }

    /// Decrypts slice of 16-byte blocks in place, returning an error if not
    /// block-aligned.
    pub fn decrypt_checked(&self, data: &mut [u8]) -> Result<(), CryptoError> {
        if !data.len().is_multiple_of(16) {
            return Err(CryptoError::InvalidCiphertext);
        }
        self.decrypt(data);
        Ok(())
    }
}

/// Adds an integer to a big-endian byte array.
/// Matches `bigNumAdd` in `Utils.cpp`.
pub fn big_num_add(num: &[u8], mut carry: usize) -> Vec<u8> {
    let mut result = Vec::from(num);
    for byte in result.iter_mut().rev() {
        let sum = (*byte as usize) + carry;
        *byte = (sum & 0xff) as u8;
        carry = sum >> 8;
        if carry == 0 {
            break;
        }
    }
    while carry > 0 {
        result.insert(0, (carry & 0xff) as u8);
        carry >>= 8;
    }
    result
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes binary slice into standard Base64 string (RFC 4648).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let chunks = data.chunks_exact(3);
    let rem = chunks.remainder();

    for chunk in chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(B64_CHARS[((n >> 18) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((n >> 12) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((n >> 6) & 0x3F) as usize] as char);
        out.push(B64_CHARS[(n & 0x3F) as usize] as char);
    }

    match rem.len() {
        1 => {
            let n = (rem[0] as u32) << 16;
            out.push(B64_CHARS[((n >> 18) & 0x3F) as usize] as char);
            out.push(B64_CHARS[((n >> 12) & 0x3F) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
            out.push(B64_CHARS[((n >> 18) & 0x3F) as usize] as char);
            out.push(B64_CHARS[((n >> 12) & 0x3F) as usize] as char);
            out.push(B64_CHARS[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        _ => {}
    }

    out
}

fn b64_val(c: u8) -> Result<u8, CryptoError> {
    match c {
        b'A'..=b'Z' => Ok(c - b'A'),
        b'a'..=b'z' => Ok(c - b'a' + 26),
        b'0'..=b'9' => Ok(c - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(CryptoError::InvalidBase64),
    }
}

/// Decodes standard Base64 string into bytes.
pub fn base64_decode(data: &str) -> Result<Vec<u8>, CryptoError> {
    let raw = data.trim_end_matches(|c: char| c.is_whitespace() || c == '=');
    let mut out = Vec::with_capacity(raw.len() * 3 / 4);
    let bytes = raw.as_bytes();
    let chunks = bytes.chunks_exact(4);
    let rem = chunks.remainder();

    for chunk in chunks {
        let v0 = b64_val(chunk[0])? as u32;
        let v1 = b64_val(chunk[1])? as u32;
        let v2 = b64_val(chunk[2])? as u32;
        let v3 = b64_val(chunk[3])? as u32;
        let n = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((n >> 16) & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push((n & 0xFF) as u8);
    }

    match rem.len() {
        2 => {
            let v0 = b64_val(rem[0])? as u32;
            let v1 = b64_val(rem[1])? as u32;
            let n = (v0 << 18) | (v1 << 12);
            out.push(((n >> 16) & 0xFF) as u8);
        }
        3 => {
            let v0 = b64_val(rem[0])? as u32;
            let v1 = b64_val(rem[1])? as u32;
            let v2 = b64_val(rem[2])? as u32;
            let n = (v0 << 18) | (v1 << 12) | (v2 << 6);
            out.push(((n >> 16) & 0xFF) as u8);
            out.push(((n >> 8) & 0xFF) as u8);
        }
        0 => {}
        _ => return Err(CryptoError::InvalidBase64),
    }

    Ok(out)
}

/// Compares two byte slices in constant time using bitwise XOR accumulation.
///
/// Returns `true` if and only if `a` and `b` have equal length and
/// identical contents.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dh_reject_public_key_out_of_bounds() -> Result<(), CryptoError> {
        let priv_key = [0x42u8; 96];
        let dh = OakleyGroup1Dh::from_private_key(priv_key)?;

        // y = 0 (< 2)
        let zero_key = [0u8; 96];
        assert_eq!(
            dh.calculate_shared(&zero_key),
            Err(CryptoError::InvalidCiphertext)
        );

        // y = 1 (< 2)
        let mut one_key = [0u8; 96];
        one_key[95] = 1;
        assert_eq!(
            dh.calculate_shared(&one_key),
            Err(CryptoError::InvalidCiphertext)
        );

        // y = p - 1 (> p - 2)
        let mut p_minus_one = DH_PRIME;
        p_minus_one[95] = 0xfe;
        assert_eq!(
            dh.calculate_shared(&p_minus_one),
            Err(CryptoError::InvalidCiphertext)
        );

        // y = p (> p - 2)
        assert_eq!(
            dh.calculate_shared(&DH_PRIME),
            Err(CryptoError::InvalidCiphertext)
        );

        // y = 2 (valid lower bound)
        let mut two_key = [0u8; 96];
        two_key[95] = 2;
        assert!(dh.calculate_shared(&two_key).is_ok());

        // y = p - 2 (valid upper bound)
        let mut p_minus_two = DH_PRIME;
        p_minus_two[95] = 0xfd;
        assert!(dh.calculate_shared(&p_minus_two).is_ok());

        Ok(())
    }

    #[test]
    fn test_constant_time_eq() {
        let a = b"hello world secure token";
        let b = b"hello world secure token";
        let c = b"hello world secure tokez";
        let d = b"short";

        assert!(constant_time_eq(a, b));
        assert!(!constant_time_eq(a, c));
        assert!(!constant_time_eq(a, d));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_biguint_sub_underflow_clamps_to_zero() {
        let a = BigUint::from_u32(10);
        let b = BigUint::from_u32(20);
        assert_eq!(a.sub(&b), BigUint::zero());
    }

    #[test]
    fn test_aes192_ecb_remainder_zeroized_and_checked() {
        let key = [0x55u8; 24];
        let cipher = Aes192Ecb::new(&key);

        let mut data = [0xffu8; 20];
        assert_eq!(
            cipher.decrypt_checked(&mut data),
            Err(CryptoError::InvalidCiphertext)
        );

        // decrypt() zeroizes remainder
        cipher.decrypt(&mut data);
        assert_eq!(&data[16..], &[0u8; 4]);
    }

    #[test]
    fn test_montgomery_mod_pow_excessive_modulus_rejected() {
        let base = BigUint::from_u32(3);
        let exp = BigUint::from_u32(5);
        let mut huge_mod = BigUint {
            limbs: vec![1; 127],
        };
        huge_mod.limbs[0] |= 1;
        assert_eq!(
            montgomery_mod_pow(&base, &exp, &huge_mod),
            Err(CryptoError::InvalidModulus)
        );
    }
}
