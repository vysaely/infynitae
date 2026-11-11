//! `zwlr_output_manager_v1`: display settings tools (wlr-randr, wdisplays, kanshi) read the
//! screens and change their mode, scale, position, rotation and VRR.
//!
//! A configuration is checked here against what the backend reported for each screen
//! ([`HeadInfo`]: its modes, its size in mm, its VRR state), then handed to the backend
//! ([`WaylandState::take_output_configurations`]): the backend owns the output lifecycle token
//! and the hardware, sets the modes, hands Core the new descriptors and positions
//! ([`WaylandState::output_change_plan`]), and reports the outcome
//! ([`WaylandState::finish_output_configuration`]). Everything changes or nothing does.
//!
//! VRR as a tool sees it is whether the screen may use it: on, or the backend's automatic mode
//! (while a window is fullscreen); off is off. Turning a screen off or on leaves or restores
//! the screen in layout and Core, through the same events as unplug/replug.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use infynitae_core::output::{OutputDescriptor, OutputId, OutputKey, OutputModel, Transform};
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_output;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};
use wayland_protocols_wlr::output_management::v1::server::{
    zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};

use crate::WaylandState;

/// One display mode of a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadMode {
    pub size: (i32, i32),
    pub refresh_mhz: i32,
    pub preferred: bool,
}

/// What the backend knows about a screen beyond Core's model. Without it (a headless output)
/// the screen has one mode, its current one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadInfo {
    pub modes: Vec<HeadMode>,
    /// Width and height in mm.
    pub physical_size: Option<(i32, i32)>,
    /// `None`: the screen has no VRR; else whether it is on.
    pub adaptive_sync: Option<bool>,
}

/// One screen's part of a configuration, checked: only what changes is set.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadChange {
    pub output: OutputId,
    /// One of the screen's modes.
    pub mode: Option<HeadMode>,
    pub position: Option<(i32, i32)>,
    pub scale_120: Option<u32>,
    /// A rotation or flip (the backend draws through it, Core swaps the view for 90° and 270°).
    pub transform: Option<Transform>,
    /// VRR allowed or off, on a screen that has it.
    pub adaptive_sync: Option<bool>,
}

/// One screen's part of a configuration as Core takes it ([`WaylandState::output_change_plan`]).
pub struct OutputChangePlan {
    pub output: OutputId,
    /// The new descriptor, for `output_reconfigured`.
    pub descriptor: Option<OutputDescriptor>,
    /// The new position, for `output_arranged`.
    pub position: Option<(i32, i32)>,
}

/// A screen being enabled from a disabled state.
#[derive(Clone, Debug)]
pub struct HeadOn {
    pub output: OutputId,
    pub descriptor: OutputDescriptor,
    pub position: (i32, i32),
    pub adaptive_sync: Option<bool>,
}

/// A configuration the client applied, for the backend to carry out.
pub struct PendingConfiguration {
    pub(crate) resource: ZwlrOutputConfigurationV1,
    pub changes: Vec<HeadChange>,
    pub off: Vec<OutputId>,
    pub on: Vec<HeadOn>,
}

