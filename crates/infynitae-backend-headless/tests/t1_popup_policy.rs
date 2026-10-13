//! INC-7 slice 3 (experimental): popup robustness on the production path (OB-18, W-SM-6).
//!
//! A client that attaches a buffer to a popup before the popup was configured, or before it
//! acknowledged its configure, violates xdg-shell. The compositor answers with the protocol
//! error on its `xdg_surface`, the client's connection ends, and the compositor is
//! left with no trace of the client: no element, no grab, no focus on a dead surface. The
//! oracle is the error the client's own connection reports and the compositor's state.
//!
//! Nested-popup parity (OB-1) is in `t1_parity`; popups over a fullscreen window are in
//! `t1_fullscreen`.

mod common;

use std::time::Duration;

use common::{Lockstep, PopupSpec, output};
use infynitae_backend_headless::set_focus;

/// The xdg_surface errors that say "a buffer on a surface that was not configured":
/// `unconfigured_buffer` (3) is the protocol's name for it; Smithay 0.7.0 reports
/// `not_constructed` (1), "Surface has not been configured yet". Either ends the client; which
/// code is sent is the library's choice and is recorded as a finding, not asserted.
const UNCONFIGURED: [u32; 2] = [1, 3];

fn assert_unconfigured_error(got: Option<(String, u32)>) {
    let (interface, code) = got.expect("the client received a protocol error");
    assert_eq!(interface, "xdg_surface");
    assert!(UNCONFIGURED.contains(&code), "xdg_surface error {code}");
}

fn spec() -> PopupSpec {
    PopupSpec {
        content: (120, 80),
        shadow: 0,
        anchor_rect: (10, 10, 1, 1),
        slide: false,
    }
}

/// Server and client turns that tolerate the client's connection ending: returns the protocol
/// error the client received, as (interface, code), once it has one.
fn pump_until_error(ls: &mut Lockstep) -> Option<(String, u32)> {
    for _ in 0..50 {
        ls.display
            .dispatch_clients(&mut ls.state)
            .expect("the compositor keeps dispatching");
        let _ = ls.display.flush_clients();
        let _ = ls.conn.flush();
        if let Some(guard) = ls.conn.prepare_read() {
            let _ = guard.read();
        }
        let _ = ls.queue.dispatch_pending(&mut ls.probe);
        if let Some(e) = ls.conn.protocol_error() {
            return Some((e.object_interface, e.code));
        }
    }
    None
}

/// What must be true of the compositor once the offending client is gone.
fn assert_no_trace(ls: &mut Lockstep, what: &str) {
    // a few more turns: the compositor must keep running without the client
    for _ in 0..5 {
        ls.display
            .dispatch_clients(&mut ls.state)
            .expect("the compositor survives");
        let _ = ls.display.flush_clients();
    }
    let core = ls.state.core();
    assert!(
        core.stage().z_order().is_empty(),
        "{what}: an element is left"
    );
    assert_eq!(
        core.focus().target(),
        None,
        "{what}: focus on a dead element"
    );
    assert!(
        core.membership().rows().is_empty(),
        "{what}: membership rows left"
    );
    assert!(
        !ls.state.keyboard_is_grabbed(),
        "{what}: keyboard grab left"
    );
    // Smithay's popup pointer grab notices that its popup is gone at the next pointer event and
    // removes itself then; until an event arrives there is nothing for it to route. One motion
    // and one click later no grab may be left.
    let grabbed_before = ls.state.is_pointer_grabbed();
    infynitae_backend_headless::pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (3.0, 2.0),
        (3.0, 2.0),
        900_000,
    )
    .expect("motion after the client is gone");
    for pressed in [true, false] {
        infynitae_backend_headless::pointer_button(
            &mut ls.state,
            &mut ls.input,
            0x110,
            pressed,
            infynitae_core::input::bindings::Modifiers::default(),
            900_001 + u64::from(!pressed),
        )
        .expect("button after the client is gone");
    }
    let _ = ls.display.dispatch_clients(&mut ls.state);
    assert!(
        !ls.state.is_pointer_grabbed(),
        "{what}: pointer grab left after the next pointer events (grabbed before them: {grabbed_before})"
    );
    assert!(
        ls.state.core().buttons().pressed().is_empty(),
        "{what}: a button is held"
    );
    assert_eq!(
        ls.state.elements().count(),
        0,
        "{what}: adapter element left"
    );
    // the output still renders (an empty frame), so the compositor is alive and consistent
    let o = ls.output();
    assert!(
        ls.state
            .core_mut()
            .render_list(o)
            .unwrap()
            .entries
            .is_empty()
    );
}

fn mapped_parent(ls: &mut Lockstep) -> usize {
    let qh = ls.qh.clone();
    let w = ls.probe.toplevel(&qh, "W", (400, 300), 0);
    ls.pump();
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
    let id = ls.state.elements().next().expect("element");
    set_focus(&mut ls.state, &mut ls.input, Some(id)).expect("focus");
    ls.pump();
    w
}

/// The popup's very first commit carries a buffer: there has been no configure at all.
#[test]
fn a_buffer_in_a_popups_initial_commit_is_a_protocol_error() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let w = mapped_parent(&mut ls);
    let qh = ls.qh.clone();
    let p = ls.probe.popup_uncommitted(&qh, "P", w, spec());
    ls.probe.draw(&qh, p); // attach + commit, as the initial commit
    assert_unconfigured_error(pump_until_error(&mut ls));
    assert_no_trace(&mut ls, "buffer in the initial commit");
}

/// The popup made its initial commit and was sent a configure, then commits a buffer without
/// acknowledging it (W-SM-6: Smithay checks this only if asked on every role commit).
#[test]
fn a_popup_buffer_before_the_configure_is_acknowledged_is_a_protocol_error() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let w = mapped_parent(&mut ls);
    let qh = ls.qh.clone();
    let p = ls.probe.popup(&qh, "P", w, spec()); // initial commit, not yet dispatched
    ls.probe.draw(&qh, p); // a buffer right behind it: no configure was acknowledged
    assert_unconfigured_error(pump_until_error(&mut ls));
    assert_no_trace(&mut ls, "buffer before ack");
}

/// The same offence inside a grabbing chain two popups deep: the grabs of the whole chain go
/// with the client, and a second, well-behaved client of the same compositor is unaffected.
#[test]
fn an_offending_nested_popup_takes_its_chain_and_nothing_else() {
    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let w = mapped_parent(&mut ls);
    let qh = ls.qh.clone();
    // a click on the window gives a serial for the grab
    for pressed in [true, false] {
        infynitae_backend_headless::pointer_button(
            &mut ls.state,
            &mut ls.input,
            0x110,
            pressed,
            infynitae_core::input::bindings::Modifiers::default(),
            20_000 + u64::from(!pressed),
        )
        .expect("button");
        ls.pump();
    }
    let serial = ls.probe.last_serial;
    let p1 = ls
        .probe
        .popup_with_grab_serial(&qh, "P1", w, spec(), serial);
    ls.pump();
    assert!(ls.state.keyboard_is_grabbed(), "the chain holds its grab");
    let p2 = ls.probe.popup(&qh, "P2", p1, spec());
    ls.probe.draw(&qh, p2);
    assert_unconfigured_error(pump_until_error(&mut ls));
    assert_no_trace(&mut ls, "offending nested popup");
}
