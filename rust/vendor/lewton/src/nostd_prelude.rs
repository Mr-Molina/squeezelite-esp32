//! Prelude for `no_std + alloc` support.

pub use alloc::{
    boxed::Box,
    string::String,
    vec,
    vec::Vec,
};

pub(crate) use crate::fmath;
#[allow(unused_imports)]
pub use crate::fmath::F32Ext;
