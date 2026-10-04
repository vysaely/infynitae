//! Minimal Smithay server state for the B7–B9 experiments.
//!
//! What is REAL (measured): Smithay 0.7.0 protocol objects, wl_surface.enter/leave via the
//! public `SpaceElement::output_enter/output_leave` on `Window`, fractional/preferred scale
//! sending, frame-callback completion via `Window::send_frame`, xdg configure traffic.
//!
//! What is MODELED (policy under test, not a production design): cameras (integer, zoom 1),
//! visibility = overlap of the element's Canvas bbox with each camera viewport, the
//! virtual vblank clock, and the scale / frame / suspension policies below.

use std::{collections::VecDeque, time::Duration};

use smithay::{
    backend::{
        allocator::dmabuf::Dmabuf,
        renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state},
    },
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_fractional_scale,
    delegate_output, delegate_seat, delegate_shm, delegate_viewporter, delegate_xdg_shell,
    desktop::{space::SpaceElement, PopupKind, PopupManager, Window},
    input::{Seat, SeatHandler, SeatState},
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_seat, wl_surface::WlSurface},
            Client, DisplayHandle, Resource,
        },
    },
    utils::{Logical, Rectangle, Serial, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, send_surface_state, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        fractional_scale::{with_fractional_scale, FractionalScaleHandler, FractionalScaleManagerState},
        output::{OutputHandler, OutputManagerState},
        selection::{
            data_device::{ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler},
            SelectionHandler,
        },
        shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState},
        shm::{ShmHandler, ShmState},
        viewporter::ViewporterState,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScalePolicy {
    /// max scale over outputs the element currently overlaps; held while off-screen
    Max,
    /// min scale over overlapped outputs; held while off-screen
    Min,
    /// scale of the "primary" output chosen like smithay's default_primary_scanout_output_compare
    /// (switch only if the new output shows >= 2x the visible area, else higher refresh)
    Primary,
    /// scale of the output showing the largest visible area right now (continuous)
    LargestArea,
    /// Max, but while a camera gesture is active the scale may only increase; decreases are
    /// evaluated once when the gesture ends (no time constant involved)
    MaxGesture,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FramePolicy {
    /// never complete callbacks of elements visible on no output
    Starve,
    /// complete callbacks of invisible elements at most once per duration
    Throttle(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClockPolicy {
    /// frame clock = visible output with the highest refresh rate
    HighestRefresh,
    /// frame clock = Primary output (same rule as ScalePolicy::Primary)
    Primary,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SuspendPolicy {
    Never,
    /// set xdg_toplevel `suspended` the moment the element is visible on no output
    Immediate,
    /// set `suspended` after being visible on no output continuously for the duration
    Debounce(Duration),
}

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub scale: ScalePolicy,
    pub frames: FramePolicy,
    pub clock: ClockPolicy,
    pub suspend: SuspendPolicy,
}

pub struct VOut {
    pub output: Output,
    pub name: String,
    /// logical viewport size
    pub size: (i32, i32),
    pub scale: f64,
    pub refresh_mhz: i32,
    /// integer Canvas camera (top-left of the viewport); zoom 1 in this harness
    pub camera: (i32, i32),
    pub next_vblank_us: f64,
}
impl VOut {
    pub fn period_us(&self) -> f64 {
        1e9 / self.refresh_mhz as f64
    }
    pub fn viewport(&self) -> Rectangle<i32, Logical> {
        Rectangle::new(self.camera.into(), self.size.into())
    }
}

pub struct Elem {
    pub id: u64,
    pub window: Window,
    pub anchor: Option<(i32, i32)>,
    pub mapped: bool,
    /// overlap with each output (RootSurfaceLocal), None if not visible there
    pub overlap: Vec<Option<Rectangle<i32, Logical>>>,
    pub area: Vec<i64>,
    pub primary: Option<usize>,
    pub scale: Option<f64>,
    pub suspended: bool,
    pub invisible_since_us: Option<u64>,
}

pub struct Server {
    pub dh: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Server>,
    pub seat: Seat<Server>,
    pub data_device_state: DataDeviceState,
    pub fractional_state: FractionalScaleManagerState,
    pub viewporter_state: ViewporterState,
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub popups: PopupManager,

    pub outputs: Vec<VOut>,
    pub elements: Vec<Elem>,
    pub next_id: u64,
    pub pending_anchors: VecDeque<(i32, i32)>,
    pub policy: Policy,
    pub now_us: u64,
    pub log: Vec<(u64, String)>,
    pub commits: Vec<(u64, u64)>, // (time, element id) for root-surface commits with a buffer
    pub pongs: Vec<u64>,
    pub quiet: bool,
    /// a camera gesture (pan) is in progress
    pub gesture_active: bool,
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

pub struct OutCfg {
    pub name: &'static str,
    pub mode: (i32, i32),
    pub scale: f64,
    pub refresh_mhz: i32,
    pub camera: (i32, i32),
}

impl Server {
    pub fn new(dh: &DisplayHandle, outs: &[OutCfg], policy: Policy) -> Self {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, "harness");
        seat.add_pointer();
        // A keyboard is required by some real clients; default XKB config.
        let _ = seat.add_keyboard(Default::default(), 200, 25);

        let mut outputs = Vec::new();
        for c in outs {
            let output = Output::new(
                c.name.to_string(),
                PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "harness".into(), model: c.name.into() },
            );
            let _g = output.create_global::<Server>(dh);
            let mode = Mode { size: c.mode.into(), refresh: c.refresh_mhz };
            output.change_current_state(Some(mode), Some(Transform::Normal), Some(Scale::Fractional(c.scale)), Some((0, 0).into()));
            output.set_preferred(mode);
            let size = (
                (c.mode.0 as f64 / c.scale).ceil() as i32,
                (c.mode.1 as f64 / c.scale).ceil() as i32,
            );
            outputs.push(VOut {
                output,
                name: c.name.to_string(),
                size,
                scale: c.scale,
                refresh_mhz: c.refresh_mhz,
                camera: c.camera,
                next_vblank_us: 0.0,
            });
        }

        Server {
            dh: dh.clone(),
            compositor_state: CompositorState::new_v6::<Self>(dh),
            xdg_shell_state: XdgShellState::new::<Self>(dh),
            shm_state: ShmState::new::<Self>(dh, vec![]),
            output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(dh),
            seat_state,
            seat,
            data_device_state: DataDeviceState::new::<Self>(dh),
            fractional_state: FractionalScaleManagerState::new::<Self>(dh),
            viewporter_state: ViewporterState::new::<Self>(dh),
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            popups: PopupManager::default(),
            outputs,
            elements: Vec::new(),
            next_id: 1,
            pending_anchors: VecDeque::new(),
            policy,
            now_us: 0,
            log: Vec::new(),
            commits: Vec::new(),
            pongs: Vec::new(),
            quiet: false,
            gesture_active: false,
        }
    }

    pub fn note(&mut self, s: String) {
        if !self.quiet {
            self.log.push((self.now_us, s));
        }
    }

    fn elem_by_root(&self, surface: &WlSurface) -> Option<usize> {
        self.elements.iter().position(|e| e.window.toplevel().map(|t| t.wl_surface() == surface).unwrap_or(false))
    }

    /// Canvas bbox of the element's whole tree, and its root surface origin (Canvas).
    pub fn canvas_bbox(e: &Elem) -> Option<(Rectangle<i32, Logical>, (i32, i32))> {
        let anchor = e.anchor?;
        let geo = e.window.geometry();
        let root = (anchor.0 - geo.loc.x, anchor.1 - geo.loc.y);
        let mut bbox = e.window.bbox_with_popups();
        bbox.loc.x += root.0;
        bbox.loc.y += root.1;
        Some((bbox, root))
    }

    /// Visibility pass: enter/leave (Smithay mechanism), primary, scale, suspension.
    pub fn refresh_visibility(&mut self) {
        let n_out = self.outputs.len();
        let viewports: Vec<_> = self.outputs.iter().map(|o| o.viewport()).collect();
        let mut notes = Vec::new();
        for ei in 0..self.elements.len() {
            let (id, mapped) = (self.elements[ei].id, self.elements[ei].mapped);
            if self.elements[ei].overlap.len() != n_out {
                self.elements[ei].overlap = vec![None; n_out];
                self.elements[ei].area = vec![0; n_out];
            }
            let geo = if mapped { Self::canvas_bbox(&self.elements[ei]) } else { None };
            for oi in 0..n_out {
                let new = geo.and_then(|(bbox, root)| {
                    viewports[oi].intersection(bbox).map(|mut ov| {
                        // overlap expressed in RootSurfaceLocal coordinates (B2.4 replacement for Space)
                        ov.loc.x -= root.0;
                        ov.loc.y -= root.1;
                        ov
                    })
                });
                let old = self.elements[ei].overlap[oi];
                let e = &mut self.elements[ei];
                match (old, new) {
                    (None, Some(ov)) => {
                        e.window.output_enter(&self.outputs[oi].output, ov);
                        notes.push(format!("E{id} visible-on {} (overlap {}x{})", self.outputs[oi].name, ov.size.w, ov.size.h));
                    }
                    (Some(a), Some(b)) if a != b => e.window.output_enter(&self.outputs[oi].output, b),
                    (Some(_), None) => {
                        e.window.output_leave(&self.outputs[oi].output);
                        notes.push(format!("E{id} invisible-on {}", self.outputs[oi].name));
                    }
                    _ => {}
                }
                e.overlap[oi] = new;
                e.area[oi] = new.map(|r| r.size.w as i64 * r.size.h as i64).unwrap_or(0);
            }
            self.update_primary(ei);
            self.apply_scale(ei, &mut notes);
            self.update_suspension(ei, &mut notes);
        }
        for n in notes {
            self.note(n);
        }
    }

    fn update_primary(&mut self, ei: usize) {
        let outs = &self.outputs;
        let e = &mut self.elements[ei];
        let visible: Vec<usize> = (0..outs.len()).filter(|&i| e.overlap[i].is_some()).collect();
        if visible.is_empty() {
            return; // primary is held while invisible (used for re-entry decisions)
        }
        let mut cur = match e.primary {
            Some(p) if visible.contains(&p) => p,
            _ => visible[0],
        };
        for &cand in &visible {
            if cand == cur {
                continue;
            }
            let (a_cur, a_new) = (e.area[cur], e.area[cand]);
            if a_new >= 2 * a_cur {
                cur = cand;
            } else if a_cur >= 2 * a_new {
            } else if outs[cand].refresh_mhz > outs[cur].refresh_mhz {
                cur = cand;
            }
        }
        e.primary = Some(cur);
    }

    pub fn clock_output(&self, ei: usize) -> Option<usize> {
        let e = &self.elements[ei];
        let visible: Vec<usize> = (0..self.outputs.len()).filter(|&i| e.overlap.get(i).map(|o| o.is_some()).unwrap_or(false)).collect();
        if visible.is_empty() {
            return None;
        }
        match self.policy.clock {
            ClockPolicy::Primary => e.primary,
            ClockPolicy::HighestRefresh => visible.into_iter().max_by_key(|&i| (self.outputs[i].refresh_mhz, std::cmp::Reverse(i))),
        }
    }

    fn desired_scale(&self, ei: usize) -> Option<f64> {
        let e = &self.elements[ei];
        let vis: Vec<usize> = (0..self.outputs.len()).filter(|&i| e.overlap.get(i).map(|o| o.is_some()).unwrap_or(false)).collect();
        if vis.is_empty() {
            return None; // hold
        }
        let sc = |i: usize| self.outputs[i].scale;
        Some(match self.policy.scale {
            ScalePolicy::Max | ScalePolicy::MaxGesture => vis.iter().map(|&i| sc(i)).fold(f64::MIN, f64::max),
            ScalePolicy::Min => vis.iter().map(|&i| sc(i)).fold(f64::MAX, f64::min),
            ScalePolicy::Primary => sc(e.primary.unwrap_or(vis[0])),
            ScalePolicy::LargestArea => sc(*vis.iter().max_by_key(|&&i| (e.area[i], std::cmp::Reverse(i))).unwrap()),
        })
    }

    fn apply_scale(&mut self, ei: usize, notes: &mut Vec<String>) {
        if let Some(s) = self.desired_scale(ei) {
            let cur = self.elements[ei].scale;
            if self.policy.scale == ScalePolicy::MaxGesture && self.gesture_active && cur.map_or(false, |c| s < c) {
                return; // decrease deferred to gesture end
            }
            if cur != Some(s) {
                self.set_element_scale(ei, s);
                notes.push(format!("E{} preferred-scale -> {s}", self.elements[ei].id));
            }
        }
    }

    pub fn set_element_scale(&mut self, ei: usize, s: f64) {
        self.elements[ei].scale = Some(s);
        self.elements[ei].window.with_surfaces(|surface, states| {
            with_fractional_scale(states, |fs| fs.set_preferred_scale(s));
            send_surface_state(surface, states, s.ceil() as i32, Transform::Normal);
        });
    }

    fn update_suspension(&mut self, ei: usize, notes: &mut Vec<String>) {
        let now = self.now_us;
        let e = &mut self.elements[ei];
        if !e.mapped {
            return;
        }
        let visible = e.overlap.iter().any(|o| o.is_some());
        if visible {
            e.invisible_since_us = None;
        } else if e.invisible_since_us.is_none() {
            e.invisible_since_us = Some(now);
        }
        let want = match self.policy.suspend {
            SuspendPolicy::Never => false,
            SuspendPolicy::Immediate => !visible,
            SuspendPolicy::Debounce(d) => e.invisible_since_us.map(|t| now - t >= d.as_micros() as u64).unwrap_or(false),
        };
        if want != e.suspended {
            e.suspended = want;
            if let Some(t) = e.window.toplevel() {
                t.with_pending_state(|st| {
                    if want {
                        st.states.set(xdg_toplevel::State::Suspended);
                    } else {
                        st.states.unset(xdg_toplevel::State::Suspended);
                    }
                });
                t.send_pending_configure();
            }
            notes.push(format!("E{} suspended={want}", e.id));
        }
    }

    /// Process one vblank of output `oi` at the current time.
    pub fn vblank(&mut self, oi: usize) {
        let t = Duration::from_micros(self.now_us);
        let out = self.outputs[oi].output.clone();
        for ei in 0..self.elements.len() {
            if !self.elements[ei].mapped {
                continue;
            }
            let visible_here = self.elements[ei].overlap.get(oi).map(|o| o.is_some()).unwrap_or(false);
            let clock = self.clock_output(ei);
            if visible_here && clock == Some(oi) {
                let o2 = &out; // closure must be Copy: capture by reference
                self.elements[ei].window.send_frame(&out, t, None, |_, _| Some(o2.clone()));
            } else if clock.is_none() {
                if let FramePolicy::Throttle(d) = self.policy.frames {
                    // fallback ticker: driven by output 0's vblanks only
                    if oi == 0 {
                        self.elements[ei].window.send_frame(&out, t, Some(d), |_, _| None);
                    }
                }
            }
        }
        // suspension debounce needs time to pass even without camera motion
        if matches!(self.policy.suspend, SuspendPolicy::Debounce(_)) {
            let mut notes = Vec::new();
            for ei in 0..self.elements.len() {
                self.update_suspension(ei, &mut notes);
            }
            for n in notes {
                self.note(n);
            }
        }
    }

    /// Next due vblank (output index, time).
    pub fn next_vblank(&self) -> (usize, f64) {
        self.outputs
            .iter()
            .enumerate()
            .map(|(i, o)| (i, o.next_vblank_us))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)))
            .unwrap()
    }
    pub fn consume_vblank(&mut self, oi: usize) {
        let p = self.outputs[oi].period_us();
        self.outputs[oi].next_vblank_us += p;
    }
}

