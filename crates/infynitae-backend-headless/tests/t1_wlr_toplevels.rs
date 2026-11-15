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
            Event::OutputEnter { output } => {
                if let Some(i) = st.outputs.iter().position(|(_, o)| o == &output) {
                    st.kb_log.push(format!("wlr_output_enter {i}"));
                }
                return;
            }
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

#[test]
fn pointer_motion_sends_no_toplevel_events_when_nothing_changed() {
    let (mut s, w1, _) = S::new();
    s.focus(Some(s.e[0]));
    s.bind();
    let _ = s.take_log();
    let sync_before = s.ls.state.wlr_toplevel_sync_count();

    for i in 0..20 {
        s.t += 1000;
        let p = 500.0 + (i as f64);
        pointer_motion(&mut s.ls.state, &mut s.ls.input, (p, p), (p, p), s.t).expect("motion");
        s.pump();
    }
    assert_eq!(
        s.take_log(),
        Vec::<String>::new(),
        "pointer motion emits no toplevel events when nothing changed"
    );
    assert_eq!(
        s.ls.state.wlr_toplevel_sync_count(),
        sync_before,
        "sync_wlr_toplevels does nothing when nothing changed"
    );

    let t1 = s.ls.probe.surfaces[w1].toplevel.clone().unwrap();
    t1.set_title("renamed".into());
    s.pump();
    assert_eq!(s.take_log(), ["title renamed", "done"], "a title change");
    assert_eq!(
        s.ls.state.wlr_toplevel_sync_count(),
        sync_before + 1,
        "title change triggers a sync"
    );
}

/// A window that fills a screen and counts as fullscreen there (`FIT_IS_FULLSCREEN`) is told to task bars as
/// fullscreen, and no longer once it does not fill it: the edge takes its closed panels off that screen then,
/// so nothing is drawn over the window (review 2026-11-11: a closed panel kept a maximized game from going to
/// the screen directly).
#[test]
fn a_window_that_fills_the_screen_is_told_as_fullscreen() {
    const FULLSCREEN: u32 = 3;
    let (mut s, _, _) = S::new();
    s.bind();
    s.take_log();
    let out = s.ls.output();
    let w = s.e[0];
    s.ls.state.set_fitted_window(out, Some(w));
    s.focus(Some(w));
    let log = s.take_log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("state") && l.contains(&FULLSCREEN.to_string())),
        "{log:?}"
    );
    s.ls.state.set_fitted_window(out, None);
    s.focus(Some(s.e[1]));
    let log = s.take_log();
    let states: Vec<&String> = log.iter().filter(|l| l.starts_with("state")).collect();
    assert!(
        !states.is_empty() && states.iter().all(|l| !l.contains(&FULLSCREEN.to_string())),
        "{log:?}"
    );
}

#[test]
fn picking_a_window_from_the_bar_moves_the_camera_and_records_history() {
    use infynitae_core::geom::{CanvasPos, Q16_ONE};
    use infynitae_core::{NavigateTo, ShellCommand, ShellStream};
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
        Anchor, KeyboardInteractivity,
    };

    let (mut s, _, _) = S::new();
    s.ls.bind_layer_shell();
    s.bind();

    let o = s.ls.output();
    let w1 = s.e[1];
    let a1 = s.ls.state.core().stage().anchor(w1).expect("w1 anchor");
    let size1 =
        s.ls.state
            .core()
            .stage()
            .tree(w1)
            .expect("w1 tree")
            .geometry
            .size;
    let expected_w1_center = CanvasPos {
        x: a1.x * Q16_ONE + i64::from(size1.w) * (Q16_ONE / 2),
        y: a1.y * Q16_ONE + i64::from(size1.h) * (Q16_ONE / 2),
    };

    // Navigate elsewhere first (incrementally so history starts empty), then make sure an
    // unvalidated client moves neither focus nor camera.
    let fx =
        s.ls.state
            .core_mut()
            .shell_command(
                &mut s.ls.shell,
                ShellCommand::Navigate {
                    output: o,
                    to: NavigateTo::Delta(-2400.0, 1200.0),
                    zoom: None,
                    stream: ShellStream::Atomic,
                },
            )
            .unwrap();
    s.ls.state.apply(&fx);
    let away = s.ls.state.core().cameras().center(0).unwrap();
    assert_ne!(away, expected_w1_center);
    assert_eq!(
        s.ls.state.core().cameras().history(0).map(|h| h.len()),
        Some(0)
    );

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
    s.focus(None);
    s.ls.probe.wlr_handles[1].activate(s.ls.probe.seat.as_ref().unwrap());
    s.pump();
    assert_eq!(s.ls.state.core().cameras().center(0), Some(away));
    assert_eq!(s.ls.state.core().focus().target(), None);
    assert_eq!(
        s.ls.state.core().cameras().history(0).map(|h| h.len()),
        Some(0)
    );

    // A layer-shell panel with exclusive keyboard focus (the Windows card) activates the picked
    // window and closes in the same flush: focus, camera jump, and history entry all land.
    let panel = s.ls.probe.layer_surface(
        &s.ls.qh,
        "windows-card",
        Layer::Overlay,
        Anchor::Right,
        (360, 480),
        0,
        KeyboardInteractivity::Exclusive,
    );
    s.pump();
    s.ls.probe.frame_and_draw(&s.ls.qh, panel);
    s.pump();
    assert_eq!(s.ls.probe.kb.active.as_deref(), Some("windows-card"));

    s.ls.probe.wlr_handles[1].activate(s.ls.probe.seat.as_ref().unwrap());
    s.ls.probe.destroy_layer(panel);
    s.pump();

    assert_eq!(s.ls.state.core().focus().target(), Some(w1));
    assert_eq!(
        s.ls.state.core().cameras().center(0),
        Some(expected_w1_center),
        "picking a window moves the camera to its centre"
    );
    assert_eq!(
        s.ls.state.core().cameras().history(0).map(|h| h.len()),
        Some(1),
        "picking a window pushes the camera history so Back returns"
    );

    s.ls.state
        .core_mut()
        .shell_command(&mut s.ls.shell, ShellCommand::ViewportRestore { output: o })
        .unwrap();
    assert_eq!(s.ls.state.core().cameras().center(0), Some(away));
    assert_eq!(
        s.ls.state.core().cameras().history(0).map(|h| h.len()),
        Some(0)
    );
}

