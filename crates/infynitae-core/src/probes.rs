//! Core-internal negative probes (OB-19). Each item is compiled only under its feature and
//! stands for policy code in a core module other than `navigation` and `entry` (for example the
//! future protocol-event policy). tools/verify-architecture requires each build to fail with the
//! asserted diagnostic and no other error.

#![allow(dead_code)]

use crate::Core;

/// Protocol policy moving a camera: `set_center` is private to `navigation` (E0624).
#[cfg(feature = "probe_protocol_moves_camera")]
fn protocol_moves_camera(core: &mut Core, proof: crate::authority::Authorized) {
    core.cameras
        .set_center(proof, 0, crate::geom::CanvasPos { x: 9, y: 9 });
}

/// PD-F2-01: protocol policy re-fitting a camera on a client commit needs a fresh proof, which
/// cannot be minted outside `entry` (E0624 on `mint`).
#[cfg(feature = "probe_protocol_refits_camera")]
fn protocol_refits_camera(_core: &mut Core) -> crate::authority::Authorized {
    crate::authority::Authorized::mint()
}

/// PD-F2-18: protocol policy creating or binding a camera needs a fresh proof (E0624 on `mint`).
#[cfg(feature = "probe_protocol_creates_camera")]
fn protocol_creates_camera(_core: &mut Core) -> crate::authority::Authorized {
    crate::authority::Authorized::mint()
}

/// INV-ARCH-2: `unsafe` is forbidden in core (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
fn unsafe_block(_core: &Core) {
    unsafe {}
}

/// INV-ARCH-2: `forbid` cannot be relaxed locally (E0453).
#[cfg(feature = "probe_unsafe_allow_override")]
#[allow(unsafe_code)]
fn unsafe_allow_override(_core: &Core) {}
