//! T1 lockstep harness (FOUNDATION_TEST_PLAN T1): the production core, adapter and headless
//! backend in one thread with a recording probe client over a socketpair. Deterministic, no real
//! time, no host compositor, no socket on disk, no `unsafe`.

#![allow(dead_code)]

pub mod input;
pub mod oracle;
pub mod probe_renderer;

use std::fs::File;
use std::io::Write as _;
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use infynitae_backend_headless::{VirtualOutput, connect, frame_fallback, vblank, vblank_period};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputId, Transform};
use infynitae_core::pointer::PointerState;
use infynitae_core::stage::ElementId;
use infynitae_core::{Core, InputPort, OutputLifecycleToken, ShellPort};
use infynitae_wayland::{ClientState, WaylandState};
use smithay::reexports::wayland_server::Display;
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_data_device::{self, WlDataDevice},
    wl_data_device_manager::WlDataDeviceManager,
    wl_data_offer::{self, WlDataOffer},
    wl_data_source::{self, WlDataSource},
    wl_keyboard::{self, WlKeyboard},
    wl_output::{self, WlOutput},
    wl_pointer::{self, WlPointer},
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface,
    wl_surface::{self, WlSurface},
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::pointer_constraints::zv1::client::{
    zwp_confined_pointer_v1::{self, ZwpConfinedPointerV1},
    zwp_locked_pointer_v1::{self, ZwpLockedPointerV1},
    zwp_pointer_constraints_v1::{self, ZwpPointerConstraintsV1},
};
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};
use wayland_protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1,
    zwp_primary_selection_device_v1::{self, ZwpPrimarySelectionDeviceV1},
    zwp_primary_selection_offer_v1::{self, ZwpPrimarySelectionOfferV1},
    zwp_primary_selection_source_v1::{self, ZwpPrimarySelectionSourceV1},
};
use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols::xdg::activation::v1::client::{
    xdg_activation_token_v1::{self, XdgActivationTokenV1},
    xdg_activation_v1::XdgActivationV1,
};
use wayland_protocols::xdg::shell::client::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::{self, XdgPositioner},
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

/// One `wl_region` request: (add?, (x, y, w, h)).
pub type RegionRequest = (bool, (i32, i32, i32, i32));

/// A popup positioner: size, anchor rectangle, slide constraint adjustment.
pub struct PopupSpec {
    pub content: (i32, i32),
    pub shadow: i32,
    pub anchor_rect: (i32, i32, i32, i32),
    pub slide: bool,
}

/// A client surface the probe created.
pub struct Surf {
    pub label: &'static str,
    pub wl: WlSurface,
    pub xdg: Option<XdgSurface>,
    pub toplevel: Option<XdgToplevel>,
    pub popup: Option<XdgPopup>,
    pub subsurface: Option<WlSubsurface>,
    /// Buffer size drawn after the first configure (xdg roles) or immediately (subsurfaces).
    pub size: (i32, i32),
    pub configured: bool,
    pub popup_configure: Option<(i32, i32, i32, i32)>,
    /// Requests the next frame and redraws on every `done` (a continuously animating client).
    pub frame_driven: bool,
}

/// `wl_keyboard` logical state as defined by the protocol, plus invariant checkers.
#[derive(Default, Debug, Clone)]
pub struct KbLogical {
    pub active: Option<String>,
    pub keys: std::collections::BTreeSet<u32>,
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    pub group: u32,
    pub awaiting_modifiers: bool,
    pub violations: Vec<String>,
}

/// What a probe client sees of the clipboard (wl_data_device): the offers it was given with
/// their MIME types, the current selection offer, its sources' `send` requests and
/// cancellations, in one ordered log ("offer N", "mime N type", "selection N|none",
/// "send type", "cancelled").
#[derive(Default)]
pub struct Clipboard {
    pub device: Option<WlDataDevice>,
    pub log: Vec<String>,
    /// The offer of the current selection, with its MIME types.
    pub selection: Option<(WlDataOffer, Vec<String>)>,
    pending: Vec<(WlDataOffer, Vec<String>)>,
    /// What a source of this client writes when it is asked to send.
    pub source_bytes: Vec<u8>,
    /// Sources of this client still alive.
    pub sources: Vec<WlDataSource>,
}

