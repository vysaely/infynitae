//! Physical input intake (C4.1, C4.2): the button ownership table (OBJ-25), the interaction state
//! machine (OBJ-26, M22, CameraPan only in INC-3) and binding resolution.
//!
//! This module decides; it does not mutate cameras or the Stage. It cannot mint a proof and has no
//! mutable access to either: `entry::physical_input` applies its decisions (INV-CAMERA-1).

pub mod bindings;
pub mod focus;
pub mod interaction;
pub mod keys;
pub mod ownership;

/// Core's input state. The pointer position is not here: it is OBJ-20 (`Core.pointer`).
#[derive(Debug, Default)]
pub(crate) struct InputState {
    pub(crate) ownership: ownership::Ownership,
    pub(crate) key_ownership: ownership::Ownership,
    pub(crate) interaction: interaction::Interaction,
    pub(crate) clock: InputClock,
    pub(crate) modifiers: bindings::Modifiers,
}

/// The input-event clock as Core has seen it: the latest time a device stamped on a physical
/// event Core accepted (microseconds on that device clock). A pointer event Core synthesises into
/// a client's stream — the one true position after an output, presentation or Stage change
/// (M10, INV-INPUT-5) — carries this time, so the stream never goes back: every event a device
/// delivers later carries at least this time, including one queued while Core synthesised.
///
/// It is neither of the other two clocks: not the session clock (M7 deadlines, milliseconds
/// since the session started) and not the presentation clock. The nested backend's input times
/// are absolute `CLOCK_MONOTONIC` readings, as libinput's are; a session time on the wire would
/// jump back by the machine's uptime at the next real event. Before the first event there is no
/// stream to continue and the time is 0, which no later event precedes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputClock(Option<u64>);

impl InputClock {
    /// A physical event stamped `time_us` was accepted. The clock never goes back: an event a
    /// device stamped earlier than one already accepted (another device, behind) leaves it.
    pub(crate) fn accepted(&mut self, time_us: u64) {
        self.0 = Some(self.0.map_or(time_us, |t| t.max(time_us)));
    }

    /// The time of an event Core synthesises, on the wire.
    pub(crate) fn wire_ms(self) -> u32 {
        self.0.map_or(0, wire_ms)
    }
}

/// An input time on the wire: `wl_pointer` and `wl_keyboard` times are milliseconds in a `u32`,
/// which wraps (about every 49.7 days); clients compare them modulo 2³².
pub(crate) fn wire_ms(time_us: u64) -> u32 {
    (time_us / 1000) as u32
}

/// Where an axis event comes from (`wl_pointer.axis_source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisSource {
    Wheel,
    Continuous,
}

/// One axis event: continuous amounts in logical px and, for wheels, the high-resolution
/// discrete steps (`axis_value120`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub source: AxisSource,
    /// (horizontal, vertical) logical px.
    pub amount: (f64, f64),
    /// (horizontal, vertical) in 1/120 detents; zero for continuous sources.
    pub v120: (i32, i32),
}

#[cfg(test)]
mod tests {
    //! T0 intake tests on the production entry point. Expected values are hand-derived.

    use crate::geom::{CanvasPos, LocalPoint, LocalRect, LocalSize, Q16_ONE};
    use crate::input::bindings::{BTN_MIDDLE, Modifiers};
    use crate::input::interaction::Interaction;
    use crate::input::ownership::Owner;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key;
    use crate::output::{OutputDescriptor, OutputId, Transform};
    use crate::pointer::PointerState;
    use crate::protocol::{Effect, PointerTarget};
    use crate::snapshot::tests::key as skey;
    use crate::snapshot::{InputRegion, SnapshotSurface, SurfaceRole, SurfaceTree};
    use crate::{Core, InputPort, OutputLifecycleToken, PhysicalEvent};

    const BTN_LEFT: u32 = 0x110;

    /// A-5: the time of a synthesised pointer event is the latest input time Core accepted. The
    /// clock never goes back (an event stamped earlier, from a device behind, leaves it), reads 0
    /// before any event, and is put on the wire as `u32` milliseconds that wrap like every input
    /// time.
    #[test]
    fn the_input_clock_never_goes_back_and_wraps_like_the_wire() {
        let mut clock = super::InputClock::default();
        assert_eq!(clock.wire_ms(), 0);
        clock.accepted(12_345);
        assert_eq!(clock.wire_ms(), 12);
        clock.accepted(9_000);
        assert_eq!(clock.wire_ms(), 12, "the clock went back");
        clock.accepted(13_999);
        assert_eq!(clock.wire_ms(), 13);
        let wrap_us = (1u64 << 32) * 1000;
        clock.accepted(wrap_us + 7_000);
        assert_eq!(clock.wire_ms(), 7, "past 2^32 ms the wire time wraps");
        assert_eq!(super::wire_ms(wrap_us - 1_000), u32::MAX);
    }

    fn desc(c: &str) -> OutputDescriptor {
        OutputDescriptor {
            key: key(c),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        }
    }

    fn logo() -> Modifiers {
        Modifiers {
            logo: true,
            ..Modifiers::default()
        }
    }

