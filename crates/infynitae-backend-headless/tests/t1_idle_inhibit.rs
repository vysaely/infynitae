//! `zwp_idle_inhibit_manager_v1` with a real client (a video player): an inhibitor on a mapped
//! window keeps the screens on (the backends skip the idle timeout while `is_idle_inhibited`);
//! destroying it, a surface that is no window, and a locked session do not.

mod common;

use common::input::*;
use wayland_protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
};

wayland_client::delegate_noop!(common::Probe: ignore ZwpIdleInhibitManagerV1);
wayland_client::delegate_noop!(common::Probe: ignore ZwpIdleInhibitorV1);

fn manager(r: &mut Rig) -> ZwpIdleInhibitManagerV1 {
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwp_idle_inhibit_manager_v1")
            .cloned()
            .expect("zwp_idle_inhibit_manager_v1 is offered");
    let qh = r.ls.qh.clone();
    r.ls.registry().bind(name, 1, &qh, ())
}

#[test]
fn a_window_playing_video_keeps_the_screens_on_until_it_stops() {
    let mut r = Rig::x8();
    let m = manager(&mut r);
    assert!(!r.ls.state.is_idle_inhibited());
    let qh = r.ls.qh.clone();
    let w =
        r.ls.probe
            .surfaces
            .iter()
            .find(|s| s.label == "W")
            .unwrap()
            .wl
            .clone();
    let inhibitor = m.create_inhibitor(&w, &qh, ());
    r.ls.pump();
    assert!(
        r.ls.state.is_idle_inhibited(),
        "the window keeps the screens on"
    );

    inhibitor.destroy();
    r.ls.pump();
    assert!(
        !r.ls.state.is_idle_inhibited(),
        "stopped: the timeout applies again"
    );
}

#[test]
fn a_surface_that_is_no_window_keeps_nothing_on() {
    let mut r = Rig::x8();
    let m = manager(&mut r);
    let qh = r.ls.qh.clone();
    let bare =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("compositor")
            .create_surface(&qh, ());
    let _inhibitor = m.create_inhibitor(&bare, &qh, ());
    r.ls.pump();
    assert!(!r.ls.state.is_idle_inhibited());
}

#[test]
fn a_locked_session_goes_dark_whatever_an_app_asks() {
    let mut r = Rig::x8();
    let m = manager(&mut r);
    let qh = r.ls.qh.clone();
    let w =
        r.ls.probe
            .surfaces
            .iter()
            .find(|s| s.label == "W")
            .unwrap()
            .wl
            .clone();
    let _inhibitor = m.create_inhibitor(&w, &qh, ());
    r.ls.pump();
    assert!(r.ls.state.is_idle_inhibited());
    r.ls.state.start_locked();
    r.ls.pump();
    assert!(
        !r.ls.state.is_idle_inhibited(),
        "locked: the screens may go off"
    );
}
