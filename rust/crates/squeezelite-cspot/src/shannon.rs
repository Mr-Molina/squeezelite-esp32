//! Pure-Rust Shannon stream cipher & MAC implementation.
//!
//! Matches upstream `Shannon.cpp` in `components/spotify/cspot/`.
//! Designed for `#![no_std]` environments with zero dynamic allocations.

/// Number of 32-bit registers in the Shannon state.
pub const SHANNON_N: usize = 16;
const FOLD: usize = SHANNON_N;
const INITKONST: u32 = 0x6996c53a;
const KEYP: usize = 13;

/// Shannon stream cipher and message authentication code (MAC) context.
#[derive(Clone, Eq, PartialEq)]
pub struct Shannon {
    r: [u32; SHANNON_N],
    crc: [u32; SHANNON_N],
    init_r: [u32; SHANNON_N],
    konst: u32,
    sbuf: u32,
    mbuf: u32,
    nbuf: i32,
    r_head: usize,
    crc_head: usize,
}

impl core::fmt::Debug for Shannon {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Shannon")
            .field("r", &"[REDACTED]")
            .field("crc", &"[REDACTED]")
            .field("init_r", &"[REDACTED]")
            .field("konst", &"[REDACTED]")
            .field("sbuf", &"[REDACTED]")
            .field("mbuf", &"[REDACTED]")
            .field("nbuf", &"[REDACTED]")
            .finish()
    }
}

