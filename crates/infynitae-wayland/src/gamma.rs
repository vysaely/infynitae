//! `zwlr_gamma_control_v1` protocol implementation for night light.
//! Allows tools like wlsunset and gammastep to adjust output gamma ramps.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use infynitae_core::output::OutputId;
use smithay::reexports::rustix::fs::{OFlags, SeekFrom, fcntl_setfl, seek};
use smithay::reexports::rustix::io::read;
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId, ObjectId};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
pub use wayland_protocols_wlr::gamma_control::v1::server::{
    zwlr_gamma_control_manager_v1::{self, ZwlrGammaControlManagerV1},
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

use crate::WaylandState;

/// Error returned when setting output gamma fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GammaSetError;

/// Backend interface for querying and setting gamma ramps.
pub trait GammaHandler: Send + Sync {
    /// Returns the gamma ramp size for this output, or None if gamma tables are unsupported.
    fn gamma_size(&self, output: OutputId) -> Option<u32>;

    /// Sets the gamma table for this output (red, green, blue ramps).
    fn set_gamma(
        &mut self,
        output: OutputId,
        red: &[u16],
        green: &[u16],
        blue: &[u16],
    ) -> Result<(), GammaSetError>;

    /// Restores the output's original gamma table.
    fn reset_gamma(&mut self, output: OutputId);
}

/// Metadata stored on each `ZwlrGammaControlV1` resource.
pub struct GammaControlUserData {
    pub output_id: Option<OutputId>,
    pub active: AtomicBool,
}

/// Global gamma control state held by `WaylandState`.
pub struct GammaState {
    pub global: GlobalId,
    pub(crate) handler: Option<Arc<Mutex<dyn GammaHandler>>>,
    pub(crate) active_controls: HashMap<OutputId, ObjectId>,
}

impl GammaState {
    pub fn new(dh: &DisplayHandle, gate: crate::trust::Gate) -> Self {
        let global = dh
            .create_global::<WaylandState, ZwlrGammaControlManagerV1, crate::trust::Gate>(1, gate);
        Self {
            global,
            handler: None,
            active_controls: HashMap::new(),
        }
    }

    pub fn set_handler(&mut self, handler: Arc<Mutex<dyn GammaHandler>>) {
        self.handler = Some(handler);
    }
}

/// Reads gamma ramps from the file descriptor without blocking and validates total size.
fn read_gamma_table(fd: &OwnedFd, gamma_size: u32) -> Result<Vec<u16>, ()> {
    let expected_u16s = 3 * (gamma_size as usize);
    let expected_bytes = expected_u16s * std::mem::size_of::<u16>();

    // Ensure non-blocking read
    let _ = fcntl_setfl(fd, OFlags::NONBLOCK);
    let _ = seek(fd, SeekFrom::Start(0));

    let mut buf = vec![0u8; expected_bytes + 1];
    let mut total_read = 0;
    while total_read < buf.len() {
        match read(fd, &mut buf[total_read..]) {
            Ok(0) => break,
            Ok(n) => total_read += n,
            Err(smithay::reexports::rustix::io::Errno::INTR) => continue,
            Err(smithay::reexports::rustix::io::Errno::AGAIN) => break,
            Err(_) => return Err(()),
        }
    }

    if total_read != expected_bytes {
        return Err(());
    }

    let mut u16_ramps = Vec::with_capacity(expected_u16s);
    for &chunk in buf[..expected_bytes].as_chunks::<2>().0 {
        u16_ramps.push(u16::from_ne_bytes(chunk));
    }
    Ok(u16_ramps)
}

// -----------------------------------------------------------------------------
// Manager Dispatch
// -----------------------------------------------------------------------------

impl GlobalDispatch<ZwlrGammaControlManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrGammaControlManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrGammaControlManagerV1,
        request: zwlr_gamma_control_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_gamma_control_manager_v1::Request::GetGammaControl { id, output } => {
                let Some(output_id) = state.output_id_from_wl(&output) else {
                    let control = data_init.init(
                        id,
                        GammaControlUserData {
                            output_id: None,
                            active: AtomicBool::new(false),
                        },
                    );
                    control.failed();
                    return;
                };

                let gamma_size = state
                    .gamma
                    .handler
                    .as_ref()
                    .and_then(|h| h.lock().unwrap().gamma_size(output_id));

                let Some(size) = gamma_size.filter(|&s| s > 0) else {
                    let control = data_init.init(
                        id,
                        GammaControlUserData {
                            output_id: Some(output_id),
                            active: AtomicBool::new(false),
                        },
                    );
                    control.failed();
                    return;
                };

                // Per output, at most one gamma control (exclusive access)
                if state.gamma.active_controls.contains_key(&output_id) {
                    let control = data_init.init(
                        id,
                        GammaControlUserData {
                            output_id: Some(output_id),
                            active: AtomicBool::new(false),
                        },
                    );
                    control.failed();
                    return;
                }

                let control = data_init.init(
                    id,
                    GammaControlUserData {
                        output_id: Some(output_id),
                        active: AtomicBool::new(true),
                    },
                );
                state.gamma.active_controls.insert(output_id, control.id());
                control.gamma_size(size);
            }
            zwlr_gamma_control_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Gamma Control Dispatch
// -----------------------------------------------------------------------------

impl Dispatch<ZwlrGammaControlV1, GammaControlUserData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        control: &ZwlrGammaControlV1,
        request: zwlr_gamma_control_v1::Request,
        data: &GammaControlUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_gamma_control_v1::Request::SetGamma { fd } => {
                let Some(output_id) = data.output_id else {
                    control.failed();
                    return;
                };

                if !data.active.load(Ordering::SeqCst)
                    || state.gamma.active_controls.get(&output_id) != Some(&control.id())
                {
                    control.failed();
                    return;
                }

                let size = match state
                    .gamma
                    .handler
                    .as_ref()
                    .and_then(|h| h.lock().unwrap().gamma_size(output_id))
                {
                    Some(s) if s > 0 => s,
                    _ => {
                        control.failed();
                        return;
                    }
                };

                let ramps = match read_gamma_table(&fd, size) {
                    Ok(r) => r,
                    Err(()) => {
                        control.post_error(
                            zwlr_gamma_control_v1::Error::InvalidGamma,
                            "invalid gamma table size",
                        );
                        return;
                    }
                };

                let sz = size as usize;
                let red = &ramps[0..sz];
                let green = &ramps[sz..2 * sz];
                let blue = &ramps[2 * sz..3 * sz];

                let res = if let Some(h) = state.gamma.handler.as_ref() {
                    h.lock().unwrap().set_gamma(output_id, red, green, blue)
                } else {
                    Err(GammaSetError)
                };

                if res.is_err() {
                    control.failed();
                }
            }
            zwlr_gamma_control_v1::Request::Destroy => {
                // Destructor request; actual state cleanup handled in `destroyed`.
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        control: &ZwlrGammaControlV1,
        data: &GammaControlUserData,
    ) {
        if let Some(output_id) = data.output_id
            && data.active.swap(false, Ordering::SeqCst)
            && state.gamma.active_controls.get(&output_id) == Some(&control.id())
        {
            state.gamma.active_controls.remove(&output_id);
            if let Some(h) = state.gamma.handler.as_ref() {
                h.lock().unwrap().reset_gamma(output_id);
            }
        }
    }
}
