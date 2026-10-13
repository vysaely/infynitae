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
pub mod input;
pub mod navigation;
pub mod output;
pub mod pointer;
pub mod projection;
pub mod protocol;
pub mod render;
pub mod snapshot;
pub mod stage;
pub mod visibility;

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

/// Capability proof granted by Core to an authenticated InputPort for the adapter-side seat
/// (XKB) update of ONE physical key event. It carries the event it authorizes, so the adapter
/// takes the key and direction from the proof, not from a separate argument (C3): a proof for
/// key X cannot update the seat for key Y. Bound to its Core, consumed on use, `!Send`.
#[derive(Debug)]
pub struct InputAuthToken {
    core_id: u64,
    key: u32,
    pressed: bool,
    _not_send: NotSend,
}

impl InputAuthToken {
    /// The ID of the Core instance that minted this capability proof.
    pub fn core_id(&self) -> u64 {
        self.core_id
    }

    /// The evdev keycode of the authorized event.
    pub fn key(&self) -> u32 {
        self.key
    }

    /// Whether the authorized event is a press.
    pub fn pressed(&self) -> bool {
        self.pressed
    }
}

/// Physical input produced by a backend (C4.1), submitted through [`InputPort`]. Times are the
/// device's monotonic microseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysicalEvent {
    /// A relative pointer device moved: accelerated and unaccelerated deltas in logical px.
    PointerMotion {
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    },
    /// A pointer button changed. `modifiers` is the seat's modifier snapshot at this event (C4.4:
    /// core keeps no copy).
    PointerButton {
        button: u32,
        pressed: bool,
        modifiers: input::bindings::Modifiers,
        time_us: u64,
    },
    /// Scrolling.
    PointerAxis { axis: input::Axis, time_us: u64 },
    /// A physical key event on the virtual or hardware keyboard (C4.4, M12, K1–K3).
    KeyboardKey {
        key: u32,
        pressed: bool,
        modifiers: input::bindings::Modifiers,
        mods_changed: bool,
        time_us: u64,
    },
}

/// A semantic command from the trusted shell (C7.2). No variants until INC-11 (shell IPC).
pub enum ShellCommand {}

/// Why an entry point refused a request.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryError {
    /// The port or token was minted by another Core (C1.5, C12.R2.3).
    ForeignPort,
    /// The output descriptor is empty or has a non-positive scale.
    InvalidDescriptor,
    /// The `OutputId` is not connected (unknown, or already disconnected).
    UnknownOutput,
    /// Reconfiguration named a different `OutputKey`: an identity change is a disconnect plus a
    /// connect, never a reconfiguration (INV-OUTPUT-2).
    IdentityChanged,
    /// A physical event with a non-finite value; nothing was changed.
    InvalidEvent,
}

/// Why no frame could be produced for an output.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The output is not connected.
    UnknownOutput,
    /// The output has no bound camera or its view parameters are invalid.
    NoView,
}

pub struct Core {
    id: u64,
    stage: stage::Stage,
    cameras: navigation::CameraSet,
    outputs: Vec<output::OutputModel>,
    output_ids: output::OutputIds,
    /// ProjectionFrame per output (OBJ-15): the last list produced for that output.
    frames: Vec<(output::OutputId, render::ProjectionFrame)>,
    /// OBJ-13, rewritten by every visibility pass.
    membership: visibility::MembershipTable,
    /// OBJ-20, core part.
    pointer: pointer::PointerState,
    /// OBJ-25 and OBJ-26.
    input: input::InputState,
    /// OBJ-22: ActiveElementFocus.
    focus: input::focus::FocusModel,
    /// OBJ-21: authoritative active pointer constraint.
    constraint: Option<pointer::ConstraintState>,
    /// OBJ-21: authoritative pending pointer constraint.
    pending_constraint: Option<pointer::ConstraintState>,
    /// OBJ-09: read only by `output_connected`.
    persisted: navigation::PersistedCameras,
    /// The placement policy of elements not yet mapped (INV-STAGE-1 writer (a)).
    placement: protocol::PlacementPolicy,
    /// NEGATIVE PROBE (PD-F2-01): Core retaining a proof for a later mutation. Must make the
    /// protocol state fail its `Send` assertion (built through infynitae-wayland's probe feature).
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<authority::Authorized>,
}

