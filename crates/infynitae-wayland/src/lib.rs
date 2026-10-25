//! Infynitae Native Smithay adapter (FOUNDATION_ARCHITECTURE C1.2).
//!
//! Bridges Wayland protocols (compositor, xdg-shell, layer-shell, outputs, seat, presentation-time,
//! fractional scale, activation, and data control) to Core. Surface commits extract snapshots for
//! Core, Core effects drive protocol responses in order, and render elements from Core render lists
//! are presented through damage trackers.
//!
//! Every Smithay handler receives `&mut` protocol state. That state holds the Core but never an
//! authority port or the output-lifecycle token: those types are `!Send` and the state is asserted
//! `Send` below, so storing one here is a compile error (C7.4, E0277). tools/verify-architecture
//! also rejects any mention of the authority entry points in this crate's sources.

#![deny(unsafe_code)]

pub mod convert;
pub mod cursor;
pub mod extension;
mod frames;
mod handlers;
mod image_copy;
mod keyboard;
mod launch;
mod layers;
pub mod limits;
mod lock;
mod outputs;
pub mod ownership;
mod pointer;
pub mod registry;
pub mod screencopy;
pub mod session;
mod signals;
mod snapshot;
pub mod xwayland;

pub use handlers::ACTIVATION_TOKENS_KEPT;
pub use keyboard::PreemptionStrategy;

use infynitae_core::Core;
use infynitae_core::stage::ElementId;
use smithay::desktop::{PopupManager, Window};
use smithay::input::{Seat, SeatState};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, GlobalId, ObjectId,
};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::dmabuf::{DmabufFeedbackBuilder, DmabufState};
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::pointer_constraints::PointerConstraintsState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::selection::primary_selection::PrimarySelectionState;
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::xdg_activation::XdgActivationState;

/// `clk_id` announced by `wp_presentation`: CLOCK_MONOTONIC on Linux. Frame times handed to
/// `complete_frames` are on this clock.
const CLOCK_MONOTONIC: u32 = 1;

/// The current time on the clock `wp_presentation` announces. A backend that presents in real
/// time takes the frame time it hands to `complete_frames` from here and from nowhere else, so
/// the timestamps clients receive are on the clock they were told (a lockstep test supplies
/// virtual frame times instead).
pub fn presentation_clock_now() -> std::time::Duration {
    smithay::utils::Clock::<smithay::utils::Monotonic>::new()
        .now()
        .into()
}

pub use registry::SurfaceRegistry;

/// Per-client data the composition root attaches when inserting a client.
#[derive(Debug, Default)]
pub struct ClientState {
    compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

/// An element's protocol handle (C2.2: handles stay in the adapter).
struct ElementHandle {
    id: ElementId,
    window: Window,
}

/// Configuration for input devices (pointers, mice, touchpads) loaded from ~/.config/infynitae/config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputDeviceConfig {
    pub mouse_accel_profile: Option<String>,
    pub mouse_accel_speed: Option<f64>,
    pub touchpad_tap: Option<bool>,
    pub touchpad_natural_scroll: Option<bool>,
}

