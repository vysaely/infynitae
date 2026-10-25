//! Infynitae Native trusted shell IPC (FOUNDATION_ARCHITECTURE C7).
//!
//! The only holder of `ShellPort` (C7.1, C7.4). Depends on Core alone: the JSON-lines protocol is
//! written here (owner decision 2026-10-20, D2), and the socket, its admission check and the loop
//! wake-up are given to it by the composition root.
//!
//! - [`json`]: the value, a strict parser and the writer.
//! - [`protocol`]: requests, replies and events.
//! - [`model`]: the shell's derived copy (Snapshot) and its diff (events).
//! - [`server`]: connections, ordering, generations, subscribers.

#![forbid(unsafe_code)]

pub mod json;
pub mod model;
pub mod protocol;
pub mod server;

pub use server::{Admit, Serviced, ShellServer, Wake};

/// INV-ARCH-2: `unsafe` is forbidden here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}
