//! wlr-foreign-toplevel-management-v1 with a real client (a task bar): the windows are listed
//! with their title, app id and state, changes follow, and the requests act on the window.
//! Activating moves the focus only when the asking client holds the pointer or the keyboard.

mod common;

use std::time::Duration;

use common::{Lockstep, Probe, output};
use infynitae_backend_headless::{pointer_motion, set_focus};
use infynitae_core::stage::ElementId;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::{
    self, ZwlrForeignToplevelHandleV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::{
    self, ZwlrForeignToplevelManagerV1,
};

impl wayland_client::Dispatch<ZwlrForeignToplevelManagerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        ev: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = ev {
            st.log.push("toplevel".into());
            st.wlr_handles.push(toplevel);
        }
    }

    wayland_client::event_created_child!(Probe, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

/// The handle's events go to the log: `title <t>`, `app_id <a>`, `state <list>`, `done`, `closed`.
impl wayland_client::Dispatch<ZwlrForeignToplevelHandleV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrForeignToplevelHandleV1,
        ev: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        st.log.push(match ev {
            Event::Title { title } => format!("title {title}"),
            Event::AppId { app_id } => format!("app_id {app_id}"),
            Event::State { state } => {
                let s: Vec<u32> = state
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_ne_bytes(*b))
                    .collect();
                format!("state {s:?}")
            }
            Event::Done => "done".into(),
            Event::Closed => "closed".into(),
            _ => return,
        });
    }
}

const ACTIVATED: u32 = 2;

struct S {
    ls: Lockstep,
    t: u64,
    e: Vec<ElementId>,
}

impl S {
    fn new() -> (S, usize, usize) {
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
        let qh = ls.qh.clone();
        let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
        let w2 = ls.probe.toplevel(&qh, "W2", (400, 300), 0);
        ls.pump();
        let e: Vec<ElementId> = ls.state.elements().collect();
        let mut s = S { ls, t: 10_000, e };
        for (idx, name) in [(w1, "one"), (w2, "two")] {
            let t = s.ls.probe.surfaces[idx].toplevel.clone().unwrap();
            t.set_title(name.into());
            t.set_app_id(format!("org.example.{name}"));
        }
        s.pump();
        (s, w1, w2)
    }

    fn pump(&mut self) {
        self.ls.pump();
        let t = self.ls.now + Duration::from_micros(1000);
        self.ls.run_until(t);
        self.ls.pump();
    }

    fn bind(&mut self) {
        let (name, _, _) = self
            .ls
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwlr_foreign_toplevel_manager_v1")
            .cloned()
            .expect("the manager is offered");
        let _manager: ZwlrForeignToplevelManagerV1 =
            self.ls.registry().bind(name, 3, &self.ls.qh, ());
        self.pump();
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.ls.probe.log)
    }

    fn focus(&mut self, target: Option<ElementId>) {
        set_focus(&mut self.ls.state, &mut self.ls.input, target).expect("set_focus");
        self.pump();
    }
}

#[test]
fn a_task_bar_sees_the_windows_and_their_changes() {
    let (mut s, w1, _) = S::new();
    s.focus(Some(s.e[0]));
    s.take_log();
    s.bind();
    let log = s.take_log();
    assert_eq!(
        log.iter().filter(|l| *l == "toplevel").count(),
        2,
        "{log:?}"
    );
    for l in [
        "title one",
        "app_id org.example.one",
        "title two",
        "app_id org.example.two",
    ] {
        assert!(log.contains(&l.to_string()), "{l}: {log:?}");
    }
    assert!(
        log.contains(&format!("state {:?}", [ACTIVATED])),
        "the focused one is activated: {log:?}"
    );

    let t1 = s.ls.probe.surfaces[w1].toplevel.clone().unwrap();
    t1.set_title("renamed".into());
    s.pump();
    assert_eq!(s.take_log(), ["title renamed", "done"], "a title change");

    s.focus(Some(s.e[1]));
    // one handle loses `activated`, the other gains it, each followed by its `done`
    let mut log = s.take_log();
    log.sort();
    let activated = format!("state {:?}", [ACTIVATED]);
    assert_eq!(
        log,
        ["done", "done", activated.as_str(), "state []"],
        "the focus moved"
    );

    // close: the window gets xdg_toplevel.close (the client decides); destroyed, the handle closes
    s.ls.probe.wlr_handles[0].close();
    s.pump();
    assert!(
        s.ls.probe.close_requests.contains(&w1),
        "close reached the window"
    );
    t1.destroy();
    s.pump();
    assert!(
        s.take_log().contains(&"closed".to_string()),
        "the handle closed"
    );
}

#[test]
fn activating_is_the_users_only_from_a_bar_they_are_using() {
    let (mut s, _, _) = S::new();
    s.bind();
    // nothing focused and the pointer over the empty canvas: the client holds neither focus
    s.focus(None);
    s.t += 1000;
    pointer_motion(
        &mut s.ls.state,
        &mut s.ls.input,
        (900.0, 500.0),
        (900.0, 500.0),
        s.t,
    )
    .expect("motion");
    s.pump();
    assert_eq!(s.ls.state.core().focus().target(), None);
    s.ls.probe.wlr_handles[1].activate(s.ls.probe.seat.as_ref().unwrap());
    s.pump();
    assert_eq!(
        s.ls.state.core().focus().target(),
        None,
        "a client the user is not using does not move the focus"
    );

    // holding the keyboard focus (on its window), it is the user's
    s.focus(Some(s.e[0]));
    s.ls.probe.wlr_handles[1].activate(s.ls.probe.seat.as_ref().unwrap());
    s.pump();
    assert_eq!(
        s.ls.state.core().focus().target(),
        Some(s.e[1]),
        "activated by the bar the user is using"
    );
}
