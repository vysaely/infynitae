//! `wlr-output-power-management-v1` integration tests (screen on/off).
//! Tests mode events, client set_mode requests, backend hooks, and invalid mode errors.

mod common;

use std::sync::{Arc, Mutex};

use common::input::*;
use common::{Lockstep, Probe, output};
use infynitae_core::output::OutputId;
use infynitae_wayland::output_power::{OutputPowerHandler, OutputPowerMode};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1;
use wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_v1::{
    self, Mode, ZwlrOutputPowerV1,
};

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputPowerManagerV1,
        _: <ZwlrOutputPowerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrOutputPowerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrOutputPowerV1,
        ev: zwlr_output_power_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_output_power_v1::Event::Mode { mode } => match mode {
                WEnum::Value(Mode::Off) => st.log.push("mode off".into()),
                WEnum::Value(Mode::On) => st.log.push("mode on".into()),
                _ => {}
            },
            zwlr_output_power_v1::Event::Failed => {
                st.log.push("failed".into());
            }
            _ => {}
        }
    }
}

fn bind<I>(r: &Rig, interface: &str, version: u32) -> I
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
    r.ls.registry().bind(name, version, &r.ls.qh, ())
}

struct FakePower {
    calls: Vec<(OutputId, OutputPowerMode)>,
}

impl OutputPowerHandler for FakePower {
    fn set_mode(&mut self, output: OutputId, mode: OutputPowerMode) {
        self.calls.push((output, mode));
    }
}

/// On binding, client receives the output's initial power mode (`Mode::On`).
#[test]
fn initial_mode_reported_on_bind() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();

    let manager: ZwlrOutputPowerManagerV1 = bind(&r, "zwlr_output_power_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let power = manager.get_output_power(&wl_output, &r.ls.qh, ());
    r.ls.pump();

    assert_eq!(r.ls.probe.log, vec!["mode on"]);
    power.destroy();
    r.ls.pump();
}

/// Setting mode off and on reports mode events and invokes backend handler.
#[test]
fn set_mode_updates_mode_and_invokes_handler() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();
    let output_id = r.ls.state.core().outputs()[0].id;

    let fake = Arc::new(Mutex::new(FakePower { calls: Vec::new() }));
    r.ls.state.set_output_power_handler(fake.clone());

    let manager: ZwlrOutputPowerManagerV1 = bind(&r, "zwlr_output_power_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let power = manager.get_output_power(&wl_output, &r.ls.qh, ());
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode on"]);
    r.ls.probe.log.clear();

    // Set mode Off
    power.set_mode(Mode::Off);
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode off"]);
    assert_eq!(
        r.ls.state.output_power_mode(output_id),
        OutputPowerMode::Off
    );
    r.ls.probe.log.clear();

    // Set mode On
    power.set_mode(Mode::On);
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode on"]);
    assert_eq!(r.ls.state.output_power_mode(output_id), OutputPowerMode::On);
    r.ls.probe.log.clear();

    let locked = fake.lock().unwrap();
    assert_eq!(
        locked.calls,
        vec![
            (output_id, OutputPowerMode::Off),
            (output_id, OutputPowerMode::On),
        ]
    );

    drop(locked);
    power.destroy();
    r.ls.pump();
}

/// Changes initiated by the compositor (idle sleep / wake) broadcast mode events to clients.
#[test]
fn compositor_initiated_mode_change_broadcasts_to_clients() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();

    let manager: ZwlrOutputPowerManagerV1 = bind(&r, "zwlr_output_power_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let power1 = manager.get_output_power(&wl_output, &r.ls.qh, ());
    let power2 = manager.get_output_power(&wl_output, &r.ls.qh, ());
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode on", "mode on"]);
    r.ls.probe.log.clear();

    // Compositor enters idle sleep
    r.ls.state.set_all_outputs_power_mode(OutputPowerMode::Off);
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode off", "mode off"]);
    r.ls.probe.log.clear();

    // Compositor wakes up
    r.ls.state.set_all_outputs_power_mode(OutputPowerMode::On);
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["mode on", "mode on"]);

    power1.destroy();
    power2.destroy();
    r.ls.pump();
}
