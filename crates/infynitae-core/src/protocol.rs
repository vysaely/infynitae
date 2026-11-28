//! Policy for events derived from untrusted Wayland client requests (C1.5 item 4): element
//! lifecycle (M1), popup placement targets (M3), and the `Effect` vocabulary. No camera
//! vocabulary: nothing here can mint an `Authorized` proof, so nothing here can mutate a camera
//! (INV-CAMERA-1).

use crate::Core;
use crate::arrange::GAP;
use crate::geom::{self, CanvasPoint, CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE};
use crate::input::Axis;
use crate::output::{self, OutputId};
use crate::pointer;
use crate::snapshot::{SnapshotError, SurfaceKey, SurfaceTree};
use crate::stage::{ElementId, Lifecycle};

mod constraints;
mod placement;
mod presentation;
mod resize;
#[cfg(test)]
mod scale_hints;
mod toplevels;

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
    /// Client-routed touchpad swipe gesture begin (`zwp_pointer_gesture_swipe_v1`).
    GestureSwipeBegin { fingers: u32, time_ms: u32 },
    /// Client-routed touchpad swipe gesture update (`zwp_pointer_gesture_swipe_v1`).
    GestureSwipeUpdate { delta: (f64, f64), time_ms: u32 },
    /// Client-routed touchpad swipe gesture end (`zwp_pointer_gesture_swipe_v1`).
    GestureSwipeEnd { cancelled: bool, time_ms: u32 },
    /// Client-routed touchpad pinch gesture begin (`zwp_pointer_gesture_pinch_v1`).
    GesturePinchBegin { fingers: u32, time_ms: u32 },
    /// Client-routed touchpad pinch gesture update (`zwp_pointer_gesture_pinch_v1`).
    GesturePinchUpdate {
        delta: (f64, f64),
        scale: f64,
        rotation: f64,
        time_ms: u32,
    },
    /// Client-routed touchpad pinch gesture end (`zwp_pointer_gesture_pinch_v1`).
    GesturePinchEnd { cancelled: bool, time_ms: u32 },
    /// Client-routed touchpad hold gesture begin (`zwp_pointer_gesture_hold_v1`).
    GestureHoldBegin { fingers: u32, time_ms: u32 },
    /// Client-routed touchpad hold gesture end (`zwp_pointer_gesture_hold_v1`).
    GestureHoldEnd { cancelled: bool, time_ms: u32 },
    /// The last output is gone (M10 → NoOutput, EE-3): install the zero-output grab with
    /// `Focus::Clear` (one `wl_pointer.leave`, nothing delivered while installed).
    PointerZeroOutputBegin,
    /// The first output after NoOutput: remove the zero-output grab before the startup placement.
    PointerZeroOutputEnd,
    /// A finger's first contact (`wl_touch.down`) on `target` at `location` (Canvas, as for
    /// `PointerMotion`: the client gets `location - origin`).
    TouchDown {
        slot: u32,
        target: PointerTarget,
        location: CanvasPos,
        time_ms: u32,
    },
    /// The finger in `slot` moved (`wl_touch.motion`), delivered to its first contact's surface.
    TouchMotion {
        slot: u32,
        location: CanvasPos,
        time_ms: u32,
    },
    /// The finger in `slot` lifted (`wl_touch.up`).
    TouchUp { slot: u32, time_ms: u32 },
    /// `wl_touch.frame`.
    TouchFrame,
    /// `wl_touch.cancel`: every finger is given up.
    TouchCancel,
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
    /// Ask the client to close (by default Super+Shift+C, xdg_toplevel.close).
    CloseRequest(ElementId),
    /// Terminate the element's client (by default Super+Shift+Delete): cuts its connection and ends its process;
    /// the adapter decides how for a client it cannot end alone (X11).
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
    /// Toggle the list of open windows (by default Super+L).
    ToggleWindowList,
    /// Toggle the app launcher (by default Super+W).
    ToggleLauncher,
    /// Launch the terminal (by default Super+Q).
    SpawnTerminal,
    /// Launch the file manager (by default Super+E).
    SpawnFileManager,
    /// Launch the system monitor (by default Super+Shift+Escape).
    SpawnSystemMonitor,
    /// Capture a fullscreen screenshot (by default Super+Shift+S or PrintScreen).
    SpawnScreenshot,
    /// Capture a region screenshot (by default Super+S, or Super+Shift+R).
    SpawnRegionScreenshot,
    /// Toggle the canvas overview radar (by default Super+Shift+Tab).
    ToggleOverview,
    /// Reload shell UI and scripts (by default Super+Shift+Backspace).
    ReloadShell,
    /// Lock the screen (by default Super+Shift+L).
    SpawnLockScreen,
    /// Open clipboard history (by default Super+V).
    SpawnClipboardHistory,
    /// Open the control menu (by default Super+A).
    SpawnControlMenu,
    /// Toggle privacy mode on the focused window (by default Super+Shift+P).
    TogglePrivacy,
    /// Toggle idle sleep inhibition (stay-awake mode, by default Super+Shift+I).
    ToggleIdleInhibit,
    /// The user turned keep-on-top on or off for a window (Super+Shift+K): the adapter remembers it for the
    /// window's app (`KEEP_ON_TOP`), so the app's windows open that way after a restart.
    KeptOnTop { element: ElementId, on: bool },
    /// Toggle tearing for the focused window while it is fullscreen (by default Super+Shift+O, or
    /// Super+Shift+T).
    ToggleTearing,
    /// Save an instant replay clip (by default Super+Alt+C).
    SaveReplay,
    /// Toggle continuous screen recording (by default Super+Alt+P, or Super+Alt+R).
    ToggleRecording,
    /// Hardware volume up.
    VolumeUp,
    /// Hardware volume down.
    VolumeDown,
    /// Hardware volume mute.
    VolumeMute,
    /// Hardware brightness up.
    BrightnessUp,
    /// Hardware brightness down.
    BrightnessDown,
    /// Hardware media play/pause.
    MediaPlayPause,
    /// Hardware media next track.
    MediaNext,
    /// Hardware media previous track.
    MediaPrev,
    /// Toggle shortcut help overlay (`Super+/` or `Super+F1`).
    ToggleShortcuts,
    /// A user command from the config, by its index in the adapter's command list.
    Exec(u16),
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
/// The windows kept on top unless `KEEP_ON_TOP` says otherwise: Firefox's and Chromium's
/// picture-in-picture, by their titles.
pub(crate) const DEFAULT_KEEP_ON_TOP: &[&str] = &["picture-in-picture", "picture in picture"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlacementPolicy {
    /// The window-geometry origin at the placement output's camera centre, decided at the
    /// initial commit and never revised: the legacy policy. Kept as an explicit policy.
    OriginAtCameraCentre,
    /// F-PLACE-1: the window geometry centred in the placement output's usable
    /// region, resolved once at the first map; an oversized window aligned to the region's
    /// origin.
    #[default]
    CentredInView,
    /// Proximity cluster tiling: places the window at the nearest free spot in the cluster
    /// (closest to the viewport centre), or centres it if empty.
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
    /// `xdg_toplevel.move`: an interactive move.
    Move,
    /// `xdg_toplevel.resize`: an interactive resize.
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
    /// The drag ended; the track goes once the client answered the configures still
    /// outstanding then (`unanswered`), or sooner with a commit of the last size sent.
    pub(crate) released: bool,
    /// After the release: configures the client has not answered yet (the one awaited at the
    /// release, if any, and the last one). A client may answer one with another size.
    pub(crate) unanswered: u8,
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

/// The longest title or app_id Core keeps, in bytes.
pub const TOPLEVEL_META_MAX: usize = 4096;

/// What a configure of a toplevel carries besides `activated`, as Core holds it now. The adapter
/// copies it into every configure it sends; it keeps no fullscreen state of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToplevelConfigureState {
    /// The output the element is fullscreen on.
    pub fullscreen: Option<OutputId>,
    /// A Super+right drag is resizing it (`xdg_toplevel` state `resizing`).
    pub resizing: bool,
    /// Maximized (`xdg_toplevel` state `maximized`): an app's button then shows "restore".
    pub maximized: bool,
    /// The size asked of the client: the fullscreen output's logical size, or none (0×0: the
    /// client chooses).
    pub size: Option<LocalSize>,
    /// Not fullscreen: the size the compositor last asked of the window and still holds it to
    /// (INV-PROTO-1, amended 2026-10-31), for a configure that offers no other size.
    pub held: Option<LocalSize>,
}

impl Core {
    /// The capabilities to advertise: those the policy can grant. Fullscreen is granted to the
    /// focused element and before a first map (INV-PRES-1); maximize, minimize and the window menu
    /// are answered by `Core::client_maximize`, `Core::client_minimize` and `Core::show_window_menu`.
    pub fn toplevel_capabilities(&self) -> ToplevelCapabilities {
        ToplevelCapabilities {
            fullscreen: true,
            maximize: true,
            minimize: true,
            window_menu: true,
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

    /// Whether the window's geometry is the logical size of a screen (a game opening a window
    /// as large as the screen, the X11 way of going fullscreen): either rounding of a
    /// fractional size counts.
    pub fn fills_a_screen(&self, id: ElementId) -> bool {
        let Some(size) = self.stage.tree(id).map(|t| t.geometry.size) else {
            return false;
        };
        let (w, h) = (f64::from(size.w), f64::from(size.h));
        self.outputs.iter().any(|o| {
            let (ew, eh) = o.logical_extent();
            (ew.floor()..=ew.ceil()).contains(&w) && (eh.floor()..=eh.ceil()).contains(&h)
        })
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
                maximized: self.maximized.iter().any(|(e, _)| *e == id),
                held: self.held_size(id),
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
            maximized: false,
            size,
            held: None,
        }
    }

    /// The size the compositor holds `id` to, if any (INV-PROTO-1, amended 2026-10-31).
    pub fn held_size(&self, id: ElementId) -> Option<LocalSize> {
        self.held_sizes
            .iter()
            .find(|(e, _)| *e == id)
            .map(|(_, s)| *s)
    }

    /// The adapter sent `id` a configure asking for `size` (a maximize, its restore, a resize,
    /// the room, the size after fullscreen): every later configure carries it too. Apps told they
    /// are tiled on every side (SQUARE_CORNERS) take a configure without a size as "back to your
    /// own size" and dropped a maximized size at the next focus change (owner, 2026-10-31).
    /// Ignored while `id` is fullscreen (its configures carry the output's size) and for an
    /// element that is not mapped.
    pub fn toplevel_size_offered(&mut self, id: ElementId, size: LocalSize) {
        if self.fullscreen_of(id).is_some() || self.stage.mapped(id).is_none() {
            return;
        }
        self.hold_size(id, size);
    }

    fn hold_size(&mut self, id: ElementId, size: LocalSize) {
        match self.held_sizes.iter_mut().find(|(e, _)| *e == id) {
            Some((_, s)) => *s = size,
            None => self.held_sizes.push((id, size)),
        }
    }

    /// `id` committed a frame of `drawn` size under the configure it acknowledged last, which
    /// asked for `acked` (none: its own choice). A client that acknowledged the held size and
    /// drew another one chose its own size: it is no longer held to it.
    pub fn toplevel_size_drawn(
        &mut self,
        id: ElementId,
        acked: Option<LocalSize>,
        drawn: LocalSize,
    ) {
        if let Some(held) = self.held_size(id)
            && acked == Some(held)
            && drawn != held
        {
            self.held_sizes.retain(|(e, _)| *e != id);
        }
    }
}

/// A spot free whatever the windows: right of all of them, at `target`'s height.
/// `start` moved as little as it takes for `size` to lie within `low..high`; what does not fit
/// keeps its start at `low`, so its top left stays reachable.
fn keep_in_range(start: i64, size: i64, low: i64, high: i64) -> i64 {
    start.min(high - size).max(low)
}

pub(crate) fn beside_all(existing: &[(i64, i64, i64, i64)], target: (i64, i64)) -> (i64, i64) {
    let right = existing.iter().map(|r| r.2).max().unwrap_or(target.0);
    (right.saturating_add(12).max(target.0), target.1)
}

/// Spots for a window of `size` flush with the edges of the rects `around` (the gap kept), at
/// `target`'s lines and at the corners where `existing` rects' edges cross: the cluster search's
/// candidates, free or not.
fn spot_candidates(
    around: &[(i64, i64, i64, i64)],
    existing: &[(i64, i64, i64, i64)],
    size: (i64, i64),
    target: (i64, i64),
) -> Vec<(i64, i64)> {
    let (new_w, new_h) = size;
    let (target_gx, target_gy) = target;
    // Generate grid-aligned candidate positions flush with existing window edges
    let mut candidates = Vec::new();
    for &w1 in around {
        // Adjacent flush placements relative to w1
        candidates.push((w1.2 + GAP, w1.1)); // Right, top-aligned
        candidates.push((w1.2 + GAP, w1.3 - new_h)); // Right, bottom-aligned
        candidates.push((w1.0 - new_w - GAP, w1.1)); // Left, top-aligned
        candidates.push((w1.0 - new_w - GAP, w1.3 - new_h)); // Left, bottom-aligned
        candidates.push((w1.0, w1.3 + GAP)); // Below, left-aligned
        candidates.push((w1.2 - new_w, w1.3 + GAP)); // Below, right-aligned
        candidates.push((w1.0, w1.1 - new_h - GAP)); // Above, left-aligned
        candidates.push((w1.2 - new_w, w1.1 - new_h - GAP)); // Above, right-aligned

        // Cursor-aligned flush placements relative to w1
        candidates.push((w1.2 + GAP, target_gy));
        candidates.push((w1.0 - new_w - GAP, target_gy));
        candidates.push((target_gx, w1.3 + GAP));
        candidates.push((target_gx, w1.1 - new_h - GAP));

        // Grid intersections with other windows
        for &w2 in existing {
            candidates.push((w1.2 + GAP, w2.1));
            candidates.push((w1.0, w2.3 + GAP));
            candidates.push((w1.0 - new_w - GAP, w2.1));
            candidates.push((w1.0, w2.1 - new_h - GAP));
            candidates.push((w1.2 + GAP, w2.3 + GAP));
        }
    }
    candidates
}

/// `(x, y, w, h)` comes within the 12 px tiling gap of one of the rects `(x0, y0, x1, y1)`.
pub(crate) fn overlaps_any(existing: &[(i64, i64, i64, i64)], r: (i64, i64, i64, i64)) -> bool {
    const GAP: i64 = 12;
    let (gx, gy) = (r.0, r.1);
    let gx1 = gx.saturating_add(r.2);
    let gy1 = gy.saturating_add(r.3);
    existing.iter().any(|&(ex0, ey0, ex1, ey1)| {
        !(gx1 + GAP <= ex0 || gx >= ex1 + GAP || gy1 + GAP <= ey0 || gy >= ey1 + GAP)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputDescriptor, Transform};
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, tests::key};

    pub(super) fn tree(mapped: bool) -> SurfaceTree {
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

    pub(super) fn connected(scale_120: u32) -> Core {
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

    // ------------------------------------------------------------ toplevel requests (M2, M9)

    use crate::{InputPort, OutputLifecycleToken};

    /// A `w × h` single-surface tree with key `k`.
    pub(super) fn win(k: u64, w: i32, h: i32, mapped: bool) -> SurfaceTree {
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

    pub(super) fn map_win(c: &mut Core, k: u64, w: i32, h: i32) -> ElementId {
        let id = c.toplevel_created();
        c.toplevel_committed(id, win(k, w, h, false)).unwrap();
        c.toplevel_committed(id, win(k, w, h, true)).unwrap();
        id
    }

    pub(super) fn out(
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

    pub(super) fn focus(c: &mut Core, i: &mut InputPort, id: ElementId) {
        c.set_focus(i, Some(id)).unwrap();
    }

    pub(super) const FS: ToplevelRequest = ToplevelRequest::SetFullscreen { output: None };

    /// What a client's request must never change: anchors, lifecycle, z-order, cameras, focus.
    pub(super) fn authority_state(c: &Core) -> String {
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

    pub(super) fn rects(c: &mut Core, o: OutputId) -> Vec<(ElementId, (i64, i64, i64, i64))> {
        c.render_list(o)
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.element, (e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1)))
            .collect()
    }

    // ------------------------------------------------------------ placement at first map

    pub(super) fn sized(w: i32, h: i32, mapped: bool) -> SurfaceTree {
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

    use crate::navigation::PersistedCameras;
    use crate::output::tests::key as okey;

    pub(super) fn plain(connector: &str) -> OutputDescriptor {
        OutputDescriptor {
            key: okey(connector),
            mode: (1280, 800),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        }
    }

    pub(super) fn placed(c: &mut Core, w: i32, h: i32) -> (ElementId, CanvasPoint) {
        let id = c.toplevel_created();
        c.toplevel_committed(id, sized(w, h, false)).unwrap();
        c.toplevel_committed(id, sized(w, h, true)).unwrap();
        (id, c.stage().anchor(id).unwrap())
    }

    pub(super) fn centred(mode: (u32, u32), scale_120: u32, camera: Option<(i64, i64)>) -> Core {
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

    // ------------------------------------------------- fullscreen and the scale policy (H-9)

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
}