#[derive(Default)]
pub struct Probe {
    pub globals: Vec<(u32, String, u32)>,
    pub clipboard: Clipboard,
    pub data_device_manager: Option<WlDataDeviceManager>,
    /// The primary selection: (manager, device, current offer with its MIME types, bytes a
    /// source of this client sends).
    pub primary: Option<(
        ZwpPrimarySelectionDeviceManagerV1,
        ZwpPrimarySelectionDeviceV1,
    )>,
    pub primary_selection: Option<(ZwpPrimarySelectionOfferV1, Vec<String>)>,
    primary_pending: Vec<(ZwpPrimarySelectionOfferV1, Vec<String>)>,
    pub primary_bytes: Vec<u8>,
    /// The `xdg_activation_token_v1.done` tokens received, in order.
    pub activation_tokens: Vec<String>,
    /// Ordered wire log of output-related events (the independent oracle for wire order):
    /// "global wl_output N", "global_remove N", "N: mode WxH@R", "N: scale S", "N: geometry
    /// transform T", "N: done", "N: xdg position X,Y", "N: xdg size W,H", "enter S N",
    /// "leave S N", "frame S", "presented #K sync N", "discarded #K".
    pub log: Vec<String>,
    /// Bound wl_output proxies by global name.
    pub outputs: Vec<(u32, WlOutput)>,
    pub xdg_output_manager: Option<ZxdgOutputManagerV1>,
    pub presentation: Option<WpPresentation>,
    feedback_seq: u32,
    /// Virtual time, set by the lockstep before each dispatch.
    pub now: Duration,
    /// (time, surface label) of every frame callback `done`.
    pub frames: Vec<(Duration, &'static str)>,
    /// One ordered wire log across objects, for the order of a frame callback relative to the
    /// event that kicked it (M16): "frame S", "configure S", "popup_done S", "kb_enter S",
    /// "kb_leave S", "key K", "pointer <line as in `ptr`>".
    pub wire: Vec<String>,
    pub compositor: Option<WlCompositor>,
    pub subcompositor: Option<WlSubcompositor>,
    pub shm: Option<WlShm>,
    pub wm_base: Option<XdgWmBase>,
    pub surfaces: Vec<Surf>,
    shm_pool: Option<(WlShmPool, File, usize)>,
    pub seat: Option<WlSeat>,
    pub pointer: Option<WlPointer>,
    pub keyboard: Option<WlKeyboard>,
    pub relative: Option<ZwpRelativePointerV1>,
    pub constraints: Option<ZwpPointerConstraintsV1>,
    pub locked: Option<ZwpLockedPointerV1>,
    pub confined: Option<ZwpConfinedPointerV1>,
    pub fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    pub fractional_scales: Vec<(usize, WpFractionalScaleV1)>,
    pub preferred_scales: Vec<(String, u32)>,
    pub preferred_buffer_scales: Vec<(String, i32)>,
    /// Ordered wire log of the scale hints and the configures they must precede:
    /// "preferred_scale S N", "preferred_buffer_scale S N", "configure S" (xdg_surface).
    pub scale_log: Vec<String>,
    pub viewporter: Option<WpViewporter>,
    pub viewports: Vec<(usize, WpViewport)>,
    /// Ordered wire log of the seat's pointer (the independent oracle for client-observed input):
    /// "enter S x,y", "leave S", "motion x,y", "button B pressed|released", "axis A v",
    /// "axis_v120 A n", "frame", "relative dx,dy unaccel ux,uy". Coordinates are the wl_fixed
    /// values the client received.
    pub ptr: Vec<String>,
    /// Every xdg_surface.configure received, by surface label, in order.
    pub configures: Vec<&'static str>,
    pub kb: KbLogical,
    pub kb_log: Vec<String>,
    /// `zwlr_layer_shell_v1` (L1–L8), its surfaces (index into `surfaces`), the configures they
    /// received (index, size) and those closed.
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub layer_surfaces: Vec<(usize, ZwlrLayerSurfaceV1)>,
    pub layer_configures: Vec<(usize, (u32, u32))>,
    pub layer_closed: Vec<usize>,
    /// `xdg_toplevel.close` requests received (surface index).
    pub close_requests: Vec<usize>,
    pub key_events: Vec<(u32, bool)>,
    pub repeat_info: Option<(i32, i32)>,
    pub destroyed_active: Vec<String>,
    pub auto_destroy_popups: bool,
    pub popup_dones: Vec<&'static str>,
    pub last_serial: u32,
    pub toplevel_states: Vec<(usize, Vec<u32>)>,
    /// Every `xdg_toplevel.configure` as received: (surface, (width, height), states).
    pub toplevel_configures: Vec<(usize, (i32, i32), Vec<u32>)>,
    /// Every `xdg_toplevel.wm_capabilities` as received: (surface, capabilities).
    pub wm_capabilities: Vec<(usize, Vec<u32>)>,
    /// Every `xdg_toplevel.configure_bounds` as received.
    pub configure_bounds: Vec<(usize, (i32, i32))>,
    /// The `time` argument of every `wl_pointer` motion, button and axis event, in order, with
    /// the event's kind ("motion", "button", "axis").
    pub ptr_times: Vec<(&'static str, u32)>,
    /// While set, an `xdg_surface.configure` is recorded in `held_acks` and neither acknowledged
    /// nor drawn for: a client that is slow, or that never answers.
    pub hold_acks: bool,
    /// (surface, serial) of every configure received while `hold_acks` was set, in order.
    pub held_acks: Vec<(usize, u32)>,
}

/// Unique shm file names across the tests of this process.
static SHM_FILES: AtomicU64 = AtomicU64::new(0);

impl Probe {
    pub fn buffer(&mut self, qh: &QueueHandle<Self>, w: i32, h: i32) -> WlBuffer {
        let stride = w * 4;
        let len = (stride * h).max(4) as usize;
        let pool = match &mut self.shm_pool {
            Some((pool, file, cap)) if *cap >= len => pool.clone(),
            Some((pool, file, cap)) => {
                let new_cap = len.max(*cap * 2);
                file.set_len(new_cap as u64).expect("resize shm file");
                pool.resize(new_cap as i32);
                *cap = new_cap;
                pool.clone()
            }
            None => {
                let initial_cap = len.max(4 * 1024 * 1024);
                let path = std::env::temp_dir().join(format!(
                    "infynitae-t1-{}-{}",
                    std::process::id(),
                    SHM_FILES.fetch_add(1, Ordering::Relaxed)
                ));
                let file = File::options()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .expect("shm file");
                let _ = std::fs::remove_file(&path);
                file.set_len(initial_cap as u64).expect("size shm file");
                let pool: WlShmPool = self.shm.as_ref().expect("wl_shm").create_pool(
                    std::os::fd::AsFd::as_fd(&file),
                    initial_cap as i32,
                    qh,
                    (),
                );
                self.shm_pool = Some((pool.clone(), file, initial_cap));
                pool
            }
        };
        pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, qh, ())
    }

    /// Binds what a second, independent client needs to map a window and be focused: the
    /// compositor, shm, xdg_wm_base and the seat's keyboard. For a probe that is not the
    /// harness's own (which `Lockstep` binds in full).
    pub fn bind_basic(&mut self, registry: &WlRegistry, qh: &QueueHandle<Self>) {
        let find = |name: &str| {
            self.globals
                .iter()
                .find(|(_, i, _)| i == name)
                .map(|(n, _, v)| (*n, *v))
                .unwrap_or_else(|| panic!("global {name} missing"))
        };
        let (n, _) = find("wl_compositor");
        self.compositor = Some(registry.bind(n, 6, qh, ()));
        let (n, _) = find("wl_shm");
        self.shm = Some(registry.bind(n, 1, qh, ()));
        let (n, v) = find("xdg_wm_base");
        self.wm_base = Some(registry.bind(n, v.min(6), qh, ()));
        let (n, v) = find("wl_seat");
        let seat: WlSeat = registry.bind(n, v.min(9), qh, ());
        self.keyboard = Some(seat.get_keyboard(qh, ()));
        self.seat = Some(seat);
    }

    /// Attaches a fresh buffer of the surface's size and commits (map or remap).
    pub fn draw(&mut self, qh: &QueueHandle<Self>, idx: usize) {
        let (w, h) = self.surfaces[idx].size;
        let buf = self.buffer(qh, w, h);
        let wl = &self.surfaces[idx].wl;
        wl.attach(Some(&buf), 0, 0);
        wl.damage_buffer(0, 0, w, h);
        wl.commit();
    }

    fn new_xdg_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        size: (i32, i32),
    ) -> usize {
        let idx = self.surfaces.len();
        let wl = self
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_surface(qh, ());
        let xdg = self
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .get_xdg_surface(&wl, qh, idx);
        self.surfaces.push(Surf {
            label,
            wl,
            xdg: Some(xdg),
            toplevel: None,
            popup: None,
            subsurface: None,
            size,
            configured: false,
            popup_configure: None,
            frame_driven: false,
        });
        idx
    }

    /// Toplevel role assigned but NO initial commit yet (protocol-legal; B1 lifecycle tests).
    pub fn toplevel_uncommitted(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        content: (i32, i32),
    ) -> usize {
        let idx = self.new_xdg_surface(qh, label, content);
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let tl = xdg.get_toplevel(qh, idx);
        self.surfaces[idx].toplevel = Some(tl);
        idx
    }

    /// Toplevel of `content` size with a CSD shadow; initial commit without a buffer.
    pub fn toplevel(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        content: (i32, i32),
        shadow: i32,
    ) -> usize {
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let idx = self.new_xdg_surface(qh, label, size);
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let tl = xdg.get_toplevel(qh, idx);
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        self.surfaces[idx].toplevel = Some(tl);
        self.surfaces[idx].wl.commit();
        idx
    }

    /// Toplevel with fractional scale bound before initial commit.
    pub fn toplevel_with_fractional_scale(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        content: (i32, i32),
        shadow: i32,
    ) -> usize {
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let idx = self.new_xdg_surface(qh, label, size);
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let tl = xdg.get_toplevel(qh, idx);
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        self.surfaces[idx].toplevel = Some(tl);
        self.get_fractional_scale(qh, idx);
        self.surfaces[idx].wl.commit();
        idx
    }

    pub fn get_fractional_scale(&mut self, qh: &QueueHandle<Self>, idx: usize) {
        if let Some(mgr) = &self.fractional_scale_manager {
            let fs = mgr.get_fractional_scale(&self.surfaces[idx].wl, qh, idx);
            self.fractional_scales.push((idx, fs));
        }
    }

    pub fn get_viewport(&mut self, qh: &QueueHandle<Self>, idx: usize) -> Option<WpViewport> {
        let mgr = self.viewporter.as_ref()?;
        let vp = mgr.get_viewport(&self.surfaces[idx].wl, qh, ());
        self.viewports.push((idx, vp.clone()));
        Some(vp)
    }

    /// Popup of `content` size with a CSD shadow at `anchor_rect` in the parent's window
    /// geometry, anchor top-left, gravity bottom-right; optional slide constraint adjustment.
    pub fn popup(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        spec: PopupSpec,
    ) -> usize {
        let idx = self.popup_uncommitted(qh, label, parent, spec);
        self.surfaces[idx].wl.commit();
        idx
    }

    /// A popup role object whose initial commit has not been made: what a client that
    /// misbehaves (a buffer in the initial commit, W-SM-6) starts from.
    pub fn popup_uncommitted(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        spec: PopupSpec,
    ) -> usize {
        let PopupSpec {
            content,
            shadow,
            anchor_rect,
            slide,
        } = spec;
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let idx = self.new_xdg_surface(qh, label, size);
        let pos: XdgPositioner = self
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .create_positioner(qh, ());
        pos.set_size(content.0, content.1);
        pos.set_anchor_rect(anchor_rect.0, anchor_rect.1, anchor_rect.2, anchor_rect.3);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        if slide {
            pos.set_constraint_adjustment(
                xdg_positioner::ConstraintAdjustment::SlideX
                    | xdg_positioner::ConstraintAdjustment::SlideY,
            );
        }
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let parent_xdg = self.surfaces[parent].xdg.clone().expect("parent xdg");
        let popup = xdg.get_popup(Some(&parent_xdg), &pos, qh, idx);
        pos.destroy();
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        self.surfaces[idx].popup = Some(popup);
        idx
    }