/// Protocol state handed to every Smithay handler.
pub struct WaylandState {
    core: Core,
    dh: DisplayHandle,
    compositor: CompositorState,
    #[expect(
        dead_code,
        reason = "owns the xdg-output manager global for the state's lifetime"
    )]
    output_manager: OutputManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_presentation global for the state's lifetime"
    )]
    presentation: PresentationState,
    outputs: Vec<outputs::OutputHandle>,
    pub(crate) disabled_outputs: Vec<(GlobalId, std::time::Instant)>,
    /// The compositor started a process of its own (a shortcut, a launch, the wallpaper): the
    /// session reaps exits from then on (SIGCHLD), also without an `-e` command.
    pub(crate) spawned_children: bool,
    /// What the backend's framebuffer is (`set_host_window_target`).
    pub(crate) target: convert::RenderTarget,
    /// The cursor clients asked for, drawn when the target draws one (`cursor`).
    cursor: smithay::input::pointer::CursorImageStatus,
    /// The focus (surface and origin) and position of the last `wl_pointer.motion` delivered,
    /// and whether pointer events wait for their `wl_pointer.frame` (`pointer`).
    pointer_sent: pointer::Sent,
    arrow: cursor::Arrow,
    pub(crate) cursor_theme: std::sync::Mutex<Option<cursor::CursorThemeManager>>,
    pub(crate) config_home: Option<std::path::PathBuf>,
    pub(crate) xcursor_path: Option<String>,
    xdg_shell: XdgShellState,
    shm: ShmState,
    popups: PopupManager,
    /// The number of xdg popups created so far: each popup's creation number orders the
    /// popups of its toplevel in the snapshot (xdg-shell stacking; see `snapshot`).
    popups_created: u64,
    seats: SeatState<WaylandState>,
    /// The single seat, with pointer and keyboard.
    seat: Seat<WaylandState>,
    pub(crate) popup_grab: Option<smithay::desktop::PopupGrab<WaylandState>>,
    pub(crate) nested_grab_keep_pointer: bool,
    pub(crate) ordered_preemption_strategy: keyboard::PreemptionStrategy,
    #[expect(
        dead_code,
        reason = "owns the zwp_pointer_constraints_v1 global for the state's lifetime"
    )]
    pointer_constraints: PointerConstraintsState,
    #[expect(
        dead_code,
        reason = "owns the zwp_relative_pointer_manager_v1 global for the state's lifetime"
    )]
    relative_pointer: RelativePointerManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_fractional_scale_manager_v1 global for the state's lifetime"
    )]
    fractional_scale_manager: FractionalScaleManagerState,
    #[expect(
        dead_code,
        reason = "owns the wp_viewporter global for the state's lifetime"
    )]
    viewporter: ViewporterState,
    /// `wp_cursor_shape_v1`: a client names a cursor shape; the compositor draws it from the
    /// user's cursor theme (`cursor`), the same as a named cursor.
    #[expect(
        dead_code,
        reason = "owns the wp_cursor_shape_manager_v1 global for the state's lifetime"
    )]
    cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState,
    /// `wl_data_device_manager`: clipboard selection and drag-and-drop offers (2026-10-16).
    /// Selections are Smithay protocol objects; who sees and sets them follows Core's
    /// keyboard focus (`SeatHandler::focus_changed`).
    data_device: DataDeviceState,
    /// `zwp_primary_selection_device_manager_v1`: the primary selection, same focus rule.
    primary_selection: PrimarySelectionState,
    /// `zwlr_data_control_manager_v1` and `ext_data_control_manager_v1` (2026-10-25): clipboard
    /// tools (wl-copy, clipboard managers) set and read selections without a window of their own.
    /// Without them wl-copy mapped a window on every copy, which the open-focus rule could take
    /// for one the user opened (the camera jumped to it on every text selection).
    pub(crate) wlr_data_control: smithay::wayland::selection::wlr_data_control::DataControlState,
    pub(crate) ext_data_control: smithay::wayland::selection::ext_data_control::DataControlState,
    /// The client that set the clipboard and the primary selection (Smithay accepts a selection
    /// only from the keyboard-focused client). A selection whose owner's connection is gone is
    /// cleared, so other clients are told it is gone (Smithay does not do it by itself).
    selection_owners: [Option<ClientId>; 2],
    selection_sources: [Option<ObjectId>; 2],
    pub(crate) data_control_owner: Option<(
        smithay::wayland::selection::SelectionTarget,
        Option<ClientId>,
        Option<ObjectId>,
    )>,
    /// A client started a drag (Smithay installed its DnD grab); `refuse_client_drags` ends it.
    drag_started: bool,
    /// `xdg_activation_v1` (B10.7, INV-INPUT-9, W-SM-8).
    activation: XdgActivationState,
    /// `zwlr_layer_shell_v1` (L1–L8) and its surfaces, in creation order.
    layer_shell: WlrLayerShellState,
    layers: Vec<layers::LayerHandle>,
    registry: SurfaceRegistry,
    elements: Vec<ElementHandle>,
    pub(crate) serial_outputs:
        std::collections::VecDeque<(smithay::utils::Serial, infynitae_core::output::OutputId)>,
    pub screencopy: screencopy::ScreencopyState,
    /// `ext-image-copy-capture-v1` output capture (`image_copy`); frames join `screencopy`'s queue.
    #[expect(
        dead_code,
        reason = "owns the ext-image-copy-capture globals for the state's lifetime"
    )]
    image_copy: image_copy::ImageCopyState,
    pub dmabuf: DmabufState,
    /// The linux-dmabuf global, replaced when the backend names its GPU (`set_dmabuf_main_device`).
    dmabuf_global: Option<smithay::wayland::dmabuf::DmabufGlobal>,
    /// The GPU clients allocate on (`set_dmabuf_main_device`), also offered to capture sessions.
    pub(crate) dmabuf_main_dev: Option<libc::dev_t>,
    pub(crate) popup_pointer_grab_serial: Option<smithay::utils::Serial>,
    /// Install serial of the popup keyboard grab (the latest level of the chain), the identity by
    /// which teardown releases exactly that grab (A1).
    pub(crate) popup_keyboard_grab_serial: Option<smithay::utils::Serial>,
    /// The caller-owned effect buffer of C14.3 for the hot entries (input, commit): cleared,
    /// not freed, between events.
    effects: infynitae_core::protocol::EffectBuf,
    /// Reused storage: the elements one frame tick completes.
    ticked: Vec<infynitae_core::stage::ElementId>,
    #[expect(
        dead_code,
        reason = "owns the zxdg_decoration_manager_v1 global for the state's lifetime"
    )]
    xdg_decoration: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    pub border_width: i32,
    pub border_ignore_csd: bool,
    pub border_active: smithay::backend::renderer::Color32F,
    pub border_inactive: smithay::backend::renderer::Color32F,
    pub border_ids: std::collections::HashMap<
        infynitae_core::stage::ElementId,
        [smithay::backend::renderer::element::Id; 4],
    >,
    pub rendered_elements: Vec<infynitae_core::stage::ElementId>,
    pub input_config: InputDeviceConfig,
    pub direct_scanout: Option<bool>,
    /// Client buffers may also go on overlay planes (`OVERLAY_PLANES`, off by default: amdgpu
    /// refused page flips for Electron's viewport-scaled subsurfaces there).
    pub overlay_planes: bool,
    pub wallpaper: Option<String>,
    pub wallpaper_mode: Option<String>,
    pub keyboard_repeat_delay: i32,
    pub keyboard_repeat_rate: i32,
    pub private_elements: std::collections::HashSet<infynitae_core::stage::ElementId>,
    pub privacy_apps: Vec<String>,
    pub privacy_titles: Vec<String>,
    pub wine_transparency: bool,
    pub force_transparent_apps: Vec<String>,
    pub manual_idle_inhibit: bool,
    /// The config's `EXEC_*` commands, by the index Core's `Effect::Exec` names.
    pub(crate) exec_commands: Vec<String>,
    /// Processes started from shortcuts, to tell the windows the user opened (`launch`).
    pub(crate) launches: launch::Launches,
    /// FOCUS_ON_OPEN / TELEPORT_ON_OPEN = "user" (the default): only windows the user opened.
    pub(crate) focus_open_user_only: bool,
    pub(crate) teleport_open_user_only: bool,
    /// FOCUS_ON_ACTIVATE = "user" (the default): only tokens from the user's launches or the app
    /// itself move the focus (`request_activation`).
    pub(crate) activate_user_only: bool,
    /// `ext-session-lock-v1`, when the backend enforces it (`enable_session_lock`).
    session_lock: Option<lock::SessionLock>,
    // probe builds only: the Send assertion must be the one and only error
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

impl WaylandState {
    /// The backend draws into a window of a host compositor through GL (the nested Winit
    /// backend): its framebuffer's rows run bottom-up, so every output is drawn flipped, and the
    /// Canvas is cleared to an opaque colour (a transparent clear lets the host's desktop show
    /// through). The headless backend keeps the default: top-down rows, a transparent clear.
    pub fn set_host_window_target(&mut self) {
        self.set_target(convert::RenderTarget {
            flip_y: true,
            clear: convert::CANVAS_CLEAR,
            draw_cursor: false,
        });
    }

    /// The backend owns the display (the console backend): nobody else draws the pointer, so the
    /// compositor does (T8); the Canvas is opaque. The framebuffer's rows run top-down (Smithay's
    /// DRM compositor renders into its own swapchain the right way up).
    pub fn set_console_target(&mut self) {
        self.screencopy.enable_capture();
        self.set_target(convert::RenderTarget {
            flip_y: false,
            clear: convert::CANVAS_CLEAR,
            draw_cursor: true,
        });
    }