impl Core {
    /// Composition root only: the Core, both authority ports and the output-lifecycle token. The
    /// persisted camera table is a passive lookup consulted only by `output_connected`.
    pub fn new(
        persisted: navigation::PersistedCameras,
    ) -> (Core, InputPort, ShellPort, OutputLifecycleToken) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        (
            Core {
                id,
                stage: stage::Stage::new(stage::InstanceId(id)),
                cameras: navigation::CameraSet::empty(),
                outputs: Vec::new(),
                output_ids: output::OutputIds::default(),
                frames: Vec::new(),
                membership: visibility::MembershipTable::default(),
                pointer: pointer::PointerState::NoOutput,
                input: input::InputState::default(),
                focus: input::focus::FocusModel::default(),
                constraint: None,
                pending_constraint: None,
                persisted,
                placement: protocol::PlacementPolicy::default(),
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

    /// Read access to the Stage. There is no mutable accessor.
    pub fn stage(&self) -> &stage::Stage {
        &self.stage
    }

    /// Connected outputs, in connection order.
    pub fn outputs(&self) -> &[output::OutputModel] {
        &self.outputs
    }

    pub fn output(&self, id: output::OutputId) -> Option<&output::OutputModel> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// The seat's physical pointer state (OBJ-20, core part).
    pub fn pointer(&self) -> pointer::PointerState {
        self.pointer
    }

    /// The interaction state (OBJ-26).
    pub fn interaction(&self) -> input::interaction::Interaction {
        self.input.interaction
    }

    /// The button ownership table (OBJ-25).
    pub fn buttons(&self) -> &input::ownership::Ownership {
        &self.input.ownership
    }

    /// The key ownership table (OBJ-25).
    pub fn keys(&self) -> &input::ownership::Ownership {
        &self.input.key_ownership
    }

    pub fn key_ownership(&self) -> &input::ownership::Ownership {
        &self.input.key_ownership
    }

    /// Read access to the keyboard focus model (OBJ-22).
    pub fn focus(&self) -> &input::focus::FocusModel {
        &self.focus
    }

    /// Read access to the authoritative active pointer constraint (OBJ-21).
    pub fn constraint(&self) -> Option<pointer::ConstraintState> {
        self.constraint
    }

    /// Read access to the authoritative pending pointer constraint (OBJ-21).
    pub fn pending_constraint(&self) -> Option<pointer::ConstraintState> {
        self.pending_constraint
    }

    /// Canvas origin of a surface from the single origin function (F-ORIGIN-1).
    pub fn surface_origin(&self, surface: snapshot::SurfaceKey) -> Option<geom::CanvasPoint> {
        let element = self.stage.element_containing_surface(surface)?;
        let anchor = self.stage.anchor(element)?;
        let tree = self.stage.tree(element)?;
        let origins = tree.origins(anchor).ok()?;
        let idx = tree.surfaces.iter().position(|s| s.key == surface)?;
        origins.get(idx).copied()
    }

    /// Sets keyboard focus directly via an authorized input port (INV-INPUT-8).
    pub fn set_focus(
        &mut self,
        port: &mut InputPort,
        target: Option<stage::ElementId>,
    ) -> Result<Vec<protocol::Effect>, EntryError> {
        self.check_port(port)?;
        if let Some(id) = target
            && self.stage.state(id) != Some(stage::Lifecycle::Mapped)
        {
            return Err(EntryError::InvalidEvent);
        }
        let mut fx = self.focus.set_focus(target);
        fx.extend(self.reconcile_constraints());
        Ok(fx)
    }

    /// Grants initial focus to `target` if the seat is eligible for initial activation (INV-INPUT-8, Section 5).
    pub fn grant_initial_focus(
        &mut self,
        port: &mut InputPort,
        target: stage::ElementId,
    ) -> Result<Vec<protocol::Effect>, EntryError> {
        self.check_port(port)?;
        if self.stage.state(target) != Some(stage::Lifecycle::Mapped) {
            return Err(EntryError::InvalidEvent);
        }
        let mut fx = self.focus.grant_initial_focus(target);
        if !fx.is_empty() {
            fx.extend(self.reconcile_constraints());
        }
        Ok(fx)
    }

    /// FocusNext binding (Super+Enter): cycles focus to next live mapped element in stage order.
    pub(crate) fn focus_next(&mut self) -> Vec<protocol::Effect> {
        let live = self.stage.mapped_elements();
        let mut fx = self.focus.focus_next(&live);
        fx.extend(self.reconcile_constraints());
        fx
    }

    /// Resolves an evdev keycode and modifier snapshot against bindings.
    pub fn resolve_key_binding(
        &self,
        key: u32,
        mods: input::bindings::Modifiers,
        interaction_active: bool,
    ) -> Option<input::keys::KeyAction> {
        input::keys::resolve_key_binding(key, mods, interaction_active)
    }

    /// The output the pointer is currently located on, if any.
    pub fn pointer_output(&self) -> Option<output::OutputId> {
        match self.pointer {
            pointer::PointerState::Located { output, .. } => Some(output),
            pointer::PointerState::NoOutput => None,
        }
    }

    /// The unique identity of this Core instance.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether `key` is currently recorded as held down (INV-INPUT-1).
    pub fn is_key_down(&self, key: u32) -> bool {
        self.input.key_ownership.owner(key).is_some()
    }

    /// Validates that an input port belongs to this Core.
    pub fn check_port(&self, port: &InputPort) -> Result<(), EntryError> {
        if port.core_id != self.id {
            return Err(EntryError::ForeignPort);
        }
        Ok(())
    }

    /// Verifies an InputPort and returns a capability proof, bound to this Core, for the seat
    /// update of exactly one physical key event (`key`, `pressed`).
    pub fn verify_input_port(
        &self,
        port: &InputPort,
        key: u32,
        pressed: bool,
    ) -> Result<InputAuthToken, EntryError> {
        self.check_port(port)?;
        Ok(InputAuthToken {
            core_id: self.id,
            key,
            pressed,
            _not_send: std::marker::PhantomData,
        })
    }

    /// The current per-surface output membership (OBJ-13).
    pub fn membership(&self) -> &visibility::MembershipTable {
        &self.membership
    }

    /// INV-FRAME-2: the element's frame-clock output (highest-refresh observing output).
    pub fn clock_output_of(&self, element: stage::ElementId) -> Option<output::OutputId> {
        visibility::clock_output(&self.membership.observers(element), &self.outputs)
    }

    /// A vblank of `output` (the backend's frame tick): the observed elements clocked by it, whose
    /// requested callbacks complete once and whose feedback is presented with `sync_output` =
    /// `output` (INV-FRAME-1, INV-FRAME-2, M16). Unobserved elements are not completed here
    /// (kick and fallback are INC-8).
    pub fn frame_tick(&self, output: output::OutputId) -> Vec<stage::ElementId> {
        // only an observed element has a clock, so the membership rows are the candidates
        self.membership
            .observed()
            .filter(|&e| self.clock_output_of(e) == Some(output))
            .collect()
    }

    pub(crate) fn views(&self) -> Vec<(output::OutputId, projection::View)> {
        self.outputs
            .iter()
            .filter_map(|o| self.view(o.id).map(|v| (o.id, v)))
            .collect()
    }

    /// The visibility pass (OBJ-13): membership recomputed from Stage, cameras and outputs; its
    /// diff becomes enter/leave effects. Run at the end of a core batch that changes the set of
    /// outputs or an output's hardware facts; a batch that changes one element or one camera
    /// runs the scoped pass for it.
    pub(crate) fn visibility_pass(&mut self) -> Vec<protocol::Effect> {
        let new = visibility::MembershipTable::compute(&self.stage, &self.views());
        let fx = self.membership.diff(&new, &self.stage);
        self.membership = new;
        fx
    }

    /// The visibility pass of a batch that changed only `element` (its snapshot, lifecycle or
    /// anchor): no other element's membership depends on it.
    pub(crate) fn visibility_pass_element(
        &mut self,
        element: stage::ElementId,
    ) -> Vec<protocol::Effect> {
        let views = self.views();
        self.membership
            .replace_element(element, &self.stage, &views)
    }

    /// The visibility pass of a batch that changed only `camera`: the outputs it is bound to are
    /// the only views that moved.
    pub(crate) fn visibility_pass_camera(&mut self, camera: usize) -> Vec<protocol::Effect> {
        let mut new = None;
        for o in &self.outputs {
            if self.cameras.camera_for_output(o.id) == Some(camera) {
                let view = self.view(o.id);
                new = Some(new.as_ref().unwrap_or(&self.membership).with_output(
                    o.id,
                    view.as_ref(),
                    &self.stage,
                ));
            }
        }
        let Some(new) = new else {
            return Vec::new();
        };
        let fx = self.membership.diff(&new, &self.stage);
        self.membership = new;
        fx
    }

    /// View(O) from the camera bound to the output (F-VIEW-1, F-LATTICE-1).
    pub fn view(&self, output: output::OutputId) -> Option<projection::View> {
        let model = self.outputs.iter().find(|o| o.id == output)?;
        let camera = self.cameras.camera_for_output(output)?;
        projection::View::new(projection::ViewParams {
            centre: self.cameras.center(camera)?,
            zoom: self.cameras.zoom(camera)?,
            scale: model.scale,
            view_size: model.view_size(),
        })
        .ok()
    }

    /// Produces the render list of an output and records it as that output's ProjectionFrame.
    pub fn render_list(
        &mut self,
        output: output::OutputId,
    ) -> Result<&render::ProjectionFrame, FrameError> {
        if !self.outputs.iter().any(|o| o.id == output) {
            return Err(FrameError::UnknownOutput);
        }
        let view = self.view(output).ok_or(FrameError::NoView)?;
        let frame = render::ProjectionFrame::build(view, &self.stage);
        self.frames.retain(|(o, _)| *o != output);
        self.frames.push((output, frame));
        Ok(&self.frames.last().expect("just pushed").1)
    }

    /// F-HIT-1 against the last frame shown on the output (built on demand if none exists yet).
    /// An entry whose surface no longer exists fails closed: no delivery until the next frame.
    pub fn hit_test(
        &mut self,
        output: output::OutputId,
        p: geom::OutputLogicalPoint,
    ) -> Option<render::Hit> {
        if !self.frames.iter().any(|(o, _)| *o == output) {
            self.render_list(output).ok()?;
        }
        let frame = &self.frames.iter().find(|(o, _)| *o == output)?.1;
        let hit = frame.select(p)?;
        let alive = self
            .stage
            .tree(hit.element)
            .is_some_and(|t| t.surfaces.iter().any(|s| s.key == hit.surface));
        alive.then_some(hit)
    }

    /// The single pointer → client path (B2.5, M10): the true position `(O, p)` is hit-tested on
    /// O's ProjectionFrame (F-HIT-1). With a hit, the Smithay location is `origin + local`, so the
    /// client receives exactly the delivered coordinate; without one, it is `q` (F-UNPROJECT-1 on
    /// the same frame's view) and the focus is cleared. `None` when the pointer has no output.
    pub(crate) fn route_pointer(
        &mut self,
    ) -> Option<(Option<protocol::PointerTarget>, geom::CanvasPos)> {
        let pointer::PointerState::Located { output, p } = self.pointer else {
            return None;
        };
        let pt = geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let hit = self.hit_test(output, pt);
        let view = self.frames.iter().find(|(o, _)| *o == output)?.1.view;
        Some(match hit {
            Some(h) => (
                Some(protocol::PointerTarget {
                    surface: h.surface,
                    origin: h.origin,
                }),
                geom::CanvasPos {
                    x: h.origin.x * geom::Q16_ONE + h.local.x,
                    y: h.origin.y * geom::Q16_ONE + h.local.y,
                },
            ),
            None => (None, view.unproject(pt)),
        })
    }

    /// One true-position event (M10 reconciliation): a routed motion and its frame.
    pub(crate) fn true_position(&mut self, time_ms: u32) -> Vec<protocol::Effect> {
        match self.route_pointer() {
            Some((target, location)) => vec![
                protocol::Effect::PointerMotion {
                    target,
                    location,
                    time_ms,
                },
                protocol::Effect::PointerFrame,
            ],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::geom::{LocalPoint, LocalRect, LocalSize, OutputLogicalPoint};
    use crate::output::{OutputDescriptor, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree, tests::key};

    fn tree(root: u64, sub: Option<u64>) -> SurfaceTree {
        let s = |k, role, w, h| SnapshotSurface {
            key: key(k),
            role,
            size: LocalSize { w, h },
            mapped: true,
            input: InputRegion::Infinite,
        };
        let mut surfaces = Vec::new();
        if let Some(k) = sub {
            surfaces.push(s(
                k,
                SurfaceRole::Subsurface {
                    parent: 1,
                    offset: LocalPoint { x: 10, y: 10 },
                },
                20,
                20,
            ));
        }
        surfaces.push(s(root, SurfaceRole::Root, 200, 100));
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 200, h: 100 },
            },
            surfaces,
        }
    }

    fn mapped(core: &mut Core, t: SurfaceTree) -> stage::ElementId {
        let id = core.toplevel_created();
        let mut first = t.clone();
        first.surfaces.iter_mut().for_each(|s| s.mapped = false);
        core.toplevel_committed(id, first).unwrap();
        core.toplevel_committed(id, t).unwrap();
        id
    }

    fn connected() -> (Core, output::OutputId) {
        let (mut core, _i, _s, mut token) =
            Core::new(crate::navigation::PersistedCameras::default());
        let o = core
            .output_connected(
                &mut token,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap()
            .0;
        (core, o)
    }

    /// INV-RENDER-2: elements front to back (reverse z-order), then snapshot order within each.
    #[test]
    fn render_list_is_front_to_back() {
        let (mut core, o) = connected();
        let a = mapped(&mut core, tree(1, Some(2)));
        let b = mapped(&mut core, tree(3, None));
        let order: Vec<_> = core
            .render_list(o)
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.element, e.surface.raw().get()))
            .collect();
        assert_eq!(order, vec![(b, 3), (a, 2), (a, 1)]);
    }

