//! `zwlr_screencopy_v1` protocol implementation for screen capture.
//! Allows xdg-desktop-portal-wlr to capture output frames for Discord, OBS, WebRTC.

use std::sync::{Arc, Mutex};
use infynitae_core::output::OutputId;
pub use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, Flags, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use smithay::reexports::wayland_server::{
    backend::GlobalId,
    protocol::{wl_buffer::WlBuffer, wl_shm},
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::WaylandState;

/// Pending screencopy capture request awaiting the next rendered frame.
#[derive(Debug)]
pub struct PendingFrame {
    pub frame: ZwlrScreencopyFrameV1,
    pub output_id: OutputId,
    pub buffer: WlBuffer,
    pub overlay_cursor: bool,
    pub with_damage: bool,
    pub region: Option<(i32, i32, i32, i32)>,
}

/// Metadata stored on each `ZwlrScreencopyFrameV1` resource.
pub struct FrameUserData {
    pub output_id: Option<OutputId>,
    pub overlay_cursor: bool,
    pub region: Option<(i32, i32, i32, i32)>,
}

/// Global Screencopy state held by `WaylandState`.
pub struct ScreencopyState {
    pub global: GlobalId,
    pub pending: Arc<Mutex<Vec<PendingFrame>>>,
    pub hide_cursor: std::sync::atomic::AtomicBool,
}

impl ScreencopyState {
    pub fn new(dh: &DisplayHandle) -> Self {
        let global = dh.create_global::<WaylandState, ZwlrScreencopyManagerV1, ()>(3, ());
        let mut hide = std::env::var("SCREENCOPY_CURSOR")
            .map(|v| matches!(v.to_lowercase().as_str(), "0" | "false" | "off" | "no"))
            .unwrap_or(false);
        if let Ok(home) = std::env::var("HOME") {
            let config_path = format!("{home}/.config/infynitae/config");
            if let Ok(content) = std::fs::read_to_string(config_path) {
                for line in content.lines() {
                    let line_clean = line.split('#').next().unwrap_or("").trim();
                    if let Some((k, v)) = line_clean.split_once('=') {
                        if k.trim() == "SCREENCOPY_CURSOR" {
                            let mode = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                            hide = matches!(mode.to_lowercase().as_str(), "0" | "false" | "off" | "no");
                        }
                    }
                }
            }
        }
        Self {
            global,
            pending: Arc::new(Mutex::new(Vec::new())),
            hide_cursor: std::sync::atomic::AtomicBool::new(hide),
        }
    }

    pub fn hide_cursor(&self) -> bool {
        self.hide_cursor.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_hide_cursor(&self, hide: bool) {
        self.hide_cursor.store(hide, std::sync::atomic::Ordering::Relaxed);
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

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
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
                    let frame = data_init.init(frame, FrameUserData {
                        output_id: None,
                        overlay_cursor: overlay_cursor != 0,
                        region: None,
                    });
                    frame.failed();
                    return;
                };

                let Some(size) = state.output_mode_size(output_id) else {
                    let frame = data_init.init(frame, FrameUserData {
                        output_id: Some(output_id),
                        overlay_cursor: overlay_cursor != 0,
                        region: None,
                    });
                    frame.failed();
                    return;
                };

                let frame = data_init.init(frame, FrameUserData {
                    output_id: Some(output_id),
                    overlay_cursor: overlay_cursor != 0,
                    region: None,
                });

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
                    let frame = data_init.init(frame, FrameUserData {
                        output_id: None,
                        overlay_cursor: overlay_cursor != 0,
                        region: None,
                    });
                    frame.failed();
                    return;
                };

                let Some(size) = state.output_mode_size(output_id) else {
                    let frame = data_init.init(frame, FrameUserData {
                        output_id: Some(output_id),
                        overlay_cursor: overlay_cursor != 0,
                        region: None,
                    });
                    frame.failed();
                    return;
                };

                let w = width.clamp(1, size.0);
                let h = height.clamp(1, size.1);

                let frame = data_init.init(frame, FrameUserData {
                    output_id: Some(output_id),
                    overlay_cursor: overlay_cursor != 0,
                    region: Some((x, y, w, h)),
                });

                if frame.version() >= 3 {
                    frame.linux_dmabuf(875713112, w as u32, h as u32);
                    frame.linux_dmabuf(875713089, w as u32, h as u32);
                }
                frame.buffer(wl_shm::Format::Xrgb8888, w as u32, h as u32, (w * 4) as u32);
                if frame.version() >= 3 {
                    frame.buffer(wl_shm::Format::Argb8888, w as u32, h as u32, (w * 4) as u32);
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
        _client: &Client,
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
                state.screencopy.pending.lock().unwrap().push(PendingFrame {
                    frame: frame.clone(),
                    output_id,
                    buffer,
                    overlay_cursor: data.overlay_cursor,
                    with_damage: false,
                    region: data.region,
                });
            }
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                state.screencopy.pending.lock().unwrap().push(PendingFrame {
                    frame: frame.clone(),
                    output_id,
                    buffer,
                    overlay_cursor: data.overlay_cursor,
                    with_damage: true,
                    region: data.region,
                });
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {
                let id = frame.id();
                state
                    .screencopy
                    .pending
                    .lock()
                    .unwrap()
                    .retain(|p| p.frame.id() != id);
            }
            _ => {}
        }
    }
}