    fn set_target(&mut self, target: convert::RenderTarget) {
        self.target = target;
        for h in &mut self.outputs {
            if let (Some(model), Some(view)) = (self.core.output(h.id), self.core.view(h.id)) {
                h.tracker = convert::damage_tracker(model, &view, self.target);
                h.tracker_k = view.k;
            }
        }
    }

    /// Removes disabled output globals whose delay has elapsed.
    pub fn prune_disabled_outputs(&mut self) {
        let now = std::time::Instant::now();
        self.disabled_outputs.retain(|(global, deadline)| {
            if *deadline <= now {
                self.dh.remove_global::<WaylandState>(global.clone());
                false
            } else {
                true
            }
        });
    }

    /// Number of disabled output globals awaiting removal (for tests).
    pub fn disabled_outputs_count(&self) -> usize {
        self.disabled_outputs.len()
    }

    /// Forces expiration of disabled outputs' removal delays (for tests).
    pub fn expire_disabled_outputs(&mut self) {
        let past = std::time::Instant::now() - std::time::Duration::from_secs(1);
        for (_, deadline) in &mut self.disabled_outputs {
            *deadline = past;
        }
    }
}

impl WaylandState {
    /// Creates the globals (`wl_compositor` v6 and `wl_subcompositor`, `wl_shm`, `xdg_wm_base`,
    /// `zxdg_output_manager_v1`, `wp_presentation`, `wl_seat` with pointer and keyboard,
    /// `zwp_relative_pointer_manager_v1`). Output globals follow Core's effects.
    pub fn new(display: &DisplayHandle, core: Core) -> Self {
        let mut seats = SeatState::new();
        let mut seat = seats.new_wl_seat(display, "seat0");
        seat.add_pointer();
        let mut repeat_delay = std::env::var("KEYBOARD_REPEAT_DELAY")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(200);
        let mut repeat_rate = std::env::var("KEYBOARD_REPEAT_RATE")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(35);
        let mut xkb_layout = std::env::var("XKB_DEFAULT_LAYOUT").unwrap_or_default();
        let mut xkb_options = std::env::var("XKB_DEFAULT_OPTIONS").unwrap_or_default();

        if let Ok(home) = std::env::var("HOME") {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                for line in content.lines() {
                    let line_clean = line.split('#').next().unwrap_or("").trim();
                    if let Some((k, v)) = line_clean.split_once('=') {
                        let key = k.trim();
                        let val = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                        match key {
                            "KEYBOARD_REPEAT_DELAY" => {
                                if let Ok(d) = val.parse::<i32>() {
                                    repeat_delay = d;
                                }
                            }
                            "KEYBOARD_REPEAT_RATE" => {
                                if let Ok(r) = val.parse::<i32>() {
                                    repeat_rate = r;
                                }
                            }
                            "XKB_DEFAULT_LAYOUT" => {
                                if !val.is_empty() {
                                    xkb_layout = val.to_string();
                                }
                            }
                            "XKB_DEFAULT_OPTIONS" if !val.is_empty() => {
                                xkb_options = val.to_string();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let xkb_config = smithay::input::keyboard::XkbConfig {
            layout: &xkb_layout,
            options: if xkb_options.is_empty() {
                None
            } else {
                Some(xkb_options)
            },
            ..Default::default()
        };
        seat.add_keyboard(xkb_config, repeat_delay, repeat_rate)
            .expect("the seat has a keyboard");
        let capabilities = convert::wm_capabilities(core.toplevel_capabilities());

        // the primary selection exists before the data-control globals that also serve it
        let primary_selection = PrimarySelectionState::new::<Self>(display);
        let wlr_data_control = smithay::wayland::selection::wlr_data_control::DataControlState::new::<
            Self,
            _,
        >(display, Some(&primary_selection), |_| true);
        let ext_data_control = smithay::wayland::selection::ext_data_control::DataControlState::new::<
            Self,
            _,
        >(display, Some(&primary_selection), |_| true);
        let mut state = WaylandState {
            core,
            dh: display.clone(),
            compositor: CompositorState::new_v6::<Self>(display),
            output_manager: OutputManagerState::new_with_xdg_output::<Self>(display),
            presentation: PresentationState::new::<Self>(display, CLOCK_MONOTONIC),
            outputs: Vec::new(),
            disabled_outputs: Vec::new(),
            spawned_children: false,
            target: convert::RenderTarget::default(),
            cursor: smithay::input::pointer::CursorImageStatus::default_named(),
            pointer_sent: pointer::Sent::default(),
            arrow: cursor::Arrow::default(),
            cursor_theme: std::sync::Mutex::new(None),
            config_home: None,
            xcursor_path: None,
            // only what Core's toplevel-request policy can grant is announced (M2)
            xdg_shell: XdgShellState::new_with_capabilities::<Self>(display, capabilities),
            shm: ShmState::new::<Self>(display, []),
            popups: PopupManager::default(),
            popups_created: 0,
            seats,
            seat,
            popup_grab: None,
            popup_pointer_grab_serial: None,
            popup_keyboard_grab_serial: None,
            nested_grab_keep_pointer: true,
            ordered_preemption_strategy: keyboard::PreemptionStrategy::Ordered,
            pointer_constraints: PointerConstraintsState::new::<Self>(display),
            relative_pointer: RelativePointerManagerState::new::<Self>(display),
            fractional_scale_manager: FractionalScaleManagerState::new::<Self>(display),
            viewporter: ViewporterState::new::<Self>(display),
            cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Self>(
                display,
            ),
            data_device: DataDeviceState::new::<Self>(display),
            primary_selection,
            wlr_data_control,
            ext_data_control,
            selection_owners: [None, None],
            selection_sources: [None, None],
            data_control_owner: None,
            drag_started: false,
            activation: XdgActivationState::new::<Self>(display),
            layer_shell: WlrLayerShellState::new::<Self>(display),
            layers: Vec::new(),
            registry: SurfaceRegistry::default(),
            elements: Vec::new(),
            serial_outputs: std::collections::VecDeque::new(),
            screencopy: screencopy::ScreencopyState::new(display),
            image_copy: image_copy::ImageCopyState::new(display),
            dmabuf: DmabufState::new(),
            dmabuf_global: None,
            dmabuf_main_dev: None,
            effects: infynitae_core::protocol::EffectBuf::new(),
            ticked: Vec::new(),
            xdg_decoration: smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<Self>(
                display,
            ),
            border_width: 1,
            border_ignore_csd: true,
            border_active: smithay::backend::renderer::Color32F::new(0.48, 0.64, 0.97, 1.0),
            border_inactive: smithay::backend::renderer::Color32F::new(0.20, 0.23, 0.33, 0.8),
            border_ids: std::collections::HashMap::new(),
            rendered_elements: Vec::new(),
            input_config: InputDeviceConfig::default(),
            direct_scanout: None,
            overlay_planes: false,
            wallpaper: None,
            wallpaper_mode: None,
            keyboard_repeat_delay: repeat_delay,
            keyboard_repeat_rate: repeat_rate,
            private_elements: std::collections::HashSet::new(),
            privacy_apps: Vec::new(),
            privacy_titles: Vec::new(),
            wine_transparency: true,
            force_transparent_apps: Vec::new(),
            manual_idle_inhibit: false,
            exec_commands: Vec::new(),
            launches: launch::Launches::default(),
            focus_open_user_only: true,
            teleport_open_user_only: true,
            activate_user_only: true,
            session_lock: None,
            // probe builds only: the Send assertion must be the one and only error
            #[cfg(feature = "probe_store_port_in_state")]
            leaked_port: unreachable!(),
            #[cfg(feature = "probe_store_output_token_in_state")]
            leaked_token: unreachable!(),
        };
        // a guess until the backend names its GPU (the console does)
        if let Some(dev) = guess_dmabuf_device() {
            state.set_dmabuf_main_device(dev);
        }
        state
    }

    /// The GPU clients should allocate their buffers on (linux-dmabuf feedback main device): the
    /// render node the backend draws with. Replaces the global made with the previous device.
    pub fn set_dmabuf_main_device(&mut self, dev: libc::dev_t) {
        self.set_dmabuf_device_formats(dev, DMABUF_FORMATS.to_vec());
    }

    /// [`set_dmabuf_main_device`](Self::set_dmabuf_main_device) with the formats and modifiers
    /// the backend's renderer imports (tiled and compressed layouts render far faster than
    /// linear ones); an empty list keeps the linear formats.
    pub fn set_dmabuf_device_formats(
        &mut self,
        dev: libc::dev_t,
        formats: Vec<smithay::backend::allocator::Format>,
    ) {
        let formats = if formats.is_empty() {
            DMABUF_FORMATS.to_vec()
        } else {
            formats
        };
        let global = match DmabufFeedbackBuilder::new(dev, formats.iter().copied()).build() {
            Ok(feedback) => self
                .dmabuf
                .create_global_with_default_feedback::<Self>(&self.dh, &feedback),
            Err(_) => self.dmabuf.create_global::<Self>(&self.dh, formats),
        };
        if let Some(old) = self.dmabuf_global.replace(global) {
            self.dmabuf.destroy_global::<Self>(&self.dh, old);
        }
        self.dmabuf_main_dev = Some(dev);
    }

    pub fn set_border_config(
        &mut self,
        width: i32,
        active: smithay::backend::renderer::Color32F,
        inactive: smithay::backend::renderer::Color32F,
    ) {
        self.border_width = width.max(0);
        self.border_active = active;
        self.border_inactive = inactive;
    }

    /// Whether an element's toplevel negotiated Client-Side Decorations (CSD).
    pub fn is_csd_element(&self, element: infynitae_core::stage::ElementId) -> bool {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
        self.window(element)
            .and_then(|w| w.toplevel())
            .map(|t| t.current_state().decoration_mode == Some(Mode::ClientSide))
            .unwrap_or(false)
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    /// The composition root and backends reach Core through the state, as handlers do.
    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    /// One Core entry into the state's reusable [`EffectBuf`](infynitae_core::protocol::EffectBuf)
    /// (C14.3), whose effects are then applied in order: `f` calls an `_into` entry of Core. On
    /// an error nothing is applied. The buffer is cleared, not freed, so a steady stream of
    /// events allocates nothing for its effects (INV-ALLOC-1).
    pub fn with_effects<R, E>(
        &mut self,
        f: impl FnOnce(&mut Core, &mut infynitae_core::protocol::EffectBuf) -> Result<R, E>,
    ) -> Result<R, E> {
        let mut fx = std::mem::take(&mut self.effects);
        fx.clear();
        let result = f(&mut self.core, &mut fx);
        if result.is_ok() {
            self.apply(&fx);
        }
        self.effects = fx;
        result
    }

    /// A vblank of `output`: Core's frame clock decides which observed elements complete (into
    /// reused storage), and their callbacks complete and their feedback is presented.
    pub fn frame_tick(
        &mut self,
        output: infynitae_core::output::OutputId,
        now: std::time::Duration,
    ) {
        let mut ticked = std::mem::take(&mut self.ticked);
        ticked.clear();
        self.core.frame_tick_into(output, &mut ticked);
        self.complete_frames(output, &ticked, now);
        self.ticked = ticked;
        self.complete_layer_frames(output, now);
        self.complete_lock_frames(output, now);
        if self.target.draw_cursor
            && let Some(o) = self.smithay_output(output).cloned()
        {
            self.complete_cursor_frames(&o, now);
        }
    }

    /// The colour the empty Canvas is cleared to on this backend's target.
    pub fn clear_color(&self) -> smithay::backend::renderer::Color32F {
        self.target.clear
    }

    /// Sets the colour the empty Canvas is cleared to on this backend's target.
    pub fn set_clear_color(&mut self, clear: smithay::backend::renderer::Color32F) {
        self.target.clear = clear;
    }

    /// Reloads configuration from ~/.config/infynitae/config into Core and compositor state.
    pub fn reload_config(&mut self) {
        let mut new_cursor_theme: Option<String> = None;
        let mut new_cursor_size: Option<u32> = None;
        let home = self
            .config_home
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| std::env::var("HOME").ok());
        if let Some(ref home) = home {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                let mut new_wallpaper: Option<String> = None;
                let mut new_wallpaper_mode: Option<String> = None;
                let mut free_cursor_on_super = true;
                let mut bind_lines: Vec<(String, String)> = Vec::new();
                let mut exec_lines: Vec<(String, String)> = Vec::new();
                for line in content.lines() {
                    if let Some((key, val)) = config_line(line) {
                        let (key, val) = (key.as_str(), val.as_str());
                        if let Some(name) = key.strip_prefix("BIND_") {
                            bind_lines.push((name.to_ascii_uppercase(), val.to_string()));
                            continue;
                        }
                        if key.starts_with("EXEC_") {
                            exec_lines.push((key.to_string(), val.to_string()));
                            continue;
                        }
                        match key {
                            "CURSOR_THEME" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_cursor_theme = Some(trimmed.to_string());
                                }
                            }
                            "CURSOR_SIZE" => {
                                if let Ok(sz) = val.parse::<u32>() {
                                    new_cursor_size = Some(sz);
                                }
                            }
                            "PAN_MODE" => match val.to_lowercase().as_str() {
                                "rts" => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Rts),
                                "free" => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Free),
                                _ => self
                                    .core
                                    .set_pan_mode(infynitae_core::navigation::PanMode::Grab),
                            },
                            "SCREENCOPY_CURSOR" => {
                                let hide = matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.screencopy.set_hide_cursor(hide);
                            }
                            "FOCUS_FOLLOWS_MOUSE" => match val.to_lowercase().as_str() {
                                "strict" => {
                                    self.core.set_focus_follows_mouse(true);
                                    self.core.set_focus_follows_mouse_strict(true);
                                }
                                "0" | "false" | "off" | "no" | "click" => {
                                    self.core.set_focus_follows_mouse(false);
                                    self.core.set_focus_follows_mouse_strict(false);
                                }
                                _ => {
                                    self.core.set_focus_follows_mouse(true);
                                    self.core.set_focus_follows_mouse_strict(false);
                                }
                            },
                            "FOCUS_CYCLE_NAVIGATE" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_focus_cycle_navigate(enabled);
                            }
                            "FOCUS_ON_OPEN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_focus_on_open(enabled);
                                self.focus_open_user_only = val.eq_ignore_ascii_case("user");
                            }
                            "TELEPORT_ON_OPEN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                                self.core.set_teleport_on_open(enabled);
                                self.teleport_open_user_only = val.eq_ignore_ascii_case("user");
                            }
                            "SUPER_ENTER_ACTION" => {
                                if val.eq_ignore_ascii_case("fit") {
                                    self.core.set_enter_mode(infynitae_core::EnterMode::Fit);
                                } else {
                                    self.core
                                        .set_enter_mode(infynitae_core::EnterMode::Maximize);
                                }
                            }
                            "FULLSCREEN_PAN" => {
                                let enabled = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no" | "exclusive"
                                );
                                self.core.set_fullscreen_pan(enabled);
                            }
                            "PLACEMENT_POLICY" => match val.to_lowercase().as_str() {
                                "center" | "centred" | "centered" => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::CentredInView,
                                    );
                                }
                                "origin" => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::OriginAtCameraCentre,
                                    );
                                }
                                _ => {
                                    self.core.set_placement_policy(
                                        infynitae_core::PlacementPolicy::ClusterTiling,
                                    );
                                }
                            },
                            "CANVAS_COLOR" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.set_clear_color(c);
                                }
                            }
                            "BORDER_WIDTH" => {
                                if let Ok(w) = val.parse::<i32>() {
                                    self.border_width = w.max(0);
                                }
                            }
                            "BORDER_IGNORE_CSD" => {
                                self.border_ignore_csd = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "BORDER_ACTIVE" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.border_active = c;
                                }
                            }
                            "BORDER_INACTIVE" => {
                                if let Some(c) = crate::parse_color_hex(val) {
                                    self.border_inactive = c;
                                }
                            }
                            "MOUSE_ACCEL_PROFILE" => {
                                self.input_config.mouse_accel_profile = Some(val.to_string());
                            }
                            "MOUSE_ACCEL_SPEED" => {
                                if let Ok(s) = val.parse::<f64>() {
                                    self.input_config.mouse_accel_speed = Some(s.clamp(-1.0, 1.0));
                                }
                            }
                            "TOUCHPAD_TAP" => {
                                self.input_config.touchpad_tap = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "TOUCHPAD_NATURAL_SCROLL" => {
                                self.input_config.touchpad_natural_scroll = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "DIRECT_SCANOUT" => {
                                self.direct_scanout = Some(matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                ));
                            }
                            "FOCUS_ON_ACTIVATE" => {
                                use infynitae_core::input::focus::ActivationPolicy;
                                let v = val.to_lowercase();
                                self.activate_user_only = v == "user";
                                self.core.set_activation_policy(match v.as_str() {
                                    "never" | "0" | "false" | "off" => {
                                        ActivationPolicy::AttentionOnly
                                    }
                                    _ => ActivationPolicy::Strict,
                                });
                            }
                            "SUPER_FREES_CURSOR" => {
                                free_cursor_on_super = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "OVERLAY_PLANES" => {
                                self.overlay_planes = matches!(
                                    val.to_lowercase().as_str(),
                                    "1" | "true" | "yes" | "on"
                                );
                            }
                            "WALLPAPER" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_wallpaper = Some(trimmed.to_string());
                                }
                            }
                            "WALLPAPER_MODE" => {
                                let trimmed = val.trim();
                                if !trimmed.is_empty() {
                                    new_wallpaper_mode = Some(trimmed.to_string());
                                }
                            }
                            "KEYBOARD_REPEAT_DELAY" => {
                                if let Ok(d) = val.parse::<i32>() {
                                    self.keyboard_repeat_delay = d;
                                    if let Some(kb) = self.seat.get_keyboard() {
                                        kb.change_repeat_info(self.keyboard_repeat_rate, d);
                                    }
                                }
                            }
                            "KEYBOARD_REPEAT_RATE" => {
                                if let Ok(r) = val.parse::<i32>() {
                                    self.keyboard_repeat_rate = r;
                                    if let Some(kb) = self.seat.get_keyboard() {
                                        kb.change_repeat_info(r, self.keyboard_repeat_delay);
                                    }
                                }
                            }
                            "PRIVACY_APPS" => {
                                self.privacy_apps = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            "PRIVACY_TITLES" => {
                                self.privacy_titles = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            "WINE_TRANSPARENCY" => {
                                self.wine_transparency = !matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                );
                            }
                            "FORCE_TRANSPARENT_APPS" => {
                                self.force_transparent_apps = val
                                    .split(',')
                                    .map(|s| s.trim().to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                            _ => {}
                        }
                    }
                }
                let (bindings, commands, problems) = user_key_bindings(&bind_lines, &exec_lines);
                for p in problems {
                    eprintln!("[infynitae] Config key binding ignored: {p}");
                }
                self.core.set_key_bindings(bindings);
                self.core.set_free_cursor_on_super(free_cursor_on_super);
                self.exec_commands = commands;
                if new_wallpaper != self.wallpaper || new_wallpaper_mode != self.wallpaper_mode {
                    self.wallpaper = new_wallpaper;
                    self.wallpaper_mode = new_wallpaper_mode;
                    let home = std::env::var("HOME").unwrap_or_default();
                    let bin = format!("{home}/.local/bin/infynitae-wallpaper");
                    let bin_fallback =
                        format!("{home}/infynitae/live/wallpaper/infynitae-wallpaper");
                    let target_bin = if std::path::Path::new(&bin).exists() {
                        bin
                    } else {
                        bin_fallback
                    };
                    let _ = std::process::Command::new(target_bin).spawn();
                    self.spawned_children = true;
                }
            }
        }
        let theme_name = new_cursor_theme
            .or_else(|| std::env::var("CURSOR_THEME").ok())
            .or_else(|| std::env::var("XCURSOR_THEME").ok())
            .unwrap_or_else(|| "default".to_string());
        let cursor_size = new_cursor_size
            .or_else(|| {
                std::env::var("CURSOR_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .or_else(|| {
                std::env::var("XCURSOR_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(24);
        let changed = match self.cursor_theme.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(mgr) => mgr.theme_name() != theme_name || mgr.size() != cursor_size,
                None => true,
            },
            Err(_) => true,
        };
        if changed {
            let home_path = self
                .config_home
                .as_ref()
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from));
            let xcursor_path = self
                .xcursor_path
                .clone()
                .or_else(|| std::env::var("XCURSOR_PATH").ok());
            let paths =
                cursor::theme_search_paths_with(home_path.as_deref(), xcursor_path.as_deref());
            self.set_cursor_theme_with_search_paths(&theme_name, cursor_size, paths);
        }
    }

    /// Current input device configuration.
    pub fn input_config(&self) -> &InputDeviceConfig {
        &self.input_config
    }

    pub fn registry(&self) -> &SurfaceRegistry {
        &self.registry
    }

    /// The protocol handle of an element (render oracles and later increments).
    pub fn window(&self, id: ElementId) -> Option<&Window> {
        self.elements.iter().find(|e| e.id == id).map(|e| &e.window)
    }

    /// The root `wl_surface` of a window, whether native Wayland (XDG) or XWayland (X11).
    pub fn window_surface(
        &self,
        id: ElementId,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        use smithay::wayland::seat::WaylandFocus;
        self.window(id)
            .and_then(|w| w.wl_surface().map(|s| s.into_owned()))
    }

    /// Elements in creation order.
    pub fn elements(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.elements.iter().map(|e| e.id)
    }

    /// Whether an element is marked private (omitted/transparent during screencopy, screenshare, RustDesk).
    pub fn is_element_private(&self, element: infynitae_core::stage::ElementId) -> bool {
        if self.private_elements.contains(&element) {
            return true;
        }
        if let Some(meta) = self.core.stage().meta(element) {
            if let Some(app_id) = &meta.app_id {
                let id_l = app_id.to_lowercase();
                if self.privacy_apps.iter().any(|p| id_l.contains(p)) {
                    return true;
                }
            }
            if let Some(title) = &meta.title {
                let t_l = title.to_lowercase();
                if self.privacy_titles.iter().any(|t| t_l.contains(t)) {
                    return true;
                }
            }
        }
        false
    }

    /// Toggles the privacy capture exemption on the currently focused window.
    pub fn toggle_privacy_focused(&mut self) {
        if let Some(focused) = self.core.focus().target() {
            if self.private_elements.contains(&focused) {
                self.private_elements.remove(&focused);
                println!("[infynitae] Privacy capture exemption DISABLED for window {focused:?}");
                self.spawn_action("notify-privacy-off");
            } else {
                self.private_elements.insert(focused);
                println!(
                    "[infynitae] Privacy capture exemption ENABLED for window {focused:?} (hidden from screenshots and screenshare)"
                );
                self.spawn_action("notify-privacy-on");
            }
        }
    }

    /// Toggles stay-awake idle sleep inhibition (blocks DPMS monitor sleep and auto-lock).
    pub fn toggle_idle_inhibit(&mut self) {
        self.manual_idle_inhibit = !self.manual_idle_inhibit;
        if self.manual_idle_inhibit {
            println!("[infynitae] Idle sleep & auto-lock INHIBITED (Stay Awake mode)");
            self.spawn_action("notify-idle-inhibit-on");
        } else {
            println!("[infynitae] Idle sleep & auto-lock RESTORED to normal timeout");
            self.spawn_action("notify-idle-inhibit-off");
        }
    }

    /// Whether idle sleep & screen auto-locking should be inhibited.
    pub fn is_idle_inhibited(&self) -> bool {
        self.manual_idle_inhibit
    }

    /// Whether an element should force transparent blending (Wine layered windows, colorkeys).
    pub fn is_element_transparent(&self, element: infynitae_core::stage::ElementId) -> bool {
        if !self.wine_transparency {
            return false;
        }
        if let Some(meta) = self.core.stage().meta(element) {
            if let Some(app_id) = &meta.app_id {
                let id_l = app_id.to_lowercase();
                if id_l.contains("wine")
                    || id_l.ends_with(".exe")
                    || self.force_transparent_apps.iter().any(|a| id_l.contains(a))
                {
                    return true;
                }
            }
            if let Some(title) = &meta.title {
                let t_l = title.to_lowercase();
                if self.force_transparent_apps.iter().any(|a| t_l.contains(a)) {
                    return true;
                }
            }
        }
        false
    }

    /// Returns the current modifier state from Smithay's XKB seat state (OBJ-27).
    pub fn seat_modifiers(&self) -> infynitae_core::input::bindings::Modifiers {
        self.seat
            .get_keyboard()
            .map(|kb| convert::modifiers_from_smithay(&kb.modifier_state()))
            .unwrap_or_default()
    }

    /// Returns whether the seat's keyboard currently has an active grab.
    pub fn keyboard_is_grabbed(&self) -> bool {
        self.keyboard_handle().is_grabbed()
    }

    /// Returns the currently focused surface of the seat's keyboard, if any.
    pub fn keyboard_current_focus(
        &self,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        self.keyboard_handle().current_focus()
    }

    /// Returns Smithay's raw XKB modifier state for testing and observation.
    pub fn keyboard_modifier_state(&self) -> smithay::input::keyboard::ModifiersState {
        self.keyboard_handle().modifier_state()
    }

    /// Updates Smithay's XKB state for the physical key event carried by `token` and returns the
    /// resulting modifiers and whether the modifier state changed, without forwarding the key to
    /// any client. The token is consumed; a token minted by another Core mutates nothing (R2-02).
    /// Only a logical transition reaches XKB (R2-04, single implementation for every backend,
    /// C3): a press of a key Core already holds, or a release of a key Core does not hold, is a
    /// duplicate/orphan that Core will absorb, so the seat must not see it either.
    pub fn sync_seat_key(
        &mut self,
        token: infynitae_core::InputAuthToken,
    ) -> (infynitae_core::input::bindings::Modifiers, bool) {
        if token.core_id() != self.core.id() {
            return (self.seat_modifiers(), false);
        }
        let (key, pressed) = (token.key(), token.pressed());
        let is_down = self.core.is_key_down(key);
        if pressed == is_down {
            return (self.seat_modifiers(), false);
        }
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::Keycode;

        let kb = self.keyboard_handle();
        let code = Keycode::new(key + 8);
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let (_, mods_changed) =
            kb.input_intercept(self, code, state, |_state, _mods, _sym| None::<()>);
        let modifiers = convert::modifiers_from_smithay(&kb.modifier_state());
        (modifiers, mods_changed)
    }

    /// Returns the seat keyboard's active LED indicator state (Caps Lock, Num Lock, Scroll Lock).
    pub fn keyboard_led_state(&self) -> smithay::input::keyboard::LedState {
        self.keyboard_handle().led_state()
    }

    /// Returns the number of interaction serials currently held in the provenance ring buffer.
    pub fn serial_outputs_len(&self) -> usize {
        self.serial_outputs.len()
    }

    /// Records the causal output on which a user interaction serial occurred (D-B6 Rule 1 provenance).
    pub(crate) fn record_serial_output(
        &mut self,
        serial: smithay::utils::Serial,
        output: infynitae_core::output::OutputId,
    ) {
        if self.serial_outputs.len() >= 64 {
            self.serial_outputs.pop_front();
        }
        self.serial_outputs.push_back((serial, output));
    }

    /// Retrieves the causal output mapped to a serial, if known.
    pub(crate) fn serial_output(
        &self,
        serial: smithay::utils::Serial,
    ) -> Option<infynitae_core::output::OutputId> {
        self.serial_outputs
            .iter()
            .rev()
            .find(|(s, _)| *s == serial)
            .map(|(_, o)| *o)
    }
}

