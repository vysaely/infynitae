//! Infynitae Native trusted shell IPC (FOUNDATION_ARCHITECTURE C7).
//!
//! INC-0 state: empty crate; it exists so the dependency and `unsafe` rules can be enforced
//! (INV-ARCH-1, INV-ARCH-2). The shell protocol arrives with INC-11.

#![forbid(unsafe_code)]

/// INV-ARCH-2: `unsafe` is forbidden here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}
