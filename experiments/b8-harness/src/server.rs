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
    delegate_output, delegate_pointer_constraints, delegate_presentation, delegate_relative_pointer, delegate_seat, delegate_shm,
    delegate_viewporter, delegate_xdg_shell,
    desktop::{find_popup_root_surface, space::SpaceElement, utils::OutputPresentationFeedback, PopupGrab, PopupKeyboardGrab, PopupKind, PopupManager, PopupPointerGrab, PopupUngrabStrategy, Window},
    input::pointer::Focus,
    input::{pointer::PointerHandle, Seat, SeatHandler, SeatState},
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
        pointer_constraints::{with_pointer_constraint, PointerConstraintsHandler, PointerConstraintsState},
        presentation::{PresentationState, Refresh},
        relative_pointer::RelativePointerManagerState,
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
    pub constraints_state: PointerConstraintsState,
    pub relative_state: RelativePointerManagerState,
    pub presentation_state: PresentationState,
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
    /// constraint-activation policy switch (X-12 models a policy that declines activation,
    /// e.g. pointer outside a lock region)
    pub activation_allowed: bool,
    pub snapshots_taken: u64,
    pub last_snapshot_len: usize,
    /// (time, element id, surface protocol id, buffer size in buffer pixels) for every commit with a buffer
    pub commit_sizes: Vec<(u64, u64, u32, (i32, i32))>,
    /// Phase D 1R: honour xdg_popup.grab with Smithay's PopupGrab (off by default: earlier experiments
    /// ran with the no-op handler)
    pub popup_grabs_enabled: bool,
    pub popup_grab: Option<PopupGrab<Server>>,
    /// Phase D 1R (PD-F2-17): Foundation maximize policy — answer set/unset_maximized with a configure
    /// that does not grant `maximized` (off by default: record-only W-SM-13 override)
    pub maximize_policy: bool,
    /// Phase D 1R: for a NESTED popup grab keep the installed PopupPointerGrab (it shares the grab chain)
    /// instead of replacing it; replacing runs the old grab's unset(), which tears down the keyboard grab
    pub nested_grab_keep_pointer: bool,
    /// Phase D 1R (PD-F2-10): Foundation fullscreen answer policy; `focused` stands in for core
    /// ActiveElementFocus (element id). Off by default (record-only W-SM-13 override).
    pub fullscreen_policy: bool,
    pub focused: Option<u64>,
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
            constraints_state: PointerConstraintsState::new::<Self>(dh),
            relative_state: RelativePointerManagerState::new::<Self>(dh),
            presentation_state: PresentationState::new::<Self>(dh, libc::CLOCK_MONOTONIC as u32),
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
            activation_allowed: true,
            snapshots_taken: 0,
            last_snapshot_len: 0,
            commit_sizes: Vec::new(),
            popup_grabs_enabled: false,
            popup_grab: None,
            maximize_policy: false,
            nested_grab_keep_pointer: false,
            fullscreen_policy: false,
            focused: None,
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
                // modeled "presentation": the element's current content is shown on its clock output
                let mut fb = OutputPresentationFeedback::new(&out);
                self.elements[ei].window.take_presentation_feedback(&mut fb, |_, _| Some(o2.clone()), |_, _| {
                    smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind::empty()
                });
                let period = Duration::from_nanos((1e12 / self.outputs[oi].refresh_mhz as f64) as u64);
                fb.presented::<_, smithay::utils::Monotonic>(t, Refresh::Fixed(period), 0, smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback::Kind::empty());
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
                // dispatch-safety check: extract the full surface-tree snapshot INSIDE the commit
                // handler, as the production adapter will (no re-entrant with_states deadlock)
                let bsize = with_renderer_surface_state(surface, |st| st.buffer_size().map(|sz| (sz.w, sz.h))).flatten();
                if let Some(b) = bsize {
                    let id = self.elements[ei].id;
                    // buffer_size() is logical; multiply back by the buffer scale for buffer pixels
                    let bs = with_renderer_surface_state(surface, |st| st.buffer_scale()).unwrap_or(1);
                    self.commit_sizes.push((self.now_us, id, surface.id().protocol_id(), (b.0 * bs, b.1 * bs)));
                }
                let snap = crate::snapshot::extract(&self.elements[ei].window);
                self.snapshots_taken += 1;
                self.last_snapshot_len = snap.surfaces.len();
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
                let (title, app_id) = smithay::wayland::compositor::with_states(surface, |st| {
                    st.data_map
                        .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                        .map(|d| {
                            let d = d.lock().unwrap();
                            (d.title.clone(), d.app_id.clone())
                        })
                        .unwrap_or((None, None))
                });
                let anchor = self.elements[ei].anchor;
                self.note(format!("E{id} mapped={has_buffer} title={title:?} app_id={app_id:?} anchor={anchor:?}"));
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
        let gone: Vec<u64> = self.elements.iter().filter(|e| e.window.toplevel() == Some(&surface)).map(|e| e.id).collect();
        self.elements.retain(|e| e.window.toplevel() != Some(&surface));
        for id in gone {
            self.note(format!("E{id} toplevel_destroyed (element removed from Stage)"));
        }
    }
    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|s| s.geometry = positioner.get_geometry());
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }
    fn reposition_request(&mut self, _s: PopupSurface, _p: PositionerState, _t: u32) {}
    // Smithay's DEFAULT implementations of these two call `send_configure()` immediately (W-SM-13).
    // Overridden: record only; configures are sent by the compositor's own policy path.
    fn maximize_request(&mut self, surface: ToplevelSurface) {
        let id = self.elements.iter().find(|e| e.window.toplevel() == Some(&surface)).map(|e| e.id);
        self.note(format!("E{} maximize_request (initial configure sent: {})", id.unwrap_or(0), surface.is_initial_configure_sent()));
        self.answer_maximize(&surface);
    }
    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        let id = self.elements.iter().find(|e| e.window.toplevel() == Some(&surface)).map(|e| e.id);
        self.note(format!("E{} unmaximize_request", id.unwrap_or(0)));
        self.answer_maximize(&surface);
    }
    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>) {
        let id = self.elements.iter().find(|e| e.window.toplevel() == Some(&surface)).map(|e| e.id);
        self.note(format!("E{} fullscreen_request output={} (initial configure sent: {})", id.unwrap_or(0), output.is_some(), surface.is_initial_configure_sent()));
        if self.fullscreen_policy && surface.is_initial_configure_sent() {
            // 1R: answer every request with a configure. Grant only to the focused element; else decline.
            let grant = id.is_some() && id == self.focused;
            let size = self.outputs.first().map(|o| o.size);
            surface.with_pending_state(|s| {
                if grant {
                    s.states.set(xdg_toplevel::State::Fullscreen);
                    s.size = size.map(|z| z.into());
                } else {
                    s.states.unset(xdg_toplevel::State::Fullscreen);
                }
            });
            surface.send_configure();
            self.note(format!("fullscreen policy: {}", if grant { "granted (focused)" } else { "declined (not focused): configure without `fullscreen`" }));
        }
    }
    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if self.fullscreen_policy && surface.is_initial_configure_sent() {
            surface.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Fullscreen);
                s.size = None;
            });
            surface.send_configure();
            self.note("fullscreen policy: unset answered with a configure without `fullscreen`".into());
        }
    }
    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        if !self.popup_grabs_enabled {
            return;
        }
        let kind = PopupKind::Xdg(surface);
        // W-SM-7 precondition: the root must exist and be an element root, else grab_popup asserts
        let Ok(root) = find_popup_root_surface(&kind) else {
            self.note("popup grab refused: root dead".into());
            return;
        };
        if self.elem_by_root(&root).is_none() {
            self.note("popup grab refused: root is not an element".into());
            return;
        }
        let seat = self.seat.clone();
        match self.popups.grab_popup::<Server>(root, kind, &seat, serial) {
            Ok(mut grab) => {
                let kb = seat.get_keyboard().unwrap();
                if kb.is_grabbed() && !(kb.has_grab(serial) || kb.has_grab(grab.previous_serial().unwrap_or(serial))) {
                    grab.ungrab(PopupUngrabStrategy::All);
                    self.note("popup grab refused: keyboard grabbed by another sequence".into());
                    return;
                }
                kb.set_focus(self, grab.current_grab(), serial);
                kb.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
                let ptr = seat.get_pointer().unwrap();
                let nested = grab.previous_serial().is_some();
                if nested && self.nested_grab_keep_pointer && ptr.is_grabbed() {
                    self.note("nested popup grab: existing PopupPointerGrab kept".into());
                } else if !(ptr.is_grabbed() && !(ptr.has_grab(serial) || ptr.has_grab(grab.previous_serial().unwrap_or(serial)))) {
                    ptr.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
                }
                self.note("popup grab established (Smithay PopupKeyboardGrab + PopupPointerGrab)".into());
                self.popup_grab = Some(grab);
            }
            Err(e) => self.note(format!("popup grab refused by Smithay: {e:?}")),
        }
    }
    fn client_pong(&mut self, _client: smithay::wayland::shell::xdg::ShellClient) {
        let t = self.now_us;
        self.pongs.push(t);
    }
}