    /// Popup with an explicit grab: sends `xdg_popup.grab(seat, serial)` before initial commit.
    pub fn popup_with_grab(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        spec: PopupSpec,
    ) -> usize {
        let PopupSpec {
            content,
            shadow,
            anchor_rect,
            slide,
        } = spec;
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let idx = self.new_xdg_surface(qh, label, size);
        let pos: XdgPositioner = self
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .create_positioner(qh, ());
        pos.set_size(content.0, content.1);
        pos.set_anchor_rect(anchor_rect.0, anchor_rect.1, anchor_rect.2, anchor_rect.3);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        if slide {
            pos.set_constraint_adjustment(
                xdg_positioner::ConstraintAdjustment::SlideX
                    | xdg_positioner::ConstraintAdjustment::SlideY,
            );
        }
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let parent_xdg = self.surfaces[parent].xdg.clone().expect("parent xdg");
        let popup = xdg.get_popup(Some(&parent_xdg), &pos, qh, idx);
        pos.destroy();
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        popup.grab(self.seat.as_ref().expect("seat"), self.last_serial);
        self.surfaces[idx].popup = Some(popup);
        self.surfaces[idx].wl.commit();
        idx
    }

    /// Popup with an explicit grab using a specific serial.
    pub fn popup_with_grab_serial(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        spec: PopupSpec,
        serial: u32,
    ) -> usize {
        let PopupSpec {
            content,
            shadow,
            anchor_rect,
            slide,
        } = spec;
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let idx = self.new_xdg_surface(qh, label, size);
        let pos: XdgPositioner = self
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .create_positioner(qh, ());
        pos.set_size(content.0, content.1);
        pos.set_anchor_rect(anchor_rect.0, anchor_rect.1, anchor_rect.2, anchor_rect.3);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        if slide {
            pos.set_constraint_adjustment(
                xdg_positioner::ConstraintAdjustment::SlideX
                    | xdg_positioner::ConstraintAdjustment::SlideY,
            );
        }
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let parent_xdg = self.surfaces[parent].xdg.clone().expect("parent xdg");
        let popup = xdg.get_popup(Some(&parent_xdg), &pos, qh, idx);
        pos.destroy();
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        popup.grab(self.seat.as_ref().expect("seat"), serial);
        self.surfaces[idx].popup = Some(popup);
        self.surfaces[idx].wl.commit();
        idx
    }

    /// A desynchronised subsurface with a buffer; `below` places it under its parent.
    pub fn subsurface(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        offset: (i32, i32),
        size: (i32, i32),
        below: bool,
    ) -> usize {
        let idx = self.subsurface_uncommitted(qh, label, parent, offset, size, below);
        self.draw(qh, idx);
        // position and stacking apply on the parent's commit
        self.surfaces[parent].wl.commit();
        idx
    }

    /// A desynchronised subsurface that has its role but no commit yet (neither its own nor
    /// its parent's): the instant a surface joins a tree.
    pub fn subsurface_uncommitted(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        offset: (i32, i32),
        size: (i32, i32),
        below: bool,
    ) -> usize {
        let idx = self.bare_surface(qh, label, size);
        self.make_subsurface(qh, idx, parent, offset, below);
        idx
    }

