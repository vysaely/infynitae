//! C1 authority spike — the minimal shape of the camera-authority boundary.
//!
//! Property demonstrated (Phase B NB-10 / CR-5):
//!   code handling an untrusted Wayland client request has no way to mutate Camera position.
//!
//! Mechanism:
//!  1. Camera state is mutable only inside `navigation` (Rust privacy: `set_center` is private to
//!     that module). Other core modules, including `protocol`, cannot call it.
//!  2. The only public entry points that reach `navigation` require an authority port
//!     (`InputPort` for physical input, `ShellPort` for the user's shell). Ports have private
//!     fields, are not `Clone`, and are minted only by `Core::new`, bound to that Core instance.
//!  3. Protocol code receives `&mut Core` (Smithay hands handlers the whole state) but no port;
//!     `Core::protocol_event` routes to `protocol`, which cannot reach `navigation`.

pub mod geom {
    /// Q16 fixed-point Canvas coordinate (B1). Minimal for the spike.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CanvasPos {
        pub x: i64,
        pub y: i64,
    }
}

pub mod stage {
    use crate::geom::CanvasPos;
    #[derive(Debug, Default)]
    pub struct Stage {
        anchors: Vec<(u64, CanvasPos)>,
        /// client-owned logical window size (protocol state mirrored by the snapshot); NOT Stage authority
        sizes: Vec<(u64, (i64, i64))>,
    }
    impl Stage {
        pub fn anchor(&self, id: u64) -> Option<CanvasPos> {
            self.anchors.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
        }
        pub fn size(&self, id: u64) -> Option<(i64, i64)> {
            self.sizes.iter().find(|(i, _)| *i == id).map(|(_, s)| *s)
        }
        pub(crate) fn insert(&mut self, id: u64, a: CanvasPos) {
            self.anchors.push((id, a));
            self.sizes.push((id, (800, 600)));
        }
        pub(crate) fn set_size(&mut self, id: u64, s: (i64, i64)) {
            if let Some(e) = self.sizes.iter_mut().find(|(i, _)| *i == id) {
                e.1 = s;
            }
        }
    }
}

pub mod navigation {
    //! The ONLY module that may mutate cameras.
    use crate::geom::CanvasPos;

    use crate::authority::Authorized;

    #[derive(Debug)]
    pub struct CameraSet {
        center: Vec<CanvasPos>,
        /// zoom in 1/1000 (spike simplification of the f64 zoom)
        zoom_milli: Vec<i64>,
    }
    impl CameraSet {
        pub(crate) fn new(n: usize) -> Self {
            CameraSet { center: vec![CanvasPos { x: 0, y: 0 }; n], zoom_milli: vec![1000; n] }
        }
        /// read access is public
        pub fn center(&self, cam: usize) -> CanvasPos {
            self.center[cam]
        }
        pub fn zoom_milli(&self, cam: usize) -> i64 {
            self.zoom_milli[cam]
        }
        /// mutation is private to this module
        fn set_center(&mut self, cam: usize, c: CanvasPos) {
            self.center[cam] = c;
        }
    }

    /// Authority carried by a camera mutation request.
    pub(crate) enum Authority {
        PhysicalInput,
        Shell,
    }

    pub(crate) fn pan(cams: &mut CameraSet, _proof: Authorized, _auth: Authority, cam: usize, dx: i64, dy: i64) {
        let c = cams.center(cam);
        cams.set_center(cam, CanvasPos { x: c.x - dx, y: c.y - dy });
    }
    pub(crate) fn jump(cams: &mut CameraSet, _proof: Authorized, _auth: Authority, cam: usize, to: CanvasPos) {
        cams.set_center(cam, to);
    }
    /// PD-F2-01: viewport-fit is ONE-SHOT. It snapshots the element geometry at this instant, computes
    /// centre and zoom, mutates the camera once, and terminates. No mode, no stored target, no proof kept.
    pub(crate) fn fit(cams: &mut CameraSet, _proof: Authorized, cam: usize, anchor: CanvasPos, size: (i64, i64), view: (i64, i64)) {
        let zoom = (view.0 * 1000 / size.0).min(view.1 * 1000 / size.1);
        cams.set_center(cam, CanvasPos { x: anchor.x + size.0 / 2, y: anchor.y + size.1 / 2 });
        cams.zoom_milli[cam] = zoom;
    }
}

pub mod protocol {
    //! Handles events derived from untrusted Wayland client requests.
    use crate::{Core, Effect};

    pub enum ProtocolEvent {
        /// xdg_activation_v1.activate with a validated token
        ActivationRequest { element: u64 },
        NewToplevel { element: u64 },
        /// a client commit changed the element's logical window size
        Commit { element: u64, size: (i64, i64) },
    }

