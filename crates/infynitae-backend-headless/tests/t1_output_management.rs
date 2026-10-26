//! wlr-output-management-v1 with a real client (a display settings tool): the screens are
//! reported, a configuration changes the scale, position or rotation in Core and is reported
//! back, VRR is switched on a screen that has it, and a configuration that cannot be carried
//! out (stale, unknown mode, turning a screen off, VRR on a screen without it) changes nothing.

mod common;

use std::cell::RefCell;

use common::{Lockstep, Probe, output};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1,
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};

thread_local! {
    /// The proxies the client got, in order (the probe has no field for them).
    static HEADS: RefCell<Vec<ZwlrOutputHeadV1>> = const { RefCell::new(Vec::new()) };
    static MODES: RefCell<Vec<ZwlrOutputModeV1>> = const { RefCell::new(Vec::new()) };
}

impl Dispatch<ZwlrOutputManagerV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrOutputManagerV1,
        ev: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_output_manager_v1::Event::Head { head } => {
                HEADS.with_borrow_mut(|h| h.push(head));
                st.log.push("head".into());
            }
            zwlr_output_manager_v1::Event::Done { serial } => st.log.push(format!("done {serial}")),
            _ => {}
        }
    }

    wayland_client::event_created_child!(Probe, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrOutputHeadV1,
        ev: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_output_head_v1::Event;
        st.log.push(match ev {
            Event::Name { name } => format!("name {name}"),
            Event::Mode { mode } => {
                MODES.with_borrow_mut(|m| m.push(mode));
                "mode".into()
            }
            Event::CurrentMode { .. } => "current_mode".into(),
            Event::Position { x, y } => format!("position {x},{y}"),
            Event::Scale { scale } => format!("scale {scale}"),
            Event::Enabled { enabled } => format!("enabled {enabled}"),
            Event::Transform { transform } => format!("transform {transform:?}"),
            Event::AdaptiveSync { state } => format!("adaptive_sync {state:?}"),
            Event::Finished => "finished".into(),
            _ => return,
        });
    }

    wayland_client::event_created_child!(Probe, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrOutputModeV1,
        ev: zwlr_output_mode_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_output_mode_v1::Event;
        st.log.push(match ev {
            Event::Size { width, height } => format!("mode size {width}x{height}"),
            Event::Refresh { refresh } => format!("mode refresh {refresh}"),
            Event::Preferred => "mode preferred".into(),
            _ => return,
        });
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for Probe {
    fn event(
        st: &mut Self,
        _: &ZwlrOutputConfigurationV1,
        ev: zwlr_output_configuration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_output_configuration_v1::Event;
        st.log.push(match ev {
            Event::Succeeded => "succeeded".into(),
            Event::Failed => "failed".into(),
            Event::Cancelled => "cancelled".into(),
            _ => return,
        });
    }
}

wayland_client::delegate_noop!(Probe: ignore ZwlrOutputConfigurationHeadV1);

struct S {
    ls: Lockstep,
    manager: ZwlrOutputManagerV1,
}

impl S {
    fn new() -> S {
        HEADS.with_borrow_mut(Vec::clear);
        MODES.with_borrow_mut(Vec::clear);
        let mut ls = Lockstep::new(Some(output((1920, 1080), 120)));
        let (name, _, _) = ls
            .probe
            .globals
            .iter()
            .find(|(_, i, _)| i == "zwlr_output_manager_v1")
            .cloned()
            .expect("the manager is offered");
        let manager: ZwlrOutputManagerV1 = ls.registry().bind(name, 4, &ls.qh, ());
        ls.pump();
        S { ls, manager }
    }

    fn serial(&self) -> u32 {
        self.ls
            .probe
            .log
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("done ").and_then(|s| s.parse().ok()))
            .expect("a done")
    }

    fn head(&self) -> ZwlrOutputHeadV1 {
        HEADS.with_borrow(|h| h[0].clone())
    }

    /// A configuration of the one head against `serial`, set up by `f`, then `apply` or `test`.
    fn configure(
        &mut self,
        serial: u32,
        apply: bool,
        f: impl FnOnce(&ZwlrOutputConfigurationHeadV1),
    ) -> String {
        let config = self.manager.create_configuration(serial, &self.ls.qh, ());
        let head = config.enable_head(&self.head(), &self.ls.qh, ());
        f(&head);
        self.ls.probe.log.clear();
        if apply {
            config.apply();
        } else {
            config.test();
        }
        self.ls.pump();
        let outcome = self
            .ls
            .probe
            .log
            .iter()
            .find(|l| matches!(l.as_str(), "succeeded" | "failed" | "cancelled"))
            .cloned()
            .unwrap_or_default();
        config.destroy();
        outcome
    }

    fn scale(&self) -> f64 {
        self.ls.state.core().outputs()[0].scale
    }
}

#[test]
fn the_screens_are_reported() {
    let s = S::new();
    let log = &s.ls.probe.log;
    assert_eq!(log.iter().filter(|l| *l == "head").count(), 1, "{log:?}");
    for l in [
        "mode size 1920x1080",
        "mode preferred",
        "current_mode",
        "enabled 1",
        "position 0,0",
        "scale 1",
    ] {
        assert!(log.contains(&l.to_string()), "{l}: {log:?}");
    }
    assert!(log.iter().any(|l| l.starts_with("name ")), "{log:?}");
    assert!(
        log.last().is_some_and(|l| l.starts_with("done ")),
        "{log:?}"
    );
}

#[test]
fn a_scale_change_applies_and_is_reported() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(s.configure(serial, true, |h| h.set_scale(1.5)), "succeeded");
    assert_eq!(s.scale(), 1.5, "Core took the scale");
    let log = &s.ls.probe.log;
    assert!(
        log.contains(&"scale 1.5".to_string()),
        "re-reported: {log:?}"
    );
    assert!(s.serial() != serial, "with a new serial");
}