    /// A tree for the INC1-F07 cases: root 1 (200x100) and subsurfaces given as
    /// (key, parent index, offset, mapped); the root is the last surface (front to back).
    fn tree_with(subs: &[(u64, usize, (i32, i32), bool)]) -> SurfaceTree {
        let mut surfaces: Vec<SnapshotSurface> = subs
            .iter()
            .map(|&(k, parent, (x, y), mapped)| SnapshotSurface {
                key: key(k),
                role: SurfaceRole::Subsurface {
                    parent,
                    offset: LocalPoint { x, y },
                },
                size: LocalSize { w: 20, h: 20 },
                mapped,
                input: InputRegion::Infinite,
            })
            .collect();
        surfaces.push(SnapshotSurface {
            key: key(1),
            role: SurfaceRole::Root,
            size: LocalSize { w: 200, h: 100 },
            mapped: true,
            input: InputRegion::Infinite,
        });
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 200, h: 100 },
            },
            surfaces,
        }
    }

    fn frame_keys(core: &mut Core, o: output::OutputId) -> Vec<u64> {
        core.render_list(o)
            .unwrap()
            .entries
            .iter()
            .map(|e| e.surface.raw().get())
            .collect()
    }

    /// Output-logical point at the centre of a subsurface at `offset` (anchor (0,0) is at the
    /// view centre (960,540), scale 1).
    fn over(offset: (i32, i32)) -> OutputLogicalPoint {
        OutputLogicalPoint {
            x: 960.0 + offset.0 as f64 + 10.5,
            y: 540.0 + offset.1 as f64 + 10.5,
        }
    }

    /// INC1-F07: shown root + unshown subsurface. The unshown key is absent from
    /// `ProjectionFrame::entries` and cannot be hit; the root below it is.
    #[test]
    fn unshown_subsurface_is_not_in_the_frame() {
        let (mut core, o) = connected();
        // the root is the last surface: index 1
        mapped(&mut core, tree_with(&[(2, 1, (10, 10), false)]));
        assert_eq!(frame_keys(&mut core, o), vec![1]);
        assert_eq!(
            core.hit_test(o, over((10, 10)))
                .map(|h| h.surface.raw().get()),
            Some(1)
        );
    }

    /// INC1-F07: shown root + shown sibling + unshown sibling.
    #[test]
    fn unshown_sibling_is_not_in_the_frame() {
        let (mut core, o) = connected();
        mapped(
            &mut core,
            tree_with(&[(2, 2, (10, 10), true), (3, 2, (50, 10), false)]),
        );
        assert_eq!(frame_keys(&mut core, o), vec![2, 1]);
        assert_eq!(
            core.hit_test(o, over((10, 10)))
                .map(|h| h.surface.raw().get()),
            Some(2)
        );
        assert_eq!(
            core.hit_test(o, over((50, 10)))
                .map(|h| h.surface.raw().get()),
            Some(1)
        );
    }

    /// INC1-F07: nested subsurfaces. A mapped child of an unmapped parent is not shown (Smithay
    /// skips the children of a surface without a view); an unmapped child of a mapped parent is
    /// not shown while its parent is.
    #[test]
    fn unshown_nested_subsurfaces_are_not_in_the_frame() {
        let (mut core, o) = connected();
        // 3 (child of 2) mapped, 2 unmapped; root index 2
        mapped(
            &mut core,
            tree_with(&[(3, 1, (5, 5), true), (2, 2, (10, 10), false)]),
        );
        assert_eq!(frame_keys(&mut core, o), vec![1]);
        let (mut core, o) = connected();
        // 3 (child of 2) unmapped, 2 mapped
        mapped(
            &mut core,
            tree_with(&[(3, 1, (5, 5), false), (2, 2, (10, 10), true)]),
        );
        assert_eq!(frame_keys(&mut core, o), vec![2, 1]);
    }

    /// INC1-F07: a subsurface leaves the frame when it is unmapped and returns when remapped.
    #[test]
    fn subsurface_unmap_and_remap_change_the_frame() {
        let (mut core, o) = connected();
        let id = mapped(&mut core, tree_with(&[(2, 1, (10, 10), true)]));
        assert_eq!(frame_keys(&mut core, o), vec![2, 1]);
        core.toplevel_committed(id, tree_with(&[(2, 1, (10, 10), false)]))
            .unwrap();
        assert_eq!(frame_keys(&mut core, o), vec![1]);
        assert_eq!(
            core.hit_test(o, over((10, 10)))
                .map(|h| h.surface.raw().get()),
            Some(1)
        );
        core.toplevel_committed(id, tree_with(&[(2, 1, (10, 10), true)]))
            .unwrap();
        assert_eq!(frame_keys(&mut core, o), vec![2, 1]);
        assert_eq!(
            core.hit_test(o, over((10, 10)))
                .map(|h| h.surface.raw().get()),
            Some(2)
        );
    }

    /// PD-F1-07: an entry of the last frame whose surface no longer exists fails closed (no
    /// delivery, no fall-through to the surface below) until the next frame is produced.
    #[test]
    fn dead_frame_entries_fail_closed_until_the_next_frame() {
        let (mut core, o) = connected();
        let a = mapped(&mut core, tree(1, Some(2)));
        core.render_list(o).unwrap();
        // anchor (0,0) is at the view centre (960,540); the subsurface covers (970..990, 550..570)
        let p = OutputLogicalPoint { x: 975.5, y: 555.5 };
        assert_eq!(core.hit_test(o, p).map(|h| h.surface.raw().get()), Some(2));
        // the client destroys the subsurface; the snapshot no longer has it
        core.toplevel_committed(a, tree(1, None)).unwrap();
        assert_eq!(
            core.hit_test(o, p),
            None,
            "a dead entry must not fall through"
        );
        core.render_list(o).unwrap();
        assert_eq!(core.hit_test(o, p).map(|h| h.surface.raw().get()), Some(1));
    }

    #[test]
    fn no_camera_exists_before_an_output_is_connected() {
        let (core, _input, _shell, _token) =
            Core::new(crate::navigation::PersistedCameras::default());
        assert_eq!(core.cameras().center(0), None);
    }
}