    fn motion(c: &mut Core, port: &mut InputPort, d: (f64, f64)) -> Vec<Effect> {
        let ev = PhysicalEvent::PointerMotion {
            delta: d,
            delta_unaccel: d,
            time_us: 0,
        };
        c.physical_input_vec(port, ev).unwrap()
    }

    fn button(
        c: &mut Core,
        port: &mut InputPort,
        b: u32,
        pressed: bool,
        modifiers: Modifiers,
    ) -> Vec<Effect> {
        let ev = PhysicalEvent::PointerButton {
            button: b,
            pressed,
            modifiers,
            time_us: 0,
        };
        c.physical_input_vec(port, ev).unwrap()
    }

    /// W: one 400x300 root centred on the camera centre (0, 0) — anchor (−200, −150) by
    /// F-PLACE-1 — with an infinite input region.
    fn map_w(c: &mut Core) {
        let tree = |mapped: bool| SurfaceTree {
            geometry: LocalRect {
                loc: LocalPoint::default(),
                size: LocalSize { w: 400, h: 300 },
            },
            surfaces: vec![SnapshotSurface {
                key: skey(1),
                role: SurfaceRole::Root,
                size: if mapped {
                    LocalSize { w: 400, h: 300 }
                } else {
                    LocalSize::default()
                },
                mapped,
                input: InputRegion::Infinite,
                frame_requested: false,
            }],
        };
        let id = c.toplevel_created();
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
    }

    /// One output A, W mapped and shown (a frame produced after the map: hit-testing reads the
    /// last frame shown), pointer at the startup centre (960, 540) ⇒ Canvas (0, 0) = W (200, 150).
    fn rig(c: &mut Core, t: &mut OutputLifecycleToken) -> OutputId {
        let (a, _) = c.output_connected(t, desc("A")).unwrap();
        map_w(c);
        c.render_list(a).unwrap();
        a
    }

    fn w_target() -> Option<PointerTarget> {
        Some(PointerTarget {
            surface: skey(1),
            origin: crate::geom::CanvasPoint { x: -200, y: -150 },
        })
    }