/// A head as a client configures it, before it is checked.
#[derive(Clone, Debug, Default)]
struct HeadRequest {
    mode: Option<RequestedMode>,
    position: Option<(i32, i32)>,
    transform: Option<Transform>,
    scale: Option<f64>,
    adaptive_sync: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
enum RequestedMode {
    Listed(HeadMode),
    Custom { size: (i32, i32), refresh_mhz: i32 },
}

/// A configuration object's state.
pub struct ConfigurationData {
    serial: u32,
    inner: Mutex<ConfigurationInner>,
}

#[derive(Default)]
struct ConfigurationInner {
    used: bool,
    heads: HashMap<OutputId, HeadRequest>,
    disabled: Vec<OutputId>,
}

pub struct HeadData {
    pub(crate) output: Arc<Mutex<OutputId>>,
}

pub struct ModeData {
    pub(crate) output: Arc<Mutex<OutputId>>,
    pub(crate) mode: HeadMode,
}

pub struct ConfigurationHeadData {
    /// `None` for a head that was refused (configured twice, or the configuration used).
    output: Option<OutputId>,
    config: ObjectId,
    set: AtomicU32,
}

const SET_MODE: u32 = 1 << 0;
const SET_POSITION: u32 = 1 << 1;
const SET_TRANSFORM: u32 = 1 << 2;
const SET_SCALE: u32 = 1 << 3;
const SET_ADAPTIVE_SYNC: u32 = 1 << 4;

/// A screen turned off through output management, kept so display tools can enable it again.
#[derive(Clone, Debug)]
pub struct DisabledHead {
    pub key: OutputKey,
    pub descriptor: OutputDescriptor,
    pub position: (i32, i32),
    pub scale_120: u32,
    pub transform: Transform,
    pub info: HeadInfo,
}

/// What one client's head was last told.
struct Head {
    output: Arc<Mutex<OutputId>>,
    manager: ObjectId,
    head: ZwlrOutputHeadV1,
    modes: Vec<(HeadMode, ZwlrOutputModeV1)>,
    told: Told,
}

#[derive(Clone, Debug, PartialEq)]
struct Told {
    enabled: bool,
    mode: Option<HeadMode>,
    position: (i32, i32),
    transform: Transform,
    scale_120: u32,
    adaptive_sync: Option<bool>,
}

pub struct OutputManagementState {
    #[expect(
        dead_code,
        reason = "owns the zwlr_output_manager_v1 global for the state's lifetime"
    )]
    global: smithay::reexports::wayland_server::backend::GlobalId,
    serial: u32,
    managers: Vec<ZwlrOutputManagerV1>,
    heads: Vec<Head>,
    pub(crate) info: HashMap<OutputId, HeadInfo>,
    pub(crate) disabled_screens: HashMap<OutputId, DisabledHead>,
    /// Configuration objects alive, by id, for their heads to reach.
    configurations: HashMap<ObjectId, ZwlrOutputConfigurationV1>,
    pending: Vec<PendingConfiguration>,
}

impl OutputManagementState {
    pub fn new(dh: &DisplayHandle, gate: crate::trust::Gate) -> Self {
        Self {
            global: dh
                .create_global::<WaylandState, ZwlrOutputManagerV1, crate::trust::Gate>(4, gate),
            serial: 1,
            managers: Vec::new(),
            heads: Vec::new(),
            info: HashMap::new(),
            disabled_screens: HashMap::new(),
            configurations: HashMap::new(),
            pending: Vec::new(),
        }
    }
}

fn transform_to_wl(t: Transform) -> wl_output::Transform {
    crate::convert::wl_transform(t)
}

fn transform_from_wl(t: wl_output::Transform) -> Option<Transform> {
    Some(match t {
        wl_output::Transform::Normal => Transform::Normal,
        wl_output::Transform::_90 => Transform::R90,
        wl_output::Transform::_180 => Transform::R180,
        wl_output::Transform::_270 => Transform::R270,
        wl_output::Transform::Flipped => Transform::Flipped,
        wl_output::Transform::Flipped90 => Transform::Flipped90,
        wl_output::Transform::Flipped180 => Transform::Flipped180,
        wl_output::Transform::Flipped270 => Transform::Flipped270,
        _ => return None,
    })
}

fn position_of(model: &OutputModel) -> (i32, i32) {
    (
        model.arrangement.0.round() as i32,
        model.arrangement.1.round() as i32,
    )
}

fn scale_120_of(model: &OutputModel) -> u32 {
    (model.scale * 120.0).round() as u32
}

impl WaylandState {
    /// The backend's facts about a screen: its modes, its size in mm, its VRR state. Set when
    /// the screen connects and whenever its VRR state changes.
    pub fn set_output_head_info(&mut self, output: OutputId, info: HeadInfo) {
        self.output_management.info.insert(output, info);
        self.output_heads_changed(output);
    }

    /// The configurations clients applied since the last call, checked, for the backend to
    /// carry out (each with [`WaylandState::output_change_plan`] and
    /// [`WaylandState::finish_output_configuration`]).
    pub fn take_output_configurations(&mut self) -> Vec<PendingConfiguration> {
        std::mem::take(&mut self.output_management.pending)
    }

