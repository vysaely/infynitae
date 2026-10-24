//! `ext-image-copy-capture-v1` output capture (2026-10-24; infynitae-wayland `image_copy`): a
//! session announces the output's buffer size and formats; frame after frame is captured (the
//! console backend draws them; here the test answers the queue as it does); a changed output
//! size fails the next capture with `buffer_constraints` and announces the new size.

mod common;

use common::Probe;
use common::input::*;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtOutputImageCaptureSourceManagerV1,
        _: <ExtOutputImageCaptureSourceManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCaptureSourceV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtImageCaptureSourceV1,
        _: <ExtImageCaptureSourceV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ExtImageCopyCaptureManagerV1,
        _: <ExtImageCopyCaptureManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
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
            E::BufferSize { width, height } => st.log.push(format!("session size {width}x{height}")),
            E::ShmFormat { format } => st.log.push(format!("session shm {format:?}")),
            E::DmabufDevice { .. } => st.log.push("session dmabuf device".into()),
            E::DmabufFormat { format, .. } => st.log.push(format!("session dmabuf {format:#x}")),
            E::Done => st.log.push("session done".into()),
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
            E::Damage { width, height, .. } => st.log.push(format!("frame damage {width}x{height}")),
            E::PresentationTime { .. } => st.log.push("frame time".into()),
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

fn bind<I: wayland_client::Proxy + 'static>(r: &mut Rig, interface: &str) -> I
where
    Probe: Dispatch<I, ()>,
{
    let (name, _, _) = r
        .ls
        .probe
        .globals
        .iter()
        .find(|(_, i, _)| i == interface)
        .cloned()
        .unwrap_or_else(|| panic!("{interface} is offered"));
    let qh = r.ls.qh.clone();
    r.ls.registry().bind(name, 1, &qh, ())
}

/// A session on the output: (session, its log lines).
fn session(r: &mut Rig) -> ExtImageCopyCaptureSessionV1 {
    let sources: ExtOutputImageCaptureSourceManagerV1 =
        bind(r, "ext_output_image_capture_source_manager_v1");
    let manager: ExtImageCopyCaptureManagerV1 = bind(r, "ext_image_copy_capture_manager_v1");
    let qh = r.ls.qh.clone();
    let output = r.ls.probe.outputs[0].1.clone();
    let source = sources.create_source(&output, &qh, ());
    let session = manager.create_session(&source, Options::PaintCursors, &qh, ());
    r.ls.pump();
    session
}

/// One capture into a fresh buffer of `size`; the backend's part (drawing, then `ready`) done
/// as the console does it.
fn capture(r: &mut Rig, session: &ExtImageCopyCaptureSessionV1, size: (i32, i32)) {
    let qh = r.ls.qh.clone();
    let frame = session.create_frame(&qh, ());
    let buffer = r.ls.probe.buffer(&qh, size.0, size.1);
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, size.0, size.1);
    frame.capture();
    r.ls.pump();
    let output = r.ls.output();
    for p in r.ls.state.screencopy.take_pending(output) {
        p.ready((size.0 as u32, size.1 as u32), std::time::Duration::from_millis(5));
    }
    r.ls.pump();
    frame.destroy();
}

#[test]
fn a_session_announces_the_output_and_captures_frame_after_frame() {
    let mut r = Rig::new(common::Lockstep::new(Some(a_1080())));
    let s = session(&mut r);
    let log = r.ls.probe.log.join("\n");
    assert!(log.contains("session size 1920x1080"), "{log}");
    assert!(log.contains("session shm Value(Xrgb8888)"), "{log}");
    assert!(log.contains("session shm Value(Argb8888)"), "{log}");
    assert!(log.contains("session done"), "{log}");

    // the stall this protocol avoids is "one frame, then nothing": capture several in a row
    for _ in 0..5 {
        capture(&mut r, &s, (1920, 1080));
    }
    let ready = r.ls.probe.log.iter().filter(|l| *l == "frame ready").count();
    assert_eq!(ready, 5, "{:?}", r.ls.probe.log);
    assert!(r.ls.probe.log.contains(&"frame damage 1920x1080".to_string()));
    assert!(r.ls.probe.log.contains(&"frame time".to_string()));
}

#[test]
fn a_resized_output_fails_the_capture_with_new_constraints() {
    let mut r = Rig::new(common::Lockstep::new(Some(a_1080())));
    let s = session(&mut r);
    let id = r.ls.output();
    r.ls.reconfigure(id, &common::output_k("A", (2560, 1440), 120, 60_000));
    r.ls.pump();
    let before = r.ls.probe.log.len();
    capture(&mut r, &s, (1920, 1080));
    let after = r.ls.probe.log[before..].join("\n");
    assert!(after.contains("frame failed BufferConstraints"), "{after}");
    assert!(after.contains("session size 2560x1440"), "{after}");
    // a buffer of the new size captures
    capture(&mut r, &s, (2560, 1440));
    assert_eq!(r.ls.probe.log.last().map(String::as_str), Some("frame ready"));
}
