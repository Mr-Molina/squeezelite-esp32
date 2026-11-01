//! Floating point math support for `no_std + alloc` environments using `libm`.

#[inline(always)]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline(always)]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

#[inline(always)]
pub fn atan(x: f32) -> f32 {
    libm::atanf(x)
}

#[inline(always)]
pub fn floor(x: f32) -> f32 {
    libm::floorf(x)
}

#[inline(always)]
pub fn sqrt(x: f32) -> f32 {
    libm::sqrtf(x)
}

#[allow(dead_code)]
#[inline(always)]
pub fn abs(x: f32) -> f32 {
    libm::fabsf(x)
}

#[inline(always)]
pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}

#[allow(dead_code)]
#[inline(always)]
pub fn exp2(x: f32) -> f32 {
    libm::exp2f(x)
}

#[allow(dead_code)]
pub trait F32Ext {
    fn sin(self) -> f32;
    fn cos(self) -> f32;
    fn atan(self) -> f32;
    fn floor(self) -> f32;
    fn sqrt(self) -> f32;
    fn abs(self) -> f32;
    fn exp(self) -> f32;
    fn exp2(self) -> f32;
}

impl F32Ext for f32 {
    #[inline(always)]
    fn sin(self) -> f32 {
        libm::sinf(self)
    }
    #[inline(always)]
    fn cos(self) -> f32 {
        libm::cosf(self)
    }
    #[inline(always)]
    fn atan(self) -> f32 {
        libm::atanf(self)
    }
    #[inline(always)]
    fn floor(self) -> f32 {
        libm::floorf(self)
    }
    #[inline(always)]
    fn sqrt(self) -> f32 {
        libm::sqrtf(self)
    }
    #[inline(always)]
    fn abs(self) -> f32 {
        libm::fabsf(self)
    }
    #[inline(always)]
    fn exp(self) -> f32 {
        libm::expf(self)
    }
    #[inline(always)]
    fn exp2(self) -> f32 {
        libm::exp2f(self)
    }
}
