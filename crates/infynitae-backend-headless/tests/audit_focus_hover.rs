//! Wire-level integration reproducers for FOCUS_FOLLOWS_MOUSE and game hover audit
//! (`docs/research/AUDIT_FOCUS_HOVER.md`).
//!
//! Marked `#[ignore = "AUDIT_FOCUS_HOVER: ..."]` so the suite stays green; run with:
//! `nice -n 19 cargo test -p infynitae-backend-headless --test audit_focus_hover -- --include-ignored --nocapture`

mod common;

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use common::input::*;
use common::{Lockstep, Probe};
use infynitae_backend_headless::{keyboard_key, pointer_button, pointer_motion, set_focus};
use infynitae_core::input::keys::KEY_LEFTMETA;
use infynitae_core::stage::ElementId;
use infynitae_wayland::ClientState;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface as ServerSurface;
use smithay::wayland::pointer_constraints::with_pointer_constraint;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, EventQueue, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};

struct Other {
    conn: Connection,
    queue: EventQueue<Probe>,
    probe: Probe,
    qh: QueueHandle<Probe>,
    registry: WlRegistry,
}

impl Other {
    fn connect(ls: &mut Lockstep) -> Other {
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        client_end.set_nonblocking(true).expect("non-blocking");
        ls.display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert second client");
        let conn = Connection::from_socket(client_end).expect("connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        Other {
            conn,
            queue,
            probe: Probe::default(),
            qh,
            registry,
        }
    }

    fn drive(&mut self, ls: &mut Lockstep) {
        let mut idle = 0;
        for _ in 0..10_000 {
            ls.pump();
            self.conn.flush().expect("flush");
            if let Some(guard) = self.conn.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("second client read: {e}"),
                }
            }
            let handled = self
                .queue
                .dispatch_pending(&mut self.probe)
                .expect("second client dispatch");
            self.conn.flush().expect("flush");
            let served = ls
                .display
                .dispatch_clients(&mut ls.state)
                .expect("server dispatch");
            ls.display.flush_clients().expect("server flush");
            if handled == 0 && served == 0 {
                idle += 1;
                if idle >= 3 {
                    return;
                }
            } else {
                idle = 0;
            }
        }
        panic!("exchange did not settle");
    }
}

fn adv(ls: &mut Lockstep) {
    ls.pump();
    let t = ls.now + Duration::from_micros(16_667);
    ls.run_until(t);
}

fn helper_server_surface(ls: &Lockstep, elem_id: ElementId) -> ServerSurface {
    ls.state
        .window(elem_id)
        .expect("window")
        .toplevel()
        .expect("toplevel")
        .wl_surface()
        .clone()
}

fn helper_smithay_active(ls: &Lockstep, elem_id: ElementId) -> bool {
    let root = helper_server_surface(ls, elem_id);
    let ptr = ls.state.seat().get_pointer().expect("seat pointer");
    with_pointer_constraint(&root, &ptr, |c| c.map(|c| c.is_active()).unwrap_or(false))
}