    /// What Core must take for `changes`, once the backend set the modes: per screen, its new
    /// descriptor (a new mode or scale) and its new position. The backend hands them to Core
    /// with its output lifecycle token. `None` if Core would refuse one: then nothing is changed.
    pub fn output_change_plan(&self, changes: &[HeadChange]) -> Option<Vec<OutputChangePlan>> {
        let mut plan = Vec::with_capacity(changes.len());
        for c in changes {
            let model = self.core.output(c.output)?;
            let mut desc = OutputDescriptor {
                key: model.key.clone(),
                mode: model.phys,
                transform: model.transform,
                scale_120: scale_120_of(model),
                refresh_mhz: model.refresh_mhz,
            };
            if let Some(m) = c.mode {
                desc.mode = (m.size.0 as u32, m.size.1 as u32);
                desc.refresh_mhz = m.refresh_mhz as u32;
            }
            if let Some(s) = c.scale_120 {
                desc.scale_120 = s;
            }
            if let Some(t) = c.transform {
                desc.transform = t;
            }
            if !infynitae_core::output::valid(&desc) {
                return None;
            }
            let reconfigure = c.mode.is_some() || c.scale_120.is_some() || c.transform.is_some();
            plan.push(OutputChangePlan {
                output: c.output,
                descriptor: reconfigure.then_some(desc),
                position: c.position,
            });
        }
        Some(plan)
    }

    /// Re-links a previously disabled head to its newly assigned `OutputId`.
    pub fn output_management_reconnect_head(
        &mut self,
        old_id: OutputId,
        new_id: OutputId,
        adaptive_sync: Option<bool>,
    ) {
        if let Some(disabled) = self.output_management.disabled_screens.remove(&old_id) {
            let mut info = disabled.info;
            if let Some(sync) = adaptive_sync {
                info.adaptive_sync = Some(sync);
            }
            self.output_management.info.insert(new_id, info);
        }
        for h in &self.output_management.heads {
            let mut out = h.output.lock().unwrap();
            if *out == old_id {
                *out = new_id;
            }
        }
    }

    /// Tells the client how its configuration went; a VRR change it made is reported to every
    /// tool.
    pub fn finish_output_configuration(&mut self, pending: PendingConfiguration, ok: bool) {
        if !ok {
            pending.resource.failed();
            for id in pending.off {
                self.output_management.disabled_screens.remove(&id);
            }
            return;
        }
        pending.resource.succeeded();
        for c in &pending.changes {
            if let Some(on) = c.adaptive_sync
                && let Some(info) = self.output_management.info.get_mut(&c.output)
            {
                info.adaptive_sync = Some(on);
                self.output_heads_changed(c.output);
            }
        }
    }

    /// A screen connected, changed or left: every manager's heads follow, with a new serial.
    pub(crate) fn output_heads_changed(&mut self, output: OutputId) {
        let om = &mut self.output_management;
        om.managers.retain(|m| m.is_alive());
        om.heads.retain(|h| h.head.is_alive());
        let model = self.core.output(output).cloned();
        let mut affected: Vec<ObjectId> = Vec::new();
        match &model {
            None => {
                if om.disabled_screens.contains_key(&output) {
                    let now = Told {
                        enabled: false,
                        mode: None,
                        position: (i32::MIN, i32::MIN),
                        transform: Transform::Normal,
                        scale_120: 0,
                        adaptive_sync: None,
                    };
                    for h in om
                        .heads
                        .iter_mut()
                        .filter(|h| *h.output.lock().unwrap() == output)
                    {
                        if send_changes(h, &now) {
                            affected.push(h.manager.clone());
                        }
                    }
                } else {
                    self.output_management.heads.retain(|h| {
                        if *h.output.lock().unwrap() == output {
                            h.head.finished();
                            affected.push(h.manager.clone());
                            false
                        } else {
                            true
                        }
                    });
                }
            }
            Some(model) => {
                let managers = self.output_management.managers.clone();
                for m in &managers {
                    let known = self
                        .output_management
                        .heads
                        .iter()
                        .any(|h| *h.output.lock().unwrap() == output && h.manager == m.id());
                    if !known {
                        self.announce_head(m, model);
                        affected.push(m.id());
                    }
                }
                let now = self.head_now(model);
                for h in self
                    .output_management
                    .heads
                    .iter_mut()
                    .filter(|h| *h.output.lock().unwrap() == output)
                {
                    if send_changes(h, &now) {
                        affected.push(h.manager.clone());
                    }
                }
            }
        }
        if !affected.is_empty() {
            self.output_management.serial = self.output_management.serial.wrapping_add(1);
            let serial = self.output_management.serial;
            for m in &self.output_management.managers {
                if affected.contains(&m.id()) {
                    m.done(serial);
                }
            }
        }
    }

