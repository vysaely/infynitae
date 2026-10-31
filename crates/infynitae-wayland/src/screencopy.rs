//! `zwlr_screencopy_v1` protocol implementation for screen capture.
//! Allows xdg-desktop-portal-wlr to capture output frames for Discord, OBS, WebRTC.
//!
//! A capture that may wait for a change (`copy_with_damage`, and every ext-image-copy frame) is
//! served only once the screen differs from that consumer's last capture, with what changed as
//! its damage ([`CaptureDamage`]); until then it waits ([`ScreencopyState::defer`]) and costs
//! nothing. A still screen shared over a remote desktop is drawn once, not every frame. A plain
//! `copy` is served at once.

use infynitae_core::output::OutputId;
pub use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, Flags, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::GlobalId,
    protocol::{wl_buffer::WlBuffer, wl_shm},
};
use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_server::backend::{ClientId, ObjectId};

use crate::WaylandState;

/// Who a capture is for, across its frames: an ext-image-copy session, or a wlr-screencopy
/// client capturing one output (and region) with damage.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Consumer {
    Session(ObjectId),
    Client(ClientId, OutputId, Option<(i32, i32, i32, i32)>),
}

pub use crate::convert::CaptureDamage;

/// The protocol object a capture answers: `zwlr_screencopy_frame_v1`, or
/// `ext_image_copy_capture_frame_v1` (`image_copy`).
#[derive(Debug, Clone)]
pub enum CaptureFrame {
    Wlr(ZwlrScreencopyFrameV1),
    Ext(crate::image_copy::ExtImageCopyCaptureFrameV1),
}

/// Pending capture request (either protocol) awaiting the next rendered frame.
#[derive(Debug)]
pub struct PendingFrame {
    pub frame: CaptureFrame,
    pub output_id: OutputId,
    pub buffer: WlBuffer,
    pub overlay_cursor: bool,
    pub with_damage: bool,
    pub region: Option<(i32, i32, i32, i32)>,
    /// A window captured alone (window sharing) instead of the output: drawn from
    /// `Core::window_frame`, the output only says which screen's loop draws it.
    pub window: Option<infynitae_core::stage::ElementId>,
    /// The screen's rotation, which the buffer is drawn through (in the panel's orientation, as
    /// wlroots does; `Normal` for a window).
    pub transform: infynitae_core::output::Transform,
    /// Whose capture it is, when it may wait for a change (`CaptureDamage`); `None`: at once.
    pub consumer: Option<Consumer>,
    /// What changed since the consumer's last capture, in buffer px; `None`: all of it.
    pub damage: Option<Vec<(i32, i32, i32, i32)>>,
}

impl PendingFrame {
    /// The capture could not be made.
    pub fn fail(&self) {
        match &self.frame {
            CaptureFrame::Wlr(f) => f.failed(),
            CaptureFrame::Ext(f) => {
                f.failed(crate::image_copy::FailureReason::Unknown);
            }
        }
    }

    /// The output was removed / stopped.
    pub fn fail_stopped(&self) {
        match &self.frame {
            CaptureFrame::Wlr(f) => f.failed(),
            CaptureFrame::Ext(f) => {
                f.failed(crate::image_copy::FailureReason::Stopped);
            }
        }
    }

    /// The buffer holds the frame: its `size`, presented at `time` (CLOCK_MONOTONIC); the
    /// damage is [`PendingFrame::damage`], or all of it.
    pub fn ready(&self, size: (u32, u32), time: std::time::Duration) {
        let whole = [(0, 0, size.0 as i32, size.1 as i32)];
        let damage = self.damage.as_deref().unwrap_or(&whole);
        let (hi, lo, ns) = (
            (time.as_secs() >> 32) as u32,
            (time.as_secs() & 0xFFFF_FFFF) as u32,
            time.subsec_nanos(),
        );
        match &self.frame {
            CaptureFrame::Wlr(f) => {
                if self.with_damage && f.version() >= 2 {
                    for &(x, y, w, h) in damage {
                        f.damage(x.max(0) as u32, y.max(0) as u32, w as u32, h as u32);
                    }
                }
                f.flags(Flags::empty());
                f.ready(hi, lo, ns);
            }
            CaptureFrame::Ext(f) => {
                f.transform(crate::convert::wl_transform(self.transform));
                for &(x, y, w, h) in damage {
                    f.damage(x, y, w, h);
                }
                f.presentation_time(hi, lo, ns);
                f.ready();
            }
        }
    }

