//! Sharing one window (`ext-foreign-toplevel-image-capture-source-v1`, the way
//! xdg-desktop-portal-wlr shares a window): a window of the window list becomes a capture source,
//! its session announces the window's size, a capture is queued as the window's, and the
//! window is drawn alone at its own coordinates. A private window, and any window while the
//! session is locked, cannot be captured; a closed window's session stops.

mod common;

use common::Probe;
use common::input::*;
use infynitae_wayland::convert;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ExportMem, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::utils::Rectangle;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{
    self, ExtSessionLockSurfaceV1,
};
use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;
use wayland_protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;

wayland_client::delegate_noop!(Probe: ignore ExtForeignToplevelImageCaptureSourceManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtImageCaptureSourceV1);
wayland_client::delegate_noop!(Probe: ignore ExtImageCopyCaptureManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtForeignToplevelHandleV1);
wayland_client::delegate_noop!(Probe: ignore WpSinglePixelBufferManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(Probe: ignore ExtSessionLockV1);

impl Dispatch<ExtSessionLockSurfaceV1, ()> for Probe {
    fn event(
        st: &mut Self,
        s: &ExtSessionLockSurfaceV1,
        ev: ext_session_lock_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = ev
        {
            s.ack_configure(serial);
            st.log.push(format!("lock surface {width}x{height}"));
        }
    }
}

thread_local! {
    static HANDLES: std::cell::RefCell<Vec<ExtForeignToplevelHandleV1>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Dispatch<ExtForeignToplevelListV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtForeignToplevelListV1,
        ev: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = ev {
            HANDLES.with_borrow_mut(|h| h.push(toplevel));
        }
    }

    wayland_client::event_created_child!(Probe, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        ev: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event as E;
        match ev {
            E::BufferSize { width, height } => {
                st.log.push(format!("session size {width}x{height}"))
            }
            E::Stopped => st.log.push("session stopped".into()),
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        ev: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event as E;
        match ev {
            E::Ready => st.log.push("frame ready".into()),
            E::Failed { reason } => st.log.push(format!(
                "frame failed {}",
                match reason {
                    WEnum::Value(r) => format!("{r:?}"),
                    WEnum::Unknown(u) => u.to_string(),
                }
            )),
            _ => {}
        }
    }
}

fn bind<I>(r: &Rig, interface: &str) -> I
where
    I: wayland_client::Proxy + 'static,
    Probe: Dispatch<I, ()>,
{
    let (name, _, _) =
        r.ls.probe
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .cloned()
            .unwrap_or_else(|| panic!("{interface} is offered"));
    r.ls.registry().bind(name, 1, &r.ls.qh, ())
}

const SQUARE: u32 = 0xFF12_AB34;

/// W (800×600 window geometry, 20 px into its surface, as a window drawing its own shadow) with
/// a 50×50 square subsurface at (10, 20) in the window geometry, and a capture session of W.
fn setup() -> (Rig, ExtImageCopyCaptureSessionV1) {
    HANDLES.with_borrow_mut(Vec::clear);
    let mut r = Rig::x8();
    r.ls.state.screencopy.enable_capture();
    let qh = r.ls.qh.clone();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    let s_idx =
        r.ls.probe
            .subsurface_uncommitted(&qh, "S", w_idx, (30, 40), (50, 50), false);
    let spb: WpSinglePixelBufferManagerV1 = bind(&r, "wp_single_pixel_buffer_manager_v1");
    let ch = |v: u32| v * 0x0101_0101;
    let buffer = spb.create_u32_rgba_buffer(ch(0x12), ch(0xAB), ch(0x34), u32::MAX, &qh, ());
    let s = r.ls.probe.surfaces[s_idx].wl.clone();
    let viewport =
        r.ls.probe
            .viewporter
            .as_ref()
            .expect("wp_viewporter")
            .get_viewport(&s, &qh, ());
    viewport.set_destination(50, 50);
    s.attach(Some(&buffer), 0, 0);
    s.damage_buffer(0, 0, 1, 1);
    s.commit();
    r.ls.probe.surfaces[w_idx].wl.commit();
    r.ls.pump();
    r.frame();

    let _list: ExtForeignToplevelListV1 = bind(&r, "ext_foreign_toplevel_list_v1");
    r.ls.pump();
    let handle = HANDLES
        .with_borrow(|h| h.first().cloned())
        .expect("W is listed");
    let sources: ExtForeignToplevelImageCaptureSourceManagerV1 =
        bind(&r, "ext_foreign_toplevel_image_capture_source_manager_v1");
    let manager: ExtImageCopyCaptureManagerV1 = bind(&r, "ext_image_copy_capture_manager_v1");
    let source = sources.create_source(&handle, &qh, ());
    let session = manager.create_session(&source, Options::PaintCursors, &qh, ());
    r.ls.pump();
    (r, session)
}

/// One capture into a buffer of `size`: what the frame said, and the window it was queued for.
fn capture(
    r: &mut Rig,
    session: &ExtImageCopyCaptureSessionV1,
    size: (i32, i32),
) -> (Vec<String>, Option<infynitae_core::stage::ElementId>) {
    let qh = r.ls.qh.clone();
    let frame = session.create_frame(&qh, ());
    let buffer = r.ls.probe.buffer(&qh, size.0, size.1);
    frame.attach_buffer(&buffer);
    frame.capture();
    r.ls.probe.log.clear();
    r.ls.pump();
    let output = r.ls.output();
    let mut window = None;
    for p in r.ls.state.screencopy.take_pending(output) {
        window = p.window;
        p.ready(
            (size.0 as u32, size.1 as u32),
            std::time::Duration::from_millis(5),
        );
    }
    r.ls.pump();
    (std::mem::take(&mut r.ls.probe.log), window)
}

#[test]
fn a_window_is_captured_alone_at_its_own_size() {
    let (mut r, session) = setup();
    assert!(
        r.ls.probe.log.contains(&"session size 800x600".to_string()),
        "the session announces the window's size: {:?}",
        r.ls.probe.log
    );
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    let (log, window) = capture(&mut r, &session, (800, 600));
    assert_eq!(window, Some(w), "queued as W's capture");
    assert!(log.contains(&"frame ready".to_string()), "{log:?}");

    // the backend draws W alone: the square at its place in W, nothing else of the screen
    let mut renderer = PixmanRenderer::new().expect("pixman");
    let (elements, k, size) = convert::window_capture_elements(&r.ls.state, &mut renderer, w)
        .ok()
        .flatten()
        .expect("W can be drawn");
    assert_eq!(size, (800, 600));
    let mut image: Image<'static, 'static> = renderer
        .create_buffer(Fourcc::Argb8888, size.into())
        .expect("target");
    let mut target = renderer.bind(&mut image).expect("bind");
    let mut tracker = smithay::backend::renderer::damage::OutputDamageTracker::new(
        size,
        k,
        smithay::utils::Transform::Normal,
    );
    tracker
        .render_output(
            &mut renderer,
            &mut target,
            0,
            &elements,
            smithay::backend::renderer::Color32F::TRANSPARENT,
        )
        .expect("render");
    let m = renderer
        .copy_framebuffer(&target, Rectangle::from_size(size.into()), Fourcc::Argb8888)
        .expect("read back");
    let px: Vec<u32> = renderer
        .map_texture(&m)
        .expect("map")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let at: Vec<(usize, usize)> = px
        .iter()
        .enumerate()
        .filter(|(_, p)| **p == SQUARE)
        .map(|(i, _)| (i % 800, i / 800))
        .collect();
    assert_eq!(at.len(), 50 * 50, "the whole square");
    let corner = at.iter().fold((usize::MAX, usize::MAX), |(x, y), p| {
        (x.min(p.0), y.min(p.1))
    });
    assert_eq!(
        corner,
        (10, 20),
        "at its place in the window geometry, the shadow left out"
    );
}

#[test]
fn a_private_window_is_not_captured() {
    let (mut r, session) = setup();
    let w = *r.ls.state.core().stage().mapped_elements().last().unwrap();
    r.ls.state.private_elements.insert(w);
    let (log, window) = capture(&mut r, &session, (800, 600));
    assert_eq!(window, None, "nothing queued");
    assert!(log.contains(&"frame failed Unknown".to_string()), "{log:?}");
}

#[test]
fn a_closed_window_stops_its_session() {
    let (mut r, session) = setup();
    let w_idx =
        r.ls.probe
            .surfaces
            .iter()
            .position(|s| s.label == "W")
            .unwrap();
    r.ls.probe.surfaces[w_idx]
        .toplevel
        .clone()
        .unwrap()
        .destroy();
    r.ls.pump();
    let (log, window) = capture(&mut r, &session, (800, 600));
    assert_eq!(window, None);
    assert!(log.contains(&"frame failed Stopped".to_string()), "{log:?}");
    assert!(log.contains(&"session stopped".to_string()), "{log:?}");
}

#[test]
fn nothing_is_captured_while_locked() {
    let (mut r, session) = setup();
    r.ls.state.enable_session_lock();
    r.ls.pump();
    let manager: ExtSessionLockManagerV1 = bind(&r, "ext_session_lock_manager_v1");
    let qh = r.ls.qh.clone();
    let lock = manager.lock(&qh, ());
    let surface =
        r.ls.probe
            .compositor
            .as_ref()
            .expect("compositor")
            .create_surface(&qh, ());
    let output = r.ls.probe.outputs[0].1.clone();
    lock.get_lock_surface(&surface, &output, &qh, ());
    r.ls.pump();
    let size =
        r.ls.probe
            .log
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("lock surface ").map(str::to_owned))
            .expect("configured");
    let (w, h) = size.split_once('x').unwrap();
    let buffer =
        r.ls.probe
            .buffer(&qh, w.parse().unwrap(), h.parse().unwrap());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    r.ls.pump();
    r.ls.state.lock_frame_drawn();
    r.ls.pump();
    assert!(r.ls.state.is_locked());
    let (log, window) = capture(&mut r, &session, (800, 600));
    assert_eq!(window, None, "nothing queued");
    assert!(log.contains(&"frame failed Unknown".to_string()), "{log:?}");
}
