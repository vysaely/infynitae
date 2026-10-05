//! T1 lockstep harness (FOUNDATION_TEST_PLAN T1): the production core, adapter and headless
//! backend in one thread with a recording probe client over a socketpair. Deterministic, no real
//! time, no host compositor, no socket on disk, no `unsafe`.

#![allow(dead_code)]

use std::fs::File;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use infynitae_backend_headless::{VirtualOutput, connect};
use infynitae_core::output::{OutputId, Transform};
use infynitae_core::{Core, OutputLifecycleToken};
use infynitae_wayland::{ClientState, WaylandState};
use smithay::reexports::wayland_server::Display;
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_compositor::WlCompositor,
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{
    xdg_popup::{self, XdgPopup},
    xdg_positioner::{self, XdgPositioner},
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
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
}

#[derive(Default)]
pub struct Probe {
    globals: Vec<(u32, String, u32)>,
    pub compositor: Option<WlCompositor>,
    pub subcompositor: Option<WlSubcompositor>,
    pub shm: Option<WlShm>,
    pub wm_base: Option<XdgWmBase>,
    pub surfaces: Vec<Surf>,
    files: Vec<File>,
}

/// Unique shm file names across the tests of this process.
static SHM_FILES: AtomicU64 = AtomicU64::new(0);

impl Probe {
    fn buffer(&mut self, qh: &QueueHandle<Self>, w: i32, h: i32) -> WlBuffer {
        let stride = w * 4;
        let len = (stride * h).max(4);
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
        std::fs::remove_file(&path).expect("unlink shm file");
        file.set_len(len as u64).expect("size shm file");
        let pool: WlShmPool = self.shm.as_ref().expect("wl_shm").create_pool(
            std::os::fd::AsFd::as_fd(&file),
            len,
            qh,
            (),
        );
        let buf = pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        self.files.push(file);
        buf
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, idx: usize) {
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
        });
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

    pub fn destroy_toplevel(&mut self, idx: usize) {
        let s = &self.surfaces[idx];
        if let Some(t) = &s.toplevel {
            t.destroy();
        }
        if let Some(x) = &s.xdg {
            x.destroy();
        }
        s.wl.destroy();
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
        _: &WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            st.globals.push((name, interface, version));
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
        _: &mut Self,
        _: &XdgToplevel,
        _: xdg_toplevel::Event,
        _: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
        if let xdg_popup::Event::Configure {
            x,
            y,
            width,
            height,
        } = ev
        {
            st.surfaces[*idx].popup_configure = Some((x, y, width, height));
        }
    }
}

delegate_noop!(Probe: ignore WlCompositor);
delegate_noop!(Probe: ignore WlSubcompositor);
delegate_noop!(Probe: ignore WlShm);
delegate_noop!(Probe: ignore WlShmPool);
delegate_noop!(Probe: ignore WlBuffer);
delegate_noop!(Probe: ignore WlSurface);
delegate_noop!(Probe: ignore WlSubsurface);
delegate_noop!(Probe: ignore WlRegion);
delegate_noop!(Probe: ignore XdgPositioner);

/// The production stack plus one probe client. The test is the composition root: it holds the
/// output-lifecycle token and connects the virtual output through the backend.
pub struct Lockstep {
    pub display: Display<WaylandState>,
    pub state: WaylandState,
    pub token: OutputLifecycleToken,
    pub output: Option<OutputId>,
    pub conn: Connection,
    pub queue: EventQueue<Probe>,
    pub probe: Probe,
    pub qh: QueueHandle<Probe>,
}

impl Lockstep {
    pub fn new(output: Option<VirtualOutput>) -> Lockstep {
        let (core, _input, _shell, token) = Core::new();
        let display: Display<WaylandState> = Display::new().expect("display");
        let state = WaylandState::new(&display.handle(), core);
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end
            .set_nonblocking(true)
            .expect("non-blocking client socket");
        display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert client");
        let conn = Connection::from_socket(client_end).expect("client connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let _registry = conn.display().get_registry(&qh, ());
        let mut ls = Lockstep {
            display,
            state,
            token,
            output: None,
            conn,
            queue,
            probe: Probe::default(),
            qh,
        };
        if let Some(o) = output {
            ls.output = Some(connect(&mut ls.state, &mut ls.token, &o).expect("connect output"));
        }
        ls.pump();
        ls.bind();
        ls
    }

    fn bind(&mut self) {
        let registry = self.conn.display().get_registry(&self.qh, ());
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

pub fn output(mode: (u32, u32), scale_120: u32) -> VirtualOutput {
    VirtualOutput {
        mode,
        transform: Transform::Normal,
        scale_120,
        refresh_mhz: 60_000,
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
