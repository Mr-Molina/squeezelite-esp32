//! Lazily built, heap-backed statics for `no_std + alloc` (WO-199).
//!
//! Replaces upstream `std::sync::LazyLock` / `std::sync::OnceLock` with
//! `once_cell::race::OnceBox` (core `AtomicPtr` CAS + `alloc::boxed::Box`).
//! Two racing initializers may both run; the loser's box is dropped (freed)
//! and every caller observes the winner. All syom initializers are pure
//! functions of constants, so the observed value is deterministic. The value
//! lives on the heap (PSRAM on ESP32), not in `.bss`.

use alloc::boxed::Box;
use core::ops::Deref;
use once_cell::race::OnceBox;

/// `LazyLock` replacement: `Deref` builds the value on first access.
pub struct LazyBox<T> {
    cell: OnceBox<T>,
    init: fn() -> T,
}

impl<T> LazyBox<T> {
    /// A lazy cell that runs `init` on first dereference.
    pub const fn new(init: fn() -> T) -> Self {
        Self { cell: OnceBox::new(), init }
    }
}

impl<T> Deref for LazyBox<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.cell.get_or_init(|| Box::new((self.init)()))
    }
}

/// `OnceLock` replacement with the `get_or_init` subset syom uses.
pub struct OnceBoxCell<T> {
    cell: OnceBox<T>,
}

impl<T> OnceBoxCell<T> {
    /// An empty cell.
    pub const fn new() -> Self {
        Self { cell: OnceBox::new() }
    }

    /// The stored value, building it with `f` if the cell is empty.
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.cell.get_or_init(|| Box::new(f()))
    }
}

impl<T> Default for OnceBoxCell<T> {
    fn default() -> Self {
        Self::new()
    }
}