    /// A layer surface (L1): a role-less surface given the layer role on no particular output,
    /// with its state, committed without a buffer (the compositor answers with a configure, which
    /// the probe acknowledges; draw after it).
    #[allow(clippy::too_many_arguments)]
    pub fn layer_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        layer: zwlr_layer_shell_v1::Layer,
        anchor: zwlr_layer_surface_v1::Anchor,
        size: (u32, u32),
        exclusive_zone: i32,
        keyboard: zwlr_layer_surface_v1::KeyboardInteractivity,
    ) -> usize {
        let idx = self.bare_surface(qh, label, (0, 0));
        let wl = self.surfaces[idx].wl.clone();
        let ls = self
            .layer_shell
            .as_ref()
            .expect("zwlr_layer_shell_v1 bound")
            .get_layer_surface(&wl, None, layer, "probe".into(), qh, idx);
        ls.set_size(size.0, size.1);
        ls.set_anchor(anchor);
        ls.set_exclusive_zone(exclusive_zone);
        ls.set_keyboard_interactivity(keyboard);
        wl.commit();
        self.layer_surfaces.push((idx, ls));
        idx
    }

    /// Destroys a layer surface and its `wl_surface`, with the keyboard bookkeeping of
    /// [`Probe::destroy_toplevel`] (a `leave` may still be in flight for the dead surface).
    pub fn destroy_layer(&mut self, idx: usize) {
        if let Some(i) = self.layer_surfaces.iter().position(|(s, _)| *s == idx) {
            let (_, ls) = self.layer_surfaces.remove(i);
            ls.destroy();
        }
        let s = &mut self.surfaces[idx];
        s.wl.destroy();
        let label = s.label;
        if self.kb.active.as_deref() == Some(label) {
            self.kb.active = None;
            self.destroyed_active.push(label.to_string());
        }
        self.kb_log.push(format!("destroy_layer {label}"));
    }

    /// A popup of the layer surface `parent` (`xdg_surface.get_popup` with no parent, then
    /// `zwlr_layer_surface_v1.get_popup`), with `xdg_popup.grab` on `grab`'s serial, then its
    /// initial commit. Placement as [`Probe::popup`].
    pub fn layer_popup(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        parent: usize,
        spec: PopupSpec,
        grab: Option<u32>,
    ) -> usize {
        let PopupSpec {
            content,
            anchor_rect,
            slide,
            ..
        } = spec;
        let idx = self.new_xdg_surface(qh, label, content);
        let pos: XdgPositioner = self
            .wm_base
            .as_ref()
            .expect("xdg_wm_base")
            .create_positioner(qh, ());
        pos.set_size(content.0, content.1);
        pos.set_anchor_rect(anchor_rect.0, anchor_rect.1, anchor_rect.2, anchor_rect.3);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        if slide {
            pos.set_constraint_adjustment(
                xdg_positioner::ConstraintAdjustment::SlideX
                    | xdg_positioner::ConstraintAdjustment::SlideY,
            );
        }
        let xdg = self.surfaces[idx].xdg.clone().expect("xdg");
        let popup = xdg.get_popup(None, &pos, qh, idx);
        pos.destroy();
        self.layer_of(parent).get_popup(&popup);
        if let Some(serial) = grab {
            popup.grab(self.seat.as_ref().expect("seat"), serial);
        }
        self.surfaces[idx].popup = Some(popup);
        self.surfaces[idx].wl.commit();
        idx
    }

    /// The layer role object of surface `idx`.
    pub fn layer_of(&self, idx: usize) -> &ZwlrLayerSurfaceV1 {
        &self
            .layer_surfaces
            .iter()
            .find(|(i, _)| *i == idx)
            .expect("a layer surface")
            .1
    }

    /// A `wl_surface` with no role.
    pub fn bare_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        label: &'static str,
        size: (i32, i32),
    ) -> usize {
        let idx = self.surfaces.len();
        let wl = self
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_surface(qh, ());
        self.surfaces.push(Surf {
            label,
            wl,
            xdg: None,
            toplevel: None,
            popup: None,
            subsurface: None,
            size,
            configured: true,
            popup_configure: None,
            frame_driven: false,
        });
        idx
    }

    /// Gives a role-less surface the (desynchronised) subsurface role under `parent`; whatever
    /// subsurfaces it already has join the parent's tree with it.
    pub fn make_subsurface(
        &mut self,
        qh: &QueueHandle<Self>,
        idx: usize,
        parent: usize,
        offset: (i32, i32),
        below: bool,
    ) {
        let parent_wl = self.surfaces[parent].wl.clone();
        let sub = self
            .subcompositor
            .as_ref()
            .expect("wl_subcompositor")
            .get_subsurface(&self.surfaces[idx].wl, &parent_wl, qh, ());
        sub.set_position(offset.0, offset.1);
        if below {
            sub.place_below(&parent_wl);
        }
        sub.set_desync();
        self.surfaces[idx].subsurface = Some(sub);
    }

    /// Destroys the surface's `wp_fractional_scale_v1`; the surface itself stays.
    pub fn destroy_fractional_scale(&mut self, idx: usize) {
        self.fractional_scales.retain(|(i, fs)| {
            if *i == idx {
                fs.destroy();
            }
            *i != idx
        });
    }

    /// Every `wp_fractional_scale_v1.preferred_scale` the surface received, in order.
    pub fn preferred_scales_of(&self, label: &str) -> Vec<u32> {
        self.preferred_scales
            .iter()
            .filter(|(l, _)| l == label)
            .map(|(_, s)| *s)
            .collect()
    }

    /// Every `wl_surface.preferred_buffer_scale` the surface received, in order.
    pub fn preferred_buffer_scales_of(&self, label: &str) -> Vec<i32> {
        self.preferred_buffer_scales
            .iter()
            .filter(|(l, _)| l == label)
            .map(|(_, b)| *b)
            .collect()
    }

    /// `wl_surface.set_input_region` from add/subtract operations, then commit.
    pub fn input_region(&mut self, qh: &QueueHandle<Self>, idx: usize, ops: &[RegionRequest]) {
        let region: WlRegion = self
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_region(qh, ());
        for (add, r) in ops {
            if *add {
                region.add(r.0, r.1, r.2, r.3);
            } else {
                region.subtract(r.0, r.1, r.2, r.3);
            }
        }
        self.surfaces[idx].wl.set_input_region(Some(&region));
        region.destroy();
        self.surfaces[idx].wl.commit();
    }

    /// Attaches a null buffer and commits: the surface is unmapped.
    pub fn unmap(&mut self, idx: usize) {
        let wl = &self.surfaces[idx].wl;
        wl.attach(None, 0, 0);
        wl.commit();
    }

    pub fn destroy_toplevel(&mut self, idx: usize) {
        let s = &mut self.surfaces[idx];
        if let Some(t) = s.toplevel.take() {
            t.destroy();
        }
        if let Some(x) = s.xdg.take() {
            x.destroy();
        }
        s.wl.destroy();
        let label = s.label;
        if self.kb.active.as_deref() == Some(label) {
            self.kb.active = None;
            self.destroyed_active.push(label.to_string());
        }
        self.kb_log.push(format!("destroy_toplevel {label}"));
    }

    pub fn destroy_popup(&mut self, idx: usize) {
        let s = &mut self.surfaces[idx];
        if let Some(p) = s.popup.take() {
            p.destroy();
        }
        if let Some(x) = s.xdg.take() {
            x.destroy();
        }
        s.wl.destroy();
        let label = s.label;
        if self.kb.active.as_deref() == Some(label) {
            self.kb.active = None;
            self.destroyed_active.push(label.to_string());
        }
        self.kb_log.push(format!("destroy_popup {label}"));
    }

    /// Label of a client surface (for the wire log).
    fn label_of(&self, s: &WlSurface) -> String {
        self.surfaces
            .iter()
            .find(|x| &x.wl == s)
            .map(|x| x.label.to_string())
            .unwrap_or_else(|| format!("#{}", s.id().protocol_id()))
    }

    /// Global name of a bound wl_output.
    fn output_name(&self, o: &WlOutput) -> u32 {
        self.outputs
            .iter()
            .find(|(_, x)| x == o)
            .map(|(n, _)| *n)
            .unwrap_or(0)
    }

    /// Requests a frame callback and a presentation feedback (sequence number returned) for the
    /// next commit of the surface, then draws and commits it.
    pub fn frame_and_draw(&mut self, qh: &QueueHandle<Self>, idx: usize) -> u32 {
        self.surfaces[idx].wl.frame(qh, idx);
        self.feedback_seq += 1;
        let seq = self.feedback_seq;
        if let Some(p) = &self.presentation {
            p.feedback(&self.surfaces[idx].wl, qh, seq);
        }
        self.draw(qh, idx);
        seq
    }

    pub fn by_label(&self, label: &str) -> &Surf {
        self.surfaces
            .iter()
            .find(|s| s.label == label)
            .expect("label")
    }

    pub fn lock_pointer(&mut self, qh: &QueueHandle<Self>, idx: usize, oneshot: bool) {
        let lt = if oneshot {
            zwp_pointer_constraints_v1::Lifetime::Oneshot
        } else {
            zwp_pointer_constraints_v1::Lifetime::Persistent
        };
        let c = self.constraints.as_ref().expect("constraints bound");
        let l = c.lock_pointer(
            &self.surfaces[idx].wl,
            self.pointer.as_ref().expect("pointer bound"),
            None,
            lt,
            qh,
            (),
        );
        self.surfaces[idx].wl.commit();
        self.locked = Some(l);
    }

    pub fn lock_pointer_with_hint(
        &mut self,
        qh: &QueueHandle<Self>,
        idx: usize,
        oneshot: bool,
        hint: (f64, f64),
    ) {
        let lt = if oneshot {
            zwp_pointer_constraints_v1::Lifetime::Oneshot
        } else {
            zwp_pointer_constraints_v1::Lifetime::Persistent
        };
        let c = self.constraints.as_ref().expect("constraints bound");
        let l = c.lock_pointer(
            &self.surfaces[idx].wl,
            self.pointer.as_ref().expect("pointer bound"),
            None,
            lt,
            qh,
            (),
        );
        l.set_cursor_position_hint(hint.0, hint.1);
        self.surfaces[idx].wl.commit();
        self.locked = Some(l);
    }

    pub fn confine_pointer(&mut self, qh: &QueueHandle<Self>, idx: usize) {
        let c = self.constraints.as_ref().expect("constraints bound");
        let cf = c.confine_pointer(
            &self.surfaces[idx].wl,
            self.pointer.as_ref().expect("pointer bound"),
            None,
            zwp_pointer_constraints_v1::Lifetime::Persistent,
            qh,
            (),
        );
        self.surfaces[idx].wl.commit();
        self.confined = Some(cf);
    }

    pub fn confine_pointer_with_region(
        &mut self,
        qh: &QueueHandle<Self>,
        idx: usize,
        region: (i32, i32, i32, i32),
    ) {
        let comp = self.compositor.as_ref().expect("compositor");
        let r = comp.create_region(qh, ());
        r.add(region.0, region.1, region.2, region.3);
        let c = self.constraints.as_ref().expect("constraints bound");
        let cf = c.confine_pointer(
            &self.surfaces[idx].wl,
            self.pointer.as_ref().expect("pointer bound"),
            Some(&r),
            zwp_pointer_constraints_v1::Lifetime::Persistent,
            qh,
            (),
        );
        self.surfaces[idx].wl.commit();
        r.destroy();
        self.confined = Some(cf);
    }

    pub fn destroy_lock(&mut self) {
        if let Some(l) = self.locked.take() {
            l.destroy();
        }
    }

    pub fn destroy_confinement(&mut self) {
        if let Some(cf) = self.confined.take() {
            cf.destroy();
        }
    }
}

