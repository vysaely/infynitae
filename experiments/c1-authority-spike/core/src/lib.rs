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

    /// PERSISTENCE identity for configuration/camera matching (E1R, E-F3-03): derived by the backend
    /// from (DRM device, connector name, EDID make, model, serial-if-present). It is NOT a physical-monitor
    /// identity (serial may be absent; identical monitors on one connector are indistinguishable) and NOT
    /// the runtime identity (`OutputId`, never reused). Collisions are handled deterministically in
    /// `bind_on_connect`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OutputKey(pub u64);

    #[derive(Debug)]
    pub struct CameraSet {
        center: Vec<CanvasPos>,
        /// zoom in 1/1000 (spike simplification of the f64 zoom)
        zoom_milli: Vec<i64>,
        /// the OutputKey each camera is remembered under (cameras survive outputs, INV-CAMERA-3)
        key: Vec<OutputKey>,
        /// runtime output currently bound to the camera, if any
        bound: Vec<Option<u64>>,
    }
    impl CameraSet {
        pub(crate) fn new() -> Self {
            CameraSet { center: vec![], zoom_milli: vec![], key: vec![], bound: vec![] }
        }
        pub fn len(&self) -> usize {
            self.center.len()
        }
        pub fn is_empty(&self) -> bool {
            self.center.is_empty()
        }
        pub fn camera_for_output(&self, output: u64) -> Option<usize> {
            self.bound.iter().position(|b| *b == Some(output))
        }
        pub fn bound_output(&self, cam: usize) -> Option<u64> {
            self.bound[cam]
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
    /// PD-F2-18 (2R): camera creation / binding on output connect. Reached ONLY from the
    /// `OutputLifecycleToken` entry. Placement is decided HERE, never by the caller: a camera remembered
    /// this session for `key` is re-bound unchanged; else a passive persisted entry for `key` seeds it;
    /// else the internal default placement (Level D policy; spike: origin, zoom 1).
    /// E1R (E-F2-04 1:1, E-F3-03 collisions): a camera is bound to at most one output. A remembered camera
    /// is re-bound only if UNBOUND (lowest camera index first, deterministic). If every camera with this key
    /// is bound (a concurrent key collision), the output gets a NEW camera at default placement, not seeded
    /// from persistence (the persisted entry belongs to the first holder of the key).
    pub(crate) fn bind_on_connect(cams: &mut CameraSet, _proof: Authorized, key: OutputKey, output: u64, persisted: &crate::PersistedCameras) -> usize {
        #[cfg(feature = "probe_sealed_key_steal")]
        let free = cams.key.iter().position(|k| *k == key); // NEGATIVE CONTROL: sealed 2R rule
        #[cfg(not(feature = "probe_sealed_key_steal"))]
        let free = (0..cams.key.len()).find(|&i| cams.key[i] == key && cams.bound[i].is_none());
        if let Some(i) = free {
            cams.bound[i] = Some(output);
            return i;
        }
        let collision = cams.key.iter().any(|k| *k == key);
        let seeded = if collision { None } else { persisted.lookup(key) };
        let (c, z) = seeded.unwrap_or((CanvasPos { x: 0, y: 0 }, 1000));
        cams.center.push(c);
        cams.zoom_milli.push(z);
        cams.key.push(key);
        cams.bound.push(Some(output));
        cams.center.len() - 1
    }
    /// PD-F2-18 (2R): output disconnect unbinds; the camera and its value are preserved.
    pub(crate) fn unbind_on_disconnect(cams: &mut CameraSet, _proof: Authorized, output: u64) -> Option<usize> {
        let i = cams.camera_for_output(output)?;
        cams.bound[i] = None;
        Some(i)
    }
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
                #[cfg(feature = "probe_protocol_creates_camera")]
                {
                    // NEGATIVE PROBE (PD-F2-18): protocol policy creating/binding a camera. Must not compile.
                    let p = &core.persisted;
                    crate::navigation::bind_on_connect(&mut core.cameras, crate::authority::Authorized::mint(), crate::navigation::OutputKey(7), 99, p);
                }
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
    /// output connected and bound to a camera (created or remembered)
    CameraBound { camera: usize, output: u64 },
    /// output disconnected; camera kept, unbound
    CameraUnbound { camera: usize, output: u64 },
    /// E1R (E-F2-03): an already-connected output changed hardware facts. Same OutputId; the bound camera
    /// (if any) keeps identity and value; derived state (view, membership, tracker) is rebuilt by the adapter.
    OutputReconfigured { output: u64, camera: Option<usize> },
    /// E1R (E-F2-05): the physical pointer state changed through output lifecycle (relocation, clamp,
    /// zero-output, first placement). Never a camera change.
    PointerState(PointerState),
}

/// E1R (E-F2-01): output transform (wl_output.transform). Hardware fact carried by the descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    Normal,
    R90,
    R180,
    R270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}
