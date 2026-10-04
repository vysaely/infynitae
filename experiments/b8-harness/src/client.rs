//! Minimal recording Wayland client ("probe").
//!
//! Creates only what the experiments need and logs every event of interest with the harness'
//! virtual time. Behaves like a well-written fractional-scale client: draws on configure and
//! (if frame-driven) only on wl_callback.done, allocates its buffer at the preferred fractional
//! scale and maps it back with wp_viewport.set_destination.

use std::{collections::HashMap, os::fd::AsFd, sync::{atomic::{AtomicU64, Ordering}, Arc}};

use wayland_client::{
    globals::{GlobalList, GlobalListContents},
    protocol::{
        wl_buffer::{self, WlBuffer},
        wl_callback::{self, WlCallback},
        wl_compositor::WlCompositor,
        wl_output::{self, WlOutput},
        wl_keyboard::{self, WlKeyboard},
        wl_pointer::{self, WlPointer},
        wl_seat::WlSeat,
        wl_subcompositor::WlSubcompositor,
        wl_subsurface::WlSubsurface,
        wl_registry::WlRegistry,
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::{self, WlSurface},
    },
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::{
    wp::{
        fractional_scale::v1::client::{
            wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
            wp_fractional_scale_v1::{self, WpFractionalScaleV1},
        },
        viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    },
    wp::pointer_constraints::zv1::client::{
        zwp_confined_pointer_v1::{self, ZwpConfinedPointerV1},
        zwp_locked_pointer_v1::{self, ZwpLockedPointerV1},
        zwp_pointer_constraints_v1::{self, ZwpPointerConstraintsV1},
    },
    wp::presentation_time::client::{
        wp_presentation::WpPresentation,
        wp_presentation_feedback::{self, WpPresentationFeedback},
    },
    wp::relative_pointer::zv1::client::{
        zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
        zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
    },
    xdg::shell::client::{
        xdg_popup::{self, XdgPopup},
        xdg_positioner::{self, XdgPositioner},
        xdg_surface::{self, XdgSurface},
        xdg_toplevel::{self, XdgToplevel},
        xdg_wm_base::{self, XdgWmBase},
    },
};

pub struct Surf {
    pub label: String,
    pub wl: WlSurface,
    pub xdg: XdgSurface,
    pub toplevel: Option<XdgToplevel>,
    pub popup: Option<XdgPopup>,
    pub viewport: WpViewport,
    pub _frac: WpFractionalScaleV1,
    /// logical surface size (including any CSD shadow)
    pub size: (i32, i32),
    pub frame_driven: bool,
    pub configured: bool,
    pub scale120: u32,
    pub last_buf: Option<(i32, i32)>,
    pub allocs: Vec<(u64, i32, i32)>,
    pub frames_done: u64,
    pub frame_pending: bool,
    pub toplevel_configures: u64,
}

pub struct Probe {
    pub now: Arc<AtomicU64>,
    pub log: Vec<(u64, String)>,
    pub compositor: WlCompositor,
    pub shm: WlShm,
    pub wm_base: XdgWmBase,
    pub frac: WpFractionalScaleManagerV1,
    pub viewporter: WpViewporter,
    pub outputs: HashMap<u32, String>,
    pub _output_objs: Vec<WlOutput>,
    pub surfaces: Vec<Surf>,
    pub record_frames: bool,
    pub seat: Option<WlSeat>,
    pub pointer: Option<WlPointer>,
    pub relative: Option<ZwpRelativePointerV1>,
    pub constraints: Option<ZwpPointerConstraintsV1>,
    pub locked: Option<ZwpLockedPointerV1>,
    pub confined: Option<ZwpConfinedPointerV1>,
    /// buttons the client currently believes are pressed
    pub buttons_down: std::collections::BTreeSet<u32>,
    pub presentation: Option<WpPresentation>,
    /// subsurfaces created by add_subsurface_* (protocol id -> surface), so they can be parents
    pub extra: Vec<(u32, WlSurface)>,
    pub keyboard: Option<WlKeyboard>,
    /// client-side logical keyboard state per the wl_keyboard spec (X-10)
    pub kb: KbLogical,
    /// request presentation feedback with every draw (X-9); the u32 tags the commit
    pub feedback_seq: Option<u32>,
}

impl Probe {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<Self>, now: Arc<AtomicU64>) -> Self {
        let compositor: WlCompositor = globals.bind(qh, 6..=6, ()).expect("wl_compositor v6");
        let shm: WlShm = globals.bind(qh, 1..=1, ()).expect("wl_shm");
        let wm_base: XdgWmBase = globals.bind(qh, 1..=6, ()).expect("xdg_wm_base");
        let frac: WpFractionalScaleManagerV1 = globals.bind(qh, 1..=1, ()).expect("fractional scale");
        let viewporter: WpViewporter = globals.bind(qh, 1..=1, ()).expect("viewporter");
        let mut outs = Vec::new();
        globals.contents().with_list(|list| {
            for g in list.iter().filter(|g| g.interface == "wl_output") {
                outs.push(globals.registry().bind::<WlOutput, _, _>(g.name, 4, qh, ()));
            }
        });
        Probe {
            now,
            log: Vec::new(),
            compositor,
            shm,
            wm_base,
            frac,
            viewporter,
            outputs: HashMap::new(),
            _output_objs: outs,
            surfaces: Vec::new(),
            record_frames: true,
            seat: None,
            pointer: None,
            relative: None,
            constraints: None,
            locked: None,
            confined: None,
            buttons_down: Default::default(),
            presentation: None,
            extra: Vec::new(),
            feedback_seq: None,
            keyboard: None,
            kb: KbLogical::default(),
        }
    }

    /// X-6: add a subsurface of `size` at `offset` (parent-surface-local) to surface `parent`,
    /// with its own buffer; applied on the parent's next commit.
    pub fn add_subsurface(&mut self, globals: &GlobalList, qh: &QueueHandle<Self>, parent: usize, label: &str, offset: (i32, i32), size: (i32, i32)) -> usize {
        self.add_subsurface_ex(globals, qh, parent, label, offset, size, false)
    }

    /// Set the input region of surface `idx` to a single rectangle (surface-local), applied on commit.
    pub fn set_input_rect(&mut self, qh: &QueueHandle<Self>, idx: usize, r: (i32, i32, i32, i32)) {
        let region = self.compositor.create_region(qh, ());
        region.add(r.0, r.1, r.2, r.3);
        self.surfaces[idx].wl.set_input_region(Some(&region));
        region.destroy();
        self.surfaces[idx].wl.commit();
    }

    /// Input region from add/subtract operations (surface-local), applied on commit.
    pub fn set_input_ops(&mut self, qh: &QueueHandle<Self>, idx: usize, ops: &[(bool, (i32, i32, i32, i32))]) {
        let region = self.compositor.create_region(qh, ());
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

    /// Like add_subsurface; `below` places the child below its parent (z-order).
    /// `parent` is a Surf index, or the protocol id returned by a previous add_subsurface_*.
    /// Returns the WlSurface protocol id of the new subsurface (it has no Surf entry).
    pub fn add_subsurface_ex(&mut self, globals: &GlobalList, qh: &QueueHandle<Self>, parent: usize, label: &str, offset: (i32, i32), size: (i32, i32), below: bool) -> usize {
        let subc: WlSubcompositor = globals.bind(qh, 1..=1, ()).expect("wl_subcompositor");
        let idx = self.surfaces.len();
        let wl = self.compositor.create_surface(qh, idx);
        let parent_wl = if parent < self.surfaces.len() { self.surfaces[parent].wl.clone() } else { self.extra.iter().find(|(id, _)| *id as usize == parent).expect("parent").1.clone() };
        let sub: WlSubsurface = subc.get_subsurface(&wl, &parent_wl, qh, ());
        sub.set_position(offset.0, offset.1);
        if below {
            sub.place_below(&parent_wl);
        }
        sub.set_desync();
        // a plain surface (no xdg role); reuse Surf bookkeeping with a dummy xdg object is not
        // possible, so draw it directly here
        let (w, h) = size;
        let stride = w * 4;
        let len = (stride * h) as usize;
        let fd = memfd(len);
        let pool: WlShmPool = self.shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let buf = pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        wl.attach(Some(&buf), 0, 0);
        wl.damage_buffer(0, 0, w, h);
        wl.commit();
        parent_wl.commit();
        let _ = label;
        let pid = wl.id().protocol_id() as usize;
        self.extra.push((pid as u32, wl.clone()));
        std::mem::forget(sub);
        std::mem::forget(wl);
        let _ = idx;
        pid
    }

    /// Client destroys a toplevel (role object, xdg_surface, wl_surface, in protocol order).
    /// X-10 modelling note: the wl_keyboard spec does not say what happens to the logical
    /// "active surface" when the CLIENT destroys it, and a server cannot send wl_keyboard.leave
    /// naming a destroyed object (wayland-backend refuses: InvalidId). The only coherent reading
    /// is that the client's own destruction nulls its logical active surface; keys/modifiers are
    /// then overwritten by the next enter. This is recorded as a protocol ambiguity, not hidden.
    pub fn destroy_toplevel(&mut self, idx: usize) {
        let surf = &self.surfaces[idx];
        if let Some(t) = &surf.toplevel {
            t.destroy();
        }
        surf.xdg.destroy();
        surf.wl.destroy();
        let label = surf.label.clone();
        if self.kb.active.as_deref() == Some(label.as_str()) {
            self.kb.active = None;
            let now = self.now.load(Ordering::Relaxed);
            self.log.push((now, format!("client-side: destroyed its keyboard-active surface {label} -> logical active surface = null (implicit; no leave is deliverable)")));
        }
    }

    /// Bind seat pointer, relative pointer and pointer constraints (X-8).
    pub fn bind_input(&mut self, globals: &GlobalList, qh: &QueueHandle<Self>) {
        let seat: WlSeat = globals.bind(qh, 1..=7, ()).expect("wl_seat");
        let pointer = seat.get_pointer(qh, ());
        let rel_mgr: ZwpRelativePointerManagerV1 = globals.bind(qh, 1..=1, ()).expect("relative pointer manager");
        let relative = rel_mgr.get_relative_pointer(&pointer, qh, ());
        let constraints: ZwpPointerConstraintsV1 = globals.bind(qh, 1..=1, ()).expect("pointer constraints");
        self.keyboard = Some(seat.get_keyboard(qh, ()));
        self.seat = Some(seat);
        self.pointer = Some(pointer);
        self.relative = Some(relative);
        self.constraints = Some(constraints);
    }

    pub fn lock_pointer(&mut self, qh: &QueueHandle<Self>, idx: usize, oneshot: bool) {
        let lt = if oneshot { zwp_pointer_constraints_v1::Lifetime::Oneshot } else { zwp_pointer_constraints_v1::Lifetime::Persistent };
        let c = self.constraints.as_ref().unwrap();
        let l = c.lock_pointer(&self.surfaces[idx].wl, self.pointer.as_ref().unwrap(), None, lt, qh, ());
        self.surfaces[idx].wl.commit();
        self.locked = Some(l);
    }

    pub fn confine_pointer(&mut self, qh: &QueueHandle<Self>, idx: usize) {
        let c = self.constraints.as_ref().unwrap();
        let cf = c.confine_pointer(&self.surfaces[idx].wl, self.pointer.as_ref().unwrap(), None, zwp_pointer_constraints_v1::Lifetime::Persistent, qh, ());
        self.surfaces[idx].wl.commit();
        self.confined = Some(cf);
    }

    fn rec(&mut self, s: String) {
        self.log.push((self.now.load(Ordering::Relaxed), s));
    }

    fn new_surface(&mut self, qh: &QueueHandle<Self>, label: &str, size: (i32, i32), frame_driven: bool) -> (usize, WlSurface, XdgSurface) {
        let idx = self.surfaces.len();
        let wl = self.compositor.create_surface(qh, idx);
        let xdg = self.wm_base.get_xdg_surface(&wl, qh, idx);
        let viewport = self.viewporter.get_viewport(&wl, qh, ());
        let frac = self.frac.get_fractional_scale(&wl, qh, idx);
        self.surfaces.push(Surf {
            label: label.into(),
            wl: wl.clone(),
            xdg: xdg.clone(),
            toplevel: None,
            popup: None,
            viewport,
            _frac: frac,
            size,
            frame_driven,
            configured: false,
            scale120: 120,
            last_buf: None,
            allocs: Vec::new(),
            frames_done: 0,
            frame_pending: false,
            toplevel_configures: 0,
        });
        (idx, wl, xdg)
    }

    /// Toplevel with `content` logical size and a CSD shadow of `shadow` px on every side.
    pub fn create_toplevel(&mut self, qh: &QueueHandle<Self>, label: &str, content: (i32, i32), shadow: i32, frame_driven: bool) -> usize {
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let (idx, wl, xdg) = self.new_surface(qh, label, size, frame_driven);
        let tl = xdg.get_toplevel(qh, idx);
        tl.set_title(label.into());
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        self.surfaces[idx].toplevel = Some(tl);
        wl.commit(); // initial commit, no buffer
        idx
    }

    /// Popup of `content` size with its own CSD shadow, positioned at `anchor_rect` (relative to
    /// the parent's window geometry) with gravity bottom-right from the anchor's top-left.
    pub fn create_popup(&mut self, qh: &QueueHandle<Self>, label: &str, parent: usize, content: (i32, i32), shadow: i32, anchor_rect: (i32, i32, i32, i32)) -> usize {
        let size = (content.0 + 2 * shadow, content.1 + 2 * shadow);
        let (idx, wl, xdg) = self.new_surface(qh, label, size, false);
        let pos: XdgPositioner = self.wm_base.create_positioner(qh, ());
        pos.set_size(content.0, content.1);
        pos.set_anchor_rect(anchor_rect.0, anchor_rect.1, anchor_rect.2, anchor_rect.3);
        pos.set_anchor(xdg_positioner::Anchor::TopLeft);
        pos.set_gravity(xdg_positioner::Gravity::BottomRight);
        let parent_xdg = self.surfaces[parent].xdg.clone();
        // role first: "A role must be assigned before any other requests are made to the xdg_surface"
        let popup = xdg.get_popup(Some(&parent_xdg), &pos, qh, idx);
        pos.destroy();
        if shadow > 0 {
            xdg.set_window_geometry(shadow, shadow, content.0, content.1);
        }
        self.surfaces[idx].popup = Some(popup);
        wl.commit();
        idx
    }

    pub fn draw(&mut self, qh: &QueueHandle<Self>, idx: usize) {
        let (w, h) = self.surfaces[idx].size;
        let s = self.surfaces[idx].scale120 as f64 / 120.0;
        let (bw, bh) = ((w as f64 * s).ceil() as i32, (h as f64 * s).ceil() as i32);
        let now = self.now.load(Ordering::Relaxed);
        let surf = &mut self.surfaces[idx];
        if surf.last_buf != Some((bw, bh)) {
            surf.allocs.push((now, bw, bh));
            surf.last_buf = Some((bw, bh));
        }
        let stride = bw * 4;
        let len = (stride * bh) as usize;
        let fd = memfd(len);
        let pool: WlShmPool = self.shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let buf = pool.create_buffer(0, bw, bh, stride, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        let surf = &mut self.surfaces[idx];
        surf.wl.attach(Some(&buf), 0, 0);
        surf.viewport.set_destination(w, h);
        surf.wl.damage_buffer(0, 0, bw, bh);
        if let (Some(p), Some(seq)) = (&self.presentation, self.feedback_seq.as_mut()) {
            *seq += 1;
            let _ = p.feedback(&surf.wl, qh, *seq);
        }
        let surf = &mut self.surfaces[idx];
        if surf.frame_driven && !surf.frame_pending {
            surf.wl.frame(qh, idx);
            surf.frame_pending = true;
        }
        surf.wl.commit();
    }
}

fn memfd(len: usize) -> std::os::fd::OwnedFd {
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new("probe").unwrap();
    // SAFETY: memfd_create with a valid NUL-terminated name; the returned fd is owned here.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    assert!(fd >= 0, "memfd_create failed");
    // SAFETY: fd is a valid, newly created descriptor that nothing else owns.
    let owned = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    // SAFETY: ftruncate on a valid fd we own.
    assert_eq!(unsafe { libc::ftruncate(fd, len as libc::off_t) }, 0);
    owned
}

fn states_str(raw: &[u8]) -> String {
    let names: Vec<String> = raw
        .chunks_exact(4)
        .map(|c| match u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) {
            1 => "maximized".into(),
            2 => "fullscreen".into(),
            3 => "resizing".into(),
            4 => "activated".into(),
            9 => "suspended".into(),
            n => format!("state{n}"),
        })
        .collect();
    format!("[{}]", names.join(","))
}

impl Dispatch<WlSurface, usize> for Probe {
    fn event(st: &mut Self, _: &WlSurface, ev: wl_surface::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let label = st.surfaces[*idx].label.clone();
        match ev {
            wl_surface::Event::Enter { output } => {
                let n = st.outputs.get(&output.id().protocol_id()).cloned().unwrap_or("?".into());
                st.rec(format!("{label}: wl_surface.enter {n}"));
            }
            wl_surface::Event::Leave { output } => {
                let n = st.outputs.get(&output.id().protocol_id()).cloned().unwrap_or("?".into());
                st.rec(format!("{label}: wl_surface.leave {n}"));
            }
            wl_surface::Event::PreferredBufferScale { factor } => st.rec(format!("{label}: wl_surface.preferred_buffer_scale {factor}")),
            wl_surface::Event::PreferredBufferTransform { .. } => {}
            _ => {}
        }
    }
}
impl Dispatch<WpFractionalScaleV1, usize> for Probe {
    fn event(st: &mut Self, _: &WpFractionalScaleV1, ev: wp_fractional_scale_v1::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = ev {
            let label = st.surfaces[*idx].label.clone();
            st.rec(format!("{label}: wp_fractional_scale_v1.preferred_scale {scale} (= {})", scale as f64 / 120.0));
            st.surfaces[*idx].scale120 = scale;
        }
    }
}
impl Dispatch<XdgSurface, usize> for Probe {
    fn event(st: &mut Self, xdg: &XdgSurface, ev: xdg_surface::Event, idx: &usize, _: &Connection, qh: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = ev {
            xdg.ack_configure(serial);
            let label = st.surfaces[*idx].label.clone();
            st.rec(format!("{label}: xdg_surface.configure serial={serial} (acked)"));
            if !st.surfaces[*idx].configured {
                st.surfaces[*idx].configured = true;
                st.draw(qh, *idx);
            }
        }
    }
}
impl Dispatch<XdgToplevel, usize> for Probe {
    fn event(st: &mut Self, _: &XdgToplevel, ev: xdg_toplevel::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let label = st.surfaces[*idx].label.clone();
        match ev {
            xdg_toplevel::Event::Configure { width, height, states } => {
                st.surfaces[*idx].toplevel_configures += 1;
                st.rec(format!("{label}: xdg_toplevel.configure {width}x{height} {}", states_str(&states)));
            }
            xdg_toplevel::Event::Close => st.rec(format!("{label}: xdg_toplevel.close")),
            _ => {}
        }
    }
}
impl Dispatch<XdgPopup, usize> for Probe {
    fn event(st: &mut Self, _: &XdgPopup, ev: xdg_popup::Event, idx: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let label = st.surfaces[*idx].label.clone();
        match ev {
            xdg_popup::Event::Configure { x, y, width, height } => st.rec(format!("{label}: xdg_popup.configure x={x} y={y} {width}x{height}")),
            xdg_popup::Event::PopupDone => st.rec(format!("{label}: xdg_popup.popup_done")),
            _ => {}
        }
    }
}
impl Dispatch<WlCallback, usize> for Probe {
    fn event(st: &mut Self, _: &WlCallback, ev: wl_callback::Event, idx: &usize, _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = ev {
            st.surfaces[*idx].frames_done += 1;
            st.surfaces[*idx].frame_pending = false;
            if st.record_frames {
                let label = st.surfaces[*idx].label.clone();
                st.rec(format!("{label}: wl_callback.done"));
            }
            if st.surfaces[*idx].frame_driven {
                st.draw(qh, *idx);
            }
        }
    }
}
impl Dispatch<XdgWmBase, ()> for Probe {
    fn event(_: &mut Self, wm: &XdgWmBase, ev: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}
impl Dispatch<WlOutput, ()> for Probe {
    fn event(st: &mut Self, o: &WlOutput, ev: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = ev {
            st.outputs.insert(o.id().protocol_id(), name);
        }
    }
}
/// wl_keyboard logical state as defined by the protocol, plus a checker for its MUST / MUST NOT rules.
#[derive(Default, Debug)]
pub struct KbLogical {
    pub active: Option<String>,
    pub keys: std::collections::BTreeSet<u32>,
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    /// `enter` must be followed by `modifiers`
    pub awaiting_modifiers: bool,
    pub violations: Vec<String>,
}

impl Dispatch<WlKeyboard, ()> for Probe {
    fn event(st: &mut Self, _: &WlKeyboard, ev: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let now = st.now.load(Ordering::Relaxed);
        let mut v: Vec<String> = Vec::new();
        let line = match ev {
            wl_keyboard::Event::Keymap { .. } | wl_keyboard::Event::RepeatInfo { .. } => return,
            wl_keyboard::Event::Enter { surface, keys, .. } => {
                let l = label_of(st, &surface);
                if st.kb.active.is_some() {
                    v.push(format!("enter({l}) while already active on {:?}", st.kb.active));
                }
                st.kb.active = Some(l.clone());
                st.kb.keys = keys.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                st.kb.awaiting_modifiers = true;
                format!("wl_keyboard.enter {l} keys={:?}", st.kb.keys)
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                let l = label_of(st, &surface);
                if st.kb.active.as_deref() != Some(l.as_str()) {
                    v.push(format!("leave({l}) but active={:?}", st.kb.active));
                }
                if st.kb.awaiting_modifiers {
                    v.push("leave before the modifiers that must follow enter".into());
                }
                // "In the wl_keyboard logical state, this event resets all values to their defaults."
                st.kb = KbLogical { violations: std::mem::take(&mut st.kb.violations), ..Default::default() };
                format!("wl_keyboard.leave {l}")
            }
            wl_keyboard::Event::Key { key, state, .. } => {
                let pressed = matches!(state, wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed));
                if st.kb.active.is_none() {
                    v.push(format!("key {key} without an active surface"));
                }
                if st.kb.awaiting_modifiers {
                    v.push("key before the modifiers that must follow enter".into());
                }
                if pressed && !st.kb.keys.insert(key) {
                    v.push(format!("pressed {key} already logically down"));
                }
                if !pressed && !st.kb.keys.remove(&key) {
                    v.push(format!("released {key} not logically down"));
                }
                format!("wl_keyboard.key {key} {}", if pressed { "pressed" } else { "released" })
            }
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, .. } => {
                st.kb.awaiting_modifiers = false;
                st.kb.depressed = mods_depressed;
                st.kb.latched = mods_latched;
                st.kb.locked = mods_locked;
                format!("wl_keyboard.modifiers dep={mods_depressed:#x} lat={mods_latched:#x} lock={mods_locked:#x}")
            }
            _ => return,
        };
        for x in &v {
            st.kb.violations.push(format!("t={now}: {x}"));
        }
        st.log.push((now, line));
        for x in v {
            st.log.push((now, format!("PROTOCOL VIOLATION: {x}")));
        }
    }
}