/// One `KEY=value` line of the config, which the session scripts source as shell: the value is
/// read as one shell word (`"…"` with `\"` escapes, `'…'`, or bare up to a blank), so a `#` or a
/// quote inside it is kept, and a ` # comment` after it is not. `None` for a comment or a line
/// without `=`. Pure.
pub fn config_line(line: &str) -> Option<(String, String)> {
    let line = line.trim_start();
    if line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    let mut out = String::new();
    let mut chars = value.trim_start().chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => break,
            '\'' => out.extend(chars.by_ref().take_while(|c| *c != '\'')),
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => out.push(e),
                            Some(e) => {
                                out.push('\\');
                                out.push(e);
                            }
                            None => out.push('\\'),
                        },
                        c => out.push(c),
                    }
                }
            }
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    Some((key.trim().to_string(), out.trim().to_string()))
}

/// The user's key bindings from config lines: `BIND_<ACTION>="CHORD[,CHORD]"` gives an action
/// its own chords (its built-in chord is freed), `BIND_<ACTION>="none"` frees it without a new
/// one; `EXEC_<NAME>="CHORD:command"` runs a command. Returns the bindings, the commands (by
/// `Exec` index) and a description of each line that was not understood, or that names a chord
/// an earlier line already bound (the earlier line keeps it). Pure.
pub fn user_key_bindings(
    binds: &[(String, String)],
    execs: &[(String, String)],
) -> (
    infynitae_core::input::keys::UserBindings,
    Vec<String>,
    Vec<String>,
) {
    use infynitae_core::input::keys::{ACTION_NAMES, KeyAction, UserBindings, parse_chord};
    let mut out = UserBindings::default();
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    // who bound each chord, for the duplicate report
    let mut bound_by: Vec<(infynitae_core::input::keys::Chord, String)> = Vec::new();
    let mut taken = |chord, line: String, problems: &mut Vec<String>, text: &str| {
        if let Some((_, first)) = bound_by.iter().find(|(c, _)| *c == chord) {
            problems.push(format!("{line}: \"{text}\" is already bound by {first}"));
            return true;
        }
        bound_by.push((chord, line));
        false
    };
    for (name, value) in binds {
        let Some(&(_, action)) = ACTION_NAMES.iter().find(|(n, _)| n == name) else {
            problems.push(format!("BIND_{name}: no such action"));
            continue;
        };
        out.moved.push(action);
        if value.eq_ignore_ascii_case("none") || value.is_empty() {
            continue;
        }
        for chord_text in value.split(',') {
            match parse_chord(chord_text) {
                Some(chord) => {
                    if !taken(
                        chord,
                        format!("BIND_{name}"),
                        &mut problems,
                        chord_text.trim(),
                    ) {
                        out.chords.push((chord, Some(action)));
                    }
                }
                None => problems.push(format!("BIND_{name}: \"{chord_text}\" is not a key chord")),
            }
        }
    }
    for (name, value) in execs {
        let Some((chord_text, command)) = value.split_once(':') else {
            problems.push(format!("{name}: expected \"CHORD:command\""));
            continue;
        };
        let (Some(chord), command) = (parse_chord(chord_text), command.trim()) else {
            problems.push(format!("{name}: \"{chord_text}\" is not a key chord"));
            continue;
        };
        if command.is_empty() {
            problems.push(format!("{name}: no command"));
            continue;
        }
        if commands.len() >= usize::from(u16::MAX) {
            problems.push(format!("{name}: too many EXEC_ commands"));
            continue;
        }
        if taken(chord, name.clone(), &mut problems, chord_text.trim()) {
            continue;
        }
        out.chords
            .push((chord, Some(KeyAction::Exec(commands.len() as u16))));
        commands.push(command.to_string());
    }
    (out, commands, problems)
}