#[test]
fn held_super_releases_pointer_lock_in_windowed_game() {
    let mut ls = Lockstep::new(Some(a_1080()));
    // the config's defaults (FREE_CURSOR_ON_SUPER, FOCUS_ON_OPEN), read at login
    ls.state.core_mut().set_free_cursor_on_super(true);
    ls.state.core_mut().set_focus_on_open(true);
    let qh = ls.qh.clone();
    let g_idx = ls.probe.toplevel(&qh, "GAME", (800, 600), 0);
    adv(&mut ls);
    ls.probe.draw(&qh, g_idx);
    adv(&mut ls);
    let game_id = ls.state.elements().last().expect("game element");
    set_focus(&mut ls.state, &mut ls.input, Some(game_id)).unwrap();
    adv(&mut ls);

    // Pointer on GAME
    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (100.0, 100.0),
        (100.0, 100.0),
        1000,
    )
    .unwrap();
    adv(&mut ls);

    // Request persistent pointer lock
    ls.probe.lock_pointer(&qh, g_idx, false);
    adv(&mut ls);

    assert!(
        helper_smithay_active(&ls, game_id),
        "precondition: pointer lock is active on GAME"
    );

    // Hold Super (KEY_LEFTMETA)
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 2000).unwrap();
    adv(&mut ls);

    assert!(
        !helper_smithay_active(&ls, game_id),
        "held Super must release pointer lock in windowed game"
    );

    // Release Super
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 3000).unwrap();
    adv(&mut ls);

    assert!(
        helper_smithay_active(&ls, game_id),
        "releasing Super re-locks in windowed game"
    );

    // Fullscreen control: in fullscreen, Super must NOT release the pointer lock
    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    adv(&mut ls);
    ls.probe.surfaces[g_idx].size = (1920, 1080);
    ls.probe.draw(&qh, g_idx);
    adv(&mut ls);
    let out = ls.output();
    assert_eq!(ls.state.core().presentation(out), Some(game_id));
    assert!(helper_smithay_active(&ls, game_id));

    // Hold Super in fullscreen
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, true, 4000).unwrap();
    adv(&mut ls);

    assert!(
        helper_smithay_active(&ls, game_id),
        "held Super must NOT release pointer lock in fullscreen game"
    );
    keyboard_key(&mut ls.state, &mut ls.input, KEY_LEFTMETA, false, 5000).unwrap();
    adv(&mut ls);
}

#[test]
fn captcha_window_from_game_gets_focus_and_frees_pointer_then_restores_lock_on_close() {
    let mut ls = Lockstep::new(Some(a_1080()));
    // the config's defaults (FREE_CURSOR_ON_SUPER, FOCUS_ON_OPEN), read at login
    ls.state.core_mut().set_free_cursor_on_super(true);
    ls.state.core_mut().set_focus_on_open(true);
    let qh = ls.qh.clone();
    let g_idx = ls.probe.toplevel(&qh, "GAME", (800, 600), 0);
    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("space.bigrat.mocktail".into());
    adv(&mut ls);
    ls.probe.draw(&qh, g_idx);
    adv(&mut ls);
    let game_id = ls.state.elements().last().expect("game element");
    set_focus(&mut ls.state, &mut ls.input, Some(game_id)).unwrap();
    adv(&mut ls);

    // Pointer on GAME
    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (100.0, 100.0),
        (100.0, 100.0),
        1000,
    )
    .unwrap();
    adv(&mut ls);

    // Request persistent pointer lock on GAME
    ls.probe.lock_pointer(&qh, g_idx, false);
    adv(&mut ls);
    assert!(
        helper_smithay_active(&ls, game_id),
        "precondition: pointer lock active on GAME"
    );
    assert_eq!(ls.probe.kb.active.as_deref(), Some("GAME"));

    // Game process opens captcha window (second toplevel from same client connection)
    let c_idx = ls.probe.toplevel(&qh, "CAPTCHA", (400, 300), 0);
    ls.probe.surfaces[c_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("org.mocktail.WebViewHelper".into());
    adv(&mut ls);
    ls.probe.draw(&qh, c_idx);
    adv(&mut ls);
    let captcha_id = ls
        .state
        .elements()
        .into_iter()
        .find(|&id| id != game_id)
        .expect("captcha element");

    // Compositor policy: captcha window is from same client as game, so it is not held
    // and is shown over the game via show_opened
    assert!(ls.state.is_same_client(captcha_id, game_id));
    assert!(
        !ls.state.open_focus_held(captcha_id, false),
        "captcha window must not be held by focus priority"
    );
    let fx = ls
        .state
        .core_mut()
        .show_opened(&mut ls.input, captcha_id)
        .unwrap();
    ls.state.apply(&fx);
    adv(&mut ls);

    // Captcha window must get keyboard focus
    assert_eq!(
        ls.probe.kb.active.as_deref(),
        Some("CAPTCHA"),
        "captcha window must receive keyboard focus on open"
    );

    // Pointer lock on GAME must be deactivated (free pointer)
    assert!(
        !helper_smithay_active(&ls, game_id),
        "pointer lock must be deactivated while captcha window is open"
    );
    assert!(
        ls.probe.ptr.iter().any(|s| s == "unlocked"),
        "client must receive unlocked event for game surface when captcha opens"
    );

    // Close captcha window
    ls.probe.destroy_toplevel(c_idx);
    adv(&mut ls);

    // Focus must return to GAME
    assert_eq!(
        ls.probe.kb.active.as_deref(),
        Some("GAME"),
        "keyboard focus must return to GAME after captcha closes"
    );

    // Pointer lock must come back to GAME
    assert!(
        helper_smithay_active(&ls, game_id),
        "pointer lock must reactivate on GAME after captcha closes"
    );
}

