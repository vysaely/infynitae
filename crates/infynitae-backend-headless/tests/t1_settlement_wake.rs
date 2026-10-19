//! INV-SCALE-3 / INV-IDLE-1 on the session loop: a scale-settlement deadline of Core's wakes an
//! otherwise idle session by itself, the epoch settles on that wake-up, and a session with no
//! deadline has nothing to wake for.
//!
//! The loop turns here are the statements of `run_headless`'s loop body, in its order, around a
//! real `Session`, a real calloop event loop and a real Core. There is no Wayland client: the
//! element is mapped through Core's protocol entry points, so the emission to a client is not
//! part of this file (TST-24 in `t1_scale` covers it with a client and a hand-driven clock).
//!
//! What opens the epochs:
//! - the continuous one by real input, a logo+middle drag (the only interaction Core has today);
//! - the discrete one by `Core::start_scale_interaction`, a test entry. No production event
//!   opens a discrete epoch yet (discrete pan and zoom steps, shell streams and interactive
//!   resize belong to INC-10, INC-11 and INC-14), so that step is a stand-in, not production
//!   evidence.

use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use infynitae_backend_headless::{
    VirtualOutput, connect, keyboard_key, pointer_button, pointer_motion, vblank,
};
use infynitae_core::geom::{LocalPoint, LocalRect, LocalSize};
use infynitae_core::input::bindings::Modifiers;
use infynitae_core::input::interaction::Interaction;
use infynitae_core::input::keys::KEY_LEFTMETA;
use infynitae_core::navigation::PersistedCameras;
use infynitae_core::output::{OutputId, Transform};
use infynitae_core::protocol::Effect;
use infynitae_core::scale::InteractionKind;
use infynitae_core::snapshot::{
    InputRegion, SnapshotSurface, SurfaceKey, SurfaceRole, SurfaceTree,
};
use infynitae_core::stage::ElementId;
use infynitae_core::{Core, InputPort, OutputLifecycleToken};
use infynitae_wayland::WaylandState;
use infynitae_wayland::session::{Session, SessionOptions};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;

const BTN_MIDDLE: u32 = 0x112;
/// Upper bound of one sleep in tests that wait for a deadline: a broken wake-up fails an
/// assertion after this long instead of blocking forever.
const CAP: Duration = Duration::from_secs(2);

fn out(connector: &str, scale_120: u32) -> VirtualOutput {
    VirtualOutput {
        key: VirtualOutput::key(connector),
        mode: (1920, 1080),
        transform: Transform::Normal,
        scale_120,
        refresh_mhz: 60_000,
    }
}

fn tree(mapped: bool) -> SurfaceTree {
    let size = LocalSize { w: 800, h: 600 };
    SurfaceTree {
        geometry: LocalRect {
            loc: LocalPoint::default(),
            size,
        },
        surfaces: vec![SnapshotSurface {
            key: SurfaceKey::from_raw(NonZeroU64::new(1).unwrap()),
            role: SurfaceRole::Root,
            size: if mapped { size } else { LocalSize::default() },
            mapped,
            input: InputRegion::Infinite,
            frame_requested: false,
        }],
    }
}

struct Loop {
    session: Session,
    state: WaylandState,
    event_loop: EventLoop<'static, ()>,
    input: InputPort,
    token: OutputLifecycleToken,
    a: OutputId,
    b: OutputId,
    w: ElementId,
    t_us: u64,
    /// Every `PreferredScale` a settlement tick produced, with the session time of the tick.
    settled: Vec<(u64, u32)>,
}

