//! Tearing (`tearing`) with a real client: a fullscreen window tears when its app asks
//! (`wp_tearing_control_v1` hint `async`) or the user turned it on (Super+Shift+T); not when it
//! is not fullscreen, after the hint goes back to vsync or the object is destroyed, with
//! `TEARING` off, or while locked. The backend flips async only when `tears` holds for a frame
//! it scans out.

mod common;

use std::os::fd::AsFd;
use std::time::Duration;

use common::{Lockstep, output};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;
use wayland_client::protocol::wl_shm;
use wayland_protocols::wp::tearing_control::v1::client::{
    wp_tearing_control_manager_v1::WpTearingControlManagerV1,
    wp_tearing_control_v1::{PresentationHint, WpTearingControlV1},
};

wayland_client::delegate_noop!(common::Probe: ignore WpTearingControlManagerV1);
wayland_client::delegate_noop!(common::Probe: ignore WpTearingControlV1);

struct T {
    ls: Lockstep,
}

impl T {
    fn new() -> T {
        T {
            ls: Lockstep::new(Some(output((640, 480), 120))),
        }
    }

    fn adv(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    fn paint(&mut self, idx: usize) {
        let (w, h) = self.ls.probe.surfaces[idx].size;
        let len = (w * h * 4) as usize;
        let fd = smithay::reexports::rustix::fs::memfd_create(
            "infynitae-t1-tearing",
            smithay::reexports::rustix::fs::MemfdFlags::CLOEXEC,
        )
        .expect("memfd");
        let file = std::fs::File::from(fd);
        std::os::unix::fs::FileExt::write_all_at(&file, &vec![0xFF; len], 0).expect("fill");
        let shm = self.ls.probe.shm.clone().expect("wl_shm");
        let pool = shm.create_pool(file.as_fd(), len as i32, &self.ls.qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &self.ls.qh, ());
        let wl = &self.ls.probe.surfaces[idx].wl;
        wl.attach(Some(&buffer), 0, 0);
        wl.damage_buffer(0, 0, w, h);
        wl.commit();
        self.adv();
    }

    fn map(&mut self, label: &'static str) -> (usize, ElementId) {
        let qh = self.ls.qh.clone();
        let idx = self.ls.probe.toplevel(&qh, label, (200, 100), 0);
        self.adv();
        self.paint(idx);
        let id = self.ls.state.elements().last().expect("element");
        set_focus(&mut self.ls.state, &mut self.ls.input, Some(id)).expect("focus");
        self.adv();
        (idx, id)
    }

    fn fullscreen(&mut self, idx: usize, on: bool) {
        let tl = self.ls.probe.surfaces[idx]
            .toplevel
            .clone()
            .expect("toplevel");
        if on {
            tl.set_fullscreen(None);
        } else {
            tl.unset_fullscreen();
        }
        self.adv();
        self.ls.probe.surfaces[idx].size = if on { (640, 480) } else { (200, 100) };
        self.paint(idx);
    }

    fn control(&mut self, idx: usize) -> WpTearingControlV1 {
        let (name, _, _) = self
            .ls
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "wp_tearing_control_manager_v1")
            .cloned()
            .expect("wp_tearing_control_manager_v1 is offered");
        let qh = self.ls.qh.clone();
        let m: WpTearingControlManagerV1 = self.ls.registry().bind(name, 1, &qh, ());
        let wl = self.ls.probe.surfaces[idx].wl.clone();
        let c = m.get_tearing_control(&wl, &qh, ());
        self.adv();
        c
    }

    fn hint(&mut self, c: &WpTearingControlV1, idx: usize, hint: PresentationHint) {
        c.set_presentation_hint(hint);
        // the hint is double-buffered: it applies at the surface's next commit
        self.ls.probe.surfaces[idx].wl.commit();
        self.adv();
    }
}

#[test]
fn a_game_that_asks_tears_while_fullscreen() {
    let mut t = T::new();
    let o = t.ls.output();
    let (w, _) = t.map("game");
    let c = t.control(w);
    t.hint(&c, w, PresentationHint::Async);
    assert!(!t.ls.state.tears(o), "not fullscreen: no tearing");

    t.fullscreen(w, true);
    assert!(t.ls.state.tears(o), "fullscreen and asked: tears");

    t.hint(&c, w, PresentationHint::Vsync);
    assert!(!t.ls.state.tears(o), "back to vsync");
    t.hint(&c, w, PresentationHint::Async);
    assert!(t.ls.state.tears(o));
    c.destroy();
    t.adv();
    assert!(!t.ls.state.tears(o), "the object destroyed: vsync");
}

#[test]
fn super_shift_t_makes_a_window_tear_that_cannot_ask() {
    let mut t = T::new();
    let o = t.ls.output();
    let (w, id) = t.map("x11 game");
    t.fullscreen(w, true);
    assert!(!t.ls.state.tears(o), "nobody asked");

    assert!(t.ls.state.toggle_tearing(id), "on");
    assert!(t.ls.state.tears(o));
    t.fullscreen(w, false);
    assert!(!t.ls.state.tears(o), "only while fullscreen");
    t.fullscreen(w, true);
    assert!(t.ls.state.tears(o), "kept for the window");

    assert!(!t.ls.state.toggle_tearing(id), "off");
    assert!(!t.ls.state.tears(o));
}

#[test]
fn tearing_off_in_the_config_or_a_locked_session_never_tears() {
    let mut t = T::new();
    let o = t.ls.output();
    let (w, id) = t.map("game");
    t.fullscreen(w, true);
    t.ls.state.toggle_tearing(id);
    assert!(t.ls.state.tears(o));

    t.ls.state.set_tearing_enabled(false);
    assert!(!t.ls.state.tears(o), "TEARING=off");
    t.ls.state.set_tearing_enabled(true);
    assert!(t.ls.state.tears(o));

    t.ls.state.start_locked();
    t.adv();
    assert!(!t.ls.state.tears(o), "locked");
}

#[test]
fn a_second_tearing_object_for_one_surface_is_a_protocol_error() {
    let mut t = T::new();
    let (w, _) = t.map("game");
    let _c = t.control(w);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.control(w)));
    assert!(res.is_err(), "tearing_control_exists");
}