impl Transform {
    /// true when the transform exchanges width and height (90/270 classes)
    pub fn swaps(self) -> bool {
        matches!(self, Transform::R90 | Transform::R270 | Transform::Flipped90 | Transform::Flipped270)
    }
}

/// E1R (E-F2-05): seat physical pointer state (OBJ-20). `NoOutput` is the stable state for Outputs = ∅:
/// no fabricated output, no fabricated coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerState {
    /// (interaction output, OutputLogical p) — p within the output's half-open logical extent
    Located { output: u64, p: (f64, f64) },
    NoOutput,
}

/// PD-F2-18 (2R): what the backend knows about an output: HARDWARE facts only. There is deliberately no
/// position, centre, zoom or camera field: the caller cannot choose camera state (no backdoor
/// `set_camera_position`). Initial placement and binding are decided inside Core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputDescriptor {
    pub key: navigation::OutputKey,
    /// E1R (E-F2-01): the PHYSICAL mode (framebuffer / wl_output.mode), BEFORE the transform. This is the
    /// size handed to Smithay's `OutputDamageTracker`; the view uses `view_phys()`.
    pub mode: (i32, i32),
    /// E1R (E-F2-01): wl_output.transform
    pub transform: Transform,
    pub scale_120: u32,
    pub refresh_mhz: u32,
}
impl OutputDescriptor {
    /// E1R (E-F2-01): view extent in physical px = mode AFTER the transform (F-VIEW-1 `view_O`)
    pub fn view_phys(&self) -> (i32, i32) {
        if self.transform.swaps() {
            (self.mode.1, self.mode.0)
        } else {
            self.mode
        }
    }
    /// OutputLogical extent = view_phys / scale
    pub fn logical_extent(&self) -> (f64, f64) {
        let (w, h) = self.view_phys();
        let s = self.scale_120 as f64 / 120.0;
        (w as f64 / s, h as f64 / s)
    }
}

/// wl_fixed resolution (2⁻⁸): the half-open clamp bound used by F-RELOCATE-1 / F-RECONF-1
const FIXED_EPS: f64 = 1.0 / 256.0;
fn clamp_into(p: (f64, f64), ext: (f64, f64)) -> (f64, f64) {
    (p.0.clamp(0.0, ext.0 - FIXED_EPS), p.1.clamp(0.0, ext.1 - FIXED_EPS))
}

