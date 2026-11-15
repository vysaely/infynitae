//! Audit proof tests for animation timing, frame callbacks, `wp_presentation`, and `FrameSchedule`
//! (`AUDIT_FRAME_TIMING`). Every test here reproduces a confirmed bug and stays `#[ignore]`d until fixed.

mod common;

use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsFd;
use std::time::Duration;

use common::Lockstep;
use common::input::{BTN_MIDDLE, Rig, a_1080, logo};
use infynitae_backend_headless::vblank_period;
use infynitae_wayland::ClientState;
use infynitae_wayland::frame_schedule::{FrameSchedule, LatencyMode};
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_compositor::WlCompositor,
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::presentation_time::client::{
    wp_presentation::WpPresentation,
    wp_presentation_feedback::{self, WpPresentationFeedback},
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::XdgToplevel,
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::Layer,
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
};

fn pan_off_screen(r: &mut Rig) {
    r.button(BTN_MIDDLE, true, logo());
    for _ in 0..10 {
        r.motion((300.0, 0.0));
    }
    r.button(BTN_MIDDLE, false, logo());
}

#[test]
#[ignore = "AUDIT_FRAME_TIMING: window_captured emits wp_presentation.presented for an off-screen window"]
fn window_captured_offscreen_must_not_emit_presentation_feedback() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    let w =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let e = r.ls.state.elements().next().unwrap();
    let output = r.ls.output();

    pan_off_screen(&mut r);
    assert!(
        !r.ls.state.core().membership().is_observed(e),
        "W must be unobserved off-screen"
    );

    let qh = r.ls.qh.clone();
    let seq = r.ls.probe.frame_and_draw(&qh, w);
    r.ls.pump();
    let mark = r.ls.probe.log.len();

    r.ls.state.window_captured(e, output, r.ls.now);
    r.ls.pump();

    let after = &r.ls.probe.log[mark..];
    assert!(
        !after
            .iter()
            .any(|l| l.contains(&format!("presented #{seq}"))),
        "off-screen capture must not claim physical scanout (INV-FRAME-5): {after:?}"
    );
}

#[test]
#[ignore = "AUDIT_FRAME_TIMING: layer surfaces never receive wp_presentation feedback"]
fn layer_surface_receives_wp_presentation_feedback_on_vblank() {
    let mut ls = Lockstep::new(Some(a_1080()));
    ls.bind_layer_shell();
    let qh = ls.qh.clone();
    let layer = ls.probe.layer_surface(
        &qh,
        "L",
        Layer::Top,
        Anchor::Top | Anchor::Left | Anchor::Right,
        (1920, 40),
        40,
        KeyboardInteractivity::None,
    );
    ls.pump();
    assert!(ls.probe.surfaces[layer].configured);

    let seq = ls.probe.frame_and_draw(&qh, layer);
    ls.pump();
    ls.run_until(ls.now + vblank_period(60_000));

    assert!(
        ls.probe.frames.iter().any(|(_, label)| *label == "L"),
        "sanity check: wl_surface.frame completed on vblank"
    );
    assert!(
        ls.probe.log.contains(&format!("presented #{seq}")),
        "visible layer surface must receive wp_presentation_feedback.presented: {:?}",
        ls.probe.log
    );
}

#[test]
#[ignore = "AUDIT_FRAME_TIMING: bufferless frame commit marks WaylandState::new_buffer = true"]
fn bufferless_frame_commit_does_not_mark_new_buffer() {
    let mut r = Rig::new(Lockstep::new(Some(a_1080())));
    r.map_w();
    let _ = r.ls.state.take_new_buffer();

    let w =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let qh = r.ls.qh.clone();
    r.ls.probe.surfaces[w].wl.frame(&qh, w);
    r.ls.probe.surfaces[w].wl.commit();
    r.ls.pump();

    assert!(
        !r.ls.state.take_new_buffer(),
        "commit without attaching a buffer must not mark a new buffer"
    );
}

#[test]
#[ignore = "AUDIT_FRAME_TIMING: FrameSchedule::start_at trusts stale last_vblank past GUARD_TRUSTED"]
fn frame_schedule_start_at_ignores_stale_last_vblank_past_guard_trusted() {
    let period = Duration::from_micros(6_060);
    let mut s = FrameSchedule::new(LatencyMode::Balanced, period);
    let mut t = Duration::from_secs(10) - period * 4;
    for _ in 0..4 {
        s.rendered(
            t + period - Duration::from_micros(1_500),
            t + period - Duration::from_micros(500),
        );
        t += period;
        s.vblank(t);
    }
    assert!(s.budget().is_some());

    // Two seconds after last vblank (> GUARD_TRUSTED = 1 s), phase is stale and must not defer.
    let stale_now = t + Duration::from_secs(2) + Duration::from_micros(200);
    assert_eq!(
        s.start_at(stale_now),
        None,
        "start_at must not defer against a last_vblank older than GUARD_TRUSTED"
    );
}

#[derive(Default)]
struct RawPresClient {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    wm_base: Option<XdgWmBase>,
    pres: Option<WpPresentation>,
    configured: bool,
    presented: Vec<(u64, u32, wp_presentation_feedback::Kind)>,
}