impl Loop {
    /// Outputs A (scale 1) and B (scale 1.5) both showing W at the Canvas origin: h = 1.5.
    fn new(name: &str) -> Loop {
        let (core, input, _shell, mut token) = Core::new(PersistedCameras::default());
        let display: Display<WaylandState> = Display::new().unwrap();
        let mut state = WaylandState::new(&display.handle(), core);
        let a = connect(&mut state, &mut token, &out("A", 120)).unwrap();
        let b = connect(&mut state, &mut token, &out("B", 180)).unwrap();
        let fx = state
            .core_mut()
            .set_renderer_device_max(infynitae_core::scale::DEFAULT_D_MAX)
            .unwrap();
        state.apply(&fx);
        let w = state.core_mut().toplevel_created();
        for mapped in [false, true] {
            let fx = state
                .core_mut()
                .toplevel_committed(w, tree(mapped))
                .unwrap();
            state.apply(&fx);
        }
        let event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
        let session = Session::new(
            display,
            &event_loop.handle(),
            &SessionOptions {
                socket_name: Some(format!("infynitae-{name}-test-{}", std::process::id())),
                refresh_mhz: 1_000_000,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let mut l = Loop {
            session,
            state,
            event_loop,
            input,
            token,
            a,
            b,
            w,
            t_us: 0,
            settled: Vec::new(),
        };
        assert_eq!(l.h(), 1.5, "W is shown on the 1.5 output");
        l.idle();
        l
    }

    fn h(&self) -> f64 {
        self.state
            .core()
            .scale_state(self.w)
            .expect("scale state")
            .h
    }

    fn deadline(&self) -> Option<u64> {
        self.state
            .core()
            .next_settlement_deadline()
            .map(|d| d.millis())
    }

    /// The sleep the loop would take now, with Core's deadline handed to the session as the run
    /// loop does at the top of every turn.
    fn sleep(&mut self) -> Option<Duration> {
        self.session
            .wake_at(self.state.core().next_settlement_deadline());
        self.session.timeout()
    }

    /// One turn of the run loop: `run_headless`'s loop body, statement for statement (the
    /// initial-focus grant left out: there is no client surface to focus).
    fn turn(&mut self, cap: Option<Duration>) {
        self.session
            .wake_at(self.state.core().next_settlement_deadline());
        let sleep = match (self.session.timeout(), cap) {
            (Some(t), Some(c)) => Some(t.min(c)),
            (t, c) => t.or(c),
        };
        self.event_loop.dispatch(sleep, &mut ()).unwrap();
        self.session.dispatch(&mut self.state);

        let now = self.session.now();
        let now_ms = now.millis();
        if self
            .state
            .core()
            .next_settlement_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            let fx = self.state.core_mut().scale_tick(now);
            for e in &fx {
                if let Effect::PreferredScale { scale_120, .. } = e {
                    self.settled.push((now_ms, *scale_120));
                }
            }
            self.state.apply(&fx);
            self.session.flush();
        }

        if self.session.frame_due() {
            vblank(
                &mut self.state,
                self.a,
                infynitae_wayland::presentation_clock_now(),
            );
            self.session.count_frame();
            self.session.flush();
        }
    }

    /// Turns until the session has nothing to wake for.
    fn idle(&mut self) {
        for _ in 0..200 {
            if self.sleep().is_none() {
                return;
            }
            self.turn(Some(Duration::from_millis(5)));
        }
        panic!("the session never became idle");
    }

    fn us(&mut self) -> u64 {
        self.t_us += 1000;
        self.t_us
    }

    fn motion(&mut self, d: (f64, f64)) {
        let t = self.us();
        pointer_motion(&mut self.state, &mut self.input, d, d, t).unwrap();
    }

    /// Logo + middle press on output B, a drag that moves B's camera off W, then the release
    /// if asked for.
    fn pan_b_away(&mut self, release: bool) {
        let t = self.us();
        keyboard_key(&mut self.state, &mut self.input, KEY_LEFTMETA, true, t).unwrap();
        let t = self.us();
        pointer_button(
            &mut self.state,
            &mut self.input,
            BTN_MIDDLE,
            true,
            Modifiers::default(),
            t,
        )
        .unwrap();
        match self.state.core().interaction() {
            Interaction::CameraPan { output, .. } => assert_eq!(output, self.b, "pan on B"),
            other => panic!("the drag did not start a pan: {other:?}"),
        }
        self.motion((5000.0, 0.0));
        if release {
            self.release();
        }
    }

    fn release(&mut self) {
        let t = self.us();
        pointer_button(
            &mut self.state,
            &mut self.input,
            BTN_MIDDLE,
            false,
            Modifiers::default(),
            t,
        )
        .unwrap();
        let t = self.us();
        keyboard_key(&mut self.state, &mut self.input, KEY_LEFTMETA, false, t).unwrap();
    }

    fn open_discrete(&mut self) -> u64 {
        let now = self.session.now();
        self.state.core_mut().start_scale_interaction(
            self.w,
            InteractionKind::WindowResize,
            false,
            now,
        );
        now.millis()
    }
}

/// The pointer starts on A; one motion takes it onto B, where the drag pans B's camera. The
/// session is then let run for 40 ms, so that no test opens its epoch at session time 0: a
/// deadline computed from time 0 instead of the step's time is then a different number.
fn on_b(name: &str) -> Loop {
    let mut l = Loop::new(name);
    l.motion((1000.0, 0.0));
    while l.session.now_ms() < 40 {
        l.turn(Some(Duration::from_millis(10)));
    }
    assert_eq!(l.sleep(), None, "idle before the test starts");
    l
}

/// The whole chain on a real loop. An open discrete epoch gives Core a deadline; the session
/// sleeps exactly until it with no input, no client and no frame; the epoch settles on that
/// wake-up, once; and afterwards the session is idle again.
#[test]
fn a_settlement_deadline_wakes_the_idle_loop_and_settles_the_epoch() {
    let mut l = on_b("settle");
    assert_eq!(l.deadline(), None, "no epoch: no deadline");
    assert_eq!(l.sleep(), None, "no deadline: nothing to wake for");

    let opened = l.open_discrete();
    let t_settle = l.state.core().scale_config().t_settle_ms;
    assert_eq!(
        l.deadline(),
        Some(opened + t_settle),
        "Core set the deadline"
    );

    // the requirement drops to 1.0 inside the epoch: the decrease is held back
    l.pan_b_away(true);
    assert_eq!(l.h(), 1.5, "decrease suppressed while the epoch is open");
    assert_eq!(
        l.deadline(),
        Some(opened + t_settle),
        "the pan's release settles pan epochs only"
    );

    // the session would sleep until the deadline and not beyond
    let remaining = (opened + t_settle).saturating_sub(l.session.now_ms());
    let sleep = l.sleep().expect("an open epoch bounds the sleep");
    assert!(
        sleep <= Duration::from_millis(remaining),
        "sleeps past the deadline: {sleep:?} > {remaining} ms"
    );
    assert!(
        sleep + Duration::from_millis(20) >= Duration::from_millis(remaining),
        "wakes long before the deadline: {sleep:?} for {remaining} ms"
    );

    // one turn: nothing but the deadline can end this sleep (the cap only keeps a broken
    // wake-up from hanging the test; it is far beyond the deadline)
    let before = Instant::now();
    l.turn(Some(CAP));
    let slept = before.elapsed();
    let woke = l.session.now_ms();
    assert!(woke >= opened + t_settle, "woke before the deadline");
    assert!(
        slept + Duration::from_millis(20) >= sleep,
        "the loop returned early: {slept:?} of {sleep:?}"
    );
    let late = woke - (opened + t_settle);
    println!("settlement wake-up: {late} ms after the deadline ({t_settle} ms epoch)");
    assert!(late < 200, "woke {late} ms after the deadline");

    assert_eq!(l.settled.len(), 1, "settled on that wake-up, once");
    assert_eq!(l.settled[0].1, 120, "to the final requirement: 1.0");
    assert_eq!(l.h(), 1.0);
    assert_eq!(l.deadline(), None, "the epoch is closed");
    assert_eq!(l.sleep(), None, "idle again: nothing to wake for");
}

/// A further step moves the deadline and the wake-up with it; an output settlement event closes
/// the epoch and cancels the wake-up without waiting for it.
#[test]
fn the_wake_up_follows_the_deadline_and_is_cancelled_with_the_epoch() {
    let mut l = on_b("move");
    let first = l.open_discrete();
    let t_settle = l.state.core().scale_config().t_settle_ms;

    // a further step 60 ms later: the deadline moves, the armed sleep follows it
    while l.session.now_ms() < first + 60 {
        l.turn(Some(Duration::from_millis(10)));
    }
    assert_eq!(l.settled.len(), 0, "not settled before its deadline");
    let second = l.open_discrete();
    assert!(second >= first + 60);
    assert_eq!(l.deadline(), Some(second + t_settle), "the step moved it");
    let sleep = l.sleep().unwrap();
    let remaining = (second + t_settle).saturating_sub(l.session.now_ms());
    assert!(
        sleep <= Duration::from_millis(remaining)
            && sleep + Duration::from_millis(20) >= Duration::from_millis(remaining),
        "the wake-up did not follow the deadline: {sleep:?} for {remaining} ms"
    );
    // the first deadline passes without a settlement
    while l.session.now_ms() < first + t_settle + 10 {
        l.turn(Some(Duration::from_millis(10)));
    }
    assert_eq!(
        l.settled.len(),
        0,
        "the superseded deadline settled nothing"
    );
    assert!(l.deadline().is_some());

    // an output settlement event closes every epoch at once (M7): the wake-up is gone
    connect(&mut l.state, &mut l.token, &out("C", 120)).unwrap();
    assert_eq!(l.deadline(), None, "closed by the settlement event");
    assert_eq!(l.sleep(), None, "the wake-up is cancelled with it");
}

/// The interaction real input can start today, a camera drag, is a continuous epoch: it has no
/// deadline, so the session does not wake for it, however long the button is held; its release
/// settles it. No real event gives the running compositor a settlement deadline.
#[test]
fn a_real_camera_drag_opens_an_epoch_without_a_deadline() {
    let mut l = on_b("drag");
    l.pan_b_away(false);
    let st = l.state.core().scale_state(l.w).unwrap();
    assert!(st.epoch.open, "the drag opened an epoch");
    assert_eq!(st.epoch.kind, Some(InteractionKind::CameraPan));
    assert!(st.epoch.continuous);
    assert_eq!(l.h(), 1.5, "decrease suppressed during the drag");
    assert_eq!(l.deadline(), None, "a continuous epoch has no deadline");
    l.idle();
    assert_eq!(l.sleep(), None, "held drag: the session is idle");
    // 400 ms, longer than the settlement timeout: nothing settles by time
    let until = l.session.now_ms() + 400;
    let mut turns = 0;
    while l.session.now_ms() < until {
        l.turn(Some(Duration::from_millis(100)));
        turns += 1;
    }
    assert!(turns <= 6, "the loop spun: {turns} turns in 400 ms");
    assert_eq!(l.settled.len(), 0);
    assert_eq!(l.h(), 1.5);

    l.release();
    assert_eq!(l.h(), 1.0, "the release settles the drag");
    assert!(!l.state.core().scale_state(l.w).unwrap().epoch.open);
    assert_eq!(l.sleep(), None);
}

/// Idle stays event-driven around a settlement: between arming and the deadline the loop takes
/// one turn, not a poll per millisecond.
#[test]
fn waiting_for_a_deadline_is_one_sleep_not_a_poll() {
    let mut l = on_b("poll");
    let opened = l.open_discrete();
    let t_settle = l.state.core().scale_config().t_settle_ms;
    let mut turns = 0;
    while l.deadline().is_some() {
        l.turn(Some(CAP));
        turns += 1;
        assert!(turns < 50, "the loop spins while waiting");
    }
    let woke = l.session.now_ms();
    assert!(woke >= opened + t_settle);
    assert!(
        woke < opened + t_settle + 200,
        "woke by the cap, not the deadline"
    );
    assert_eq!(turns, 1, "one sleep until the deadline");
}
