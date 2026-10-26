//! Game mode (`background`): a fullscreen window the GPU shows directly is the game until it is
//! no longer fullscreen; a composited frame or a window that is not fullscreen is no game.
//!
//! The headless backend composites every frame, so the DRM backend's report of a direct
//! scanout (`frame_scanned_out`) is made by hand.

mod common;

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use common::{Lockstep, output};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;
use wayland_client::protocol::wl_shm;

struct T {
    ls: Lockstep,
}

impl T {
    fn adv(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(16_667);
        self.ls.run_until(t);
    }

    fn paint(&mut self, idx: usize) {
        let (w, h) = self.ls.probe.surfaces[idx].size;
        let len = (w * h * 4) as usize;
        let fd = smithay::reexports::rustix::fs::memfd_create(
            "infynitae-t1-game-mode",
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
        // the client obeys the configure's size
        let size = if on { (640, 480) } else { (200, 100) };
        self.ls.probe.surfaces[idx].size = size;
        self.paint(idx);
        self.ls.state.background_update(Instant::now());
    }
}

#[test]
fn a_fullscreen_window_shown_directly_is_the_game_until_it_leaves_fullscreen() {
    let mut t = T {
        ls: Lockstep::new(Some(output((640, 480), 120))),
    };
    let o = t.ls.output();
    let (w, id) = t.map("game");
    let pid = std::process::id();

    // not fullscreen: a scanout report changes nothing
    t.ls.state.frame_scanned_out(o, true);
    assert_eq!(t.ls.state.game(), None);

    t.fullscreen(w, true);
    assert_eq!(t.ls.state.core().presentation(o), Some(id));
    // fullscreen but composited: no game
    t.ls.state.frame_scanned_out(o, false);
    assert_eq!(t.ls.state.game(), None);
    // shown directly: the game
    t.ls.state.frame_scanned_out(o, true);
    assert_eq!(t.ls.state.game(), Some((id, pid)));
    // a composited frame on the way (a notification over it) does not end it
    t.ls.state.frame_scanned_out(o, false);
    t.ls.state.background_update(Instant::now());
    assert_eq!(t.ls.state.game(), Some((id, pid)));

    t.fullscreen(w, false);
    assert_eq!(t.ls.state.game(), None, "game mode ends with fullscreen");
}

#[test]
fn closing_the_game_ends_game_mode() {
    let mut t = T {
        ls: Lockstep::new(Some(output((640, 480), 120))),
    };
    let o = t.ls.output();
    let (w, id) = t.map("game");
    t.fullscreen(w, true);
    t.ls.state.frame_scanned_out(o, true);
    assert_eq!(t.ls.state.game().map(|g| g.0), Some(id));

    let tl = t.ls.probe.surfaces[w].toplevel.take().expect("toplevel");
    tl.destroy();
    t.ls.probe.surfaces[w].wl.attach(None, 0, 0);
    t.ls.probe.surfaces[w].wl.commit();
    t.adv();
    t.ls.state.background_update(Instant::now());
    assert_eq!(t.ls.state.game(), None);
}
