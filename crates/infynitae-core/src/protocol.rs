//! Policy for events derived from untrusted Wayland client requests (C1.5 item 4): element
//! lifecycle (M1), popup placement targets (M3), and the `Effect` vocabulary. No camera
//! vocabulary: nothing here can mint an `Authorized` proof, so nothing here can mutate a camera
//! (INV-CAMERA-1).

use crate::Core;
use crate::geom::{self, CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE};
use crate::input::Axis;
use crate::output::{self, OutputId};
use crate::pointer;
use crate::snapshot::{SnapshotError, SurfaceKey, SurfaceTree};
use crate::stage::{ElementId, Lifecycle};

/// The surface a pointer event is routed to and its Canvas origin (B2.5): Smithay's focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerTarget {
    pub surface: SurfaceKey,
    pub origin: CanvasPoint,
}

/// Decisions the adapter applies to protocol objects (C2.1 `Effect` list), in order. Only core
/// constructs them (source rule `effects-only-from-core`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Send the toplevel's initial configure (M1).
    InitialConfigure(ElementId),
    /// Create the output's `wl_output` global from Core's OutputModel (M4).
    OutputAdded(OutputId),
    /// Re-announce the output's state from Core's OutputModel (M4 reconfiguration).
    OutputChanged(OutputId),
    /// Destroy the output's global; every leave for it precedes this effect (INV-OUTPUT-1).
    OutputRemoved(OutputId),
    /// `wl_surface.enter` (F-MEMBER-1, INV-RENDER-3).
    Enter {
        output: OutputId,
        surface: SurfaceKey,
    },
    /// `wl_surface.leave`.
    Leave {
        output: OutputId,
        surface: SurfaceKey,
    },
    /// Routed absolute motion at the pointer's true position (M10): Smithay `motion` with focus
    /// `target` and `location` in Canvas (INV-COORD-5). `target = None` clears the focus.
    PointerMotion {
        target: Option<PointerTarget>,
        location: CanvasPos,
        time_ms: u32,
    },
    /// Relative motion from a physical delta on the client path only (INV-INPUT-4). `target` is
    /// the routing of the same event (Smithay's pending focus).
    PointerRelative {
        target: Option<PointerTarget>,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    },
    /// A client-owned button (INV-INPUT-1).
    PointerButton {
        button: u32,
        pressed: bool,
        time_ms: u32,
    },
    /// A client-routed axis event.
    PointerAxis { axis: Axis, time_ms: u32 },
    /// `wl_pointer.frame`: ends the events of one physical event.
    PointerFrame,
    /// The last output is gone (M10 → NoOutput, EE-3): install the zero-output grab with
    /// `Focus::Clear` (one `wl_pointer.leave`, nothing delivered while installed).
    PointerZeroOutputBegin,
    /// The first output after NoOutput: remove the zero-output grab before the startup placement.
    PointerZeroOutputEnd,
    /// Change keyboard focus to `Option<ElementId>` (INV-INPUT-8).
    KeyboardFocus(Option<ElementId>),
    /// Send a configure to an xdg_toplevel, e.g. for `activated` state updates (INV-PROTO-1). Its
    /// states and size are Core's at that moment ([`Core::toplevel_configure_state`]).
    ToplevelConfigure(ElementId),
    /// Send one configure that is not fullscreen and offers `size` (M9: the size saved when
    /// fullscreen was granted). The offer is made once; later configures carry no size.
    ToplevelConfigureSize { element: ElementId, size: LocalSize },
    /// Dismiss active popup grab for an element before a camera change (INV-INPUT-2, OB-4).
    DismissPopupGrab(ElementId),
    /// A client-owned key event to forward to the focused client (K1, K2, K3).
    KeyboardForward {
        key: u32,
        pressed: bool,
        mods_changed: bool,
        time_us: u64,
    },
    /// A compositor-owned key event intercepted and consumed by the compositor.
    KeyboardIntercept { key: u32 },
    /// Activate a pointer constraint (zwp_pointer_constraints_v1) on `surface` (M13).
    ConstraintActivate {
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
    },
    /// Deactivate a pointer constraint on `surface` (M13, W-SM-4a).
    ConstraintDeactivate {
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
    },
    /// A preferred scale update for an element (F-SCALE-3, INV-SCALE-5).
    PreferredScale {
        element: ElementId,
        scale_120: u32,
        integer_scale: u32,
    },
    /// `xdg_toplevel.close` to an element: the client is asked to close (Super+Delete).
    CloseRequest(ElementId),
    /// The element's client is ended (Super+Shift+Delete): its connection cut and its process
    /// killed; the adapter decides how for a client it cannot end alone (X11).
    ForceClose(ElementId),
    /// Configure a layer surface with the size its arrangement gives it (L1, L4).
    LayerConfigure {
        layer: crate::layer::LayerId,
        size: LocalSize,
    },
    /// `zwlr_layer_surface_v1.closed`: its output is gone; the surface is no longer Core's.
    LayerClosed(crate::layer::LayerId),
    /// The keyboard goes to a layer surface (L5); it comes back with `KeyboardFocus`.
    LayerKeyboardFocus(crate::layer::LayerId),
    /// The preferred scale of a layer surface's tree: its output's scale (a layer surface is never
    /// zoomed by a camera).
    LayerPreferredScale {
        layer: crate::layer::LayerId,
        scale_120: u32,
        integer_scale: u32,
    },
    /// Super+W: toggles the Dynamic Island launcher
    ToggleLauncher,
    /// Super+Q: launches terminal
    SpawnTerminal,
    /// Super+E: launches file manager
    SpawnFileManager,
    /// Super+Shift+S or PrintScreen: capture fullscreen screenshot
    SpawnScreenshot,
    /// Super+Shift+R: capture region screenshot
    SpawnRegionScreenshot,
    /// Super+Shift+Tab: toggles canvas overview radar
    ToggleOverview,
    /// Super+Shift+BackSpace: reload shell UI and scripts
    ReloadShell,
    /// Super+L: lock screen
    SpawnLockScreen,
    /// Super+Shift+P: toggle privacy mode on focused window
    TogglePrivacy,
    /// Super+Shift+I: toggle idle sleep inhibition (stay awake mode)
    ToggleIdleInhibit,
    /// Hardware Volume Up
    VolumeUp,
    /// Hardware Volume Down
    VolumeDown,
    /// Hardware Volume Mute
    VolumeMute,
    /// Hardware Brightness Up
    BrightnessUp,
    /// Hardware Brightness Down
    BrightnessDown,
    /// Hardware Media Play / Pause
    MediaPlayPause,
    /// Hardware Media Next Song
    MediaNext,
    /// Hardware Media Previous Song
    MediaPrev,
}

/// Caller-owned, reusable storage for the effects of one Core entry (C14.3 `EffectBuf`). An
/// entry appends its effects in order and never removes one; the caller applies them and clears
/// the buffer before the next event (`clear` keeps the capacity). Capacity and overflow: it
/// grows, by one allocation, only when an event produces more effects than it has room for;
/// nothing is ever dropped or truncated. A steady state produces no larger batch than its warm-up,
/// so after warm-up it allocates nothing (INV-ALLOC-1, OB-42).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectBuf {
    pub(crate) fx: Vec<Effect>,
}

impl EffectBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer with room for `n` effects before it must grow.
    pub fn with_capacity(n: usize) -> Self {
        EffectBuf {
            fx: Vec::with_capacity(n),
        }
    }

    /// Forgets the effects, keeps the storage.
    pub fn clear(&mut self) {
        self.fx.clear();
    }

    pub fn capacity(&self) -> usize {
        self.fx.capacity()
    }

    pub fn into_vec(self) -> Vec<Effect> {
        self.fx
    }
}

impl std::ops::Deref for EffectBuf {
    type Target = [Effect];
    fn deref(&self) -> &[Effect] {
        &self.fx
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// The element does not exist (destroyed or never created).
    StaleElement,
    /// The snapshot is structurally invalid; it was not stored.
    Snapshot(SnapshotError),
    /// A buffer was committed before the initial configure (M1: `ensure_configured` → protocol
    /// error, W-SM-6).
    BufferBeforeConfigure,
}

/// Where the placement policy puts an ordinary toplevel (INV-STAGE-1 writer (a), C12.F1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlacementPolicy {
    /// The window-geometry origin at the placement output's camera centre, decided at the
    /// initial commit and never revised: the rule of INC-1…INC-5. Kept as an explicit policy.
    OriginAtCameraCentre,
    /// F-PLACE-1: the window geometry centred in the placement output's usable
    /// region, resolved once at the first map; an oversized window aligned to the region's
    /// origin.
    #[default]
    CentredInView,
    /// Proximity Cluster Tiling: places window at the nearest free spot in the cluster
    /// (closest to the viewport center), or center if empty.
    ClusterTiling,
}

/// A request a client makes on its `xdg_toplevel` (M2). The adapter forwards every one of them
/// and answers none by itself; what is answered, and with what, is decided here (INV-PROTO-1,
/// INV-PRES-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToplevelRequest {
    SetMaximized,
    UnsetMaximized,
    /// `xdg_toplevel.set_fullscreen`, with the output the client named if it is one of ours.
    SetFullscreen {
        output: Option<OutputId>,
    },
    UnsetFullscreen,
    SetMinimized,
    /// `xdg_toplevel.move`: an interactive move. Owned by INC-14.
    Move,
    /// `xdg_toplevel.resize`: an interactive resize. Owned by INC-14.
    Resize,
    ShowWindowMenu,
}

impl ToplevelRequest {
    pub const ALL: [ToplevelRequest; 8] = [
        ToplevelRequest::SetMaximized,
        ToplevelRequest::UnsetMaximized,
        ToplevelRequest::SetFullscreen { output: None },
        ToplevelRequest::UnsetFullscreen,
        ToplevelRequest::SetMinimized,
        ToplevelRequest::Move,
        ToplevelRequest::Resize,
        ToplevelRequest::ShowWindowMenu,
    ];
}

/// A Super+right drag's window (`Core::resize`): what was asked of the client and what it drew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResizeTrack {
    pub(crate) element: ElementId,
    pub(crate) left: bool,
    pub(crate) top: bool,
    pub(crate) start_anchor: CanvasPoint,
    pub(crate) start_size: (i32, i32),
    /// The size the pointer asks for now.
    pub(crate) target: (i32, i32),
    /// The size last sent in a configure.
    pub(crate) sent: (i32, i32),
    /// A configure was sent and the client has not committed since.
    pub(crate) awaiting: bool,
    /// The drag ended; the track goes with the client's next commit.
    pub(crate) released: bool,
}

/// What Core does with a [`ToplevelRequest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToplevelDecision {
    /// Fullscreen granted on an output (M9 `Windowed → Fullscreen{O}`): exactly one configure
    /// with `fullscreen` and the output's logical size.
    Granted,
    /// Fullscreen ended at the client's request (M9 `Fullscreen{O} → Windowed`): exactly one
    /// configure without `fullscreen`, offering the size saved at the grant.
    Released,
    /// Answered with exactly one configure that does not grant the state asked for. Nothing is
    /// stored, and no anchor, camera, focus or lifecycle state changes.
    Declined,
    /// A fullscreen request before the initial commit (M2 pre-configure): remembered, not
    /// answered; the initial configure carries the decision.
    Recorded,
    /// Not answered: the protocol requires no answer to this request, or the toplevel has no
    /// configure sequence to answer in (unmapped, or a non-fullscreen request before its initial
    /// configure). Nothing is stored and nothing changes.
    Ignored,
}

/// What a toplevel is told the compositor can do (`xdg_toplevel.wm_capabilities`). A capability
/// is set only if [`Core::toplevel_request`] can grant the corresponding request, so that a
/// client is not shown a control that does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToplevelCapabilities {
    pub window_menu: bool,
    pub maximize: bool,
    pub fullscreen: bool,
    pub minimize: bool,
}

/// OBJ-19 `FullscreenState`: element W is in protocol fullscreen on output O. While W is mapped,
/// `OutputPresentation(O) = Exclusive(W)`: O shows W's tree through the presentation view
/// (F-PRESENT-1) instead of its camera's view. The anchor of W and every camera are untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullscreenState {
    pub element: ElementId,
    pub output: OutputId,
    /// W's window-geometry size at the grant, offered back when fullscreen ends. `None` when W
    /// had no size yet (a grant before its first map).
    pub saved: Option<LocalSize>,
}

/// The longest title or app_id Core keeps, in bytes (INC-11).
pub const TOPLEVEL_META_MAX: usize = 4096;

/// What a configure of a toplevel carries besides `activated`, as Core holds it now. The adapter
/// copies it into every configure it sends; it keeps no fullscreen state of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToplevelConfigureState {
    /// The output the element is fullscreen on.
    pub fullscreen: Option<OutputId>,
    /// A Super+right drag is resizing it (`xdg_toplevel` state `resizing`).
    pub resizing: bool,
    /// The size asked of the client: the fullscreen output's logical size, or none (0×0: the
    /// client chooses).
    pub size: Option<LocalSize>,
}

impl Core {
    /// The capabilities to advertise: those the policy can grant. Fullscreen is granted to the
    /// focused element and before a first map (INV-PRES-1); maximize and minimize never are
    /// (INV-PROTO-1); there is no window menu.
    pub fn toplevel_capabilities(&self) -> ToplevelCapabilities {
        ToplevelCapabilities {
            fullscreen: true,
            ..ToplevelCapabilities::default()
        }
    }

    /// OBJ-19, read-only.
    pub fn fullscreen(&self) -> &[FullscreenState] {
        &self.fullscreen
    }

    /// The elements whose default placement was postponed past a fullscreen first map and is
    /// still waiting for their first windowed presentation, read-only.
    pub fn placements_pending(&self) -> Vec<ElementId> {
        self.windowed_placement_pending.clone()
    }

    pub fn fullscreen_of(&self, id: ElementId) -> Option<FullscreenState> {
        self.fullscreen.iter().copied().find(|f| f.element == id)
    }

    /// The element presented exclusively on `output`, if any: a fullscreen element that is
    /// mapped. A grant whose element is not mapped yet presents nothing.
    pub fn presentation(&self, output: OutputId) -> Option<ElementId> {
        self.fullscreen
            .iter()
            .find(|f| f.output == output && self.stage.mapped(f.element).is_some())
            .map(|f| f.element)
    }

    /// The state a configure of `id` carries (C5.2 "Configure: `fullscreen` state, size = O's
    /// logical size at zoom 1"). A fractional logical size is rounded down, so the client's
    /// surface is never larger than the output; F-PRESENT-1 centres the remainder.
    pub fn toplevel_configure_state(&self, id: ElementId) -> ToplevelConfigureState {
        let resizing = self.resize.is_some_and(|r| r.element == id && !r.released);
        let Some(fs) = self.fullscreen_of(id) else {
            return ToplevelConfigureState {
                resizing,
                ..ToplevelConfigureState::default()
            };
        };
        let size = self.outputs.iter().find(|o| o.id == fs.output).map(|o| {
            let (w, h) = o.logical_extent();
            LocalSize {
                w: w.floor() as i32,
                h: h.floor() as i32,
            }
        });
        ToplevelConfigureState {
            fullscreen: Some(fs.output),
            resizing,
            size,
        }
    }

    /// F-PRESENT-1: the presentation view of the mapped fullscreen element of `model`'s output.
    /// Zoom 1; W's window-geometry origin at the output's origin, or W centred if it is smaller
    /// than the output: `v_P = anchor(W) − max(0, (view_O/scale_O − size(W)) / 2)`.
    pub(crate) fn presentation_view(
        &self,
        model: &crate::output::OutputModel,
    ) -> Option<crate::projection::View> {
        let element = self.presentation(model.id)?;
        let (anchor, tree, _) = self.stage.mapped(element)?;
        let extent = model.logical_extent();
        // the centre of a view whose origin is v_P
        let centre = |anchor: i64, extent: f64, size: i32| {
            let margin = ((extent - f64::from(size)) / 2.0).max(0.0);
            crate::geom::q16(anchor as f64 - margin + extent / 2.0)
        };
        let mut view = crate::projection::View::new(crate::projection::ViewParams {
            centre: crate::geom::CanvasPos {
                x: centre(anchor.x, extent.0, tree.geometry.size.w),
                y: centre(anchor.y, extent.1, tree.geometry.size.h),
            },
            zoom: 1.0,
            scale: model.scale,
            view_size: model.view_size(),
        })
        .ok()?;
        view.only = Some(element);
        Some(view)
    }

    /// C5.2 output choice: the output the client named; else W's frame-clock output if W is
    /// observed; else the output under the pointer; else the first output.
    fn fullscreen_output(&self, id: ElementId, requested: Option<OutputId>) -> Option<OutputId> {
        requested
            .filter(|r| self.outputs.iter().any(|o| o.id == *r))
            .or_else(|| self.clock_output_of(id))
            .or_else(|| self.pointer_output())
            .or_else(|| self.outputs.first().map(|o| o.id))
    }

    /// C12.F13: the elements whose own presentation started or ended
    /// since `before` are the subjects of a client's fullscreen enter/exit; every other element
    /// the change touches may rise but not fall (`Sweep::presentation_change`).
    fn presentation_stimulus(&mut self, before: &[(OutputId, Option<ElementId>)]) {
        for &(o, p) in before {
            let now = self.presentation(o);
            if now != p {
                for e in [p, now].into_iter().flatten() {
                    self.scale_sweep.stimulate(e);
                }
                self.scale_sweep.presentation_change = true;
            }
        }
    }

    /// The effects of an output's presentation having started, ended or changed shape, without
    /// the scale evaluation, which the caller runs once, after this (INV-SCALE-5): its
    /// ProjectionFrame is made stale, its membership recomputed (other elements leave or return)
    /// and every element it observed before or observes now marked for that evaluation (a
    /// scoped pass during a camera drag visits only marked elements), the constraints are
    /// reconciled (M13), and the pointer gets one true position if it is on that output, free and
    /// not held by a kept lock (M10; no absolute motion to a locked surface, INV-INPUT-5 as
    /// widened by C12.F17 Q2).
    fn presentation_membership(&mut self, outputs: &[OutputId], out: &mut Vec<Effect>) {
        // the next table is built in its reused storage (as in `visibility_pass_camera`)
        let mut next = std::mem::take(&mut self.membership_next);
        let mut changed = false;
        for &o in outputs {
            self.invalidate_frame(o);
            if self.outputs.iter().any(|m| m.id == o) {
                let view = self.view(o);
                if changed {
                    next.recompute_output(o, view.as_ref(), &self.stage);
                } else {
                    next.assign_with_output(
                        &self.membership,
                        o,
                        view.as_ref(),
                        &self.stage,
                        &mut self.rows_buf,
                    );
                    changed = true;
                }
            }
        }
        if changed {
            // a scoped evaluation (during a camera drag the pass is not full) must still visit
            // every element these outputs observed before or observe now
            for (element, _, o) in self.membership.rows().iter().chain(next.rows()) {
                if outputs.contains(o) {
                    self.scale_sweep.mark(*element);
                }
            }
            self.membership.diff(&next, &self.stage, out);
            std::mem::swap(&mut self.membership, &mut next);
        }
        self.membership_next = next;
        let on_one = matches!(self.pointer, crate::pointer::PointerState::Located { output, .. } if outputs.contains(&output));
        out.extend(self.reconcile_constraints());
        if on_one && !self.input.interaction.active() && !self.locked() {
            self.true_position_into(out);
        }
    }

    /// What every output presents now, to tell afterwards which presentations a change touched.
    fn presentations(&self) -> Vec<(OutputId, Option<ElementId>)> {
        let mut out = Vec::new();
        self.fill_presentations(&mut out);
        out
    }

    /// [`Core::presentations`] into reused storage (cleared, not freed).
    fn fill_presentations(&self, out: &mut Vec<(OutputId, Option<ElementId>)>) {
        out.clear();
        out.extend(self.outputs.iter().map(|o| (o.id, self.presentation(o.id))));
    }

    /// Fullscreen entry and exit, and an output reconfiguration that resizes a presented element:
    /// every scale hint of the batch goes before its first configure,
    /// as the initial hint precedes the initial configure (M15), so the client draws the frame the
    /// configure asks for at the hint in force. Stable: each group keeps its order; in place.
    pub(crate) fn hints_before_configures(fx: &mut [Effect]) {
        let configure = |e: &Effect| {
            matches!(
                e,
                Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
            )
        };
        let Some(first) = fx.iter().position(configure) else {
            return;
        };
        let mut at = first;
        for k in first..fx.len() {
            if matches!(fx[k], Effect::PreferredScale { .. }) {
                fx[at..=k].rotate_right(1);
                at += 1;
            }
        }
    }

    /// The presentation change of the outputs whose presentation differs from `before`;
    /// when none does (a grant to an element that is not mapped yet, say), only the constraints
    /// are reconciled and the pointer is left alone.
    fn presentation_changed_since(
        &mut self,
        before: &[(OutputId, Option<ElementId>)],
    ) -> Vec<Effect> {
        let changed: Vec<OutputId> = before
            .iter()
            .filter(|(o, p)| self.presentation(*o) != *p)
            .map(|(o, _)| *o)
            .collect();
        if changed.is_empty() {
            self.reconcile_constraints()
        } else {
            self.presentation_stimulus(before);
            let mut fx = Vec::new();
            self.presentation_membership(&changed, &mut fx);
            // C12.F13: subjects and touched elements are classified per element
            fx.extend(self.evaluate_scale_policy(true));
            fx
        }
    }

    /// The configure that tells `old`'s element it is no longer fullscreen, offering the size
    /// saved at its grant (M9: "configure with the saved size").
    fn restore_effect(old: FullscreenState) -> Effect {
        match old.saved {
            Some(size) => Effect::ToplevelConfigureSize {
                element: old.element,
                size,
            },
            None => Effect::ToplevelConfigure(old.element),
        }
    }

    /// M9 `Windowed → Fullscreen{O}`: stores the state (moving W's own if it was fullscreen
    /// elsewhere) and, if W is mapped, replaces every older grant on O, whose elements are told
    /// so. A grant to an element that is not mapped replaces nothing yet: it does so when the
    /// element maps ([`Core::take_output`]), so a client that never draws cannot end another
    /// element's presentation (audit finding A-3). Returns the replaced elements' configures. It
    /// sends no configure to W: the caller does, as the answer to the request.
    fn store_fullscreen(
        &mut self,
        id: ElementId,
        output: OutputId,
        saved: Option<LocalSize>,
    ) -> Vec<Effect> {
        let mut fx = Vec::new();
        let mut saved = saved;
        if let Some(i) = self.fullscreen.iter().position(|f| f.element == id) {
            // still fullscreen: the size to return to is the one saved at the first grant
            saved = self.fullscreen.remove(i).saved;
        }
        self.fullscreen.push(FullscreenState {
            element: id,
            output,
            saved,
        });
        if self.stage.mapped(id).is_some() {
            fx.extend(self.take_output(id));
        }
        fx
    }

    /// `id`'s grant, now that its element is mapped, replaces the older grants on its output
    /// (INV-PRES-1 "a newer grant replaces an older one"): the presented element's, and any older
    /// grant still waiting for its element's first map. A newer grant still waiting is kept: it
    /// replaces this one when its element maps. `fullscreen` is kept in the order the grants
    /// were made. Returns the replaced elements' configures.
    fn take_output(&mut self, id: ElementId) -> Vec<Effect> {
        let Some(mine) = self.fullscreen.iter().position(|f| f.element == id) else {
            return Vec::new();
        };
        let output = self.fullscreen[mine].output;
        self.unsuppress_presentation(output);
        let (ended, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fullscreen)
            .into_iter()
            .enumerate()
            .partition(|(i, f)| {
                f.output == output
                    && f.element != id
                    && (*i < mine || self.stage.mapped(f.element).is_some())
            });
        self.fullscreen = kept.into_iter().map(|(_, f)| f).collect();
        ended
            .into_iter()
            .map(|(_, old)| self.fullscreen_ended(old))
            .collect()
    }