    fn head_modes(&self, model: &OutputModel) -> Vec<HeadMode> {
        self.output_management
            .info
            .get(&model.id)
            .map(|i| i.modes.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| {
                vec![HeadMode {
                    size: (model.phys.0 as i32, model.phys.1 as i32),
                    refresh_mhz: model.refresh_mhz as i32,
                    preferred: true,
                }]
            })
    }

    fn head_now(&self, model: &OutputModel) -> Told {
        let current = (model.phys.0 as i32, model.phys.1 as i32);
        let mode = self
            .head_modes(model)
            .into_iter()
            .find(|m| m.size == current && m.refresh_mhz == model.refresh_mhz as i32);
        Told {
            enabled: true,
            mode,
            position: position_of(model),
            transform: model.transform,
            scale_120: scale_120_of(model),
            adaptive_sync: self
                .output_management
                .info
                .get(&model.id)
                .and_then(|i| i.adaptive_sync),
        }
    }

    /// Creates the head of `model` for one manager and tells it everything.
    fn announce_head(&mut self, manager: &ZwlrOutputManagerV1, model: &OutputModel) {
        let Some(client) = manager.client() else {
            return;
        };
        let id_cell = Arc::new(Mutex::new(model.id));
        let Ok(head) = client.create_resource::<ZwlrOutputHeadV1, _, WaylandState>(
            &self.dh,
            manager.version(),
            HeadData {
                output: id_cell.clone(),
            },
        ) else {
            return;
        };
        manager.head(&head);
        let key = &model.key;
        head.name(key.connector.clone());
        let known = |s: &str| !s.is_empty() && s != "Unknown";
        head.description(if known(&key.make) && known(&key.model) {
            format!("{} {} ({})", key.make, key.model, key.connector)
        } else {
            key.connector.clone()
        });
        if head.version() >= 2 {
            if known(&key.make) {
                head.make(key.make.clone());
            }
            if known(&key.model) {
                head.model(key.model.clone());
            }
            if let Some(serial) = &key.serial {
                head.serial_number(serial.clone());
            }
        }
        if let Some((w, h)) = self
            .output_management
            .info
            .get(&model.id)
            .and_then(|i| i.physical_size)
            .filter(|(w, h)| *w > 0 && *h > 0)
        {
            head.physical_size(w, h);
        }
        let mut modes = Vec::new();
        for mode in self.head_modes(model) {
            let Ok(res) = client.create_resource::<ZwlrOutputModeV1, _, WaylandState>(
                &self.dh,
                head.version(),
                ModeData {
                    output: id_cell.clone(),
                    mode,
                },
            ) else {
                continue;
            };
            head.mode(&res);
            res.size(mode.size.0, mode.size.1);
            res.refresh(mode.refresh_mhz);
            if mode.preferred {
                res.preferred();
            }
            modes.push((mode, res));
        }
        head.enabled(1);
        let now = self.head_now(model);
        let mut h = Head {
            output: id_cell,
            manager: manager.id(),
            head,
            modes,
            told: Told {
                enabled: true,
                mode: None,
                position: (i32::MIN, i32::MIN),
                transform: now.transform,
                scale_120: 0,
                adaptive_sync: None,
            },
        };
        h.head.transform(transform_to_wl(now.transform));
        send_changes(&mut h, &now);
        self.output_management.heads.push(h);
    }

