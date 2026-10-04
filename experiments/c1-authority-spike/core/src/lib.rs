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
    }
    impl Stage {
        pub fn anchor(&self, id: u64) -> Option<CanvasPos> {
            self.anchors.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
        }
        pub(crate) fn insert(&mut self, id: u64, a: CanvasPos) {
            self.anchors.push((id, a));
        }
    }
}

pub mod navigation {
    //! The ONLY module that may mutate cameras.
    use crate::geom::CanvasPos;

    #[derive(Debug)]
    pub struct CameraSet {
        center: Vec<CanvasPos>,
    }
    impl CameraSet {
        pub(crate) fn new(n: usize) -> Self {
            CameraSet { center: vec![CanvasPos { x: 0, y: 0 }; n] }
        }
        /// read access is public
        pub fn center(&self, cam: usize) -> CanvasPos {
            self.center[cam]
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

    pub(crate) fn pan(cams: &mut CameraSet, _auth: Authority, cam: usize, dx: i64, dy: i64) {
        let c = cams.center(cam);
        cams.set_center(cam, CanvasPos { x: c.x - dx, y: c.y - dy });
    }
    pub(crate) fn jump(cams: &mut CameraSet, _auth: Authority, cam: usize, to: CanvasPos) {
        cams.set_center(cam, to);
    }
}

pub mod protocol {
    //! Handles events derived from untrusted Wayland client requests.
    use crate::{Core, Effect};

    pub enum ProtocolEvent {
        /// xdg_activation_v1.activate with a validated token
        ActivationRequest { element: u64 },
        NewToplevel { element: u64 },
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
}

/// Authority to submit physical input. Minted only by `Core::new`; not Clone; private field.
pub struct InputPort {
    core_id: u64,
}
/// Authority to submit shell navigation. Minted only by `Core::new`; not Clone; private field.
pub struct ShellPort {
    core_id: u64,
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
}

impl Core {
    /// Composition root only: returns the Core and the two authority ports.
    pub fn new(cameras: usize) -> (Core, InputPort, ShellPort) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        (
            Core { id, stage: stage::Stage::default(), cameras: navigation::CameraSet::new(cameras), attention: vec![] },
            InputPort { core_id: id },
            ShellPort { core_id: id },
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
    pub fn physical_input(&mut self, port: &mut InputPort, ev: PhysicalEvent) -> Result<Vec<Effect>, AuthorityError> {
        if port.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        match ev {
            PhysicalEvent::PanDelta { camera, dx, dy } => {
                navigation::pan(&mut self.cameras, navigation::Authority::PhysicalInput, camera, dx, dy);
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
                navigation::jump(&mut self.cameras, navigation::Authority::Shell, camera, to);
                Ok(vec![Effect::CameraMoved(camera)])
            }
        }
    }
}