#[test]
#[ignore = "AUDIT_LAYERS: wlr_toplevels misses dirty sync when client commits acked fullscreen/unfullscreen"]
fn audit_layers_commit_fullscreen_or_unfullscreen_does_not_sync_wlr_toplevels() {
    const FULLSCREEN: u32 = 3;
    let (mut s, w1, _) = S::new();
    s.focus(Some(s.e[0]));
    s.bind();
    s.take_log();

    // W1 enters fullscreen and commits its acked 1920x1080 frame: handlers.rs:251 sets
    // wlr_toplevels.dirty = true AFTER with_effects() without calling sync_wlr_toplevels().
    s.ls.probe.surfaces[w1]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    s.pump();
    s.ls.probe.surfaces[w1].size = (1920, 1080);
    s.ls.probe.draw(&s.ls.qh, w1);
    s.pump();
    let log = s.take_log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("state") && l.contains(&FULLSCREEN.to_string())),
        "committing acked fullscreen must immediately notify wlr_toplevels (saw {log:?})"
    );
}

#[test]
#[ignore = "AUDIT_LAYERS: wlr_toplevels reports Fullscreen while presentation is suppressed"]
fn audit_layers_wlr_toplevel_reports_fullscreen_while_presentation_suppressed() {
    use infynitae_backend_headless::keyboard_key;
    use infynitae_core::input::keys::{KEY_LEFTMETA, KEY_TAB};

    const FULLSCREEN: u32 = 3;
    let (mut s, w1, _) = S::new();
    s.ls.state.core_mut().set_fullscreen_pan(true);
    s.focus(Some(s.e[0]));
    s.bind();
    s.take_log();

    s.ls.probe.surfaces[w1]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    s.pump();
    s.ls.probe.surfaces[w1].size = (1920, 1080);
    s.ls.probe.draw(&s.ls.qh, w1);
    s.pump();
    // Flush dirty flag left unsynced by handlers.rs:251 via metadata change so precondition holds.
    s.ls.probe.surfaces[w1]
        .toplevel
        .as_ref()
        .unwrap()
        .set_title("flush".into());
    s.pump();
    let o = s.ls.output();
    assert_eq!(s.ls.state.core().presentation(o), Some(s.e[0]));
    let log = s.take_log();
    assert!(
        log.iter()
            .any(|l| l.starts_with("state") && l.contains(&FULLSCREEN.to_string())),
        "precondition fullscreen entered: {log:?}"
    );

    // Super+Tab switches to W2 and suppresses W1's presentation: desktop & Layer::Top are shown again.
    for (key, pressed) in [
        (KEY_LEFTMETA, true),
        (KEY_TAB, true),
        (KEY_TAB, false),
        (KEY_LEFTMETA, false),
    ] {
        s.t += 1000;
        keyboard_key(&mut s.ls.state, &mut s.ls.input, key, pressed, s.t).expect("key");
    }
    s.pump();
    assert!(s.ls.state.core().is_presentation_suppressed(o));

    let log = s.take_log();
    let w1_states: Vec<&String> = log.iter().filter(|l| l.starts_with("state")).collect();
    assert!(
        !w1_states.is_empty()
            && !w1_states
                .iter()
                .any(|l| *l == &format!("state [{FULLSCREEN}]")),
        "suppressed presentation must clear Fullscreen on wlr_toplevels (saw {log:?})"
    );
}

