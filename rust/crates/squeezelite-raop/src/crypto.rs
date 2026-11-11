//! Cryptographic primitives for AirPlay 1 (RAOP).
//!
//! Provides pure-Rust `#![no_std]` RSA private key operations
//! (modular exponentiation, PKCS#1 v1.5 and OAEP unpadding) and AES-128-CBC.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

/// Cryptographic errors encountered during RSA or AES operations.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CryptoError {
    /// Input buffer length does not match expected block size.
    InvalidInputLength,
    /// Ciphertext exceeds the modulus length.
    CiphertextTooLarge,
    /// PKCS#1 padding verification failed.
    InvalidPadding,
    /// Decrypted payload is not 16 bytes (AES-128 key length).
    InvalidKeyLength,
    /// The modulus is even or zero.
    InvalidModulus,
    /// BigUint parsing error.
    InvalidNumber,
    /// Base64 decoding failed or invalid input length.
    InvalidBase64,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInputLength => write!(f, "Invalid input length"),
            Self::CiphertextTooLarge => write!(f, "Ciphertext exceeds modulus"),
            Self::InvalidPadding => write!(f, "Invalid cryptographic padding"),
            Self::InvalidKeyLength => write!(f, "Expected 16-byte AES-128 key"),
            Self::InvalidModulus => write!(f, "Modulus must be odd and non-zero"),
            Self::InvalidNumber => write!(f, "Failed to parse big number"),
            Self::InvalidBase64 => write!(f, "Invalid base64 encoding"),
        }
    }
}

/// Unsigned arbitrary-precision integer for RSA arithmetic.
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

    /// Returns true if the number is zero.
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Returns true if the number is odd.
    pub fn is_odd(&self) -> bool {
        self.limbs.first().is_some_and(|&limb| (limb & 1) == 1)
    }

    /// Returns the number of significant limbs.
    pub fn limb_count(&self) -> usize {
        self.limbs.len()
    }

    /// Creates a BigUint from big-endian bytes.
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::zero();
        }
        let mut limbs = Vec::new();
        let mut idx = bytes.len();
        while idx > 0 {
            let start = idx.saturating_sub(4);
            let chunk = &bytes[start..idx];
            let mut limb = 0u32;
            for &byte in chunk {
                limb = (limb << 8) | (byte as u32);
            }
            limbs.push(limb);
            idx = start;
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
            bytes.push((limb >> 24) as u8);
            bytes.push((limb >> 16) as u8);
            bytes.push((limb >> 8) as u8);
            bytes.push(limb as u8);
        }
        // Strip leading zeros
        let first_non_zero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        if first_non_zero == bytes.len() {
            vec![0]
        } else {
            bytes[first_non_zero..].to_vec()
        }
    }

    /// Exports BigUint to fixed-length big-endian bytes with leading zeros.
    pub fn to_be_bytes_padded(&self, len: usize) -> Result<Vec<u8>, CryptoError> {
        let raw = self.to_be_bytes();
        if raw.len() > len {
            return Err(CryptoError::CiphertextTooLarge);
        }
        let mut padded = vec![0u8; len];
        let start = len - raw.len();
        padded[start..].copy_from_slice(&raw);
        Ok(padded)
    }

    /// Strips trailing zero limbs.
    fn normalize(&mut self) {
        while let Some(&last) = self.limbs.last() {
            if last == 0 {
                self.limbs.pop();
            } else {
                break;
            }
        }
    }

    /// Bit length of this integer.
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

    /// Subtracts another BigUint using wrapping limb subtraction.
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
            let a_val = a as u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let prod = a_val * (b as u64) + (result[i + j] as u64) + carry;
                result[i + j] = prod as u32;
                carry = prod >> 32;
            }
            result[i + other.limbs.len()] += carry as u32;
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
            match a.cmp(b) {
                Ordering::Equal => continue,
                non_eq => return non_eq,
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

/// Computes -n^(-1) mod 2^32 using Newton's method.
fn inv32(n: u32) -> u32 {
    let mut x = 1u32;
    for _ in 0..5 {
        x = x.wrapping_mul(2u32.wrapping_sub(n.wrapping_mul(x)));
    }
    0u32.wrapping_sub(x)
}

/// Montgomery reduction: computes (t / R) mod n where R = 2^(32 * k).
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
    if res >= n_big {
        res = res.sub(&n_big);
    }

    let mut out = res.limbs;
    out.resize(k, 0);
    out
}

/// Multiplies two limb slices into a product vector.
fn mul_limbs(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut res = vec![0u32; a.len() + b.len() + 4];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        let x_val = x as u64;
        for (j, &y) in b.iter().enumerate() {
            let prod = x_val * (y as u64) + (res[i + j] as u64) + carry;
            res[i + j] = prod as u32;
            carry = prod >> 32;
        }
        let mut p = i + b.len();
        while carry > 0 && p < res.len() {
            let sum = (res[p] as u64) + carry;
            res[p] = sum as u32;
            carry = sum >> 32;
            p += 1;
        }
    }
    res
}

