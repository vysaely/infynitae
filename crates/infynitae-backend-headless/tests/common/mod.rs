//! T1 lockstep harness (FOUNDATION_TEST_PLAN T1): the production core, adapter and headless
//! backend in one thread with a recording probe client over a socketpair. Deterministic, no real
//! time, no host compositor, no socket on disk, no `unsafe`.

#![allow(dead_code)]

pub mod input;
pub mod oracle;
pub mod probe_renderer;

use std::fs::File;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use infynitae_backend_headless::{VirtualOutput, connect, vblank, vblank_period};
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputId, Transform};
use infynitae_core::{Core, InputPort, OutputLifecycleToken};
use infynitae_wayland::{ClientState, WaylandState};
use smithay::reexports::wayland_server::Display;
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
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
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::{self, WpPresentation},
    wp_presentation_feedback::{self, WpPresentationFeedback},
};
use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
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

#[derive(Default)]
pub struct Probe {
    globals: Vec<(u32, String, u32)>,
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
    /// Ordered wire log of the seat's pointer (the independent oracle for client-observed input):
    /// "enter S x,y", "leave S", "motion x,y", "button B pressed|released", "axis A v",
    /// "axis_v120 A n", "frame", "relative dx,dy unaccel ux,uy". Coordinates are the wl_fixed
    /// values the client received.
    pub ptr: Vec<String>,
    /// Every xdg_surface.configure received, by surface label, in order.
    pub configures: Vec<&'static str>,
    pub kb: KbLogical,
    pub kb_log: Vec<String>,
    pub key_events: Vec<(u32, bool)>,
    pub repeat_info: Option<(i32, i32)>,
    pub destroyed_active: Vec<String>,
    pub auto_destroy_popups: bool,
    pub popup_dones: Vec<&'static str>,
    pub last_serial: u32,
    pub toplevel_states: Vec<(usize, Vec<u32>)>,
}

/// Unique shm file names across the tests of this process.
static SHM_FILES: AtomicU64 = AtomicU64::new(0);

impl Probe {
    fn buffer(&mut self, qh: &QueueHandle<Self>, w: i32, h: i32) -> WlBuffer {
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

    /// Popup of `content` size with a CSD shadow at `anchor_rect` in the parent's window
    /// geometry, anchor top-left, gravity bottom-right; optional slide constraint adjustment.
    pub fn popup(
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
        self.surfaces[idx].wl.commit();
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
        let idx = self.surfaces.len();
        let wl = self
            .compositor
            .as_ref()
            .expect("wl_compositor")
            .create_surface(qh, ());
        let parent_wl = self.surfaces[parent].wl.clone();
        let sub = self
            .subcompositor
            .as_ref()
            .expect("wl_subcompositor")
            .get_subsurface(&wl, &parent_wl, qh, ());
        sub.set_position(offset.0, offset.1);
        if below {
            sub.place_below(&parent_wl);
        }
        sub.set_desync();
        self.surfaces.push(Surf {
            label,
            wl,
            xdg: None,
            toplevel: None,
            popup: None,
            subsurface: Some(sub),
            size,
            configured: true,
            popup_configure: None,
            frame_driven: false,
        });
        self.draw(qh, idx);
        parent_wl.commit(); // position and stacking apply on the parent's commit
        idx
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
        if let xdg_toplevel::Event::Configure { states, .. } = ev {
            let label = st.surfaces[*idx].label;
            let raw_states: Vec<u32> = states
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_ne_bytes(*c))
                .collect();
            let is_activated = raw_states.contains(&4);
            st.toplevel_states.push((*idx, raw_states));
            st.kb_log.push(format!(
                "toplevel_configure {label} activated={is_activated}"
            ));
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
                st.kb_log.push(format!("popup_done {label}"));
                if st.auto_destroy_popups && st.surfaces[*idx].popup.is_some() {
                    st.destroy_popup(*idx);
                }
            }
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
                surface_x,
                surface_y,
                ..
            } => format!("motion {surface_x},{surface_y}"),
            wl_pointer::Event::Button {
                serial,
                button,
                state,
                ..
            } => {
                st.last_serial = serial;
                let s = match state.into_result() {
                    Ok(wl_pointer::ButtonState::Pressed) => "pressed",
                    _ => "released",
                };
                format!("button {button} {s}")
            }
            wl_pointer::Event::Axis { axis, value, .. } => format!("axis {axis:?} {value}"),
            wl_pointer::Event::AxisValue120 { axis, value120 } => {
                format!("axis_v120 {axis:?} {value120}")
            }
            wl_pointer::Event::Frame => "frame".to_string(),
            _ => return,
        };
        st.ptr.push(line);
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
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                let label = st.label_of(&surface);
                if st.kb.active.is_none() && st.destroyed_active.contains(&label) {
                    st.kb_log.push(format!(
                        "wl_keyboard.leave {label} (in-flight after destroy)"
                    ));
                    return;
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

delegate_noop!(Probe: ignore ZwpRelativePointerManagerV1);
delegate_noop!(Probe: ignore WlCompositor);
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
        let (core, input, _shell, token) = Core::new(persisted);
        let display: Display<WaylandState> = Display::new().expect("display");
        let state = WaylandState::new(&display.handle(), core);
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

    /// Runs every vblank due up to `t` in time order, pumping after each.
    pub fn run_until(&mut self, t: Duration) {
        loop {
            let next = self
                .vblanks
                .iter()
                .copied()
                .filter(|(_, at)| *at <= t)
                .min_by_key(|(o, at)| (*at, *o));
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
        self.probe.seat = Some(seat);
        self.probe.pointer = Some(pointer);
        self.probe.keyboard = Some(keyboard);
        self.pump();
    }

    /// Exchanges messages until both sides are quiescent.
    pub fn pump(&mut self) {
        let mut idle = 0;
        for _ in 0..10_000 {
            let served = self
                .display
                .dispatch_clients(&mut self.state)
                .expect("server dispatch");
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
