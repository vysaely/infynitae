//! `ext-idle-notify-v1` integration tests (INV-IDLE-1).

mod common;

use std::time::Duration;

use common::input::*;
use common::{Lockstep, Probe, output};
use infynitae_backend_headless::keyboard_key;
use infynitae_core::input::keys::KEY_A;
use infynitae_wayland::session::{Session, SessionOptions};
use smithay::reexports::calloop::EventLoop;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

wayland_client::delegate_noop!(Probe: ignore ExtIdleNotifierV1);

impl Dispatch<ExtIdleNotificationV1, ()> for Probe {
    fn event(
        state: &mut Self,
        _proxy: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                state.idle_events.push("idled");
            }
            ext_idle_notification_v1::Event::Resumed => {
                state.idle_events.push("resumed");
            }
            _ => {}
        }
    }
}

/// Binds the global named `interface` (the probe saw every global at connection).
fn bind<I>(r: &Rig, interface: &str, version: u32) -> I
where
    I: wayland_client::Proxy + 'static,
    Probe: wayland_client::Dispatch<I, ()>,
{
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .cloned()
            .unwrap_or_else(|| panic!("{interface} is offered"));
    r.ls.registry().bind(name, version, &r.ls.qh, ())
}

fn seat(r: &Rig) -> WlSeat {
    r.ls.probe
        .seat
        .as_ref()
        .expect("probe bound wl_seat")
        .clone()
}

/// A 100 ms notification is idled after 100 ms without input and resumed by the next key.
#[test]
fn a_100ms_notification_is_idled_after_100ms_and_resumed_by_next_key() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let notifier: ExtIdleNotifierV1 = bind(&r, "ext_idle_notifier_v1", 2);
    let s = seat(&r);
    let qh = r.ls.qh.clone();
    let _notification = notifier.get_idle_notification(100, &s, &qh, ());
    r.ls.pump();
    assert_eq!(
        r.ls.probe.idle_events,
        Vec::<&str>::new(),
        "initially active"
    );

    // Advance virtual time by 50 ms: timeout not yet reached
    r.ls.run_until(r.ls.now + Duration::from_millis(50));
    assert_eq!(
        r.ls.probe.idle_events,
        Vec::<&str>::new(),
        "still active at 50ms"
    );

    // Advance virtual time to 100 ms: timeout reached, notification is idled
    r.ls.run_until(r.ls.now + Duration::from_millis(50));
    assert_eq!(r.ls.probe.idle_events, vec!["idled"], "idled after 100ms");

    // Next key resumes the notification
    keyboard_key(
        &mut r.ls.state,
        &mut r.ls.input,
        KEY_A,
        true,
        r.ls.now.as_micros() as u64,
    )
    .unwrap();
    r.ls.pump();
    assert_eq!(
        r.ls.probe.idle_events,
        vec!["idled", "resumed"],
        "resumed by next key"
    );
}

/// With an idle inhibitor active, a standard notification is never idled.
#[test]
fn with_an_inhibitor_active_it_is_never_idled() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.state.manual_idle_inhibit = true;

    let notifier: ExtIdleNotifierV1 = bind(&r, "ext_idle_notifier_v1", 2);
    let s = seat(&r);
    let qh = r.ls.qh.clone();
    let _notification = notifier.get_idle_notification(100, &s, &qh, ());
    r.ls.pump();

    // Advance virtual time by 200 ms without input: never idles while inhibited
    r.ls.run_until(r.ls.now + Duration::from_millis(200));
    assert_eq!(
        r.ls.probe.idle_events,
        Vec::<&str>::new(),
        "never idled with inhibitor"
    );
}

/// An input-idle notification is idled even with an idle inhibitor active.
#[test]
fn an_input_idle_notification_is_idled_even_with_the_inhibitor() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.state.manual_idle_inhibit = true;

    let notifier: ExtIdleNotifierV1 = bind(&r, "ext_idle_notifier_v1", 2);
    let s = seat(&r);
    let qh = r.ls.qh.clone();
    let _notification = notifier.get_input_idle_notification(100, &s, &qh, ());
    r.ls.pump();

    // Advance virtual time by 100 ms: input-idle ignores inhibitors and idles
    r.ls.run_until(r.ls.now + Duration::from_millis(100));
    assert_eq!(
        r.ls.probe.idle_events,
        vec!["idled"],
        "idled despite inhibitor"
    );

    // Next key resumes it
    keyboard_key(
        &mut r.ls.state,
        &mut r.ls.input,
        KEY_A,
        true,
        r.ls.now.as_micros() as u64,
    )
    .unwrap();
    r.ls.pump();
    assert_eq!(
        r.ls.probe.idle_events,
        vec!["idled", "resumed"],
        "resumed by next key"
    );
}

/// With no notification, the session's wake deadline is unchanged (INV-IDLE-1).
#[test]
fn with_no_notification_session_wake_deadline_is_unchanged() {
    let (core, _input, _shell, _token) =
        infynitae_core::Core::new(infynitae_core::navigation::PersistedCameras::default());
    let display =
        smithay::reexports::wayland_server::Display::<infynitae_wayland::WaylandState>::new()
            .unwrap();
    let mut state = infynitae_wayland::WaylandState::new(&display.handle(), core);
    let mut event_loop: EventLoop<()> = EventLoop::try_new().unwrap();
    let mut session = Session::new(
        display,
        &event_loop.handle(),
        &SessionOptions {
            socket_name: Some(format!("infynitae-idle-test-{}", std::process::id())),
            refresh_mhz: 60_000,
            ..SessionOptions::default()
        },
    )
    .unwrap();

    // Consume the initial startup frame so the session is idle
    event_loop.dispatch(Some(Duration::ZERO), &mut ()).unwrap();
    session.dispatch(&mut state);
    assert!(session.frame_due());
    assert_eq!(session.timeout(), None, "session is idle");

    let now = Duration::from_secs(10);
    // With no idle notifications, deadline is None
    assert_eq!(state.idle_notify_deadline(), None);

    // Setting idle deadline with None leaves session timeout unchanged (None)
    session.set_idle_deadline(state.idle_notify_check(now), now);
    assert_eq!(
        session.timeout(),
        None,
        "timeout unchanged with no notification"
    );

    // With a pending notification deadline, timeout is armed
    session.set_idle_deadline(Some(now + Duration::from_millis(500)), now);
    let t = session.timeout().expect("armed for idle notification");
    assert!(t <= Duration::from_millis(500) && t > Duration::from_millis(400));

    // When disarmed, timeout is None again (INV-IDLE-1)
    session.set_idle_deadline(None, now);
    assert_eq!(session.timeout(), None, "idle again when disarmed");
}