/// Montgomery modular exponentiation: computes (base^exp) mod modulus.
pub fn montgomery_mod_pow(
    base: &BigUint,
    exp: &BigUint,
    modulus: &BigUint,
) -> Result<BigUint, CryptoError> {
    if !modulus.is_odd() {
        return Err(CryptoError::InvalidModulus);
    }
    if modulus.is_zero() {
        return Err(CryptoError::InvalidModulus);
    }
    if exp.is_zero() {
        return Ok(BigUint::one());
    }

    let k = modulus.limbs.len();
    const MAX_LIMBS: usize = 128;
    if k >= MAX_LIMBS {
        return Err(CryptoError::CiphertextTooLarge);
    }
    let n_limbs = modulus.limbs.clone();
    let n_prime = inv32(n_limbs[0]);

    // Precompute R^2 mod N where R = 2^(32 * k) using fixed stack buffers
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

    let mut r2_limbs = r.limbs;
    r2_limbs.resize(k, 0);

    let mut c_limbs = base.limbs.clone();
    c_limbs.resize(k, 0);

    // Convert base to Montgomery form: c_bar = mont_reduce(base * R^2)
    let mut prod_c_r2 = mul_limbs(&c_limbs, &r2_limbs);
    let c_bar = mont_reduce(&mut prod_c_r2, &n_limbs, n_prime);

    // Initial accumulator = 1 in Montgomery form = mont_reduce(1 * R^2)
    let mut one_limbs = vec![0u32; k];
    one_limbs[0] = 1;
    let mut prod_one_r2 = mul_limbs(&one_limbs, &r2_limbs);
    let mut x_bar = mont_reduce(&mut prod_one_r2, &n_limbs, n_prime);

    // Exponentiate MSB to LSB over fixed modulus bit size (k * 32)
    let total_bits = k * 32;
    for bit_idx in (0..total_bits).rev() {
        let limb_idx = bit_idx / 32;
        let bit_in_limb = bit_idx % 32;
        let bit = if limb_idx < exp.limbs.len() {
            (exp.limbs[limb_idx] >> bit_in_limb) & 1
        } else {
            0
        };

        let mut sq = mul_limbs(&x_bar, &x_bar);
        x_bar = mont_reduce(&mut sq, &n_limbs, n_prime);

        let mut mul = mul_limbs(&x_bar, &c_bar);
        let next = mont_reduce(&mut mul, &n_limbs, n_prime);

        let mask = 0u32.wrapping_sub(bit);
        for i in 0..x_bar.len() {
            x_bar[i] = (next[i] & mask) | (x_bar[i] & !mask);
        }
    }

    // Convert from Montgomery form: x = mont_reduce(x_bar * 1)
    let mut final_prod = vec![0u32; 2 * k + 4];
    final_prod[..k].copy_from_slice(&x_bar);
    let final_limbs = mont_reduce(&mut final_prod, &n_limbs, n_prime);

    let mut res = BigUint { limbs: final_limbs };
    res.normalize();
    Ok(res)
}