impl Dispatch<WlRegistry, ()> for Probe {
    fn event(
        st: &mut Self,
        registry: &WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match ev {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == "wl_output" {
                    st.log.push(format!("global wl_output {name}"));
                    let o: WlOutput = registry.bind(name, version.min(4), qh, ());
                    if let Some(m) = &st.xdg_output_manager {
                        m.get_xdg_output(&o, qh, name);
                    }
                    st.outputs.push((name, o));
                }
                st.globals.push((name, interface, version));
            }
            wl_registry::Event::GlobalRemove { name } => {
                if st.outputs.iter().any(|(n, _)| *n == name) {
                    st.log.push(format!("global_remove {name}"));
                    st.outputs.retain(|(n, o)| {
                        if *n == name {
                            o.release();
                        }
                        *n != name
                    });
                }
                st.globals.retain(|(n, _, _)| *n != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for Probe {
    fn event(
        st: &mut Self,
        o: &WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let n = st.output_name(o);
        let line = match ev {
            wl_output::Event::Mode {
                width,
                height,
                refresh,
                ..
            } => format!("{n}: mode {width}x{height}@{refresh}"),
            wl_output::Event::Scale { factor } => format!("{n}: scale {factor}"),
            wl_output::Event::Geometry { transform, .. } => {
                format!("{n}: geometry transform {transform:?}")
            }
            wl_output::Event::Done => format!("{n}: done"),
            wl_output::Event::Name { name } => format!("{n}: name {name}"),
            _ => return,
        };
        st.log.push(line);
    }
}

impl Dispatch<ZxdgOutputV1, u32> for Probe {
    fn event(
        st: &mut Self,
        _: &ZxdgOutputV1,
        ev: zxdg_output_v1::Event,
        n: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                st.log.push(format!("{n}: xdg position {x},{y}"))
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                st.log.push(format!("{n}: xdg size {width},{height}"))
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSurface, ()> for Probe {
    fn event(
        st: &mut Self,
        s: &WlSurface,
        ev: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let label = st.label_of(s);
        match ev {
            wl_surface::Event::Enter { output } => {
                let n = st.output_name(&output);
                st.log.push(format!("enter {label} {n}"));
            }
            wl_surface::Event::Leave { output } => {
                let n = st.output_name(&output);
                st.log.push(format!("leave {label} {n}"));
            }
            wl_surface::Event::PreferredBufferScale { factor } => {
                st.log
                    .push(format!("preferred_buffer_scale {label} {factor}"));
                st.scale_log
                    .push(format!("preferred_buffer_scale {label} {factor}"));
                st.preferred_buffer_scales.push((label, factor));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, usize> for Probe {
    fn event(
        st: &mut Self,
        _: &WlCallback,
        ev: wl_callback::Event,
        idx: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = ev {
            let label = st.surfaces[*idx].label;
            st.log.push(format!("frame {label}"));
            st.wire.push(format!("frame {label}"));
            st.frames.push((st.now, label));
            if st.surfaces[*idx].frame_driven {
                st.surfaces[*idx].wl.frame(qh, *idx);
                st.draw(qh, *idx);
            }
        }
    }
}

impl Dispatch<WpPresentation, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &WpPresentation,
        _: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpPresentationFeedback, u32> for Probe {
    fn event(
        st: &mut Self,
        _: &WpPresentationFeedback,
        ev: wp_presentation_feedback::Event,
        seq: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            wp_presentation_feedback::Event::SyncOutput { output } => {
                let n = st.output_name(&output);
                st.log.push(format!("sync #{seq} {n}"));
            }
            wp_presentation_feedback::Event::Presented { .. } => {
                st.log.push(format!("presented #{seq}"))
            }
            wp_presentation_feedback::Event::Discarded => st.log.push(format!("discarded #{seq}")),
            _ => {}
        }
    }
}

delegate_noop!(Probe: ignore ZxdgOutputManagerV1);
delegate_noop!(Probe: ignore WpFractionalScaleManagerV1);
delegate_noop!(Probe: ignore WpViewporter);
delegate_noop!(Probe: ignore WpViewport);

impl Dispatch<WpFractionalScaleV1, usize> for Probe {
    fn event(
        st: &mut Self,
        _: &WpFractionalScaleV1,
        ev: wp_fractional_scale_v1::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev {
            let label = st.surfaces[*idx].label;
            st.log.push(format!("preferred_scale {label} {scale}"));
            st.scale_log
                .push(format!("preferred_scale {label} {scale}"));
            st.preferred_scales.push((label.to_string(), scale));
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Probe {
    fn event(
        _: &mut Self,
        wm: &XdgWmBase,
        ev: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, usize> for Probe {
    fn event(
        st: &mut Self,
        xdg: &XdgSurface,
        ev: xdg_surface::Event,
        idx: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = ev {
            st.configures.push(st.surfaces[*idx].label);
            st.scale_log
                .push(format!("configure {}", st.surfaces[*idx].label));
            if st.hold_acks {
                st.held_acks.push((*idx, serial));
                return;
            }
            xdg.ack_configure(serial);
            if !st.surfaces[*idx].configured {
                st.surfaces[*idx].configured = true;
                st.draw(qh, *idx);
            }
        }
    }
}

impl Dispatch<XdgToplevel, usize> for Probe {
    fn event(
        st: &mut Self,
        _: &XdgToplevel,
        ev: xdg_toplevel::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let words = |bytes: &[u8]| -> Vec<u32> {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c))
                .collect()
        };
        if let xdg_toplevel::Event::Close = ev {
            st.close_requests.push(*idx);
            return;
        }
        match ev {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let label = st.surfaces[*idx].label;
                let raw_states = words(&states);
                let is_activated = raw_states.contains(&4);
                st.toplevel_configures
                    .push((*idx, (width, height), raw_states.clone()));
                st.toplevel_states.push((*idx, raw_states));
                st.kb_log.push(format!(
                    "toplevel_configure {label} activated={is_activated}"
                ));
                st.wire.push(format!("configure {label}"));
            }
            xdg_toplevel::Event::WmCapabilities { capabilities } => {
                st.wm_capabilities.push((*idx, words(&capabilities)));
            }
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                st.configure_bounds.push((*idx, (width, height)));
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgPopup, usize> for Probe {
    fn event(
        st: &mut Self,
        _: &XdgPopup,
        ev: xdg_popup::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            xdg_popup::Event::Configure {
                x,
                y,
                width,
                height,
            } => {
                st.surfaces[*idx].popup_configure = Some((x, y, width, height));
            }
            xdg_popup::Event::PopupDone => {
                let label = st.surfaces[*idx].label;
                st.popup_dones.push(label);
                st.wire.push(format!("popup_done {label}"));
                st.kb_log.push(format!("popup_done {label}"));
                if st.auto_destroy_popups && st.surfaces[*idx].popup.is_some() {
                    st.destroy_popup(*idx);
                }
            }
            _ => {}
        }
    }
}

impl Probe {
    /// A global this client was told about: `(name, version)`.
    pub fn global(&self, interface: &str) -> Option<(u32, u32)> {
        self.globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .map(|(n, _, v)| (*n, *v))
    }

    /// Binds `wl_data_device_manager` (v3) and creates this client's data device.
    pub fn bind_clipboard(&mut self, registry: &WlRegistry, qh: &QueueHandle<Self>) {
        let (n, v) = self
            .global("wl_data_device_manager")
            .expect("wl_data_device_manager is offered");
        let manager: WlDataDeviceManager = registry.bind(n, v.min(3), qh, ());
        let device = manager.get_data_device(self.seat.as_ref().expect("seat"), qh, ());
        self.clipboard.device = Some(device);
        self.data_device_manager = Some(manager);
    }

    /// Offers `mimes` as the clipboard selection with `serial`; `bytes` is what is sent.
    pub fn copy(&mut self, qh: &QueueHandle<Self>, mimes: &[&str], bytes: &[u8], serial: u32) {
        let manager = self.data_device_manager.as_ref().expect("bound");
        let source = manager.create_data_source(qh, ());
        for m in mimes {
            source.offer(m.to_string());
        }
        self.clipboard.source_bytes = bytes.to_vec();
        self.clipboard
            .device
            .as_ref()
            .expect("device")
            .set_selection(Some(&source), serial);
        self.clipboard.sources.push(source);
    }

    /// Asks the current selection for `mime`; returns the read end of the pipe.
    pub fn paste(&mut self, mime: &str) -> std::io::PipeReader {
        let (offer, _) = self
            .clipboard
            .selection
            .as_ref()
            .expect("a selection offer");
        let (reader, writer) = std::io::pipe().expect("pipe");
        offer.receive(mime.to_string(), writer.as_fd());
        drop(writer);
        reader
    }
}

impl Dispatch<WlDataDevice, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlDataDevice,
        ev: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            wl_data_device::Event::DataOffer { id } => {
                st.clipboard
                    .log
                    .push(format!("offer {}", id.id().protocol_id()));
                st.clipboard.pending.push((id, Vec::new()));
            }
            wl_data_device::Event::Selection { id } => {
                st.clipboard.selection = id.and_then(|id| {
                    let i = st.clipboard.pending.iter().position(|(o, _)| *o == id)?;
                    Some(st.clipboard.pending.swap_remove(i))
                });
                let line = match &st.clipboard.selection {
                    Some((o, _)) => format!("selection {}", o.id().protocol_id()),
                    None => "selection none".to_string(),
                };
                st.clipboard.log.push(line);
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(Probe, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlDataOffer, ()> for Probe {
    fn event(
        st: &mut Self,
        offer: &WlDataOffer,
        ev: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = ev {
            st.clipboard
                .log
                .push(format!("mime {} {mime_type}", offer.id().protocol_id()));
            if let Some((_, mimes)) = st.clipboard.pending.iter_mut().find(|(o, _)| o == offer) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<WlDataSource, ()> for Probe {
    fn event(
        st: &mut Self,
        source: &WlDataSource,
        ev: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            wl_data_source::Event::Send { mime_type, fd } => {
                st.clipboard.log.push(format!("send {mime_type}"));
                let mut file = File::from(fd);
                let _ = file.write_all(&st.clipboard.source_bytes);
            }
            wl_data_source::Event::Cancelled => {
                st.clipboard.log.push("cancelled".to_string());
                st.clipboard.sources.retain(|s| s != source);
                source.destroy();
            }
            _ => {}
        }
    }
}

delegate_noop!(Probe: ignore WlDataDeviceManager);
delegate_noop!(Probe: ignore ZwpPrimarySelectionDeviceManagerV1);

impl Probe {
    /// Binds the primary selection manager and creates this client's device.
    pub fn bind_primary(&mut self, registry: &WlRegistry, qh: &QueueHandle<Self>) {
        let (n, _) = self
            .global("zwp_primary_selection_device_manager_v1")
            .expect("primary selection is offered");
        let manager: ZwpPrimarySelectionDeviceManagerV1 = registry.bind(n, 1, qh, ());
        let device = manager.get_device(self.seat.as_ref().expect("seat"), qh, ());
        self.primary = Some((manager, device));
    }

    pub fn copy_primary(&mut self, qh: &QueueHandle<Self>, mime: &str, bytes: &[u8], serial: u32) {
        let (manager, device) = self.primary.as_ref().expect("bound");
        let source = manager.create_source(qh, ());
        source.offer(mime.to_string());
        self.primary_bytes = bytes.to_vec();
        device.set_selection(Some(&source), serial);
    }

    pub fn paste_primary(&mut self, mime: &str) -> std::io::PipeReader {
        let (offer, _) = self.primary_selection.as_ref().expect("a primary offer");
        let (reader, writer) = std::io::pipe().expect("pipe");
        offer.receive(mime.to_string(), writer.as_fd());
        drop(writer);
        reader
    }
}

impl Dispatch<ZwpPrimarySelectionDeviceV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpPrimarySelectionDeviceV1,
        ev: zwp_primary_selection_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_primary_selection_device_v1::Event::DataOffer { offer } => {
                st.primary_pending.push((offer, Vec::new()));
            }
            zwp_primary_selection_device_v1::Event::Selection { id } => {
                st.primary_selection = id.and_then(|id| {
                    let i = st.primary_pending.iter().position(|(o, _)| *o == id)?;
                    Some(st.primary_pending.swap_remove(i))
                });
            }
            _ => {}
        }
    }

    wayland_client::event_created_child!(Probe, ZwpPrimarySelectionDeviceV1, [
        zwp_primary_selection_device_v1::EVT_DATA_OFFER_OPCODE => (ZwpPrimarySelectionOfferV1, ()),
    ]);
}

impl Dispatch<ZwpPrimarySelectionOfferV1, ()> for Probe {
    fn event(
        st: &mut Self,
        offer: &ZwpPrimarySelectionOfferV1,
        ev: zwp_primary_selection_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_offer_v1::Event::Offer { mime_type } = ev
            && let Some((_, m)) = st.primary_pending.iter_mut().find(|(o, _)| o == offer)
        {
            m.push(mime_type);
        }
    }
}

impl Dispatch<ZwpPrimarySelectionSourceV1, ()> for Probe {
    fn event(
        st: &mut Self,
        source: &ZwpPrimarySelectionSourceV1,
        ev: zwp_primary_selection_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_primary_selection_source_v1::Event::Send { fd, .. } => {
                let mut file = File::from(fd);
                let _ = file.write_all(&st.primary_bytes);
            }
            zwp_primary_selection_source_v1::Event::Cancelled => source.destroy(),
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlPointer, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlPointer,
        ev: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let line = match ev {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                st.last_serial = serial;
                format!("enter {} {surface_x},{surface_y}", st.label_of(&surface))
            }
            wl_pointer::Event::Leave { surface, .. } => format!("leave {}", st.label_of(&surface)),
            wl_pointer::Event::Motion {
                time,
                surface_x,
                surface_y,
            } => {
                st.ptr_times.push(("motion", time));
                format!("motion {surface_x},{surface_y}")
            }
            wl_pointer::Event::Button {
                serial,
                time,
                button,
                state,
            } => {
                st.ptr_times.push(("button", time));
                st.last_serial = serial;
                let s = match state.into_result() {
                    Ok(wl_pointer::ButtonState::Pressed) => "pressed",
                    _ => "released",
                };
                format!("button {button} {s}")
            }
            wl_pointer::Event::Axis { time, axis, value } => {
                st.ptr_times.push(("axis", time));
                format!("axis {axis:?} {value}")
            }
            wl_pointer::Event::AxisValue120 { axis, value120 } => {
                format!("axis_v120 {axis:?} {value120}")
            }
            wl_pointer::Event::Frame => "frame".to_string(),
            _ => return,
        };
        st.wire.push(format!("pointer {line}"));
        st.ptr.push(line);
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for Probe {
    fn event(
        st: &mut Self,
        ls: &ZwlrLayerSurfaceV1,
        ev: zwlr_layer_surface_v1::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                ls.ack_configure(serial);
                st.surfaces[*idx].size = (width as i32, height as i32);
                st.surfaces[*idx].configured = true;
                st.layer_configures.push((*idx, (width, height)));
            }
            zwlr_layer_surface_v1::Event::Closed => st.layer_closed.push(*idx),
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &WlKeyboard,
        ev: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            wl_keyboard::Event::Keymap { fd, .. } => {
                // fd is an OwnedFd. Dropping it closes the shm file descriptor immediately.
                drop(fd);
            }
            wl_keyboard::Event::Enter {
                serial,
                surface,
                keys,
            } => {
                st.last_serial = serial;
                let label = st.label_of(&surface);
                if st.kb.active.is_some() {
                    st.kb.violations.push(format!(
                        "enter({label}) while already active on {:?}",
                        st.kb.active
                    ));
                }
                st.kb.active = Some(label.clone());
                st.kb.keys = keys
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_ne_bytes(*c))
                    .collect();
                st.kb.awaiting_modifiers = true;
                st.kb_log
                    .push(format!("wl_keyboard.enter {label} keys={:?}", st.kb.keys));
                st.wire.push(format!("kb_enter {label}"));
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                let label = st.label_of(&surface);
                if st.kb.active.is_none() {
                    if let Some(pos) = st.destroyed_active.iter().position(|x| x == &label) {
                        let destroyed = st.destroyed_active.remove(pos);
                        st.kb_log.push(format!(
                            "wl_keyboard.leave {destroyed} (in-flight after destroy)"
                        ));
                        return;
                    } else if surface.id().protocol_id() == 0 && !st.destroyed_active.is_empty() {
                        let destroyed = st.destroyed_active.pop().unwrap();
                        st.kb_log.push(format!(
                            "wl_keyboard.leave {destroyed} (in-flight after destroy)"
                        ));
                        return;
                    }
                }
                if st.kb.active.as_deref() != Some(label.as_str()) {
                    st.kb
                        .violations
                        .push(format!("leave({label}) but active={:?}", st.kb.active));
                }
                if st.kb.awaiting_modifiers {
                    st.kb
                        .violations
                        .push("leave before modifiers following enter".into());
                }
                st.kb.active = None;
                st.kb.keys.clear();
                st.kb.depressed = 0;
                st.kb.latched = 0;
                st.kb.locked = 0;
                st.kb.awaiting_modifiers = false;
                st.kb_log.push(format!("wl_keyboard.leave {label}"));
                st.wire.push(format!("kb_leave {label}"));
            }
            wl_keyboard::Event::Key {
                serial, key, state, ..
            } => {
                st.last_serial = serial;
                let pressed = matches!(
                    state,
                    wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed)
                );
                if st.kb.active.is_none() {
                    st.kb
                        .violations
                        .push(format!("key {key} without active surface"));
                }
                if st.kb.awaiting_modifiers {
                    st.kb
                        .violations
                        .push("key before modifiers following enter".into());
                }
                if pressed && !st.kb.keys.insert(key) {
                    st.kb
                        .violations
                        .push(format!("pressed {key} already logically down"));
                }
                if !pressed && !st.kb.keys.remove(&key) {
                    st.kb
                        .violations
                        .push(format!("released {key} not logically down"));
                }
                st.key_events.push((key, pressed));
                st.wire.push(format!("key {key}"));
                st.kb_log.push(format!(
                    "wl_keyboard.key {key} {}",
                    if pressed { "pressed" } else { "released" }
                ));
            }
            wl_keyboard::Event::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                st.last_serial = serial;
                st.kb.awaiting_modifiers = false;
                st.kb.depressed = mods_depressed;
                st.kb.latched = mods_latched;
                st.kb.locked = mods_locked;
                st.kb.group = group;
                st.kb_log.push(format!(
                    "wl_keyboard.modifiers dep={mods_depressed:#x} lat={mods_latched:#x} lock={mods_locked:#x}"
                ));
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                st.repeat_info = Some((rate, delay));
                st.kb_log
                    .push(format!("wl_keyboard.repeat_info rate={rate} delay={delay}"));
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpRelativePointerV1,
        ev: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion {
            dx,
            dy,
            dx_unaccel,
            dy_unaccel,
            ..
        } = ev
        {
            st.ptr.push(format!(
                "relative {dx},{dy} unaccel {dx_unaccel},{dy_unaccel}"
            ));
        }
    }
}

impl Dispatch<ZwpLockedPointerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpLockedPointerV1,
        ev: zwp_locked_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_locked_pointer_v1::Event::Locked => st.ptr.push("locked".into()),
            zwp_locked_pointer_v1::Event::Unlocked => st.ptr.push("unlocked".into()),
            _ => {}
        }
    }
}

impl Dispatch<ZwpConfinedPointerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwpConfinedPointerV1,
        ev: zwp_confined_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_confined_pointer_v1::Event::Confined => st.ptr.push("confined".into()),
            zwp_confined_pointer_v1::Event::Unconfined => st.ptr.push("unconfined".into()),
            _ => {}
        }
    }
}

delegate_noop!(Probe: ignore ZwpPointerConstraintsV1);
delegate_noop!(Probe: ignore ZwpRelativePointerManagerV1);
delegate_noop!(Probe: ignore WlCompositor);
delegate_noop!(Probe: ignore XdgActivationV1);

impl Dispatch<XdgActivationTokenV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &XdgActivationTokenV1,
        ev: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token } = ev {
            st.activation_tokens.push(token);
        }
    }
}
delegate_noop!(Probe: ignore WlSubcompositor);
delegate_noop!(Probe: ignore WlShm);
delegate_noop!(Probe: ignore WlShmPool);
delegate_noop!(Probe: ignore WlBuffer);
delegate_noop!(Probe: ignore WlSubsurface);
delegate_noop!(Probe: ignore WlRegion);
delegate_noop!(Probe: ignore XdgPositioner);

/// The production stack plus one probe client. The test is the composition root: it holds the
/// output-lifecycle token and connects virtual outputs through the backend. Time is virtual: each
/// connected output has a vblank every period, processed in time order (ties: lowest OutputId).
pub struct Lockstep {
    pub display: Display<WaylandState>,
    pub state: WaylandState,
    pub token: OutputLifecycleToken,
    /// The virtual input device's port (the test is the composition root).
    pub input: InputPort,
    /// The shell's port (INC-11): a test drives `shell_command` as the shell server does.
    pub shell: ShellPort,
    pub output: Option<OutputId>,
    pub conn: Connection,
    pub queue: EventQueue<Probe>,
    pub probe: Probe,
    pub qh: QueueHandle<Probe>,
    pub now: Duration,
    /// (output, next vblank time)
    vblanks: Vec<(OutputId, Duration)>,
    /// The client's single wl_registry (a second one would bind every output twice).
    registry: WlRegistry,
    client: smithay::reexports::wayland_server::Client,
}

impl Lockstep {
    pub fn new(output: Option<VirtualOutput>) -> Lockstep {
        Lockstep::with(PersistedCameras::default(), output.as_slice())
    }

    /// A Core with a persisted camera table and the given outputs connected at startup.
    pub fn with(persisted: PersistedCameras, outputs: &[VirtualOutput]) -> Lockstep {
        let (core, input, shell, token) = Core::new(persisted);
        let display: Display<WaylandState> = Display::new().expect("display");
        let mut state = WaylandState::new(&display.handle(), core);
        // as the run loop does: the ceiling is what the renderer reports (OB-13)
        state
            .report_renderer_limits(&infynitae_backend_headless::HeadlessRenderer::default())
            .expect("the fixture renderer reports its limit");
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end
            .set_nonblocking(true)
            .expect("non-blocking client socket");
        let client = display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert client");
        let conn = Connection::from_socket(client_end).expect("client connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        let mut ls = Lockstep {
            display,
            state,
            token,
            input,
            shell,
            output: None,
            conn,
            queue,
            probe: Probe::default(),
            qh,
            now: Duration::ZERO,
            vblanks: Vec::new(),
            registry,
            client,
        };
        // bind the managers first so wl_output globals get their xdg_output at announcement
        ls.pump();
        ls.bind();
        for o in outputs {
            let id = ls.connect(o);
            ls.output.get_or_insert(id);
        }
        ls.pump();
        ls
    }

    /// Hotplug (or startup) of a virtual output through the token.
    pub fn connect(&mut self, o: &VirtualOutput) -> OutputId {
        let id = connect(&mut self.state, &mut self.token, o).expect("connect output");
        self.vblanks
            .push((id, self.now + vblank_period(o.refresh_mhz)));
        self.pump();
        id
    }

    pub fn reconfigure(&mut self, id: OutputId, o: &VirtualOutput) {
        infynitae_backend_headless::reconfigure(&mut self.state, &mut self.token, id, o)
            .expect("reconfigure");
        self.pump();
    }

    pub fn disconnect(&mut self, id: OutputId) {
        infynitae_backend_headless::disconnect(&mut self.state, &mut self.token, id)
            .expect("disconnect");
        self.vblanks.retain(|(o, _)| *o != id);
        self.pump();
    }

    /// Runs every vblank and every off-screen fallback (M16) due up to `t` in time order,
    /// pumping after each. A fallback due at a vblank's time runs after it.
    pub fn run_until(&mut self, t: Duration) {
        loop {
            let next = self
                .vblanks
                .iter()
                .copied()
                .filter(|(_, at)| *at <= t)
                .min_by_key(|(o, at)| (*at, *o));
            let fallback = self
                .state
                .core()
                .frame_fallback_deadline()
                .map(|d| d.max(self.now))
                .filter(|d| *d <= t && next.is_none_or(|(_, at)| *d < at));
            if let Some(at) = fallback {
                self.now = at;
                self.pump();
                continue;
            }
            let Some((o, at)) = next else { break };
            self.now = at;
            vblank(&mut self.state, o, at);
            let refresh = self
                .state
                .core()
                .output(o)
                .map(|m| m.refresh_mhz)
                .unwrap_or(60_000);
            for v in self.vblanks.iter_mut().filter(|(x, _)| *x == o) {
                v.1 = at + vblank_period(refresh);
            }
            self.pump();
        }
        self.now = t;
    }

    fn bind(&mut self) {
        let registry = self.registry.clone();
        let find = |name: &str| {
            self.probe
                .globals
                .iter()
                .find(|(_, i, _)| i == name)
                .map(|(n, _, v)| (*n, *v))
                .unwrap_or_else(|| panic!("global {name} missing"))
        };
        let (n, _) = find("wl_compositor");
        self.probe.compositor = Some(registry.bind(n, 6, &self.qh, ()));
        let (n, _) = find("wl_subcompositor");
        self.probe.subcompositor = Some(registry.bind(n, 1, &self.qh, ()));
        let (n, _) = find("wl_shm");
        self.probe.shm = Some(registry.bind(n, 1, &self.qh, ()));
        let (n, v) = find("xdg_wm_base");
        self.probe.wm_base = Some(registry.bind(n, v.min(6), &self.qh, ()));
        let (n, v) = find("zxdg_output_manager_v1");
        self.probe.xdg_output_manager = Some(registry.bind(n, v.min(3), &self.qh, ()));
        let (n, _) = find("wp_presentation");
        self.probe.presentation = Some(registry.bind(n, 1, &self.qh, ()));
        let (n, v) = find("wl_seat");
        let seat: WlSeat = registry.bind(n, v.min(9), &self.qh, ());
        let pointer = seat.get_pointer(&self.qh, ());
        let keyboard = seat.get_keyboard(&self.qh, ());
        let (n, _) = find("zwp_relative_pointer_manager_v1");
        let manager: ZwpRelativePointerManagerV1 = registry.bind(n, 1, &self.qh, ());
        self.probe.relative = Some(manager.get_relative_pointer(&pointer, &self.qh, ()));
        let (n, _) = find("zwp_pointer_constraints_v1");
        self.probe.constraints = Some(registry.bind(n, 1, &self.qh, ()));
        if let Some((n, v)) = self
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "wp_fractional_scale_manager_v1")
            .map(|(n, _, v)| (*n, *v))
        {
            self.probe.fractional_scale_manager = Some(registry.bind(n, v.min(1), &self.qh, ()));
        }
        if let Some((n, v)) = self
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "wp_viewporter")
            .map(|(n, _, v)| (*n, *v))
        {
            self.probe.viewporter = Some(registry.bind(n, v.min(1), &self.qh, ()));
        }
        self.probe.seat = Some(seat);
        self.probe.pointer = Some(pointer);
        self.probe.keyboard = Some(keyboard);
        self.pump();
    }

    /// The client's single registry.
    pub fn registry(&self) -> &WlRegistry {
        &self.registry
    }

    /// Binds `zwlr_layer_shell_v1` (L1–L8).
    pub fn bind_layer_shell(&mut self) {
        let (n, v, _) = self
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwlr_layer_shell_v1")
            .map(|(n, i, v)| (*n, *v, i.clone()))
            .expect("zwlr_layer_shell_v1 is offered");
        self.probe.layer_shell = Some(self.registry.bind(n, v.min(4), &self.qh, ()));
        self.pump();
    }

    /// Binds `xdg_activation_v1` (INC-12).
    pub fn activation(&mut self) -> XdgActivationV1 {
        let (n, _, _) = self
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "xdg_activation_v1")
            .expect("xdg_activation_v1 is offered")
            .clone();
        self.registry.bind(n, 1, &self.qh, ())
    }

