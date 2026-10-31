//! `zwlr_output_power_management_v1` protocol implementation for screen on/off.
//! Allows tools like wlopm and idle daemons to power down and wake screens.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use infynitae_core::output::OutputId;
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};
pub use wayland_protocols_wlr::output_power_management::v1::server::{
    zwlr_output_power_manager_v1::{self, ZwlrOutputPowerManagerV1},
    zwlr_output_power_v1::{self, Mode, ZwlrOutputPowerV1},
};

use crate::WaylandState;

/// Output power mode (on or off).
pub type OutputPowerMode = Mode;

/// Backend interface for applying power mode changes to displays.
pub trait OutputPowerHandler: Send + Sync {
    /// Applies a power mode change (blanking/turning off or unblanking/turning on).
    fn set_mode(&mut self, output: OutputId, mode: OutputPowerMode);
}

/// Metadata stored on each `ZwlrOutputPowerV1` resource.
pub struct OutputPowerUserData {
    pub output_id: Option<OutputId>,
}

/// Global output power management state held by `WaylandState`.
pub struct OutputPowerState {
    pub global: GlobalId,
    pub(crate) handler: Option<Arc<Mutex<dyn OutputPowerHandler>>>,
    pub(crate) modes: HashMap<OutputId, OutputPowerMode>,
    pub(crate) listeners: HashMap<OutputId, Vec<ZwlrOutputPowerV1>>,
}

impl OutputPowerState {
    pub fn new(dh: &DisplayHandle, gate: crate::trust::Gate) -> Self {
        let global =
            dh.create_global::<WaylandState, ZwlrOutputPowerManagerV1, crate::trust::Gate>(1, gate);
        Self {
            global,
            handler: None,
            modes: HashMap::new(),
            listeners: HashMap::new(),
        }
    }

    pub fn set_handler(&mut self, handler: Arc<Mutex<dyn OutputPowerHandler>>) {
        self.handler = Some(handler);
    }

    /// Returns the current power mode for an output (defaults to `Mode::On`).
    pub fn mode(&self, output: OutputId) -> OutputPowerMode {
        self.modes.get(&output).copied().unwrap_or(Mode::On)
    }

    /// Sets the power mode for an output, notifies backend handler and reports mode to listeners.
    pub fn set_mode(&mut self, output: OutputId, mode: OutputPowerMode) {
        let old_mode = self.modes.insert(output, mode).unwrap_or(Mode::On);
        if old_mode != mode {
            if let Some(list) = self.listeners.get_mut(&output) {
                list.retain(|l| l.is_alive());
                for l in list {
                    l.mode(mode);
                }
            }
            if let Some(h) = self.handler.as_ref() {
                h.lock().unwrap().set_mode(output, mode);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Manager Dispatch
// -----------------------------------------------------------------------------

impl GlobalDispatch<ZwlrOutputPowerManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputPowerManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrOutputPowerManagerV1,
        request: zwlr_output_power_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_power_manager_v1::Request::GetOutputPower { id, output } => {
                let Some(output_id) = state.output_id_from_wl(&output) else {
                    let power = data_init.init(id, OutputPowerUserData { output_id: None });
                    power.failed();
                    return;
                };

                let current_mode = state.output_power.mode(output_id);
                let power = data_init.init(
                    id,
                    OutputPowerUserData {
                        output_id: Some(output_id),
                    },
                );
                // Protocol: mode event sent immediately on creation
                power.mode(current_mode);
                state
                    .output_power
                    .listeners
                    .entry(output_id)
                    .or_default()
                    .push(power);
            }
            zwlr_output_power_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

// -----------------------------------------------------------------------------
// Output Power Dispatch
// -----------------------------------------------------------------------------

impl Dispatch<ZwlrOutputPowerV1, OutputPowerUserData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        power: &ZwlrOutputPowerV1,
        request: zwlr_output_power_v1::Request,
        data: &OutputPowerUserData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_power_v1::Request::SetMode { mode } => {
                let Some(output_id) = data.output_id else {
                    power.failed();
                    return;
                };

                let power_mode = match mode {
                    WEnum::Value(Mode::Off) => Mode::Off,
                    WEnum::Value(Mode::On) => Mode::On,
                    _ => {
                        power.post_error(
                            zwlr_output_power_v1::Error::InvalidMode,
                            "invalid output power mode",
                        );
                        return;
                    }
                };

                state.output_power.set_mode(output_id, power_mode);
            }
            zwlr_output_power_v1::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        power: &ZwlrOutputPowerV1,
        data: &OutputPowerUserData,
    ) {
        if let Some(output_id) = data.output_id {
            let pid = power.id();
            if let Some(list) = state.output_power.listeners.get_mut(&output_id) {
                list.retain(|l| l.id() != pid);
            }
        }
    }
}
