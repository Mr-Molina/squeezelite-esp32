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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shannon {
    r: [u32; SHANNON_N],
    crc: [u32; SHANNON_N],
    init_r: [u32; SHANNON_N],
    konst: u32,
    sbuf: u32,
    mbuf: u32,
    nbuf: i32,
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
        }
    }

    fn sbox1(mut w: u32) -> u32 {
        w ^= w.rotate_left(5) | w.rotate_left(7);
        w ^= w.rotate_left(19) | w.rotate_left(22);
        w
    }

    fn sbox2(mut w: u32) -> u32 {
        w ^= w.rotate_left(7) | w.rotate_left(22);
        w ^= w.rotate_left(5) | w.rotate_left(19);
        w
    }

    fn cycle(&mut self) {
        let mut t = self.r[12] ^ self.r[13] ^ self.konst;
        t = Self::sbox1(t) ^ self.r[0].rotate_left(1);
        for i in 1..SHANNON_N {
            self.r[i - 1] = self.r[i];
        }
        self.r[SHANNON_N - 1] = t;
        let t2 = Self::sbox2(self.r[2] ^ self.r[15]);
        self.r[0] ^= t2;
        self.sbuf = t2 ^ self.r[8] ^ self.r[12];
    }

    fn crcfunc(&mut self, i: u32) {
        let t = self.crc[0] ^ self.crc[2] ^ self.crc[15] ^ i;
        for j in 1..SHANNON_N {
            self.crc[j - 1] = self.crc[j];
        }
        self.crc[SHANNON_N - 1] = t;
    }

    fn macfunc(&mut self, i: u32) {
        self.crcfunc(i);
        self.r[KEYP] ^= i;
    }

    fn init_state(&mut self) {
        self.r[0] = 1;
        self.r[1] = 1;
        for i in 2..SHANNON_N {
            self.r[i] = self.r[i - 1].wrapping_add(self.r[i - 2]);
        }
        self.konst = INITKONST;
    }

    fn save_state(&mut self) {
        self.init_r.copy_from_slice(&self.r);
    }

    fn reload_state(&mut self) {
        self.r.copy_from_slice(&self.init_r);
    }

    fn genkonst(&mut self) {
        self.konst = self.r[0];
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
            self.r[KEYP] ^= k;
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
            self.r[KEYP] ^= k;
            self.cycle();
        }

        self.r[KEYP] ^= keylen as u32;
        self.cycle();

        self.crc.copy_from_slice(&self.r);
        self.diffuse();

        for idx in 0..SHANNON_N {
            self.r[idx] ^= self.crc[idx];
        }
    }

    /// Sets key and initializes cipher state.
    pub fn key(&mut self, key_bytes: &[u8]) {
        self.init_state();
        self.load_key(key_bytes);
        self.genkonst();
        self.save_state();
        self.nbuf = 0;
    }

    /// Sets initialization vector (nonce).
    pub fn nonce(&mut self, nonce_bytes: &[u8]) {
        self.reload_state();
        self.konst = INITKONST;
        self.load_key(nonce_bytes);
        self.genkonst();
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
            let t = u32::from_le_bytes([
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
            ]);
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
            let mut t = u32::from_le_bytes([
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
            ]);
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
            let t = u32::from_le_bytes([
                buf[pos],
                buf[pos + 1],
                buf[pos + 2],
                buf[pos + 3],
            ]) ^ self.sbuf;
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
        self.r[KEYP] ^= INITKONST ^ ((self.nbuf as u32) << 3);
        self.nbuf = 0;

        for i in 0..SHANNON_N {
            self.r[i] ^= self.crc[i];
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
}