#[test]
fn captcha_window_from_fullscreen_game_gets_focus_and_frees_pointer_then_restores_lock_on_close() {
    let mut ls = Lockstep::new(Some(a_1080()));
    // the config's defaults (FREE_CURSOR_ON_SUPER, FOCUS_ON_OPEN), read at login
    ls.state.core_mut().set_free_cursor_on_super(true);
    ls.state.core_mut().set_focus_on_open(true);
    let qh = ls.qh.clone();
    let g_idx = ls.probe.toplevel(&qh, "GAME", (1920, 1080), 0);
    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("space.bigrat.mocktail".into());
    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    adv(&mut ls);
    ls.probe.draw(&qh, g_idx);
    adv(&mut ls);
    let game_id = ls.state.elements().last().expect("game element");
    set_focus(&mut ls.state, &mut ls.input, Some(game_id)).unwrap();
    adv(&mut ls);
    assert!(
        ls.state.core().fullscreen_of(game_id).is_some(),
        "precondition: GAME is fullscreen"
    );

    // Pointer on GAME
    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (100.0, 100.0),
        (100.0, 100.0),
        1000,
    )
    .unwrap();
    adv(&mut ls);

    // Request persistent pointer lock on GAME
    ls.probe.lock_pointer(&qh, g_idx, false);
    adv(&mut ls);
    assert!(
        helper_smithay_active(&ls, game_id),
        "precondition: pointer lock active on fullscreen GAME"
    );
    assert_eq!(ls.probe.kb.active.as_deref(), Some("GAME"));

    // Game process opens captcha window (second toplevel from same client connection)
    let c_idx = ls.probe.toplevel(&qh, "CAPTCHA", (400, 300), 0);
    ls.probe.surfaces[c_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("org.mocktail.WebViewHelper".into());
    adv(&mut ls);
    ls.probe.draw(&qh, c_idx);
    adv(&mut ls);
    let captcha_id = ls
        .state
        .elements()
        .into_iter()
        .find(|&id| id != game_id)
        .expect("captcha element");

    // Compositor policy: captcha window is from same client as fullscreen game,
    // so it is NOT held by fullscreen priority and is shown over the game via show_opened
    assert!(ls.state.is_same_client(captcha_id, game_id));
    assert!(
        !ls.state.open_focus_held(captcha_id, false),
        "captcha window must not be held by fullscreen focus priority"
    );
    let fx = ls
        .state
        .core_mut()
        .show_opened(&mut ls.input, captcha_id)
        .unwrap();
    ls.state.apply(&fx);
    adv(&mut ls);

    // Captcha window must get keyboard focus
    assert_eq!(
        ls.probe.kb.active.as_deref(),
        Some("CAPTCHA"),
        "captcha window must receive keyboard focus over fullscreen game"
    );

    // Pointer lock on GAME must be deactivated (free pointer)
    assert!(
        !helper_smithay_active(&ls, game_id),
        "pointer lock must be deactivated while captcha window is open"
    );
    assert!(
        ls.probe.ptr.iter().any(|s| s == "unlocked"),
        "client must receive unlocked event for game surface when captcha opens"
    );

    // Close captcha window
    ls.probe.destroy_toplevel(c_idx);
    adv(&mut ls);

    // Focus must return to GAME
    assert_eq!(
        ls.probe.kb.active.as_deref(),
        Some("GAME"),
        "keyboard focus must return to fullscreen GAME after captcha closes"
    );

    // Pointer lock must come back to GAME
    assert!(
        helper_smithay_active(&ls, game_id),
        "pointer lock must reactivate on fullscreen GAME after captcha closes"
    );
}

