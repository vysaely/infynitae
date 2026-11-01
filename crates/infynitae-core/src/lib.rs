//! Infynitae Native core: the spatial and policy authority (FOUNDATION_ARCHITECTURE C1.2).
//!
//! Core coordinates the infinite canvas stage, per-output cameras and projection views, output
//! lifecycle and arrangement, pointer and keyboard input routing, focus, layer surfaces, window
//! clustering, scale policy, and frame scheduling.
//!
//! Authority model (INV-CAMERA-1, INV-OUTPUT-2, C1.5):
//! * camera state is mutable only inside [`navigation`], and every mutation consumes a fresh
//!   [`authority::Authorized`] proof;
//! * a proof can be minted only inside the port-carrying entry points (module `entry`);
//! * [`Core::new`] is the only place that creates [`InputPort`], [`ShellPort`] and
//!   [`OutputLifecycleToken`]; they have private fields, are not `Clone`, are `!Send` and are
//!   bound to the Core that minted them.

#![forbid(unsafe_code)]

pub mod frames;
pub mod geom;
pub mod input;
pub mod layer;
pub mod navigation;
pub mod output;
pub mod pointer;
pub mod projection;
pub mod protocol;
pub mod render;
pub mod scale;
pub mod snapshot;
pub mod stage;
pub mod touch;
pub mod visibility;

mod entry;
#[cfg(test)]
mod shell_tests;

pub use entry::authority;
use std::time::Duration;

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
    /// Centers the camera of the active output on an element.
    RecenterElement { element: stage::ElementId },
    /// A finger touched a touchscreen on `output` at `p` (output logical px, the screen's
    /// rotation already applied), in `slot` (one per finger).
    TouchDown {
        slot: u32,
        output: output::OutputId,
        p: (f64, f64),
        time_us: u64,
    },
    /// A finger down in `slot` moved to `p` on its output.
    TouchMotion {
        slot: u32,
        p: (f64, f64),
        time_us: u64,
    },
    /// The finger in `slot` lifted.
    TouchUp { slot: u32, time_us: u64 },
    /// The end of one touch frame (the events of a moment, all fingers).
    TouchFrame,
    /// The touchscreen gave up the touches (a gesture taken by the hardware, a palm).
    TouchCancel,
}

/// A semantic command from the trusted shell (C7.2, INV-SHELL-1): a value Core validates and then
/// executes through the same single-writer paths user input uses. There is no raw state write, no
/// size write and no mutable access to Core (owner decision 2026-10-20, D3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShellCommand {
    /// Centres the camera of `output` on a target at its current zoom (no zoom change, D5). Atomic
    /// (ADR-11: no animation); refused while `output` presents a protocol-fullscreen element or
    /// an interaction owns the pointer.
    Navigate {
        output: output::OutputId,
        to: NavigateTo,
        stream: ShellStream,
    },
    /// M8 `FitViewport(O, W)` named by the shell: F-FIT-1 on `element`'s window geometry now.
    ViewportFit {
        output: output::OutputId,
        element: stage::ElementRef,
    },
    /// M8 `ViewportRestore(O)`: the newest entry of O's history (OBJ-30).
    ViewportRestore { output: output::OutputId },
    /// Keyboard focus to `element` by an explicit action (INV-INPUT-8; A-8 declines a hidden one).
    Focus { element: stage::ElementRef },
    /// Clears `element`'s attention mark (OBJ-04, INV-INPUT-9).
    AcknowledgeAttention { element: stage::ElementRef },
    /// Places `element`'s anchor at `to` (Canvas px) through `stage::place`: R3 rounding, ±2³⁶,
    /// mapped and not fullscreen; rejected otherwise (C7.2). Never a size write.
    MoveElement {
        element: stage::ElementRef,
        to: (f64, f64),
        stream: ShellStream,
    },
    /// Asks `element` for `size` (a configure, as a Super+right drag asks; the size is held,
    /// INV-PROTO-1), or, with `maximized` (the place and size it had before), maximizes it as
    /// Super+Enter does on the first output, keeping its anchor. Session restore brings a window
    /// back at its size and maximized state this way (owner, 2026-10-31). Mapped, not fullscreen,
    /// sizes 1..=32768; rejected otherwise.
    ResizeElement {
        element: stage::ElementRef,
        size: (i32, i32),
        maximized: Option<((f64, f64), (i32, i32))>,
    },
}

/// Where [`ShellCommand::Navigate`] centres the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavigateTo {
    /// The centre of the element's window geometry.
    Element(stage::ElementRef),
    /// A Canvas position in px (quantised once, Q16 ties-even).
    Canvas(f64, f64),
    /// The current centre moved by a Canvas-px delta.
    Delta(f64, f64),
}

/// A command's place in a bounded shell stream (INV-SCALE-3 `ShellStream`; owner decision
/// 2026-10-20, D8). Outside a stream a command is an atomic, one-step epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellStream {
    /// Not part of a stream (or, while one is open, a step of it).
    #[default]
    Atomic,
    /// Opens the stream (a step if one is already open).
    Begin,
    /// A step of the open stream (opens it if none is).
    Step,
    /// The last step: the stream's epoch settles after it (explicit end-of-stream).
    End,
}

/// Why Core refused a shell command. Nothing was changed (commands are atomic, C7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    /// The port was minted by another Core.
    ForeignPort,
    /// The element reference resolves to no element of this instance (INV-STAGE-3).
    StaleElement,
    /// The output is not connected.
    UnknownOutput,
    /// A number is not finite or lies outside the Canvas (±2³⁶ px).
    InvalidArgument,
    /// The element is not mapped (or has no window geometry yet).
    NotMapped,
    /// The output presents a protocol-fullscreen element (C12.F17 Q1 B scope 2), or the element
    /// is fullscreen (C7.2 `MoveElement`).
    Presenting,
    /// An interaction owns the pointer (the camera is being dragged).
    Busy,
    /// Nothing to do: an empty history, or a degenerate window geometry (F-FIT-1).
    Refused,
}

/// Why an entry point refused a request.
#[derive(Debug, PartialEq, Eq)]
pub enum EntryError {
    /// The port or token was minted by another Core (C1.5, C12.R2.3).
    ForeignPort,
    /// The output descriptor is outside the representable domain (F-VIEW-1, C12.F14): a mode
    /// side of 0 or above 2¹⁵ px, or a scale outside 0.25 … 8; nothing was changed (M4).
    InvalidDescriptor,
    /// The `OutputId` is not connected (unknown, or already disconnected).
    UnknownOutput,
    /// Reconfiguration named a different `OutputKey`: an identity change is a disconnect plus a
    /// connect, never a reconfiguration (INV-OUTPUT-2).
    IdentityChanged,
    /// A physical event with a non-finite value; nothing was changed.
    InvalidEvent,
}

pub use protocol::PlacementPolicy;

/// Action taken on Super+Enter: toggle fitting the camera to the window (`Fit`), or toggle
/// maximizing the window to fill the usable viewport (`Maximize`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EnterMode {
    /// Super+Enter fits the camera to the window.
    #[default]
    Fit,
    /// Super+Enter maximizes the window to fill the usable viewport.
    Maximize,
}

/// Why no frame could be produced for an output.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The output is not connected.
    UnknownOutput,
    /// The output has no bound camera or its view parameters are invalid.
    NoView,
}

/// `s_max(E)` (F-SCALE-2, C12.F11): the largest logical side of E's tree and of every size Core
/// asks E for while that request stands — the protocol-fullscreen size while E is granted (C5.2:
/// the output's logical size, as `toplevel_configure_state` reports it) and the saved size
/// offered when its fullscreen ended, until E redraws at another size (M9).
fn s_max_of(
    stage: &stage::Stage,
    fullscreen: &[protocol::FullscreenState],
    restore_offers: &[(stage::ElementId, geom::LocalSize)],
    outputs: &[output::OutputModel],
    element: stage::ElementId,
) -> u32 {
    let tree = scale::s_max_for_tree(stage.tree(element));
    let granted = fullscreen
        .iter()
        .find(|f| f.element == element)
        .and_then(|f| outputs.iter().find(|o| o.id == f.output))
        .map_or(0, |o| {
            let (w, h) = o.logical_extent();
            (w.floor().max(0.0) as u32).max(h.floor().max(0.0) as u32)
        });
    let offered = restore_offers
        .iter()
        .find(|(e, _)| *e == element)
        .map_or(0, |(_, size)| size.w.max(size.h).max(0) as u32);
    tree.max(granted).max(offered)
}

/// The ProjectionFrame of one output and whether it is current: a change that makes it stale
/// (a pan, a presentation, an output reconfiguration) keeps its storage, and the next render list
/// rebuilds into it.
struct FrameSlot {
    output: output::OutputId,
    frame: render::ProjectionFrame,
    current: bool,
}

/// O's lattice reference `R_O` (OBJ-12, F-LATTICE-1 as amended by C12.F14) and the `k_O` and
/// `view_O` it was set at: a change of either (re)defines the view, and the reference is set
/// from the view origin again. History-dependent per-output state; dropped with the output.
#[derive(Clone, Copy, Debug)]
struct LatticeSlot {
    output: output::OutputId,
    r: (i64, i64),
    k: f64,
    view_size: (u32, u32),
}

