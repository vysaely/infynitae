//! `ext-image-copy-capture-v1` with `ext-output-image-capture-source-v1` (2026-10-24): output
//! capture through the newer capture protocol. xdg-desktop-portal-wlr uses it when offered;
//! up to 0.8.4 its wlr-screencopy path delivers one frame and then stalls (its PipeWire stream
//! is driven by ext-image-copy; fixed upstream after 0.8.4), so screen sharing with a
//! distribution's portal needs this protocol.
//!
//! A captured frame joins the wlr-screencopy queue (`screencopy::PendingFrame`): the backend
//! draws both the same way. A session announces the output's size, shm formats and the GPU and
//! linear dmabuf formats; when the size changes, the next capture fails with
//! `buffer_constraints` and the new constraints follow (the protocol's resize path).
//!
//! A window alone can be captured too (`ext-foreign-toplevel-image-capture-source-v1`, a window
//! of `ext_foreign_toplevel_list_v1`): sharing one window. It is drawn by itself at zoom 1 and
//! the first screen's scale (`Core::window_frame`), the size of its window geometry, wherever it
//! is on the canvas and whether or not it is in view. Never while the session is locked, never
//! a private window (`PRIVACY_APPS`, `PRIVACY_TITLES`): such a capture fails. Cursor sessions
//! are not offered: a cursor session's capture session is stopped at once.

use std::sync::Mutex;

use infynitae_core::output::OutputId;
use infynitae_core::stage::ElementId;
use smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::ext_foreign_toplevel_image_capture_source_manager_v1::{
    self, ExtForeignToplevelImageCaptureSourceManagerV1,
};
use smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::{
    ext_image_capture_source_v1::{self, ExtImageCaptureSourceV1},
    ext_output_image_capture_source_manager_v1::{self, ExtOutputImageCaptureSourceManagerV1},
};
pub use smithay::reexports::wayland_protocols::ext::image_copy_capture::v1::server::ext_image_copy_capture_frame_v1::{
    ExtImageCopyCaptureFrameV1, FailureReason,
};
use smithay::reexports::wayland_protocols::ext::image_copy_capture::v1::server::{
    ext_image_copy_capture_cursor_session_v1::{self, ExtImageCopyCaptureCursorSessionV1},
    ext_image_copy_capture_frame_v1,
    ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::GlobalId,
};

use crate::WaylandState;
use crate::screencopy::{CaptureFrame, PendingFrame};

/// DRM fourcc codes of the formats offered (the same as wlr-screencopy's): XR24 and AR24.
const DRM_XRGB8888: u32 = 0x3432_5258;
const DRM_ARGB8888: u32 = 0x3432_5241;
/// `DRM_FORMAT_MOD_LINEAR`.
const MOD_LINEAR: u64 = 0;

/// The two globals, alive for the state's lifetime.
pub struct ImageCopyState {
    _sources: GlobalId,
    _window_sources: GlobalId,
    _manager: GlobalId,
}

impl ImageCopyState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            _sources: dh
                .create_global::<WaylandState, ExtOutputImageCaptureSourceManagerV1, ()>(1, ()),
            _window_sources: dh
                .create_global::<WaylandState, ExtForeignToplevelImageCaptureSourceManagerV1, ()>(
                    1,
                    (),
                ),
            _manager: dh.create_global::<WaylandState, ExtImageCopyCaptureManagerV1, ()>(1, ()),
        }
    }
}

/// What a capture source shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Output(OutputId),
    Window(ElementId),
}

/// A capture source: what it was made for (`None` when unknown).
pub struct SourceData {
    target: Option<Target>,
}

/// A capture session: its target, whether cursors are painted, and the buffer size it last
/// announced.
pub struct SessionData {
    target: Option<Target>,
    paint_cursors: bool,
    size: Mutex<Option<(i32, i32)>>,
}