#[test]
fn roblox_stepped_aside_flips_keyboard_focus_between_game_and_other_while_button_held() {
    let mut ls = Lockstep::new(Some(a_1080()));
    ls.state.core_mut().set_focus_follows_mouse(true);
    ls.state.core_mut().set_focus_follows_mouse_strict(true);

    // Client 1: fullscreen game ("space.bigrat.mocktail")
    let qh_game = ls.qh.clone();
    let g_idx = ls.probe.toplevel(&qh_game, "GAME", (800, 600), 0);
    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_app_id("space.bigrat.mocktail".into());
    adv(&mut ls);
    ls.probe.draw(&qh_game, g_idx);
    adv(&mut ls);
    let game_id = ls.state.elements().last().expect("game element");
    let game_el = infynitae_core::stage::ElementRef {
        instance: ls.state.core().stage().instance(),
        element: game_id,
    };
    let fx_place_game = ls
        .state
        .core_mut()
        .shell_command(
            &mut ls.shell,
            infynitae_core::ShellCommand::MoveElement {
                element: game_el,
                to: (-300.0, -300.0),
                stream: infynitae_core::ShellStream::Atomic,
            },
        )
        .unwrap();
    ls.state.apply(&fx_place_game);
    set_focus(&mut ls.state, &mut ls.input, Some(game_id)).unwrap();
    adv(&mut ls);

    ls.probe.surfaces[g_idx]
        .toplevel
        .as_ref()
        .unwrap()
        .set_fullscreen(None);
    adv(&mut ls);
    ls.probe.surfaces[g_idx].size = (1920, 1080);
    ls.probe.draw(&qh_game, g_idx);
    adv(&mut ls);
    let out = ls.output();
    assert_eq!(ls.state.core().presentation(out), Some(game_id));

    // Client 2: sibling window ("other") brought over the stepped-aside game via overview/show_opened
    let mut other = Other::connect(&mut ls);
    other.drive(&mut ls);
    other.probe.bind_basic(&other.registry, &other.qh);
    let ptr_o = other
        .probe
        .seat
        .as_ref()
        .unwrap()
        .get_pointer(&other.qh, ());
    other.probe.pointer = Some(ptr_o);
    other.drive(&mut ls);
    let o_idx = other.probe.toplevel(&other.qh, "other", (500, 400), 0);
    other.drive(&mut ls);
    other.probe.draw(&other.qh, o_idx);
    other.drive(&mut ls);
    let other_id = ls
        .state
        .elements()
        .into_iter()
        .find(|&id| id != game_id)
        .expect("other element");

    // Step aside fullscreen game and bring "other" up on its left (-800..-300) via show_opened + MoveElement
    let element = infynitae_core::stage::ElementRef {
        instance: ls.state.core().stage().instance(),
        element: other_id,
    };
    let fx = ls
        .state
        .core_mut()
        .shell_command(
            &mut ls.shell,
            infynitae_core::ShellCommand::MoveElement {
                element,
                to: (-800.0, -200.0),
                stream: infynitae_core::ShellStream::Atomic,
            },
        )
        .unwrap();
    ls.state.apply(&fx);
    let fx2 = ls
        .state
        .core_mut()
        .show_opened(&mut ls.input, other_id)
        .unwrap();
    ls.state.apply(&fx2);
    other.drive(&mut ls);
    assert!(ls.state.core().is_presentation_suppressed(out));

    // Focus GAME while keeping it stepped aside (GAME occupies [-300, 1620], "other" occupies [-800, -300])
    set_focus(&mut ls.state, &mut ls.input, Some(game_id)).unwrap();
    other.drive(&mut ls);
    assert!(ls.state.core().is_presentation_suppressed(out));
    // Move pointer onto GAME's exposed region (-100, 0) in canvas space
    pointer_motion(
        &mut ls.state,
        &mut ls.input,
        (-100.0, 0.0),
        (-100.0, 0.0),
        1000,
    )
    .unwrap();
    other.drive(&mut ls);
    assert_eq!(ls.probe.kb.active.as_deref(), Some("GAME"));

    // Hold BTN_RIGHT (e.g., third-person camera orbit / macro drag without Wayland pointer lock)
    pointer_button(&mut ls.state, &mut ls.input, BTN_RIGHT, true, none(), 2000).unwrap();
    other.drive(&mut ls);

    ls.probe.kb_log.clear();
    other.probe.kb_log.clear();

    // Sweep left onto "other" (-500, 0) and back onto GAME (-100, 0) twice while holding BTN_RIGHT
    for i in 0..2u64 {
        let t0 = 3000 + i * 2000;
        pointer_motion(
            &mut ls.state,
            &mut ls.input,
            (-400.0, 0.0),
            (-400.0, 0.0),
            t0,
        )
        .unwrap();
        other.drive(&mut ls);
        pointer_motion(
            &mut ls.state,
            &mut ls.input,
            (400.0, 0.0),
            (400.0, 0.0),
            t0 + 1000,
        )
        .unwrap();
        other.drive(&mut ls);
    }

    assert!(
        !ls.probe
            .kb_log
            .iter()
            .any(|l| l.starts_with("wl_keyboard.leave")),
        "GAME holding BTN_RIGHT flipped keyboard focus on crossing into other window: game_kb={:?}, other_kb={:?}",
        ls.probe.kb_log,
        other.probe.kb_log
    );
}