impl CompositorHandler for Server {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().expect("harness client data").compositor_state
    }
    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(p) = get_parent(&root) {
                root = p;
            }
            if let Some(ei) = self.elem_by_root(&root) {
                self.elements[ei].window.on_commit();
                // count content commits on ANY surface of the element's tree (e.g. Firefox draws
                // into a subsurface); the root's mapping logic below is separate
                if &root != surface && with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false) {
                    let id = self.elements[ei].id;
                    self.commits.push((self.now_us, id));
                }
            }
        }
        self.popups.commit(surface);

        if let Some(ei) = self.elem_by_root(surface) {
            let toplevel = self.elements[ei].window.toplevel().cloned();
            let has_buffer = with_renderer_surface_state(surface, |s| s.buffer().is_some()).unwrap_or(false);
            if let Some(t) = toplevel {
                if !t.is_initial_configure_sent() {
                    // placement at initial commit (B5); scale chosen before the initial configure
                    if self.elements[ei].anchor.is_none() {
                        self.elements[ei].anchor = Some(self.pending_anchors.pop_front().unwrap_or((0, 0)));
                    }
                    let a = self.elements[ei].anchor.unwrap();
                    let initial = self
                        .outputs
                        .iter()
                        .filter(|o| o.viewport().contains(a))
                        .map(|o| o.scale)
                        .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))
                        .unwrap_or(1.0);
                    self.set_element_scale(ei, initial);
                    t.send_configure();
                    let id = self.elements[ei].id;
                    self.note(format!("E{id} initial configure (initial scale {initial})"));
                }
            }
            let id = self.elements[ei].id;
            if has_buffer {
                self.commits.push((self.now_us, id));
            }
            if has_buffer != self.elements[ei].mapped {
                self.elements[ei].mapped = has_buffer;
                self.note(format!("E{id} mapped={has_buffer}"));
                self.refresh_visibility();
            }
        }

        if let Some(PopupKind::Xdg(p)) = self.popups.find_popup(surface) {
            if !p.is_initial_configure_sent() {
                let _ = p.send_configure();
            }
            // popup geometry changes can change enter/leave of the popup surface
            self.refresh_visibility();
        }
    }
}

