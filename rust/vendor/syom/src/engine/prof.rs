//! Optional stage timers. Inactive unless the process has `SYOM_PROF` set.
//! WO-199: without `std` there is no clock or environment, so the timers are
//! permanently off and compile to nothing.

#[allow(unused_imports)]
use crate::nostd_prelude::*;
#[cfg(feature = "std")]
use std::time::Instant;

/// Stand-in timestamp type for the `no_std` build (never constructed).
#[cfg(not(feature = "std"))]
#[derive(Clone, Copy)]
pub(crate) struct Instant;

#[cfg(feature = "std")]
pub(crate) fn on() -> bool {
    static ON: crate::lazy::OnceBoxCell<bool> = crate::lazy::OnceBoxCell::new();
    *ON.get_or_init(|| std::env::var_os("SYOM_PROF").is_some())
}

#[cfg(not(feature = "std"))]
pub(crate) fn on() -> bool {
    false
}

#[cfg(feature = "std")]
pub(crate) fn stamp() -> Option<Instant> {
    on().then(Instant::now)
}

#[cfg(not(feature = "std"))]
pub(crate) fn stamp() -> Option<Instant> {
    None
}

#[cfg(feature = "std")]
pub(crate) fn ns(t: Option<Instant>) -> u64 {
    match t {
        Some(t) => u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX),
        None => 0,
    }
}

#[cfg(not(feature = "std"))]
pub(crate) fn ns(_t: Option<Instant>) -> u64 {
    0
}