/// Pure-Rust SHA-1 message digest implementation.
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

    /// One-shot SHA-1 computation.
    pub fn digest(data: &[u8]) -> [u8; 20] {
        let mut ctx = Self::new();
        ctx.update(data);
        ctx.finalize()
    }

    fn process_block(&mut self) {
        let mut w = [0u32; 80];
        for (i, item) in w.iter_mut().enumerate().take(16) {
            let b = &self.buffer[i * 4..(i + 1) * 4];
            *item = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for (i, &w_i) in w.iter().enumerate() {
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
                .wrapping_add(w_i);
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

/// Mask Generation Function 1 (MGF1) using SHA-1.
fn mgf1_sha1(seed: &[u8], mask_len: usize) -> Vec<u8> {
    let mut mask = Vec::with_capacity(mask_len);
    let mut counter = 0u32;
    while mask.len() < mask_len {
        let mut ctx = Sha1::new();
        ctx.update(seed);
        ctx.update(&counter.to_be_bytes());
        let digest = ctx.finalize();
        let take = core::cmp::min(digest.len(), mask_len - mask.len());
        mask.extend_from_slice(&digest[..take]);
        counter += 1;
    }
    mask
}

/// Unpads PKCS#1 v1.5 block type 02 (encryption) and extracts 16-byte key.
pub fn unpad_pkcs1_v15_type2(plaintext: &[u8]) -> Result<[u8; 16], CryptoError> {
    if plaintext.len() < 11 {
        return Err(CryptoError::InvalidPadding);
    }

    let mut bad_pad = 0u32;
    bad_pad |= plaintext[0] as u32;
    bad_pad |= (plaintext[1] as u32) ^ 0x02;

    let mut found_sep = 0u32;
    let mut sep_idx = 0usize;
    for (i, &b) in plaintext.iter().enumerate().skip(2) {
        let is_zero = if b == 0x00 { 1u32 } else { 0u32 };
        let first_zero = is_zero & (1 ^ found_sep);
        found_sep |= is_zero;
        sep_idx |= i * (first_zero as usize);
    }

    bad_pad |= 1 ^ found_sep;
    if sep_idx < 10 {
        bad_pad |= 1;
    }

    if bad_pad != 0 {
        return Err(CryptoError::InvalidPadding);
    }

    let payload = &plaintext[sep_idx + 1..];
    if payload.len() != 16 {
        return Err(CryptoError::InvalidKeyLength);
    }
    let mut key = [0u8; 16];
    key.copy_from_slice(payload);
    Ok(key)
}

/// Unpads RSAES-OAEP with SHA-1 and extracts the 16-byte AES key.
pub fn unpad_oaep_sha1(plaintext: &[u8]) -> Result<[u8; 16], CryptoError> {
    if plaintext.len() < 42 {
        return Err(CryptoError::InvalidPadding);
    }

    let mut bad = 0u32;
    bad |= plaintext[0] as u32;

    let masked_seed = &plaintext[1..21];
    let masked_db = &plaintext[21..];

    let seed_mask = mgf1_sha1(masked_db, 20);
    let mut seed = [0u8; 20];
    for i in 0..20 {
        seed[i] = masked_seed[i] ^ seed_mask[i];
    }

    let db_mask = mgf1_sha1(&seed, masked_db.len());
    let mut db = vec![0u8; masked_db.len()];
    for i in 0..masked_db.len() {
        db[i] = masked_db[i] ^ db_mask[i];
    }

    let l_hash = Sha1::digest(b"");
    for i in 0..20 {
        bad |= (db[i] ^ l_hash[i]) as u32;
    }

    let mut looking_for_one = 1u32;
    let mut sep_idx = 0usize;
    let mut invalid_ps = 0u32;

    for (i, &b) in db.iter().enumerate().skip(20) {
        let is_one = if b == 0x01 { 1u32 } else { 0u32 };
        let is_zero = if b == 0x00 { 1u32 } else { 0u32 };

        invalid_ps |= looking_for_one & (1 ^ (is_zero | is_one));
        let first_one = is_one & looking_for_one;
        sep_idx |= i * (first_one as usize);
        looking_for_one &= 1 ^ is_one;
    }

    bad |= invalid_ps;
    bad |= looking_for_one;

    if bad != 0 {
        return Err(CryptoError::InvalidPadding);
    }

    let payload = &db[sep_idx + 1..];
    if payload.len() != 16 {
        return Err(CryptoError::InvalidKeyLength);
    }
    let mut key = [0u8; 16];
    key.copy_from_slice(payload);
    Ok(key)
}

/// Pads a message using PKCS#1 v1.5 block type 01 for Apple-Response.
pub fn pad_pkcs1_v15_type1(message: &[u8], target_len: usize) -> Result<Vec<u8>, CryptoError> {
    if target_len < message.len() + 11 {
        return Err(CryptoError::InvalidInputLength);
    }
    let mut buf = vec![0u8; target_len];
    buf[0] = 0x00;
    buf[1] = 0x01;
    let pad_len = target_len - message.len() - 3;
    for i in 0..pad_len {
        buf[2 + i] = 0xFF;
    }
    buf[2 + pad_len] = 0x00;
    buf[3 + pad_len..].copy_from_slice(message);
    Ok(buf)
}

/// RSA Private Key supporting arbitrary modulus bit sizes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RsaPrivateKey {
    pub n: BigUint,
    pub d: BigUint,
    n_len: usize,
}

impl RsaPrivateKey {
    /// Constructs an RSA private key from big-endian modulus and exponent bytes.
    pub fn from_be_bytes(n_bytes: &[u8], d_bytes: &[u8]) -> Self {
        Self {
            n: BigUint::from_be_bytes(n_bytes),
            d: BigUint::from_be_bytes(d_bytes),
            n_len: n_bytes.len(),
        }
    }

    /// Modulus length in bytes.
    pub fn modulus_bytes(&self) -> usize {
        self.n_len
    }

    /// Performs private key operation: m = c^d mod n.
    pub fn decrypt_raw(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let c = BigUint::from_be_bytes(ciphertext);
        if c >= self.n {
            return Err(CryptoError::CiphertextTooLarge);
        }
        let m = montgomery_mod_pow(&c, &self.d, &self.n)?;
        m.to_be_bytes_padded(self.n_len)
    }

    /// Decrypts encrypted AES session key from RTSP SDP `rsaaeskey`.
    ///
    /// Auto-detects PKCS#1 v1.5 type 02 or OAEP padding.
    pub fn decrypt_session_key(&self, ciphertext: &[u8]) -> Result<[u8; 16], CryptoError> {
        let decrypted = self.decrypt_raw(ciphertext)?;
        // Try PKCS#1 v1.5 type 2 first
        if let Ok(key) = unpad_pkcs1_v15_type2(&decrypted) {
            return Ok(key);
        }
        // Fallback to OAEP with SHA-1
        unpad_oaep_sha1(&decrypted)
    }

    /// Signs an Apple-Challenge payload using PKCS#1 v1.5 block type 01.
    pub fn sign_apple_challenge(&self, challenge_data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let padded = pad_pkcs1_v15_type1(challenge_data, self.n_len)?;
        let m = BigUint::from_be_bytes(&padded);
        let s = montgomery_mod_pow(&m, &self.d, &self.n)?;
        s.to_be_bytes_padded(self.n_len)
    }
}

/// AES Forward S-box.
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

/// AES Inverse S-box initializer.
const INV_S_BOX_INIT: [u8; 256] = [
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

// JUSTIFICATION: Placing INV_S_BOX in internal .dram1 avoids SPI flash
// cache stalls during real-time AirPlay RTP packet decryption.
// SAFETY: .dram1 is a valid initialized RWDATA section in esp-hal's linker
// script for plain [u8; 256] data with no alignment or init hazards.
#[cfg_attr(target_arch = "xtensa", allow(unsafe_code))]
#[cfg_attr(target_arch = "xtensa", link_section = ".dram1")]
static INV_S_BOX: [u8; 256] = INV_S_BOX_INIT;

/// AES Rcon array.
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
const fn gf_mul(x: u8, factor: u8) -> u8 {
    let x2 = xtime(x);
    let x4 = xtime(x2);
    let x8 = xtime(x4);
    match factor {
        9 => x8 ^ x,
        11 => x8 ^ x2 ^ x,
        13 => x8 ^ x4 ^ x,
        14 => x8 ^ x4 ^ x2,
        _ => 0,
    }
}

const fn compute_td0() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let s = INV_S_BOX_INIT[i];
        table[i] = ((gf_mul(s, 14) as u32) << 24)
            | ((gf_mul(s, 9) as u32) << 16)
            | ((gf_mul(s, 13) as u32) << 8)
            | (gf_mul(s, 11) as u32);
        i += 1;
    }
    table
}

// JUSTIFICATION: Placing TD0 in internal .dram1 avoids SPI flash cache
// stalls during real-time AirPlay RTP packet decryption.
// SAFETY: .dram1 is a valid initialized RWDATA section in esp-hal's linker
// script for plain [u32; 256] data with no alignment or init hazards.
#[cfg_attr(target_arch = "xtensa", allow(unsafe_code))]
#[cfg_attr(target_arch = "xtensa", link_section = ".dram1")]
static TD0: [u32; 256] = compute_td0();

#[inline(always)]
const fn inv_mix_col_word(w: u32) -> u32 {
    let b0 = (w >> 24) as u8;
    let b1 = (w >> 16) as u8;
    let b2 = (w >> 8) as u8;
    let b3 = w as u8;

    let r0 = gf_mul(b0, 14) ^ gf_mul(b1, 11) ^ gf_mul(b2, 13) ^ gf_mul(b3, 9);
    let r1 = gf_mul(b0, 9) ^ gf_mul(b1, 14) ^ gf_mul(b2, 11) ^ gf_mul(b3, 13);
    let r2 = gf_mul(b0, 13) ^ gf_mul(b1, 9) ^ gf_mul(b2, 14) ^ gf_mul(b3, 11);
    let r3 = gf_mul(b0, 11) ^ gf_mul(b1, 13) ^ gf_mul(b2, 9) ^ gf_mul(b3, 14);

    u32::from_be_bytes([r0, r1, r2, r3])
}

/// AES-128-CBC Decryptor in pure `#![no_std]`.
#[derive(Clone, Debug)]
pub struct Aes128Cbc {
    round_keys: [u32; 44],
}

impl Aes128Cbc {
    /// Creates a new AES-128 decryptor with a 16-byte key.
    pub fn new(key: &[u8; 16]) -> Self {
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
        for w in &mut rk[4..40] {
            *w = inv_mix_col_word(*w);
        }
        Self { round_keys: rk }
    }

    /// Decrypts a single 16-byte block in place.
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut s0 =
            u32::from_be_bytes([block[0], block[1], block[2], block[3]]) ^ self.round_keys[40];
        let mut s1 =
            u32::from_be_bytes([block[4], block[5], block[6], block[7]]) ^ self.round_keys[41];
        let mut s2 =
            u32::from_be_bytes([block[8], block[9], block[10], block[11]]) ^ self.round_keys[42];
        let mut s3 =
            u32::from_be_bytes([block[12], block[13], block[14], block[15]]) ^ self.round_keys[43];

        for round in (1..10).rev() {
            let rk = &self.round_keys[round * 4..round * 4 + 4];
            let t0 = TD0[(s0 >> 24) as usize]
                ^ TD0[((s3 >> 16) & 0xFF) as usize].rotate_right(8)
                ^ TD0[((s2 >> 8) & 0xFF) as usize].rotate_right(16)
                ^ TD0[(s1 & 0xFF) as usize].rotate_right(24)
                ^ rk[0];
            let t1 = TD0[(s1 >> 24) as usize]
                ^ TD0[((s0 >> 16) & 0xFF) as usize].rotate_right(8)
                ^ TD0[((s3 >> 8) & 0xFF) as usize].rotate_right(16)
                ^ TD0[(s2 & 0xFF) as usize].rotate_right(24)
                ^ rk[1];
            let t2 = TD0[(s2 >> 24) as usize]
                ^ TD0[((s1 >> 16) & 0xFF) as usize].rotate_right(8)
                ^ TD0[((s0 >> 8) & 0xFF) as usize].rotate_right(16)
                ^ TD0[(s3 & 0xFF) as usize].rotate_right(24)
                ^ rk[2];
            let t3 = TD0[(s3 >> 24) as usize]
                ^ TD0[((s2 >> 16) & 0xFF) as usize].rotate_right(8)
                ^ TD0[((s1 >> 8) & 0xFF) as usize].rotate_right(16)
                ^ TD0[(s0 & 0xFF) as usize].rotate_right(24)
                ^ rk[3];
            s0 = t0;
            s1 = t1;
            s2 = t2;
            s3 = t3;
        }

        let out0 = (((INV_S_BOX[(s0 >> 24) as usize] as u32) << 24)
            | ((INV_S_BOX[((s3 >> 16) & 0xFF) as usize] as u32) << 16)
            | ((INV_S_BOX[((s2 >> 8) & 0xFF) as usize] as u32) << 8)
            | (INV_S_BOX[(s1 & 0xFF) as usize] as u32))
            ^ self.round_keys[0];
        let out1 = (((INV_S_BOX[(s1 >> 24) as usize] as u32) << 24)
            | ((INV_S_BOX[((s0 >> 16) & 0xFF) as usize] as u32) << 16)
            | ((INV_S_BOX[((s3 >> 8) & 0xFF) as usize] as u32) << 8)
            | (INV_S_BOX[(s2 & 0xFF) as usize] as u32))
            ^ self.round_keys[1];
        let out2 = (((INV_S_BOX[(s2 >> 24) as usize] as u32) << 24)
            | ((INV_S_BOX[((s1 >> 16) & 0xFF) as usize] as u32) << 16)
            | ((INV_S_BOX[((s0 >> 8) & 0xFF) as usize] as u32) << 8)
            | (INV_S_BOX[(s3 & 0xFF) as usize] as u32))
            ^ self.round_keys[2];
        let out3 = (((INV_S_BOX[(s3 >> 24) as usize] as u32) << 24)
            | ((INV_S_BOX[((s2 >> 16) & 0xFF) as usize] as u32) << 16)
            | ((INV_S_BOX[((s1 >> 8) & 0xFF) as usize] as u32) << 8)
            | (INV_S_BOX[(s0 & 0xFF) as usize] as u32))
            ^ self.round_keys[3];

        block[0..4].copy_from_slice(&out0.to_be_bytes());
        block[4..8].copy_from_slice(&out1.to_be_bytes());
        block[8..12].copy_from_slice(&out2.to_be_bytes());
        block[12..16].copy_from_slice(&out3.to_be_bytes());
    }

    /// Decrypts packet buffer in-place using CBC mode.
    ///
    /// Following the RAOP specification (`components/raop/rtp.c`):
    /// Only complete 16-byte blocks (`data.len() & !0x0f`) are decrypted.
    /// Trailing bytes remain unencrypted.
    #[allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]
    pub fn decrypt_in_place(&self, iv: &[u8; 16], data: &mut [u8]) {
        let aes_len = data.len() & !0x0f;
        let mut prev_block = *iv;
        for chunk in data[..aes_len].chunks_exact_mut(16) {
            let mut block = [0u8; 16];
            block.copy_from_slice(chunk);
            let next_iv = block;
            self.decrypt_block(&mut block);
            for (dst, (&b, &p)) in chunk.iter_mut().zip(block.iter().zip(&prev_block)) {
                *dst = b ^ p;
            }
            prev_block = next_iv;
        }
    }
}

