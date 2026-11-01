//! `libm`-backed float methods for the `no_std` build (WO-199).
//!
//! `core` lacks the inherent `f32`/`f64` methods that call the platform libm
//! (`sqrt`, `floor`, `exp2`, ...). With `std` on, the inherent methods win
//! method resolution and these traits are never imported.
#![cfg_attr(feature = "std", allow(dead_code))]

/// `f32` math the decoder calls with method syntax.
pub(crate) trait F32Math: Sized {
    fn sqrt(self) -> Self;
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn trunc(self) -> Self;
    fn exp2(self) -> Self;
    fn powf(self, n: Self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn ln(self) -> Self;
    fn log2(self) -> Self;
    fn log10(self) -> Self;
    fn cbrt(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn sin_cos(self) -> (Self, Self);
    fn rem_euclid(self, rhs: Self) -> Self;
    fn div_euclid(self, rhs: Self) -> Self;
}

/// `f64` math the decoder calls with method syntax.
pub(crate) trait F64Math: Sized {
    fn sqrt(self) -> Self;
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn trunc(self) -> Self;
    fn exp2(self) -> Self;
    fn powf(self, n: Self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn ln(self) -> Self;
    fn log2(self) -> Self;
    fn log10(self) -> Self;
    fn cbrt(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn sin_cos(self) -> (Self, Self);
    fn rem_euclid(self, rhs: Self) -> Self;
    fn div_euclid(self, rhs: Self) -> Self;
}

/// Repeated multiplication, matching LLVM's `powi` lowering order closely
/// enough for exact powers of two (the only bit-sensitive decoder use).
fn powi_f64(x: f64, n: i32) -> f64 {
    let mut base = x;
    let mut e = n.unsigned_abs();
    let mut acc = 1.0f64;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        base *= base;
        e >>= 1;
    }
    if n < 0 { 1.0 / acc } else { acc }
}

fn powi_f32(x: f32, n: i32) -> f32 {
    let mut base = x;
    let mut e = n.unsigned_abs();
    let mut acc = 1.0f32;
    while e > 0 {
        if e & 1 == 1 {
            acc *= base;
        }
        base *= base;
        e >>= 1;
    }
    if n < 0 { 1.0 / acc } else { acc }
}

impl F32Math for f32 {
    fn sqrt(self) -> Self {
        libm::sqrtf(self)
    }
    fn floor(self) -> Self {
        libm::floorf(self)
    }
    fn ceil(self) -> Self {
        libm::ceilf(self)
    }
    fn round(self) -> Self {
        libm::roundf(self)
    }
    fn trunc(self) -> Self {
        libm::truncf(self)
    }
    fn exp2(self) -> Self {
        libm::exp2f(self)
    }
    fn powf(self, n: Self) -> Self {
        libm::powf(self, n)
    }
    fn powi(self, n: i32) -> Self {
        powi_f32(self, n)
    }
    fn ln(self) -> Self {
        libm::logf(self)
    }
    fn log2(self) -> Self {
        libm::log2f(self)
    }
    fn log10(self) -> Self {
        libm::log10f(self)
    }
    fn cbrt(self) -> Self {
        libm::cbrtf(self)
    }
    fn sin(self) -> Self {
        libm::sinf(self)
    }
    fn cos(self) -> Self {
        libm::cosf(self)
    }
    fn sin_cos(self) -> (Self, Self) {
        libm::sincosf(self)
    }
    fn rem_euclid(self, rhs: Self) -> Self {
        let r = libm::fmodf(self, rhs);
        if r < 0.0 { r + rhs.abs() } else { r }
    }
    fn div_euclid(self, rhs: Self) -> Self {
        let q = libm::truncf(self / rhs);
        if libm::fmodf(self, rhs) < 0.0 {
            if rhs > 0.0 { q - 1.0 } else { q + 1.0 }
        } else {
            q
        }
    }
}

impl F64Math for f64 {
    fn sqrt(self) -> Self {
        libm::sqrt(self)
    }
    fn floor(self) -> Self {
        libm::floor(self)
    }
    fn ceil(self) -> Self {
        libm::ceil(self)
    }
    fn round(self) -> Self {
        libm::round(self)
    }
    fn trunc(self) -> Self {
        libm::trunc(self)
    }
    fn exp2(self) -> Self {
        libm::exp2(self)
    }
    fn powf(self, n: Self) -> Self {
        libm::pow(self, n)
    }
    fn powi(self, n: i32) -> Self {
        powi_f64(self, n)
    }
    fn ln(self) -> Self {
        libm::log(self)
    }
    fn log2(self) -> Self {
        libm::log2(self)
    }
    fn log10(self) -> Self {
        libm::log10(self)
    }
    fn cbrt(self) -> Self {
        libm::cbrt(self)
    }
    fn sin(self) -> Self {
        libm::sin(self)
    }
    fn cos(self) -> Self {
        libm::cos(self)
    }
    fn sin_cos(self) -> (Self, Self) {
        libm::sincos(self)
    }
    fn rem_euclid(self, rhs: Self) -> Self {
        let r = libm::fmod(self, rhs);
        if r < 0.0 { r + rhs.abs() } else { r }
    }
    fn div_euclid(self, rhs: Self) -> Self {
        let q = libm::trunc(self / rhs);
        if libm::fmod(self, rhs) < 0.0 {
            if rhs > 0.0 { q - 1.0 } else { q + 1.0 }
        } else {
            q
        }
    }
}