    /// Exchanges messages until both sides are quiescent. Like a backend's loop iteration,
    /// every round ends with the off-screen fallback evaluation at the current virtual time.
    pub fn pump(&mut self) {
        let mut idle = 0;
        for _ in 0..10_000 {
            let served = self
                .display
                .dispatch_clients(&mut self.state)
                .expect("server dispatch");
            // as the session does after each dispatch
            self.state.settle_unlock();
            self.state.clear_orphaned_selections();
            self.state.refuse_client_drags();
            frame_fallback(&mut self.state, self.now);
            self.display.flush_clients().expect("server flush");
            self.conn.flush().expect("client flush");
            if let Some(guard) = self.conn.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("client read: {e}"),
                }
            }
            self.probe.now = self.now;
            let handled = self
                .queue
                .dispatch_pending(&mut self.probe)
                .expect("client dispatch");
            self.conn.flush().expect("client flush");
            if served == 0 && handled == 0 {
                idle += 1;
                if idle >= 3 {
                    return;
                }
            } else {
                idle = 0;
            }
        }
        panic!("lockstep did not settle");
    }

    pub fn output(&self) -> OutputId {
        self.output.expect("an output was connected")
    }
}

impl Drop for Lockstep {
    fn drop(&mut self) {
        self.display.handle().backend_handle().kill_client(
            self.client.id(),
            smithay::reexports::wayland_server::backend::DisconnectReason::ConnectionClosed,
        );
        let _ = self
            .display
            .backend()
            .dispatch_single_client(&mut self.state, self.client.id());
        let _ = self.display.flush_clients();
    }
}