    /// The client path: motion routed through the hit test, the relative delta, one frame.
    #[test]
    fn client_motion_is_routed_with_its_relative_delta() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        rig(&mut c, &mut t);
        let fx = motion(&mut c, &mut i, (10.5, 20.0));
        assert_eq!(
            fx,
            vec![
                Effect::PointerMotion {
                    target: w_target(),
                    location: CanvasPos {
                        x: 10 * Q16_ONE + Q16_ONE / 2,
                        y: 20 * Q16_ONE
                    },
                    time_ms: 0
                },
                Effect::PointerRelative {
                    target: w_target(),
                    delta: (10.5, 20.0),
                    delta_unaccel: (10.5, 20.0),
                    time_us: 0
                },
                Effect::PointerFrame
            ]
        );
    }

    /// M6/M22: the binding press acquires (no effect); pan motion changes only the camera (and
    /// membership); no client effect at all; the release gives exactly one true-position event.
    #[test]
    fn a_pan_emits_no_client_effect_and_ends_with_one_true_position() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = rig(&mut c, &mut t);
        assert!(button(&mut c, &mut i, BTN_MIDDLE, true, logo()).is_empty());
        assert!(matches!(c.interaction(), Interaction::CameraPan { .. }));
        for _ in 0..10 {
            let fx = motion(&mut c, &mut i, (-5.0, 3.0));
            assert!(
                fx.iter()
                    .all(|e| matches!(e, Effect::Enter { .. } | Effect::Leave { .. })),
                "INV-INPUT-3 violated: client effect during pan: {fx:?}"
            );
        }
        assert_eq!(c.cameras().center(0), Some(pos(50, -30)));
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: a,
                p: (960.0, 540.0)
            },
            "the pointer moved during a pan"
        );
        let fx = button(&mut c, &mut i, BTN_MIDDLE, false, logo());
        // q = (0,0) + (50, −30) = (50, −30): inside W, Canvas [−200, 200) × [−150, 150) ⇒ W
        assert_eq!(
            fx,
            vec![
                Effect::PointerMotion {
                    target: w_target(),
                    location: pos(50, -30),
                    time_ms: 0
                },
                Effect::PointerFrame
            ]
        );
    }

    fn pos(x: i64, y: i64) -> CanvasPos {
        CanvasPos {
            x: x * Q16_ONE,
            y: y * Q16_ONE,
        }
    }

    /// INV-COORD-3 / OB-21 T0: the camera is start − Q16(Σ) of the f64 sum of the whole gesture;
    /// per-event quantisation (the forbidden rule) drifts measurably over 10⁵ events.
    #[test]
    fn pan_quantises_the_gesture_sum_once_per_update() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        rig(&mut c, &mut t);
        button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        let d = (0.3, -0.7);
        let (mut sum, mut per_event) = ((0.0f64, 0.0f64), (0i64, 0i64));
        let q = |v: f64| (v * 65536.0).round_ties_even() as i64;
        for _ in 0..100_000 {
            motion(&mut c, &mut i, d);
            sum = (sum.0 + d.0, sum.1 + d.1);
            per_event = (per_event.0 + q(d.0), per_event.1 + q(d.1));
        }
        let want = CanvasPos {
            x: -q(sum.0),
            y: -q(sum.1),
        };
        assert_eq!(c.cameras().center(0), Some(want));
        let drift = (want.x + per_event.0)
            .abs()
            .max((want.y + per_event.1).abs());
        assert!(
            drift >= Q16_ONE / 4,
            "per-event rounding would not drift: control is vacuous ({drift})"
        );
    }

    /// F-PAN-1 at output scale 1.5: dp_phys = 1.5·dp_logical and the term divides by s_O·z_C, so
    /// the camera moves by exactly the logical delta (direct manipulation in Canvas px).
    #[test]
    fn pan_at_fractional_scale_moves_by_the_logical_delta() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let mut d = desc("A");
        d.scale_120 = 180;
        c.output_connected(&mut t, d).unwrap();
        button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        motion(&mut c, &mut i, (30.0, -12.0));
        assert_eq!(c.cameras().center(0), Some(pos(-30, 12)));
    }

    /// INV-INPUT-2: no acquisition while the client owns a button; the binding press is the
    /// client's. INV-INPUT-1: during an interaction every press is the compositor's; releases
    /// follow the press owner after the interaction ended.
    #[test]
    fn ownership_is_decided_at_press() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        rig(&mut c, &mut t);
        button(&mut c, &mut i, BTN_LEFT, true, Modifiers::default());
        let fx = button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        assert_eq!(c.interaction(), Interaction::Idle);
        assert_eq!(c.buttons().owner(BTN_MIDDLE), Some(Owner::Client));
        assert!(matches!(
            fx[0],
            Effect::PointerButton {
                button: BTN_MIDDLE,
                pressed: true,
                ..
            }
        ));
        button(&mut c, &mut i, BTN_MIDDLE, false, logo());
        button(&mut c, &mut i, BTN_LEFT, false, Modifiers::default());
        assert!(c.buttons().pressed().is_empty());
        // acquired now; a press during the pan is compositor-owned through its release
        button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        assert!(button(&mut c, &mut i, BTN_LEFT, true, Modifiers::default()).is_empty());
        assert_eq!(c.buttons().owner(BTN_LEFT), Some(Owner::Compositor));
        assert_eq!(button(&mut c, &mut i, BTN_MIDDLE, false, logo()).len(), 2);
        assert!(button(&mut c, &mut i, BTN_LEFT, false, Modifiers::default()).is_empty());
        // a release that was never pressed is ignored
        assert!(button(&mut c, &mut i, 0x113, false, Modifiers::default()).is_empty());
    }

    /// M10 NoOutput: motion, presses and axis are consumed; the pointer is not fabricated.
    #[test]
    fn no_output_consumes_everything() {
        let (mut c, mut i, _, _) = Core::new(PersistedCameras::default());
        assert!(motion(&mut c, &mut i, (5.0, 5.0)).is_empty());
        assert!(button(&mut c, &mut i, BTN_MIDDLE, true, logo()).is_empty());
        assert_eq!(
            c.interaction(),
            Interaction::Idle,
            "acquired without an output"
        );
        assert_eq!(c.buttons().owner(BTN_MIDDLE), Some(Owner::Compositor));
        assert!(button(&mut c, &mut i, BTN_MIDDLE, false, logo()).is_empty());
        assert_eq!(c.pointer(), PointerState::NoOutput);
    }

    /// C4.6: removing the pan's output orphans the pan; the release reconciles on the relocated
    /// output; the remaining output's camera is never moved by orphaned motion.
    #[test]
    fn output_removal_orphans_the_pan() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        motion(&mut c, &mut i, (4.0, 0.0));
        let fx = c.output_disconnected(&mut t, a).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::PointerMotion { .. })),
            "KeepFocus violated at orphaning: {fx:?}"
        );
        assert_eq!(
            c.interaction(),
            Interaction::Orphaned {
                initiator: BTN_MIDDLE
            }
        );
        let cam_b = c.cameras().center(1);
        assert!(motion(&mut c, &mut i, (100.0, 0.0)).is_empty());
        assert_eq!(c.cameras().center(1), cam_b);
        let fx = button(&mut c, &mut i, BTN_MIDDLE, false, logo());
        assert_eq!(fx.len(), 2, "one true-position event: {fx:?}");
        assert!(matches!(c.pointer(), PointerState::Located { output, .. } if output == b));
    }

    /// INC2-F07 rule extended to camera changes: a pan makes the output's ProjectionFrame stale
    /// (its storage is kept for the next list, C14.3), so the next hit test never reads a frame
    /// from a camera that no longer exists.
    #[test]
    fn a_pan_drops_the_output_frame() {
        let (mut c, mut i, _, mut t) = Core::new(PersistedCameras::default());
        let a = rig(&mut c, &mut t);
        c.render_list(a).unwrap();
        button(&mut c, &mut i, BTN_MIDDLE, true, logo());
        motion(&mut c, &mut i, (1.0, 0.0));
        assert!(c.current_frame(a).is_none(), "stale frame kept");
    }
}