#[test]
#[ignore = "AUDIT_LAYERS: clearing fitted window in render leaves wlr_toplevels unsynced"]
fn audit_layers_fitted_window_clear_in_render_leaves_wlr_toplevels_unsynced() {
    use smithay::backend::renderer::pixman::PixmanRenderer;
    use std::os::fd::AsFd;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wayland_client::protocol::wl_shm;

    static SEQ: AtomicUsize = AtomicUsize::new(0);
    const FULLSCREEN: u32 = 3;

    let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
    let qh = ls.qh.clone();
    let (name, _, _) = ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "zwlr_foreign_toplevel_manager_v1")
        .cloned()
        .expect("manager");
    let _mgr: ZwlrForeignToplevelManagerV1 = ls.registry().bind(name, 3, &qh, ());

    let idx = ls.probe.toplevel(&qh, "F", (1920, 1080), 0);
    ls.pump();
    let make_xrgb_buf = |probe: &Probe, qh: &wayland_client::QueueHandle<Probe>, w: i32, h: i32| {
        let len = (w * h * 4) as usize;
        let path = std::env::temp_dir().join(format!(
            "infynitae-audit-layers-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let _ = std::fs::remove_file(&path);
        file.set_len(len as u64).unwrap();
        let pool = probe
            .shm
            .as_ref()
            .unwrap()
            .create_pool(file.as_fd(), len as i32, qh, ());
        let buf = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Xrgb8888, qh, ());
        pool.destroy();
        buf
    };

    // Draw an opaque 1920x1080 frame filling the view, render it, then sync focus so handle sees Fullscreen.
    let buf = make_xrgb_buf(&ls.probe, &qh, 1920, 1080);
    let wl = ls.probe.surfaces[idx].wl.clone();
    wl.attach(Some(&buf), 0, 0);
    wl.damage_buffer(0, 0, 1920, 1080);
    wl.commit();
    ls.pump();
    let out = ls.output();
    let id = ls.state.elements().next().unwrap();
    let instance = ls.state.core().stage().instance();
    let fx = ls
        .state
        .core_mut()
        .shell_command(
            &mut ls.shell,
            infynitae_core::ShellCommand::ViewportFit {
                output: out,
                element: infynitae_core::stage::ElementRef {
                    instance,
                    element: id,
                },
            },
        )
        .unwrap();
    ls.state.apply(&fx);
    let mut renderer = PixmanRenderer::new().unwrap();
    let _ = infynitae_wayland::convert::output_elements_marked(
        &mut ls.state,
        &mut renderer,
        out,
        false,
        None,
    )
    .unwrap();
    assert_eq!(ls.state.fitted_window(out), Some(id));
    set_focus(&mut ls.state, &mut ls.input, Some(id)).unwrap();
    ls.pump();
    let log = std::mem::take(&mut ls.probe.log);
    assert!(
        log.iter()
            .any(|l| l.starts_with("state") && l.contains(&FULLSCREEN.to_string())),
        "precondition: fitted window reported Fullscreen: {log:?}"
    );

    // Client shrinks to 400x300 (apply() runs at commit before render still seeing old fitted=Some(id)).
    let small = make_xrgb_buf(&ls.probe, &qh, 400, 300);
    let wl = ls.probe.surfaces[idx].wl.clone();
    wl.attach(Some(&small), 0, 0);
    wl.damage_buffer(0, 0, 400, 300);
    wl.commit();
    ls.pump();
    let _ = std::mem::take(&mut ls.probe.log);

    // Next render clears fitted_window(out) -> None, followed by frame_tick + pump.
    let _ = infynitae_wayland::convert::output_elements_marked(
        &mut ls.state,
        &mut renderer,
        out,
        false,
        None,
    )
    .unwrap();
    assert_eq!(ls.state.fitted_window(out), None);
    ls.state.frame_tick(out, ls.now);
    ls.pump();

    let log = std::mem::take(&mut ls.probe.log);
    assert!(
        log.iter()
            .any(|l| l.starts_with("state") && !l.contains(&FULLSCREEN.to_string())),
        "clearing fitted window during render must sync cleared Fullscreen state to wlr_toplevels: {log:?}"
    );
}

#[test]
#[ignore = "AUDIT_LAYERS: wlr_toplevels announces OutputEnter for every output regardless of presentation"]
fn audit_layers_wlr_toplevel_announces_output_enter_for_non_presenting_output() {
    let mut ls = Lockstep::with(
        Default::default(),
        &[
            common::output_k("A", (1920, 1080), 120, 60_000),
            common::output_k("B", (1920, 1080), 120, 60_000),
        ],
    );
    let qh = ls.qh.clone();
    let w1 = ls.probe.toplevel(&qh, "W1", (400, 300), 0);
    ls.pump();
    let e0 = ls.state.elements().next().unwrap();
    set_focus(&mut ls.state, &mut ls.input, Some(e0)).unwrap();
    let wl_a = ls.probe.outputs[0].1.clone();
    ls.probe.surfaces[w1]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(Some(&wl_a));
    ls.pump();

    ls.probe.kb_log.clear();
    let (name, _, _) = ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == "zwlr_foreign_toplevel_manager_v1")
        .cloned()
        .expect("manager");
    let _mgr: ZwlrForeignToplevelManagerV1 = ls.registry().bind(name, 3, &qh, ());
    ls.pump();

    assert_eq!(
        ls.probe.kb_log,
        vec!["wlr_output_enter 0".to_string()],
        "window presenting exclusively on output 0 must not report OutputEnter on output 1"
    );
}
