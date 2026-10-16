//! INC-12 T1 (EXPERIMENTAL): `xdg_activation_v1` on the production path (B10.7; INV-INPUT-9;
//! W-SM-8; outcome policy PROV-10, provisional).
//!
//! The probe client maps W and V; W has the keyboard focus. A token is valid only if the client
//! created it with a serial of the seat no older than the keyboard's last focus `enter` (W-SM-8:
//! Smithay alone accepts every token). A valid token activates V: the keyboard focus moves to V
//! and no camera moves. A token without a serial, or with a serial from before the last focus
//! change, activates nothing.

mod common;

use common::{Lockstep, output};
use infynitae_backend_headless::set_focus;
use infynitae_core::stage::ElementId;

struct S {
    ls: Lockstep,
    w: usize,
    v: usize,
    ew: ElementId,
    ev: ElementId,
}

fn scene() -> S {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    ls.pump();
    ls.probe.draw(&qh, w);
    ls.pump();
    let v = ls.probe.toplevel(&qh, "V", (300, 200), 0);
    ls.pump();
    ls.probe.draw(&qh, v);
    ls.pump();
    let ids: Vec<_> = ls.state.elements().collect();
    let (ew, ev) = (ids[0], ids[1]);
    set_focus(&mut ls.state, &mut ls.input, Some(ew)).unwrap();
    ls.pump();
    assert_eq!(ls.probe.kb.active.as_deref(), Some("W"));
    S { ls, w, v, ew, ev }
}

impl S {
    /// A token created by the client, with `serial` (or none) and W as the requesting surface.
    fn token(&mut self, serial: Option<u32>) -> String {
        let activation = self.ls.activation();
        let qh = self.ls.qh.clone();
        let token = activation.get_activation_token(&qh, ());
        if let Some(serial) = serial {
            token.set_serial(serial, self.ls.probe.seat.as_ref().unwrap());
        }
        token.set_surface(&self.ls.probe.surfaces[self.w].wl);
        token.commit();
        self.ls.pump();
        self.ls.probe.activation_tokens.pop().expect("a token")
    }

    fn activate(&mut self, token: &str, idx: usize) {
        let activation = self.ls.activation();
        activation.activate(token.to_string(), &self.ls.probe.surfaces[idx].wl);
        self.ls.pump();
    }

    fn camera(&self) -> String {
        let c = self.ls.state.core().cameras();
        format!(
            "{:?}",
            (0..c.len())
                .map(|i| (c.center(i), c.zoom(i)))
                .collect::<Vec<_>>()
        )
    }
}

/// A token created with the serial of the focus `enter` W received activates V: V gets the
/// keyboard focus (with its configure), and no camera moves (INV-INPUT-9).
#[test]
fn a_token_from_the_focused_client_activates_a_window() {
    let mut s = scene();
    let camera = s.camera();
    let serial = s.ls.probe.last_serial;
    let token = s.token(Some(serial));
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ev));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("V"));
    assert_eq!(s.camera(), camera, "activation never moves a camera");
    // a token is used once
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ew)).unwrap();
    s.ls.pump();
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew), "spent");
}

/// W-SM-8: a token without a serial is refused at creation; activating with it does nothing.
#[test]
fn a_token_without_a_serial_activates_nothing() {
    let mut s = scene();
    let token = s.token(None);
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew));
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("W"));
}

/// W-SM-8: a serial from before the last focus change does not show that the user just acted
/// in the requesting client; the token is refused.
#[test]
fn a_token_with_a_serial_older_than_the_last_focus_change_activates_nothing() {
    let mut s = scene();
    let old = s.ls.probe.last_serial;
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ev)).unwrap();
    s.ls.pump();
    set_focus(&mut s.ls.state, &mut s.ls.input, Some(s.ew)).unwrap();
    s.ls.pump();
    assert!(s.ls.probe.last_serial != old, "a newer enter");
    let token = s.token(Some(old));
    s.activate(&token, s.v);
    assert_eq!(s.ls.state.core().focus().target(), Some(s.ew));
}