impl Drop for Shannon {
    fn drop(&mut self) {
        self.r.fill(0);
        core::hint::black_box(&mut self.r);
        self.crc.fill(0);
        core::hint::black_box(&mut self.crc);
        self.init_r.fill(0);
        core::hint::black_box(&mut self.init_r);
        self.konst = 0;
        core::hint::black_box(&mut self.konst);
        self.sbuf = 0;
        core::hint::black_box(&mut self.sbuf);
        self.mbuf = 0;
        core::hint::black_box(&mut self.mbuf);
        self.nbuf = 0;
        core::hint::black_box(&mut self.nbuf);
        self.r_head = 0;
        core::hint::black_box(&mut self.r_head);
        self.crc_head = 0;
        core::hint::black_box(&mut self.crc_head);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for Shannon {
    fn default() -> Self {
        Self::new()
    }
}

impl Shannon {
    /// Creates a new uninitialized Shannon cipher state.
    pub const fn new() -> Self {
        Self {
            r: [0u32; SHANNON_N],
            crc: [0u32; SHANNON_N],
            init_r: [0u32; SHANNON_N],
            konst: 0,
            sbuf: 0,
            mbuf: 0,
            nbuf: 0,
            r_head: 0,
            crc_head: 0,
        }
    }

    #[inline(always)]
    fn r_get(&self, i: usize) -> u32 {
        self.r[(self.r_head + i) & 0x0F]
    }

    #[inline(always)]
    fn r_xor(&mut self, i: usize, val: u32) {
        self.r[(self.r_head + i) & 0x0F] ^= val;
    }

    #[inline(always)]
    fn crc_get(&self, i: usize) -> u32 {
        self.crc[(self.crc_head + i) & 0x0F]
    }

    #[inline(always)]
    fn sbox1(mut w: u32) -> u32 {
        w ^= w.rotate_left(5) | w.rotate_left(7);
        w ^= w.rotate_left(19) | w.rotate_left(22);
        w
    }

    #[inline(always)]
    fn sbox2(mut w: u32) -> u32 {
        w ^= w.rotate_left(7) | w.rotate_left(22);
        w ^= w.rotate_left(5) | w.rotate_left(19);
        w
    }

    #[inline(always)]
    fn cycle(&mut self) {
        let mut t = self.r_get(12) ^ self.r_get(13) ^ self.konst;
        t = Self::sbox1(t) ^ self.r_get(0).rotate_left(1);
        self.r_head = (self.r_head + 1) & 0x0F;
        self.r[(self.r_head + 15) & 0x0F] = t;
        let t2 = Self::sbox2(self.r_get(2) ^ self.r_get(15));
        self.r_xor(0, t2);
        self.sbuf = t2 ^ self.r_get(8) ^ self.r_get(12);
    }

    #[inline(always)]
    fn crcfunc(&mut self, i: u32) {
        let t = self.crc_get(0) ^ self.crc_get(2) ^ self.crc_get(15) ^ i;
        self.crc_head = (self.crc_head + 1) & 0x0F;
        self.crc[(self.crc_head + 15) & 0x0F] = t;
    }

    #[inline(always)]
    fn macfunc(&mut self, i: u32) {
        self.crcfunc(i);
        self.r_xor(KEYP, i);
    }

    fn init_state(&mut self) {
        self.r_head = 0;
        self.crc_head = 0;
        self.r[0] = 1;
        self.r[1] = 1;
        for i in 2..SHANNON_N {
            self.r[i] = self.r[i - 1].wrapping_add(self.r[i - 2]);
        }
        self.konst = INITKONST;
    }

    fn save_state(&mut self) {
        for i in 0..SHANNON_N {
            self.init_r[i] = self.r_get(i);
        }
    }

    fn reload_state(&mut self) {
        self.r.copy_from_slice(&self.init_r);
        self.r_head = 0;
        self.crc_head = 0;
    }

    fn genkonst(&mut self) {
        self.konst = self.r_get(0);
    }

    fn diffuse(&mut self) {
        for _ in 0..FOLD {
            self.cycle();
        }
    }

    fn load_key(&mut self, key: &[u8]) {
        let keylen = key.len();
        let word_len = keylen & !0x3;
        let mut i = 0;
        while i < word_len {
            let k = u32::from_le_bytes([key[i], key[i + 1], key[i + 2], key[i + 3]]);
            self.r_xor(KEYP, k);
            self.cycle();
            i += 4;
        }

        if i < keylen {
            let mut xtra = [0u8; 4];
            let mut j = 0;
            while i < keylen {
                xtra[j] = key[i];
                j += 1;
                i += 1;
            }
            let k = u32::from_le_bytes(xtra);
            self.r_xor(KEYP, k);
            self.cycle();
        }

        self.r_xor(KEYP, keylen as u32);
        self.cycle();

        for idx in 0..SHANNON_N {
            self.crc[idx] = self.r_get(idx);
        }
        self.crc_head = 0;
        self.diffuse();

        for idx in 0..SHANNON_N {
            let crc_val = self.crc_get(idx);
            self.r_xor(idx, crc_val);
        }
    }

    /// Sets key and initializes cipher state.
    pub fn key(&mut self, key_bytes: &[u8]) {
        self.init_state();
        self.load_key(key_bytes);
        self.genkonst();
        self.save_state();
        self.sbuf = 0;
        self.mbuf = 0;
        self.nbuf = 0;
    }

    /// Sets initialization vector (nonce).
    pub fn nonce(&mut self, nonce_bytes: &[u8]) {
        self.reload_state();
        self.konst = INITKONST;
        self.load_key(nonce_bytes);
        self.genkonst();
        self.sbuf = 0;
        self.mbuf = 0;
        self.nbuf = 0;
    }

    /// Generates pseudorandom keystream XORed into `buf`.
    pub fn stream(&mut self, buf: &mut [u8]) {
        let mut pos = 0;
        let mut nbytes = buf.len();

        while self.nbuf != 0 && nbytes != 0 {
            buf[pos] ^= (self.sbuf & 0xFF) as u8;
            pos += 1;
            self.sbuf >>= 8;
            self.nbuf -= 8;
            nbytes -= 1;
        }

        let end_words = pos + (nbytes & !0x03);
        while pos < end_words {
            self.cycle();
            let word = self.sbuf.to_le_bytes();
            buf[pos] ^= word[0];
            buf[pos + 1] ^= word[1];
            buf[pos + 2] ^= word[2];
            buf[pos + 3] ^= word[3];
            pos += 4;
        }

        nbytes &= 0x03;
        if nbytes != 0 {
            self.cycle();
            self.nbuf = 32;
            while self.nbuf != 0 && nbytes != 0 {
                buf[pos] ^= (self.sbuf & 0xFF) as u8;
                pos += 1;
                self.sbuf >>= 8;
                self.nbuf -= 8;
                nbytes -= 1;
            }
        }
    }

    /// Accumulates MAC without encrypting.
    pub fn maconly(&mut self, buf: &[u8]) {
        let mut pos = 0;
        let mut nbytes = buf.len();

        if self.nbuf != 0 {
            while self.nbuf != 0 && nbytes != 0 {
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
            if self.nbuf != 0 {
                return;
            }
            self.macfunc(self.mbuf);
        }

        let end_words = pos + (nbytes & !0x03);
        while pos < end_words {
            self.cycle();
            let t = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
            self.macfunc(t);
            pos += 4;
        }

        nbytes &= 0x03;
        if nbytes != 0 {
            self.cycle();
            self.mbuf = 0;
            self.nbuf = 32;
            while self.nbuf != 0 && nbytes != 0 {
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
        }
    }

    /// Encrypts plaintext in-place and accumulates MAC.
    pub fn encrypt(&mut self, buf: &mut [u8]) {
        let mut pos = 0;
        let mut nbytes = buf.len();

        if self.nbuf != 0 {
            while self.nbuf != 0 && nbytes != 0 {
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                buf[pos] ^= ((self.sbuf >> (32 - self.nbuf)) & 0xFF) as u8;
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
            if self.nbuf != 0 {
                return;
            }
            self.macfunc(self.mbuf);
        }

        let end_words = pos + (nbytes & !0x03);
        while pos < end_words {
            self.cycle();
            let mut t = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
            self.macfunc(t);
            t ^= self.sbuf;
            let out_bytes = t.to_le_bytes();
            buf[pos..pos + 4].copy_from_slice(&out_bytes);
            pos += 4;
        }

        nbytes &= 0x03;
        if nbytes != 0 {
            self.cycle();
            self.mbuf = 0;
            self.nbuf = 32;
            while self.nbuf != 0 && nbytes != 0 {
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                buf[pos] ^= ((self.sbuf >> (32 - self.nbuf)) & 0xFF) as u8;
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
        }
    }

    /// Decrypts ciphertext in-place and accumulates MAC.
    pub fn decrypt(&mut self, buf: &mut [u8]) {
        let mut pos = 0;
        let mut nbytes = buf.len();

        if self.nbuf != 0 {
            while self.nbuf != 0 && nbytes != 0 {
                buf[pos] ^= ((self.sbuf >> (32 - self.nbuf)) & 0xFF) as u8;
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
            if self.nbuf != 0 {
                return;
            }
            self.macfunc(self.mbuf);
        }

        let end_words = pos + (nbytes & !0x03);
        while pos < end_words {
            self.cycle();
            let t = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
                ^ self.sbuf;
            self.macfunc(t);
            let out_bytes = t.to_le_bytes();
            buf[pos..pos + 4].copy_from_slice(&out_bytes);
            pos += 4;
        }

        nbytes &= 0x03;
        if nbytes != 0 {
            self.cycle();
            self.mbuf = 0;
            self.nbuf = 32;
            while self.nbuf != 0 && nbytes != 0 {
                buf[pos] ^= ((self.sbuf >> (32 - self.nbuf)) & 0xFF) as u8;
                self.mbuf ^= (buf[pos] as u32) << (32 - self.nbuf);
                pos += 1;
                self.nbuf -= 8;
                nbytes -= 1;
            }
        }
    }

    /// Emits final MAC into `buf` (typically 4 bytes).
    pub fn finish(&mut self, buf: &mut [u8]) {
        let mut pos = 0;
        let mut nbytes = buf.len();

        if self.nbuf != 0 {
            self.macfunc(self.mbuf);
        }

        self.cycle();
        self.r_xor(KEYP, INITKONST ^ ((self.nbuf as u32) << 3));
        self.nbuf = 0;

        for i in 0..SHANNON_N {
            let crc_val = self.crc_get(i);
            self.r_xor(i, crc_val);
        }
        self.diffuse();

        while nbytes > 0 {
            self.cycle();
            if nbytes >= 4 {
                let w = self.sbuf.to_le_bytes();
                buf[pos..pos + 4].copy_from_slice(&w);
                pos += 4;
                nbytes -= 4;
            } else {
                let w = self.sbuf.to_le_bytes();
                buf[pos..pos + nbytes].copy_from_slice(&w[..nbytes]);
                break;
            }
        }
    }

    /// Computes final MAC and verifies against `expected` in constant time.
    ///
    /// Returns `true` if and only if the generated MAC matches `expected`.
    pub fn finish_and_verify(&mut self, expected: &[u8]) -> bool {
        let mut diff = 0u8;
        let mut pos = 0;
        let mut nbytes = expected.len();

        if self.nbuf != 0 {
            self.macfunc(self.mbuf);
        }

        self.cycle();
        self.r_xor(KEYP, INITKONST ^ ((self.nbuf as u32) << 3));
        self.nbuf = 0;

        for i in 0..SHANNON_N {
            let crc_val = self.crc_get(i);
            self.r_xor(i, crc_val);
        }
        self.diffuse();

        while nbytes > 0 {
            self.cycle();
            let w = self.sbuf.to_le_bytes();
            let take = core::cmp::min(4, nbytes);
            for i in 0..take {
                diff |= expected[pos + i] ^ w[i];
            }
            pos += take;
            nbytes -= take;
        }

        diff == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;

    #[test]
    fn test_finish_and_verify() {
        let key = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut s1 = Shannon::new();
        s1.key(&key);
        s1.nonce(&[0x10, 0x20]);
        let mut msg1 = *b"test payload message";
        s1.encrypt(&mut msg1);
        let mut mac = [0u8; 4];
        s1.finish(&mut mac);

        let mut s2 = Shannon::new();
        s2.key(&key);
        s2.nonce(&[0x10, 0x20]);
        let mut msg2 = *b"test payload message";
        s2.encrypt(&mut msg2);

        // Matches valid MAC
        assert!(s2.finish_and_verify(&mac));

        // Corrupted MAC fails
        let mut s3 = Shannon::new();
        s3.key(&key);
        s3.nonce(&[0x10, 0x20]);
        let mut msg3 = *b"test payload message";
        s3.encrypt(&mut msg3);
        let mut bad_mac = mac;
        bad_mac[0] ^= 0x01;
        assert!(!s3.finish_and_verify(&bad_mac));
    }

    #[test]
    fn test_shannon_nonce_clears_state() {
        let key = [0xaa; 16];
        let mut s = Shannon::new();
        s.key(&key);

        // Partially consume keystream to populate sbuf / nbuf
        let mut stream_buf = [0u8; 3];
        s.stream(&mut stream_buf);
        assert_ne!(s.nbuf, 0);

        // Call nonce() - must clear sbuf, mbuf, nbuf
        s.nonce(&[0x01, 0x02, 0x03]);
        assert_eq!(s.sbuf, 0);
        assert_eq!(s.mbuf, 0);
        assert_eq!(s.nbuf, 0);
    }

    #[test]
    fn test_shannon_debug_redacted() {
        let key = [0x55; 16];
        let mut s = Shannon::new();
        s.key(&key);
        let debug_str = alloc::format!("{s:?}");
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("0x6996c53a"));
    }
}