#[test]
fn a_position_change_moves_the_screen_in_core() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_position(100, 50)),
        "succeeded"
    );
    assert_eq!(s.ls.state.core().outputs()[0].arrangement, (100.0, 50.0));
    assert!(
        s.ls.probe.log.contains(&"position 100,50".to_string()),
        "{:?}",
        s.ls.probe.log
    );
}

#[test]
fn a_stale_configuration_is_cancelled() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(s.configure(serial, true, |h| h.set_scale(1.5)), "succeeded");
    assert_eq!(
        s.configure(serial, true, |h| h.set_scale(2.0)),
        "cancelled",
        "the screens changed since"
    );
    assert_eq!(s.scale(), 1.5);
}

#[test]
fn what_cannot_be_done_fails_and_changes_nothing() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| {
            h.set_scale(1.5);
            h.set_custom_mode(1234, 567, 0);
        }),
        "failed",
        "a mode the screen does not have"
    );
    assert_eq!(s.scale(), 1.0, "and the scale with it was not applied");

    let config = s.manager.create_configuration(serial, &s.ls.qh, ());
    config.disable_head(&s.head());
    s.ls.probe.log.clear();
    config.apply();
    s.ls.pump();
    assert!(
        s.ls.probe.log.contains(&"failed".to_string()),
        "turning a screen off: {:?}",
        s.ls.probe.log
    );
}

#[test]
fn a_test_changes_nothing() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, false, |h| h.set_scale(2.0)),
        "succeeded"
    );
    assert_eq!(s.scale(), 1.0);
    assert_eq!(
        s.configure(serial, false, |h| h.set_custom_mode(1234, 567, 0)),
        "failed"
    );
}

#[test]
fn a_listed_mode_is_accepted() {
    let mut s = S::new();
    let serial = s.serial();
    let mode = MODES.with_borrow(|m| m[0].clone());
    assert_eq!(
        s.configure(serial, true, |h| h.set_mode(&mode)),
        "succeeded"
    );
}

/// A rotation (a convertible in tablet mode, a portrait monitor): Core's view turns with it (90°
/// swaps width and height), the head reports it, and turning back restores the landscape view.
#[test]
fn a_rotation_turns_the_screen() {
    use wayland_client::protocol::wl_output::Transform as T;
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_transform(T::_90)),
        "succeeded"
    );
    let out = &s.ls.state.core().outputs()[0];
    assert_eq!(out.transform, infynitae_core::output::Transform::R90);
    let view = s.ls.state.core().view(out.id).expect("a view").size;
    assert_eq!(view, (1080, 1920), "the view is portrait");
    assert!(
        s.ls.probe
            .log
            .iter()
            .any(|l| l.starts_with("transform") && l.contains("_90")),
        "reported: {:?}",
        s.ls.probe.log
    );
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_transform(T::Normal)),
        "succeeded"
    );
    let out = &s.ls.state.core().outputs()[0];
    assert_eq!(s.ls.state.core().view(out.id).unwrap().size, (1920, 1080));
}

/// A scale Core cannot take (beyond 8) fails, and the screen keeps its scale.
#[test]
fn a_scale_out_of_range_fails() {
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(s.configure(serial, true, |h| h.set_scale(10.0)), "failed");
    assert_eq!(s.scale(), 1.0);
}

/// The screen has VRR (the console backend reports it): `allowed` is its state now.
fn with_vrr(s: &mut S, allowed: bool) {
    let id = s.ls.state.core().outputs()[0].id;
    s.ls.state.set_output_head_info(
        id,
        infynitae_wayland::output_management::HeadInfo {
            adaptive_sync: Some(allowed),
            ..Default::default()
        },
    );
    s.ls.pump();
}

/// A tool turns VRR off and on again (wlr-randr --adaptive-sync): it applies, and every tool is
/// told the new state with a new serial.
#[test]
fn vrr_switches_and_is_reported() {
    use zwlr_output_head_v1::AdaptiveSyncState as A;
    let mut s = S::new();
    with_vrr(&mut s, true);
    assert!(
        s.ls.probe
            .log
            .contains(&"adaptive_sync Value(Enabled)".to_string()),
        "reported: {:?}",
        s.ls.probe.log
    );
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_adaptive_sync(A::Disabled)),
        "succeeded"
    );
    assert!(
        s.ls.probe
            .log
            .contains(&"adaptive_sync Value(Disabled)".to_string()),
        "re-reported: {:?}",
        s.ls.probe.log
    );
    assert!(s.serial() != serial, "with a new serial");
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_adaptive_sync(A::Enabled)),
        "succeeded"
    );
    assert!(
        s.ls.probe
            .log
            .contains(&"adaptive_sync Value(Enabled)".to_string()),
        "{:?}",
        s.ls.probe.log
    );
}

/// A test of a VRR switch succeeds and changes nothing; a screen without VRR refuses it.
#[test]
fn vrr_test_changes_nothing_and_no_vrr_fails() {
    use zwlr_output_head_v1::AdaptiveSyncState as A;
    let mut s = S::new();
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, true, |h| h.set_adaptive_sync(A::Enabled)),
        "failed",
        "the screen has no VRR"
    );
    with_vrr(&mut s, true);
    let serial = s.serial();
    assert_eq!(
        s.configure(serial, false, |h| h.set_adaptive_sync(A::Disabled)),
        "succeeded"
    );
    assert!(
        !s.ls
            .probe
            .log
            .iter()
            .any(|l| l.starts_with("adaptive_sync") || l.starts_with("done")),
        "a test changes and reports nothing: {:?}",
        s.ls.probe.log
    );
}