/// The dmabuf formats offered when the backend names none: linear only, which any GPU can
/// import and scan out.
const DMABUF_FORMATS: [smithay::backend::allocator::Format; 2] = [
    smithay::backend::allocator::Format {
        code: smithay::backend::allocator::Fourcc::Argb8888,
        modifier: smithay::backend::allocator::Modifier::Linear,
    },
    smithay::backend::allocator::Format {
        code: smithay::backend::allocator::Fourcc::Xrgb8888,
        modifier: smithay::backend::allocator::Modifier::Linear,
    },
];

/// A DRM device for backends that do not name theirs: the first render node, else the first
/// card node.
fn guess_dmabuf_device() -> Option<libc::dev_t> {
    use std::os::unix::fs::MetadataExt;
    let mut nodes: Vec<std::path::PathBuf> = std::fs::read_dir("/dev/dri")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    nodes.sort();
    let pick = |prefix: &str| {
        nodes.iter().find_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            if name.starts_with(prefix) {
                std::fs::metadata(p).ok().map(|m| m.rdev() as libc::dev_t)
            } else {
                None
            }
        })
    };
    pick("renderD").or_else(|| pick("card"))
}

/// Parses a hex color `#RRGGBB` or `#RRGGBBAA` into a `Color32F`.
pub fn parse_color_hex(hex: &str) -> Option<smithay::backend::renderer::Color32F> {
    let s = hex
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        Some(smithay::backend::renderer::Color32F::new(r, g, b, 1.0))
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        let a = u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0;
        Some(smithay::backend::renderer::Color32F::new(r, g, b, a))
    } else {
        None
    }
}