/// Airport Express 2048-bit RSA Private Key Modulus ($n$).
pub const AIRPORT_EXPRESS_N_BYTES: [u8; 256] = [
    0xe7, 0xd7, 0x44, 0xf2, 0xa2, 0xe2, 0x78, 0x8b, 0x6c, 0x1f, 0x55, 0xa0, 0x8e, 0xb7, 0x05, 0x44,
    0xa8, 0xfa, 0x79, 0x45, 0xaa, 0x8b, 0xe6, 0xc6, 0x2c, 0xe5, 0xf5, 0x1c, 0xbd, 0xd4, 0xdc, 0x68,
    0x42, 0xfe, 0x3d, 0x10, 0x83, 0xdd, 0x2e, 0xde, 0xc1, 0xbf, 0xd4, 0x25, 0x2d, 0xc0, 0x2e, 0x6f,
    0x39, 0x8b, 0xdf, 0x0e, 0x61, 0x48, 0xea, 0x84, 0x85, 0x5e, 0x2e, 0x44, 0x2d, 0xa6, 0xd6, 0x26,
    0x64, 0xf6, 0x74, 0xa1, 0xf3, 0x04, 0x92, 0x9a, 0xde, 0x4f, 0x68, 0x93, 0xef, 0x2d, 0xf6, 0xe7,
    0x11, 0xa8, 0xc7, 0x7a, 0x0d, 0x91, 0xc9, 0xd9, 0x80, 0x82, 0x2e, 0x50, 0xd1, 0x29, 0x22, 0xaf,
    0xea, 0x40, 0xea, 0x9f, 0x0e, 0x14, 0xc0, 0xf7, 0x69, 0x38, 0xc5, 0xf3, 0x88, 0x2f, 0xc0, 0x32,
    0x3d, 0xd9, 0xfe, 0x55, 0x15, 0x5f, 0x51, 0xbb, 0x59, 0x21, 0xc2, 0x01, 0x62, 0x9f, 0xd7, 0x33,
    0x52, 0xd5, 0xe2, 0xef, 0xaa, 0xbf, 0x9b, 0xa0, 0x48, 0xd7, 0xb8, 0x13, 0xa2, 0xb6, 0x76, 0x7f,
    0x6c, 0x3c, 0xcf, 0x1e, 0xb4, 0xce, 0x67, 0x3d, 0x03, 0x7b, 0x0d, 0x2e, 0xa3, 0x0c, 0x5f, 0xff,
    0xeb, 0x06, 0xf8, 0xd0, 0x8a, 0xdd, 0xe4, 0x09, 0x57, 0x1a, 0x9c, 0x68, 0x9f, 0xef, 0x10, 0x72,
    0x88, 0x55, 0xdd, 0x8c, 0xfb, 0x9a, 0x8b, 0xef, 0x5c, 0x89, 0x43, 0xef, 0x3b, 0x5f, 0xaa, 0x15,
    0xdd, 0xe6, 0x98, 0xbe, 0xdd, 0xf3, 0x59, 0x96, 0x03, 0xeb, 0x3e, 0x6f, 0x61, 0x37, 0x2b, 0xb6,
    0x28, 0xf6, 0x55, 0x9f, 0x59, 0x9a, 0x78, 0xbf, 0x50, 0x06, 0x87, 0xaa, 0x7f, 0x49, 0x76, 0xc0,
    0x56, 0x2d, 0x41, 0x29, 0x56, 0xf8, 0x98, 0x9e, 0x18, 0xa6, 0x35, 0x5b, 0xd8, 0x15, 0x97, 0x82,
    0x5e, 0x0f, 0xc8, 0x75, 0x34, 0x3e, 0xc7, 0x82, 0x11, 0x76, 0x25, 0xcd, 0xbf, 0x98, 0x44, 0x7b,
];

