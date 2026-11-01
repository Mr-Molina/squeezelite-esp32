//! Names the `std` prelude provides that a `no_std + alloc` build must import
//! explicitly (WO-199). Every module does
//! `#[allow(unused_imports)] use crate::nostd_prelude::*;`; with `std` on the
//! re-exports are the very same types, so behavior is unchanged.
#![allow(unused_imports)]

pub(crate) use alloc::borrow::ToOwned;
pub(crate) use alloc::boxed::Box;
pub(crate) use alloc::string::{String, ToString};
pub(crate) use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
pub(crate) use crate::fmath::{F32Math, F64Math};