impl Dispatch<WlRegistry, ()> for RawPresClient {
    fn event(
        st: &mut Self,
        reg: &WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_compositor" => st.compositor = Some(reg.bind(name, version.min(6), qh, ())),
                "wl_shm" => st.shm = Some(reg.bind(name, 1, qh, ())),
                "xdg_wm_base" => st.wm_base = Some(reg.bind(name, version.min(6), qh, ())),
                "wp_presentation" => st.pres = Some(reg.bind(name, version.min(2), qh, ())),
                _ => {}
            }
        }
    }
}

fn pump_client(
    ls: &mut Lockstep,
    conn: &Connection,
    queue: &mut wayland_client::EventQueue<RawPresClient>,
    client: &mut RawPresClient,
) {
    for _ in 0..20 {
        let _ = conn.flush();
        ls.pump();
        if let Some(guard) = conn.prepare_read() {
            let _ = guard.read();
        }
        queue.dispatch_pending(client).unwrap();
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for RawPresClient {
    fn event(
        _: &mut Self,
        wm: &XdgWmBase,
        ev: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = ev {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for RawPresClient {
    fn event(
        st: &mut Self,
        xdg: &XdgSurface,
        ev: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = ev {
            xdg.ack_configure(serial);
            st.configured = true;
        }
    }
}

impl Dispatch<WpPresentationFeedback, ()> for RawPresClient {
    fn event(
        st: &mut Self,
        _: &WpPresentationFeedback,
        ev: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_presentation_feedback::Event::Presented {
            refresh,
            seq_hi,
            seq_lo,
            flags,
            ..
        } = ev
        {
            let seq = (u64::from(seq_hi) << 32) | u64::from(seq_lo);
            let flags = match flags {
                wayland_client::WEnum::Value(k) => k,
                wayland_client::WEnum::Unknown(bits) => {
                    wp_presentation_feedback::Kind::from_bits_truncate(bits)
                }
            };
            st.presented.push((seq, refresh, flags));
        }
    }
}

delegate_noop!(RawPresClient: ignore WlCompositor);
delegate_noop!(RawPresClient: ignore WlSurface);
delegate_noop!(RawPresClient: ignore WlShm);
delegate_noop!(RawPresClient: ignore WlShmPool);
delegate_noop!(RawPresClient: ignore WlBuffer);
delegate_noop!(RawPresClient: ignore XdgToplevel);
delegate_noop!(RawPresClient: ignore WpPresentation);

fn create_shm_buffer(shm: &WlShm, qh: &QueueHandle<RawPresClient>, w: i32, h: i32) -> WlBuffer {
    let stride = w * 4;
    let size = (stride * h) as usize;
    let path = std::env::temp_dir().join(format!("infynitae-pres-audit-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    let _ = std::fs::remove_file(&path);
    file.write_all(&vec![0xffu8; size]).unwrap();
    let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
    pool.create_buffer(0, w, h, stride, wl_shm::Format::Argb8888, qh, ())
}

#[test]
#[ignore = "AUDIT_FRAME_TIMING: complete_frames hardcodes Kind::empty() on vblank presentation"]
fn complete_frames_sets_vsync_flag_on_vblank_presentation() {
    let mut ls = Lockstep::new(Some(a_1080()));
    let (server_end, client_end) = std::os::unix::net::UnixStream::pair().unwrap();
    client_end.set_nonblocking(true).unwrap();
    ls.display
        .handle()
        .insert_client(server_end, std::sync::Arc::new(ClientState::default()))
        .unwrap();

    let conn = Connection::from_socket(client_end).unwrap();
    let mut queue = conn.new_event_queue::<RawPresClient>();
    let qh = queue.handle();
    let _reg = conn.display().get_registry(&qh, ());
    let mut client = RawPresClient::default();
    pump_client(&mut ls, &conn, &mut queue, &mut client);

    let compositor = client.compositor.clone().unwrap();
    let shm = client.shm.clone().unwrap();
    let wm_base = client.wm_base.clone().unwrap();
    let pres = client.pres.clone().unwrap();

    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let _toplevel = xdg.get_toplevel(&qh, ());
    surface.commit();
    pump_client(&mut ls, &conn, &mut queue, &mut client);
    assert!(client.configured);

    let buf = create_shm_buffer(&shm, &qh, 200, 200);
    pres.feedback(&surface, &qh, ());
    surface.attach(Some(&buf), 0, 0);
    surface.damage_buffer(0, 0, 200, 200);
    surface.commit();
    pump_client(&mut ls, &conn, &mut queue, &mut client);

    ls.run_until(ls.now + vblank_period(60_000));
    if let Some(guard) = conn.prepare_read() {
        let _ = guard.read();
    }
    queue.dispatch_pending(&mut client).unwrap();

    assert_eq!(client.presented.len(), 1, "expected 1 presented event");
    let (_, _, flags) = client.presented[0];
    assert!(
        flags.contains(wp_presentation_feedback::Kind::Vsync),
        "vblank presentation omitted Vsync flag: {flags:?}"
    );
}