/// A frame: its session, the buffer attached, whether `capture` was sent.
pub struct FrameData {
    session: ExtImageCopyCaptureSessionV1,
    buffer: Mutex<Option<WlBuffer>>,
    captured: Mutex<bool>,
}

impl WaylandState {
    /// A capture of `window` alone was drawn on `output`'s loop at `now`: the window's frame
    /// callbacks complete, so a shared window draws at the rate the share takes frames even off
    /// screen (`Core::frame_captured`).
    pub fn window_captured(
        &mut self,
        window: ElementId,
        output: OutputId,
        now: std::time::Duration,
    ) {
        if self.core.frame_captured(window) {
            self.complete_frames(output, &[window], now);
        }
    }

    /// The scale a window is captured at: the first screen's.
    pub(crate) fn window_capture_scale(&self) -> f64 {
        self.core.outputs().first().map_or(1.0, |o| o.scale)
    }

    /// The size of a capture of `target` now; `None` when it cannot be captured (the output is
    /// gone, the window unmapped).
    fn capture_size(&self, target: Target) -> Option<(i32, i32)> {
        match target {
            Target::Output(o) => self.output_mode_size(o),
            Target::Window(e) => self
                .core
                .window_frame(e, self.window_capture_scale())
                .map(|f| (f.view.size.0 as i32, f.view.size.1 as i32)),
        }
    }

    /// Tells `session` what to capture into for its target now, or that it stopped (the
    /// target is gone).
    fn send_capture_constraints(&self, session: &ExtImageCopyCaptureSessionV1, data: &SessionData) {
        let Some(size) = data.target.and_then(|t| self.capture_size(t)) else {
            session.stopped();
            return;
        };
        session.buffer_size(size.0 as u32, size.1 as u32);
        session.shm_format(wl_shm::Format::Xrgb8888);
        session.shm_format(wl_shm::Format::Argb8888);
        if let Some(dev) = self.dmabuf_main_dev {
            session.dmabuf_device(dev.to_ne_bytes().to_vec());
            for format in [DRM_XRGB8888, DRM_ARGB8888] {
                session.dmabuf_format(format, MOD_LINEAR.to_ne_bytes().to_vec());
            }
        }
        session.done();
        *data.size.lock().unwrap() = Some(size);
    }
}

impl GlobalDispatch<ExtOutputImageCaptureSourceManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtOutputImageCaptureSourceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ExtOutputImageCaptureSourceManagerV1,
        request: ext_output_image_capture_source_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_output_image_capture_source_manager_v1::Request::CreateSource {
            source,
            output,
        } = request
        {
            let target = state.output_id_from_wl(&output).map(Target::Output);
            data_init.init(source, SourceData { target });
        }
    }
}

impl GlobalDispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtForeignToplevelImageCaptureSourceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtForeignToplevelImageCaptureSourceManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ExtForeignToplevelImageCaptureSourceManagerV1,
        request: ext_foreign_toplevel_image_capture_source_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_foreign_toplevel_image_capture_source_manager_v1::Request::CreateSource {
            source,
            toplevel_handle,
        } = request
        {
            let target = state.listed_toplevel(&toplevel_handle).map(Target::Window);
            data_init.init(source, SourceData { target });
        }
    }
}