/// Airport Express 2048-bit RSA Private Key Exponent ($d$).
pub const AIRPORT_EXPRESS_D_BYTES: [u8; 256] = [
    0xe5, 0xf0, 0x0c, 0x72, 0xf5, 0x77, 0xd6, 0x04, 0xb9, 0xa4, 0xce, 0x41, 0x22, 0xaa, 0x84, 0xb0,
    0x17, 0x43, 0xec, 0x99, 0x5a, 0xcf, 0xcc, 0x7f, 0x4a, 0xb2, 0x7c, 0x0b, 0x18, 0x7f, 0x90, 0x66,
    0x5b, 0xe3, 0x59, 0xdf, 0x12, 0x59, 0x81, 0x8d, 0xee, 0xed, 0x79, 0xd3, 0xb1, 0xef, 0x84, 0x5e,
    0x4d, 0xdd, 0xda, 0xc9, 0xa1, 0x55, 0x37, 0x3b, 0x5e, 0x27, 0x0d, 0x8e, 0x13, 0x15, 0x00, 0x1a,
    0x2e, 0x52, 0x7d, 0x54, 0xcd, 0xf9, 0x00, 0x0a, 0x57, 0x68, 0xbc, 0x98, 0xd4, 0x44, 0x6b, 0x37,
    0xbb, 0xbd, 0x00, 0xb2, 0x9d, 0xd8, 0xb5, 0x30, 0x62, 0x13, 0x3b, 0x2a, 0x6e, 0x77, 0xf4, 0xee,
    0x32, 0x50, 0x56, 0x22, 0x90, 0x4d, 0xa7, 0x20, 0xfb, 0x1c, 0x12, 0xc0, 0x39, 0x96, 0xda, 0x71,
    0x3a, 0x05, 0x06, 0x09, 0x8e, 0xdb, 0xed, 0xec, 0xf9, 0x36, 0xd0, 0xfa, 0x9c, 0xbd, 0x59, 0x29,
    0xab, 0xb0, 0xed, 0xa3, 0x57, 0x99, 0x50, 0x2f, 0x98, 0x94, 0xdc, 0xb8, 0xfc, 0x56, 0x9a, 0x89,
    0x2d, 0x17, 0x78, 0x03, 0x24, 0xa2, 0xb6, 0xc3, 0x16, 0x6e, 0x34, 0x67, 0x09, 0x13, 0x4b, 0x85,
    0x40, 0x41, 0xb8, 0x67, 0x70, 0x6b, 0x58, 0xfe, 0xf2, 0xa0, 0xdb, 0x92, 0x2b, 0x77, 0x62, 0x8b,
    0x68, 0xe6, 0x96, 0x93, 0xc7, 0xaf, 0x43, 0xbf, 0x2a, 0x73, 0xd0, 0xb7, 0x32, 0x37, 0x7a, 0x0b,
    0xa1, 0x7b, 0x44, 0xf0, 0x51, 0xe9, 0xbf, 0x79, 0x84, 0x9d, 0xcb, 0x33, 0x32, 0x57, 0x1f, 0xd8,
    0xa7, 0x09, 0x33, 0xc2, 0xd6, 0x0b, 0xde, 0xc4, 0x79, 0x93, 0x4a, 0x3d, 0xac, 0xa4, 0x0b, 0xb6,
    0xf2, 0xf3, 0x7c, 0x0a, 0x9d, 0x07, 0x10, 0x6e, 0xad, 0xc8, 0xb3, 0x69, 0xa0, 0x3f, 0x2f, 0x41,
    0xc8, 0x80, 0x09, 0x8e, 0x8a, 0xdd, 0x46, 0x24, 0x0d, 0xac, 0x68, 0xcc, 0x53, 0x54, 0xf3, 0x61,
];