pub struct Core {
    id: u64,
    stage: stage::Stage,
    cameras: navigation::CameraSet,
    outputs: Vec<output::OutputModel>,
    output_ids: output::OutputIds,
    /// ProjectionFrame per output (OBJ-15): the last list produced for that output, and whether
    /// it is still current. A stale frame keeps its storage for the next list (C14.3).
    frames: Vec<FrameSlot>,
    /// `R_O` per output (OBJ-12): written only by [`Core::update_lattice`], on F-LATTICE-1's
    /// transitions; read by every camera view, so render, membership and hit test share it.
    lattice: Vec<LatticeSlot>,
    /// OBJ-13, rewritten by every visibility pass.
    membership: visibility::MembershipTable,
    /// The storage of the next membership table of a scoped pass; swapped with `membership`
    /// (C14.3: per-output buffers reused; INV-ALLOC-1).
    membership_next: visibility::MembershipTable,
    /// Reused storage: every output's view, for one pass (`with_views`).
    views_buf: Vec<(output::OutputId, projection::View)>,
    /// Reused storage: one element's new membership rows.
    rows_buf: Vec<visibility::Row>,
    /// Reused storage: what every output presents, before a commit.
    presentations_buf: Vec<(output::OutputId, Option<stage::ElementId>)>,
    /// Reused storage: the outputs whose presentation a commit touched.
    touched_buf: Vec<output::OutputId>,
    /// OBJ-20, core part.
    pointer: pointer::PointerState,
    /// OBJ-25 and OBJ-26.
    input: input::InputState,
    /// OBJ-22: ActiveElementFocus.
    focus: input::focus::FocusModel,
    /// OBJ-21: authoritative active pointer constraint.
    constraint: Option<pointer::ConstraintState>,
    /// Authoritative pending pointer constraints per surface.
    pending_constraints: Vec<pointer::ConstraintState>,
    /// Super is held: pointer constraints (a game's locked or confined cursor) are released and
    /// stay pending until it is let go (owner, 2026-10-25: "feel free in my desktop").
    constraints_suspended: bool,
    /// Whether a held Super suspends constraints (`set_free_cursor_on_super`; off in Core: the
    /// specified behaviour keeps a lock through a Super+middle pan).
    free_cursor_on_super: bool,
    /// The focused window holds a keyboard-shortcuts inhibitor (`set_shortcuts_inhibited`):
    /// its keys go to it, except Super+L.
    shortcuts_inhibited: bool,
    /// OBJ-09: read only by `output_connected`.
    persisted: navigation::PersistedCameras,
    /// Authoritative scale policy configuration (PROV-1..3, PROV-5).
    scale_config: scale::ScalePolicyConfig,
    /// OBJ-16: Authoritative scale state per element.
    scales: scale::ScaleStates,
    /// Which elements the next batch-end scale evaluation visits (derived from `scales`).
    scale_sweep: scale::Sweep,
    /// The last ladder built (derived from the outputs and the configuration).
    ladders: scale::Ladders,
    /// Test reference: every batch-end scale evaluation visits every mapped element.
    #[cfg(test)]
    scale_reference: bool,
    /// Test observation: batch-end scale evaluations so far, (full, scoped).
    #[cfg(test)]
    scale_passes: (usize, usize),
    /// The latest session time handed to the scale policy (M7 deadlines are on this clock).
    scale_clock: Option<scale::SessionTime>,
    /// The placement policy of elements not yet mapped (INV-STAGE-1 writer (a)).
    placement: protocol::PlacementPolicy,
    /// OBJ-19: the protocol-fullscreen states, at most one per output and one per element. The
    /// only store of fullscreen state; written by `core::protocol` alone (INV-PRES-1).
    fullscreen: Vec<protocol::FullscreenState>,
    /// M2 pre-configure: toplevels that asked for fullscreen before their initial commit, with
    /// the output they named. Decided at the initial commit.
    premap_fullscreen: Vec<(stage::ElementId, Option<output::OutputId>)>,
    /// The size offered to an element when its fullscreen ended, kept until its window geometry
    /// changes: until then its geometry is still the fullscreen one, so a new grant saves this
    /// size and not that one. Written by `core::protocol` alone.
    restore_offers: Vec<(stage::ElementId, geom::LocalSize)>,
    /// The size each window was last asked for by the compositor (a maximize and its restore, a
    /// resize, the room, the size after fullscreen), carried by every later configure until the
    /// client acknowledges it and draws another size, or unmaps (INV-PROTO-1, amended
    /// 2026-10-31). Written by `core::protocol` alone.
    held_sizes: Vec<(stage::ElementId, geom::LocalSize)>,
    /// The windows Super+Enter maximized (`EnterMode::Maximize`) and the output whose usable
    /// area they fill: a change of that output fits them to it again.
    maximized: Vec<(stage::ElementId, output::OutputId)>,
    /// Windows whose fullscreen ended and that have not drawn a windowed frame yet: that frame
    /// decides whether they keep their place (owner, 2026-10-31). Written by `core::protocol`
    /// (and dropped by a Super+drag out of fullscreen, which places the window itself).
    returning: Vec<stage::ElementId>,
    /// The window a Super+right drag resizes, until the client has drawn the last size asked of
    /// it (`core::protocol` writes it).
    resize: Option<protocol::ResizeTrack>,
    /// Layer surfaces (L1), in creation order; the store and its only writer is `core::layer`.
    layers: Vec<layer::LayerRecord>,
    next_layer: u64,
    /// The usable area of each output after its layers' exclusive zones (L4); an output without
    /// an entry uses its whole logical extent.
    usable: Vec<(output::OutputId, layer::Area)>,
    /// A bounded shell stream is open (INV-SCALE-3 `ShellStream`): the scale policy treats its
    /// camera and Stage steps as one epoch until its explicit end (or a settlement event).
    shell_stream: bool,
    /// Elements that left protocol fullscreen before ever having been placed as a window. Their
    /// default placement is resolved at their first windowed presentation: the first commit
    /// after the client acknowledged a configure without the fullscreen state (P9b).
    windowed_placement_pending: Vec<stage::ElementId>,
    /// OBJ-17: frame liveness of every element (M16). Written by the commit entry points,
    /// `frame_tick`, `frame_kick` and `frame_fallback` only.
    frame_liveness: frames::FrameLiveness,
    /// Canvas pan interaction style (Grab vs Rts).
    pub pan_mode: navigation::PanMode,
    /// Whether keyboard focus follows pointer hover (Focus Follows Mouse).
    pub focus_follows_mouse: bool,
    /// Whether focus is cleared when pointer hovers over empty canvas (Strict vs Sloppy).
    pub focus_follows_mouse_strict: bool,
    /// The window under the pointer at the last motion: hover moves the focus only when this
    /// changes, so a focus moved by keyboard stays until the pointer really goes somewhere.
    hover_seen: Option<stage::ElementId>,
    /// Whether focus cycling (Super+Tab) moves the camera to the focused window.
    pub focus_cycle_navigate: bool,
    /// Super+Enter action mode: Maximize (fill usable viewport) vs Fit (camera zoom).
    pub enter_mode: EnterMode,
    /// Outputs on which fullscreen presentation is temporarily suppressed (e.g. user navigated to another window).
    pub(crate) presentation_suppressed: Vec<output::OutputId>,
    /// Whether Super+Middle pan or camera navigation moves out of fullscreen presentation.
    pub fullscreen_pan: bool,
    /// Whether newly opened windows are automatically focused.
    pub focus_on_open: bool,
    /// The user's key bindings (`set_key_bindings`).
    key_bindings: input::keys::UserBindings,
    /// `xdg_toplevel` parents, (child, parent), from the adapter (`set_toplevel_parent`).
    toplevel_parents: Vec<(stage::ElementId, stage::ElementId)>,
    /// The centre of each app's last destroyed window (app_id lowercase), newest last, at most
    /// 32: its next window takes that place (`successor_anchor`).
    closed_windows: Vec<(String, geom::CanvasPoint)>,
    /// Whether camera teleports/recenters to newly opened windows.
    pub teleport_on_open: bool,
    /// Whether physical input occurred since the last query (INV-IDLE-1).
    pub(crate) physical_input_activity: bool,
    /// The fingers down on a surface (`touch`).
    pub(crate) touches: touch::Touches,
    /// NEGATIVE PROBE (PD-F2-01): Core retaining a proof for a later mutation. Must make the
    /// protocol state fail its `Send` assertion (built through infynitae-wayland's probe feature).
    #[cfg(feature = "probe_sticky_fit_retains_authority")]
    sticky_fit: Option<authority::Authorized>,
}