    /// Forgets `id`'s fullscreen state and every pending piece of it — a recorded pre-map
    /// request, a postponed windowed placement — without telling the client (unmap and
    /// destruction: the protocol state went with the surface's role state). The caller reports
    /// the presentation that ended.
    fn drop_fullscreen(&mut self, id: ElementId) {
        self.premap_fullscreen.retain(|(e, _)| *e != id);
        self.fullscreen.retain(|f| f.element != id);
        self.windowed_placement_pending.retain(|e| *e != id);
        self.restore_offers.retain(|(e, _)| *e != id);
    }

    /// Default windowed placement (F-PLACE-1), resolved once per element through `stage::place`.
    /// Does nothing for an element already placed.
    fn place_by_default(&mut self, id: ElementId, tree: &SurfaceTree) -> Result<(), ProtocolError> {
        self.windowed_placement_pending.retain(|e| *e != id);
        if self.stage.default_placed(id) {
            return Ok(());
        }
        let stale = |_| ProtocolError::StaleElement;
        if let Some(anchor) = self.first_map_anchor(tree) {
            self.stage.place(id, anchor).map_err(stale)?;
        }
        self.stage.set_default_placed(id).map_err(stale)
    }

    /// An element's fullscreen ended while it stays mapped (unset, replacement, output removal):
    /// returns the configure that tells it so, and, if it has never been placed as a window,
    /// postpones its default placement to its first windowed presentation (P9b): the first
    /// commit after the client acknowledged a configure without the fullscreen state.
    fn fullscreen_ended(&mut self, old: FullscreenState) -> Effect {
        if !self.stage.default_placed(old.element)
            && self.stage.mapped(old.element).is_some()
            && !self.windowed_placement_pending.contains(&old.element)
        {
            self.windowed_placement_pending.push(old.element);
        }
        // The size offered is remembered: the element's geometry is still the fullscreen one
        // until it redraws, so a grant made before that must save the offered size again, not
        // the geometry (the size a window returns to is its last windowed size).
        self.restore_offers.retain(|(e, _)| *e != old.element);
        if let Some(size) = old.saved {
            self.restore_offers.push((old.element, size));
        }
        Self::restore_effect(old)
    }