    /// Announces a screen turned off through output management as a disabled head (`enabled 0`).
    fn announce_disabled_head(
        &mut self,
        manager: &ZwlrOutputManagerV1,
        disabled_id: OutputId,
        disabled: &DisabledHead,
    ) {
        let Some(client) = manager.client() else {
            return;
        };
        let id_cell = Arc::new(Mutex::new(disabled_id));
        let Ok(head) = client.create_resource::<ZwlrOutputHeadV1, _, WaylandState>(
            &self.dh,
            manager.version(),
            HeadData {
                output: id_cell.clone(),
            },
        ) else {
            return;
        };
        manager.head(&head);
        let key = &disabled.key;
        head.name(key.connector.clone());
        let known = |s: &str| !s.is_empty() && s != "Unknown";
        head.description(if known(&key.make) && known(&key.model) {
            format!("{} {} ({})", key.make, key.model, key.connector)
        } else {
            key.connector.clone()
        });
        if head.version() >= 2 {
            if known(&key.make) {
                head.make(key.make.clone());
            }
            if known(&key.model) {
                head.model(key.model.clone());
            }
            if let Some(serial) = &key.serial {
                head.serial_number(serial.clone());
            }
        }
        if let Some((w, h)) = disabled
            .info
            .physical_size
            .filter(|(w, h)| *w > 0 && *h > 0)
        {
            head.physical_size(w, h);
        }
        let head_modes = if !disabled.info.modes.is_empty() {
            disabled.info.modes.clone()
        } else {
            vec![HeadMode {
                size: (
                    disabled.descriptor.mode.0 as i32,
                    disabled.descriptor.mode.1 as i32,
                ),
                refresh_mhz: disabled.descriptor.refresh_mhz as i32,
                preferred: true,
            }]
        };
        let mut modes = Vec::new();
        for mode in head_modes {
            let Ok(res) = client.create_resource::<ZwlrOutputModeV1, _, WaylandState>(
                &self.dh,
                head.version(),
                ModeData {
                    output: id_cell.clone(),
                    mode,
                },
            ) else {
                continue;
            };
            head.mode(&res);
            res.size(mode.size.0, mode.size.1);
            res.refresh(mode.refresh_mhz);
            if mode.preferred {
                res.preferred();
            }
            modes.push((mode, res));
        }
        head.enabled(0);
        let h = Head {
            output: id_cell,
            manager: manager.id(),
            head,
            modes,
            told: Told {
                enabled: false,
                mode: None,
                position: (i32::MIN, i32::MIN),
                transform: disabled.transform,
                scale_120: 0,
                adaptive_sync: None,
            },
        };
        self.output_management.heads.push(h);
    }

