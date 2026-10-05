//! Infynitae Native Smithay adapter (FOUNDATION_ARCHITECTURE C1.2).
//!
//! INC-0 state: the protocol-state shape and its `Send` assertion only. No protocol handler, global
//! or Smithay integration exists yet (INC-1 onwards).
//!
//! Every Smithay handler receives `&mut` protocol state. That state holds the Core but never an
//! authority port or the output-lifecycle token: those types are `!Send` and the state is asserted
//! `Send` below, so storing one here is a compile error (C7.4, E0277). tools/verify-architecture
//! also rejects any mention of the authority entry points in this crate's sources.

#![deny(unsafe_code)]

use infynitae_core::Core;

/// Protocol state handed to every Smithay handler.
pub struct WaylandState {
    #[expect(
        dead_code,
        reason = "INC-0 shape; read by protocol handlers from INC-1"
    )]
    core: Core,
    #[cfg(feature = "probe_store_port_in_state")]
    leaked_port: infynitae_core::InputPort,
    #[cfg(feature = "probe_store_output_token_in_state")]
    leaked_token: infynitae_core::OutputLifecycleToken,
}

/// C7.4 / EC-15: protocol state must be `Send`.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<WaylandState>();
};

/// INV-ARCH-2: `unsafe` is denied here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}
