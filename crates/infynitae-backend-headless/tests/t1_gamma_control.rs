//! `wlr-gamma-control-v1` integration tests (night light).
//! Tests exclusive per-output control, gamma_size advertisement, size validation,
//! and cleanup upon destruction.

mod common;

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

use common::input::*;
use common::{Lockstep, Probe, output};
use infynitae_core::output::OutputId;
use infynitae_wayland::gamma::GammaHandler;
use smithay::reexports::rustix::fs::{MemfdFlags, memfd_create};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::gamma_control::v1::client::zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1;
use wayland_protocols_wlr::gamma_control::v1::client::zwlr_gamma_control_v1::{
    self, ZwlrGammaControlV1,
};

impl Dispatch<ZwlrGammaControlManagerV1, ()> for Probe {
    fn event(
        _: &mut Self,
        _: &ZwlrGammaControlManagerV1,
        _: <ZwlrGammaControlManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrGammaControlV1,
        ev: zwlr_gamma_control_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                st.log.push(format!("gamma_size {size}"));
            }
            zwlr_gamma_control_v1::Event::Failed => {
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

type GammaRamps = (Vec<u16>, Vec<u16>, Vec<u16>);

struct FakeGamma {
    sizes: HashMap<OutputId, u32>,
    set_calls: Vec<(OutputId, GammaRamps)>,
    reset_calls: Vec<OutputId>,
}

impl GammaHandler for FakeGamma {
    fn gamma_size(&self, output: OutputId) -> Option<u32> {
        self.sizes.get(&output).copied()
    }

    fn set_gamma(
        &mut self,
        output: OutputId,
        red: &[u16],
        green: &[u16],
        blue: &[u16],
    ) -> Result<(), infynitae_wayland::gamma::GammaSetError> {
        self.set_calls
            .push((output, (red.to_vec(), green.to_vec(), blue.to_vec())));
        Ok(())
    }

    fn reset_gamma(&mut self, output: OutputId) {
        self.reset_calls.push(output);
    }
}

/// A backend without gamma support (headless default) immediately sends `failed`.
#[test]
fn backend_without_gamma_fails_immediately() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();
    let manager: ZwlrGammaControlManagerV1 = bind(&r, "zwlr_gamma_control_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let control = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();

    assert_eq!(r.ls.probe.log, vec!["failed"]);
    control.destroy();
    r.ls.pump();
}

/// A backend with gamma support advertises the gamma size.
#[test]
fn backend_with_gamma_advertises_size_and_applies_ramps() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();
    let output_id = r.ls.state.core().outputs()[0].id;

    let fake = Arc::new(Mutex::new(FakeGamma {
        sizes: HashMap::from([(output_id, 256)]),
        set_calls: Vec::new(),
        reset_calls: Vec::new(),
    }));
    r.ls.state.set_gamma_handler(fake.clone());

    let manager: ZwlrGammaControlManagerV1 = bind(&r, "zwlr_gamma_control_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let control = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();

    assert_eq!(r.ls.probe.log, vec!["gamma_size 256"]);
    r.ls.probe.log.clear();

    // Create a memfd with 3 ramps of 256 u16 values (1536 bytes)
    let fd = memfd_create("gamma", MemfdFlags::CLOEXEC).unwrap();
    let mut file = std::fs::File::from(fd);
    let mut data = Vec::with_capacity(3 * 256 * 2);
    for ramp_idx in 0..3 {
        for i in 0..256u16 {
            let val = i * 256 + ramp_idx;
            data.extend_from_slice(&val.to_ne_bytes());
        }
    }
    file.write_all(&data).unwrap();

    control.set_gamma(file.as_fd());
    r.ls.pump();

    let locked = fake.lock().unwrap();
    assert_eq!(locked.set_calls.len(), 1);
    assert_eq!(locked.set_calls[0].0, output_id);
    assert_eq!(locked.set_calls[0].1.0.len(), 256);
    assert_eq!(locked.set_calls[0].1.1.len(), 256);
    assert_eq!(locked.set_calls[0].1.2.len(), 256);
    drop(locked);

    // Destruction restores original ramp
    control.destroy();
    r.ls.pump();

    let locked = fake.lock().unwrap();
    assert_eq!(locked.reset_calls, vec![output_id]);
}

/// A second gamma control on the same output gets `failed`.
#[test]
fn at_most_one_gamma_control_per_output() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    r.ls.probe.log.clear();
    let output_id = r.ls.state.core().outputs()[0].id;

    let fake = Arc::new(Mutex::new(FakeGamma {
        sizes: HashMap::from([(output_id, 256)]),
        set_calls: Vec::new(),
        reset_calls: Vec::new(),
    }));
    r.ls.state.set_gamma_handler(fake.clone());

    let manager: ZwlrGammaControlManagerV1 = bind(&r, "zwlr_gamma_control_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();

    // First control succeeds
    let control1 = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["gamma_size 256"]);
    r.ls.probe.log.clear();

    // Second control fails
    let control2 = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["failed"]);
    r.ls.probe.log.clear();

    control2.destroy();
    r.ls.pump();

    // Releasing control1 allows a new control
    control1.destroy();
    r.ls.pump();

    let control3 = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();
    assert_eq!(r.ls.probe.log, vec!["gamma_size 256"]);
    control3.destroy();
    r.ls.pump();
}

/// A client setting gamma with the wrong table size receives protocol error invalid_gamma.
#[test]
fn wrong_gamma_size_triggers_protocol_error() {
    let mut r = Rig::new(Lockstep::new(Some(output((1920, 1080), 120))));
    let output_id = r.ls.state.core().outputs()[0].id;

    let fake = Arc::new(Mutex::new(FakeGamma {
        sizes: HashMap::from([(output_id, 256)]),
        set_calls: Vec::new(),
        reset_calls: Vec::new(),
    }));
    r.ls.state.set_gamma_handler(fake.clone());

    let manager: ZwlrGammaControlManagerV1 = bind(&r, "zwlr_gamma_control_manager_v1", 1);
    let wl_output = r.ls.probe.outputs[0].1.clone();
    let control = manager.get_gamma_control(&wl_output, &r.ls.qh, ());
    r.ls.pump();

    // Wrong size: only 100 bytes instead of 1536
    let fd = memfd_create("gamma_wrong", MemfdFlags::CLOEXEC).unwrap();
    let mut file = std::fs::File::from(fd);
    file.write_all(&[0u8; 100]).unwrap();

    control.set_gamma(file.as_fd());
    // Pump should detect the error and the client connection will receive the protocol error
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        r.ls.pump();
    }));
    assert!(res.is_err(), "protocol error invalid_gamma raised");
}