    /// Checks a configuration against the screens: the changes it makes, or `None` if it
    /// cannot be carried out.
    fn check_configuration(
        &self,
        heads: &HashMap<OutputId, HeadRequest>,
        disabled: &[OutputId],
    ) -> CheckResult {
        let currently_enabled: Vec<OutputId> = self.core.outputs().iter().map(|o| o.id).collect();

        for &id in disabled {
            if !currently_enabled.contains(&id)
                && !self.output_management.disabled_screens.contains_key(&id)
            {
                return CheckResult::Fail;
            }
        }
        let to_turn_off: Vec<OutputId> = disabled
            .iter()
            .copied()
            .filter(|id| currently_enabled.contains(id))
            .collect();

        let mut changes = Vec::new();
        let mut on = Vec::new();

        for (output, req) in heads {
            if let Some(model) = self.core.output(*output) {
                let now = self.head_now(model);
                let modes = self.head_modes(model);
                let mode = match req.mode {
                    None => None,
                    Some(RequestedMode::Listed(m)) => {
                        let Some(found) = modes.iter().find(|x| **x == m) else {
                            return CheckResult::Fail;
                        };
                        Some(*found)
                    }
                    Some(RequestedMode::Custom { size, refresh_mhz }) => {
                        let Some(found) = modes
                            .iter()
                            .filter(|m| m.size == size)
                            .filter(|m| {
                                refresh_mhz == 0 || (m.refresh_mhz - refresh_mhz).abs() < 500
                            })
                            .max_by_key(|m| m.refresh_mhz)
                        else {
                            return CheckResult::Fail;
                        };
                        Some(*found)
                    }
                }
                .filter(|m| Some(*m) != now.mode);
                let transform = req.transform.filter(|t| *t != now.transform);
                let adaptive_sync = match (req.adaptive_sync, now.adaptive_sync) {
                    (None, _) => None,
                    (Some(_), None) => return CheckResult::Fail,
                    (Some(v), Some(was)) => (v != was).then_some(v),
                };
                let scale_120 = match req.scale {
                    None => None,
                    Some(s) => {
                        if !(s.is_finite() && s > 0.0) {
                            return CheckResult::Fail;
                        }
                        Some((s * 120.0).round() as u32).filter(|s| *s != now.scale_120)
                    }
                };
                let position = req.position.filter(|p| *p != now.position);
                if mode.is_some()
                    || scale_120.is_some()
                    || position.is_some()
                    || transform.is_some()
                    || adaptive_sync.is_some()
                {
                    changes.push(HeadChange {
                        output: *output,
                        mode,
                        position,
                        scale_120,
                        transform,
                        adaptive_sync,
                    });
                }
            } else if let Some(dh) = self.output_management.disabled_screens.get(output) {
                let modes = if !dh.info.modes.is_empty() {
                    dh.info.modes.clone()
                } else {
                    vec![HeadMode {
                        size: (dh.descriptor.mode.0 as i32, dh.descriptor.mode.1 as i32),
                        refresh_mhz: dh.descriptor.refresh_mhz as i32,
                        preferred: true,
                    }]
                };

                let mode = match req.mode {
                    None => HeadMode {
                        size: (dh.descriptor.mode.0 as i32, dh.descriptor.mode.1 as i32),
                        refresh_mhz: dh.descriptor.refresh_mhz as i32,
                        preferred: true,
                    },
                    Some(RequestedMode::Listed(m)) => {
                        let Some(found) = modes.iter().find(|x| **x == m) else {
                            return CheckResult::Fail;
                        };
                        *found
                    }
                    Some(RequestedMode::Custom { size, refresh_mhz }) => {
                        let Some(found) = modes
                            .iter()
                            .filter(|m| m.size == size)
                            .filter(|m| {
                                refresh_mhz == 0 || (m.refresh_mhz - refresh_mhz).abs() < 500
                            })
                            .max_by_key(|m| m.refresh_mhz)
                        else {
                            return CheckResult::Fail;
                        };
                        *found
                    }
                };

                let transform = req.transform.unwrap_or(dh.transform);
                let scale_120 = match req.scale {
                    None => dh.scale_120,
                    Some(s) => {
                        if !(s.is_finite() && s > 0.0) {
                            return CheckResult::Fail;
                        }
                        (s * 120.0).round() as u32
                    }
                };
                let position = req.position.unwrap_or(dh.position);
                let adaptive_sync = match (req.adaptive_sync, dh.info.adaptive_sync) {
                    (None, _) => None,
                    (Some(_), None) => return CheckResult::Fail,
                    (Some(v), Some(_)) => Some(v),
                };

                let desc = OutputDescriptor {
                    key: dh.key.clone(),
                    mode: (mode.size.0 as u32, mode.size.1 as u32),
                    transform,
                    scale_120,
                    refresh_mhz: mode.refresh_mhz as u32,
                };
                if !infynitae_core::output::valid(&desc) {
                    return CheckResult::Fail;
                }

                on.push(HeadOn {
                    output: *output,
                    descriptor: desc,
                    position,
                    adaptive_sync,
                });
            } else {
                return CheckResult::Fail;
            }
        }

        changes.sort_by_key(|c| c.output);
        on.sort_by_key(|o| o.output);

        let remaining_enabled = (currently_enabled.len() - to_turn_off.len()) + on.len();
        if remaining_enabled == 0 {
            return CheckResult::Cancel;
        }

        CheckResult::Ok {
            changes,
            off: to_turn_off,
            on,
        }
    }
}

enum CheckResult {
    Cancel,
    Fail,
    Ok {
        changes: Vec<HeadChange>,
        off: Vec<OutputId>,
        on: Vec<HeadOn>,
    },
}

/// Sends what differs from what the head was told (no `done`): whether anything was sent.
fn send_changes(h: &mut Head, now: &Told) -> bool {
    let mut sent = false;
    if !h.told.enabled && now.enabled {
        h.head.enabled(1);
        sent = true;
    }
    if now.enabled {
        if now.mode != h.told.mode
            && let Some((_, res)) = h.modes.iter().find(|(m, _)| Some(*m) == now.mode)
        {
            h.head.current_mode(res);
            sent = true;
        }
        if now.position != h.told.position {
            h.head.position(now.position.0, now.position.1);
            sent = true;
        }
        if now.transform != h.told.transform {
            h.head.transform(transform_to_wl(now.transform));
            sent = true;
        }
        if now.scale_120 != h.told.scale_120 {
            h.head.scale(f64::from(now.scale_120) / 120.0);
            sent = true;
        }
        if h.head.version() >= 4
            && now.adaptive_sync != h.told.adaptive_sync
            && let Some(on) = now.adaptive_sync
        {
            h.head.adaptive_sync(if on {
                zwlr_output_head_v1::AdaptiveSyncState::Enabled
            } else {
                zwlr_output_head_v1::AdaptiveSyncState::Disabled
            });
            sent = true;
        }
    } else if h.told.enabled && !now.enabled {
        h.head.enabled(0);
        sent = true;
    }
    h.told = now.clone();
    sent
}