impl BufferHandler for Server {
    fn buffer_destroyed(&mut self, _b: &wl_buffer::WlBuffer) {}
}
impl ShmHandler for Server {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for Server {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let id = self.next_id;
        self.next_id += 1;
        let window = Window::new_wayland_window(surface);
        self.elements.push(Elem {
            id,
            window,
            anchor: None,
            mapped: false,
            overlap: vec![None; self.outputs.len()],
            area: vec![0; self.outputs.len()],
            primary: None,
            scale: None,
            suspended: false,
            invisible_since_us: None,
        });
        self.note(format!("E{id} new_toplevel"));
    }
    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.elements.retain(|e| e.window.toplevel() != Some(&surface));
    }
    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|s| s.geometry = positioner.get_geometry());
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }
    fn reposition_request(&mut self, _s: PopupSurface, _p: PositionerState, _t: u32) {}
    fn grab(&mut self, _s: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
    fn client_pong(&mut self, _client: smithay::wayland::shell::xdg::ShellClient) {
        let t = self.now_us;
        self.pongs.push(t);
    }
}

impl OutputHandler for Server {}
impl SeatHandler for Server {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;
    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}
impl SelectionHandler for Server {
    type SelectionUserData = ();
}
impl ClientDndGrabHandler for Server {}
impl ServerDndGrabHandler for Server {}
impl DataDeviceHandler for Server {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl FractionalScaleHandler for Server {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        // If the element already has a preferred scale, send it to the new object.
        let mut root = surface.clone();
        while let Some(p) = get_parent(&root) {
            root = p;
        }
        if let Some(ei) = self.elem_by_root(&root) {
            if let Some(s) = self.elements[ei].scale {
                self.set_element_scale(ei, s);
            }
        }
    }
}
impl DmabufHandler for Server {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }
    fn dmabuf_imported(&mut self, _g: &DmabufGlobal, _d: Dmabuf, notifier: ImportNotifier) {
        // The harness never samples buffers; accepting is sufficient for protocol behavior.
        let _ = notifier.successful::<Server>();
    }
}

delegate_compositor!(Server);
delegate_xdg_shell!(Server);
delegate_shm!(Server);
delegate_output!(Server);
delegate_seat!(Server);
delegate_data_device!(Server);
delegate_fractional_scale!(Server);
delegate_viewporter!(Server);
delegate_dmabuf!(Server);

pub fn surface_id(s: &WlSurface) -> String {
    format!("{}", s.id().protocol_id())
}