    pub(crate) fn handle(core: &mut Core, ev: ProtocolEvent) -> Vec<Effect> {
        match ev {
            ProtocolEvent::ActivationRequest { element } => {
                #[cfg(feature = "probe_protocol_moves_camera")]
                {
                    // NEGATIVE PROBE: must not compile (private method / private module item).
                    core.cameras.set_center(0, crate::geom::CanvasPos { x: 9, y: 9 });
                }
                // Allowed outcomes: focus or attention. Never a camera change.
                core.attention.push(element);
                vec![Effect::AttentionRequested(element)]
            }
            ProtocolEvent::NewToplevel { element } => {
                core.stage.insert(element, crate::geom::CanvasPos { x: 0, y: 0 });
                vec![]
            }
            ProtocolEvent::Commit { element, size } => {
                core.stage.set_size(element, size);
                #[cfg(feature = "probe_protocol_refits_camera")]
                {
                    // NEGATIVE PROBE (PD-F2-01): the sealed C5.1 "re-fit on resize" written as protocol
                    // policy. Must not compile: an `Authorized` proof cannot be minted outside the
                    // port-carrying entry points (private constructor).
                    let a = core.stage.anchor(element).unwrap();
                    crate::navigation::fit(&mut core.cameras, crate::authority::Authorized::mint(), 0, a, size, (1920, 1080));
                }
                vec![]
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    CameraMoved(usize),
    AttentionRequested(u64),
}

/// Physical input as produced by a backend (backend-neutral).
pub enum PhysicalEvent {
    PanDelta { camera: usize, dx: i64, dy: i64 },
}

/// The user's shell asking for navigation (e.g. after the user acted on an attention indicator).
pub enum ShellCommand {
    NavigateTo { camera: usize, to: geom::CanvasPos },
    /// one-shot viewport-fit (PD-F2-01)
    FitViewport { camera: usize, element: u64, view: (i64, i64) },
}

/// Marker making a type `!Send` (and `!Sync`). Ports are deliberately `!Send` so that any
/// protocol-state type asserted `Send` cannot contain one (C-NB-2 hardening, E0277 on violation).
type NotSend = std::marker::PhantomData<*const ()>;

/// Authority to submit physical input. Minted only by `Core::new`; not Clone; private field; !Send.
pub struct InputPort {
    core_id: u64,
    _not_send: NotSend,
}
/// Authority to submit shell navigation. Minted only by `Core::new`; not Clone; private field; !Send.
pub struct ShellPort {
    core_id: u64,
    _not_send: NotSend,
}

#[derive(Debug, PartialEq)]
pub enum AuthorityError {
    /// a port minted for another Core instance
    ForeignPort,
}

pub struct Core {
    id: u64,
    stage: stage::Stage,
    cameras: navigation::CameraSet,
    attention: Vec<u64>,
    /// NEGATIVE PROBE (PD-F2-01): a sticky fit mode that keeps authority to re-fit later
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<(u64, authority::Authorized)>,
}

impl Core {
    /// Composition root only: returns the Core and the two authority ports.
    pub fn new(cameras: usize) -> (Core, InputPort, ShellPort) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        (
            Core {
                id,
                stage: stage::Stage::default(),
                cameras: navigation::CameraSet::new(cameras),
                attention: vec![],
                #[cfg(feature = "probe_sticky_fit_retains_authority")]
                sticky_fit: None,
            },
            InputPort { core_id: id, _not_send: std::marker::PhantomData },
            ShellPort { core_id: id, _not_send: std::marker::PhantomData },
        )
    }
    pub fn stage(&self) -> &stage::Stage {
        &self.stage
    }
    pub fn cameras(&self) -> &navigation::CameraSet {
        &self.cameras
    }
    pub fn protocol_event(&mut self, ev: protocol::ProtocolEvent) -> Vec<Effect> {
        protocol::handle(self, ev)
    }
}

/// Port-carrying entry points (C1.5). The ONLY place an `Authorized` proof can be minted (PD-F2-01):
/// `mint` is private to this module and its descendants; `protocol`, `navigation` and every other
/// module of the crate cannot call it (E0624).
mod entry {
    use super::*;

    pub mod authority {
        //! PD-F2-01 (Phase D 1R): every camera mutation consumes a fresh `Authorized` proof.
        //! * minted only by the port-carrying entry points (constructor private to `entry`);
        //! * not Clone/Copy: one proof per authorized action, consumed by value;
        //! * `!Send`: a proof retained in Core state (a "sticky" mode re-using authority later) makes
        //!   the protocol state fail its `Send` assertion (E0277), like a stored port.
        pub struct Authorized {
            _not_send: std::marker::PhantomData<*const ()>,
        }
        impl Authorized {
            pub(in crate::entry) fn mint() -> Self {
                Authorized { _not_send: std::marker::PhantomData }
            }
        }
    }

    impl Core {
    pub fn physical_input(&mut self, port: &mut InputPort, ev: PhysicalEvent) -> Result<Vec<Effect>, AuthorityError> {
        if port.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        match ev {
            PhysicalEvent::PanDelta { camera, dx, dy } => {
                navigation::pan(&mut self.cameras, authority::Authorized::mint(), navigation::Authority::PhysicalInput, camera, dx, dy);
                Ok(vec![Effect::CameraMoved(camera)])
            }
        }
    }
    pub fn shell_command(&mut self, port: &mut ShellPort, cmd: ShellCommand) -> Result<Vec<Effect>, AuthorityError> {
        if port.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        match cmd {
            ShellCommand::NavigateTo { camera, to } => {
                navigation::jump(&mut self.cameras, authority::Authorized::mint(), navigation::Authority::Shell, camera, to);
                Ok(vec![Effect::CameraMoved(camera)])
            }
            ShellCommand::FitViewport { camera, element, view } => {
                let (Some(a), Some(sz)) = (self.stage.anchor(element), self.stage.size(element)) else {
                    return Ok(vec![]); // stale element: rejected, nothing mutated
                };
                navigation::fit(&mut self.cameras, authority::Authorized::mint(), camera, a, sz, view);
                Ok(vec![Effect::CameraMoved(camera)])
            }
        }
    }
    }
}
pub use entry::authority;