    /// The protocol object's id (to drop a destroyed frame from the queue).
    pub fn id(&self) -> smithay::reexports::wayland_server::backend::ObjectId {
        match &self.frame {
            CaptureFrame::Wlr(f) => f.id(),
            CaptureFrame::Ext(f) => f.id(),
        }
    }
}

/// Metadata stored on each `ZwlrScreencopyFrameV1` resource.
pub struct FrameUserData {
    pub output_id: Option<OutputId>,
    pub overlay_cursor: bool,
    pub region: Option<(i32, i32, i32, i32)>,
    pub used: std::sync::atomic::AtomicBool,
}

/// Global Screencopy state held by `WaylandState`.
pub struct ScreencopyState {
    pub global: GlobalId,
    pub pending: Arc<Mutex<Vec<PendingFrame>>>,
    /// Captures waiting for the screen to change (back in `pending` at its next redraw).
    waiting: Mutex<Vec<PendingFrame>>,
    /// What changed for each consumer since its last capture.
    pub damage: Mutex<CaptureDamage>,
    pub hide_cursor: std::sync::atomic::AtomicBool,
    pub serves_capture: std::sync::atomic::AtomicBool,
}

impl ScreencopyState {
    pub fn new(dh: &DisplayHandle, gate: crate::trust::Gate) -> Self {
        let global =
            dh.create_global::<WaylandState, ZwlrScreencopyManagerV1, crate::trust::Gate>(3, gate);
        let mut hide = std::env::var("SCREENCOPY_CURSOR")
            .map(|v| matches!(v.to_lowercase().as_str(), "0" | "false" | "off" | "no"))
            .unwrap_or(false);
        if let Ok(home) = std::env::var("HOME") {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                for (k, v) in content.lines().filter_map(crate::config_line) {
                    if k.trim() == "SCREENCOPY_CURSOR" {
                        let mode = v.trim();
                        hide = matches!(mode.to_lowercase().as_str(), "0" | "false" | "off" | "no");
                    }
                }
            }
        }
        Self {
            global,
            pending: Arc::new(Mutex::new(Vec::new())),
            waiting: Mutex::new(Vec::new()),
            damage: Mutex::new(CaptureDamage::default()),
            hide_cursor: std::sync::atomic::AtomicBool::new(hide),
            serves_capture: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn serves_capture(&self) -> bool {
        self.serves_capture
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn enable_capture(&self) {
        self.serves_capture
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn hide_cursor(&self) -> bool {
        self.hide_cursor.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_hide_cursor(&self, hide: bool) {
        self.hide_cursor
            .store(hide, std::sync::atomic::Ordering::Relaxed);
    }

    /// Captures that found the screen unchanged: they wait for its next redraw
    /// ([`wake_waiting`](Self::wake_waiting)), which does not come by itself.
    pub fn defer(&self, frames: Vec<PendingFrame>) {
        self.waiting.lock().unwrap().extend(frames);
    }

    /// The screen `output_id` redraws: its waiting captures look again.
    pub fn wake_waiting(&self, output_id: OutputId) {
        let mut waiting = self.waiting.lock().unwrap();
        let mut pending = self.pending.lock().unwrap();
        let mut i = 0;
        while i < waiting.len() {
            if waiting[i].output_id == output_id {
                pending.push(waiting.swap_remove(i));
            } else {
                i += 1;
            }
        }
    }

    /// Forgets a destroyed frame, served or waiting.
    pub fn forget(&self, id: &ObjectId) {
        self.pending.lock().unwrap().retain(|p| p.id() != *id);
        self.waiting.lock().unwrap().retain(|p| p.id() != *id);
    }

    /// Takes all pending requests for a specific output to fulfill during rendering.
    pub fn take_pending(&self, output_id: OutputId) -> Vec<PendingFrame> {
        let mut list = self.pending.lock().unwrap();
        let mut taken = Vec::new();
        let mut i = 0;
        while i < list.len() {
            if list[i].output_id == output_id {
                taken.push(list.swap_remove(i));
            } else {
                i += 1;
            }
        }
        taken
    }

    /// Fails and removes all pending capture requests for a specific output when it disappears.
    pub fn fail_pending_output(&self, output_id: OutputId) {
        self.wake_waiting(output_id);
        let mut list = self.pending.lock().unwrap();
        let mut i = 0;
        while i < list.len() {
            if list[i].output_id == output_id {
                let frame = list.swap_remove(i);
                frame.fail_stopped();
            } else {
                i += 1;
            }
        }
    }

    /// Whether any capture requests are waiting for `output_id`.
    pub fn has_pending(&self, output_id: OutputId) -> bool {
        self.pending
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.output_id == output_id)
    }
}

// -----------------------------------------------------------------------------
// Manager Dispatch
// -----------------------------------------------------------------------------

impl GlobalDispatch<ZwlrScreencopyManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => {
                let Some(output_id) = state.output_id_from_wl(&output) else {
                    let frame = data_init.init(
                        frame,
                        FrameUserData {
                            output_id: None,
                            overlay_cursor: overlay_cursor != 0,
                            region: None,
                            used: std::sync::atomic::AtomicBool::new(false),
                        },
                    );
                    frame.failed();
                    return;
                };

                let Some(size) = state.output_mode_size(output_id) else {
                    let frame = data_init.init(
                        frame,
                        FrameUserData {
                            output_id: Some(output_id),
                            overlay_cursor: overlay_cursor != 0,
                            region: None,
                            used: std::sync::atomic::AtomicBool::new(false),
                        },
                    );
                    frame.failed();
                    return;
                };

                let frame = data_init.init(
                    frame,
                    FrameUserData {
                        output_id: Some(output_id),
                        overlay_cursor: overlay_cursor != 0,
                        region: None,
                        used: std::sync::atomic::AtomicBool::new(false),
                    },
                );

                // Advertise standard ARGB8888 and XRGB8888 formats
                if frame.version() >= 3 {
                    frame.linux_dmabuf(875713112, size.0 as u32, size.1 as u32);
                    frame.linux_dmabuf(875713089, size.0 as u32, size.1 as u32);
                }
                frame.buffer(
                    wl_shm::Format::Xrgb8888,
                    size.0 as u32,
                    size.1 as u32,
                    (size.0 * 4) as u32,
                );
                if frame.version() >= 3 {
                    frame.buffer(
                        wl_shm::Format::Argb8888,
                        size.0 as u32,
                        size.1 as u32,
                        (size.0 * 4) as u32,
                    );
                    frame.buffer_done();
                }
            }
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => {
                let Some(output_id) = state.output_id_from_wl(&output) else {
                    let frame = data_init.init(
                        frame,
                        FrameUserData {
                            output_id: None,
                            overlay_cursor: overlay_cursor != 0,
                            region: None,
                            used: std::sync::atomic::AtomicBool::new(false),
                        },
                    );
                    frame.failed();
                    return;
                };

                let Some(size) = state.output_mode_size(output_id) else {
                    let frame = data_init.init(
                        frame,
                        FrameUserData {
                            output_id: Some(output_id),
                            overlay_cursor: overlay_cursor != 0,
                            region: None,
                            used: std::sync::atomic::AtomicBool::new(false),
                        },
                    );
                    frame.failed();
                    return;
                };

                // the region is in what the screen shows; its buffer in the panel's orientation
                let transform = state
                    .core
                    .output(output_id)
                    .map_or(infynitae_core::output::Transform::Normal, |m| m.transform);
                let view = crate::convert::capture_buffer_size(size, transform);
                let w = width.clamp(1, view.0);
                let h = height.clamp(1, view.1);
                let (bw, bh) = crate::convert::capture_buffer_size((w, h), transform);

                let frame = data_init.init(
                    frame,
                    FrameUserData {
                        output_id: Some(output_id),
                        overlay_cursor: overlay_cursor != 0,
                        region: Some((x, y, w, h)),
                        used: std::sync::atomic::AtomicBool::new(false),
                    },
                );

                if frame.version() >= 3 {
                    frame.linux_dmabuf(875713112, bw as u32, bh as u32);
                    frame.linux_dmabuf(875713089, bw as u32, bh as u32);
                }
                frame.buffer(
                    wl_shm::Format::Xrgb8888,
                    bw as u32,
                    bh as u32,
                    (bw * 4) as u32,
                );
                if frame.version() >= 3 {
                    frame.buffer(
                        wl_shm::Format::Argb8888,
                        bw as u32,
                        bh as u32,
                        (bw * 4) as u32,
                    );
                    frame.buffer_done();
                }
            }
            zwlr_screencopy_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Frame Dispatch
// -----------------------------------------------------------------------------

impl Dispatch<ZwlrScreencopyFrameV1, FrameUserData> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        frame: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &FrameUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(output_id) = data.output_id else {
            frame.failed();
            return;
        };
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => {
                if data.used.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    frame.post_error(
                        zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                        "the frame has already been used to copy a wl_buffer",
                    );
                    return;
                }
                if !state.screencopy.serves_capture()
                    || !state.outputs.iter().any(|o| o.id == output_id)
                {
                    frame.failed();
                    return;
                }
                state.note_capture(client);
                state.screencopy.pending.lock().unwrap().push(PendingFrame {
                    frame: CaptureFrame::Wlr(frame.clone()),
                    output_id,
                    buffer,
                    overlay_cursor: data.overlay_cursor,
                    with_damage: false,
                    region: data.region,
                    window: None,
                    transform: state
                        .core
                        .output(output_id)
                        .map_or(infynitae_core::output::Transform::Normal, |m| m.transform),
                    consumer: None,
                    damage: None,
                });
            }
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                if data.used.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    frame.post_error(
                        zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                        "the frame has already been used to copy a wl_buffer",
                    );
                    return;
                }
                if !state.screencopy.serves_capture()
                    || !state.outputs.iter().any(|o| o.id == output_id)
                {
                    frame.failed();
                    return;
                }
                state.note_capture(client);
                state.screencopy.pending.lock().unwrap().push(PendingFrame {
                    frame: CaptureFrame::Wlr(frame.clone()),
                    output_id,
                    buffer,
                    overlay_cursor: data.overlay_cursor,
                    with_damage: true,
                    region: data.region,
                    window: None,
                    transform: state
                        .core
                        .output(output_id)
                        .map_or(infynitae_core::output::Transform::Normal, |m| m.transform),
                    consumer: Some(Consumer::Client(client.id(), output_id, data.region)),
                    damage: None,
                });
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {
                state.screencopy.forget(&frame.id());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::wayland_server::Display;

    #[test]
    fn serves_capture_defaults_to_false_and_enables() {
        let display: Display<WaylandState> = Display::new().unwrap();
        let state = ScreencopyState::new(
            &display.handle(),
            crate::trust::SharedPolicy::default().gate(crate::trust::Capability::Capture),
        );
        assert!(!state.serves_capture());
        state.enable_capture();
        assert!(state.serves_capture());
    }

    #[test]
    fn frame_used_atomic_rejects_second_use() {
        let used = std::sync::atomic::AtomicBool::new(false);
        assert!(!used.swap(true, std::sync::atomic::Ordering::SeqCst));
        assert!(used.swap(true, std::sync::atomic::Ordering::SeqCst));
    }
}