/// The instance named by external references (INV-STAGE-3: `(InstanceId, ElementId)`). Element ids
/// restart at 1 in every compositor process and a shell outlives a restart, so the instance must
/// differ between runs, not only between the Cores of one process: the wall-clock start time, the
/// pid and the Core's id, mixed. Below 2⁵³ so a JSON number carries it exactly; never 0.
fn instance_id(core_id: u64) -> stage::InstanceId {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mixed = nanos ^ (u64::from(std::process::id()) << 21) ^ core_id.rotate_left(43);
    stage::InstanceId((mixed & ((1 << 53) - 1)).max(1))
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
                stage: stage::Stage::new(instance_id(id)),
                cameras: navigation::CameraSet::empty(),
                outputs: Vec::new(),
                output_ids: output::OutputIds::default(),
                frames: Vec::new(),
                lattice: Vec::new(),
                membership: visibility::MembershipTable::default(),
                membership_next: visibility::MembershipTable::default(),
                views_buf: Vec::new(),
                rows_buf: Vec::new(),
                presentations_buf: Vec::new(),
                touched_buf: Vec::new(),
                pointer: pointer::PointerState::NoOutput,
                input: input::InputState::default(),
                focus: input::focus::FocusModel::default(),
                constraint: None,
                pending_constraints: Vec::new(),
                constraints_suspended: false,
                free_cursor_on_super: false,
                shortcuts_inhibited: false,
                persisted,
                scale_config: scale::ScalePolicyConfig::default(),
                scales: scale::ScaleStates::default(),
                scale_sweep: scale::Sweep::default(),
                ladders: scale::Ladders::default(),
                #[cfg(test)]
                scale_reference: false,
                #[cfg(test)]
                scale_passes: (0, 0),
                scale_clock: None,
                placement: protocol::PlacementPolicy::default(),
                fullscreen: Vec::new(),
                premap_fullscreen: Vec::new(),
                restore_offers: Vec::new(),
                held_sizes: Vec::new(),
                maximized: Vec::new(),
                returning: Vec::new(),
                resize: None,
                shell_stream: false,
                layers: Vec::new(),
                next_layer: 0,
                usable: Vec::new(),
                windowed_placement_pending: Vec::new(),
                pan_mode: navigation::PanMode::default(),
                focus_follows_mouse: false,
                focus_follows_mouse_strict: false,
                hover_seen: None,
                focus_cycle_navigate: false,
                enter_mode: EnterMode::default(),
                presentation_suppressed: Vec::new(),
                fullscreen_pan: false,
                focus_on_open: false,
                key_bindings: input::keys::UserBindings::default(),
                toplevel_parents: Vec::new(),
                closed_windows: Vec::new(),
                teleport_on_open: false,
                physical_input_activity: false,
                touches: touch::Touches::default(),
                frame_liveness: frames::FrameLiveness::default(),
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

    /// Sets what `xdg_activation_v1` requests may do (Level D configuration; the default is
    /// `Strict`). Moves no focus by itself.
    pub fn set_activation_policy(&mut self, policy: input::focus::ActivationPolicy) {
        self.focus.set_activation_policy(policy);
    }

    /// Sets the canvas pan mode (`Grab` for paper dragging or `Rts` for camera scrolling).
    pub fn set_pan_mode(&mut self, mode: navigation::PanMode) {
        self.pan_mode = mode;
    }

    /// Sets the action of Super+Enter (`Fit` for camera zoom or `Maximize` to fill usable viewport).
    pub fn set_enter_mode(&mut self, mode: EnterMode) {
        self.enter_mode = mode;
    }

    /// Read access to the Super+Enter mode.
    pub fn enter_mode(&self) -> EnterMode {
        self.enter_mode
    }

    /// Sets whether keyboard focus follows mouse pointer motion (hover-to-focus).
    pub fn set_focus_follows_mouse(&mut self, enabled: bool) {
        self.focus_follows_mouse = enabled;
    }

    /// Read access to whether focus follows mouse motion.
    pub fn focus_follows_mouse(&self) -> bool {
        self.focus_follows_mouse
    }

    /// Sets whether focus follows mouse in strict mode (clears focus when hovering canvas).
    pub fn set_focus_follows_mouse_strict(&mut self, strict: bool) {
        self.focus_follows_mouse_strict = strict;
    }

    /// Read access to whether focus follows mouse is in strict mode.
    pub fn focus_follows_mouse_strict(&self) -> bool {
        self.focus_follows_mouse_strict
    }

    /// The window under the pointer, none under a layer surface above the windows: what strict
    /// focus follows.
    pub(crate) fn hovered_window(&mut self) -> Option<stage::ElementId> {
        let pointer::PointerState::Located { output, p } = self.pointer else {
            return None;
        };
        if self.layer_hit(output, p, true).is_some() {
            return None;
        }
        self.hit_test(output, geom::OutputLogicalPoint { x: p.0, y: p.1 })
            .map(|h| h.element)
    }

    /// Strict focus after the focused window went away: the window under the pointer, or none,
    /// never the last one used elsewhere on the canvas (Super+C next would close that one).
    pub(crate) fn strict_successor(
        &mut self,
        gone: stage::ElementId,
        out: &mut Vec<protocol::Effect>,
    ) {
        if !(self.focus_follows_mouse && self.focus_follows_mouse_strict) {
            return;
        }
        let hovered = self.hovered_window().filter(|&e| e != gone);
        if self.focus.target() != hovered {
            out.extend(self.focus.set_focus(hovered));
        }
    }

    /// Sets whether Super+Tab focus cycling automatically moves camera to the focused window.
    pub fn set_focus_cycle_navigate(&mut self, enabled: bool) {
        self.focus_cycle_navigate = enabled;
    }

    /// Read access to whether focus cycling navigates the camera.
    pub fn focus_cycle_navigate(&self) -> bool {
        self.focus_cycle_navigate
    }

    /// Whether physical input occurred since the last call. Cleared on read (INV-IDLE-1).
    pub fn take_physical_input_activity(&mut self) -> bool {
        std::mem::take(&mut self.physical_input_activity)
    }

    /// Whether fullscreen presentation is temporarily suppressed on `output`.
    pub fn is_presentation_suppressed(&self, output: output::OutputId) -> bool {
        self.presentation_suppressed.contains(&output)
    }

    /// Temporarily suppresses fullscreen presentation on `output` (e.g. to reveal canvas or other windows).
    pub fn suppress_presentation(
        &mut self,
        output: output::OutputId,
        out: &mut Vec<protocol::Effect>,
    ) {
        if !self.presentation_suppressed.contains(&output) {
            self.presentation_suppressed.push(output);
            self.reconcile_presentation_change(output, out);
        }
    }

    /// Unsuppresses fullscreen presentation on `output` (returning to exclusive fullscreen presentation).
    pub fn unsuppress_presentation(
        &mut self,
        output: output::OutputId,
        out: &mut Vec<protocol::Effect>,
    ) {
        if let Some(pos) = self
            .presentation_suppressed
            .iter()
            .position(|&o| o == output)
        {
            self.presentation_suppressed.swap_remove(pos);
            self.reconcile_presentation_change(output, out);
        }
    }

    /// Ends a suppression without reconciling: for a caller that recomputes presentation
    /// membership and scale itself right after (a grant taking the output, `take_output`).
    pub(crate) fn clear_presentation_suppression(&mut self, output: output::OutputId) {
        if let Some(pos) = self
            .presentation_suppressed
            .iter()
            .position(|&o| o == output)
        {
            self.presentation_suppressed.swap_remove(pos);
            self.invalidate_frame(output);
        }
    }

    /// Reconciles view switch on `output` (visibility pass, scale evaluation, pointer true position per M10).
    fn reconcile_presentation_change(
        &mut self,
        output: output::OutputId,
        out: &mut Vec<protocol::Effect>,
    ) {
        if let Some(camera) = self.cameras.camera_for_output(output) {
            self.camera_written(output, camera, out);
        } else {
            self.update_lattice(output);
            self.invalidate_frame(output);
            out.extend(self.visibility_pass());
            let on_it = matches!(self.pointer, pointer::PointerState::Located { output: o, .. } if o == output);
            if on_it && !self.input.interaction.active() {
                out.extend(self.reconcile_constraints());
                if !self.locked() {
                    self.true_position_into(out);
                }
            }
        }
    }

    /// Sets whether Super+Middle pan or camera navigation moves out of fullscreen presentation.
    pub fn set_fullscreen_pan(&mut self, enabled: bool) {
        self.fullscreen_pan = enabled;
    }

    /// Read access to whether fullscreen pan is enabled.
    pub fn fullscreen_pan(&self) -> bool {
        self.fullscreen_pan
    }

    /// Sets whether newly opened windows are automatically focused.
    pub fn set_focus_on_open(&mut self, enabled: bool) {
        self.focus_on_open = enabled;
    }

    /// Read access to whether focus on open is enabled.
    pub fn focus_on_open(&self) -> bool {
        self.focus_on_open
    }

    /// Sets whether camera teleports to newly opened windows.
    pub fn set_teleport_on_open(&mut self, enabled: bool) {
        self.teleport_on_open = enabled;
    }

    /// Read access to whether teleport on open is enabled.
    pub fn teleport_on_open(&self) -> bool {
        self.teleport_on_open
    }

    /// Centers the active output's camera on `element`.
    pub fn recenter_element(
        &mut self,
        port: &mut InputPort,
        element: stage::ElementId,
        buf: &mut protocol::EffectBuf,
    ) -> Result<(), EntryError> {
        self.physical_input(port, PhysicalEvent::RecenterElement { element }, buf)
    }

    /// Read access to the authoritative active pointer constraint (OBJ-21).
    pub fn constraint(&self) -> Option<pointer::ConstraintState> {
        self.constraint
    }

    /// Read access to the authoritative pending pointer constraint (OBJ-21).
    pub fn pending_constraint(&self) -> Option<pointer::ConstraintState> {
        self.pending_constraints.last().copied()
    }

    /// Read access to all authoritative pending pointer constraints.
    pub fn pending_constraints(&self) -> &[pointer::ConstraintState] {
        &self.pending_constraints
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
        // A-8: focus does not move to an element a presentation hides; the request is declined
        if target.is_some_and(|id| self.hidden_by_presentation(id)) {
            return Ok(Vec::new());
        }
        let mut fx = self.focus.set_focus(target);
        fx.extend(self.reconcile_constraints());
        Ok(fx)
    }

    /// A window that is an app's own notification (Steam's toasts, `notificationtoasts_<n>_desktop`,
    /// drawn as small X11 windows): placed in the screen's bottom right corner, never focused.
    pub fn is_notification(&self, id: stage::ElementId) -> bool {
        self.stage.meta(id).is_some_and(|m| {
            m.app_id
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case("steam"))
                && m.title
                    .as_deref()
                    .is_some_and(|t| t.starts_with("notificationtoasts_"))
        })
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
        if self.hidden_by_presentation(target) || self.is_notification(target) {
            return Ok(Vec::new());
        }
        let mut fx = self.focus.grant_initial_focus(target);
        if !fx.is_empty() {
            fx.extend(self.reconcile_constraints());
        }
        Ok(fx)
    }

    /// A-8: `element` is hidden by a presentation — an output presents another
    /// element, the camera bound to that output would show `element` were the presentation not
    /// replacing its view, and no output observes it. Keyboard focus is not moved to such an
    /// element (`set_focus`, the initial grant, FocusNext, the successor of a lost target). An
    /// element no output would show anyway (far from every view) is not hidden by a
    /// presentation and stays focusable, as without one.
    pub fn hidden_by_presentation(&self, element: stage::ElementId) -> bool {
        if !self.presenting() || self.membership.observing_outputs(element).next().is_some() {
            return false;
        }
        let Some((anchor, tree, bounds)) = self.stage.mapped(element) else {
            return false;
        };
        self.outputs.iter().any(|o| {
            matches!(self.presentation(o.id), Some(p) if p != element)
                && self.camera_view(o.id).is_some_and(|view| {
                    !render::entries_culled(&view, element, anchor, tree, bounds).is_empty()
                })
        })
    }

    /// The mapped elements, bottom to top, that keyboard focus may move to (A-8). Without a
    /// presentation that is every mapped element, at no extra cost.
    pub(crate) fn focusable_elements(&self) -> Vec<stage::ElementId> {
        let mut live = self.stage.mapped_elements();
        if self.presenting() && !self.fullscreen_pan {
            live.retain(|&e| !self.hidden_by_presentation(e));
        }
        live
    }

    /// The mapped elements in creation order (for Super+1..9 slot focus).
    pub(crate) fn focusable_elements_in_creation_order(&self) -> Vec<stage::ElementId> {
        self.stage.mapped_in_creation_order()
    }

    /// Windows partitioned into connected spatial clusters (sorted by earliest creation order).
    pub fn clusters(&self) -> Vec<Vec<stage::ElementId>> {
        let live = self.focusable_elements_in_creation_order();
        if live.is_empty() {
            return Vec::new();
        }
        let rects: Vec<(i64, i64, i64, i64)> = live
            .iter()
            .map(|&e| {
                let a = self
                    .stage
                    .anchor(e)
                    .unwrap_or(geom::CanvasPoint { x: 0, y: 0 });
                let (w, h) = self
                    .stage
                    .tree(e)
                    .map(|t| (t.geometry.size.w as i64, t.geometry.size.h as i64))
                    .unwrap_or((0, 0));
                (a.x, a.y, a.x.saturating_add(w), a.y.saturating_add(h))
            })
            .collect();

        const CLUSTER_GAP: i64 = 300;
        let n = live.len();
        let mut visited = vec![false; n];
        let mut clusters: Vec<Vec<stage::ElementId>> = Vec::new();

        for i in 0..n {
            if visited[i] {
                continue;
            }
            visited[i] = true;
            let mut cluster = vec![live[i]];
            let mut queue = vec![i];
            while let Some(curr) = queue.pop() {
                let (ax0, ay0, ax1, ay1) = rects[curr];
                for j in 0..n {
                    if !visited[j] {
                        let (bx0, by0, bx1, by1) = rects[j];
                        let dx = (ax0 - bx1).max(bx0 - ax1).max(0);
                        let dy = (ay0 - by1).max(by0 - ay1).max(0);
                        if dx <= CLUSTER_GAP && dy <= CLUSTER_GAP {
                            visited[j] = true;
                            cluster.push(live[j]);
                            queue.push(j);
                        }
                    }
                }
            }
            clusters.push(cluster);
        }
        clusters
    }

    /// Returns the cluster containing `element`, or a single-element list if none.
    pub fn cluster_for_element(&self, element: stage::ElementId) -> Vec<stage::ElementId> {
        for cluster in self.clusters() {
            if cluster.contains(&element) {
                return cluster;
            }
        }
        vec![element]
    }

    /// Whether any output presents an element (a mapped fullscreen element).
    fn presenting(&self) -> bool {
        self.fullscreen()
            .iter()
            .any(|f| self.stage.mapped(f.element).is_some())
    }

    /// FocusNext binding (Super+Tab): the window that most recently asked for attention if one
    /// does, else the window used before the focused one (`FocusModel::focus_next`). Returns the
    /// effects and whether it went to a window that asked for attention (the camera then always
    /// follows).
    pub(crate) fn focus_next(&mut self) -> (Vec<protocol::Effect>, bool) {
        let live = self.focusable_elements();
        let (mut fx, attention) = match self.focus.focus_attention(&live) {
            Some(fx) => (fx, true),
            None => (self.focus.focus_next(&live), false),
        };
        // Super+Tab brings the newly focused window to the front; what lies under the pointer may
        // have changed, so it gets one true-position event (M10), as after a camera write
        let raised = self.focus.target().is_some_and(|id| self.raise_by_user(id));
        fx.extend(self.reconcile_constraints());
        if raised
            && matches!(self.pointer, pointer::PointerState::Located { .. })
            && !self.input.interaction.active()
            && !self.locked()
        {
            self.true_position_into(&mut fx);
        }
        (fx, attention)
    }

    /// A new window opened without the focus: FOCUS_ON_OPEN "user" (the user did not open it)
    /// or "priority" (the focus was busy) held it back. It is marked for attention, so Super+Tab
    /// reaches it first and the edge shows the mark (owner, 2026-10-31). Never a focus or camera
    /// change; a window that is not mapped is ignored.
    pub fn opened_without_focus(&mut self, id: stage::ElementId) {
        if self.stage.state(id) == Some(stage::Lifecycle::Mapped) {
            self.focus.mark_attention(id);
        }
    }

    /// Resolves an evdev keycode and modifier snapshot against bindings.
    pub fn resolve_key_binding(
        &self,
        key: u32,
        mods: input::bindings::Modifiers,
        interaction_active: bool,
    ) -> Option<input::keys::KeyAction> {
        input::keys::resolve_key_binding_with(&self.key_bindings, key, mods, interaction_active)
    }

    /// SUPER_FREES_CURSOR: holding Super releases a game's locked or confined pointer until it
    /// is let go.
    pub fn set_free_cursor_on_super(&mut self, enabled: bool) {
        self.free_cursor_on_super = enabled;
    }

    /// The focused window took over the compositor's shortcuts (`keyboard-shortcuts-inhibit`), or
    /// gave them back. While it holds them, every key goes to it except Super+L, Super+Tab, Super+W,
    /// Super+D and Super+A, so an app can never keep you from locking (owner, 2026-10-25), leaving it or
    /// reaching the edge (2026-11-01). Quitting the session and switching
    /// terminals are the console backend's and never reach Core.
    pub fn set_shortcuts_inhibited(&mut self, inhibited: bool) {
        self.shortcuts_inhibited = inhibited;
    }

    /// The user's key bindings from the config (live reload), checked before the built-in ones.
    pub fn set_key_bindings(&mut self, bindings: input::keys::UserBindings) {
        self.key_bindings = bindings;
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
        visibility::clock_output(self.membership.observing_outputs(element), &self.outputs)
    }

    /// A vblank of `output` (the backend's frame tick): the observed elements clocked by it, whose
    /// requested callbacks complete once and whose feedback is presented with `sync_output` =
    /// `output` (INV-FRAME-1, INV-FRAME-2, M16). Their callbacks are recorded as completed
    /// (OBJ-17). Unobserved elements are not completed here: see [`Core::frame_kick`] and
    /// [`Core::frame_fallback`]. Allocating; adapters use [`Core::frame_tick_into`] (a source
    /// rule forbids this form outside Core).
    pub fn frame_tick_vec(&mut self, output: output::OutputId) -> Vec<stage::ElementId> {
        let mut out = Vec::new();
        self.frame_tick_into(output, &mut out);
        out
    }

    /// [`Core::frame_tick_vec`] into the caller's reusable buffer: the clocked elements are appended
    /// to `out` (the caller clears it) and recorded as completed (OBJ-17); allocation only when
    /// `out` must grow (INV-ALLOC-1).
    pub fn frame_tick_into(&mut self, output: output::OutputId, out: &mut Vec<stage::ElementId>) {
        let start = out.len();
        // only an observed element has a clock, so the membership rows are the candidates
        out.extend(
            self.membership
                .observed()
                .filter(|&e| self.clock_output_of(e) == Some(output)),
        );
        for &e in &out[start..] {
            self.frame_liveness.completed(e);
        }
    }

    /// The kick (INV-FRAME-3 (a), M16): the adapter asks before it applies each effect. The
    /// elements returned are unobserved, have a pending callback, and are about to receive a
    /// non-initial configure, a `popup_done` or an input event (including focus `enter`/`leave`)
    /// through `effect`; their withheld callbacks must complete before the effect is applied,
    /// and are recorded as completed. Initial configures and `wl_surface` enter/leave do not
    /// kick. A `DismissPopupGrab` reaches the element whose popup holds the active grab, which
    /// the adapter owns and passes as `popup_grab_owner` (`None`: no grab, no `popup_done`, no
    /// kick); it is ignored for every other effect.
    pub fn frame_kick(
        &mut self,
        effect: &protocol::Effect,
        popup_grab_owner: Option<stage::ElementId>,
    ) -> [Option<stage::ElementId>; 2] {
        use protocol::Effect as E;
        let mut recipients = [None, None];
        match *effect {
            E::ToplevelConfigure(e)
            | E::ToplevelConfigureSize { element: e, .. }
            | E::CloseRequest(e)
            | E::ForceClose(e) => recipients[0] = Some(e),
            E::DismissPopupGrab(_) => recipients[0] = popup_grab_owner,
            E::KeyboardFocus(new) => {
                let old = std::mem::replace(&mut self.frame_liveness.keyboard_recipient, new);
                recipients = [old, new.filter(|n| Some(*n) != old)];
            }
            // the keyboard leaves the window for a layer surface: the window gets `leave`
            E::LayerKeyboardFocus(_) => {
                recipients[0] = self.frame_liveness.keyboard_recipient.take();
            }
            E::KeyboardForward { .. } => recipients[0] = self.frame_liveness.keyboard_recipient,
            E::PointerMotion { target, .. } => {
                let new = target.map(|t| t.surface);
                let old = std::mem::replace(&mut self.frame_liveness.pointer_surface, new);
                // a pointer that leaves a surface sends it `leave`
                let old = old.filter(|o| Some(*o) != new);
                recipients = [
                    old.and_then(|s| self.frame_kick_surface(s)),
                    new.and_then(|s| self.frame_kick_surface(s)),
                ];
                if recipients[0] == recipients[1] {
                    recipients[1] = None;
                }
            }
            E::PointerRelative { target, .. } => {
                recipients[0] = target.and_then(|t| self.frame_kick_surface(t.surface));
            }
            E::PointerButton { .. } | E::PointerAxis { .. } => {
                recipients[0] = self
                    .frame_liveness
                    .pointer_surface
                    .and_then(|s| self.frame_kick_surface(s));
            }
            E::ConstraintActivate { surface, .. } | E::ConstraintDeactivate { surface, .. } => {
                recipients[0] = self.frame_kick_surface(surface);
            }
            // a finger on a window is input to it: the window it landed on
            E::TouchDown { target, .. } => {
                recipients[0] = self.frame_kick_surface(target.surface);
            }
            E::TouchMotion { slot, .. } => {
                recipients[0] = self
                    .touches
                    .get(slot)
                    .and_then(|t| self.frame_kick_surface(t.target.surface));
            }
            E::TouchUp { .. } | E::TouchFrame | E::TouchCancel => {}
            E::InitialConfigure(_)
            | E::LayerConfigure { .. }
            | E::LayerClosed(_)
            | E::LayerPreferredScale { .. }
            | E::OutputAdded(_)
            | E::OutputChanged(_)
            | E::OutputRemoved(_)
            | E::Enter { .. }
            | E::Leave { .. }
            | E::PointerFrame
            | E::PointerZeroOutputBegin
            | E::PointerZeroOutputEnd
            | E::KeyboardIntercept { .. }
            // a scale hint is not in INV-FRAME-3's kick list (ARCHITECTURE scenario 11); an
            // unobserved element's hint is held, except that P7 still lowers it
            | E::PreferredScale { .. }
            | E::ToggleWindowList
            | E::ToggleLauncher
            | E::SpawnTerminal
            | E::SpawnFileManager
            | E::SpawnScreenshot
            | E::ToggleOverview
            | E::SpawnRegionScreenshot
            | E::ReloadShell
            | E::SpawnLockScreen
            | E::SpawnClipboardHistory
            | E::SpawnControlMenu
            | E::TogglePrivacy
            | E::ToggleIdleInhibit
            | E::ToggleTearing
            | E::SaveReplay
            | E::ToggleRecording
            | E::VolumeUp
            | E::VolumeDown
            | E::VolumeMute
            | E::BrightnessUp
            | E::BrightnessDown
            | E::MediaPlayPause
            | E::MediaNext
            | E::MediaPrev
            | E::Exec(_) => {}
        }
        recipients.map(|r| {
            r.filter(|&e| {
                let observed = self.membership.is_observed(e);
                self.frame_liveness.kick(e, observed)
            })
        })
    }

    /// The pending, unobserved element whose surface tree holds `surface`. Only elements with a
    /// pending callback are examined: the kick concerns no other element, and an input event on
    /// an observed or idle surface costs no search.
    fn frame_kick_surface(&self, surface: snapshot::SurfaceKey) -> Option<stage::ElementId> {
        self.frame_liveness.pending_where(
            |e| self.membership.is_observed(e),
            |e| {
                self.stage
                    .tree(e)
                    .is_some_and(|t| t.surfaces.iter().any(|s| s.key == surface))
            },
        )
    }

    /// The fallback (INV-FRAME-3 (b), M16) at `now` on the presentation clock: the unobserved
    /// elements whose callbacks have been pending for at least the period P since their last
    /// completion; the adapter completes them. Completions decided since the last call (vblank,
    /// kick) are stamped with `now`. The backend calls this once per loop iteration, after
    /// applying that iteration's effects, and arms its timer from
    /// [`Core::frame_fallback_deadline`].
    pub fn frame_fallback(&mut self, now: Duration) -> Vec<stage::ElementId> {
        let mut due = Vec::new();
        let membership = &self.membership;
        self.frame_liveness
            .evaluate(now, |e| membership.is_observed(e), &mut due);
        due
    }

    /// A capture of `element` alone was drawn (sharing one window, M16's capture rule): its
    /// pending callbacks complete, whether or not it is on a screen. Whether there were any: the
    /// adapter then completes them (`WaylandState::window_captured`).
    pub fn frame_captured(&mut self, element: stage::ElementId) -> bool {
        let pending = self.frame_liveness.requested(element);
        self.frame_liveness.completed(element);
        pending
    }

    /// When the fallback timer must next fire, on the clock of [`Core::frame_fallback`]; `None`
    /// when no unobserved element has a pending callback, and then no timer is armed
    /// (INV-IDLE-1).
    pub fn frame_fallback_deadline(&self) -> Option<Duration> {
        self.frame_liveness
            .deadline(|e| self.membership.is_observed(e))
    }

    /// Sets the off-screen fallback period P (PROV-4; finite and > 0).
    pub fn set_frame_fallback_period(
        &mut self,
        period: Duration,
    ) -> Result<(), frames::InvalidFallbackPeriod> {
        self.frame_liveness.set_period(period)
    }

    /// The off-screen fallback period P.
    pub fn frame_fallback_period(&self) -> Duration {
        self.frame_liveness.period()
    }

    /// Whether `element` has a callback that has not completed (OBJ-17).
    pub fn frame_pending(&self, element: stage::ElementId) -> bool {
        self.frame_liveness.requested(element)
    }

    pub(crate) fn views(&self) -> Vec<(output::OutputId, projection::View)> {
        let mut out = Vec::new();
        self.fill_views(&mut out);
        out
    }

    /// Every output's view, into reused storage (cleared, not freed).
    fn fill_views(&self, out: &mut Vec<(output::OutputId, projection::View)>) {
        out.clear();
        out.extend(
            self.outputs
                .iter()
                .filter_map(|o| self.view(o.id).map(|v| (o.id, v))),
        );
    }

    /// Every output's frame is stale (a z-order change shows wherever the element is).
    pub(crate) fn invalidate_all_frames(&mut self) {
        for slot in &mut self.frames {
            slot.current = false;
        }
    }

    /// The output's frame is stale (its view or what it shows changed); its storage is kept.
    pub(crate) fn invalidate_frame(&mut self, output: output::OutputId) {
        for slot in self.frames.iter_mut().filter(|s| s.output == output) {
            slot.current = false;
        }
    }

    /// The output is gone: its frame and storage go with it.
    pub(crate) fn remove_frame(&mut self, output: output::OutputId) {
        self.frames.retain(|s| s.output != output);
    }

    /// The output's current frame, if one is.
    fn current_frame(&self, output: output::OutputId) -> Option<&render::ProjectionFrame> {
        self.frames
            .iter()
            .find(|s| s.output == output && s.current)
            .map(|s| &s.frame)
    }

    /// The visibility pass (OBJ-13): membership recomputed from Stage, cameras and outputs; its
    /// diff becomes enter/leave effects. Run at the end of a core batch that changes the set of
    /// outputs or an output's hardware facts; a batch that changes one element or one camera
    /// runs the scoped pass for it.
    pub(crate) fn visibility_pass(&mut self) -> Vec<protocol::Effect> {
        self.visibility_pass_internal(false)
    }

    pub(crate) fn visibility_pass_internal(
        &mut self,
        is_client_response: bool,
    ) -> Vec<protocol::Effect> {
        let new = visibility::MembershipTable::compute(&self.stage, &self.views());
        let mut fx = Vec::new();
        self.membership.diff(&new, &self.stage, &mut fx);
        self.membership = new;
        self.scale_sweep.all = true;
        // an output change: a system change that touches every element (C12.F13)
        self.scale_sweep.system = true;
        fx.extend(self.evaluate_scale_policy(is_client_response));
        fx
    }

    /// The visibility pass of a batch that changed only `element` (its snapshot, lifecycle or
    /// anchor): no other element's membership depends on it. It marks the element for the batch's
    /// one scale evaluation (INV-SCALE-5), which the caller runs after every other change of the
    /// batch (a presentation that started or ended).
    pub(crate) fn membership_pass_element(
        &mut self,
        element: stage::ElementId,
        out: &mut Vec<protocol::Effect>,
    ) {
        let mut views = std::mem::take(&mut self.views_buf);
        self.fill_views(&mut views);
        self.membership
            .replace_element(element, &self.stage, &views, &mut self.rows_buf, out);
        self.views_buf = views;
        self.scale_sweep.mark(element);
    }

    /// The visibility pass of a batch that changed only `camera`: the outputs it is bound to are
    /// the only views that moved. The scale policy is evaluated for the elements those views
    /// observed before or observe after: no other element's requirement ρ can have changed.
    pub(crate) fn visibility_pass_camera(
        &mut self,
        camera: usize,
        out: &mut Vec<protocol::Effect>,
    ) {
        // the next table is built in its reused storage: a copy of this one, then each moved
        // output's rows recomputed in place
        let mut next = std::mem::take(&mut self.membership_next);
        let mut moved_any = false;
        for i in 0..self.outputs.len() {
            let o = self.outputs[i].id;
            if self.cameras.camera_for_output(o) == Some(camera) {
                let view = self.view(o);
                if moved_any {
                    next.recompute_output(o, view.as_ref(), &self.stage);
                } else {
                    next.assign_with_output(
                        &self.membership,
                        o,
                        view.as_ref(),
                        &self.stage,
                        &mut self.rows_buf,
                    );
                    moved_any = true;
                }
            }
        }
        if moved_any {
            let moved = |o| self.cameras.camera_for_output(o) == Some(camera);
            for (element, _, o) in self.membership.rows().iter().chain(next.rows()) {
                if moved(*o) {
                    self.scale_sweep.mark(*element);
                }
            }
            self.membership.diff(&next, &self.stage, out);
            std::mem::swap(&mut self.membership, &mut next);
        }
        self.membership_next = next;
        out.extend(self.evaluate_scale_policy(false));
    }

    /// Read scale policy configuration (PROV-1..3, PROV-5).
    pub fn scale_config(&self) -> &scale::ScalePolicyConfig {
        &self.scale_config
    }

    /// Update scale policy configuration. A value outside its allowed range (§PROV) is refused
    /// and the configuration in force is kept.
    pub fn set_scale_config(
        &mut self,
        config: scale::ScalePolicyConfig,
    ) -> Result<Vec<protocol::Effect>, scale::ScaleConfigError> {
        config.validate()?;
        self.scale_config = config;
        self.scale_sweep.all = true;
        // C12.F13: a configuration change touches no element; P7 applies at once
        Ok(self.apply_device_bound_now())
    }

    /// Ingests the renderer's device limit `D_max` (OB-13, F-SCALE-2, OBJ-29) and runs P7.
    /// `D_max = 0` is refused: the ceiling in force is kept and nothing is emitted.
    pub fn set_renderer_device_max(
        &mut self,
        d_max: u32,
    ) -> Result<Vec<protocol::Effect>, scale::ScaleConfigError> {
        scale::ScalePolicyConfig {
            d_max,
            ..self.scale_config
        }
        .validate()?;
        self.scale_config.d_max = d_max;
        self.scale_sweep.all = true;
        Ok(self.apply_device_bound_now())
    }

    /// P7 for every element under the configuration in force, at once (a reported device limit,
    /// a configuration change): lowered hints, and integer hints whose bound changed. Releases no
    /// hold (C12.F13).
    fn apply_device_bound_now(&mut self) -> Vec<protocol::Effect> {
        let mut fx = Vec::new();
        for (element, st) in &mut self.scales {
            let s_max = s_max_of(
                &self.stage,
                &self.fullscreen,
                &self.restore_offers,
                &self.outputs,
                *element,
            );
            let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
            let h_max = scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
            let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
            let lowered = st.p7_check(ladder, h_max, self.scale_config.sigma);
            // b(h) depends on H_dev(E) even when h itself still fits (E1R-N3)
            let s120 = scale::preferred_scale_120(st.h);
            let b = scale::preferred_buffer_scale(st.h, h_dev);
            if lowered
                || st.last_emitted_scale_120 != Some(s120)
                || st.last_emitted_buffer_scale != Some(b)
            {
                st.last_emitted_scale_120 = Some(s120);
                st.last_emitted_buffer_scale = Some(b);
                fx.push(protocol::Effect::PreferredScale {
                    element: *element,
                    scale_120: s120,
                    integer_scale: b,
                });
            }
        }
        fx
    }

    /// Preferred scale for an element: (scale_120, integer_buffer_scale).
    pub fn preferred_scale_of(&self, element: stage::ElementId) -> Option<(u32, u32)> {
        self.scales.get(element).map(|st| {
            let s_max = s_max_of(
                &self.stage,
                &self.fullscreen,
                &self.restore_offers,
                &self.outputs,
                element,
            );
            let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
            (
                scale::preferred_scale_120(st.h),
                scale::preferred_buffer_scale(st.h, h_dev),
            )
        })
    }

    /// Read scale state of an element.
    pub fn scale_state(&self, element: stage::ElementId) -> Option<&scale::ScaleState> {
        self.scales.get(element)
    }

    /// Returns the smallest active deadline among open ScaleInteractionEpochs.
    pub fn next_settlement_deadline(&self) -> Option<scale::SessionTime> {
        self.scales
            .iter()
            .filter_map(|(_, st)| {
                if st.epoch.open {
                    st.epoch.deadline
                } else {
                    None
                }
            })
            .min()
            .map(scale::SessionTime::from_millis)
    }

    /// The session clock as Core has seen it: the latest [`scale::SessionTime`] any caller
    /// handed in. A time earlier than that is read as that latest time, so a clock that steps
    /// backwards cannot produce a deadline in the past or settle an epoch out of order.
    fn session_time(&mut self, now: scale::SessionTime) -> u64 {
        self.scale_clock = self.scale_clock.max(Some(now));
        self.scale_clock.unwrap_or(now).millis()
    }

    /// Advances time for active ScaleInteractionEpochs, settling those whose deadline expired.
    pub fn scale_tick(&mut self, now: scale::SessionTime) -> Vec<protocol::Effect> {
        let now_ms = self.session_time(now);
        let mut fx = Vec::new();
        for (element, st) in &mut self.scales {
            let s_max = s_max_of(
                &self.stage,
                &self.fullscreen,
                &self.restore_offers,
                &self.outputs,
                *element,
            );
            let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
            let h_max = scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
            let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
            let open = st.epoch.open;
            let settled = st.tick(now_ms, ladder, h_max, self.scale_config.sigma);
            self.scale_sweep.closed |= open && !st.epoch.open;
            if settled {
                let s120 = scale::preferred_scale_120(st.h);
                let b = scale::preferred_buffer_scale(st.h, h_dev);
                st.last_emitted_scale_120 = Some(s120);
                st.last_emitted_buffer_scale = Some(b);
                fx.push(protocol::Effect::PreferredScale {
                    element: *element,
                    scale_120: s120,
                    integer_scale: b,
                });
            }
        }
        fx
    }

    /// Explicitly opens a ScaleInteractionEpoch on an element (TST-24).
    pub fn start_scale_interaction(
        &mut self,
        element: stage::ElementId,
        kind: scale::InteractionKind,
        continuous: bool,
        now: scale::SessionTime,
    ) {
        let now_ms = self.session_time(now);
        if let Some(st) = self.scales.get_mut(element) {
            // A step is not an evaluation: the batch that ends after it evaluates the element.
            self.scale_sweep.mark(element);
            if continuous {
                st.continuous_step(kind);
            } else {
                st.discrete_step(kind, now_ms, self.scale_config.t_settle_ms);
            }
        }
    }

    /// Settles open epochs matching `kind` across all elements.
    pub fn settle_interaction(&mut self, kind: scale::InteractionKind) -> Vec<protocol::Effect> {
        let mut fx = Vec::new();
        for (element, st) in &mut self.scales {
            if st.epoch.open && st.epoch.kind == Some(kind) {
                let s_max = s_max_of(
                    &self.stage,
                    &self.fullscreen,
                    &self.restore_offers,
                    &self.outputs,
                    *element,
                );
                let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
                let h_max = scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
                let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
                self.scale_sweep.closed = true;
                if st.settle(ladder, h_max, self.scale_config.sigma) {
                    let s120 = scale::preferred_scale_120(st.h);
                    let b = scale::preferred_buffer_scale(st.h, h_dev);
                    st.last_emitted_scale_120 = Some(s120);
                    st.last_emitted_buffer_scale = Some(b);
                    fx.push(protocol::Effect::PreferredScale {
                        element: *element,
                        scale_120: s120,
                        integer_scale: b,
                    });
                }
            }
        }
        fx
    }

    /// Settles all open epochs immediately (on output topology changes).
    pub fn settle_all_epochs(&mut self) -> Vec<protocol::Effect> {
        let mut fx = Vec::new();
        for (element, st) in &mut self.scales {
            let s_max = s_max_of(
                &self.stage,
                &self.fullscreen,
                &self.restore_offers,
                &self.outputs,
                *element,
            );
            let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
            let h_max = scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
            let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);
            self.scale_sweep.closed |= st.epoch.open;
            if st.settle(ladder, h_max, self.scale_config.sigma) {
                let s120 = scale::preferred_scale_120(st.h);
                let b = scale::preferred_buffer_scale(st.h, h_dev);
                st.last_emitted_scale_120 = Some(s120);
                st.last_emitted_buffer_scale = Some(b);
                fx.push(protocol::Effect::PreferredScale {
                    element: *element,
                    scale_120: s120,
                    integer_scale: b,
                });
            }
        }
        fx
    }

    /// Calculates required scale ρ(E) over observing presentations (F-SCALE-1, INV-SCALE-1).
    pub fn compute_rho(&self, element: stage::ElementId) -> Option<f64> {
        let observers = self.membership.observing_outputs(element);
        // an output presenting an element observes through the F-PRESENT-1 view, at zoom 1
        scale::compute_rho(observers, &self.outputs, &self.cameras, |o| {
            self.presentation(o).is_some() && !self.is_presentation_suppressed(o)
        })
    }

    /// The batch-end evaluation of the scale policy: for every mapped element whose evaluation
    /// can change anything, which [`scale::Sweep`] decides from the elements marked in it (whose
    /// inputs this batch changed) and the kind of batch. A full pass visits the mapped elements
    /// bottom to top; a scoped one, the marked elements in ascending order.
    pub(crate) fn evaluate_scale_policy(
        &mut self,
        is_client_response: bool,
    ) -> Vec<protocol::Effect> {
        let mut fx = Vec::new();

        // The interaction this batch belongs to. A camera drag is continuous: its epoch has no
        // deadline and is settled by the release. This path has no clock, so it can open only
        // such an epoch; a discrete step needs the time of the step and enters through
        // `start_scale_interaction`.
        let continuous_kind = match self.input.interaction {
            input::interaction::Interaction::CameraPan { .. }
            | input::interaction::Interaction::ElementMove { pan: Some(_), .. } => {
                Some(scale::InteractionKind::CameraPan)
            }
            // a shell stream's steps form one epoch, ended by its explicit end-of-stream
            _ if self.shell_stream => Some(scale::InteractionKind::ShellStream),
            _ => None,
        };
        #[cfg(not(test))]
        let reference = false;
        #[cfg(test)]
        let reference = self.scale_reference;
        let full = reference
            || self
                .scale_sweep
                .needs_full_pass(continuous_kind.is_some(), is_client_response);
        let mapped = |e| self.stage.state(e) == Some(stage::Lifecycle::Mapped);
        // an output change touches every element; otherwise the marks say which elements this
        // batch's stimulus touched (C12.F13). A full re-evaluation asked for by a device limit or
        // a configuration (`all`) touches nothing by itself.
        let everything = self.scale_sweep.system;
        let mut visit = self.scale_sweep.take_marked();
        self.scale_sweep.remember_touched(&visit);
        if full {
            visit.clear();
            visit.extend(self.stage.z_order().iter().copied().filter(|&e| mapped(e)));
        } else {
            visit.retain(|&e| mapped(e));
        }
        let mut closed = false;
        #[cfg(test)]
        if full {
            self.scale_passes.0 += 1;
        } else {
            self.scale_passes.1 += 1;
        }

        for &element in &visit {
            let rho = self.compute_rho(element);
            let s_max = s_max_of(
                &self.stage,
                &self.fullscreen,
                &self.restore_offers,
                &self.outputs,
                element,
            );
            let h_dev = scale::h_dev(s_max, self.scale_config.d_max);
            let h_max = scale::h_max(self.scale_config.h_cfg, s_max, self.scale_config.d_max);
            let ladder = self.ladders.get(&self.outputs, h_max, self.scale_config.r);

            // C12.F13: the subject of a client's fullscreen enter/exit,
            // and the elements that presentation change touches
            let subject = self.scale_sweep.is_stimulated(element);
            let by_presentation = self.scale_sweep.presentation_change && !subject;
            let touched = everything || self.scale_sweep.is_touched(element);
            let sigma = self.scale_config.sigma;
            let scale_state = self
                .scales
                .get_or_insert_with(element, || scale::ScaleState::new(rho.unwrap_or(1.0)));
            if subject {
                // its own fullscreen enter/exit settles its epoch (M9)
                scale_state.end_epoch();
            } else if !scale_state.epoch.open
                && let Some(kind) = continuous_kind
            {
                // An epoch that is already open is left as it is: an evaluation is not a step,
                // and re-arming it here would move a discrete epoch's deadline at every client
                // commit.
                scale_state.continuous_step(kind);
            }
            // a decrease other than P7 (P7 always applies)
            let lowers = scale_state.h <= h_max + 1e-12
                && rho.is_some_and(|r| {
                    scale::target(r, ladder, h_max, sigma) < scale_state.h - 1e-12
                });
            let changed = if subject {
                scale_state.presentation_hold = false;
                scale_state.evaluate(rho, ladder, h_max, &self.scale_config, false)
            } else if by_presentation && lowers {
                // another client's presentation never lowers this element: held
                scale_state.presentation_hold = true;
                scale_state.last_rho = rho;
                false
            } else if by_presentation {
                // ... but may raise it (a revealed window is never left low); a requirement back
                // at or above the hint ends the hold
                scale_state.presentation_hold = false;
                scale_state.evaluate(rho, ladder, h_max, &self.scale_config, false)
            } else if !is_client_response && touched {
                // a non-client stimulus that touches it (and is not another client's
                // presentation) releases the hold
                scale_state.presentation_hold = false;
                scale_state.evaluate(rho, ladder, h_max, &self.scale_config, false)
            } else if !is_client_response && scale_state.presentation_hold && lowers {
                // a camera action elsewhere (visited by a full pass, not touched) keeps the hold
                scale_state.last_rho = rho;
                false
            } else if !is_client_response {
                scale_state.evaluate(rho, ladder, h_max, &self.scale_config, false)
            } else if scale_state.presentation_hold && lowers {
                scale_state.last_rho = rho;
                false
            } else {
                // a requirement at or above the hint ends a hold: from then on the element's own
                // geometry decides again (SC-E), with nothing left of another client's change
                if !lowers {
                    scale_state.presentation_hold = false;
                }
                scale_state.evaluate(rho, ladder, h_max, &self.scale_config, true)
            };

            let s120 = scale::preferred_scale_120(scale_state.h);
            let b = scale::preferred_buffer_scale(scale_state.h, h_dev);

            if changed
                || scale_state.last_emitted_scale_120 != Some(s120)
                || scale_state.last_emitted_buffer_scale != Some(b)
            {
                scale_state.last_emitted_scale_120 = Some(s120);
                scale_state.last_emitted_buffer_scale = Some(b);
                fx.push(protocol::Effect::PreferredScale {
                    element,
                    scale_120: s120,
                    integer_scale: b,
                });
            }
            closed |= !scale_state.epoch.open;
        }
        self.scale_sweep.evaluated(full, closed, visit);
        fx
    }

    /// View(O): the presentation view of a mapped protocol-fullscreen element on the output
    /// (F-PRESENT-1), else the view of the camera bound to it (F-VIEW-1, F-LATTICE-1). The
    /// camera is not consulted, read or changed while a presentation view replaces it.
    pub fn view(&self, output: output::OutputId) -> Option<projection::View> {
        let model = self.outputs.iter().find(|o| o.id == output)?;
        if self.presentation(output).is_some()
            && !self.is_presentation_suppressed(output)
            && let Some(view) = self.presentation_view(model)
        {
            return Some(view);
        }
        self.camera_view(output)
    }

    /// The view of the camera bound to `output`, whether or not a presentation replaces it,
    /// with the output's kept lattice reference (F-LATTICE-1): the reference is kept while `k_O`
    /// and `view_O` are those it was set at and the bound holds, else set from the view origin.
    fn camera_view(&self, output: output::OutputId) -> Option<projection::View> {
        let model = self.outputs.iter().find(|o| o.id == output)?;
        let camera = self.cameras.camera_for_output(output)?;
        let params = projection::ViewParams {
            centre: self.cameras.center(camera)?,
            zoom: self.cameras.zoom(camera)?,
            scale: model.scale,
            view_size: model.view_size(),
        };
        let k = params.zoom * params.scale;
        let kept = self
            .lattice
            .iter()
            .find(|l| l.output == output && l.k == k && l.view_size == params.view_size)
            .map(|l| l.r);
        projection::View::with_kept_reference(params, kept).ok()
    }

    /// F-LATTICE-1's transitions for `output`: records the reference its camera view now uses,
    /// with the `k_O` and `view_O` it was set at. Called after every change of what that view is
    /// computed from — a connect or (re-)binding (no reference yet: set from the origin), a
    /// reconfiguration, and every camera write (a pan keeps the reference unless the bound is
    /// exceeded; a zoom change sets it again). Allocation-free once the output has a slot.
    pub(crate) fn update_lattice(&mut self, output: output::OutputId) {
        let Some(view) = self.camera_view(output) else {
            self.lattice.retain(|l| l.output != output);
            return;
        };
        let slot = LatticeSlot {
            output,
            r: view.r,
            k: view.k,
            view_size: view.size,
        };
        match self.lattice.iter_mut().find(|l| l.output == output) {
            Some(l) => *l = slot,
            None => self.lattice.push(slot),
        }
    }

    /// The output is gone: its reference goes with it (never persisted, OBJ-12).
    pub(crate) fn remove_lattice(&mut self, output: output::OutputId) {
        self.lattice.retain(|l| l.output != output);
    }

    /// One window alone, for capturing it (sharing a window): its shown surfaces under a view
    /// whose origin is the corner of its window geometry, at zoom 1 and output scale `scale`, the
    /// size of the window geometry. Nothing else is in it: no other window, no layer surface,
    /// no cursor. `None` for a window that is not mapped or has no size.
    pub fn window_frame(
        &self,
        element: stage::ElementId,
        scale: f64,
    ) -> Option<render::ProjectionFrame> {
        self.stage.mapped(element)?;
        let anchor = self.stage.anchor(element)?;
        let tree = self.stage.tree(element)?;
        let size = tree.geometry.size;
        if size.w <= 0 || size.h <= 0 {
            return None;
        }
        let view_size = (
            (f64::from(size.w) * scale).round() as u32,
            (f64::from(size.h) * scale).round() as u32,
        );
        let centre = geom::CanvasPos {
            x: anchor.x * geom::Q16_ONE + i64::from(size.w) * geom::Q16_ONE / 2,
            y: anchor.y * geom::Q16_ONE + i64::from(size.h) * geom::Q16_ONE / 2,
        };
        let view = projection::View::new(projection::ViewParams {
            centre,
            zoom: 1.0,
            scale,
            view_size,
        })
        .ok()?;
        let entries = render::entries_for(&view, element, anchor, tree);
        Some(render::ProjectionFrame { view, entries })
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
        let i = match self.frames.iter().position(|s| s.output == output) {
            Some(i) => i,
            None => {
                self.frames.push(FrameSlot {
                    output,
                    frame: render::ProjectionFrame {
                        view,
                        entries: Vec::new(),
                    },
                    current: false,
                });
                self.frames.len() - 1
            }
        };
        let stage = &self.stage;
        let slot = &mut self.frames[i];
        slot.frame.rebuild(view, stage);
        slot.current = true;
        Ok(&self.frames[i].frame)
    }

    /// F-HIT-1 against the last frame shown on the output (built on demand if none exists yet).
    /// An entry whose surface no longer exists fails closed: no delivery until the next frame.
    pub fn hit_test(
        &mut self,
        output: output::OutputId,
        p: geom::OutputLogicalPoint,
    ) -> Option<render::Hit> {
        if self.current_frame(output).is_none() {
            self.render_list(output).ok()?;
        }
        let frame = self.current_frame(output)?;
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
        // L3: overlay and top surfaces first, then the Canvas, then bottom and background. A layer
        // surface's events are in its own surface-local frame: origin 0, the location the position
        // in it, so no camera zoom ever scales them.
        let layer_target = |h: layer::LayerHit| {
            (
                Some(protocol::PointerTarget {
                    surface: h.surface,
                    origin: geom::CanvasPoint { x: 0, y: 0 },
                }),
                geom::CanvasPos {
                    x: h.local.x,
                    y: h.local.y,
                },
            )
        };
        if let Some(h) = self.layer_hit(output, p, true) {
            return Some(layer_target(h));
        }
        let hit = self.hit_test(output, pt);
        let view = self.current_frame(output)?.view;
        if hit.is_none()
            && let Some(h) = self.layer_hit(output, p, false)
        {
            return Some(layer_target(h));
        }
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

    /// One true-position event (M10 reconciliation): a routed motion and its frame, at the
    /// input clock's time (the event that caused the batch, or the latest one before it).
    pub(crate) fn true_position(&mut self) -> Vec<protocol::Effect> {
        let mut out = Vec::new();
        self.true_position_into(&mut out);
        out
    }

    /// [`Core::true_position`] appended to `out`.
    pub(crate) fn true_position_into(&mut self, out: &mut Vec<protocol::Effect>) {
        let time_ms = self.input.clock.wire_ms();
        if let Some((target, location)) = self.route_pointer() {
            out.extend([
                protocol::Effect::PointerMotion {
                    target,
                    location,
                    time_ms,
                },
                protocol::Effect::PointerFrame,
            ]);
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
            frame_requested: false,
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

    /// C1: a scale parameter outside its §PROV range is refused at the Core boundary and the
    /// configuration in force is kept; a valid one replaces it.
    #[test]
    fn scale_config_is_validated_at_the_core_boundary() {
        let (mut core, _o) = connected();
        let in_force = *core.scale_config();
        for bad in [
            scale::ScalePolicyConfig {
                h_cfg: f64::NAN,
                ..in_force
            },
            scale::ScalePolicyConfig { r: 1.0, ..in_force },
            scale::ScalePolicyConfig {
                sigma: 0.25,
                ..in_force
            },
            scale::ScalePolicyConfig {
                t_settle_ms: 0,
                ..in_force
            },
            scale::ScalePolicyConfig {
                d_max: 0,
                ..in_force
            },
        ] {
            assert!(core.set_scale_config(bad).is_err(), "{bad:?}");
            assert_eq!(*core.scale_config(), in_force, "kept after {bad:?}");
        }
        assert_eq!(
            core.set_renderer_device_max(0),
            Err(scale::ScaleConfigError::DeviceMax)
        );
        assert_eq!(*core.scale_config(), in_force);
        let tuned = scale::ScalePolicyConfig {
            h_cfg: 2.0,
            r: 1.5,
            sigma: 0.1,
            t_settle_ms: 250,
            d_max: 8192,
            sharpen_after: true,
        };
        assert_eq!(core.set_scale_config(tuned).map(|_| ()), Ok(()));
        assert_eq!(*core.scale_config(), tuned);
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
                frame_requested: false,
            })
            .collect();
        surfaces.push(SnapshotSurface {
            key: key(1),
            role: SurfaceRole::Root,
            size: LocalSize { w: 200, h: 100 },
            mapped: true,
            input: InputRegion::Infinite,
            frame_requested: false,
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

    /// Output-logical point at the centre of a 20x20 subsurface at `offset`. The 200x100 window
    /// is centred on the view centre (960,540) at scale 1, so its anchor (−100,−50) is at
    /// (860,490).
    fn over(offset: (i32, i32)) -> OutputLogicalPoint {
        OutputLogicalPoint {
            x: 860.0 + offset.0 as f64 + 10.5,
            y: 490.0 + offset.1 as f64 + 10.5,
        }
    }

    /// Shown root + unshown subsurface (INC1-F07). The unshown key is absent from
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

    /// Shown root + shown sibling + unshown sibling (INC1-F07).
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

    /// Nested subsurfaces (INC1-F07). A mapped child of an unmapped parent is not shown (Smithay
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

    /// A subsurface leaves the frame when it is unmapped and returns when remapped (INC1-F07).
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
        // the 200x100 window is centred: anchor (−100,−50) is at (860,490) and the subsurface at
        // (+10,+10) covers (870..890, 500..520)
        let p = OutputLogicalPoint { x: 875.5, y: 505.5 };
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

    // ---- Fit and bounded restore history (F-FIT-1, OBJ-30, M8; C12.F15, C12.F16)
    //
    // Every test drives the production entries: key presses through `physical_input` (Super+Enter
    // fit toggle), pans through it, outputs through the lifecycle entries. Camera values
    // are compared exactly (Q16 centre, zoom bits).

    use crate::navigation::{CameraValue, HISTORY_CAPACITY};

    struct Nav {
        core: Core,
        port: InputPort,
        shell: ShellPort,
        token: OutputLifecycleToken,
        t: u64,
    }

    fn desc_a(connector: &str, scale_120: u32) -> OutputDescriptor {
        OutputDescriptor {
            key: crate::output::tests::key(connector),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        }
    }

    impl Nav {
        /// Output A (1920 × 1080, scale 1), W (200 × 100) mapped and centred and a frame shown, the
        /// pointer at the view centre (startup rule), over W.
        fn new() -> (Nav, output::OutputId, stage::ElementId) {
            let (core, port, shell, token) =
                Core::new(crate::navigation::PersistedCameras::default());
            let mut n = Nav {
                core,
                port,
                shell,
                token,
                t: 0,
            };
            let o = n.connect("A");
            let w = mapped(&mut n.core, tree(1, None));
            // the frame shown after the map: hit testing reads what was shown (INV-HIT-1)
            n.core.render_list(o).unwrap();
            (n, o, w)
        }

        fn connect(&mut self, connector: &str) -> output::OutputId {
            self.core
                .output_connected(&mut self.token, desc_a(connector, 120))
                .unwrap()
                .0
        }

        fn ev(&mut self, ev: PhysicalEvent) -> Vec<protocol::Effect> {
            self.core.physical_input_vec(&mut self.port, ev).unwrap()
        }

        /// Super+Enter (the fit toggle; Super+F was removed by the owner, 2026-10-21): it fits,
        /// and restores when the camera holds that fit.
        fn key(&mut self) -> Vec<protocol::Effect> {
            self.t += 1000;
            let modifiers = input::bindings::Modifiers {
                logo: true,
                ..Default::default()
            };
            let mut fx = self.ev(PhysicalEvent::KeyboardKey {
                key: input::keys::KEY_ENTER,
                pressed: true,
                modifiers,
                mods_changed: false,
                time_us: self.t,
            });
            self.t += 1000;
            fx.extend(self.ev(PhysicalEvent::KeyboardKey {
                key: input::keys::KEY_ENTER,
                pressed: false,
                modifiers,
                mods_changed: false,
                time_us: self.t,
            }));
            fx
        }

        /// M8 `FitViewport(O, W)` for the element under the pointer on the pointer's output, through
        /// Core's shell entry (the same F-FIT-1 path as the key; the key itself, Super+Enter, is a
        /// toggle and is tested by `key`). Nothing when no element is under the pointer or Core
        /// refuses.
        fn fit(&mut self) -> Vec<protocol::Effect> {
            let pointer::PointerState::Located { output, p } = self.core.pointer() else {
                return Vec::new();
            };
            let pt = geom::OutputLogicalPoint { x: p.0, y: p.1 };
            let Some(element) = self.core.hit_test(output, pt).map(|h| h.element) else {
                return Vec::new();
            };
            let element = stage::ElementRef {
                instance: self.core.stage().instance(),
                element,
            };
            self.core
                .shell_command(
                    &mut self.shell,
                    ShellCommand::ViewportFit { output, element },
                )
                .unwrap_or_default()
        }

        /// M8 `ViewportRestore(O)` on the pointer's output, through Core's shell entry.
        fn restore(&mut self) -> Vec<protocol::Effect> {
            let Some(output) = self.core.pointer_output() else {
                return Vec::new();
            };
            self.core
                .shell_command(&mut self.shell, ShellCommand::ViewportRestore { output })
                .unwrap_or_default()
        }

        fn motion(&mut self, d: (f64, f64)) {
            self.t += 1000;
            self.ev(PhysicalEvent::PointerMotion {
                delta: d,
                delta_unaccel: d,
                time_us: self.t,
            });
        }

        /// A camera pan by `d` logical px (Logo + middle button), which moves the camera only.
        fn pan(&mut self, d: (f64, f64)) {
            let logo = input::bindings::Modifiers {
                logo: true,
                ..Default::default()
            };
            for pressed in [true, false] {
                self.t += 1000;
                self.ev(PhysicalEvent::PointerButton {
                    button: input::bindings::BTN_MIDDLE,
                    pressed,
                    modifiers: logo,
                    time_us: self.t,
                });
                if pressed {
                    self.motion(d);
                }
            }
        }

        fn camera(&self, o: output::OutputId) -> CameraValue {
            let c = self.core.cameras().camera_for_output(o).unwrap();
            CameraValue {
                center: self.core.cameras().center(c).unwrap(),
                zoom: self.core.cameras().zoom(c).unwrap(),
            }
        }

        fn history(&self, o: output::OutputId) -> Vec<CameraValue> {
            let c = self.core.cameras().camera_for_output(o).unwrap();
            self.core.cameras().history(c).unwrap().values().collect()
        }
    }

    /// Exact camera-value equality: the Q16 centre and the zoom's bits.
    fn same(a: CameraValue, b: CameraValue) -> bool {
        a.center == b.center && a.zoom.to_bits() == b.zoom.to_bits()
    }

    fn motions(fx: &[protocol::Effect]) -> usize {
        fx.iter()
            .filter(|e| matches!(e, protocol::Effect::PointerMotion { .. }))
            .count()
    }

    /// F-FIT-1 on the production path: W's window geometry (−100, −50, 200, 100) on a 1920 × 1080
    /// view at scale 1 gives zoom min(9.6, 10.8) = 9.6 and centre (0, 0); one mutation; the prior
    /// value is the history's only entry; W's anchor and size are unchanged (INV-CAMERA-5); the
    /// camera is accepted by `View::new`; and exactly one true-position event follows (M10: the
    /// pointer is on A, no interaction).
    #[test]
    fn a_fit_records_the_prior_value_and_restore_returns_it_exactly() {
        let (mut n, a, w) = Nav::new();
        n.pan((37.3, -11.9)); // a prior value that is not the default
        let before = n.camera(a);
        let (anchor, size) = (
            n.core.stage().anchor(w),
            n.core.stage().tree(w).unwrap().geometry,
        );
        let fx = n.fit();
        assert_eq!(n.camera(a).zoom, 9.6);
        assert_eq!(n.camera(a).center, geom::CanvasPos { x: 0, y: 0 });
        assert!(n.history(a).len() == 1 && same(n.history(a)[0], before));
        assert_eq!(motions(&fx), 1, "M10: one true-position event after a fit");
        assert_eq!(n.core.stage().anchor(w), anchor);
        assert_eq!(n.core.stage().tree(w).unwrap().geometry, size);
        assert!(
            n.core.view(a).is_some(),
            "View::new accepts the fitted camera"
        );
        let fx = n.restore();
        assert!(same(n.camera(a), before), "restore is exact");
        assert!(n.history(a).is_empty(), "a restore records nothing");
        assert_eq!(
            motions(&fx),
            1,
            "M10: one true-position event after a restore"
        );
        assert!(n.core.view(a).is_some());
        // empty: refused, nothing changes
        let fx = n.restore();
        assert!(same(n.camera(a), before) && n.history(a).is_empty());
        assert_eq!(motions(&fx), 0, "a refused restore writes nothing");
    }

    /// Restore applies the stored zoom verbatim (C12.F16.3 item 7): at scale 1.25 the fit's zoom is
    /// 9.6 / 1.25 = 7.68, not a multiple of 1/120; after a second fit from it, the restore returns it
    /// bit for bit (no re-quantisation, no clamp, no recomputation).
    #[test]
    fn restore_is_exact_for_a_zoom_off_the_lattice() {
        let (mut n, a, _) = Nav::new();
        n.core
            .output_reconfigured(&mut n.token, a, desc_a("A", 150))
            .unwrap();
        n.core.render_list(a).unwrap();
        // the view centre is now logical (768, 432): move the pointer there, onto W
        n.motion((768.0 - 960.0, 432.0 - 540.0));
        n.fit();
        let first = n.camera(a);
        assert_eq!(first.zoom, 9.6 / 1.25);
        assert_ne!((first.zoom * 120.0).fract(), 0.0, "off the 1/120 lattice");
        n.pan((3.3, 1.7));
        let prior = n.camera(a);
        n.fit();
        n.restore();
        assert!(same(n.camera(a), prior), "{:?} vs {prior:?}", n.camera(a));
        n.restore();
        assert!(same(
            n.camera(a),
            CameraValue {
                center: geom::CanvasPos { x: 0, y: 0 },
                zoom: 1.0
            }
        ));
    }

    /// A fit and a restore are camera writes: membership follows them (F-MEMBER-1). U, 700 px right of
    /// W, is on A at zoom 1; the fit to W (zoom 9.6, a 200 px wide Canvas view) leaves it off A, and
    /// the restore brings it back; W stays on A throughout.
    #[test]
    fn a_fit_and_a_restore_update_membership() {
        let (mut n, a, w) = Nav::new();
        n.pan((-700.0, 0.0)); // C = start − Σ: the camera moves to x = +700
        let u = mapped(&mut n.core, tree(9, None)); // placed at the view centre
        n.pan((700.0, 0.0)); // back on W
        n.core.render_list(a).unwrap();
        let on = |n: &Nav, e| n.core.membership().observers(e).contains(&a);
        assert!(on(&n, w) && on(&n, u), "both on A at zoom 1");
        n.fit();
        assert_eq!(n.camera(a).zoom, 9.6);
        assert!(on(&n, w) && !on(&n, u), "the fit: U left A");
        n.restore();
        assert!(on(&n, w) && on(&n, u), "the restore: U back on A");
    }

    /// During an interaction the bindings do not resolve: with a pan held, Super+Enter neither
    /// fits nor records nor restores, and sends no true-position event; the pan alone moves
    /// the camera.
    #[test]
    fn no_fit_or_restore_during_an_interaction() {
        let (mut n, a, _) = Nav::new();
        n.fit();
        assert_eq!(n.history(a).len(), 1);
        let logo = input::bindings::Modifiers {
            logo: true,
            ..Default::default()
        };
        n.t += 1000;
        n.ev(PhysicalEvent::PointerButton {
            button: input::bindings::BTN_MIDDLE,
            pressed: true,
            modifiers: logo,
            time_us: n.t,
        });
        n.motion((5.0, 0.0));
        let during = n.camera(a);
        for _ in 0..2 {
            let fx = n.key();
            assert!(same(n.camera(a), during), "a camera action inside the pan");
            assert_eq!(n.history(a).len(), 1);
            assert_eq!(motions(&fx), 0);
        }
    }

    /// An empty history refuses a restore without mutating the camera or the history (C12.F16.3
    /// item 4); a following fit and restore still work.
    #[test]
    fn restore_with_an_empty_history_is_refused() {
        let (mut n, a, _) = Nav::new();
        let v0 = n.camera(a);
        assert!(n.history(a).is_empty());
        let fx = n.restore();
        assert!(same(n.camera(a), v0) && n.history(a).is_empty() && motions(&fx) == 0);
        n.fit();
        n.restore();
        assert!(same(n.camera(a), v0));
    }

    /// Records `count` fits, each from a distinct prior value (a pan before every fit); returns the
    /// prior values in order.
    fn fits(n: &mut Nav, a: output::OutputId, count: usize) -> Vec<CameraValue> {
        let mut priors = Vec::new();
        for i in 0..count {
            n.pan(((i % 7) as f64 * 0.37 + 0.11, ((i % 5) as f64 - 2.0) * 0.23));
            priors.push(n.camera(a));
            let before = n.history(a).len();
            n.fit();
            assert_eq!(
                n.history(a).len(),
                (before + 1).min(HISTORY_CAPACITY),
                "fit {i} was refused"
            );
        }
        priors
    }

    /// Repeated restores unwind in reverse chronological order (M8, C12.F16.6 item 3).
    #[test]
    fn restores_unwind_consecutive_fits_newest_first() {
        let (mut n, a, _) = Nav::new();
        let priors = fits(&mut n, a, 5);
        for i in 1..5 {
            assert!(!same(priors[i], priors[i - 1]), "distinct prior values");
        }
        for want in priors.iter().rev() {
            n.restore();
            assert!(same(n.camera(a), *want));
        }
        let last = n.camera(a);
        n.restore();
        assert!(
            same(n.camera(a), last) && n.history(a).is_empty(),
            "a sixth is refused"
        );
    }

    /// Exactly 256 retained entries; 256 restores return them newest first; the 257th is refused
    /// (C12.F16.6 item 4).
    #[test]
    fn the_history_holds_exactly_256_entries() {
        let (mut n, a, _) = Nav::new();
        let priors = fits(&mut n, a, HISTORY_CAPACITY);
        let held = n.history(a);
        assert_eq!(held.len(), 256);
        assert!(held.iter().zip(&priors).all(|(h, p)| same(*h, *p)));
        for want in priors.iter().rev() {
            n.restore();
            assert!(same(n.camera(a), *want));
        }
        let at = n.camera(a);
        assert_eq!(motions(&n.restore()), 0);
        assert!(same(n.camera(a), at) && n.history(a).is_empty());
    }

    /// The 257th successful fit evicts only the oldest entry; restores then return the values before
    /// fits 257 … 2 and refuse: the value before fit 1 is lost, as C12.F16 decides (item 5).
    #[test]
    fn the_257th_fit_evicts_only_the_oldest_entry() {
        let (mut n, a, _) = Nav::new();
        let priors = fits(&mut n, a, HISTORY_CAPACITY + 1);
        let held = n.history(a);
        assert_eq!(held.len(), 256);
        assert!(
            held.iter().zip(&priors[1..]).all(|(h, p)| same(*h, *p)),
            "the history is the priors of fits 2 … 257, oldest first"
        );
        for want in priors[1..].iter().rev() {
            n.restore();
            assert!(same(n.camera(a), *want));
        }
        assert!(same(n.camera(a), priors[1]));
        n.restore();
        assert!(
            same(n.camera(a), priors[1]) && !same(n.camera(a), priors[0]),
            "the oldest value is unrecoverable"
        );
    }

    /// Refused fits — no output under the pointer, no element under it, a degenerate window
    /// geometry — change neither the camera nor the history, at 0, 1 and 256 entries; nothing is
    /// evicted at capacity (C12.F16.6 item 6).
    #[test]
    fn refused_fits_change_neither_camera_nor_history() {
        for prefill in [0usize, 1, HISTORY_CAPACITY] {
            let (mut n, a, _) = Nav::new();
            fits(&mut n, a, prefill);
            let (cam, held) = (n.camera(a), n.history(a));
            let unchanged = |n: &Nav, what: &str| {
                assert!(same(n.camera(a), cam), "{prefill}/{what}: camera changed");
                let now = n.history(a);
                assert!(
                    now.len() == held.len() && now.iter().zip(&held).all(|(x, y)| same(*x, *y)),
                    "{prefill}/{what}: history changed"
                );
            };
            // no element under the pointer: move to the view's top-left corner, off W at any of
            // these cameras (W is centred and at most 1920 × 960 px)
            let p = match n.core.pointer() {
                pointer::PointerState::Located { p, .. } => p,
                _ => unreachable!(),
            };
            n.motion((-p.0, -p.1));
            assert!(
                n.core
                    .hit_test(a, OutputLogicalPoint { x: 0.5, y: 0.5 })
                    .is_none()
            );
            assert_eq!(motions(&n.fit()), 0);
            unchanged(&n, "no element");
            n.motion((p.0, p.1));
            // a degenerate window geometry (the same surfaces, geometry 0 × 100)
            let w = n.core.stage().z_order()[0];
            let mut t = n.core.stage().tree(w).unwrap().clone();
            let keep = t.clone();
            t.geometry.size.w = 0;
            n.core.toplevel_committed(w, t).unwrap();
            n.fit();
            unchanged(&n, "zero width");
            n.core.toplevel_committed(w, keep).unwrap();
        }
        // no output: the pointer is on none
        let (mut n, a, _) = Nav::new();
        fits(&mut n, a, 3);
        n.core.output_disconnected(&mut n.token, a).unwrap();
        assert_eq!(n.core.pointer(), pointer::PointerState::NoOutput);
        let cams = n.core.cameras();
        let values: Vec<_> = (0..cams.len())
            .map(|c| (cams.center(c), cams.zoom(c).map(f64::to_bits)))
            .collect();
        let fx = n.fit();
        assert_eq!(motions(&fx), 0);
        let cams = n.core.cameras();
        for (c, v) in values.iter().enumerate() {
            assert_eq!(
                (cams.center(c), cams.zoom(c).map(f64::to_bits)),
                *v,
                "no output: refused"
            );
            assert!(cams.history(c).unwrap().is_empty());
        }
    }

    /// A second element V (120 × 40, root surface `root`), mapped after W.
    fn map_small(n: &mut Nav, root: u64) -> stage::ElementId {
        let mut t = tree(root, None);
        t.geometry.size = LocalSize { w: 120, h: 40 };
        t.surfaces
            .iter_mut()
            .for_each(|s| s.size = LocalSize { w: 120, h: 40 });
        mapped(&mut n.core, t)
    }

    /// Moves the pointer onto `e` on `o`, at the first point of a 4 px grid of the shown frame
    /// where `e` is hit (the frame is produced first: hit testing reads what was shown).
    fn point_at(n: &mut Nav, o: output::OutputId, e: stage::ElementId) {
        n.core.render_list(o).unwrap();
        let target = (0..270)
            .flat_map(|j| (0..480).map(move |i| (i as f64 * 4.0 + 1.5, j as f64 * 4.0 + 1.5)))
            .find(|&(x, y)| {
                n.core
                    .hit_test(o, OutputLogicalPoint { x, y })
                    .is_some_and(|h| h.element == e)
            })
            .expect("the element is visible somewhere on the output");
        let p = match n.core.pointer() {
            pointer::PointerState::Located { p, .. } => p,
            _ => unreachable!(),
        };
        n.motion((target.0 - p.0, target.1 - p.1));
        n.core.render_list(o).unwrap();
    }

    /// C12.F16.6 item 3 as worded: fits from distinct values to two different elements at
    /// different zooms (W 200 × 100 → 9.6, V 120 × 40 → 16), a pan before each, then five
    /// restores return v₄ … v₀ in order and a sixth is refused.
    #[test]
    fn restores_unwind_fits_of_two_elements_at_different_zooms() {
        let (mut n, a, w) = Nav::new();
        let v = map_small(&mut n, 7);
        let mut priors = Vec::new();
        let mut zooms = Vec::new();
        for i in 0..5 {
            n.pan((0.5 + i as f64 * 0.3, -0.25 * i as f64));
            let e = if i % 2 == 0 { v } else { w };
            point_at(&mut n, a, e);
            priors.push(n.camera(a));
            n.fit();
            assert_eq!(n.history(a).len(), i + 1, "fit {i} refused");
            zooms.push(n.camera(a).zoom);
        }
        assert_eq!(
            zooms,
            vec![16.0, 9.6, 16.0, 9.6, 16.0],
            "both elements fitted"
        );
        for i in 0..5 {
            for j in 0..i {
                assert!(!same(priors[i], priors[j]), "distinct prior values");
            }
        }
        assert!(
            priors.iter().any(|p| p.zoom != priors[0].zoom),
            "different zooms"
        );
        for want in priors.iter().rev() {
            n.restore();
            assert!(same(n.camera(a), *want));
        }
        let last = n.camera(a);
        assert_eq!(motions(&n.restore()), 0);
        assert!(
            same(n.camera(a), last) && n.history(a).is_empty(),
            "a sixth is refused"
        );
    }

    /// C12.F16.6 item 6, the cases `refused_fits_change_neither_camera_nor_history` does not take:
    /// W unmapped while the shown frame still hits it, and a zero-height W, at 0, 1 and 256
    /// entries; and O not connected at 0, 1 and 256 entries.
    #[test]
    fn refused_fits_of_an_unmapped_or_zero_height_window_and_without_an_output() {
        for prefill in [0usize, 1, HISTORY_CAPACITY] {
            let (mut n, a, w) = Nav::new();
            fits(&mut n, a, prefill);
            n.core.render_list(a).unwrap();
            let (cam, held) = (n.camera(a), n.history(a));
            let unchanged = |n: &Nav, what: &str| {
                assert!(same(n.camera(a), cam), "{prefill}/{what}: camera changed");
                let now = n.history(a);
                assert!(
                    now.len() == held.len() && now.iter().zip(&held).all(|(x, y)| same(*x, *y)),
                    "{prefill}/{what}: history changed"
                );
            };
            let keep = n.core.stage().tree(w).unwrap().clone();
            // zero height (the same surfaces, geometry 200 × 0)
            let mut t = keep.clone();
            t.geometry.size.h = 0;
            n.core.toplevel_committed(w, t).unwrap();
            assert_eq!(motions(&n.fit()), 0);
            unchanged(&n, "zero height");
            n.core.toplevel_committed(w, keep.clone()).unwrap();
            n.core.render_list(a).unwrap();
            // unmapped, with no frame produced after the unmap
            let mut gone = keep.clone();
            gone.surfaces.iter_mut().for_each(|s| s.mapped = false);
            n.core.toplevel_committed(w, gone).unwrap();
            assert!(n.core.stage().mapped(w).is_none());
            let p = match n.core.pointer() {
                pointer::PointerState::Located { p, .. } => p,
                _ => unreachable!(),
            };
            assert!(
                n.core
                    .hit_test(a, OutputLogicalPoint { x: p.0, y: p.1 })
                    .is_none(),
                "an unmapped W is not hit, even in the frame shown before the unmap"
            );
            assert_eq!(motions(&n.fit()), 0);
            unchanged(&n, "unmapped");
        }
        for prefill in [0usize, 1, HISTORY_CAPACITY] {
            let (mut n, a, _) = Nav::new();
            fits(&mut n, a, prefill);
            let c = n.core.cameras().camera_for_output(a).unwrap();
            let cam = n.camera(a);
            n.core.output_disconnected(&mut n.token, a).unwrap();
            assert_eq!(n.core.pointer(), pointer::PointerState::NoOutput);
            assert_eq!(motions(&n.fit()), 0);
            let cams = n.core.cameras();
            assert!(
                cams.center(c) == Some(cam.center)
                    && cams.zoom(c).map(f64::to_bits) == Some(cam.zoom.to_bits())
                    && cams.history(c).unwrap().is_empty(),
                "{prefill}/no output: refused"
            );
        }
    }

    /// C12.F16.6 item 7: the binding restores on the pointer's output, so a restore after
    /// O's disconnection reaches no output: nothing changes, and after O reconnects a fit and a
    /// restore behave as items 2–3.
    #[test]
    fn a_restore_after_the_output_is_disconnected_changes_nothing() {
        let (mut n, a, _) = Nav::new();
        let priors = fits(&mut n, a, 2);
        let c = n.core.cameras().camera_for_output(a).unwrap();
        let cam = n.camera(a);
        n.core.output_disconnected(&mut n.token, a).unwrap();
        assert_eq!(motions(&n.restore()), 0);
        let cams = n.core.cameras();
        assert!(
            cams.center(c) == Some(cam.center)
                && cams.zoom(c).map(f64::to_bits) == Some(cam.zoom.to_bits()),
            "the camera keeps its exact value"
        );
        // emptied by the unbinding itself (C12.F16.3), not by the refused restore
        assert!(cams.history(c).unwrap().is_empty());
        assert!(!same(cam, priors[1]));
        let a2 = n.connect("A");
        n.core.render_list(a2).unwrap();
        let v0 = n.camera(a2);
        n.fit();
        assert_eq!(n.history(a2).len(), 1);
        n.restore();
        assert!(same(n.camera(a2), v0) && n.history(a2).is_empty());
    }

    /// Two outputs, each its own camera and history: interleaved fits restore only their own
    /// output's values; exhausting one leaves the other intact (C12.F16.6 item 8).
    #[test]
    fn two_outputs_keep_separate_histories() {
        let (mut n, a, _) = Nav::new();
        let b = n.connect("B");
        // B's camera starts at the default, so W is under the pointer at B's centre too
        let to_b = |n: &mut Nav, on_b: bool| {
            let p = match n.core.pointer() {
                pointer::PointerState::Located { p, output } => (p, output),
                _ => unreachable!(),
            };
            let target = if on_b { 1920.0 + 960.0 } else { 960.0 };
            let now = if p.1 == a { p.0.0 } else { 1920.0 + p.0.0 };
            n.motion((target - now, 540.0 - p.0.1));
        };
        let mut pa = Vec::new();
        let mut pb = Vec::new();
        for i in 0..4 {
            to_b(&mut n, false);
            n.pan((1.0 + i as f64, 0.5));
            pa.push(n.camera(a));
            n.fit();
            to_b(&mut n, true);
            n.pan((-2.0 - i as f64, 0.25));
            pb.push(n.camera(b));
            n.fit();
        }
        assert_eq!((n.history(a).len(), n.history(b).len()), (4, 4));
        for want in pb.iter().rev() {
            n.restore();
            assert!(same(n.camera(b), *want));
        }
        n.restore(); // refused on B
        assert!(same(n.camera(b), pb[0]));
        assert!(
            n.history(a).iter().zip(&pa).all(|(h, p)| same(*h, *p)) && n.history(a).len() == 4,
            "A's history untouched by B's restores"
        );
        to_b(&mut n, false);
        for want in pa.iter().rev() {
            n.restore();
            assert!(same(n.camera(a), *want));
        }
    }

    /// Output lifecycle (C12.F16.3 items 5–6, C12.F16.6 item 9): a reconfiguration keeps the
    /// history and a restore after it is exact; a disconnect empties it with the unbinding (the
    /// camera's value kept); the reconnected output — a new `OutputId`, the same remembered camera —
    /// starts empty, so no entry of the earlier connection is ever applied.
    #[test]
    fn disconnect_empties_the_history_and_reconfiguration_keeps_it() {
        let (mut n, a, _) = Nav::new();
        let priors = fits(&mut n, a, 3);
        n.core
            .output_reconfigured(&mut n.token, a, desc_a("A", 150))
            .unwrap();
        assert_eq!(n.history(a).len(), 3, "reconfiguration keeps the history");
        n.restore();
        assert!(
            same(n.camera(a), priors[2]),
            "restore after a reconfiguration is exact"
        );
        assert!(n.core.view(a).is_some());
        let camera = n.core.cameras().camera_for_output(a).unwrap();
        let kept = n.camera(a);
        n.core.output_disconnected(&mut n.token, a).unwrap();
        let h = n.core.cameras().history(camera).unwrap();
        assert!(
            h.is_empty() && h.reserved() == 0,
            "emptied, storage released"
        );
        let a2 = n.connect("A");
        assert_ne!(a2, a);
        assert_eq!(
            n.core.cameras().camera_for_output(a2),
            Some(camera),
            "re-bound"
        );
        assert!(same(n.camera(a2), kept), "the camera keeps its value");
        assert!(n.history(a2).is_empty());
        let fx = n.restore();
        assert!(
            same(n.camera(a2), kept) && motions(&fx) == 0,
            "refused: nothing to apply"
        );
    }

    /// Bounded storage (C12.F16.6 item 10): 100,000 fits alternating two elements (restores
    /// interleaved) never hold more than 256 entries, and the storage, reserved at the binding's
    /// first fit, never changes.
    #[test]
    fn a_hundred_thousand_fits_keep_the_history_bounded() {
        let (mut n, a, w) = Nav::new();
        let v = map_small(&mut n, 7);
        // a point on each element under each fitted camera: fitting W shows V too (both centred),
        // and fitting V leaves W visible around it
        point_at(&mut n, a, w);
        let c = n.core.cameras().camera_for_output(a).unwrap();
        assert_eq!(n.core.cameras().history(c).unwrap().reserved(), 0);
        let mut spots = [None, None];
        for i in 0..100_000u32 {
            let k = (i % 2) as usize;
            let e = [w, v][k];
            match spots[k] {
                Some((x, y))
                    if n.core
                        .hit_test(a, OutputLogicalPoint { x, y })
                        .is_some_and(|h| h.element == e) =>
                {
                    let p = match n.core.pointer() {
                        pointer::PointerState::Located { p, .. } => p,
                        _ => unreachable!(),
                    };
                    n.motion((x - p.0, y - p.1));
                }
                _ => {
                    point_at(&mut n, a, e);
                    spots[k] = match n.core.pointer() {
                        pointer::PointerState::Located { p, .. } => Some(p),
                        _ => None,
                    };
                }
            }
            n.fit();
            assert_eq!(
                n.camera(a).zoom,
                [9.6, 16.0][k],
                "fit {i} fitted the other element"
            );
            if i % 97 == 0 {
                n.restore();
            }
            let h = n.core.cameras().history(c).unwrap();
            assert!(h.len() <= HISTORY_CAPACITY);
            assert_eq!(h.reserved(), HISTORY_CAPACITY);
        }
        assert_eq!(n.history(a).len(), HISTORY_CAPACITY);
    }

    /// INV-SCALE-1 / F-SCALE-1: compute_rho respects presentation suppression. When presentation
    /// is suppressed, the output observes through its camera view at its real zoom, not zoom 1.
    #[test]
    fn compute_rho_and_scale_policy_respect_presentation_suppression() {
        use crate::navigation::PersistedCameras;
        use crate::protocol::ToplevelRequest;

        let persisted = PersistedCameras::from_entries(vec![(
            crate::output::tests::key("A"),
            crate::geom::CanvasPos { x: 0, y: 0 },
            2.0, // camera zoom 2
        )])
        .unwrap();

        let (mut core, mut i, _s, mut token) = Core::new(persisted);
        let o = core
            .output_connected(
                &mut token,
                OutputDescriptor {
                    key: crate::output::tests::key("A"),
                    mode: (1920, 1080),
                    transform: Transform::Normal,
                    scale_120: 120, // scale 1.0
                    refresh_mhz: 60_000,
                },
            )
            .unwrap()
            .0;

        let w = mapped(&mut core, tree(1, None));
        core.set_focus(&mut i, Some(w)).unwrap();

        // Grant fullscreen on output O
        core.toplevel_request(w, ToplevelRequest::SetFullscreen { output: Some(o) })
            .unwrap();
        assert_eq!(core.presentation(o), Some(w));

        // When presenting and NOT suppressed, compute_rho sees zoom 1 -> rho = 1.0
        assert_eq!(core.compute_rho(w), Some(1.0));
        assert_eq!(core.preferred_scale_of(w), Some((120, 1)));

        // Suppress presentation: output observes through camera view (zoom 2, scale 1 -> rho = 2.0)
        core.presentation_suppressed.push(o);
        assert_eq!(core.compute_rho(w), Some(2.0));

        let _ = core.evaluate_scale_policy(false);
        assert_eq!(
            core.preferred_scale_of(w),
            Some((240, 2)),
            "observed window gets preferred scale 2 at camera zoom 2 when suppressed"
        );
    }
}