impl Dispatch<ExtImageCaptureSourceV1, SourceData> for WaylandState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ExtImageCaptureSourceV1,
        _request: ext_image_capture_source_v1::Request,
        _data: &SourceData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<ExtImageCopyCaptureManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtImageCopyCaptureManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ExtImageCopyCaptureManagerV1,
        request: ext_image_copy_capture_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_image_copy_capture_manager_v1::Request::CreateSession {
                session,
                source,
                options,
            } => {
                let data = SessionData {
                    target: source.data::<SourceData>().and_then(|d| d.target),
                    paint_cursors: options
                        .into_result()
                        .is_ok_and(|o| o.contains(Options::PaintCursors)),
                    size: Mutex::new(None),
                };
                let session = data_init.init(session, data);
                if let Some(data) = session.data::<SessionData>() {
                    state.send_capture_constraints(&session, data);
                }
            }
            ext_image_copy_capture_manager_v1::Request::CreatePointerCursorSession {
                session,
                ..
            } => {
                data_init.init(session, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureCursorSessionV1, ()> for WaylandState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ExtImageCopyCaptureCursorSessionV1,
        request: ext_image_copy_capture_cursor_session_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_cursor_session_v1::Request::GetCaptureSession { session } =
            request
        {
            // cursor images are not offered: the session ends before it starts
            let session = data_init.init(
                session,
                SessionData {
                    target: None,
                    paint_cursors: false,
                    size: Mutex::new(None),
                },
            );
            session.stopped();
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, SessionData> for WaylandState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        resource: &ExtImageCopyCaptureSessionV1,
        request: ext_image_copy_capture_session_v1::Request,
        _data: &SessionData,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_image_copy_capture_session_v1::Request::CreateFrame { frame } = request {
            data_init.init(
                frame,
                FrameData {
                    session: resource.clone(),
                    buffer: Mutex::new(None),
                    captured: Mutex::new(false),
                },
            );
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, FrameData> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        frame: &ExtImageCopyCaptureFrameV1,
        request: ext_image_copy_capture_frame_v1::Request,
        data: &FrameData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_image_copy_capture_frame_v1::Request::AttachBuffer { buffer } => {
                *data.buffer.lock().unwrap() = Some(buffer);
            }
            ext_image_copy_capture_frame_v1::Request::Capture => {
                if std::mem::replace(&mut *data.captured.lock().unwrap(), true) {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::AlreadyCaptured,
                        "capture was already sent",
                    );
                    return;
                }
                if !state.screencopy.serves_capture() {
                    frame.failed(FailureReason::Unknown);
                    return;
                }
                let Some(buffer) = data.buffer.lock().unwrap().clone() else {
                    frame.post_error(
                        ext_image_copy_capture_frame_v1::Error::NoBuffer,
                        "capture without attach_buffer",
                    );
                    return;
                };
                let Some(session) = data.session.data::<SessionData>() else {
                    frame.failed(FailureReason::Unknown);
                    return;
                };
                let current = session.target.and_then(|t| state.capture_size(t));
                let Some(target) = session.target.filter(|_| current.is_some()) else {
                    frame.failed(FailureReason::Stopped);
                    data.session.stopped();
                    return;
                };
                if let Target::Window(e) = target
                    && (state.is_locked() || state.is_element_private(e))
                {
                    // locked, or a window kept out of captures: nothing of it is shared
                    frame.failed(FailureReason::Unknown);
                    return;
                }
                if *session.size.lock().unwrap() != current {
                    // the target changed size: the client reallocates for the new constraints
                    frame.failed(FailureReason::BufferConstraints);
                    state.send_capture_constraints(&data.session, session);
                    return;
                }
                // a window is drawn by the first screen's loop
                let (output_id, window) = match target {
                    Target::Output(o) => (o, None),
                    Target::Window(e) => match state.core.outputs().first() {
                        Some(o) => (o.id, Some(e)),
                        None => {
                            frame.failed(FailureReason::Unknown);
                            return;
                        }
                    },
                };
                state.note_capture(client);
                state.screencopy.pending.lock().unwrap().push(PendingFrame {
                    frame: CaptureFrame::Ext(frame.clone()),
                    output_id,
                    buffer,
                    overlay_cursor: session.paint_cursors && window.is_none(),
                    with_damage: true,
                    region: None,
                    window,
                });
            }
            ext_image_copy_capture_frame_v1::Request::Destroy => {
                let id = frame.id();
                state
                    .screencopy
                    .pending
                    .lock()
                    .unwrap()
                    .retain(|p| p.id() != id);
            }
            _ => {}
        }
    }
}