/// The relative motion that takes the pointer from where Core has it to `local` (logical px) of
/// `element`'s window geometry, through the view of the pointer's output. A test that needs the
/// pointer over a particular place says so with this instead of relying on where an earlier
/// motion happened to leave it; it then checks the result with [`hit_under_pointer`].
pub fn delta_to(ls: &Lockstep, element: ElementId, local: (f64, f64)) -> (f64, f64) {
    use infynitae_core::geom::{CanvasPos, Q16_ONE};
    let core = ls.state.core();
    let PointerState::Located { output, p } = core.pointer() else {
        panic!("the pointer has no output");
    };
    let anchor = core.stage().anchor(element).expect("the element is placed");
    let view = core.view(output).expect("the pointer's output has a view");
    let q = |a: i64, l: f64| a * Q16_ONE + (l * Q16_ONE as f64).round() as i64;
    let target = view.project(CanvasPos {
        x: q(anchor.x, local.0),
        y: q(anchor.y, local.1),
    });
    (target.x - p.0, target.y - p.1)
}

/// What the production hit-test finds under the pointer: the element and the point local to its
/// window geometry (logical px, rounded to 1/256 as a client would receive it).
pub fn hit_under_pointer(ls: &mut Lockstep) -> Option<(ElementId, (f64, f64))> {
    use infynitae_core::geom::{OutputLogicalPoint, Q16_ONE};
    let PointerState::Located { output, p } = ls.state.core().pointer() else {
        return None;
    };
    let hit = ls
        .state
        .core_mut()
        .hit_test(output, OutputLogicalPoint { x: p.0, y: p.1 })?;
    let anchor = ls.state.core().stage().anchor(hit.element)?;
    let wl_fixed = |v: f64| (v * 256.0).round() / 256.0;
    let local =
        |origin: i64, l: i64, a: i64| wl_fixed((origin - a) as f64 + l as f64 / Q16_ONE as f64);
    Some((
        hit.element,
        (
            local(hit.origin.x, hit.local.x, anchor.x),
            local(hit.origin.y, hit.local.y, anchor.y),
        ),
    ))
}