impl Server {
    /// PD-F2-17 policy under test: answer with a configure; never grant `maximized`; no Stage/camera change.
    fn answer_maximize(&mut self, surface: &ToplevelSurface) {
        if !self.maximize_policy || !surface.is_initial_configure_sent() {
            return; // pre-map: recorded only; the initial configure proceeds normally
        }
        surface.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Maximized);
        });
        surface.send_configure();
        self.note("maximize policy: declined; configure sent without `maximized`".into());
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
delegate_pointer_constraints!(Server);
delegate_relative_pointer!(Server);
delegate_presentation!(Server);

impl PointerConstraintsHandler for Server {
    /// Policy under test (B10): a constraint is activated when it is created on the surface that
    /// currently has pointer focus; otherwise when that surface next gains focus (see input.rs).
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if self.activation_allowed && pointer.current_focus().as_ref() == Some(surface) {
            with_pointer_constraint(surface, pointer, |c| {
                if let Some(c) = c {
                    if !c.is_active() {
                        c.activate();
                    }
                }
            });
            self.note("constraint activated on creation (surface focused)".into());
        }
    }
    fn cursor_position_hint(&mut self, _s: &WlSurface, _p: &PointerHandle<Self>, _l: smithay::utils::Point<f64, Logical>) {}
}

pub fn surface_id(s: &WlSurface) -> String {
    format!("{}", s.id().protocol_id())
}