    /// INV-OUTPUT-1 / M9 `Fullscreen{O} | O disconnected → Windowed`: the states on a removed
    /// output are dropped, each with its configure.
    pub(crate) fn fullscreen_output_removed(&mut self, output: OutputId) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.premap_fullscreen
            .iter_mut()
            .filter(|(_, o)| *o == Some(output))
            .for_each(|(_, o)| *o = None);
        while let Some(i) = self.fullscreen.iter().position(|f| f.output == output) {
            let old = self.fullscreen.remove(i);
            fx.push(self.fullscreen_ended(old));
        }
        fx
    }

    /// M4 reconfiguration: "one configure only for an element in protocol fullscreen on this
    /// output" — its size follows the output's.
    pub(crate) fn fullscreen_output_reconfigured(&self, output: OutputId) -> Vec<Effect> {
        self.fullscreen
            .iter()
            .filter(|f| f.output == output)
            .map(|f| Effect::ToplevelConfigure(f.element))
            .collect()
    }

    /// C7.2 `MoveElement` after its validation (`Core::shell_command`): the anchor is written here,
    /// in the Stage's single writer (INV-STAGE-1), then the element's membership and the batch's
    /// one scale evaluation follow, as for any change of one element.
    pub(crate) fn place_by_shell(
        &mut self,
        id: ElementId,
        anchor: CanvasPoint,
        out: &mut Vec<Effect>,
    ) -> Result<(), ProtocolError> {
        self.stage
            .place(id, anchor)
            .map_err(|_| ProtocolError::StaleElement)?;
        self.invalidate_all_frames();
        // (spelled as a reborrow: the commit path's call is a seeded-control anchor)
        self.membership_pass_element(id, &mut *out);
        out.extend(self.evaluate_scale_policy(false));
        Ok(())
    }

    /// Super+left drag: the window's anchor follows the pointer, written here, in the Stage's single
    /// writer (INV-STAGE-1), with the same consequences as a shell `MoveElement` (membership, one
    /// scale evaluation).
    pub(crate) fn place_by_user(
        &mut self,
        id: ElementId,
        anchor: CanvasPoint,
        out: &mut Vec<Effect>,
    ) -> Result<(), ProtocolError> {
        self.place_by_shell(id, anchor, out)
    }

    /// When `target_elem` expands or moves to `target_rect` `(x0, y0, x1, y1)`, displace any other
    /// mapped windows that intersect `target_rect` so they are pushed to the adjacent cluster slot
    /// with the uniform 12px gap. Neither window ever covers or stomps on another window.
    pub(crate) fn push_overlapping_cluster(
        &mut self,
        target_elem: ElementId,
        target_rect: (i64, i64, i64, i64),
        out: &mut Vec<Effect>,
    ) {
        const GAP: i64 = 12;
        let mapped = self.stage.mapped_elements();
        let mut overlapping = Vec::new();

        for other in mapped {
            if other == target_elem {
                continue;
            }
            if let Some(a) = self.stage.anchor(other)
                && let Some(t) = self.stage.tree(other)
            {
                let w = t.geometry.size.w as i64;
                let h = t.geometry.size.h as i64;
                if w <= 0 || h <= 0 {
                    continue;
                }
                let gx0 = a.x + t.geometry.loc.x as i64;
                let gy0 = a.y + t.geometry.loc.y as i64;
                let gx1 = gx0.saturating_add(w);
                let gy1 = gy0.saturating_add(h);

                // Check collision/intersection
                let intersects = !(gx1 <= target_rect.0 || gx0 >= target_rect.2 || gy1 <= target_rect.1 || gy0 >= target_rect.3);
                if intersects {
                    overlapping.push((other, gx0, gy0, w, h, t.geometry.loc.x as i64, t.geometry.loc.y as i64));
                }
            }
        }

        if overlapping.is_empty() {
            return;
        }

        // Sort overlapping windows from left to right
        overlapping.sort_by_key(|&(_, gx0, ..)| gx0);

        let target_mid_x = (target_rect.0 + target_rect.2) / 2;
        let mut right_cursor = target_rect.2 + GAP;
        let mut left_cursor = target_rect.0 - GAP;

        for (other, gx0, gy0, w, _, loc_x, loc_y) in overlapping {
            let other_mid_x = gx0 + w / 2;
            let new_gx0 = if other_mid_x >= target_mid_x {
                let chosen = right_cursor;
                right_cursor = chosen + w + GAP;
                chosen
            } else {
                let chosen = left_cursor - w;
                left_cursor = chosen - GAP;
                chosen
            };

            let new_anchor = CanvasPoint {
                x: new_gx0 - loc_x,
                y: gy0 - loc_y,
            };
            let _ = self.place_by_shell(other, new_anchor, out);
        }
    }

    /// Super+right drag starts on `id`: the size and the anchor at the press are kept, so the
    /// corner opposite the dragged one stays put whatever size the client draws.
    pub(crate) fn begin_resize(
        &mut self,
        id: ElementId,
        left: bool,
        top: bool,
    ) -> Option<(i32, i32)> {
        let (anchor, tree) = (self.stage.anchor(id)?, self.stage.tree(id)?);
        let size = (tree.geometry.size.w, tree.geometry.size.h);
        self.resize = Some(ResizeTrack {
            element: id,
            left,
            top,
            start_anchor: anchor,
            start_size: size,
            target: size,
            sent: size,
            awaiting: false,
            released: false,
        });
        Some(size)
    }

    /// One motion of the resize: the size the pointer asks for. At most one configure is
    /// outstanding: a newer size waits for the client's commit of the last one.
    pub(crate) fn resize_to(&mut self, size: (i32, i32), out: &mut Vec<Effect>) {
        let Some(r) = self.resize.as_mut() else {
            return;
        };
        r.target = size;
        if !r.awaiting && r.sent != size {
            r.sent = size;
            r.awaiting = true;
            out.push(Effect::ToplevelConfigureSize {
                element: r.element,
                size: LocalSize {
                    w: size.0,
                    h: size.1,
                },
            });
        }
    }

    /// The drag ended: one last configure, without `resizing`, at the last size asked for. The
    /// track stays until the client has drawn it (the anchor follows that commit).
    pub(crate) fn end_resize(&mut self, out: &mut Vec<Effect>) {
        let Some(r) = self.resize.as_mut() else {
            return;
        };
        r.released = true;
        r.sent = r.target;
        r.awaiting = true;
        out.push(Effect::ToplevelConfigureSize {
            element: r.element,
            size: LocalSize {
                w: r.target.0,
                h: r.target.1,
            },
        });
    }

    /// A commit of the resized window: the anchor moves so that the corner opposite the dragged
    /// one stays where it was at the press; then the next size waits, if the pointer asked for
    /// another one meanwhile. Returns the anchor to write.
    fn resize_committed(
        &mut self,
        id: ElementId,
        size: LocalSize,
        out: &mut Vec<Effect>,
    ) -> Option<CanvasPoint> {
        let r = self.resize.as_mut().filter(|r| r.element == id)?;
        let anchor = CanvasPoint {
            x: r.start_anchor.x
                + if r.left {
                    i64::from(r.start_size.0 - size.w)
                } else {
                    0
                },
            y: r.start_anchor.y
                + if r.top {
                    i64::from(r.start_size.1 - size.h)
                } else {
                    0
                },
        };
        r.awaiting = false;
        if r.released {
            self.resize = None;
        } else if r.target != r.sent {
            let target = r.target;
            self.resize_to(target, out);
        }
        Some(anchor)
    }

    /// A user action on a window brings it to the top of the z-order: a click on it, or Super+Tab
    /// to it (the C4 focus model: "click → raise + keyboard focus"; the focus model requests the
    /// raise of the Stage, FOUNDATION_BLOCKERS z-order row). Written here, in the Stage's single
    /// writer (INV-STAGE-1). Activation requests of clients never raise (ADR-01: z-order is not
    /// coupled to activation). Nothing happens for an unmapped element or the top one; otherwise
    /// every output's frame is stale. Returns whether the order changed.
    pub(crate) fn raise_by_user(&mut self, id: ElementId) -> bool {
        // allocation-free: the click path is a hot path (OB-42)
        if self.stage.mapped(id).is_none() || self.stage.top_mapped() == Some(id) {
            return false;
        }
        self.stage.raise(id);
        self.invalidate_all_frames();
        true
    }

    /// `xdg_toplevel.set_title` / `set_app_id` (INC-11, owner decision 2026-10-20 D9): the
    /// toplevel's title and app_id, replaced wholesale, kept as display metadata for the shell.
    /// Each is cut to [`TOPLEVEL_META_MAX`] bytes at a character boundary, so a client cannot make
    /// Core hold an unbounded string. Decides nothing, emits nothing.
    pub fn toplevel_metadata(
        &mut self,
        id: ElementId,
        title: Option<&str>,
        app_id: Option<&str>,
    ) -> Result<(), ProtocolError> {
        let cut = |s: &str| {
            let mut end = s.len().min(TOPLEVEL_META_MAX);
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            s[..end].to_owned()
        };
        self.stage
            .set_meta(
                id,
                crate::stage::ToplevelMeta {
                    title: title.map(cut),
                    app_id: app_id.map(cut),
                },
            )
            .map_err(|_| ProtocolError::StaleElement)
    }

    /// B10.7 / INV-INPUT-9: an `xdg_activation_v1` request for `id`. `validated`: its token
    /// passed the adapter's provenance check (W-SM-8: created by the client that held the
    /// keyboard focus, with a serial of this seat no older than the last focus change, unused,
    /// unexpired). Outcome (`ActivationPolicy`; PROV-10, provisional; OB-40): under the default
    /// `Strict` policy a validated request for an element that may take keyboard focus — mapped
    /// and not hidden by a presentation (INV-INPUT-8) — focuses it, an activation grant; every
    /// other request for a mapped element marks it for attention, which only the shell renders;
    /// an element that is not mapped is ignored. It never moves a camera or an anchor: nothing
    /// here can name one.
    pub fn activation_request(
        &mut self,
        id: ElementId,
        validated: bool,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        if state != Lifecycle::Mapped {
            return Ok(Vec::new());
        }
        let eligible = !self.hidden_by_presentation(id);
        let mut fx = self.focus.activation(id, validated, eligible);
        if !fx.is_empty() {
            fx.extend(self.reconcile_constraints());
        }
        Ok(fx)
    }

    /// M2 / M9: a client's `xdg_toplevel` request. The policy, in one place:
    ///
    /// - `set_maximized` / `unset_maximized`: declined, one configure; `maximized` is never
    ///   granted (INV-PROTO-1).
    /// - `set_fullscreen`: granted if the element holds ActiveElementFocus, or before its first
    ///   map; otherwise declined at once, nothing stored (INV-PRES-1). Before the initial commit
    ///   it is recorded and decided in the initial configure. Either way exactly one configure
    ///   answers it.
    /// - `unset_fullscreen`: ends fullscreen with one configure offering the saved size; when
    ///   not fullscreen, one configure.
    /// - `set_minimized`: ignored (INV-PROTO-1). `show_window_menu`: ignored, there is none.
    /// - `move`, `resize`: ignored; interactive move and resize are INC-14.
    ///
    /// Nothing is answered while the toplevel is unmapped. No request moves an anchor, a camera
    /// or the focus.
    pub fn toplevel_request(
        &mut self,
        id: ElementId,
        request: ToplevelRequest,
    ) -> Result<(ToplevelDecision, Vec<Effect>), ProtocolError> {
        use ToplevelDecision::*;
        use ToplevelRequest::*;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        let answerable = matches!(state, Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped);
        let declined = |answerable: bool| match answerable {
            true => (Declined, vec![Effect::ToplevelConfigure(id)]),
            false => (Ignored, Vec::new()),
        };
        Ok(match request {
            SetMaximized | UnsetMaximized => declined(answerable),
            SetMinimized | Move | Resize | ShowWindowMenu => (Ignored, Vec::new()),
            SetFullscreen { output } => match state {
                Lifecycle::AwaitingInitialCommit => {
                    self.premap_fullscreen.retain(|(e, _)| *e != id);
                    self.premap_fullscreen.push((id, output));
                    (Recorded, Vec::new())
                }
                Lifecycle::Unmapped => (Ignored, Vec::new()),
                Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped => {
                    let before_first_map = !self.stage.mapped_before(id);
                    let allowed = before_first_map || self.focus.target() == Some(id);
                    match self.fullscreen_output(id, output).filter(|_| allowed) {
                        Some(o) => {
                            // the last windowed size: one still on offer, else the geometry
                            let saved = self
                                .restore_offers
                                .iter()
                                .find(|(e, _)| *e == id)
                                .map(|(_, size)| *size)
                                .or_else(|| {
                                    self.stage.mapped(id).map(|(_, tree, _)| tree.geometry.size)
                                });
                            let before = self.presentations();
                            let replaced = self.store_fullscreen(id, o, saved);
                            let mut fx = vec![Effect::ToplevelConfigure(id)];
                            fx.extend(replaced);
                            fx.extend(self.presentation_changed_since(&before));
                            Self::hints_before_configures(&mut fx);
                            (Granted, fx)
                        }
                        None => declined(true),
                    }
                }
            },
            UnsetFullscreen => match state {
                Lifecycle::AwaitingInitialCommit => {
                    self.premap_fullscreen.retain(|(e, _)| *e != id);
                    (Ignored, Vec::new())
                }
                Lifecycle::Unmapped => (Ignored, Vec::new()),
                Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped => {
                    match self.fullscreen.iter().position(|f| f.element == id) {
                        Some(i) => {
                            let before = self.presentations();
                            let old = self.fullscreen.remove(i);
                            let mut fx = vec![self.fullscreen_ended(old)];
                            fx.extend(self.presentation_changed_since(&before));
                            Self::hints_before_configures(&mut fx);
                            (Released, fx)
                        }
                        None => declined(true),
                    }
                }
            },
        })
    }

    /// M1 `new_toplevel`: mints an `ElementId`; no anchor yet.
    pub fn toplevel_created(&mut self) -> ElementId {
        self.stage.create()
    }

    /// A committed surface-tree state of an element, for a caller that does not track the
    /// client's acknowledgements: the commit is taken to carry the state Core decided last (a
    /// client that acknowledges every configure before it commits). The adapter calls
    /// [`Core::toplevel_committed_acked`].
    pub fn toplevel_committed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let mut out = EffectBuf::new();
        self.toplevel_committed_into(id, tree, &mut out)?;
        Ok(out.into_vec())
    }

    /// [`Core::toplevel_committed`] into the caller's reusable [`EffectBuf`] (C14.3).
    pub fn toplevel_committed_into(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        out: &mut EffectBuf,
    ) -> Result<(), ProtocolError> {
        let acked_fullscreen = self.fullscreen_of(id).is_some();
        self.toplevel_committed_acked_into(id, tree, acked_fullscreen, out)
    }

    /// [`Core::toplevel_committed_acked`] into the caller's reusable [`EffectBuf`] (C14.3): the
    /// effects are appended in order; on an error nothing is appended.
    pub fn toplevel_committed_acked_into(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
        out: &mut EffectBuf,
    ) -> Result<(), ProtocolError> {
        let start = out.fx.len();
        let requested = tree.frame_requested();
        let result = self.committed(id, tree, acked_fullscreen, &mut out.fx, start);
        if result.is_err() {
            out.fx.truncate(start);
        } else {
            // OBJ-17: what the commit left pending (M16)
            self.frame_liveness.committed(id, requested);
        }
        result
    }

    /// A committed surface-tree state of an element (written wholesale per commit, OBJ-05).
    /// Drives M1 from the root surface's buffer state. `acked_fullscreen`: whether the state
    /// this commit applies — the last configure the client acknowledged before it — contains
    /// `fullscreen` (xdg-shell applies an acknowledged configure's state at the next commit).
    /// A rejected commit changes nothing.
    pub fn toplevel_committed_acked(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
    ) -> Result<Vec<Effect>, ProtocolError> {
        let mut out = EffectBuf::new();
        self.toplevel_committed_acked_into(id, tree, acked_fullscreen, &mut out)?;
        Ok(out.into_vec())
    }

    /// The body of [`Core::toplevel_committed_acked`]: appends to `out` from `start`. The
    /// presentation snapshot and the touched outputs use reused storage (C14.3); an error leaves
    /// that storage empty, which costs only its capacity.
    fn committed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
        acked_fullscreen: bool,
        out: &mut Vec<Effect>,
        start: usize,
    ) -> Result<(), ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        let has_buffer = tree.root().is_some_and(|r| r.mapped);
        let stale = |_| ProtocolError::StaleElement;
        let mut before = std::mem::take(&mut self.presentations_buf);
        self.fill_presentations(&mut before);
        match (state, has_buffer) {
            (Lifecycle::AwaitingInitialCommit | Lifecycle::Unmapped, true) => {
                return Err(ProtocolError::BufferBeforeConfigure);
            }
            (Lifecycle::AwaitingInitialCommit, false) => {
                let anchor = self.initial_anchor();
                self.stage.place(id, anchor).map_err(stale)?;
                self.stage.set_tree(id, Some(tree.clone())).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                // M2 pre-configure: a recorded fullscreen request is decided here, in the initial
                // configure (before the first map, INV-PRES-1); placement ran first (C5.2)
                let grant = match self.premap_fullscreen.iter().position(|(e, _)| *e == id) {
                    Some(i) => {
                        let (_, requested) = self.premap_fullscreen.remove(i);
                        self.fullscreen_output(id, requested)
                    }
                    None => None,
                };
                // M15 / F-SCALE-3: Initial scale prediction & emission before InitialConfigure
                let views: Vec<_> = self
                    .outputs
                    .iter()
                    .filter_map(|o| self.view(o.id).map(|v| (o.id, v)))
                    .collect();
                let granted = grant.and_then(|o| self.outputs.iter().find(|m| m.id == o));
                let rho_pred = match granted {
                    // C12.F12: the presentation the grant starts at the
                    // first map (scale_O, zoom 1) and the other outputs' cameras that see the rect
                    Some(o) => {
                        let others: Vec<_> =
                            views.iter().copied().filter(|(v, _)| *v != o.id).collect();
                        crate::scale::predict_rho_observed(
                            anchor,
                            tree.geometry,
                            &self.outputs,
                            &others,
                        )
                        .map_or(o.scale, |r| r.max(o.scale))
                    }
                    None => crate::scale::predict_rho(anchor, tree.geometry, &self.outputs, &views),
                };
                // C12.F11: the configured fullscreen size bounds it too
                let configured = granted.map_or(0, |o| {
                    let (w, h) = o.logical_extent();
                    (w.floor().max(0.0) as u32).max(h.floor().max(0.0) as u32)
                });
                let s_max = crate::scale::s_max_for_tree(Some(&tree)).max(configured);
                let h_dev = crate::scale::h_dev(s_max, self.scale_config.d_max);
                let h_max =
                    crate::scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
                let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
                let initial_h =
                    crate::scale::target(rho_pred, ladder, h_max, self.scale_config.sigma);
                let s120 = crate::scale::preferred_scale_120(initial_h);
                let b = crate::scale::preferred_buffer_scale(initial_h, h_dev);

                let mut st = crate::scale::ScaleState::new(initial_h);
                st.last_emitted_scale_120 = Some(s120);
                st.last_emitted_buffer_scale = Some(b);
                st.last_rho = Some(rho_pred);
                self.scales.set(id, st);

                out.extend([
                    Effect::PreferredScale {
                        element: id,
                        scale_120: s120,
                        integer_scale: b,
                    },
                    Effect::InitialConfigure(id),
                ]);
                if let Some(o) = grant {
                    out.extend(self.store_fullscreen(id, o, None));
                }
            }
            (Lifecycle::Unmapped, false) => {
                // anchor kept (INV-STAGE-5)
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::AwaitingFirstBuffer)
                    .map_err(stale)?;
                out.push(Effect::InitialConfigure(id));
            }
            (Lifecycle::AwaitingFirstBuffer, false) => {
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
            }
            (Lifecycle::Mapped, true) => {
                // a window being resized from its left or top keeps its opposite corner in place
                if let Some(anchor) = self.resize_committed(id, tree.geometry.size, out) {
                    self.stage.place(id, anchor).map_err(stale)?;
                }
                // An element that left fullscreen without ever having been a window is placed at
                // its first windowed presentation (P9b): the first commit after it acknowledged a
                // configure without the fullscreen state, whatever its size. Frames committed
                // before that acknowledgement are fullscreen frames, whatever their size.
                let arrived = !acked_fullscreen && self.windowed_placement_pending.contains(&id);
                if arrived && self.fullscreen_of(id).is_none() {
                    self.place_by_default(id, &tree)?;
                }
                // the client redrew at another size: a size still on offer is spent
                if self.stage.tree(id).is_some_and(|old| old.geometry.size != tree.geometry.size)
                {
                    self.restore_offers.retain(|(e, _)| *e != id);
                }
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                if self.placement == PlacementPolicy::ClusterTiling && self.fullscreen_of(id).is_some() {
                    if let (Some(a), Some(t)) = (self.stage.anchor(id), self.stage.tree(id)) {
                        let w = t.geometry.size.w as i64;
                        let h = t.geometry.size.h as i64;
                        if w > 0 && h > 0 {
                            let target_rect = (a.x, a.y, a.x + w, a.y + h);
                            self.push_overlapping_cluster(id, target_rect, out);
                        }
                    }
                }
            }
            (Lifecycle::AwaitingFirstBuffer, true) => {
                // The size is known now, so the placement policy can use it: default placement
                // is resolved at the element's first windowed presentation. A map in protocol
                // fullscreen is not one and leaves the opportunity for later; a remap of a
                // placed element keeps its anchor (INV-STAGE-5).
                if self.fullscreen_of(id).is_none() {
                    self.place_by_default(id, &tree)?;
                }
                self.stage.set_tree(id, Some(tree)).map_err(stale)?;
                self.stage.set_state(id, Lifecycle::Mapped).map_err(stale)?;
                self.stage.raise(id);
                // a grant made before this map takes its output now (A-3)
                out.extend(self.take_output(id));
                if self.placement == PlacementPolicy::ClusterTiling && self.fullscreen_of(id).is_some() {
                    if let (Some(a), Some(t)) = (self.stage.anchor(id), self.stage.tree(id)) {
                        let w = t.geometry.size.w as i64;
                        let h = t.geometry.size.h as i64;
                        if w > 0 && h > 0 {
                            let target_rect = (a.x, a.y, a.x + w, a.y + h);
                            self.push_overlapping_cluster(id, target_rect, out);
                        }
                    }
                }
            }
            (Lifecycle::Mapped, false) => {
                // null-buffer commit: unmap; identity and anchor kept, snapshot dropped
                self.stage.set_tree(id, None).map_err(stale)?;
                self.stage
                    .set_state(id, Lifecycle::Unmapped)
                    .map_err(stale)?;
                // the successor is chosen among the elements focus may move to (A-8)
                let live: Vec<ElementId> = self
                    .focusable_elements()
                    .into_iter()
                    .filter(|&e| e != id)
                    .collect();
                // M1: "fullscreen dropped (M9)"; the client must request it again after a remap
                self.drop_fullscreen(id);
                out.extend(self.focus.target_lost(id, &live));
            }
        }
        self.membership_pass_element(id, out);
        // A presentation that started or ended in this commit (a fullscreen element's first map;
        // an older grant replaced), and any commit of a presented element: its view depends on
        // its geometry (F-PRESENT-1).
        let mut touched = std::mem::take(&mut self.touched_buf);
        touched.clear();
        touched.extend(
            before
                .iter()
                .filter(|(o, p)| self.presentation(*o) != *p)
                .map(|(o, _)| *o),
        );
        // a presentation that started or ended is a non-client stimulus (C14.4 SC-E); a commit
        // alone is a client response
        let presentation_moved = !touched.is_empty();
        if presentation_moved {
            // C12.F13: classified per element
            self.presentation_stimulus(&before);
        }
        let client_response = true;
        if let Some(fs) = self.fullscreen_of(id)
            && self.presentation(fs.output) == Some(id)
            && !touched.contains(&fs.output)
        {
            touched.push(fs.output);
        }
        if touched.is_empty() {
            out.extend(self.reconcile_constraints());
        } else {
            self.presentation_membership(&touched, out);
        }
        self.touched_buf = touched;
        self.presentations_buf = before;
        out.extend(self.evaluate_scale_policy(client_response));
        if presentation_moved {
            Self::hints_before_configures(&mut out[start..]);
        }
        Ok(())
    }

    /// A change of a mapped element's surface tree that is NOT a commit of its root surface: a
    /// child surface (popup, desynchronised subsurface) committed, or a popup was destroyed. It
    /// replaces the snapshot of a Mapped element (hit-testing and rendering follow, INV-HIT-1) and
    /// never drives M1: only the root surface's own commit (`toplevel_committed`) may map, unmap
    /// or configure an element. An element that is not Mapped has no hit-testable tree, so the
    /// change is ignored until its root commits. A rejected tree changes nothing.
    pub fn toplevel_tree_changed(
        &mut self,
        id: ElementId,
        tree: SurfaceTree,
    ) -> Result<Vec<Effect>, ProtocolError> {
        tree.validate().map_err(ProtocolError::Snapshot)?;
        let state = self.stage.state(id).ok_or(ProtocolError::StaleElement)?;
        // OBJ-17: a child surface's commit may leave a callback pending (M16)
        self.frame_liveness.committed(id, tree.frame_requested());
        // The root's mapped state is committed state of the root surface; only its own commit
        // changes it, so a tree without a mapped root is never applied here.
        if state != Lifecycle::Mapped || !tree.root().is_some_and(|r| r.mapped) {
            return Ok(Vec::new());
        }
        self.stage
            .set_tree(id, Some(tree))
            .map_err(|_| ProtocolError::StaleElement)?;
        // P7 for the new tree is the first rule of the batch-end evaluation (ScaleState::step)
        let mut fx = Vec::new();
        self.membership_pass_element(id, &mut fx);
        // the presentation view of a fullscreen element depends on its geometry (F-PRESENT-1)
        match self.fullscreen_of(id) {
            Some(fs) if self.presentation(fs.output) == Some(id) => {
                self.presentation_membership(&[fs.output], &mut fx)
            }
            _ => fx.extend(self.reconcile_constraints()),
        }
        fx.extend(self.evaluate_scale_policy(true));
        Ok(fx)
    }

    /// M1 destroy: purges every reference; idempotent and order-independent (INV-STAGE-4). The
    /// dead client gets no leave: its membership rows are dropped silently. Target loss triggers
    /// successor focus transition (K7, M11).
    pub fn toplevel_destroyed(&mut self, id: ElementId) -> Vec<Effect> {
        self.scales.remove(id);
        // a drag on it ends with the release; nothing is sized or moved any more
        self.input.interaction.orphan_element(id);
        if self.resize.is_some_and(|r| r.element == id) {
            self.resize = None;
        }
        for slot in &mut self.frames {
            slot.frame.entries.retain(|e| e.element != id);
        }
        let before = self.presentations();
        let existed = self.stage.destroy(id);
        if !existed {
            return Vec::new();
        }
        self.frame_liveness.destroyed(id);
        let live: Vec<ElementId> = self.focusable_elements();
        let mut fx = self.focus.target_lost(id, &live);
        self.drop_fullscreen(id);
        let ended: Vec<OutputId> = before
            .iter()
            .filter(|(_, p)| *p == Some(id))
            .map(|(o, _)| *o)
            .collect();
        if !ended.is_empty() {
            // C12.F13: the elements it revealed may rise, not fall
            self.presentation_stimulus(&before);
            self.presentation_membership(&ended, &mut fx);
        }
        // Evaluated as client-caused: the destruction of an element is not one of the
        // non-client stimuli of SC-E (C14.4), so it must not reverse a hint another element
        // is holding (INV-SCALE-4 d). The end of a presentation it held is one (fullscreen exit).
        let mut vis = Vec::new();
        self.membership_pass_element(id, &mut vis);
        vis.extend(self.evaluate_scale_policy(true));
        // The pass also evaluates the scale policy of the surviving elements (INV-SCALE-5), so
        // it may return a hint that a client-caused evaluation is allowed to move. The destroyed
        // element itself has no row left to diff.
        debug_assert!(
            !vis.iter()
                .any(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. })),
            "a destroyed element produced enter or leave effects"
        );
        fx.extend(vis);
        fx.extend(self.reconcile_constraints());
        fx
    }

    /// M3: the constraint target for a popup of `parent`: the anchor output's viewport,
    /// inward-rounded (R8), relative to the parent's window-geometry origin. The anchor output is
    /// (D-B6-BOUNDARY):
    /// - Rule 1: output on which causing input event occurred, if parent is visible on it;
    /// - Rule 2: else parent's frame-clock output (highest refresh observing output);
    /// - Rule 3: else none (`None`: no constraint adjustment).
    ///
    /// `None` also when the target does not fit protocol integers.
    pub fn popup_constraint_target(
        &self,
        parent: ElementId,
        causing_output: Option<OutputId>,
    ) -> Option<LocalRect> {
        let anchor = self.stage.anchor(parent)?;
        // C5.2: the popups of a fullscreen element are constrained to its fullscreen output
        let presented = self
            .fullscreen_of(parent)
            .map(|f| f.output)
            .filter(|o| self.presentation(*o) == Some(parent));
        let output = match (presented, causing_output) {
            (Some(o), _) => o,
            (None, Some(o)) if self.membership.observers(parent).contains(&o) => o,
            _ => self.clock_output_of(parent)?,
        };
        let view = self.view(output)?;
        let extent = |px: u32| crate::geom::q16(px as f64 / view.k);
        let (x0q, y0q) = (view.c_eff.x, view.c_eff.y);
        let (x1q, y1q) = (x0q + extent(view.size.0), y0q + extent(view.size.1));
        // R8: ceiling at the minimum edges, floor at the maximum edges
        let ceil = |v: i64| -(-v).div_euclid(Q16_ONE);
        let floor = |v: i64| v.div_euclid(Q16_ONE);
        let to_i32 = |v: i64| i32::try_from(v).ok();
        let x0 = to_i32(ceil(x0q) - anchor.x)?;
        let y0 = to_i32(ceil(y0q) - anchor.y)?;
        let x1 = to_i32(floor(x1q) - anchor.x)?;
        let y1 = to_i32(floor(y1q) - anchor.y)?;
        Some(LocalRect {
            loc: LocalPoint { x: x0, y: y0 },
            size: LocalSize {
                w: x1.checked_sub(x0)?.max(0),
                h: y1.checked_sub(y0)?.max(0),
            },
        })
    }

    /// The anchor written at the initial commit (INV-STAGE-1, C12.F1): the bound camera's centre
    /// of the first connected output (lowest `OutputId`), rounded R3; the Canvas origin without
    /// an output. Under the default policy it is provisional until the first map; under
    /// [`PlacementPolicy::OriginAtCameraCentre`] it is final.
    fn initial_anchor(&self) -> CanvasPoint {
        self.outputs
            .first()
            .and_then(|o| self.cameras.camera_for_output(o.id))
            .and_then(|c| self.cameras.center(c))
            .and_then(|c| c.round_to_point())
            .unwrap_or(CanvasPoint { x: 0, y: 0 })
    }

    /// Default placement at first map, when the window-geometry size is known (F-PLACE-1).
    ///
    /// [`PlacementPolicy::CentredInView`]: the window geometry is centred in the usable region of
    /// the placement output (the first connected output, as for [`Core::initial_anchor`]), in
    /// Canvas coordinates through that output's camera: per axis, with `c` the camera centre,
    /// `u` the usable logical extent and `s` the outer size,
    /// `anchor = c − u/2 + (u − s)/2 = c − s/2` when `s ≤ u`. A window larger than the region on
    /// an axis is aligned to the region's origin on that axis (`anchor = c − u/2`), so that its
    /// top-left corner — title bar, menus — is the part that stays reachable. The result is
    /// rounded once by R3 ([`CanvasPos::round_to_point`]). The usable region is the output view
    /// less the exclusive zones of its layer surfaces (L4), centred at its own centre.
    ///
    /// `None` keeps the anchor of the initial commit: the legacy policy, no output, or a
    /// position outside the Canvas bound.
    fn first_map_anchor(&self, tree: &SurfaceTree) -> Option<CanvasPoint> {
        if self.placement == PlacementPolicy::OriginAtCameraCentre {
            return None;
        }
        let output = self.outputs.first()?;
        let centre = self
            .cameras
            .camera_for_output(output.id)
            .and_then(|c| self.cameras.center(c))?;
        // the usable region (L4): the output view less its layers' exclusive zones, centred at its
        // own centre, which lies `d` from the view's (Canvas px at the camera's zoom)
        let extent = output.logical_extent();
        let (ux, uy, uw, uh) = self
            .usable_area(output.id)
            .unwrap_or((0.0, 0.0, extent.0, extent.1));
        let zoom = self
            .cameras
            .camera_for_output(output.id)
            .and_then(|c| self.cameras.zoom(c))
            .unwrap_or(1.0);
        let d = (
            (ux + uw / 2.0 - extent.0 / 2.0) / zoom,
            (uy + uh / 2.0 - extent.1 / 2.0) / zoom,
        );
        let usable = (uw, uh);
        let axis = |c: i64, usable: f64, outer: i32| {
            let half_outer = i64::from(outer.max(0)) * Q16_ONE / 2;
            let half_usable = geom::q16(usable / 2.0);
            c - half_outer.min(half_usable)
        };
        let center_anchor = CanvasPos {
            x: axis(
                centre.x.saturating_add(geom::q16(d.0)),
                usable.0,
                tree.geometry.size.w,
            ),
            y: axis(
                centre.y.saturating_add(geom::q16(d.1)),
                usable.1,
                tree.geometry.size.h,
            ),
        }
        .round_to_point()?;

        if self.placement == PlacementPolicy::CentredInView {
            return Some(center_anchor);
        }

        // PlacementPolicy::ClusterTiling
        let new_w = tree.geometry.size.w as i64;
        let new_h = tree.geometry.size.h as i64;
        let new_loc_x = tree.geometry.loc.x as i64;
        let new_loc_y = tree.geometry.loc.y as i64;
        if new_w <= 0 || new_h <= 0 {
            return Some(center_anchor);
        }

        // Visual geometry bounds of all currently mapped windows
        let mut existing: Vec<(i64, i64, i64, i64)> = Vec::new();
        for elem in self.stage.mapped_elements() {
            if let Some(a) = self.stage.anchor(elem)
                && let Some(t) = self.stage.tree(elem)
            {
                let ew = t.geometry.size.w as i64;
                let eh = t.geometry.size.h as i64;
                if ew > 0 && eh > 0 {
                    let gx0 = a.x + t.geometry.loc.x as i64;
                    let gy0 = a.y + t.geometry.loc.y as i64;
                    existing.push((gx0, gy0, gx0.saturating_add(ew), gy0.saturating_add(eh)));
                }
            }
        }

        let center_gx = center_anchor.x;
        let center_gy = center_anchor.y;

        if existing.is_empty() {
            return Some(CanvasPoint {
                x: center_gx - new_loc_x,
                y: center_gy - new_loc_y,
            });
        }

        const GAP: i64 = 12;
        let overlaps = |gx: i64, gy: i64| -> bool {
            let gx1 = gx.saturating_add(new_w);
            let gy1 = gy.saturating_add(new_h);
            existing.iter().any(|&(ex0, ey0, ex1, ey1)| {
                !(gx1 + GAP <= ex0 || gx >= ex1 + GAP || gy1 + GAP <= ey0 || gy >= ey1 + GAP)
            })
        };

        if !overlaps(center_gx, center_gy) {
            return Some(CanvasPoint {
                x: center_gx - new_loc_x,
                y: center_gy - new_loc_y,
            });
        }

        // Generate grid-aligned candidate positions flush with existing window edges
        let mut candidates = Vec::new();
        for &w1 in &existing {
            // Adjacent flush placements relative to w1
            candidates.push((w1.2 + GAP, w1.1));                 // Right, top-aligned
            candidates.push((w1.2 + GAP, w1.3 - new_h));         // Right, bottom-aligned
            candidates.push((w1.0 - new_w - GAP, w1.1));         // Left, top-aligned
            candidates.push((w1.0 - new_w - GAP, w1.3 - new_h)); // Left, bottom-aligned
            candidates.push((w1.0, w1.3 + GAP));                 // Below, left-aligned
            candidates.push((w1.2 - new_w, w1.3 + GAP));         // Below, right-aligned
            candidates.push((w1.0, w1.1 - new_h - GAP));         // Above, left-aligned
            candidates.push((w1.2 - new_w, w1.1 - new_h - GAP)); // Above, right-aligned

            // Grid intersections with other windows
            for &w2 in &existing {
                candidates.push((w1.2 + GAP, w2.1));
                candidates.push((w1.0, w2.3 + GAP));
                candidates.push((w1.0 - new_w - GAP, w2.1));
                candidates.push((w1.0, w2.1 - new_h - GAP));
                candidates.push((w1.2 + GAP, w2.3 + GAP));
            }
        }

        let best = candidates
            .into_iter()
            .filter(|&(gx, gy)| !overlaps(gx, gy))
            .min_by_key(|&(gx, gy)| {
                let dx = (gx - center_gx) as i128;
                let dy = (gy - center_gy) as i128;
                dx * dx + dy * dy
            });

        let (chosen_gx, chosen_gy) = best.unwrap_or((center_gx, center_gy));
        Some(CanvasPoint {
            x: chosen_gx - new_loc_x,
            y: chosen_gy - new_loc_y,
        })
    }

    /// The placement policy for elements mapped from now on. It never moves an element that is
    /// already placed.
    pub fn set_placement_policy(&mut self, policy: PlacementPolicy) {
        self.placement = policy;
    }

    pub fn placement_policy(&self) -> PlacementPolicy {
        self.placement
    }

    /// Requests a pointer constraint from client protocol (M13, OBJ-21).
    /// If preconditions are met, activates immediately; otherwise recorded as pending.
    pub fn request_pointer_constraint(
        &mut self,
        surface: SurfaceKey,
        kind: crate::pointer::ConstraintKind,
        lifetime: crate::pointer::ConstraintLifetime,
        region: Option<LocalRect>,
        cursor_position_hint: Option<LocalPoint>,
    ) -> Vec<Effect> {
        let lock_point = match cursor_position_hint {
            Some(hint) => hint,
            None => {
                let hit_local = self.route_pointer_hit_local(surface);
                hit_local.unwrap_or_default()
            }
        };
        let candidate = crate::pointer::ConstraintState {
            surface,
            kind,
            lifetime,
            lock_point,
            region,
        };

        self.pending_constraint = Some(candidate);

        if self.can_activate_constraint(&candidate) {
            let mut fx = Vec::new();
            if let Some(prev) = self.constraint
                && (prev.surface != surface || prev.kind != kind)
            {
                fx.push(Effect::ConstraintDeactivate {
                    surface: prev.surface,
                    kind: prev.kind,
                });
            }
            self.constraint = Some(candidate);
            if candidate.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                self.pending_constraint = None;
            }
            fx.push(Effect::ConstraintActivate { surface, kind });
            fx
        } else {
            Vec::new()
        }
    }

    /// Sets or updates the cursor position hint of a locked pointer constraint.
    pub fn set_cursor_position_hint(&mut self, surface: SurfaceKey, hint: LocalPoint) {
        if let Some(c) = &mut self.constraint
            && c.surface == surface
            && c.kind == crate::pointer::ConstraintKind::Lock
        {
            c.lock_point = hint;
        }
        if let Some(c) = &mut self.pending_constraint
            && c.surface == surface
            && c.kind == crate::pointer::ConstraintKind::Lock
        {
            c.lock_point = hint;
        }
    }

    /// Client destroys a pointer constraint (M13).
    pub fn destroy_pointer_constraint(&mut self, surface: SurfaceKey) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(c) = self.constraint
            && c.surface == surface
        {
            self.constraint = None;
            // W-SM-4a: deactivate only if delivered focus does not change in this batch
            let delivered_focus = self
                .route_pointer()
                .and_then(|(tgt, _)| tgt)
                .map(|t| t.surface);
            if delivered_focus == Some(surface) {
                fx.push(Effect::ConstraintDeactivate {
                    surface,
                    kind: c.kind,
                });
            }
        }
        if let Some(c) = self.pending_constraint
            && c.surface == surface
        {
            self.pending_constraint = None;
        }
        fx
    }

    /// M13 batch-end reconciliation pass (INV-INPUT-5).
    /// Runs once at the end of every core batch where view/descriptor, geometry/origin,
    /// output set, ActiveElementFocus, or an interaction/grab ended.
    pub fn reconcile_constraints(&mut self) -> Vec<Effect> {
        // Deferral rule: deferred while compositor pointer interaction or client-owned grab sequence is active.
        if self.input.interaction.active() || self.input.ownership.client_owns_any() {
            return Vec::new();
        }

        let mut fx = Vec::new();

        // 1. Check active constraint, if any
        if let Some(c) = self.constraint {
            let still_valid = self.is_constraint_still_valid(&c);
            if !still_valid {
                self.constraint = None;
                if c.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                    self.pending_constraint = None;
                }

                // W-SM-4a: explicit deactivate ONLY if delivered focus does not change in this batch;
                // otherwise Smithay's leave() does it.
                let delivered_focus = self
                    .route_pointer()
                    .and_then(|(tgt, _)| tgt)
                    .map(|t| t.surface);
                if delivered_focus == Some(c.surface) {
                    fx.push(Effect::ConstraintDeactivate {
                        surface: c.surface,
                        kind: c.kind,
                    });
                }
            } else if c.kind == crate::pointer::ConstraintKind::Lock {
                // Lock is maintained. Update physical pointer to derived lock point.
                if let Some(p_lock) = self.derived_lock_output_point(&c)
                    && let pointer::PointerState::Located { output, .. } = self.pointer
                {
                    self.pointer = pointer::PointerState::Located {
                        output,
                        p: (p_lock.x, p_lock.y),
                    };
                }
            }
        }

        // 2. If no constraint is active, check if pending constraint can activate
        if self.constraint.is_none()
            && let Some(pending) = self.pending_constraint
            && self.can_activate_constraint(&pending)
        {
            self.constraint = Some(pending);
            if pending.lifetime == crate::pointer::ConstraintLifetime::Oneshot {
                self.pending_constraint = None;
            }
            fx.push(Effect::ConstraintActivate {
                surface: pending.surface,
                kind: pending.kind,
            });
        }

        fx
    }

    /// Surface-local point if pointer currently hits `surface`.
    fn route_pointer_hit_local(&mut self, surface: SurfaceKey) -> Option<LocalPoint> {
        let pointer::PointerState::Located { output, p } = self.pointer else {
            return None;
        };
        let pt = geom::OutputLogicalPoint { x: p.0, y: p.1 };
        let hit = self.hit_test(output, pt)?;
        if hit.surface == surface {
            Some(LocalPoint {
                x: (hit.local.x / geom::Q16_ONE) as i32,
                y: (hit.local.y / geom::Q16_ONE) as i32,
            })
        } else {
            None
        }
    }

    /// M13 activation precondition guard:
    /// [ActiveElementFocus = element(S) ∧ pointer focus S ∧ inside region ∧ no acquisition]
    fn can_activate_constraint(&mut self, c: &crate::pointer::ConstraintState) -> bool {
        // 1. no compositor pointer interaction or client-owned grab being acquired
        if self.input.interaction.active() || self.input.ownership.client_owns_any() {
            return false;
        }

        // 2. ActiveElementFocus == element(S)
        let Some(elem) = self.stage.element_containing_surface(c.surface) else {
            return false;
        };
        if self.focus.target() != Some(elem) {
            return false;
        }

        // 3. Pointer focus on S
        let Some(local_pt) = self.route_pointer_hit_local(c.surface) else {
            return false;
        };

        // 4. Pointer inside region
        if let Some(r) = c.region
            && !r.contains(local_pt.x as i64, local_pt.y as i64)
        {
            return false;
        }

        true
    }

    /// Checks if an active constraint remains valid at batch end.
    fn is_constraint_still_valid(&mut self, c: &crate::pointer::ConstraintState) -> bool {
        let Some(elem) = self.stage.element_containing_surface(c.surface) else {
            return false;
        };
        if self.focus.target() != Some(elem) {
            return false;
        }

        match c.kind {
            crate::pointer::ConstraintKind::Lock => {
                let pointer::PointerState::Located { output, .. } = self.pointer else {
                    return false;
                };
                let Some(model) = self.output(output) else {
                    return false;
                };
                let Some(view) = self.view(output) else {
                    return false;
                };
                let Some(origin) = self.surface_origin(c.surface) else {
                    return false;
                };
                let q_lock = crate::pointer::derive_lock_point(origin, c.lock_point);
                let p_lock = view.project(q_lock);
                let extent = model.logical_extent();
                p_lock.x >= 0.0 && p_lock.y >= 0.0 && p_lock.x < extent.0 && p_lock.y < extent.1
            }
            crate::pointer::ConstraintKind::Confine => {
                let Some(local_pt) = self.route_pointer_hit_local(c.surface) else {
                    return false;
                };
                if let Some(r) = c.region
                    && !r.contains(local_pt.x as i64, local_pt.y as i64)
                {
                    return false;
                }
                true
            }
        }
    }

    fn derived_lock_output_point(
        &mut self,
        c: &crate::pointer::ConstraintState,
    ) -> Option<geom::OutputLogicalPoint> {
        let pointer::PointerState::Located { output, .. } = self.pointer else {
            return None;
        };
        let origin = self.surface_origin(c.surface)?;
        let q_lock = crate::pointer::derive_lock_point(origin, c.lock_point);
        let view = self.view(output)?;
        Some(view.project(q_lock))
    }

    /// Routes motion when a pointer lock is active: zero absolute motion to the locked surface;
    /// relative motion emitted from physical delta only.
    pub(crate) fn locked_motion(
        &mut self,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    ) -> Vec<Effect> {
        let target = self.route_pointer().and_then(|(tgt, _)| tgt);
        vec![
            Effect::PointerRelative {
                target,
                delta,
                delta_unaccel,
                time_us,
            },
            Effect::PointerFrame,
        ]
    }

    /// F-CONFINE-1: Clamps physical pointer half-open to project_O(region) ∩ [0, logical_O).
    /// No Arrangement crossing.
    pub(crate) fn confined_motion(
        &mut self,
        output: output::OutputId,
        p: (f64, f64),
        delta: (f64, f64),
        c: &crate::pointer::ConstraintState,
    ) -> pointer::PointerState {
        let (Some(model), Some(view), Some(origin)) = (
            self.output(output),
            self.view(output),
            self.surface_origin(c.surface),
        ) else {
            return pointer::PointerState::Located { output, p };
        };

        let rect = c.region.unwrap_or_else(|| {
            let size = self
                .stage
                .element_containing_surface(c.surface)
                .and_then(|e| self.stage.tree(e))
                .and_then(|t| t.surfaces.iter().find(|s| s.key == c.surface))
                .map(|s| s.size)
                .unwrap_or_default();
            LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size,
            }
        });

        let x0 = (origin.x + rect.loc.x as i64) * geom::Q16_ONE;
        let y0 = (origin.y + rect.loc.y as i64) * geom::Q16_ONE;
        let x1 = (origin.x + rect.loc.x as i64 + rect.size.w as i64) * geom::Q16_ONE;
        let y1 = (origin.y + rect.loc.y as i64 + rect.size.h as i64) * geom::Q16_ONE;

        let p0 = view.project(geom::CanvasPos { x: x0, y: y0 });
        let p1 = view.project(geom::CanvasPos { x: x1, y: y1 });

        let rx_min = p0.x.min(p1.x);
        let rx_max = p0.x.max(p1.x);
        let ry_min = p0.y.min(p1.y);
        let ry_max = p0.y.max(p1.y);

        let extent = model.logical_extent();
        let min_x = rx_min.max(0.0);
        let max_x = rx_max.min(extent.0);
        let min_y = ry_min.max(0.0);
        let max_y = ry_max.min(extent.1);

        let clamped_x = (p.0 + delta.0).clamp(min_x, (max_x - pointer::WL_FIXED_EPS).max(min_x));
        let clamped_y = (p.1 + delta.1).clamp(min_y, (max_y - pointer::WL_FIXED_EPS).max(min_y));

        pointer::PointerState::Located {
            output,
            p: (clamped_x, clamped_y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputDescriptor, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, tests::key};

    fn tree(mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 100, h: 50 },
            },
            surfaces: vec![SnapshotSurface {
                key: key(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 100, h: 50 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        }
    }

    fn connected(scale_120: u32) -> Core {
        let (mut core, _i, _s, mut token) =
            Core::new(crate::navigation::PersistedCameras::default());
        core.output_connected(
            &mut token,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        core
    }

    /// B1: a surface-tree change that is not a root commit never drives M1 — in every lifecycle
    /// state the state, anchor and effects are those of a no-op, except that a Mapped element's
    /// snapshot is replaced (literal expectations, independent of `toplevel_committed`).
    #[test]
    fn tree_change_never_drives_the_lifecycle() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        // AwaitingInitialCommit: no placement, no configure
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        core.toplevel_committed(id, tree(false)).unwrap();
        // AwaitingFirstBuffer: a mapped-looking tree does not map it
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::AwaitingFirstBuffer));
        core.toplevel_committed(id, tree(true)).unwrap();
        // Mapped: an unmapped-looking tree does not unmap it; a mapped tree replaces the snapshot
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert!(core.stage().tree(id).is_some());
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        core.toplevel_committed(id, tree(false)).unwrap();
        // Unmapped: no re-arming of the initial configure, and the snapshot stays dropped even for
        // a mapped-looking tree (M1: an unmapped element has no hit-testable tree)
        assert_eq!(core.toplevel_tree_changed(id, tree(false)), Ok(vec![]));
        assert_eq!(core.toplevel_tree_changed(id, tree(true)), Ok(vec![]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert!(core.stage().tree(id).is_none());
        // a destroyed element is stale
        core.toplevel_destroyed(id);
        assert_eq!(
            core.toplevel_tree_changed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_map_unmap_remap_destroy() {
        let mut core = connected(120);
        let o = core.outputs()[0].id;
        let enter = Effect::Enter {
            output: o,
            surface: key(1),
        };
        let leave = Effect::Leave {
            output: o,
            surface: key(1),
        };
        let id = core.toplevel_created();
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![
                Effect::PreferredScale {
                    element: id,
                    scale_120: 120,
                    integer_scale: 1,
                },
                Effect::InitialConfigure(id),
            ])
        );
        // INV-STAGE-1 (C12.F1): a provisional anchor at the initial commit, the camera centre
        let provisional = core.stage().anchor(id).expect("placed at initial commit");
        assert_eq!(provisional, CanvasPoint { x: 0, y: 0 }, "camera centre");
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![]));
        // first buffer: mapped, on top, and its surface enters the output (F-MEMBER-1)
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        // default placement resolved at the first map: 100x50 centred on (0,0)
        let anchor = core.stage().anchor(id).expect("placed");
        assert_eq!(anchor, CanvasPoint { x: -50, y: -25 }, "F-PLACE-1");
        assert_eq!(core.stage().state(id), Some(Lifecycle::Mapped));
        assert_eq!(core.stage().z_order(), &[id]);
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Ok(vec![]),
            "no repeated enter"
        );
        // unmap keeps id and anchor, drops the snapshot, and leaves every output (M1)
        assert_eq!(core.toplevel_committed(id, tree(false)), Ok(vec![leave]));
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert!(core.stage().tree(id).is_none());
        // remap: initial commit again, anchor kept, on top
        let other = core.toplevel_created();
        core.toplevel_committed(other, tree(false)).unwrap();
        core.toplevel_committed(other, tree(true)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(false)),
            Ok(vec![Effect::InitialConfigure(id)])
        );
        assert_eq!(core.toplevel_committed(id, tree(true)), Ok(vec![enter]));
        assert_eq!(core.stage().anchor(id), Some(anchor));
        assert_eq!(
            core.stage().z_order(),
            &[other, id],
            "remap re-shows on top"
        );
        // destroy: no event to the dead client; membership rows dropped
        let _fx = core.toplevel_destroyed(id);
        assert!(!core.membership().rows().iter().any(|(e, _, _)| *e == id));
        assert!(
            core.toplevel_destroyed(id).is_empty(),
            "destroy is idempotent"
        );
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::StaleElement)
        );
    }

    #[test]
    fn m1_illegal_transitions_are_rejected_without_mutation() {
        let mut core = connected(120);
        let id = core.toplevel_created();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        assert_eq!(core.stage().anchor(id), None);
        let mut bad = tree(false);
        bad.surfaces.clear();
        assert!(matches!(
            core.toplevel_committed(id, bad),
            Err(ProtocolError::Snapshot(_))
        ));
        assert_eq!(
            core.stage().state(id),
            Some(Lifecycle::AwaitingInitialCommit)
        );
        // after unmap, a buffer before the new initial configure is rejected too
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.toplevel_committed(id, tree(true)),
            Err(ProtocolError::BufferBeforeConfigure)
        );
        assert_eq!(core.stage().state(id), Some(Lifecycle::Unmapped));
    }

    /// Protocol activity never touches cameras (INV-CAMERA-1).
    #[test]
    fn protocol_events_never_move_cameras() {
        let mut core = connected(150);
        let before = (
            core.cameras().center(0),
            core.cameras().zoom(0),
            core.cameras().len(),
        );
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        core.toplevel_destroyed(id);
        assert_eq!(
            before,
            (
                core.cameras().center(0),
                core.cameras().zoom(0),
                core.cameras().len()
            )
        );
    }

    /// M3 / R8 / D-B6-BOUNDARY: the target is the anchor output's viewport (here the parent's
    /// clock output), inward-rounded, relative to the parent's geometry origin (the anchor).
    /// Expected values are hand-derived with exact rational arithmetic
    /// (evidence/phase-f/INC-1/corrective/unproject_reference.py), not with production code.
    #[test]
    fn popup_target_is_the_viewport_inward_rounded() {
        // k = 1, camera centre (0,0), 1920x1080 → viewport [−960,960) × [−540,540). The target is
        // parent-relative: before the first map the anchor is the camera centre; after it the
        // 100x50 window is centred, anchor (−50,−25), and the viewport is at (−910,−515)
        let mut core = connected(120);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "unobserved parent: no anchor output (rule 3)"
        );
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -910, y: -515 },
                LocalSize { w: 1920, h: 1080 }
            )
        );
        // k = 1.75: c_eff = (−35951177, −20222537) Q16; the viewport's fractional edges
        // (−548.57…, 548.57…) × (−308.57…, 308.57…) round inward to [−548, 548) × [−308, 308);
        // relative to the anchor (−50,−25) that is (−498,−283)
        let mut core = connected(210);
        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();
        let t = core.popup_constraint_target(id, None).unwrap();
        assert_eq!(
            (t.loc, t.size),
            (
                LocalPoint { x: -498, y: -283 },
                LocalSize { w: 1096, h: 616 }
            )
        );
        core.toplevel_destroyed(id);
        assert_eq!(
            core.popup_constraint_target(id, None),
            None,
            "a destroyed parent has no target"
        );
    }

    #[test]
    fn popup_target_causing_output_rule1_and_rule2_fallback() {
        use crate::navigation::PersistedCameras;
        use crate::output::tests::key as okey;

        // Setup outputs: o1, o2, and o3 (with far-away camera)
        let persisted = PersistedCameras::from_entries(vec![(
            okey("out3"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 100_000 << 16,
            },
            1.0,
        )])
        .unwrap();
        let (mut core, _, _, mut tok) = Core::new(persisted);
        let (o1, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out1"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        let (o2, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out2"),
                    mode: (800, 600),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 120_000, // higher refresh -> would be clock output (rule 2)
                },
            )
            .unwrap();
        let (o3, _) = core
            .output_connected(
                &mut tok,
                OutputDescriptor {
                    key: okey("out3"),
                    mode: (640, 480),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 30_000,
                },
            )
            .unwrap();

        let id = core.toplevel_created();
        core.toplevel_committed(id, tree(false)).unwrap();
        core.toplevel_committed(id, tree(true)).unwrap();

        // If parent is visible on both o1 and o2:
        // Rule 1: causing_output = o1 selects o1, even though o2 has higher refresh!
        if core.membership().observers(id).contains(&o1)
            && core.membership().observers(id).contains(&o2)
        {
            let t_o1 = core.popup_constraint_target(id, Some(o1)).unwrap();
            let t_o2 = core.popup_constraint_target(id, Some(o2)).unwrap();
            assert_eq!(t_o1.size, LocalSize { w: 1920, h: 1080 });
            assert_eq!(t_o2.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 2 fallback: None selects clock output (o2)
        if core.membership().observers(id).contains(&o2) {
            let t_default = core.popup_constraint_target(id, None).unwrap();
            assert_eq!(t_default.size, LocalSize { w: 800, h: 600 });
        }

        // Rule 1 visibility guard (ATK-12 / Mutation P): o3 does not observe parent id.
        // Therefore passing Some(o3) must NOT select o3 (640x480), but fall back to Rule 2 (o2, 800x600)!
        assert!(!core.membership().observers(id).contains(&o3));
        let t_unobserved = core.popup_constraint_target(id, Some(o3)).unwrap();
        assert_eq!(t_unobserved.size, LocalSize { w: 800, h: 600 });
    }
    // ------------------------------------------------------------ toplevel requests (M2, M9)

    use crate::{InputPort, OutputLifecycleToken};

    /// A `w × h` single-surface tree with key `k`.
    fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(k),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        }
    }

    fn map_win(c: &mut Core, k: u64, w: i32, h: i32) -> ElementId {
        let id = c.toplevel_created();
        c.toplevel_committed(id, win(k, w, h, false)).unwrap();
        c.toplevel_committed(id, win(k, w, h, true)).unwrap();
        id
    }

    fn out(
        c: &mut Core,
        t: &mut OutputLifecycleToken,
        name: &str,
        mode: (u32, u32),
        scale_120: u32,
    ) -> OutputId {
        c.output_connected(
            t,
            OutputDescriptor {
                key: crate::output::tests::key(name),
                mode,
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap()
        .0
    }

    fn focus(c: &mut Core, i: &mut InputPort, id: ElementId) {
        c.set_focus(i, Some(id)).unwrap();
    }

    const FS: ToplevelRequest = ToplevelRequest::SetFullscreen { output: None };

    /// What a client's request must never change: anchors, lifecycle, z-order, cameras, focus.
    fn authority_state(c: &Core) -> String {
        let elements: Vec<_> = c
            .stage()
            .z_order()
            .iter()
            .map(|id| (*id, c.stage().anchor(*id), c.stage().state(*id)))
            .collect();
        format!(
            "{:?}",
            (
                elements,
                c.cameras().center(0),
                c.cameras().zoom(0),
                c.cameras().center(1),
                c.focus().target(),
            )
        )
    }

    fn rects(c: &mut Core, o: OutputId) -> Vec<(ElementId, (i64, i64, i64, i64))> {
        c.render_list(o)
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.element, (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)))
            .collect()
    }

    /// M2 as a table for an element that does not hold the focus: for every request in every
    /// lifecycle state, the decision and the effects. Stage, cameras and focus never change.
    #[test]
    fn toplevel_requests_are_answered_by_policy_and_change_nothing() {
        use ToplevelDecision::*;
        use ToplevelRequest::*;
        let reach = [
            Lifecycle::AwaitingInitialCommit,
            Lifecycle::AwaitingFirstBuffer,
            Lifecycle::Mapped,
            Lifecycle::Unmapped,
        ];
        for state in reach {
            for request in ToplevelRequest::ALL {
                let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
                out(&mut c, &mut t, "A", (1920, 1080), 120);
                let other = map_win(&mut c, 9, 300, 200);
                focus(&mut c, &mut i, other);
                let id = c.toplevel_created();
                if state != Lifecycle::AwaitingInitialCommit {
                    c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                }
                if matches!(state, Lifecycle::Mapped | Lifecycle::Unmapped) {
                    c.toplevel_committed(id, win(1, 100, 50, true)).unwrap();
                }
                if state == Lifecycle::Unmapped {
                    c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                }
                assert_eq!(c.stage().state(id), Some(state));
                let configured =
                    matches!(state, Lifecycle::AwaitingFirstBuffer | Lifecycle::Mapped);
                let want = match (request, state) {
                    (SetFullscreen { .. }, Lifecycle::AwaitingInitialCommit) => Recorded,
                    // before the first map: granted without the focus
                    (SetFullscreen { .. }, Lifecycle::AwaitingFirstBuffer) => Granted,
                    (SetMaximized | UnsetMaximized | SetFullscreen { .. } | UnsetFullscreen, _)
                        if configured =>
                    {
                        Declined
                    }
                    _ => Ignored,
                };
                let before = authority_state(&c);
                let (decision, fx) = c.toplevel_request(id, request).unwrap();
                assert_eq!(decision, want, "{request:?} in {state:?}");
                assert_eq!(
                    fx,
                    match want {
                        Granted | Declined => vec![Effect::ToplevelConfigure(id)],
                        Released => unreachable!(),
                        Recorded | Ignored => vec![],
                    },
                    "{request:?} in {state:?}"
                );
                assert_eq!(authority_state(&c), before, "{request:?} in {state:?}");
                assert_eq!(
                    c.fullscreen_of(id).is_some(),
                    want == Granted,
                    "{request:?} in {state:?}: stored state"
                );
            }
        }
    }

    /// A request on a destroyed element is stale, whatever it asks.
    #[test]
    fn a_toplevel_request_on_a_destroyed_element_is_stale() {
        let mut c = connected(120);
        let id = map_win(&mut c, 1, 100, 50);
        c.toplevel_destroyed(id);
        for request in ToplevelRequest::ALL {
            assert_eq!(
                c.toplevel_request(id, request),
                Err(ProtocolError::StaleElement)
            );
        }
    }

    /// What is advertised is what the policy can grant: fullscreen, and nothing else. Found by
    /// trying every request in every lifecycle state, with and without the focus.
    #[test]
    fn advertised_capabilities_are_exactly_what_the_policy_grants() {
        use ToplevelRequest::*;
        let mut granted = Vec::new();
        for focused in [false, true] {
            for stage in 0..4 {
                for request in ToplevelRequest::ALL {
                    let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
                    out(&mut c, &mut t, "A", (1920, 1080), 120);
                    let id = c.toplevel_created();
                    if stage >= 1 {
                        c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                    }
                    if stage >= 2 {
                        c.toplevel_committed(id, win(1, 100, 50, true)).unwrap();
                        if focused {
                            focus(&mut c, &mut i, id);
                        }
                    }
                    if stage == 3 {
                        c.toplevel_committed(id, win(1, 100, 50, false)).unwrap();
                    }
                    if c.toplevel_request(id, request).unwrap().0 == ToplevelDecision::Granted {
                        granted.push(request);
                    }
                }
            }
        }
        granted.dedup();
        assert!(granted.iter().all(|r| *r == FS), "{granted:?}");
        let caps = connected(120).toplevel_capabilities();
        assert_eq!(caps.fullscreen, granted.contains(&FS));
        assert_eq!(caps.maximize, granted.contains(&SetMaximized));
        assert_eq!(caps.minimize, granted.contains(&SetMinimized));
        assert_eq!(caps.window_menu, granted.contains(&ShowWindowMenu));
        assert_eq!(
            caps,
            ToplevelCapabilities {
                fullscreen: true,
                ..ToplevelCapabilities::default()
            }
        );
    }

    /// M9 rows 1, 2, 3 and C5.2 Case B, with values derived by hand. Output A 1920x1080 at scale
    /// 1, camera (0,0). W1 and W2 are 400x300, centred: anchor (−200,−150).
    ///
    /// - W2 (not focused) asks: declined, one configure, nothing stored.
    /// - W1 (focused) asks: granted on A. Saved size 400x300. Its configure carries `fullscreen`
    ///   and 1920x1080. W2 leaves A. While W1 is still 400x300 the presentation view centres it:
    ///   v_P = (−200,−150) − ((1920−400)/2, (1080−300)/2) = (−960,−540), so it is drawn at
    ///   [760,1160)x[390,690), where the camera showed it.
    /// - W1 commits 1920x1080: v_P = anchor, so it covers [0,1920)x[0,1080); the anchor is still
    ///   (−200,−150).
    /// - W1 asks to leave: one configure offering 400x300; W2 returns; the view is the camera's
    ///   again, exactly as before.
    #[test]
    fn fullscreen_is_granted_to_the_focused_element_and_presented_on_its_output() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 400, 300);
        focus(&mut c, &mut i, w1);
        let camera_view = c.view(a).unwrap();
        let untouched = authority_state(&c);
        assert_eq!(
            rects(&mut c, a),
            vec![(w2, (760, 390, 1160, 690)), (w1, (760, 390, 1160, 690))]
        );

        // not focused: declined
        let (d, fx) = c.toplevel_request(w2, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w2)]
            )
        );
        assert!(
            c.fullscreen().is_empty(),
            "a declined request stored something"
        );
        assert_eq!(
            c.toplevel_configure_state(w2),
            ToplevelConfigureState::default()
        );

        // focused: granted
        let (d, fx) = c.toplevel_request(w1, FS).unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(
            fx[0],
            Effect::ToplevelConfigure(w1),
            "the answer comes first"
        );
        assert_eq!(
            fx.iter()
                .filter(|e| matches!(
                    e,
                    Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
                ))
                .count(),
            1,
            "exactly one configure: {fx:?}"
        );
        assert!(
            fx.contains(&Effect::Leave {
                output: a,
                surface: key(2)
            }),
            "the other element leaves the output: {fx:?}"
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Leave { surface, .. } if *surface == key(1)))
        );
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w1,
                output: a,
                saved: Some(LocalSize { w: 400, h: 300 })
            }]
        );
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState {
                fullscreen: Some(a),
                resizing: false,
                size: Some(LocalSize { w: 1920, h: 1080 })
            }
        );
        assert_eq!(c.presentation(a), Some(w1));
        assert_eq!(c.membership().observers(w2), vec![], "W2 is not shown on A");
        let v = c.view(a).unwrap();
        assert_eq!(v.only, Some(w1));
        assert_eq!(
            (v.v.x, v.v.y, v.zoom),
            (-960 * Q16_ONE, -540 * Q16_ONE, 1.0)
        );
        assert_eq!(rects(&mut c, a), vec![(w1, (760, 390, 1160, 690))]);
        assert_eq!(
            authority_state(&c),
            untouched,
            "a grant moved an anchor, a camera or the focus"
        );

        // the client resizes to the configured size
        let fx = c.toplevel_committed(w1, win(1, 1920, 1080, true)).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::ToplevelConfigure(_))),
            "{fx:?}"
        );
        assert_eq!(c.stage().anchor(w1), Some(CanvasPoint { x: -200, y: -150 }));
        let v = c.view(a).unwrap();
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE, -150 * Q16_ONE),
            "v_P = anchor"
        );
        assert_eq!(rects(&mut c, a), vec![(w1, (0, 0, 1920, 1080))]);
        // every pixel of the output is W1's: the corners hit it at its own local coordinates
        for (p, local) in [((0.5, 0.5), (0, 0)), ((1919.5, 1079.5), (1919, 1079))] {
            let hit = c
                .hit_test(a, crate::geom::OutputLogicalPoint { x: p.0, y: p.1 })
                .unwrap();
            assert_eq!(hit.element, w1);
            assert_eq!((hit.local.x / Q16_ONE, hit.local.y / Q16_ONE), local);
        }
        // popups of the fullscreen element are constrained to the output: its whole surface
        assert_eq!(
            c.popup_constraint_target(w1, None),
            Some(LocalRect {
                loc: LocalPoint { x: 0, y: 0 },
                size: LocalSize { w: 1920, h: 1080 }
            })
        );

        // a second request while fullscreen is answered, and the saved size is not overwritten
        let (d, fx) = c.toplevel_request(w1, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Granted,
                vec![Effect::ToplevelConfigure(w1)]
            )
        );
        assert_eq!(c.fullscreen()[0].saved, Some(LocalSize { w: 400, h: 300 }));

        // leave
        let (d, fx) = c
            .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(d, ToplevelDecision::Released);
        assert_eq!(
            fx[0],
            Effect::ToplevelConfigureSize {
                element: w1,
                size: LocalSize { w: 400, h: 300 }
            }
        );
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(2)
            }),
            "W2 returns: {fx:?}"
        );
        assert!(c.fullscreen().is_empty());
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState::default()
        );
        assert_eq!(
            c.view(a),
            Some(camera_view),
            "the camera's view, exactly as it was"
        );
        assert_eq!(authority_state(&c), untouched);
        // not fullscreen any more: a further unset is answered once and changes nothing
        let (d, fx) = c
            .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w1)]
            )
        );
    }

    /// M2 pre-configure and C5.2 "request before first map": the request is recorded, the
    /// initial configure carries `fullscreen` and the output's size, nothing is presented until
    /// the element is mapped, and placement runs regardless. An unset before the initial commit
    /// withdraws it. After a remap the client must ask again, and then needs the focus.
    #[test]
    fn a_fullscreen_request_before_the_first_map_is_granted_in_the_initial_configure() {
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let other = map_win(&mut c, 9, 300, 200);
        let w = c.toplevel_created();
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (ToplevelDecision::Recorded, vec![])
        );
        assert!(
            c.fullscreen().is_empty(),
            "decided at the initial commit, not before"
        );
        let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        // with the scale policy present the initial hint precedes the configure (M15)
        assert!(
            matches!(fx.first(), Some(Effect::PreferredScale { element, .. }) if *element == w),
            "the initial scale hint comes first: {fx:?}"
        );
        assert_eq!(
            fx[1..],
            [Effect::InitialConfigure(w)],
            "one configure, the initial one"
        );
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState {
                fullscreen: Some(a),
                resizing: false,
                size: Some(LocalSize { w: 1920, h: 1080 })
            }
        );
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w,
                output: a,
                saved: None
            }]
        );
        assert_eq!(
            c.presentation(a),
            None,
            "nothing of W exists to present yet"
        );
        assert_eq!(
            c.membership().observers(other),
            vec![a],
            "the output still shows the Stage"
        );
        // first map at the configured size: presented, and the other element leaves. It is not
        // placed as a window yet: the anchor is still the provisional one, the camera centre
        let fx = c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: 0, y: 0 }));
        assert!(
            !c.stage().default_placed(w),
            "a fullscreen map used up the placement"
        );
        assert!(
            fx.contains(&Effect::Leave {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(1)
            }),
            "{fx:?}"
        );
        assert_eq!(c.presentation(a), Some(w));
        assert_eq!(rects(&mut c, a), vec![(w, (0, 0, 1920, 1080))]);
        // leaving: no size was saved, so none is offered
        let (d, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(d, ToplevelDecision::Released);
        assert_eq!(fx[0], Effect::ToplevelConfigure(w));
        // unmap, re-show: before the buffer, without the focus, the request is declined
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (ToplevelDecision::Ignored, vec![])
        );
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert_eq!(c.stage().state(w), Some(Lifecycle::AwaitingFirstBuffer));
        assert_eq!(
            c.toplevel_request(w, FS).unwrap(),
            (
                ToplevelDecision::Declined,
                vec![Effect::ToplevelConfigure(w)]
            ),
            "a remap is not a first map"
        );

        // withdrawn before the initial commit
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        assert_eq!(
            c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
                .unwrap(),
            (ToplevelDecision::Ignored, vec![])
        );
        c.toplevel_committed(x, win(3, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(x),
            ToplevelConfigureState::default()
        );
        // between the initial configure and the first buffer: granted at once, one configure
        let (d, fx) = c.toplevel_request(x, FS).unwrap();
        assert_eq!(
            (d, fx),
            (
                ToplevelDecision::Granted,
                vec![Effect::ToplevelConfigure(x)]
            )
        );
    }

    /// A window whose first map is in protocol fullscreen keeps its default-placement
    /// opportunity. It is placed by F-PLACE-1 at its first windowed presentation: the first
    /// commit, after fullscreen ended, whose geometry differs from the one it had then. Commits
    /// still in flight at the fullscreen size are not that. After it, the anchor is final.
    /// Camera at (700, −300), so every value differs from the default-camera case:
    /// provisional anchor (700, −300); 400x300 centred there is (500, −450).
    #[test]
    fn a_window_that_starts_fullscreen_is_placed_at_its_first_windowed_size() {
        let cam = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("A"),
            CanvasPos {
                x: 700 * Q16_ONE,
                y: -300 * Q16_ONE,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, _i, _, mut t) = Core::new(cam);
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        let provisional = CanvasPoint { x: 700, y: -300 };
        assert_eq!(c.stage().anchor(w), Some(provisional));
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (0, 0, 1920, 1080))],
            "presented at the origin"
        );
        // a second commit while fullscreen changes nothing about placement
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert!(!c.stage().default_placed(w));

        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(provisional),
            "the exit itself moves nothing"
        );
        // frames still in flight: committed before the client acknowledged the exit, whatever
        // their size (P9b: the acknowledgement, not the size, marks the first windowed frame)
        c.toplevel_committed_acked(w, win(1, 1920, 1080, true), true)
            .unwrap();
        c.toplevel_committed_acked(w, win(1, 400, 300, true), true)
            .unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(provisional),
            "placed at a frame committed under the fullscreen state"
        );
        // the first commit after the acknowledgement: placed now, centred on the camera
        c.toplevel_committed_acked(w, win(1, 400, 300, true), false)
            .unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: 500, y: -450 }));
        assert!(c.stage().default_placed(w));
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (760, 390, 1160, 690))],
            "centred in the view"
        );
        // final: a later size, another fullscreen round trip and a remap keep it
        let placed = CanvasPoint { x: 500, y: -450 };
        c.toplevel_committed(w, win(1, 640, 480, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(placed));
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 300, 200, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(placed), "a remap placed it again");
    }

    /// P9b: a client that keeps the fullscreen size as its windowed size is placed at its first
    /// windowed commit (oversized: F-PLACE-1's fallback puts its origin at the view's corner);
    /// a second fullscreen round trip before it acknowledges keeps the placement waiting; Core
    /// never resizes it.
    #[test]
    fn a_window_keeping_the_fullscreen_size_is_placed_at_its_first_windowed_commit() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        focus(&mut c, &mut i, w);
        let provisional = c.stage().anchor(w);
        // out of fullscreen and back in before the client acknowledged the exit
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c.toplevel_request(w, FS).unwrap().1;
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::ToplevelConfigureSize { .. }))
        );
        c.toplevel_committed_acked(w, win(1, 1920, 1080, true), true)
            .unwrap();
        assert_eq!(c.stage().anchor(w), provisional, "placed while fullscreen");
        // the exit, acknowledged; the client keeps 1920x1080. Placement sends nothing: no
        // configure, no size (Core never resizes a client to place it)
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c
            .toplevel_committed_acked(w, win(1, 1920, 1080, true), false)
            .unwrap();
        assert!(
            !fx.iter().any(|e| matches!(
                e,
                Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
            )),
            "placement configured the client: {fx:?}"
        );
        assert!(
            c.stage().default_placed(w),
            "a window keeping its size is never placed"
        );
        // 1920x1080 fills the 1920x1080 view exactly: its origin at the view's corner
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -960, y: -540 }));
        assert!(c.placements_pending().is_empty());
    }

    /// The other ways such a window first becomes a window: fullscreen ends by replacement or by
    /// the removal of its output (placed at its next size, like after an unset); it is unmapped
    /// while fullscreen and re-shown as a window (placed at that map). And a window that was
    /// placed before going fullscreen returns to exactly its anchor: nothing is re-resolved.
    #[test]
    fn placement_after_fullscreen_covers_every_exit_and_restores_existing_anchors() {
        // unmapped while fullscreen-first, re-shown windowed
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = c.toplevel_created();
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap(); // unmap
        assert!(c.fullscreen().is_empty());
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(CanvasPoint { x: -200, y: -150 }),
            "placed at the re-show"
        );

        // replaced by a newer grant, then resized
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(2, 0, 0, false)).unwrap();
        c.toplevel_committed(x, win(2, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(x));
        // W is hidden on A; a second output shows it, so it may take the focus (A-8)
        let b = out(&mut c, &mut t, "B", (1920, 1080), 120);
        focus(&mut c, &mut i, w);
        let on_a = ToplevelRequest::SetFullscreen { output: Some(a) };
        c.toplevel_request(w, on_a).unwrap(); // replaces x on A
        c.output_disconnected(&mut t, b).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        assert_eq!(
            c.stage().anchor(x),
            Some(CanvasPoint { x: 0, y: 0 }),
            "still provisional"
        );
        c.toplevel_committed(x, win(2, 600, 400, true)).unwrap();
        assert_eq!(c.stage().anchor(x), Some(CanvasPoint { x: -300, y: -200 }));

        // w was a placed window before it went fullscreen: its anchor comes back untouched
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));
        c.toplevel_committed(w, win(1, 500, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(w),
            Some(CanvasPoint { x: -200, y: -150 }),
            "re-centred"
        );

        // its output is removed while it is fullscreen-first; destroyed before its first
        // windowed size, it leaves nothing pending for a later element
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let y = c.toplevel_created();
        c.toplevel_request(y, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(y, win(3, 0, 0, false)).unwrap();
        c.toplevel_committed(y, win(3, 1280, 720, true)).unwrap();
        c.output_disconnected(&mut t, b).unwrap();
        assert!(c.fullscreen().is_empty());
        assert!(!c.stage().default_placed(y));
        c.toplevel_committed(y, win(3, 500, 300, true)).unwrap();
        assert_eq!(
            c.stage().anchor(y),
            Some(CanvasPoint { x: -250, y: -150 }),
            "placed on the surviving output"
        );
        let z = c.toplevel_created();
        c.toplevel_request(z, FS).unwrap();
        c.toplevel_committed(z, win(4, 0, 0, false)).unwrap();
        c.toplevel_committed(z, win(4, 1920, 1080, true)).unwrap();
        c.toplevel_request(z, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.placements_pending(), vec![z], "waiting for its size");
        c.toplevel_destroyed(z);
        assert!(c.placements_pending().is_empty(), "gone with the element");
        // the same when it is unmapped instead
        let u = c.toplevel_created();
        c.toplevel_request(u, FS).unwrap();
        c.toplevel_committed(u, win(6, 0, 0, false)).unwrap();
        c.toplevel_committed(u, win(6, 1920, 1080, true)).unwrap();
        c.toplevel_request(u, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.placements_pending(), vec![u]);
        c.toplevel_committed(u, win(6, 0, 0, false)).unwrap(); // unmap
        assert!(c.placements_pending().is_empty(), "gone with the unmap");
        c.toplevel_destroyed(u);
        let n = c.toplevel_created();
        c.toplevel_committed(n, win(5, 0, 0, false)).unwrap();
        c.toplevel_committed(n, win(5, 200, 100, true)).unwrap();
        c.toplevel_committed(n, win(5, 300, 100, true)).unwrap();
        assert_eq!(
            c.stage().anchor(n),
            Some(CanvasPoint { x: -100, y: -50 }),
            "an ordinary window, placed once"
        );
    }

    /// M9 rows 4 and 5: losing the focus does not end fullscreen; a newer grant on the same
    /// output replaces the older one, whose element gets one configure with its saved size.
    #[test]
    fn a_newer_grant_replaces_the_older_one_and_focus_loss_does_not() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w1 = map_win(&mut c, 1, 400, 300);
        let w2 = map_win(&mut c, 2, 500, 350);
        focus(&mut c, &mut i, w1);
        c.toplevel_request(w1, FS).unwrap();
        // W2 is hidden on A by the presentation; a second output shows it, so it may take the
        // focus (A-8), and it names A in its request
        out(&mut c, &mut t, "B", (1920, 1080), 120);
        focus(&mut c, &mut i, w2);
        assert_eq!(c.focus().target(), Some(w2));
        assert_eq!(c.presentation(a), Some(w1), "focus loss ended fullscreen");
        assert_eq!(c.fullscreen().len(), 1);
        let on_a = ToplevelRequest::SetFullscreen { output: Some(a) };
        let (d, fx) = c.toplevel_request(w2, on_a).unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(fx[0], Effect::ToplevelConfigure(w2));
        assert_eq!(
            fx[1],
            Effect::ToplevelConfigureSize {
                element: w1,
                size: LocalSize { w: 400, h: 300 }
            },
            "the replaced element is told, with its saved size"
        );
        assert!(fx.contains(&Effect::Leave {
            output: a,
            surface: key(1)
        }));
        assert!(fx.contains(&Effect::Enter {
            output: a,
            surface: key(2)
        }));
        assert_eq!(
            c.fullscreen(),
            &[FullscreenState {
                element: w2,
                output: a,
                saved: Some(LocalSize { w: 500, h: 350 })
            }]
        );
        assert_eq!(
            c.toplevel_configure_state(w1),
            ToplevelConfigureState::default()
        );
    }

    /// Every way a presentation ends leaves no state behind: unmap, destruction (mapped, and
    /// between grant and first map), and removal of the output, which sends the configure with
    /// the saved size before the output goes. A reconfigured output re-sends one configure with
    /// its new size.
    #[test]
    fn fullscreen_ends_cleanly_on_unmap_destroy_and_output_changes() {
        // unmap
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let other = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(c.membership().observers(other), vec![]);
        let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert!(c.fullscreen().is_empty(), "state survived the unmap");
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::ToplevelConfigureSize { .. })),
            "an unmapped surface was sent a size: {fx:?}"
        );
        assert_eq!(c.presentation(a), None);
        // remapped: not fullscreen, and placed where it was
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState::default()
        );
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));

        // destroyed while presented
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        let fx = c.toplevel_destroyed(w);
        assert!(c.fullscreen().is_empty());
        assert!(
            fx.contains(&Effect::Enter {
                output: a,
                surface: key(9)
            }),
            "{fx:?}"
        );
        assert_eq!(c.membership().observers(other), vec![a]);
        // destroyed between the grant and the first map, and with a request only recorded
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(3, 0, 0, false)).unwrap();
        assert_eq!(c.fullscreen().len(), 1);
        c.toplevel_destroyed(x);
        let y = c.toplevel_created();
        c.toplevel_request(y, FS).unwrap();
        c.toplevel_destroyed(y);
        assert!(c.fullscreen().is_empty());
        let z = c.toplevel_created();
        c.toplevel_committed(z, win(4, 0, 0, false)).unwrap();
        assert_eq!(
            c.toplevel_configure_state(z),
            ToplevelConfigureState::default(),
            "a dead element's recorded request reached another"
        );

        // output reconfigured, then removed
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        let (d, _) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(c.fullscreen()[0].output, b, "the output the client named");
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        assert_eq!(
            c.membership().observers(w),
            vec![a, b],
            "A's camera still shows it"
        );
        let fx = c
            .output_reconfigured(
                &mut t,
                b,
                OutputDescriptor {
                    key: crate::output::tests::key("B"),
                    mode: (2560, 1440),
                    transform: Transform::Normal,
                    scale_120: 240,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        assert_eq!(
            fx.iter()
                .filter(|e| **e == Effect::ToplevelConfigure(w))
                .count(),
            1,
            "one configure for the element in fullscreen on it: {fx:?}"
        );
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        let fx = c.output_disconnected(&mut t, b).unwrap();
        let restore = fx
            .iter()
            .position(|e| {
                *e == Effect::ToplevelConfigureSize {
                    element: w,
                    size: LocalSize { w: 400, h: 300 },
                }
            })
            .expect("the configure with the saved size");
        let removed = fx
            .iter()
            .position(|e| *e == Effect::OutputRemoved(b))
            .unwrap();
        assert!(restore < removed);
        assert!(c.fullscreen().is_empty());
        assert_eq!(
            c.toplevel_configure_state(w),
            ToplevelConfigureState::default()
        );
    }

    /// C5.2 output choice: the named output if it exists; else the frame-clock output of an
    /// observed element; else the output under the pointer.
    #[test]
    fn the_fullscreen_output_is_chosen_by_the_sealed_rule() {
        let far = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("B"),
            CanvasPos {
                x: 50_000 * Q16_ONE,
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(far);
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1280, 720), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        assert_eq!(c.membership().observers(w), vec![a], "shown on A only");
        let on = |c: &mut Core, output| {
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output })
                .unwrap();
            let o = c.fullscreen()[0].output;
            c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            o
        };
        assert_eq!(on(&mut c, Some(b)), b, "named");
        assert_eq!(on(&mut c, None), a, "its clock output");
        let gone = {
            let x = out(&mut c, &mut t, "X", (800, 600), 120);
            c.output_disconnected(&mut t, x).unwrap();
            x
        };
        assert_eq!(
            on(&mut c, Some(gone)),
            a,
            "a named output that no longer exists"
        );
        // presented on B while A's camera still shows it at its real size and place
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.presentation(b), Some(w));
        assert_eq!(c.presentation(a), None);
        assert_eq!(rects(&mut c, a), vec![(w, (760, 390, 1160, 690))]);
        // centred on B: (1280−400)/2 = 440, (720−300)/2 = 210
        assert_eq!(rects(&mut c, b), vec![(w, (440, 210, 840, 510))]);
        // its popups are constrained to the fullscreen output B, whose origin is (440, 210) up
        // and left of the window — not to A's camera view, which would be (−760, −390) 1920x1080
        assert_eq!(
            c.popup_constraint_target(w, Some(a)),
            Some(LocalRect {
                loc: LocalPoint { x: -440, y: -210 },
                size: LocalSize { w: 1280, h: 720 },
            })
        );
        assert_eq!(
            c.cameras().center(1),
            Some(CanvasPos {
                x: 50_000 * Q16_ONE,
                y: 0
            })
        );
    }

    /// F-PRESENT-1 on fractional outputs, by hand. The anchor of a 400x300 window centred on a
    /// camera at (0,0) is (−200,−150).
    ///
    /// - 1920x1080 at scale 1.5: logical 1280x720, the configure size. Smaller window: margins
    ///   (440, 210), v_P = (−640,−360), drawn at physical [660,1260)x[315,765). A 2000x1000
    ///   window: wider than the output, so x is origin-aligned and cropped (v_P.x = anchor.x);
    ///   taller too (1000 > 720): v_P.y = anchor.y.
    /// - 1366x768 at scale 1.25: logical 1092.8x614.4; the configure size rounds down to
    ///   1092x614, and a window of that size has margins (0.4, 0.2).
    #[test]
    fn the_presentation_view_is_f_present_1() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 180);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1280, h: 720 })
        );
        let v = c.view(a).unwrap();
        assert_eq!((v.v.x, v.v.y, v.k), (-640 * Q16_ONE, -360 * Q16_ONE, 1.5));
        assert_eq!(rects(&mut c, a), vec![(w, (660, 315, 1260, 765))]);
        c.toplevel_committed(w, win(1, 2000, 1000, true)).unwrap();
        let v = c.view(a).unwrap();
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE, -150 * Q16_ONE),
            "origin-aligned"
        );
        assert_eq!(
            rects(&mut c, a),
            vec![(w, (0, 0, 3000, 1500))],
            "cropped by the output"
        );

        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1366, 768), 150);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            c.toplevel_configure_state(w).size,
            Some(LocalSize { w: 1092, h: 614 })
        );
        c.toplevel_committed(w, win(1, 1092, 614, true)).unwrap();
        let v = c.view(a).unwrap();
        // v_P = (−200 − 0.4, −150 − 0.2), Q16 by R7: 0.4·65536 = 26214.4 → 26214, 0.2·65536 = 13107.2 → 13107
        assert_eq!(
            (v.v.x, v.v.y),
            (-200 * Q16_ONE - 26214, -150 * Q16_ONE - 13107)
        );
    }

    /// A-8: keyboard focus does not move to an element a presentation hides (an output presents
    /// another element and its camera would show this one, and no output observes it): neither
    /// by `set_focus`, nor FocusNext, nor the initial grant, nor as the successor of a lost
    /// target. The same element is focusable while another output shows it.
    #[test]
    fn focus_does_not_move_to_an_element_a_presentation_hides() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        let o = map_win(&mut c, 2, 200, 100);
        let v = map_win(&mut c, 3, 100, 100);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(w));
        assert!(c.hidden_by_presentation(o) && c.hidden_by_presentation(v));
        assert!(
            !c.hidden_by_presentation(w),
            "the presenter is never hidden"
        );

        // set_focus is declined; FocusNext cycles among what may be focused (W alone)
        assert_eq!(c.set_focus(&mut i, Some(o)).unwrap(), vec![]);
        assert_eq!(c.focus().target(), Some(w));
        c.focus_next();
        assert_eq!(
            c.focus().target(),
            Some(w),
            "FocusNext moved to a hidden element"
        );

        // a second output whose camera shows O and V: they are not hidden there
        let b = out(&mut c, &mut t, "B", (1920, 1080), 120);
        assert!(!c.membership().observers(o).is_empty());
        assert!(!c.hidden_by_presentation(o));
        focus(&mut c, &mut i, o);
        focus(&mut c, &mut i, v);
        assert_eq!(c.focus().target(), Some(v));
        // B goes: O and V are hidden again; V keeps the focus it had (nothing moved it)
        c.output_disconnected(&mut t, b).unwrap();
        assert!(c.hidden_by_presentation(o) && c.hidden_by_presentation(v));
        // V goes: the successor skips O (more recent, hidden) for W
        c.toplevel_destroyed(v);
        assert_eq!(
            c.focus().target(),
            Some(w),
            "the successor was a hidden element"
        );

        // the presentation ends: O is focusable again
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert!(!c.hidden_by_presentation(o));
        focus(&mut c, &mut i, o);
        assert_eq!(c.focus().target(), Some(o));

        // the initial grant (nothing focused yet) skips a hidden element
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let o = map_win(&mut c, 2, 200, 100);
        let x = c.toplevel_created();
        c.toplevel_request(x, FS).unwrap();
        c.toplevel_committed(x, win(4, 0, 0, false)).unwrap();
        c.toplevel_committed(x, win(4, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(a), Some(x));
        assert_eq!(c.grant_initial_focus(&mut i, o).unwrap(), vec![]);
        assert_eq!(c.focus().target(), None);
        assert!(c.focus().is_initial_eligible());
        c.grant_initial_focus(&mut i, x).unwrap();
        assert_eq!(c.focus().target(), Some(x));
    }

    /// A-3: a grant made before an element's first map takes the output only when the element
    /// maps. Until then the presented element keeps it and is told nothing; a newcomer that is
    /// destroyed or unmapped first, or never draws, ends nothing. Two waiting grants take the
    /// output in the order their elements map. A grant to a mapped, focused element still
    /// replaces every older grant on the output, a waiting one included. Output removal ends
    /// every grant on the output.
    #[test]
    fn a_pre_map_grant_takes_the_output_when_its_element_maps() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let o = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(o), Some(w));
        let told = |fx: &[Effect], e: ElementId| {
            fx.iter().any(|f| {
                matches!(f, Effect::ToplevelConfigure(x) | Effect::ToplevelConfigureSize { element: x, .. } if *x == e)
            })
        };
        // a newcomer asks before its initial commit: granted, waiting, W untouched
        let premap = |c: &mut Core, k: u64| {
            let x = c.toplevel_created();
            assert_eq!(
                c.toplevel_request(x, FS).unwrap().0,
                ToplevelDecision::Recorded
            );
            let fx = c.toplevel_committed(x, win(k, 0, 0, false)).unwrap();
            (x, fx)
        };
        let (x, fx) = premap(&mut c, 2);
        assert!(
            !told(&fx, w),
            "W was told to leave at another element's initial commit"
        );
        assert_eq!(c.presentation(o), Some(w));
        assert_eq!(c.fullscreen_of(x).map(|f| f.output), Some(o));
        // destroyed before drawing: nothing ends
        let fx = c.toplevel_destroyed(x);
        assert!(!told(&fx, w));
        assert_eq!(c.presentation(o), Some(w));
        assert_eq!(c.fullscreen_of(w).map(|f| f.output), Some(o));

        // two waiting newcomers, the older maps first: it takes the output and the newer keeps
        // waiting; when the newer maps, it replaces the older
        let (x, _) = premap(&mut c, 3);
        let (y, _) = premap(&mut c, 4);
        assert_eq!(c.presentation(o), Some(w));
        let fx = c.toplevel_committed(x, win(3, 1920, 1080, true)).unwrap();
        assert!(
            told(&fx, w),
            "W keeps the output after a newer grant's element mapped"
        );
        assert!(
            !told(&fx, y),
            "a newer waiting grant was ended by an older one's map"
        );
        assert_eq!(c.presentation(o), Some(x));
        assert_eq!(c.fullscreen_of(w), None);
        let fx = c.toplevel_committed(y, win(4, 1920, 1080, true)).unwrap();
        assert!(told(&fx, x));
        assert_eq!(c.presentation(o), Some(y));
        // the newer maps first: it replaces the presenter and the older waiting grant
        let (p, _) = premap(&mut c, 8);
        let (q, _) = premap(&mut c, 9);
        let fx = c.toplevel_committed(q, win(9, 1920, 1080, true)).unwrap();
        assert!(
            told(&fx, y) && told(&fx, p),
            "older grants survived a newer one's map"
        );
        assert_eq!(c.fullscreen_of(p), None);
        c.toplevel_committed(p, win(8, 640, 480, true)).unwrap();
        assert_eq!(
            c.presentation(o),
            Some(q),
            "an ended waiting grant took the output"
        );
        let x = q;

        // the presenter unmaps while a grant waits: no presentation until the waiting one maps
        let (z, _) = premap(&mut c, 5);
        c.toplevel_committed(x, win(3, 1920, 1080, false)).unwrap();
        assert_eq!(c.presentation(o), None);
        c.toplevel_committed(z, win(5, 1920, 1080, true)).unwrap();
        assert_eq!(c.presentation(o), Some(z));

        // a focused, mapped element's grant replaces the presenter and a waiting grant
        let (v, _) = premap(&mut c, 6);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let fx = c.toplevel_request(w, FS).unwrap().1;
        assert!(
            told(&fx, z) && told(&fx, v),
            "older grants on the output survived"
        );
        assert_eq!(c.fullscreen_of(v), None);
        c.toplevel_committed(v, win(6, 640, 480, true)).unwrap();
        assert_eq!(
            c.presentation(o),
            Some(w),
            "a replaced waiting grant took the output"
        );

        // output removal ends a waiting grant too
        let (u, _) = premap(&mut c, 7);
        assert!(c.fullscreen_of(u).is_some());
        c.output_disconnected(&mut t, o).unwrap();
        assert_eq!(c.fullscreen_of(u), None);
        assert_eq!(c.fullscreen_of(w), None);
    }

    /// M9 "configure with the saved size": the size offered is the last *windowed* size. A grant
    /// made while the element still has its fullscreen geometry (it was told to leave and has not
    /// redrawn) saves the size on offer again; once the element redraws at another size the offer
    /// is spent and the geometry is what is saved. Unmap and destruction forget the offer.
    #[test]
    fn the_saved_size_is_the_last_windowed_size() {
        const UNFS: ToplevelRequest = ToplevelRequest::UnsetFullscreen;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        let offer = |size: (i32, i32)| Effect::ToplevelConfigureSize {
            element: w,
            size: LocalSize {
                w: size.0,
                h: size.1,
            },
        };
        let saved = |c: &Core| c.fullscreen_of(w).and_then(|f| f.saved).map(|s| (s.w, s.h));

        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((400, 300)));
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((400, 300)));
        // again before the element redrew: its geometry is still 1920x1080
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(
            saved(&c),
            Some((400, 300)),
            "the fullscreen geometry was saved"
        );
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((400, 300)));

        // the element redraws at a size of its own choosing: the offer is spent
        c.toplevel_committed(w, win(1, 500, 350, true)).unwrap();
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((500, 350)));
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        assert_eq!(c.toplevel_request(w, UNFS).unwrap().1[0], offer((500, 350)));

        // an unmap forgets the offer: after the re-show the geometry is the only size known
        c.toplevel_committed(w, win(1, 1920, 1080, false)).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, false)).unwrap();
        c.toplevel_committed(w, win(1, 640, 480, true)).unwrap();
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        assert_eq!(saved(&c), Some((640, 480)));
    }

    /// "Fullscreen never touches cameras": a camera drag on a fullscreen output moves that
    /// output's camera and not what the output shows; when fullscreen ends, the output shows the
    /// camera where the drag left it.
    #[test]
    fn a_presentation_is_independent_of_the_camera() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, FS).unwrap();
        c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
        let presented = c.view(a).unwrap();
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: logo,
            time_us: 0,
        };
        c.physical_input_vec(&mut i, button(true)).unwrap();
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (-300.0, 0.0),
                delta_unaccel: (-300.0, 0.0),
                time_us: 0,
            },
        )
        .unwrap();
        c.physical_input_vec(&mut i, button(false)).unwrap();
        assert_eq!(
            c.cameras().center(0),
            Some(CanvasPos {
                x: 300 * Q16_ONE,
                y: 0
            }),
            "the camera moved"
        );
        assert_eq!(c.view(a), Some(presented), "what the output shows did not");
        assert_eq!(c.stage().anchor(w), Some(CanvasPoint { x: -200, y: -150 }));
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let v = c.view(a).unwrap();
        assert_eq!((v.only, v.v.x), (None, (300 - 960) * Q16_ONE));
    }

    // ------------------------------------------------------------ placement at first map

    fn sized(w: i32, h: i32, mapped: bool) -> SurfaceTree {
        SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w, h },
            },
            surfaces: vec![SnapshotSurface {
                key: key(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w, h }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        }
    }

    use crate::PhysicalEvent;
    use crate::input::bindings::Modifiers;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key as okey;

    fn plain(connector: &str) -> OutputDescriptor {
        OutputDescriptor {
            key: okey(connector),
            mode: (1280, 800),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        }
    }

    fn placed(c: &mut Core, w: i32, h: i32) -> (ElementId, CanvasPoint) {
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(w, h, false)).unwrap();
        c.toplevel_committed(id, sized(w, h, true)).unwrap();
        (id, c.stage().anchor(id).unwrap())
    }

    fn centred(mode: (u32, u32), scale_120: u32, camera: Option<(i64, i64)>) -> Core {
        let entries = camera
            .map(|(x, y)| {
                vec![(
                    okey("A"),
                    CanvasPos {
                        x: x * Q16_ONE,
                        y: y * Q16_ONE,
                    },
                    1.0,
                )]
            })
            .unwrap_or_default();
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::from_entries(entries).unwrap());
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let d = OutputDescriptor {
            key: okey("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        c.output_connected(&mut t, d).unwrap();
        c
    }

    /// Core's default is the centred policy (C12.F1). The legacy rule stays available as an
    /// explicit policy: the geometry origin at the camera centre, decided at the initial commit
    /// and not revised at the first map, for any size.
    #[test]
    fn the_legacy_policy_places_the_origin_at_the_camera_centre() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(c.placement_policy(), PlacementPolicy::CentredInView);
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: 0, y: 0 });
        assert_eq!(placed(&mut c, 5000, 4000).1, CanvasPoint { x: 0, y: 0 });
        // a window placed under the legacy rule keeps its anchor when the policy changes
        let (id, a) = placed(&mut c, 400, 300);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        c.toplevel_committed(id, sized(400, 300, true)).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
        assert_eq!(placed(&mut c, 400, 300).1, CanvasPoint { x: -200, y: -150 });
    }

    /// The legacy rule through a persisted camera away from the origin, negative coordinates
    /// included: the anchor is the camera centre, rounded by R3.
    #[test]
    fn the_legacy_policy_follows_the_camera() {
        let entries = vec![(
            okey("A"),
            CanvasPos {
                x: -5000 * Q16_ONE + Q16_ONE / 2,
                y: 3001 * Q16_ONE + Q16_ONE / 2,
            },
            1.0,
        )];
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::from_entries(entries).unwrap());
        c.set_placement_policy(PlacementPolicy::OriginAtCameraCentre);
        c.output_connected(&mut t, plain("A")).unwrap();
        // −4999.5 → −5000 and 3001.5 → 3002: ties to even
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5000, y: 3002 }
        );
    }

    /// A window that fits is centred: x = usable_x + (usable_w − w)/2, y likewise. KCalc's
    /// 646x513 on a 1280x800 output with the camera at the Canvas origin: the view is
    /// [−640, 640) × [−400, 400), so the anchor is (−640 + 317, −400 + 143.5) = (−323, −256.5),
    /// and R3 (ties to even) gives −256. The whole window is inside the view.
    #[test]
    fn a_window_that_fits_is_centred_in_the_view() {
        let mut c = centred((1280, 800), 120, None);
        let (_, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        assert!(a.x >= -640 && a.x + 646 <= 640 && a.y >= -400 && a.y + 513 <= 400);
        // even sizes centre exactly
        let (_, a) = placed(&mut c, 400, 300);
        assert_eq!(a, CanvasPoint { x: -200, y: -150 });
    }

    /// An oversized window is aligned to the region's origin on the axis it does not fit, and
    /// centred on the other: its top-left corner is what stays reachable.
    #[test]
    fn an_oversized_window_is_aligned_to_the_region_origin() {
        let mut c = centred((1280, 800), 120, None);
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -640, y: -150 }
        );
        assert_eq!(
            placed(&mut c, 300, 2000).1,
            CanvasPoint { x: -150, y: -400 }
        );
        assert_eq!(
            placed(&mut c, 5000, 4000).1,
            CanvasPoint { x: -640, y: -400 }
        );
        // exactly the size of the region: both rules agree
        assert_eq!(
            placed(&mut c, 1280, 800).1,
            CanvasPoint { x: -640, y: -400 }
        );
    }

    /// The usable region is the output's logical extent, so the output scale matters and the
    /// physical mode does not: 1920x1080 at 1.5 is 1280x720 logical; 3840x2160 at 2.0 is
    /// 1920x1080 logical.
    #[test]
    fn placement_uses_the_logical_extent_of_a_scaled_output() {
        let mut c = centred((1920, 1080), 180, None);
        assert_eq!(placed(&mut c, 646, 513).1, CanvasPoint { x: -323, y: -256 });
        // 800 logical px fits in 1280 but 750 does not fit in 720: aligned to the top
        assert_eq!(placed(&mut c, 800, 750).1, CanvasPoint { x: -400, y: -360 });
        let mut c = centred((3840, 2160), 240, None);
        assert_eq!(
            placed(&mut c, 1900, 1000).1,
            CanvasPoint { x: -950, y: -500 }
        );
        assert_eq!(
            placed(&mut c, 2000, 1000).1,
            CanvasPoint { x: -960, y: -500 }
        );
    }

    /// F-PLACE-1 in exact integers, independent of Core's Q16 and f64 path: per axis
    /// `c − min(s, u)/2` with `u = 120·view/scale_120`, rounded half to even.
    fn exact_axis(c: i64, view: u32, scale_120: u32, s: i32) -> i64 {
        let d = 2 * i128::from(scale_120);
        let half = (i128::from(s) * i128::from(scale_120)).min(120 * i128::from(view));
        let n = 2 * i128::from(c) * i128::from(scale_120) - half;
        let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
        let up = 2 * r > d || (2 * r == d && q % 2 != 0);
        (q + i128::from(up)) as i64
    }

    /// The five examples of the amendment draft that no other test carries (fractional usable
    /// extent, half-pixel ties of both parities), then a sweep against the exact model:
    /// 6 scales × 3 cameras × 4 modes × 40 sizes.
    #[test]
    fn placement_agrees_with_the_exact_model_on_fractional_extents_and_ties() {
        let mut c = centred((1366, 768), 150, None); // 1092.8 × 614.4 logical
        assert_eq!(placed(&mut c, 801, 601).1, CanvasPoint { x: -400, y: -300 });
        assert_eq!(
            placed(&mut c, 2000, 601).1,
            CanvasPoint { x: -546, y: -300 }
        );
        let mut c = centred((1280, 800), 120, None);
        // −322.5 → −322 and −255.5 → −256: ties go to the even pixel
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -256 });
        assert_eq!(placed(&mut c, 1, 1).1, CanvasPoint { x: 0, y: 0 });
        let mut c = centred((1280, 800), 120, Some((1, 1)));
        // −321.5 → −322 and −254.5 → −254
        assert_eq!(placed(&mut c, 645, 511).1, CanvasPoint { x: -322, y: -254 });

        let mut checked = 0;
        for scale_120 in [90u32, 120, 150, 180, 210, 240] {
            for camera in [(0i64, 0i64), (7001, 333), (-123_457, -9)] {
                for mode in [(1280u32, 800u32), (1366, 768), (1365, 767), (3840, 2160)] {
                    let mut c = centred(mode, scale_120, Some(camera));
                    for k in 0..40 {
                        let (w, h) = (1 + 97 * k, 3 + 61 * k);
                        let want = CanvasPoint {
                            x: exact_axis(camera.0, mode.0, scale_120, w),
                            y: exact_axis(camera.1, mode.1, scale_120, h),
                        };
                        assert_eq!(
                            placed(&mut c, w, h).1,
                            want,
                            "{w}×{h} on {mode:?} at {scale_120}/120, camera {camera:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 2880);
    }

    /// Placement goes through the camera: with the camera elsewhere on the Canvas, positive or
    /// negative, the window is centred in what that camera shows.
    #[test]
    fn placement_follows_the_camera_including_negative_canvas_coordinates() {
        let mut c = centred((1280, 800), 120, Some((9000, 700)));
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint {
                x: 9000 - 323,
                y: 700 - 256
            }
        );
        let mut c = centred((1280, 800), 120, Some((-5000, -3000)));
        // −3000 − 256.5 = −3256.5, ties to even: −3256
        assert_eq!(
            placed(&mut c, 646, 513).1,
            CanvasPoint { x: -5323, y: -3256 }
        );
        assert_eq!(
            placed(&mut c, 2000, 300).1,
            CanvasPoint { x: -5640, y: -3150 }
        );
    }

    /// Placement happens once. A later commit with another size, a tree change, a camera pan,
    /// an output reconfiguration, a second output, and an unmap followed by a remap all leave
    /// the anchor where it is (INV-STAGE-5; placement is not camera movement).
    #[test]
    fn a_placed_window_is_not_moved_again() {
        let entries = PersistedCameras::default();
        let (mut c, mut port, _, mut t) = Core::new(entries);
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let d = |mode, scale_120| OutputDescriptor {
            key: okey("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        let (o, _) = c.output_connected(&mut t, d((1280, 800), 120)).unwrap();
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: -323, y: -256 });
        let anchor = |c: &Core| c.stage().anchor(id).unwrap();

        c.toplevel_committed(id, sized(900, 700, true)).unwrap();
        assert_eq!(anchor(&c), a, "a resize commit");
        c.toplevel_tree_changed(id, sized(300, 200, true)).unwrap();
        assert_eq!(anchor(&c), a, "a tree change");

        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        c.physical_input_vec(&mut port, button(true)).unwrap();
        c.physical_input_vec(
            &mut port,
            PhysicalEvent::PointerMotion {
                delta: (-400.0, 250.0),
                delta_unaccel: (-400.0, 250.0),
                time_us: 0,
            },
        )
        .unwrap();
        c.physical_input_vec(&mut port, button(false)).unwrap();
        assert_eq!(anchor(&c), a, "a camera pan");

        c.output_reconfigured(&mut t, o, d((2560, 1440), 180))
            .unwrap();
        assert_eq!(anchor(&c), a, "an output reconfiguration");
        let second = OutputDescriptor {
            key: okey("B"),
            ..d((1920, 1080), 120)
        };
        c.output_connected(&mut t, second).unwrap();
        assert_eq!(anchor(&c), a, "another output");

        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // unmap
        c.toplevel_committed(id, sized(300, 200, false)).unwrap(); // initial commit of the remap
        c.toplevel_committed(id, sized(300, 200, true)).unwrap(); // remap
        assert_eq!(anchor(&c), a, "a remap keeps its anchor");

        // a new window is placed by the camera as it is now, not as it was
        let (_, b) = placed(&mut c, 400, 300);
        assert_ne!(
            b,
            CanvasPoint { x: -200, y: -150 },
            "the pan moved the view"
        );
    }

    /// Popups and subsurfaces belong to their parent's tree: they do not change where the
    /// window is placed (only the window geometry counts) and are never placed by themselves.
    #[test]
    fn popups_and_subsurfaces_do_not_take_part_in_placement() {
        let mut c = centred((1280, 800), 120, None);
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(646, 513, false)).unwrap();
        let mut tree = sized(646, 513, true);
        tree.surfaces.push(SnapshotSurface {
            key: key(2),
            role: SurfaceRole::Subsurface {
                parent: 0,
                offset: LocalPoint { x: -200, y: 900 },
            },
            size: LocalSize { w: 3000, h: 50 },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
        });
        tree.surfaces.push(SnapshotSurface {
            key: key(3),
            role: SurfaceRole::Popup {
                position: LocalPoint { x: 10, y: 30 },
                geometry_loc: LocalPoint::default(),
            },
            size: LocalSize { w: 180, h: 240 },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
        });
        c.toplevel_committed(id, tree).unwrap();
        assert_eq!(c.stage().anchor(id), Some(CanvasPoint { x: -323, y: -256 }));
    }

    /// Without an output there is no view to centre in: the anchor of the initial commit is
    /// kept, and an output that connects later does not move the window.
    #[test]
    fn without_an_output_the_initial_anchor_is_kept() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        c.set_placement_policy(PlacementPolicy::CentredInView);
        let (id, a) = placed(&mut c, 646, 513);
        assert_eq!(a, CanvasPoint { x: 0, y: 0 });
        c.output_connected(&mut t, plain("A")).unwrap();
        assert_eq!(c.stage().anchor(id), Some(a));
    }

    // ------------------------------------------------- fullscreen and the scale policy (H-9)

    /// The `(scale_120, integer)` hints the effects send to `id`.
    fn hints(fx: &[Effect], id: ElementId) -> Vec<(u32, u32)> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::PreferredScale {
                    element,
                    scale_120,
                    integer_scale,
                } if *element == id => Some((*scale_120, *integer_scale)),
                _ => None,
            })
            .collect()
    }

    /// A persisted camera for output B centred at `b_centre_x` px with `b_zoom` (the
    /// persisted-camera path, INV-CAMERA-1).
    fn camera_b(b_centre_x: i64, b_zoom: f64) -> PersistedCameras {
        PersistedCameras::from_entries(vec![(
            crate::output::tests::key("B"),
            crate::geom::CanvasPos {
                x: b_centre_x << 16,
                y: 0,
            },
            b_zoom,
        )])
        .unwrap()
    }

    /// Output A (scale 1) at the Canvas origin, then output B (scale 2) with the camera of
    /// [`camera_b`]. Windows are placed on A (the lowest-id output).
    fn two_scales(c: &mut Core, t: &mut OutputLifecycleToken) -> (OutputId, OutputId) {
        let a = out(c, t, "A", (1920, 1080), 120);
        let b = out(c, t, "B", (1920, 1080), 240);
        (a, b)
    }

    /// INV-SCALE-1 / C14.4: fullscreen enter and exit are non-client stimuli, so the hint follows
    /// the presenting output when W goes fullscreen on an output that did not observe it, and
    /// returns when the presentation ends.
    #[test]
    fn fullscreen_enter_and_exit_move_the_hint_to_the_presenting_output() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        assert_eq!(
            c.preferred_scale_of(w),
            Some((120, 1)),
            "windowed on A only"
        );
        focus(&mut c, &mut i, w);
        let (d, fx) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(d, ToplevelDecision::Granted);
        assert_eq!(c.presentation(b), Some(w));
        assert_eq!(hints(&fx, w), vec![(240, 2)], "B presents W: rho = 2");
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(c.presentation(b), None);
        assert_eq!(
            hints(&fx, w),
            vec![(120, 1)],
            "the presentation ended: rho = 1"
        );
    }

    /// INV-SCALE-1 "the protocol-fullscreen presentation contributes scale_O (zoom 1)": B observes
    /// W windowed through its camera at zoom 0.5 (rho 2 × 0.5 = 1) and presents it at zoom 1
    /// (rho 2), with no membership row added or removed by the presentation.
    #[test]
    fn a_presenting_output_contributes_its_scale_at_zoom_one() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 0.5));
        let (a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        assert!(c.membership().observers(w).contains(&a));
        assert!(
            c.membership().observers(w).contains(&b),
            "B's camera sees W"
        );
        assert_eq!(c.compute_rho(w), Some(1.0), "max(1 × 1, 2 × 0.5)");
        assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
        focus(&mut c, &mut i, w);
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.compute_rho(w), Some(2.0), "B presents W at zoom 1");
        assert_eq!(hints(&fx, w), vec![(240, 2)]);
    }

    /// A grant made before the first map (C12.F6), as C12.F12 adopts: the initial
    /// hint, still before the initial configure (M15), is predicted from the presentation the
    /// grant starts (B, scale 2, zoom 1) and the other outputs' cameras (A sees the rect: 1), not
    /// from B's camera (zoom 0.5); the map then finds it right and sends no second hint.
    #[test]
    fn a_pre_map_grant_takes_the_presenting_outputs_scale_when_it_maps() {
        let (mut c, _i, _, mut t) = Core::new(camera_b(100_000, 0.5));
        let (_a, b) = two_scales(&mut c, &mut t);
        let w = c.toplevel_created();
        let (d, _) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(d, ToplevelDecision::Recorded);
        let fx = c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        assert_eq!(
            fx[..2],
            [
                Effect::PreferredScale {
                    element: w,
                    scale_120: 240,
                    integer_scale: 2
                },
                Effect::InitialConfigure(w)
            ],
            "the initial hint is B's, before the one initial configure: {fx:?}"
        );
        assert_eq!(c.toplevel_configure_state(w).fullscreen, Some(b));
        assert_eq!(c.presentation(b), None, "not presented before the map");
        let fx = c.toplevel_committed(w, win(1, 960, 540, true)).unwrap();
        assert_eq!(c.presentation(b), Some(w));
        assert!(hints(&fx, w).is_empty(), "already right: {fx:?}");
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
    }

    /// OB-13 on the integration, with C12.F11: a grant's configure asks for the
    /// output's size, and the hint that reaches the client before it (item 3) already fits that
    /// size within D_max. W is seen by a scale-3 output P; D_max 8192; granted fullscreen on a
    /// 3840-wide scale-1 output O. Without C12.F11: hint 3, a 11,520-px first buffer.
    #[test]
    fn a_grant_never_asks_for_a_buffer_over_the_device_limit() {
        let persisted = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("O"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, mut i, _, mut t) = Core::new(persisted);
        out(&mut c, &mut t, "P", (1920, 1080), 360);
        let o = out(&mut c, &mut t, "O", (3840, 2160), 120);
        c.set_renderer_device_max(8192).unwrap();
        let w = map_win(&mut c, 1, 400, 300);
        assert_eq!(c.preferred_scale_of(w), Some((360, 3)));
        focus(&mut c, &mut i, w);
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
            .unwrap();
        let size = c
            .toplevel_configure_state(w)
            .size
            .expect("a fullscreen size");
        let (s120, b) = c.preferred_scale_of(w).unwrap();
        let side = size.w.max(size.h) as u64;
        assert!(side * u64::from(b) <= 8192, "integer: {side} x {b}");
        assert!(
            side * u64::from(s120) <= 8192 * 120,
            "fractional: {side} x {s120}/120"
        );
        let hint = fx
            .iter()
            .position(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == w))
            .expect("the hint is lowered in the grant's batch");
        let configure = fx
            .iter()
            .position(|e| matches!(e, Effect::ToplevelConfigure(e) if *e == w))
            .unwrap();
        assert!(hint < configure, "{fx:?}");
    }

    /// C12.F11: the saved size offered at fullscreen exit is a size Core asks for. X (3000 px
    /// wide, seen by A at scale 1 and B at scale 2, D_max 4096) is presented on B and redraws at
    /// B's 1920 px, so its hint rises to 2. The unset offers 3000 px again: the hint in force
    /// when that configure is sent must bound it (3000 x 2 would be 6000 px), and stays bounded
    /// until X redraws at another size.
    #[test]
    fn the_size_offered_at_fullscreen_exit_fits_the_device_limit() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        c.set_renderer_device_max(4096).unwrap();
        let x = map_win(&mut c, 9, 3000, 200);
        let fits = |c: &Core, side: u64| {
            let (s120, b) = c.preferred_scale_of(x).unwrap();
            side * u64::from(b) <= 4096 && side * u64::from(s120) <= 4096 * 120
        };
        assert!(fits(&c, 3000));
        focus(&mut c, &mut i, x);
        c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
        // a non-client stimulus (A's refresh changes) re-evaluates X at its fullscreen size
        c.output_reconfigured(
            &mut t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 75_000,
            },
        )
        .unwrap();
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "presented on B");
        focus(&mut c, &mut i, x);
        let (_, fx) = c
            .toplevel_request(x, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        let configure = fx
            .iter()
            .position(|e| {
                matches!(e, Effect::ToplevelConfigureSize { element, size }
                    if *element == x && (size.w, size.h) == (3000, 200))
            })
            .expect("the saved size is offered");
        assert!(fits(&c, 3000), "{:?}", c.preferred_scale_of(x));
        let hint = fx
            .iter()
            .position(|e| matches!(e, Effect::PreferredScale { element, .. } if *element == x))
            .expect("lowered in the unset's batch");
        assert!(hint < configure, "{fx:?}");
        // a commit at the fullscreen size (the client has not redrawn yet) keeps the bound
        c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
        assert!(fits(&c, 3000), "{:?}", c.preferred_scale_of(x));
    }

    /// C12.F11 + C12.F12 at the initial commit: a pre-map grant's initial hint is
    /// already bounded by the configured size, so the first buffer fits D_max. P (scale 3) sees the
    /// placed rect, so the prediction is 3; O, the granted output, is 3840 logical px wide:
    /// unbounded, the first buffer would be 11,520 px for D_max 8192.
    #[test]
    fn a_pre_map_grant_initial_hint_fits_the_device_limit() {
        let persisted = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("O"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, _i, _, mut t) = Core::new(persisted);
        out(&mut c, &mut t, "P", (1920, 1080), 360);
        let o = out(&mut c, &mut t, "O", (3840, 2160), 120);
        c.set_renderer_device_max(8192).unwrap();
        let w = c.toplevel_created();
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
            .unwrap();
        c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
        let size = c
            .toplevel_configure_state(w)
            .size
            .expect("a fullscreen size");
        let (s120, b) = c.preferred_scale_of(w).unwrap();
        let side = size.w.max(size.h) as u64;
        assert!(side * u64::from(b) <= 8192, "integer: {side} x {b}");
        assert!(
            side * u64::from(s120) <= 8192 * 120,
            "fractional: {side} x {s120}/120"
        );
    }

    /// A presentation hides every other element on its output (OBJ-19). X, seen by A (scale 1)
    /// and B (scale 2), loses B when W's presentation starts there, whether the grant is made to
    /// a mapped W or before W's first map. The change comes from W's client: X's hint holds (it
    /// is not lowered by another client's presentation), while W, the subject, takes B's scale.
    #[test]
    fn a_clients_presentation_holds_the_hints_of_the_elements_it_hides() {
        for premap in [false, true] {
            let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
            let (a, b) = two_scales(&mut c, &mut t);
            let x = map_win(&mut c, 9, 300, 200);
            assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "A and B see X");
            let w = c.toplevel_created();
            let fx = if premap {
                c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                    .unwrap();
                c.toplevel_committed(w, win(1, 0, 0, false)).unwrap();
                c.toplevel_committed(w, win(1, 960, 540, true)).unwrap()
            } else {
                c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
                c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
                focus(&mut c, &mut i, w);
                c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                    .unwrap()
                    .1
            };
            assert_eq!(c.presentation(b), Some(w), "premap {premap}");
            assert_eq!(c.membership().observers(x), vec![a], "premap {premap}");
            assert_eq!(c.compute_rho(x), Some(1.0));
            assert!(hints(&fx, x).is_empty(), "premap {premap}: {fx:?}");
            assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held");
            assert_eq!(c.preferred_scale_of(w), Some((240, 2)), "premap {premap}");
        }
    }

    /// A presentation the system ends (its output removed) is a non-client change for everyone
    /// (a full pass, INV-OUTPUT-1): unlike a client's, it may lower an unrelated element's hint.
    #[test]
    fn a_presentation_the_system_ends_moves_unrelated_hints() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held while hidden");
        let fx = c.output_disconnected(&mut t, b).unwrap();
        assert_eq!(c.membership().observers(x), vec![a]);
        assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
    }

    /// M9 "settlement": W's fullscreen exit during a camera drag settles W's epoch at once, so
    /// the decrease a drag suppresses happens in that batch; X, another client's window, keeps
    /// its epoch and its hint.
    #[test]
    fn a_subjects_fullscreen_exit_during_a_drag_settles_its_epoch() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        c.physical_input_vec(&mut i, button(true)).unwrap();
        c.physical_input_vec(
            &mut i,
            PhysicalEvent::PointerMotion {
                delta: (-3.0, 0.0),
                delta_unaccel: (-3.0, 0.0),
                time_us: 0,
            },
        )
        .unwrap();
        assert!(
            c.scale_state(w).unwrap().epoch.open,
            "the drag opened W's epoch"
        );
        assert!(c.scale_state(x).unwrap().epoch.open);
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(hints(&fx, w), vec![(120, 1)], "settled now: {fx:?}");
        assert!(!c.scale_state(w).unwrap().epoch.open);
        assert!(c.scale_state(x).unwrap().epoch.open, "X's epoch is not W's");
        assert!(hints(&fx, x).is_empty());
        c.physical_input_vec(&mut i, button(false)).unwrap();
    }

    /// C12.F13. Cross-client churn (independent review, 2026-10-15): a client that never holds focus
    /// cycles create, pre-map grant, map and destroy; another client focused toggles fullscreen.
    /// X, seen by A and B, never changes its hint; the presenting clients' own windows may.
    #[test]
    fn a_client_cycling_fullscreen_does_not_churn_another_clients_hint() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let held = c.preferred_scale_of(x);
        let mut changes = 0;
        for k in 0..20u64 {
            let w = c.toplevel_created();
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            c.toplevel_committed(w, win(100 + k, 0, 0, false)).unwrap();
            let fx = c
                .toplevel_committed(w, win(100 + k, 960, 540, true))
                .unwrap();
            assert_eq!(c.presentation(b), Some(w));
            changes += hints(&fx, x).len();
            changes += hints(&c.toplevel_destroyed(w), x).len();
        }
        let v = map_win(&mut c, 50, 400, 300);
        for _ in 0..20 {
            focus(&mut c, &mut i, v);
            let (_, fx) = c
                .toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            changes += hints(&fx, x).len();
            let (_, fx) = c
                .toplevel_request(v, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            changes += hints(&fx, x).len();
        }
        assert_eq!(changes, 0);
        assert_eq!(c.preferred_scale_of(x), held);
    }

    /// C12.F13, review probe A: X, seen by A (scale 1) and B (scale 2), commits
    /// between another client's fullscreen toggles on B. Hidden, X is held; its own commits do
    /// not lower it (the hold lasts until a non-client stimulus): no change at all.
    #[test]
    fn a_held_window_committing_between_toggles_keeps_its_hint() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        let mut changes = 0;
        for _ in 0..5 {
            focus(&mut c, &mut i, w);
            let (_, fx) = c
                .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            changes += hints(&fx, x).len();
            changes += hints(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap(), x).len();
            let (_, fx) = c
                .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            changes += hints(&fx, x).len();
            changes += hints(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap(), x).len();
        }
        assert_eq!(changes, 0);
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
    }

    /// C12.F13, review probe C: X never commits; W1 toggles fullscreen on B while a
    /// second client cycles create / pre-map grant on C / map / destroy. No change of X's hint.
    #[test]
    fn a_passive_window_under_two_presentations_keeps_its_hint() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let o3 = out(&mut c, &mut t, "C", (1920, 1080), 180);
        let x = map_win(&mut c, 9, 300, 200);
        let w1 = map_win(&mut c, 1, 400, 300);
        let mut changes = 0;
        for k in 0..5u64 {
            focus(&mut c, &mut i, w1);
            let (_, fx) = c
                .toplevel_request(w1, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            changes += hints(&fx, x).len();
            let v = c.toplevel_created();
            c.toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(o3) })
                .unwrap();
            c.toplevel_committed(v, win(100 + k, 0, 0, false)).unwrap();
            changes += hints(
                &c.toplevel_committed(v, win(100 + k, 960, 540, true))
                    .unwrap(),
                x,
            )
            .len();
            changes += hints(&c.toplevel_destroyed(v), x).len();
            focus(&mut c, &mut i, w1);
            let (_, fx) = c
                .toplevel_request(w1, ToplevelRequest::UnsetFullscreen)
                .unwrap();
            changes += hints(&fx, x).len();
        }
        assert_eq!(changes, 0);
    }

    /// C12.F13, review probe B: X is held at B's scale while W is presented on B; a
    /// system change (A reconfigured) is a non-client stimulus and lowers X; when W's client ends
    /// the presentation (unset, unmap, destroy), X is revealed on B and is raised again (a rise
    /// is never refused).
    #[test]
    fn a_revealed_window_is_raised_after_a_system_change_lowered_it() {
        for end in ["unset", "unmap", "destroy"] {
            let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
            let (a, b) = two_scales(&mut c, &mut t);
            let x = map_win(&mut c, 9, 300, 200);
            let w = map_win(&mut c, 1, 400, 300);
            focus(&mut c, &mut i, w);
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "{end}: held");
            c.output_reconfigured(
                &mut t,
                a,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 75_000,
                },
            )
            .unwrap();
            assert_eq!(
                c.preferred_scale_of(x),
                Some((120, 1)),
                "{end}: lowered by the system"
            );
            let fx = match end {
                "unset" => {
                    c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                        .unwrap()
                        .1
                }
                "unmap" => c.toplevel_committed(w, win(1, 400, 300, false)).unwrap(),
                _ => c.toplevel_destroyed(w),
            };
            assert_eq!(
                hints(&fx, x),
                vec![(240, 2)],
                "{end}: revealed on B: {fx:?}"
            );
        }
    }

    /// C12.F13, review probe F: a hold ends when it is no longer needed. X reaches
    /// into B (scale 2) by its own size; another client toggles fullscreen on B (set, unset);
    /// then X shrinks itself off B: its own geometry lowers its hint, as without the toggle.
    #[test]
    fn a_hold_ends_when_the_presentation_that_caused_it_ends() {
        for toggle in [false, true] {
            let (mut c, mut i, _, mut t) = Core::new(camera_b(1000, 1.0));
            let (a, b) = two_scales(&mut c, &mut t);
            let x = map_win(&mut c, 9, 1200, 200);
            assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "X reaches into B");
            if toggle {
                let w = map_win(&mut c, 1, 400, 300);
                focus(&mut c, &mut i, w);
                c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                    .unwrap();
                focus(&mut c, &mut i, w);
                c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                    .unwrap();
                assert!(!c.scale_state(x).unwrap().presentation_hold, "no hold left");
            }
            let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
            assert_eq!(c.membership().observers(x), vec![a], "toggle {toggle}");
            assert_eq!(hints(&fx, x), vec![(120, 1)], "toggle {toggle}: {fx:?}");
        }
    }

    /// Persisted cameras at the given x centres (px), zoom 1, for the outputs named.
    fn cameras_at(at: &[(&str, i64)]) -> PersistedCameras {
        PersistedCameras::from_entries(
            at.iter()
                .map(|(name, x)| {
                    (
                        crate::output::tests::key(name),
                        crate::geom::CanvasPos { x: x << 16, y: 0 },
                        1.0,
                    )
                })
                .collect(),
        )
        .unwrap()
    }

    /// C12.F13: the element's own evaluation ends a hold its requirement no longer
    /// needs. X (300 px, seen by A and B) is held while W is presented on B; X grows onto D
    /// (scale 2): its requirement is back at its hint, the hold ends; X shrinks off D: its own
    /// geometry lowers it.
    #[test]
    fn a_held_windows_own_growth_ends_its_hold() {
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 1500)]));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let d = out(&mut c, &mut t, "D", (1920, 1080), 240);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        c.toplevel_committed(x, win(9, 2400, 200, true)).unwrap();
        assert!(c.membership().observers(x).contains(&d));
        assert!(
            !c.scale_state(x).unwrap().presentation_hold,
            "needed no more"
        );
        let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
        assert_eq!(c.membership().observers(x), vec![a]);
        assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
    }

    /// C12.F13: a subject's own fullscreen ends its hold. X (seen by A, E and B) is
    /// held while W is presented on B; X goes fullscreen on A and back (a subject both times);
    /// then X shrinks off A, onto E alone (scale 0.75): its own geometry lowers it.
    #[test]
    fn a_held_windows_own_fullscreen_ends_its_hold() {
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("A", 1500), ("B", 1500)]));
        // E first: X is placed on E (the lowest-id output), origin-aligned (wider than E's view),
        // and A and B see its right part
        let e = out(&mut c, &mut t, "E", (1920, 1080), 90);
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let x = map_win(&mut c, 9, 3000, 200);
        assert_eq!(c.membership().observers(x), vec![e, a, b]);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        focus(&mut c, &mut i, x);
        c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        focus(&mut c, &mut i, x);
        c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert!(
            !c.scale_state(x).unwrap().presentation_hold,
            "its own fullscreen ended it"
        );
        let fx = c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
        assert_eq!(c.membership().observers(x), vec![e], "off A, on E alone");
        let (s120, _) = hints(&fx, x).pop().expect("lowered by its own geometry");
        assert_eq!(s120, 90, "{fx:?}");
    }

    /// C12.F13: P7 lowers a held window (the device bound is a correctness rule).
    #[test]
    fn p7_lowers_a_held_window() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        c.set_renderer_device_max(4096).unwrap();
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)), "held");
        assert!(c.scale_state(x).unwrap().presentation_hold);
        // X grows to 3000 px: 3000 x 2 > 4096
        let fx = c.toplevel_committed(x, win(9, 3000, 200, true)).unwrap();
        let (s120, _) = hints(&fx, x).pop().expect("P7 lowers it");
        assert!(u64::from(s120) * 3000 <= 4096 * 120, "{s120}");
    }

    /// C12.F13: a camera action releases the hold of the elements it touches only. The pointer
    /// is on A (scale 1, the first output); X is seen by D (scale 1) and B (scale 2), not by A
    /// (A's camera was panned away first), and is held at 2 while W is presented on B. A drag of
    /// A far from X keeps the hold through the drag and its settling release; a drag that
    /// brings X into A's view touches it: the hold ends and X takes its requirement (1).
    #[test]
    fn a_camera_drag_releases_only_the_holds_it_touches() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let d = out(&mut c, &mut t, "D", (1920, 1080), 120);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        let motion = |dx: f64| PhysicalEvent::PointerMotion {
            delta: (dx, 0.0),
            delta_unaccel: (dx, 0.0),
            time_us: 0,
        };
        let drag = |c: &mut Core, i: &mut InputPort, steps: &[f64]| {
            let mut fx = c.physical_input_vec(i, button(true)).unwrap();
            for dx in steps {
                fx.extend(c.physical_input_vec(i, motion(*dx)).unwrap());
            }
            fx.extend(c.physical_input_vec(i, button(false)).unwrap());
            fx
        };
        assert_eq!(c.pointer_output(), Some(a));
        drag(&mut c, &mut i, &[-2500.0]);
        assert_eq!(c.membership().observers(x), vec![b, d], "A panned away");
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        let fx = drag(&mut c, &mut i, &[-500.0, -500.0]);
        assert!(hints(&fx, x).is_empty(), "a drag elsewhere: {fx:?}");
        assert!(c.scale_state(x).unwrap().presentation_hold, "kept");
        assert_eq!(c.preferred_scale_of(x), Some((240, 2)));
        let fx = drag(&mut c, &mut i, &[1750.0, 1750.0]);
        assert_eq!(c.membership().observers(x), vec![a, d], "A sees X again");
        assert!(!c.scale_state(x).unwrap().presentation_hold, "released");
        assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
    }

    /// The X of [`the_size_offered_at_fullscreen_exit_fits_the_device_limit`] up to the unset:
    /// 3000 px wide, seen by A (scale 1) and B (scale 2), D_max 4096, presented on B, redrawn at
    /// B's 1920 px, then unset (3000 px offered).
    fn offered_after_unset(
        c: &mut Core,
        i: &mut InputPort,
        t: &mut OutputLifecycleToken,
    ) -> ElementId {
        let (_a, b) = two_scales(c, t);
        c.set_renderer_device_max(4096).unwrap();
        let x = map_win(c, 9, 3000, 200);
        focus(c, i, x);
        c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(x, win(9, 1920, 1080, true)).unwrap();
        focus(c, i, x);
        c.toplevel_request(x, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        x
    }

    /// C12.F11 (adversarial, 2026-10-16): the offer bounds the hint only until the client redraws
    /// at another size. X redraws at the offered 3000 px (its own tree bounds it now), then
    /// shrinks to 300 px: nothing bounds it any more, and a later non-client stimulus finds the
    /// requirement of A and B (2) — the offer did not leave a permanent cap.
    #[test]
    fn a_spent_restore_offer_no_longer_bounds_the_hint() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let x = offered_after_unset(&mut c, &mut i, &mut t);
        c.toplevel_committed(x, win(9, 3000, 200, true)).unwrap();
        assert!(
            c.restore_offers.is_empty(),
            "redrawn at another size: spent"
        );
        c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
        // A becomes scale 2 (a non-client stimulus): X's requirement is 2, and 300 x 2 fits
        // 4096; a stale 3000 px offer would cap it at 1
        let fx = rescale_a(&mut c, &mut t, 240);
        assert_eq!(hints(&fx, x), vec![(240, 2)], "{fx:?}");
    }

    /// Output A reconfigured to `scale_120` with the same logical size (1920 x 1080), a
    /// non-client stimulus for everything.
    fn rescale_a(c: &mut Core, t: &mut OutputLifecycleToken, scale_120: u32) -> Vec<Effect> {
        let a = c
            .outputs()
            .iter()
            .find(|o| o.key == crate::output::tests::key("A"))
            .unwrap()
            .id;
        c.output_reconfigured(
            t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920 * scale_120 / 120, 1080 * scale_120 / 120),
                transform: Transform::Normal,
                scale_120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap()
    }

    /// C12.F11 (adversarial): a re-grant before the client redrew saves the size still on offer
    /// (its last windowed size), not the fullscreen geometry it still has; the second unset offers
    /// 3000 px again and the hint still fits it.
    #[test]
    fn a_re_grant_before_the_redraw_offers_the_windowed_size_again() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let x = offered_after_unset(&mut c, &mut i, &mut t);
        let b = c.outputs().iter().find(|o| o.scale == 2.0).unwrap().id;
        focus(&mut c, &mut i, x);
        c.toplevel_request(x, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        focus(&mut c, &mut i, x);
        let (_, fx) = c
            .toplevel_request(x, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::ToplevelConfigureSize { element, size }
                if *element == x && (size.w, size.h) == (3000, 200))
            ),
            "{fx:?}"
        );
        let (s120, n) = c.preferred_scale_of(x).unwrap();
        assert!(3000 * u64::from(n) <= 4096 && 3000 * u64::from(s120) <= 4096 * 120);
    }

    /// C12.F11 + C12.F10 (adversarial): an unmap spends the offer with the rest of the
    /// fullscreen state; the re-shown window (300 px) is bounded by its own tree only.
    #[test]
    fn an_unmap_spends_the_restore_offer() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let x = offered_after_unset(&mut c, &mut i, &mut t);
        c.toplevel_committed(x, win(9, 0, 0, false)).unwrap();
        c.toplevel_committed(x, win(9, 300, 200, false)).unwrap();
        c.toplevel_committed(x, win(9, 300, 200, true)).unwrap();
        assert!(c.restore_offers.is_empty());
        let fx = rescale_a(&mut c, &mut t, 240);
        assert_eq!(
            hints(&fx, x),
            vec![(240, 2)],
            "bounded by its 300 px only: {fx:?}"
        );
    }

    /// C12.F13 (adversarial): an output connected is a system change and touches every element:
    /// a hold ends there even though the new output never observes the held element.
    #[test]
    fn an_output_connected_releases_every_hold() {
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("F", 100_000)]));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        let (_, fx) = c
            .output_connected(
                &mut t,
                OutputDescriptor {
                    key: crate::output::tests::key("F"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        assert_eq!(c.membership().observers(x), vec![a], "F looks elsewhere");
        assert!(!c.scale_state(x).unwrap().presentation_hold, "released");
        assert_eq!(hints(&fx, x), vec![(120, 1)], "{fx:?}");
    }

    /// C12.F13: a reported device limit applies P7 at once and touches no hold; nor does it leave
    /// the next, unrelated stimulus releasing every hold. X is held at B's scale while W is
    /// presented on B; a limit is reported; then a camera drag of A far from X (touching nothing
    /// of X): the hold stays. (The sweep's "re-evaluate everything" flag of the report used to be
    /// read as "touched everything" by the drag's evaluation.)
    #[test]
    fn a_device_limit_report_releases_no_hold_now_or_later() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
        let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let _d = out(&mut c, &mut t, "D", (1920, 1080), 120);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        let motion = |dx: f64| PhysicalEvent::PointerMotion {
            delta: (dx, 0.0),
            delta_unaccel: (dx, 0.0),
            time_us: 0,
        };
        let drag = |c: &mut Core, i: &mut InputPort, dx: f64| {
            c.physical_input_vec(i, button(true)).unwrap();
            c.physical_input_vec(i, motion(dx)).unwrap();
            c.physical_input_vec(i, button(false)).unwrap();
        };
        drag(&mut c, &mut i, -2500.0);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        let held = c.preferred_scale_of(x);
        let fx = c.set_renderer_device_max(16384).unwrap();
        assert!(hints(&fx, x).is_empty(), "{fx:?}");
        assert!(
            c.scale_state(x).unwrap().presentation_hold,
            "not released by the report"
        );
        drag(&mut c, &mut i, -500.0);
        assert!(
            c.scale_state(x).unwrap().presentation_hold,
            "a drag elsewhere after the report does not release it"
        );
        assert_eq!(c.preferred_scale_of(x), held);
    }

    /// C12.F13 as adopted (P-3): the re-evaluation a scale-configuration change causes releases
    /// no hold, now or later. As `a_device_limit_report_releases_no_hold_now_or_later`: X is held
    /// at B's scale (2) while W is presented on B; a configuration change that does not lower it
    /// (a higher `H_cfg`) emits nothing for X, and neither it nor the full pass it schedules (run
    /// by a later drag elsewhere) releases the hold. When P7 does lower a held element, it lowers
    /// it to the target of its requirement, after which the hold ends by the rule "requirement
    /// back at or above its hint" (`a_configuration_change_applies_p7_at_once_and_releases_no_hold`).
    #[test]
    fn a_configuration_change_releases_no_hold_later() {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(cameras_at(&[("B", 0), ("D", 0)]));
        let _a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let _d = out(&mut c, &mut t, "D", (1920, 1080), 120);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: Modifiers {
                logo: true,
                ..Modifiers::default()
            },
            time_us: 0,
        };
        let motion = |dx: f64| PhysicalEvent::PointerMotion {
            delta: (dx, 0.0),
            delta_unaccel: (dx, 0.0),
            time_us: 0,
        };
        let drag = |c: &mut Core, i: &mut InputPort, dx: f64| {
            c.physical_input_vec(i, button(true)).unwrap();
            c.physical_input_vec(i, motion(dx)).unwrap();
            c.physical_input_vec(i, button(false)).unwrap();
        };
        drag(&mut c, &mut i, -2500.0);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        let config = crate::scale::ScalePolicyConfig {
            h_cfg: c.scale_config().h_cfg * 2.0,
            ..*c.scale_config()
        };
        let fx = c.set_scale_config(config).unwrap();
        assert!(hints(&fx, x).is_empty(), "{fx:?}");
        assert!(
            c.scale_state(x).unwrap().presentation_hold,
            "not released by the change"
        );
        let held = c.preferred_scale_of(x);
        drag(&mut c, &mut i, -500.0);
        assert!(
            c.scale_state(x).unwrap().presentation_hold,
            "a drag elsewhere after the change does not release it"
        );
        assert_eq!(c.preferred_scale_of(x), held);
    }

    /// C12.F13 as adopted (package rev. 2, P-3): a change of the scale configuration applies P7
    /// at once and releases no hold. X (300 px, A and B: hint 2) is held while W is presented on
    /// B; H_cfg is lowered to 1.5: X is lowered in that call, to a hint the new ceiling allows,
    /// and stays held.
    #[test]
    fn a_configuration_change_applies_p7_at_once_and_releases_no_hold() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold, "held");
        let config = crate::scale::ScalePolicyConfig {
            h_cfg: 1.5,
            ..*c.scale_config()
        };
        let fx = c.set_scale_config(config).unwrap();
        let (s120, _) = hints(&fx, x).pop().expect("P7 at once");
        assert!(s120 <= 180, "{s120}");
        assert!(c.scale_state(x).unwrap().presentation_hold, "still held");
    }

    /// C12.F13 (adversarial): a hold survives no change of identity. X is held while W is
    /// presented on B; X is destroyed; a new window Y mapped at the same place is not held and
    /// takes its requirement (A and B's, 2, once W's presentation ends).
    #[test]
    fn a_hold_dies_with_its_element() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert!(c.scale_state(x).unwrap().presentation_hold);
        c.toplevel_destroyed(x);
        assert!(c.scale_state(x).is_none());
        let y = map_win(&mut c, 10, 300, 200);
        assert_eq!(c.membership().observers(y), vec![a], "hidden on B");
        assert!(!c.scale_state(y).unwrap().presentation_hold);
        assert_eq!(c.preferred_scale_of(y), Some((120, 1)));
    }

    /// The end of a presentation, however its client ends it (unset, unmap, destroy), reveals
    /// the elements it hid. A revealed element whose requirement rises still gets the higher
    /// hint in that batch (a hold never leaves it under-resolved): X mapped while W was presented
    /// on B, so its hint is A's; revealed, it takes B's.
    #[test]
    fn the_end_of_a_presentation_raises_the_hint_of_a_revealed_element() {
        for end in ["unset", "unmap", "destroy"] {
            let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
            let (a, b) = two_scales(&mut c, &mut t);
            let w = map_win(&mut c, 1, 400, 300);
            focus(&mut c, &mut i, w);
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
            let x = map_win(&mut c, 9, 300, 200);
            assert_eq!(c.membership().observers(x), vec![a], "{end}: hidden on B");
            assert_eq!(c.preferred_scale_of(x), Some((120, 1)), "{end}");
            let fx = match end {
                "unset" => {
                    c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                        .unwrap()
                        .1
                }
                "unmap" => c.toplevel_committed(w, win(1, 400, 300, false)).unwrap(),
                _ => c.toplevel_destroyed(w),
            };
            assert_eq!(c.presentation(b), None, "{end}");
            assert_eq!(c.membership().observers(x), vec![a, b], "{end}");
            assert_eq!(hints(&fx, x), vec![(240, 2)], "{end}: {fx:?}");
        }
    }

    /// The subject's own fullscreen enter and exit are a non-client stimulus (C14.4): W's last
    /// stimulus lowered it (A's scale fell to 1), and a client response could not raise it
    /// (SC-E); its own grant on B raises it, and its own unset lowers it again.
    #[test]
    fn a_subjects_own_fullscreen_is_a_non_client_stimulus() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let w = map_win(&mut c, 1, 400, 300);
        c.output_reconfigured(
            &mut t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
        assert_eq!(c.scale_state(w).unwrap().last_stimulus_direction, -1);
        focus(&mut c, &mut i, w);
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(hints(&fx, w), vec![(240, 2)], "{fx:?}");
        let (_, fx) = c
            .toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
    }

    /// Fullscreen entry and exit put every scale hint of the batch before its first configure
    /// (as M15 orders the initial hint before the initial configure): a grant, an unset (its
    /// configure offers the saved size), and the first map of a pre-map grant that replaces the
    /// older presentation on the output (the replaced element's configure).
    #[test]
    fn fullscreen_batches_put_hints_before_configures() {
        let order = |fx: &[Effect]| {
            let hint = fx
                .iter()
                .rposition(|e| matches!(e, Effect::PreferredScale { .. }))
                .expect("a hint");
            let configure = fx
                .iter()
                .position(|e| {
                    matches!(
                        e,
                        Effect::ToplevelConfigure(_) | Effect::ToplevelConfigureSize { .. }
                    )
                })
                .expect("a configure");
            assert!(hint < configure, "{fx:?}");
        };
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        order(
            &c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap()
                .1,
        );
        order(
            &c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap()
                .1,
        );
        // W presented on B; V, granted B before its first map, replaces it when it maps
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        let v = c.toplevel_created();
        c.toplevel_request(v, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        c.toplevel_committed(v, win(2, 0, 0, false)).unwrap();
        let fx = c.toplevel_committed(v, win(2, 960, 540, true)).unwrap();
        assert_eq!(c.presentation(b), Some(v));
        assert!(!hints(&fx, w).is_empty(), "W left B: {fx:?}");
        order(&fx);
    }

    /// The scale hints of a run in which a presentation starts or ends during a camera drag
    /// (the evaluation of a drag's batches is scoped, `Sweep`), with the scoped or the full
    /// reference evaluation.
    fn presentation_under_a_drag(reference: bool, start: bool) -> Vec<(u32, u32)> {
        use crate::PhysicalEvent;
        use crate::input::bindings::Modifiers;
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        c.scale_reference = reference;
        let (_a, b) = two_scales(&mut c, &mut t);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        if !start {
            c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
                .unwrap();
        }
        let logo = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        let button = |pressed| PhysicalEvent::PointerButton {
            button: 0x112,
            pressed,
            modifiers: logo,
            time_us: 0,
        };
        let mut seen = Vec::new();
        let mut log = |fx: &[Effect]| {
            seen.extend(hints(fx, x));
            seen.push((0, 0));
            seen.extend(hints(fx, w));
            seen.push((1, 1));
        };
        log(&c.physical_input_vec(&mut i, button(true)).unwrap());
        log(&c
            .physical_input_vec(
                &mut i,
                PhysicalEvent::PointerMotion {
                    delta: (-3.0, 0.0),
                    delta_unaccel: (-3.0, 0.0),
                    time_us: 0,
                },
            )
            .unwrap());
        let request = if start {
            ToplevelRequest::SetFullscreen { output: Some(b) }
        } else {
            ToplevelRequest::UnsetFullscreen
        };
        log(&c.toplevel_request(w, request).unwrap().1);
        log(&c.physical_input_vec(&mut i, button(false)).unwrap());
        log(&c.toplevel_committed(x, win(9, 300, 200, true)).unwrap());
        seen.extend(c.preferred_scale_of(x));
        seen.extend(c.preferred_scale_of(w));
        seen
    }

    /// A presentation that starts or ends during a camera drag: the drag's scoped evaluation
    /// visits the elements it hides or reveals, so the hints equal the full reference.
    #[test]
    fn a_presentation_change_during_a_camera_drag_matches_the_full_reference() {
        for start in [true, false] {
            assert_eq!(
                presentation_under_a_drag(false, start),
                presentation_under_a_drag(true, start),
                "start {start}"
            );
        }
    }

    /// INV-SCALE-5 "evaluation once per core batch after all its updates": every batch that
    /// starts, ends or re-derives a presentation evaluates the policy exactly once.
    #[test]
    fn each_presentation_batch_evaluates_the_policy_once() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(0, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        let passes = |c: &Core| c.scale_passes.0 + c.scale_passes.1;
        let once = |c: &mut Core, what: &str, f: &mut dyn FnMut(&mut Core)| {
            let before = passes(c);
            f(c);
            assert_eq!(passes(c), before + 1, "{what}");
        };
        focus(&mut c, &mut i, w);
        let fs = ToplevelRequest::SetFullscreen { output: Some(b) };
        once(&mut c, "grant", &mut |c| {
            c.toplevel_request(w, fs).unwrap();
        });
        once(&mut c, "commit of the presented element", &mut |c| {
            c.toplevel_committed(w, win(1, 960, 540, true)).unwrap();
        });
        once(&mut c, "tree change of the presented element", &mut |c| {
            c.toplevel_tree_changed(w, win(1, 960, 540, true)).unwrap();
        });
        once(&mut c, "unset", &mut |c| {
            c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
                .unwrap();
        });
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, fs).unwrap();
        once(&mut c, "unmap of the presented element", &mut |c| {
            c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        });
        let v = c.toplevel_created();
        c.toplevel_request(v, fs).unwrap();
        c.toplevel_committed(v, win(2, 0, 0, false)).unwrap();
        once(&mut c, "first map of a pre-map grant", &mut |c| {
            c.toplevel_committed(v, win(2, 960, 540, true)).unwrap();
        });
        once(&mut c, "destruction of the presented element", &mut |c| {
            c.toplevel_destroyed(v);
        });
    }

    /// A child surface's commit (`toplevel_tree_changed`) is a client response (SC-E): after a
    /// non-client stimulus lowered W's hint, a subsurface that reaches B (scale 2) does not raise
    /// it; a root commit that grows onto B does not either.
    #[test]
    fn client_responses_do_not_reverse_a_lowered_hint() {
        for by_child in [true, false] {
            let (mut c, _i, _, mut t) = Core::new(camera_b(1500, 1.0));
            let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
            let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
            let w = map_win(&mut c, 1, 400, 300);
            assert_eq!(c.preferred_scale_of(w), Some((132, 2)));
            // the non-client stimulus: A's scale falls to 1
            c.output_reconfigured(
                &mut t,
                a,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
            assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
            let fx = if by_child {
                let mut tree = win(1, 400, 300, true);
                tree.surfaces.insert(
                    0,
                    SnapshotSurface {
                        key: key(2),
                        role: SurfaceRole::Subsurface {
                            parent: 1,
                            offset: LocalPoint { x: 1500, y: 0 },
                        },
                        size: LocalSize { w: 400, h: 300 },
                        mapped: true,
                        input: InputRegion::Infinite,
                        frame_requested: false,
                    },
                );
                c.toplevel_tree_changed(w, tree).unwrap()
            } else {
                c.toplevel_committed(w, win(1, 2000, 300, true)).unwrap()
            };
            assert!(
                c.membership().observers(w).contains(&b),
                "by child {by_child}"
            );
            assert_eq!(c.compute_rho(w), Some(2.0));
            assert!(hints(&fx, w).is_empty(), "by child {by_child}: {fx:?}");
            assert_eq!(c.preferred_scale_of(w), Some((120, 1)));
        }
    }

    /// A commit of a presented element that starts or ends no presentation is a client response
    /// (SC-E): it must not release the hint SC-E holds for another element. X's hint was lowered
    /// by a non-client stimulus (A's scale falls to 1) and then held at 1 although X grew onto B
    /// (rho 2); W, presented on C, commits. Found by the independent review: evaluated as a
    /// non-client stimulus, every frame of a fullscreen client would release every such hold.
    #[test]
    fn a_commit_of_the_presented_element_releases_no_held_hint() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(1500, 1.0));
        let a = out(&mut c, &mut t, "A", (1920, 1080), 132);
        let b = out(&mut c, &mut t, "B", (1920, 1080), 240);
        let o = out(&mut c, &mut t, "C", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
            .unwrap();
        assert_eq!(c.presentation(o), Some(w));
        let x = map_win(&mut c, 9, 400, 300);
        c.output_reconfigured(
            &mut t,
            a,
            OutputDescriptor {
                key: crate::output::tests::key("A"),
                mode: (1920, 1080),
                transform: Transform::Normal,
                scale_120: 120,
                refresh_mhz: 60_000,
            },
        )
        .unwrap();
        c.toplevel_committed(x, win(9, 2000, 300, true)).unwrap();
        assert!(c.membership().observers(x).contains(&b));
        assert_eq!(c.compute_rho(x), Some(2.0));
        assert_eq!(c.preferred_scale_of(x), Some((120, 1)), "held by SC-E");
        for _ in 0..2 {
            let fx = c.toplevel_committed(w, win(1, 1920, 1080, true)).unwrap();
            assert!(hints(&fx, x).is_empty(), "{fx:?}");
        }
        assert_eq!(c.preferred_scale_of(x), Some((120, 1)));
    }

    /// F-SCALE-3: the initial prediction compares the placed rect with each view in one unit
    /// (Q16). A's camera is restored 100,000 px away; B (scale 2) looks at the origin; W is placed
    /// on A: its initial hint is A's, and the map does not renegotiate it.
    #[test]
    fn the_initial_hint_of_a_window_far_from_the_origin_is_its_outputs() {
        let persisted = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("A"),
            crate::geom::CanvasPos {
                x: 100_000 << 16,
                y: 0,
            },
            1.0,
        )])
        .unwrap();
        let (mut c, _i, _, mut t) = Core::new(persisted);
        let (a, b) = two_scales(&mut c, &mut t);
        let w = c.toplevel_created();
        let fx = c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
        let fx = c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(
            c.membership().observers(w),
            vec![a],
            "B does not see W ({b:?})"
        );
        assert!(
            hints(&fx, w).is_empty(),
            "no renegotiation at the map: {fx:?}"
        );
    }

    /// Output changes: B's scale changes while it presents W (the hint follows, at zoom 1), and
    /// B's removal ends the presentation (the hint returns to A's scale).
    #[test]
    fn output_changes_move_the_hint_of_a_presented_element() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let (_a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
        let fx = c
            .output_reconfigured(
                &mut t,
                b,
                OutputDescriptor {
                    key: crate::output::tests::key("B"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 180,
                    refresh_mhz: 60_000,
                },
            )
            .unwrap();
        assert_eq!(c.presentation(b), Some(w));
        assert_eq!(hints(&fx, w), vec![(180, 2)], "{fx:?}");
        let hint = fx
            .iter()
            .position(|e| matches!(e, Effect::PreferredScale { .. }))
            .unwrap();
        let configure = fx
            .iter()
            .position(|e| matches!(e, Effect::ToplevelConfigure(e) if *e == w))
            .expect("the presented element is told its new size");
        assert!(hint < configure, "the hint before the configure: {fx:?}");
        let fx = c.output_disconnected(&mut t, b).unwrap();
        assert!(c.fullscreen().is_empty());
        assert_eq!(hints(&fx, w), vec![(120, 1)], "{fx:?}");
    }

    /// Unmap drops the presentation (C12.F10) and sends the unmapped element no hint (an
    /// unobserved element holds h, INV-SCALE-5); after a windowed remap on A the hint is never
    /// below A's requirement, and the next fullscreen on B raises it again.
    #[test]
    fn unmap_and_remap_of_a_presented_element() {
        let (mut c, mut i, _, mut t) = Core::new(camera_b(100_000, 1.0));
        let (a, b) = two_scales(&mut c, &mut t);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
        let fx = c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        assert_eq!(c.presentation(b), None);
        assert!(hints(&fx, w).is_empty(), "{fx:?}");
        c.toplevel_committed(w, win(1, 400, 300, false)).unwrap();
        c.toplevel_committed(w, win(1, 400, 300, true)).unwrap();
        assert_eq!(c.membership().observers(w), vec![a]);
        assert!(c.preferred_scale_of(w).unwrap().0 >= 120);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(b) })
            .unwrap();
        assert_eq!(c.preferred_scale_of(w), Some((240, 2)));
    }
    /// F-VIEW-1's representable domain (C12.F14) at Core's entry points: a persisted zoom and an output's mode
    /// or scale outside the representable domain are refused where they enter (the persisted
    /// table is rejected whole, as for any invalid entry; the output is not connected or not
    /// reconfigured), never kept and saturated later.
    #[test]
    fn the_domain_is_enforced_where_zooms_and_outputs_enter_core() {
        use crate::EntryError;
        use crate::geom::{MODE_MAX, SCALE_120_MAX, SCALE_120_MIN, ZOOM_MAX, ZOOM_MIN};
        use crate::navigation::PersistedError;
        let table = |z: f64| {
            PersistedCameras::from_entries(vec![(
                crate::output::tests::key("A"),
                crate::geom::CanvasPos { x: 0, y: 0 },
                z,
            )])
        };
        for z in [
            ZOOM_MIN / 2.0,
            ZOOM_MAX * 2.0,
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            1e-15,
            1e15,
        ] {
            assert_eq!(
                table(z).unwrap_err(),
                PersistedError::Zoom { entry: 0 },
                "{z}"
            );
        }
        assert!(table(ZOOM_MIN).is_ok() && table(ZOOM_MAX).is_ok());
        let desc = |mode, scale_120| OutputDescriptor {
            key: crate::output::tests::key("A"),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        };
        let (mut c, _i, _, mut t) = Core::new(PersistedCameras::default());
        for (mode, scale_120) in [
            ((MODE_MAX + 1, 1080), 120),
            ((1920, 0), 120),
            ((1920, 1080), SCALE_120_MIN - 1),
            ((1920, 1080), SCALE_120_MAX + 1),
        ] {
            assert_eq!(
                c.output_connected(&mut t, desc(mode, scale_120))
                    .map(|_| ()),
                Err(EntryError::InvalidDescriptor),
                "{mode:?} {scale_120}"
            );
        }
        let (a, _) = c.output_connected(&mut t, desc((1920, 1080), 120)).unwrap();
        assert_eq!(
            c.output_reconfigured(&mut t, a, desc((1920, 1080), SCALE_120_MAX + 1))
                .map(|_| ()),
            Err(EntryError::InvalidDescriptor)
        );
    }

    /// F-VIEW-1's representable domain (C12.F14): Core runs at the extremes of the domain — a persisted zoom of
    /// ZOOM_MIN on a MODE_MAX × MODE_MAX output at scale 0.25, and ZOOM_MAX on a 1 × 1 output at
    /// scale 8 — through map, render list, hit test and pointer routing, without overflow.
    #[test]
    fn core_runs_at_the_extremes_of_the_domain() {
        use crate::PhysicalEvent;
        use crate::geom::{MODE_MAX, SCALE_120_MAX, SCALE_120_MIN, ZOOM_MAX, ZOOM_MIN};
        for (zoom, mode, scale_120) in [
            (ZOOM_MIN, (MODE_MAX, MODE_MAX), SCALE_120_MIN),
            (ZOOM_MAX, (1, 1), SCALE_120_MAX),
            (ZOOM_MAX, (MODE_MAX, MODE_MAX), SCALE_120_MIN),
            (ZOOM_MIN, (1, 1), SCALE_120_MAX),
        ] {
            let persisted = PersistedCameras::from_entries(vec![(
                crate::output::tests::key("A"),
                crate::geom::CanvasPos {
                    x: crate::geom::CANVAS_LIMIT_PX << 16,
                    y: -(crate::geom::CANVAS_LIMIT_PX << 16),
                },
                zoom,
            )])
            .unwrap();
            let (mut c, mut i, _, mut t) = Core::new(persisted);
            let a = out(&mut c, &mut t, "A", mode, scale_120);
            let w = map_win(&mut c, 1, 400, 300);
            assert!(c.render_list(a).is_ok(), "{zoom} {mode:?} {scale_120}");
            let _ = c.hit_test(a, crate::geom::OutputLogicalPoint { x: 0.5, y: 0.5 });
            for d in [(1.0, 1.0), (-1e6, 1e6)] {
                c.physical_input_vec(
                    &mut i,
                    PhysicalEvent::PointerMotion {
                        delta: d,
                        delta_unaccel: d,
                        time_us: 0,
                    },
                )
                .unwrap();
            }
            assert_eq!(c.cameras().zoom(0), Some(zoom));
            let _ = w;
        }
    }

    /// The cameras and anchors: what an activation must never change (INV-INPUT-9, B10.7).
    fn spatial_state(c: &Core) -> String {
        let cams: Vec<_> = (0..c.cameras().len())
            .map(|i| (c.cameras().center(i), c.cameras().zoom(i)))
            .collect();
        let anchors: Vec<_> = c
            .stage()
            .z_order()
            .iter()
            .map(|id| c.stage().anchor(*id))
            .collect();
        format!("{cams:?} {anchors:?}")
    }

    /// INC-12 / INV-INPUT-9 (PROV-10, provisional): a validated activation of a window that may
    /// take focus is a focus grant: the focus moves, with its configures, and no camera or
    /// anchor changes.
    #[test]
    fn an_activation_grants_focus_and_moves_nothing_spatial() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let a = map_win(&mut c, 1, 400, 300);
        let w = map_win(&mut c, 2, 300, 200);
        focus(&mut c, &mut i, a);
        let before = spatial_state(&c);
        let fx = c.activation_request(w, true).unwrap();
        assert_eq!(c.focus().target(), Some(w));
        assert!(fx.contains(&Effect::KeyboardFocus(Some(w))), "{fx:?}");
        assert!(
            fx.contains(&Effect::ToplevelConfigure(a)),
            "a loses `activated`"
        );
        assert_eq!(spatial_state(&c), before);
        assert!(c.focus().attention().is_empty());
        assert!(
            c.activation_request(w, true).unwrap().is_empty(),
            "already focused: nothing"
        );
    }

    /// INC-12 / INV-INPUT-8 + INV-INPUT-9: a window hidden by a presentation may not take focus
    /// (A-8), so its activation asks for attention instead; focus stays; the attention ends when
    /// the window gets focus by an explicit action.
    #[test]
    fn an_activation_of_a_hidden_window_asks_for_attention() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let x = map_win(&mut c, 9, 300, 200);
        let w = map_win(&mut c, 1, 400, 300);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        assert!(c.hidden_by_presentation(x));
        let before = spatial_state(&c);
        let fx = c.activation_request(x, true).unwrap();
        assert!(fx.is_empty(), "{fx:?}");
        assert_eq!(c.focus().target(), Some(w), "focus stays");
        assert_eq!(c.focus().attention(), [x]);
        assert!(c.activation_request(x, true).unwrap().is_empty());
        assert_eq!(c.focus().attention(), [x], "marked once");
        assert_eq!(spatial_state(&c), before);
        c.toplevel_request(w, ToplevelRequest::UnsetFullscreen)
            .unwrap();
        focus(&mut c, &mut i, x);
        assert!(c.focus().attention().is_empty(), "focused: no attention");
    }

    /// INC-12: an activation of an element that is not mapped is ignored; of a destroyed one,
    /// refused as stale; an element that asked for attention and is destroyed leaves no trace.
    #[test]
    fn an_activation_of_an_unmapped_or_destroyed_window_changes_nothing() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = out(&mut c, &mut t, "A", (1920, 1080), 120);
        let w = map_win(&mut c, 1, 400, 300);
        let u = c.toplevel_created();
        assert!(
            c.activation_request(u, true).unwrap().is_empty(),
            "not mapped"
        );
        assert_eq!(c.focus().target(), None);
        let x = map_win(&mut c, 9, 300, 200);
        focus(&mut c, &mut i, w);
        c.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(a) })
            .unwrap();
        c.activation_request(x, true).unwrap();
        assert_eq!(c.focus().attention(), [x]);
        c.toplevel_destroyed(x);
        assert!(
            c.focus().attention().is_empty(),
            "no attention for the dead"
        );
        assert_eq!(
            c.activation_request(x, true),
            Err(ProtocolError::StaleElement)
        );
    }

    /// INC-12 review (activation policy): under the default `Strict` policy only a validated
    /// request focuses; an unvalidated one (no token from the focused client) only asks for
    /// attention. `AttentionOnly` never focuses; `Permissive` focuses whatever the token. No
    /// policy moves a camera or an anchor.
    #[test]
    fn the_activation_policy_decides_who_may_take_focus() {
        use crate::input::focus::ActivationPolicy;
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        out(&mut c, &mut t, "A", (1920, 1080), 120);
        let a = map_win(&mut c, 1, 400, 300);
        let w = map_win(&mut c, 2, 300, 200);
        focus(&mut c, &mut i, a);
        let before = spatial_state(&c);
        assert_eq!(
            c.focus().activation_policy(),
            ActivationPolicy::Strict,
            "default"
        );
        assert!(c.activation_request(w, false).unwrap().is_empty());
        assert_eq!(
            (c.focus().target(), c.focus().attention()),
            (Some(a), &[w][..])
        );
        c.set_activation_policy(ActivationPolicy::AttentionOnly);
        assert!(c.activation_request(w, true).unwrap().is_empty());
        assert_eq!(c.focus().target(), Some(a), "never under AttentionOnly");
        c.set_activation_policy(ActivationPolicy::Permissive);
        c.activation_request(w, false).unwrap();
        assert_eq!(c.focus().target(), Some(w), "any request under Permissive");
        assert!(c.focus().attention().is_empty());
        assert_eq!(spatial_state(&c), before);
    }
}