/// Returns the standard Airport Express 2048-bit RSA Private Key.
pub fn airport_express_key() -> RsaPrivateKey {
    RsaPrivateKey::from_be_bytes(&AIRPORT_EXPRESS_N_BYTES, &AIRPORT_EXPRESS_D_BYTES)
}

/// Encodes binary bytes to standard Base64 string.
pub fn base64_encode(data: &[u8]) -> String {
    const B64_TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as usize
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as usize
        } else {
            0
        };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_TABLE[(triple >> 18) & 0x3F] as char);
        out.push(B64_TABLE[(triple >> 12) & 0x3F] as char);

        if i + 1 < data.len() {
            out.push(B64_TABLE[(triple >> 6) & 0x3F] as char);
        } else {
            out.push('=');
        }

        if i + 2 < data.len() {
            out.push(B64_TABLE[triple & 0x3F] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }
    out
}

/// Decodes standard or unpadded Base64 string into binary bytes.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, CryptoError> {
    let clean: Vec<u8> = input
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace())
        .collect();

    if clean.len() % 4 == 1 {
        return Err(CryptoError::InvalidBase64);
    }

    let mut seen_padding = false;
    for &b in &clean {
        if b == b'=' {
            seen_padding = true;
        } else if seen_padding {
            return Err(CryptoError::InvalidBase64);
        }
    }

    fn b64_val(b: u8) -> Result<u32, CryptoError> {
        match b {
            b'A'..=b'Z' => Ok((b - b'A') as u32),
            b'a'..=b'z' => Ok((b - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((b - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(CryptoError::InvalidBase64),
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < clean.len() {
        if clean[i] == b'=' {
            break;
        }
        let v0 = b64_val(clean[i])?;
        let v1 = if i + 1 < clean.len() && clean[i + 1] != b'=' {
            b64_val(clean[i + 1])?
        } else {
            0
        };
        let v2 = if i + 2 < clean.len() && clean[i + 2] != b'=' {
            b64_val(clean[i + 2])?
        } else {
            0
        };
        let v3 = if i + 3 < clean.len() && clean[i + 3] != b'=' {
            b64_val(clean[i + 3])?
        } else {
            0
        };

        let triple = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;

        out.push((triple >> 16) as u8);
        if i + 1 < clean.len()
            && clean[i + 1] != b'='
            && i + 2 < clean.len()
            && clean[i + 2] != b'='
        {
            out.push((triple >> 8) as u8);
        }
        if i + 2 < clean.len()
            && clean[i + 2] != b'='
            && i + 3 < clean.len()
            && clean[i + 3] != b'='
        {
            out.push(triple as u8);
        }

        i += 4;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_unpad_pkcs1_v15_corrupted_prefix() {
        let mut pad = [0xFFu8; 32];
        pad[0] = 0x01; // corrupted byte 0
        pad[1] = 0x02;
        pad[15] = 0x00; // sep
        assert_eq!(
            unpad_pkcs1_v15_type2(&pad),
            Err(CryptoError::InvalidPadding)
        );
    }

    #[test]
    fn test_constant_time_unpad_oaep_corrupted_prefix() {
        let mut oaep = [0u8; 64];
        oaep[0] = 0x01; // corrupted byte 0
        assert_eq!(unpad_oaep_sha1(&oaep), Err(CryptoError::InvalidPadding));
    }

    #[test]
    fn test_montgomery_mod_pow_fixed_bit_len() {
        let base = BigUint::from_u32(3);
        let exp1 = BigUint::from_u32(5);
        let modulus = BigUint::from_u32(17);
        let res1 = montgomery_mod_pow(&base, &exp1, &modulus).expect("mod pow succeeds");
        assert_eq!(res1, BigUint::from_u32(5)); // 3^5 = 243 = 14*17 + 5 = 5 mod 17
    }

    #[test]
    fn test_aes128_fips197_appendix_b_decrypt_block() {
        let key = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let mut block = [
            0x39, 0x25, 0x84, 0x1d, 0x02, 0xdc, 0x09, 0xfb, 0xdc, 0x11, 0x85, 0x97, 0x19, 0x6a,
            0x0b, 0x32,
        ];
        let expected = [
            0x32, 0x43, 0xf6, 0xa8, 0x88, 0x5a, 0x30, 0x8d, 0x31, 0x31, 0x98, 0xa2, 0xe0, 0x37,
            0x07, 0x34,
        ];
        let cipher = Aes128Cbc::new(&key);
        cipher.decrypt_block(&mut block);
        assert_eq!(block, expected);
    }

    #[test]
    fn test_aes128_cbc_nist_sp800_38a_f22_and_raop_tail() {
        let key = [
            0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf,
            0x4f, 0x3c,
        ];
        let iv = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let mut data = [
            // Block 1
            0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b, 0x12, 0xe9,
            0x19, 0x7d, // Block 2
            0x50, 0x86, 0xcb, 0x9b, 0x50, 0x72, 0x19, 0xee, 0x95, 0xdb, 0x11, 0x3a, 0x91, 0x76,
            0x78, 0xb2, // Block 3
            0x73, 0xbe, 0xd6, 0xb8, 0xe3, 0xc1, 0x74, 0x3b, 0x71, 0x16, 0xe6, 0x9e, 0x22, 0x22,
            0x95, 0x16, // Block 4
            0x3f, 0xf1, 0xca, 0xa1, 0x68, 0x1f, 0xac, 0x09, 0x12, 0x0e, 0xca, 0x30, 0x75, 0x86,
            0xe1, 0xa7, // Trailing 5 unencrypted bytes
            0xde, 0xad, 0xbe, 0xef, 0x42,
        ];
        let expected_prefix = [
            // Block 1
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a, // Block 2
            0xae, 0x2d, 0x8a, 0x57, 0x1e, 0x03, 0xac, 0x9c, 0x9e, 0xb7, 0x6f, 0xac, 0x45, 0xaf,
            0x8e, 0x51, // Block 3
            0x30, 0xc8, 0x1c, 0x46, 0xa3, 0x5c, 0xe4, 0x11, 0xe5, 0xfb, 0xc1, 0x19, 0x1a, 0x0a,
            0x52, 0xef, // Block 4
            0xf6, 0x9f, 0x24, 0x45, 0xdf, 0x4f, 0x9b, 0x17, 0xad, 0x2b, 0x41, 0x7b, 0xe6, 0x6c,
            0x37, 0x10,
        ];
        let expected_tail = [0xde, 0xad, 0xbe, 0xef, 0x42];

        let cipher = Aes128Cbc::new(&key);
        cipher.decrypt_in_place(&iv, &mut data);
        assert_eq!(&data[..64], &expected_prefix);
        assert_eq!(&data[64..], &expected_tail);
    }
}