/// PD-F2-18 (2R): persisted camera state, a PASSIVE read-only lookup table handed to `Core::new`.
/// There is no public restore API: entries are consulted only inside `output_connected`, so startup
/// and runtime hotplug take the identical authorized path.
#[derive(Debug, Default)]
pub struct PersistedCameras {
    entries: Vec<(navigation::OutputKey, geom::CanvasPos, i64)>,
}
impl PersistedCameras {
    /// composition root: built from the (validated, all-or-nothing) persistence file
    pub fn from_entries(entries: Vec<(navigation::OutputKey, geom::CanvasPos, i64)>) -> Self {
        PersistedCameras { entries }
    }
    pub(crate) fn lookup(&self, key: navigation::OutputKey) -> Option<(geom::CanvasPos, i64)> {
        self.entries.iter().find(|(k, _, _)| *k == key).map(|(_, c, z)| (*c, *z))
    }
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

/// PD-F2-18 (2R): authority for output lifecycle events (connect/disconnect). A DEDICATED token, distinct
/// from `InputPort`: minted only by `Core::new`; private fields (unforgeable); not Clone; `!Send` (cannot
/// be stored in protocol state, E0277); bound to its Core. Held by the backend's output/display source.
pub struct OutputLifecycleToken {
    core_id: u64,
    _not_send: NotSend,
}

#[derive(Debug, PartialEq)]
pub enum AuthorityError {
    /// a port minted for another Core instance
    ForeignPort,
    /// E1R (E-F2-03): reconfiguration of an OutputId that is not connected
    UnknownOutput,
    /// E1R (E-F2-03): the descriptor names another OutputKey: an identity change is a disconnect +
    /// connect, never a reconfiguration
    IdentityChanged,
}

pub struct Core {
    id: u64,
    stage: stage::Stage,
    cameras: navigation::CameraSet,
    attention: Vec<u64>,
    /// passive persisted camera lookup (read-only after construction)
    persisted: PersistedCameras,
    outputs: Vec<(u64, OutputDescriptor)>,
    next_output: u64,
    /// E1R (E-F2-05): OBJ-20 physical pointer state
    pointer: PointerState,
    /// Arrangement origin per output, fixed at connect (spike stand-in for the configured Arrangement:
    /// the next free x to the right of the outputs present at connect time)
    arr: Vec<(u64, (f64, f64))>,
    /// NEGATIVE PROBE (PD-F2-01): a sticky fit mode that keeps authority to re-fit later
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<(u64, authority::Authorized)>,
}

impl Core {
    /// Composition root only: returns the Core, the two authority ports and the output-lifecycle token.
    /// No camera exists until an output is connected through the token.
    pub fn new(persisted: PersistedCameras) -> (Core, InputPort, ShellPort, OutputLifecycleToken) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        (
            Core {
                id,
                stage: stage::Stage::default(),
                cameras: navigation::CameraSet::new(),
                attention: vec![],
                persisted,
                outputs: vec![],
                next_output: 1,
                pointer: PointerState::NoOutput,
                arr: vec![],
                #[cfg(feature = "probe_sticky_fit_retains_authority")]
                sticky_fit: None,
            },
            InputPort { core_id: id, _not_send: std::marker::PhantomData },
            ShellPort { core_id: id, _not_send: std::marker::PhantomData },
            OutputLifecycleToken { core_id: id, _not_send: std::marker::PhantomData },
        )
    }
    pub fn stage(&self) -> &stage::Stage {
        &self.stage
    }
    pub fn cameras(&self) -> &navigation::CameraSet {
        &self.cameras
    }
    pub fn outputs(&self) -> &[(u64, OutputDescriptor)] {
        &self.outputs
    }
    pub fn pointer(&self) -> PointerState {
        self.pointer
    }
    /// Arrangement origin of an output (fixed at connect; the production Arrangement is configuration,
    /// Level D, and does not move when another output is removed).
    fn arr_origin(&self, output: u64) -> (f64, f64) {
        self.arr.iter().find(|(o, _)| *o == output).map(|(_, a)| *a).expect("arr_origin of an unknown output")
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
    /// PD-F2-18 (2R): the ONLY path that creates or binds cameras (startup and runtime hotplug alike).
    pub fn output_connected(&mut self, token: &mut OutputLifecycleToken, desc: OutputDescriptor) -> Result<(u64, Vec<Effect>), AuthorityError> {
        if token.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        let output = self.next_output;
        self.next_output += 1;
        let x = self.outputs.iter().map(|(o, d)| self.arr_origin(*o).0 + d.logical_extent().0).fold(0.0, f64::max);
        self.arr.push((output, (x, 0.0)));
        self.outputs.push((output, desc));
        let camera = navigation::bind_on_connect(&mut self.cameras, authority::Authorized::mint(), desc.key, output, &self.persisted);
        let mut fx = vec![Effect::CameraBound { camera, output }];
        // E1R (E-F2-05): leaving the zero-output state uses the startup placement rule (centre of the
        // first output's logical extent); a Located pointer is not moved by a connect.
        if self.pointer == PointerState::NoOutput {
            let e = desc.logical_extent();
            self.pointer = PointerState::Located { output, p: clamp_into((e.0 / 2.0, e.1 / 2.0), e) };
            fx.push(Effect::PointerState(self.pointer));
        }
        Ok((output, fx))
    }
    /// PD-F2-18 (2R): unbind; the camera and its value survive (INV-CAMERA-3).
    pub fn output_disconnected(&mut self, token: &mut OutputLifecycleToken, output: u64) -> Result<Vec<Effect>, AuthorityError> {
        if token.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        if !self.outputs.iter().any(|(o, _)| *o == output) {
            return Err(AuthorityError::UnknownOutput);
        }
        // F-RELOCATE-1 (E1R domain): a = arr_origin(O_lost) + p computed BEFORE removal; nearest surviving
        // output if Outputs \ {O_lost} ≠ ∅, else the zero-output state (no nearest-output selection on ∅).
        let reloc = match self.pointer {
            PointerState::Located { output: o, p } if o == output => {
                let lost = self.arr_origin(output);
                Some((lost.0 + p.0, lost.1 + p.1))
            }
            _ => None,
        };
        self.outputs.retain(|(o, _)| *o != output);
        self.arr.retain(|(o, _)| *o != output);
        let mut fx = match navigation::unbind_on_disconnect(&mut self.cameras, authority::Authorized::mint(), output) {
            Some(camera) => vec![Effect::CameraUnbound { camera, output }],
            None => vec![],
        };
        if let Some(a) = reloc {
            self.pointer = if self.outputs.is_empty() {
                PointerState::NoOutput
            } else {
                let mut best: Option<(f64, u64, (f64, f64), (f64, f64))> = None;
                for (o, d) in &self.outputs {
                    let org = self.arr_origin(*o);
                    let e = d.logical_extent();
                    let c = clamp_into((a.0 - org.0, a.1 - org.1), e);
                    let dist = (org.0 + c.0 - a.0).powi(2) + (org.1 + c.1 - a.1).powi(2);
                    if best.map_or(true, |b| dist < b.0) {
                        best = Some((dist, *o, org, c));
                    }
                }
                let (_, o, _, c) = best.expect("non-empty by the branch condition");
                PointerState::Located { output: o, p: c }
            };
            fx.push(Effect::PointerState(self.pointer));
        }
        Ok(fx)
    }
    /// E1R (E-F2-03): an already-connected output changed hardware facts (mode, transform, scale,
    /// refresh). Same OutputId; no camera argument exists; camera identity and value are untouched (no
    /// `Authorized` proof is minted here). The pointer's OutputLogical p is clamped into the new extent
    /// (F-RECONF-1). Derived state (view, membership, damage tracker, epoch settlement, M13
    /// reconciliation) is rebuilt by the adapter from the new descriptor.
    pub fn output_reconfigured(&mut self, token: &mut OutputLifecycleToken, output: u64, desc: OutputDescriptor) -> Result<Vec<Effect>, AuthorityError> {
        if token.core_id != self.id {
            return Err(AuthorityError::ForeignPort);
        }
        let Some(slot) = self.outputs.iter_mut().find(|(o, _)| *o == output) else {
            return Err(AuthorityError::UnknownOutput);
        };
        if slot.1.key != desc.key {
            return Err(AuthorityError::IdentityChanged);
        }
        slot.1 = desc;
        let mut fx = vec![Effect::OutputReconfigured { output, camera: self.cameras.camera_for_output(output) }];
        if let PointerState::Located { output: o, p } = self.pointer {
            if o == output {
                let c = clamp_into(p, desc.logical_extent());
                if c != p {
                    self.pointer = PointerState::Located { output, p: c };
                    fx.push(Effect::PointerState(self.pointer));
                }
            }
        }
        Ok(fx)
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