/// INV-ARCH-2: `unsafe` is denied here (lint `unsafe_code`).
#[cfg(feature = "probe_unsafe_block")]
pub fn probe_unsafe_block() {
    unsafe {}
}

#[cfg(test)]
mod key_binding_config_tests {
    use super::user_key_bindings;
    use infynitae_core::input::keys::{KeyAction, parse_chord};

    fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// BIND_ moves actions (several chords allowed) or frees them with "none"; EXEC_ adds
    /// commands in order; lines that cannot be understood are reported, not applied.
    #[test]
    fn config_lines_become_bindings_and_commands() {
        let (b, cmds, problems) = user_key_bindings(
            &kv(&[
                ("TERMINAL", "SUPER+RETURN,SUPER+T"),
                ("LAUNCHER", "none"),
                ("TELEPORT", "SUPER+X"),
                ("LOCK", "SUPER+NOPE"),
            ]),
            &kv(&[
                ("EXEC_BROWSER", "SUPER+B: firefox --new-window"),
                ("EXEC_BAD", "firefox"),
            ]),
        );
        assert_eq!(
            b.chords,
            vec![
                (
                    parse_chord("SUPER+RETURN").unwrap(),
                    Some(KeyAction::SpawnTerminal)
                ),
                (
                    parse_chord("SUPER+T").unwrap(),
                    Some(KeyAction::SpawnTerminal)
                ),
                (parse_chord("SUPER+B").unwrap(), Some(KeyAction::Exec(0))),
            ]
        );
        assert_eq!(
            b.moved,
            vec![
                KeyAction::SpawnTerminal,
                KeyAction::ToggleLauncher,
                KeyAction::SpawnLockScreen
            ]
        );
        assert_eq!(cmds, vec!["firefox --new-window".to_string()]);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    /// A chord bound by two lines stays with the first, and the second is reported.
    #[test]
    fn a_chord_bound_twice_is_reported() {
        let (b, cmds, problems) = user_key_bindings(
            &kv(&[("CLOSE", "SUPER+Q")]),
            &kv(&[("EXEC_X", "SUPER+Q:foo"), ("EXEC_Y", "SUPER+Y:bar")]),
        );
        assert_eq!(
            b.chords
                .iter()
                .filter(|(c, _)| *c == parse_chord("SUPER+Q").unwrap())
                .count(),
            1
        );
        assert_eq!(cmds, vec!["bar".to_string()]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("EXEC_X") && problems[0].contains("BIND_CLOSE"));
    }

    /// Config lines are read as the shell reads them: quotes and `#` inside a value are kept,
    /// a trailing comment is not.
    #[test]
    fn config_values_are_read_as_shell_words() {
        use super::config_line;
        let v = |l: &str| config_line(l).map(|(_, v)| v);
        assert_eq!(
            v(r#"EXEC_NOTE="SUPER+N:sh -c 'notify-send hi'""#).as_deref(),
            Some("SUPER+N:sh -c 'notify-send hi'")
        );
        assert_eq!(
            v(r#"EXEC_WEB="SUPER+B:firefox https://x.org/#top"  # docs"#).as_deref(),
            Some("SUPER+B:firefox https://x.org/#top")
        );
        assert_eq!(v("PAN_MODE=rts # comment").as_deref(), Some("rts"));
        assert_eq!(v("BG='#1e1e2e'").as_deref(), Some("#1e1e2e"));
        assert_eq!(v(r#"X="a \"b\" c""#).as_deref(), Some(r#"a "b" c"#));
        assert_eq!(
            config_line("export KEY=1"),
            Some(("KEY".to_string(), "1".to_string()))
        );
        assert_eq!(config_line("# EXEC_X=\"SUPER+X:foo\""), None);
        assert_eq!(config_line("no equals"), None);
    }

    #[test]
    fn parse_color_hex_handles_non_ascii_and_malformed() {
        use super::parse_color_hex;
        assert!(parse_color_hex("aé123").is_none());
        assert!(parse_color_hex("#aé1234").is_none());
        assert!(parse_color_hex("#12345é").is_none());
        assert!(parse_color_hex("").is_none());
        assert!(parse_color_hex("invalid").is_none());

        let c = parse_color_hex("#ff0000").unwrap();
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (1.0, 0.0, 0.0, 1.0));

        let c2 = parse_color_hex("#00ff0080").unwrap();
        assert_eq!((c2.r(), c2.g(), c2.b()), (0.0, 1.0, 0.0));
        assert!((c2.a() - 128.0 / 255.0).abs() < 1e-4);
    }
}