/// Asserts that the pointer is over some window at `local` of its window geometry.
pub fn assert_pointer_on_a_window_at(ls: &mut Lockstep, local: (f64, f64)) {
    assert_eq!(
        hit_under_pointer(ls).map(|(_, l)| l),
        Some(local),
        "the pointer is not at the window-local position the scenario needs"
    );
}

/// Asserts that the pointer is over `element` at `local` of its window geometry.
pub fn assert_pointer_at(ls: &mut Lockstep, element: ElementId, local: (f64, f64), what: &str) {
    assert_eq!(
        hit_under_pointer(ls),
        Some((element, local)),
        "{what}: the pointer is not where the scenario needs it"
    );
}

pub fn output(mode: (u32, u32), scale_120: u32) -> VirtualOutput {
    output_k("A", mode, scale_120, 60_000)
}

/// A virtual output on a named connector.
pub fn output_k(
    connector: &str,
    mode: (u32, u32),
    scale_120: u32,
    refresh_mhz: u32,
) -> VirtualOutput {
    VirtualOutput {
        key: VirtualOutput::key(connector),
        mode,
        transform: Transform::Normal,
        scale_120,
        refresh_mhz,
    }
}

/// The xsnap/xhit tree (TST-09, TST-10). W: content 400x300 with a 20 px shadow; input region =
/// window geometry minus a hole over S2; S1 above at (37,11) with S1a nested at (10,10); S2 below
/// the root at (−15,50); popup P (shadow 10) anchored at (100,50); PS inside P at (5,5).
pub fn build_xhit_tree(ls: &mut Lockstep, content: (i32, i32), hole: bool) {
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", content, 20);
    ls.pump();
    let mut ops = vec![(true, (20, 20, content.0, content.1))];
    if hole {
        ops.push((false, (20, 55, 20, 30)));
    }
    ls.probe.input_region(&qh, w, &ops);
    let s1 = ls
        .probe
        .subsurface(&qh, "S1", w, (37, 11), (101, 53), false);
    ls.probe
        .subsurface(&qh, "S1a", s1, (10, 10), (30, 20), false);
    ls.probe.surfaces[s1].wl.commit();
    ls.probe.subsurface(&qh, "S2", w, (-15, 50), (60, 40), true);
    ls.pump();
    let p = ls.probe.popup(
        &qh,
        "P",
        w,
        PopupSpec {
            content: (120, 80),
            shadow: 10,
            anchor_rect: (100, 50, 1, 1),
            slide: false,
        },
    );
    ls.pump();
    ls.probe.subsurface(&qh, "PS", p, (5, 5), (30, 30), false);
    ls.pump();
}
