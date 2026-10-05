//! Infynitae Native core: the spatial and policy authority (FOUNDATION_ARCHITECTURE C1.2).
//!
//! INC-0 state: authority **shape** only. The types and entry points below exist so that the
//! architecture probes in `enforcement/probes` and `tools/verify-architecture` compile against the
//! real production boundary (OB-19). No camera, output or input behaviour exists yet; later
//! increments populate these boundaries (FOUNDATION_IMPLEMENTATION_PLAN INC-1 onwards).
//!
//! Authority model (INV-CAMERA-1, INV-OUTPUT-2, C1.5):
//! * camera state is mutable only inside [`navigation`], and every mutation consumes a fresh
//!   [`authority::Authorized`] proof;
//! * a proof can be minted only inside the port-carrying entry points (module `entry`);
//! * [`Core::new`] is the only place that creates [`InputPort`], [`ShellPort`] and
//!   [`OutputLifecycleToken`]; they have private fields, are not `Clone`, are `!Send` and are
//!   bound to the Core that minted them.

#![forbid(unsafe_code)]

pub mod geom;
pub mod navigation;
pub mod output;

mod entry;
pub use entry::authority;

#[cfg(any(
    feature = "probe_protocol_moves_camera",
    feature = "probe_protocol_refits_camera",
    feature = "probe_protocol_creates_camera",
    feature = "probe_unsafe_block",
    feature = "probe_unsafe_allow_override"
))]
mod probes;

/// Marker that makes a type `!Send` and `!Sync`. Ports, the lifecycle token and the proof carry it,
/// so protocol state asserted `Send` cannot contain them (C7.4, E0277 on violation).
type NotSend = std::marker::PhantomData<*const ()>;

/// Authority to submit physical input (C1.5). Minted only by [`Core::new`].
pub struct InputPort {
    core_id: u64,
    _not_send: NotSend,
}

/// Authority to submit shell commands (C1.5, C7). Minted only by [`Core::new`].
pub struct ShellPort {
    core_id: u64,
    _not_send: NotSend,
}

/// Authority for output connect, reconfiguration and disconnect (INV-OUTPUT-2). Distinct from
/// [`InputPort`]; exactly one per Core, returned once by [`Core::new`] (C12.R2.3).
pub struct OutputLifecycleToken {
    core_id: u64,
    _not_send: NotSend,
}

/// Physical input produced by a backend. INC-0: no variants, so no input can be submitted.
pub enum PhysicalEvent {}

/// A semantic command from the trusted shell (C7.2). INC-0: no variants.
pub enum ShellCommand {}

/// Why an entry point refused a request.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryError {
    /// The port or token was minted by another Core (C1.5, C12.R2.3).
    ForeignPort,
    /// INC-0: the entry point is an authority boundary only; its behaviour arrives with the
    /// increment that owns it (output lifecycle: INC-2).
    Unsupported,
}

pub struct Core {
    id: u64,
    cameras: navigation::CameraSet,
    /// NEGATIVE PROBE (PD-F2-01): Core retaining a proof for a later mutation. Must make the
    /// protocol state fail its `Send` assertion (built through infynitae-wayland's probe feature).
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<authority::Authorized>,
}

impl Core {
    /// Composition root only: the Core, both authority ports and the output-lifecycle token.
    pub fn new() -> (Core, InputPort, ShellPort, OutputLifecycleToken) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        (
            Core {
                id,
                cameras: navigation::CameraSet::empty(),
                #[cfg(feature = "probe_sticky_fit_retains_authority")]
                sticky_fit: None,
            },
            InputPort {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
            ShellPort {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
            OutputLifecycleToken {
                core_id: id,
                _not_send: std::marker::PhantomData,
            },
        )
    }

    /// Read access to the cameras. There is no mutable accessor.
    pub fn cameras(&self) -> &navigation::CameraSet {
        &self.cameras
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_camera_exists_in_inc0() {
        let (core, _input, _shell, _token) = Core::new();
        assert_eq!(core.cameras().center(0), None);
    }
}