#[test]
fn top_layer_on_demand_panel_gives_the_keyboard_back_on_reentry_wire() {
    let mut ls = Lockstep::new(Some(a_1080()));
    ls.bind_layer_shell();
    let mut r = Rig::new(ls);
    r.ls.state.core_mut().set_focus_follows_mouse(true);

    // Map W centered, then move near top strip via Super+Left or place at top-center
    r.map_w();
    let w_id = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    // Place W spanning Canvas y in [-520..-120] => screen y in [20..420], x in [-200..200] => screen [760..1160]
    let element = infynitae_core::stage::ElementRef {
        instance: r.ls.state.core().stage().instance(),
        element: w_id,
    };
    let fx =
        r.ls.state
            .core_mut()
            .shell_command(
                &mut r.ls.shell,
                infynitae_core::ShellCommand::MoveElement {
                    element,
                    to: (-200.0, -520.0),
                    stream: infynitae_core::ShellStream::Atomic,
                },
            )
            .unwrap();
    r.ls.state.apply(&fx);
    r.ls.pump();
    r.frame();

    // Map an OnDemand Top strip across the top 80 px of the output (exclusive_zone = 0)
    let p_idx = r.ls.probe.layer_surface(
        &r.ls.qh.clone(),
        "panel",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (0, 80),
        0,
        KeyboardInteractivity::OnDemand,
    );
    r.ls.pump();
    r.ls.probe.frame_and_draw(&r.ls.qh.clone(), p_idx);
    r.ls.pump();
    r.frame();

    // Move pointer onto W below the panel: (960, 200) = delta (0, -340) from (960, 540)
    r.motion((0.0, -340.0));
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("W"));

    // Move up onto panel: (960, 40) = delta (0, -160) and click it -> OnDemand layer takes keyboard focus
    r.motion((0.0, -160.0));
    r.button(BTN_LEFT, true, none());
    r.button(BTN_LEFT, false, none());
    assert_eq!(r.ls.probe.kb.active.as_deref(), Some("panel"));

    // Move back down onto W: (960, 200) = delta (0, +160)
    r.motion((0.0, 160.0));
    assert_eq!(
        r.ls.probe.kb.active.as_deref(),
        Some("W"),
        "returning pointer from OnDemand panel onto W should restore wl_keyboard focus to W, not keep it trapped on panel (kb_log={:?})",
        r.ls.probe.kb_log
    );
}