impl GlobalDispatch<ZwlrOutputManagerV1, crate::trust::Gate> for WaylandState {
    fn can_view(client: Client, gate: &crate::trust::Gate) -> bool {
        gate.allows(&client)
    }
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrOutputManagerV1>,
        _global_data: &crate::trust::Gate,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        let models: Vec<OutputModel> = state.core.outputs().to_vec();
        for model in &models {
            state.announce_head(&manager, model);
        }
        let mut disabled: Vec<(OutputId, DisabledHead)> = state
            .output_management
            .disabled_screens
            .iter()
            .map(|(id, d)| (*id, d.clone()))
            .collect();
        disabled.sort_by_key(|(id, _)| *id);
        for (id, d) in &disabled {
            state.announce_disabled_head(&manager, *id, d);
        }
        manager.done(state.output_management.serial);
        state.output_management.managers.push(manager);
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        manager: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                let config = data_init.init(
                    id,
                    ConfigurationData {
                        serial,
                        inner: Mutex::new(ConfigurationInner::default()),
                    },
                );
                state
                    .output_management
                    .configurations
                    .insert(config.id(), config);
            }
            zwlr_output_manager_v1::Request::Stop => {
                let om = &mut state.output_management;
                om.managers.retain(|m| m != manager);
                om.heads.retain(|h| h.manager != manager.id());
                manager.finished();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrOutputHeadV1, HeadData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        head: &ZwlrOutputHeadV1,
        request: zwlr_output_head_v1::Request,
        _data: &HeadData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_output_head_v1::Request::Release = request {
            state.output_management.heads.retain(|h| h.head != *head);
        }
    }
}