fn label_of(st: &Probe, s: &WlSurface) -> String {
    s.data::<usize>().map(|i| st.surfaces[*i].label.clone()).unwrap_or("?".into())
}
impl Dispatch<WlPointer, ()> for Probe {
    fn event(st: &mut Self, _: &WlPointer, ev: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                let l = label_of(st, &surface);
                st.rec(format!("wl_pointer.enter {l} ({surface_x:.3},{surface_y:.3})"));
            }
            wl_pointer::Event::Leave { surface, .. } => {
                let l = label_of(st, &surface);
                st.rec(format!("wl_pointer.leave {l}"));
                // wl_pointer.leave does not release buttons in the protocol; keep the client's view as-is
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => st.rec(format!("wl_pointer.motion ({surface_x:.3},{surface_y:.3})")),
            wl_pointer::Event::Button { button, state, .. } => {
                let pressed = matches!(state, wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed));
                if pressed {
                    st.buttons_down.insert(button);
                } else {
                    st.buttons_down.remove(&button);
                }
                st.rec(format!("wl_pointer.button {button} {}", if pressed { "pressed" } else { "released" }));
            }
            _ => {}
        }
    }
}
impl Dispatch<ZwpRelativePointerV1, ()> for Probe {
    fn event(st: &mut Self, _: &ZwpRelativePointerV1, ev: zwp_relative_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, dx_unaccel, dy_unaccel, .. } = ev {
            st.rec(format!("relative_motion ({dx:.3},{dy:.3}) unaccel ({dx_unaccel:.3},{dy_unaccel:.3})"));
        }
    }
}
impl Dispatch<ZwpLockedPointerV1, ()> for Probe {
    fn event(st: &mut Self, _: &ZwpLockedPointerV1, ev: zwp_locked_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            zwp_locked_pointer_v1::Event::Locked => st.rec("locked_pointer.locked".into()),
            zwp_locked_pointer_v1::Event::Unlocked => st.rec("locked_pointer.unlocked".into()),
            _ => {}
        }
    }
}
impl Dispatch<ZwpConfinedPointerV1, ()> for Probe {
    fn event(st: &mut Self, _: &ZwpConfinedPointerV1, ev: zwp_confined_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            zwp_confined_pointer_v1::Event::Confined => st.rec("confined_pointer.confined".into()),
            zwp_confined_pointer_v1::Event::Unconfined => st.rec("confined_pointer.unconfined".into()),
            _ => {}
        }
    }
}
impl Dispatch<WlSeat, ()> for Probe {
    fn event(_: &mut Self, _: &WlSeat, _: wayland_client::protocol::wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
wayland_client::delegate_noop!(Probe: ignore ZwpRelativePointerManagerV1);
wayland_client::delegate_noop!(Probe: ignore WlSubcompositor);
wayland_client::delegate_noop!(Probe: ignore wayland_client::protocol::wl_region::WlRegion);
wayland_client::delegate_noop!(Probe: ignore WpPresentation);
impl Dispatch<WpPresentationFeedback, u32> for Probe {
    fn event(st: &mut Self, _: &WpPresentationFeedback, ev: wp_presentation_feedback::Event, seq: &u32, _: &Connection, _: &QueueHandle<Self>) {
        match ev {
            wp_presentation_feedback::Event::Presented { .. } => st.rec(format!("presentation_feedback#{seq}.presented")),
            wp_presentation_feedback::Event::Discarded => st.rec(format!("presentation_feedback#{seq}.discarded")),
            _ => {}
        }
    }
}
wayland_client::delegate_noop!(Probe: ignore WlSubsurface);
wayland_client::delegate_noop!(Probe: ignore ZwpPointerConstraintsV1);

impl Dispatch<WlBuffer, ()> for Probe {
    fn event(_: &mut Self, b: &WlBuffer, ev: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = ev {
            b.destroy();
        }
    }
}
impl Dispatch<WlRegistry, GlobalListContents> for Probe {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
wayland_client::delegate_noop!(Probe: ignore WlCompositor);
wayland_client::delegate_noop!(Probe: ignore WlShm);
wayland_client::delegate_noop!(Probe: ignore WlShmPool);
wayland_client::delegate_noop!(Probe: ignore WpFractionalScaleManagerV1);
wayland_client::delegate_noop!(Probe: ignore WpViewporter);
wayland_client::delegate_noop!(Probe: ignore WpViewport);
wayland_client::delegate_noop!(Probe: ignore XdgPositioner);