impl Dispatch<ZwlrOutputModeV1, ModeData> for WaylandState {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _mode: &ZwlrOutputModeV1,
        _request: zwlr_output_mode_v1::Request,
        _data: &ModeData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        // `release` is the only request
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ConfigurationData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        data: &ConfigurationData,
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_output_configuration_v1::{Error, Request};
        let mut inner = data.inner.lock().unwrap();
        let configure = |inner: &mut ConfigurationInner, output: OutputId| -> bool {
            if inner.used {
                resource.post_error(Error::AlreadyUsed, "configuration already used");
                return false;
            }
            if inner.heads.contains_key(&output) || inner.disabled.contains(&output) {
                resource.post_error(Error::AlreadyConfiguredHead, "head configured twice");
                return false;
            }
            true
        };
        match request {
            Request::EnableHead { id, head } => {
                let output = head.data::<HeadData>().map(|d| *d.output.lock().unwrap());
                let output = output.filter(|o| configure(&mut inner, *o));
                if let Some(o) = output {
                    inner.heads.insert(o, HeadRequest::default());
                }
                // the object must exist whatever happened: it is created
                data_init.init(
                    id,
                    ConfigurationHeadData {
                        output,
                        config: resource.id(),
                        set: AtomicU32::new(0),
                    },
                );
            }
            Request::DisableHead { head } => {
                if let Some(o) = head.data::<HeadData>().map(|d| *d.output.lock().unwrap())
                    && configure(&mut inner, o)
                {
                    inner.disabled.push(o);
                }
            }
            Request::Apply | Request::Test => {
                if inner.used {
                    resource.post_error(Error::AlreadyUsed, "configuration already used");
                    return;
                }
                inner.used = true;
                if data.serial != state.output_management.serial {
                    resource.cancelled();
                    return;
                }
                let checked = state.check_configuration(&inner.heads, &inner.disabled);
                match (checked, matches!(request, Request::Apply)) {
                    (CheckResult::Cancel, _) => resource.cancelled(),
                    (CheckResult::Fail, _) => resource.failed(),
                    (CheckResult::Ok { .. }, false) => resource.succeeded(),
                    (CheckResult::Ok { changes, off, on }, true) => {
                        for &id in &off {
                            if let Some(model) = state.core.output(id) {
                                let key = model.key.clone();
                                let desc = OutputDescriptor {
                                    key: model.key.clone(),
                                    mode: model.phys,
                                    transform: model.transform,
                                    scale_120: scale_120_of(model),
                                    refresh_mhz: model.refresh_mhz,
                                };
                                let pos = position_of(model);
                                let scale_120 = scale_120_of(model);
                                let transform = model.transform;
                                let info = state
                                    .output_management
                                    .info
                                    .get(&id)
                                    .cloned()
                                    .unwrap_or_else(|| HeadInfo {
                                        physical_size: None,
                                        modes: Vec::new(),
                                        adaptive_sync: None,
                                    });
                                state.output_management.disabled_screens.insert(
                                    id,
                                    DisabledHead {
                                        key,
                                        descriptor: desc,
                                        position: pos,
                                        scale_120,
                                        transform,
                                        info,
                                    },
                                );
                            }
                        }
                        if changes.is_empty() && off.is_empty() && on.is_empty() {
                            resource.succeeded();
                        } else {
                            state.output_management.pending.push(PendingConfiguration {
                                resource: resource.clone(),
                                changes,
                                off,
                                on,
                            });
                        }
                    }
                }
            }
            Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        resource: &ZwlrOutputConfigurationV1,
        _data: &ConfigurationData,
    ) {
        state
            .output_management
            .configurations
            .remove(&resource.id());
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ConfigurationHeadData> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        data: &ConfigurationHeadData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_output_configuration_head_v1::{Error, Request};
        let once = |flag: u32| -> bool {
            if data.set.fetch_or(flag, Ordering::SeqCst) & flag != 0 {
                resource.post_error(Error::AlreadySet, "property already set");
                return false;
            }
            true
        };
        let Some(config) = state
            .output_management
            .configurations
            .get(&data.config)
            .cloned()
        else {
            return;
        };
        let Some(cdata) = config.data::<ConfigurationData>() else {
            return;
        };
        let mut inner = cdata.inner.lock().unwrap();
        let Some(output) = data.output else {
            return;
        };
        let Some(head) = inner.heads.get_mut(&output) else {
            return;
        };
        match request {
            Request::SetMode { mode } => {
                let Some(m) = mode.data::<ModeData>() else {
                    return;
                };
                if *m.output.lock().unwrap() != output {
                    resource.post_error(Error::InvalidMode, "the mode is another head's");
                    return;
                }
                if once(SET_MODE) {
                    head.mode = Some(RequestedMode::Listed(m.mode));
                }
            }
            Request::SetCustomMode {
                width,
                height,
                refresh,
            } => {
                if width <= 0 || height <= 0 || refresh < 0 {
                    resource.post_error(Error::InvalidCustomMode, "invalid custom mode");
                    return;
                }
                if once(SET_MODE) {
                    head.mode = Some(RequestedMode::Custom {
                        size: (width, height),
                        refresh_mhz: refresh,
                    });
                }
            }
            Request::SetPosition { x, y } => {
                if once(SET_POSITION) {
                    head.position = Some((x, y));
                }
            }
            Request::SetTransform { transform } => {
                let Some(t) = transform.into_result().ok().and_then(transform_from_wl) else {
                    resource.post_error(Error::InvalidTransform, "unknown transform");
                    return;
                };
                if once(SET_TRANSFORM) {
                    head.transform = Some(t);
                }
            }
            Request::SetScale { scale } => {
                if !(scale.is_finite() && scale > 0.0) {
                    resource.post_error(Error::InvalidScale, "invalid scale");
                    return;
                }
                if once(SET_SCALE) {
                    head.scale = Some(scale);
                }
            }
            Request::SetAdaptiveSync { state: sync } => {
                let on = match sync {
                    WEnum::Value(zwlr_output_head_v1::AdaptiveSyncState::Enabled) => true,
                    WEnum::Value(zwlr_output_head_v1::AdaptiveSyncState::Disabled) => false,
                    _ => {
                        resource.post_error(
                            Error::InvalidAdaptiveSyncState,
                            "unknown adaptive sync state",
                        );
                        return;
                    }
                };
                if once(SET_ADAPTIVE_SYNC) {
                    head.adaptive_sync = Some(on);
                }
            }
            _ => {}
        }
    }
}
