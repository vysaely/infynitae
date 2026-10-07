//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! Each entry point first checks that its port or token belongs to this Core. INC-3 state:
//! * the three output-lifecycle entries implement M4/M5 for any number of outputs (startup and
//!   hotplug alike, reconfiguration, disconnection, the zero-output state) and the pointer's M10
//!   transitions;
//! * physical pointer input (C4.2: ownership, M22 CameraPan, routing, KeepFocus);
//! * the shell vocabulary has no variants yet (INC-11).

use crate::input::bindings;
use crate::input::interaction::Interaction;
use crate::input::ownership::Owner;
use crate::output::{self, OutputDescriptor, OutputId, OutputModel};
use crate::pointer::{self, PointerState};
use crate::protocol::Effect;
use crate::{
    Core, EntryError, InputPort, OutputLifecycleToken, PhysicalEvent, ShellCommand, ShellPort,
};

pub mod authority {
    //! Every camera mutation consumes a fresh proof (INV-CAMERA-1): minted only in `entry`,
    //! not `Clone`, `!Send`, consumed by value.

    pub struct Authorized {
        _not_send: crate::NotSend,
    }

    impl Authorized {
        pub(in crate::entry) fn mint() -> Self {
            Authorized {
                _not_send: std::marker::PhantomData,
            }
        }
    }
}

impl Core {
    fn check(&self, core_id: u64) -> Result<(), EntryError> {
        if core_id == self.id {
            Ok(())
        } else {
            Err(EntryError::ForeignPort)
        }
    }

    /// C4.2 pointer intake. P0 validate; P1 ownership continuation (releases follow the press
    /// owner, motion follows the interaction); P2 binding resolution before any client dispatch;
    /// P3/P4 routing through the ProjectionFrame hit test. Returns the effects in order.
    pub fn physical_input(
        &mut self,
        port: &mut InputPort,
        ev: PhysicalEvent,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check(port.core_id)?;
        match ev {
            PhysicalEvent::PointerMotion {
                delta,
                delta_unaccel,
                time_us,
            } => {
                let finite = [delta.0, delta.1, delta_unaccel.0, delta_unaccel.1]
                    .iter()
                    .all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                // a motion owned by a pan mutates a camera: its proof is minted here, per event
                let proof = matches!(self.input.interaction, Interaction::CameraPan { .. })
                    .then(authority::Authorized::mint);
                Ok(self.pointer_motion(proof, delta, delta_unaccel, time_us))
            }
            PhysicalEvent::PointerButton {
                button,
                pressed,
                modifiers,
                time_us,
            } => Ok(if pressed {
                self.button_press(button, modifiers, ms(time_us))
            } else {
                self.button_release(button, ms(time_us))
            }),
            PhysicalEvent::PointerAxis { axis, time_us } => {
                let finite = [axis.amount.0, axis.amount.1].iter().all(|v| v.is_finite());
                if !finite {
                    return Err(EntryError::InvalidEvent);
                }
                // consumed during an interaction and in NoOutput (M10, M22)
                if self.input.interaction.active() || self.pointer == PointerState::NoOutput {
                    return Ok(Vec::new());
                }
                Ok(vec![
                    Effect::PointerAxis {
                        axis,
                        time_ms: ms(time_us),
                    },
                    Effect::PointerFrame,
                ])
            }
            PhysicalEvent::KeyboardKey {
                key,
                pressed,
                modifiers,
                mods_changed,
                time_us,
            } => Ok(if pressed {
                self.keyboard_press(key, modifiers, mods_changed, time_us)
            } else {
                self.keyboard_release(key, mods_changed, time_us)
            }),
        }
    }

    /// K1/K3 keyboard press intake in Core (INV-INPUT-1, OBJ-25).
    fn keyboard_press(
        &mut self,
        key: u32,
        modifiers: crate::input::bindings::Modifiers,
        mods_changed: bool,
        time_us: u64,
    ) -> Vec<Effect> {
        // M12 Down->press repeat: if already held by Compositor, stay compositor-owned and consume (K1)
        if self.input.key_ownership.owner(key) == Some(Owner::Compositor) {
            return vec![Effect::KeyboardIntercept { key }];
        }
        // Repeated press of already logically-down client key must not emit duplicate pressed (R03, K1/M12)
        if self.input.key_ownership.owner(key) == Some(Owner::Client) {
            return Vec::new();
        }

        let interaction_active = self.input.interaction.active();
        let matched = self.resolve_key_binding(key, modifiers, interaction_active);

        // K3: keys that change XKB modifier or lock state are never compositor-owned (observe mode)
        if mods_changed {
            self.input.key_ownership.press(key, Owner::Client);
            let mut fx = Vec::new();
            if let Some(action) = matched {
                fx.extend(self.apply_key_action(action));
            }
            fx.push(Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: true,
                time_us,
            });
            fx
        } else if let Some(action) = matched {
            // K1: compositor-owned press is consumed
            self.input.key_ownership.press(key, Owner::Compositor);
            let mut fx = self.apply_key_action(action);
            fx.push(Effect::KeyboardIntercept { key });
            fx
        } else {
            // Client-owned press: forwarded
            self.input.key_ownership.press(key, Owner::Client);
            vec![Effect::KeyboardForward {
                key,
                pressed: true,
                mods_changed: false,
                time_us,
            }]
        }
    }

    /// K2 keyboard release intake in Core (INV-INPUT-1, OBJ-25).
    fn keyboard_release(&mut self, key: u32, mods_changed: bool, time_us: u64) -> Vec<Effect> {
        // Release follows recorded owner. Orphan release (None) is dropped and never forwarded.
        match self.input.key_ownership.release(key) {
            Some(Owner::Client) => vec![Effect::KeyboardForward {
                key,
                pressed: false,
                mods_changed,
                time_us,
            }],
            Some(Owner::Compositor) => vec![Effect::KeyboardIntercept { key }],
            None => Vec::new(),
        }
    }

    fn apply_key_action(&mut self, action: crate::input::keys::KeyAction) -> Vec<Effect> {
        match action {
            crate::input::keys::KeyAction::FocusNext => self.focus_next(),
            crate::input::keys::KeyAction::Cancel => {
                if self.input.interaction.active() {
                    self.input.interaction = Interaction::Idle;
                }
                Vec::new()
            }
            crate::input::keys::KeyAction::Noop => Vec::new(),
        }
    }

    /// Motion follows the interaction (INV-INPUT-1): a pan moves the camera and nothing reaches a
    /// client (INV-INPUT-3, KeepFocus); an orphaned interaction or NoOutput consumes it; otherwise
    /// the pointer moves in the Arrangement and the event is routed, with its relative motion.
    fn pointer_motion(
        &mut self,
        proof: Option<authority::Authorized>,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    ) -> Vec<Effect> {
        match self.input.interaction {
            Interaction::CameraPan { output, camera, .. } => {
                let (Some(scale), Some(zoom)) = (
                    self.output(output).map(|o| o.scale),
                    self.cameras.zoom(camera),
                ) else {
                    return Vec::new();
                };
                let (Some(proof), Some((camera, start, sum))) =
                    (proof, self.input.interaction.pan_motion(delta, scale, zoom))
                else {
                    return Vec::new();
                };
                self.cameras.pan(proof, camera, start, sum);
                self.frames.retain(|(o, _)| *o != output);
                #[allow(unused_mut)]
                let mut fx = self.visibility_pass();
                #[cfg(feature = "probe_pan_motion_bypass")]
                // NEGATIVE CONTROL (INV-INPUT-3 bypass switch, TST-05): pan motion also reaches
                // the client. The x8 tests must fail under it.
                fx.extend(self.routed_motion(delta, delta_unaccel, time_us));
                fx
            }
            Interaction::Orphaned { .. } => Vec::new(),
            Interaction::Idle => {
                let PointerState::Located { output, p } = self.pointer else {
                    return Vec::new();
                };
                if let Some(c) = self.constraint {
                    if c.kind == pointer::ConstraintKind::Lock {
                        return self.locked_motion(delta, delta_unaccel, time_us);
                    }
                    if c.kind == pointer::ConstraintKind::Confine {
                        self.pointer = self.confined_motion(output, p, delta, &c);
                        return self.routed_motion(delta, delta_unaccel, time_us);
                    }
                }
                if let Some(moved) = pointer::moved(&self.outputs, output, p, delta) {
                    self.pointer = moved;
                }
                self.routed_motion(delta, delta_unaccel, time_us)
            }
        }
    }

    /// The client path of one motion event: the true-position motion, the relative motion of the
    /// physical delta to the same routing (INV-INPUT-4), one frame.
    fn routed_motion(
        &mut self,
        delta: (f64, f64),
        delta_unaccel: (f64, f64),
        time_us: u64,
    ) -> Vec<Effect> {
        let Some((target, location)) = self.route_pointer() else {
            return Vec::new();
        };
        vec![
            Effect::PointerMotion {
                target,
                location,
                time_ms: ms(time_us),
            },
            Effect::PointerRelative {
                target,
                delta,
                delta_unaccel,
                time_us,
            },
            Effect::PointerFrame,
        ]
    }

    /// Ownership is decided here, at press, before any client dispatch (INV-INPUT-1, C4.2 P2).
    fn button_press(
        &mut self,
        button: u32,
        mods: bindings::Modifiers,
        time_ms: u32,
    ) -> Vec<Effect> {
        if self.input.ownership.owner(button).is_some() {
            return Vec::new(); // a repeated press keeps its first owner
        }
        // M22: any press during an interaction is owned by it; M10: NoOutput consumes presses
        let PointerState::Located { output, p } = self.pointer else {
            self.input.ownership.press(button, Owner::Compositor);
            return Vec::new();
        };
        if self.input.interaction.active() {
            self.input.ownership.press(button, Owner::Compositor);
            return Vec::new();
        }
        let camera = self.cameras.camera_for_output(output);
        let acquire = bindings::is_pan_binding(button, mods)
            // INV-INPUT-2: never while the client owns a button
            && !self.input.ownership.client_owns_any()
            // OB-24 gate: no pan coexists with a persistent zoom ≠ 1
            && camera.and_then(|c| self.cameras.zoom(c)) == Some(1.0);
        if let (true, Some(camera), Some(start)) =
            (acquire, camera, camera.and_then(|c| self.cameras.center(c)))
        {
            // M6 Idle → Panning: the press is consumed; KeepFocus (no event).
            // OB-4: Dismiss active popup grabs before the camera change.
            self.input.interaction = Interaction::CameraPan {
                output,
                camera,
                initiator: button,
                start,
                sum: (0.0, 0.0),
            };
            self.input.ownership.press(button, Owner::Compositor);
            let mut fx = Vec::new();
            if let Some(target) = self.focus.target() {
                fx.push(Effect::DismissPopupGrab(target));
            }
            return fx;
        }
        self.input.ownership.press(button, Owner::Client);
        let mut fx = Vec::new();
        // Click-to-focus (INV-INPUT-8, M11): an explicit user action.
        // If the click hits an element, that element gains ActiveElementFocus.
        let pt = crate::geom::OutputLogicalPoint { x: p.0, y: p.1 };
        if let Some(hit) = self.hit_test(output, pt) {
            fx.extend(self.focus.set_focus(Some(hit.element)));
        }
        fx.push(Effect::PointerButton {
            button,
            pressed: true,
            time_ms,
        });
        fx.push(Effect::PointerFrame);
        fx
    }

    /// A release follows the press owner (INV-INPUT-1). The initiator's release ends the
    /// interaction with batch-end reconciliation: one true-position event (M10).
    fn button_release(&mut self, button: u32, time_ms: u32) -> Vec<Effect> {
        match self.input.ownership.release(button) {
            None => Vec::new(),
            Some(Owner::Compositor) => {
                if self.input.interaction.initiator() != Some(button) {
                    return Vec::new();
                }
                self.input.interaction = Interaction::Idle;
                let mut fx = self.reconcile_constraints();
                if self.constraint.is_none() {
                    fx.extend(self.true_position(time_ms));
                }
                fx
            }
            // In NoOutput the zero-output grab receives it and delivers nothing, keeping
            // Smithay's pressed-button set truthful (no stuck implicit grab after reconnect).
            Some(Owner::Client) => {
                let mut fx = vec![
                    Effect::PointerButton {
                        button,
                        pressed: false,
                        time_ms,
                    },
                    Effect::PointerFrame,
                ];
                if !self.input.ownership.client_owns_any() {
                    fx.extend(self.reconcile_constraints());
                }
                fx
            }
        }
    }

    pub fn shell_command(
        &mut self,
        port: &mut ShellPort,
        cmd: ShellCommand,
    ) -> Result<(), EntryError> {
        self.check(port.core_id)?;
        match cmd {}
    }

    /// M4 `Absent → Enabled` (startup and hotplug alike): validates the hardware facts, allocates
    /// a new `OutputId`, binds a camera by the M5 precedence (one proof), places the output in the
    /// Arrangement, leaves `NoOutput` by the startup rule, and runs the visibility pass. Effects:
    /// `OutputAdded` first, then enters.
    pub fn output_connected(
        &mut self,
        token: &mut OutputLifecycleToken,
        desc: OutputDescriptor,
    ) -> Result<(OutputId, Vec<Effect>), EntryError> {
        self.check(token.core_id)?;
        if !output::valid(&desc) {
            return Err(EntryError::InvalidDescriptor);
        }
        let id = self.output_ids.next();
        let right_edge = self
            .outputs
            .iter()
            .map(|o| o.arrangement.0 + o.logical_extent().0)
            .fold(0.0, f64::max);
        let model = OutputModel::from_descriptor(id, &desc, (right_edge, 0.0));
        self.cameras.bind_on_connect(
            authority::Authorized::mint(),
            &desc.key,
            id,
            &self.persisted,
        );
        let was_zero = self.pointer == PointerState::NoOutput;
        if was_zero {
            self.pointer = pointer::startup(&model);
        }
        self.outputs.push(model);
        let mut fx = vec![Effect::OutputAdded(id)];
        fx.extend(self.visibility_pass());
        if was_zero {
            // M10 NoOutput → Located: the grab goes first, then one routed motion (enter) unless
            // an orphaned interaction still owns the pointer
            fx.push(Effect::PointerZeroOutputEnd);
            if !self.input.interaction.active() {
                fx.extend(self.true_position(0));
            }
        }
        fx.extend(self.reconcile_constraints());
        Ok((id, fx))
    }

    /// M4 reconfiguration: new hardware facts for a connected output, same `OutputId`, same
    /// `OutputKey` (else rejected, nothing mutated). Not a camera authority: no proof is minted
    /// and no camera item is called (E1R; checked by tools/verify-architecture). The view,
    /// ProjectionFrame and membership are rebuilt; the pointer is clamped (F-RECONF-1). Effects:
    /// `OutputChanged`, then leaves and enters.
    pub fn output_reconfigured(
        &mut self,
        token: &mut OutputLifecycleToken,
        output: OutputId,
        desc: OutputDescriptor,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check(token.core_id)?;
        let slot = self
            .outputs
            .iter_mut()
            .find(|o| o.id == output)
            .ok_or(EntryError::UnknownOutput)?;
        if slot.key != desc.key {
            return Err(EntryError::IdentityChanged);
        }
        if !output::valid(&desc) {
            return Err(EntryError::InvalidDescriptor);
        }
        *slot = OutputModel::from_descriptor(output, &desc, slot.arrangement);
        let model = slot.clone();
        self.frames.retain(|(o, _)| *o != output);
        self.pointer = pointer::reconfigured(self.pointer, &model);
        let mut fx = vec![Effect::OutputChanged(output)];
        fx.extend(self.visibility_pass());
        // M10: at most one true-position event, none while an interaction owns the pointer
        let on_it = matches!(self.pointer, PointerState::Located { output: o, .. } if o == output);
        if on_it && !self.input.interaction.active() {
            fx.extend(self.true_position(0));
        }
        fx.extend(self.reconcile_constraints());
        Ok(fx)
    }

    /// M4 `Enabled → Removed` (INV-OUTPUT-1): the camera is unbound and kept (one proof); the
    /// output's ProjectionFrame is dropped; the pointer is relocated to a surviving output
    /// (F-RELOCATE-1) or, when none survives, enters `NoOutput`. Effects: every leave for the
    /// output, then `OutputRemoved`.
    pub fn output_disconnected(
        &mut self,
        token: &mut OutputLifecycleToken,
        output: OutputId,
    ) -> Result<Vec<Effect>, EntryError> {
        self.check(token.core_id)?;
        let index = self
            .outputs
            .iter()
            .position(|o| o.id == output)
            .ok_or(EntryError::UnknownOutput)?;
        let lost = self.outputs.remove(index);
        self.frames.retain(|(o, _)| *o != output);
        self.cameras
            .unbind_on_disconnect(authority::Authorized::mint(), output);
        // C4.6: a pan on the removed output keeps its ownership and consumes motion until release
        self.input.interaction.orphan_if_on(output);
        let mut pointer_fx = Vec::new();
        if let PointerState::Located { output: o, p } = self.pointer
            && o == output
        {
            self.pointer = match self.outputs.split_first() {
                Some((first, rest)) => pointer::relocate(&lost, p, first, rest),
                None => PointerState::NoOutput,
            };
            if self.pointer == PointerState::NoOutput {
                // M10 → NoOutput: Focus::Clear grab (leave), nothing to the old focus
                self.constraint = None;
                pointer_fx.push(Effect::PointerZeroOutputBegin);
            } else {
                let recon_fx = self.reconcile_constraints();
                pointer_fx.extend(recon_fx);
                if !self.input.interaction.active() && self.constraint.is_none() {
                    pointer_fx.extend(self.true_position(0));
                }
            }
        }
        let mut fx = self.visibility_pass();
        fx.extend(pointer_fx);
        fx.push(Effect::OutputRemoved(output));
        Ok(fx)
    }
}

/// `wl_pointer` timestamps: milliseconds with wrap-around.
fn ms(time_us: u64) -> u32 {
    (time_us / 1000) as u32
}

#[cfg(test)]
mod tests {
    //! The 2R / E1R output-lifecycle tests (spike EE-4 / ER2-1) ported to the production core.
    //! Expected values are hand-derived literals, not values computed by the code under test.

    use crate::geom::CanvasPos;
    use crate::navigation::PersistedCameras;
    use crate::output::tests::key;
    use crate::output::{OutputDescriptor, OutputId, OutputKey, Transform};
    use crate::pointer::PointerState;
    use crate::protocol::Effect;
    use crate::{Core, EntryError};

    fn desc_k(
        k: OutputKey,
        mode: (u32, u32),
        t: Transform,
        scale_120: u32,
        refresh_mhz: u32,
    ) -> OutputDescriptor {
        OutputDescriptor {
            key: k,
            mode,
            transform: t,
            scale_120,
            refresh_mhz,
        }
    }

    fn desc(connector: &str) -> OutputDescriptor {
        desc_k(key(connector), (1920, 1080), Transform::Normal, 120, 60_000)
    }

    fn pos(x: i64, y: i64) -> CanvasPos {
        CanvasPos {
            x: x << 16,
            y: y << 16,
        }
    }

    fn camera_of(c: &Core, o: OutputId) -> usize {
        c.cameras().camera_for_output(o).expect("bound")
    }

    /// A token minted by another Core grants nothing (C1.5, C12.R2.3).
    #[test]
    fn foreign_tokens_grant_nothing() {
        let (mut a, _, _, mut own) = Core::new(PersistedCameras::default());
        let (_b, _, _, mut foreign) = Core::new(PersistedCameras::default());
        let (o, _) = a.output_connected(&mut own, desc("A")).unwrap();
        assert_eq!(
            a.output_connected(&mut foreign, desc("B")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            a.output_reconfigured(&mut foreign, o, desc("A")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            a.output_disconnected(&mut foreign, o).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            (a.cameras().len(), a.outputs().len()),
            (1, 1),
            "a foreign token changed state"
        );
    }

    /// M4/M5: no camera exists before a connect; each connect creates and binds its own camera at
    /// the default placement; connecting another output leaves existing cameras untouched.
    #[test]
    fn connect_creates_and_binds_one_camera_per_output() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert!(c.cameras().is_empty());
        let (a, fx) = c.output_connected(&mut t, desc("A")).unwrap();
        // M10 NoOutput → Located: the startup pointer (960, 540) unprojects through
        // c_eff = (−960, −540) to Canvas (0, 0); nothing is mapped, so the focus stays clear
        assert_eq!(
            fx,
            vec![
                Effect::OutputAdded(a),
                Effect::PointerZeroOutputEnd,
                Effect::PointerMotion {
                    target: None,
                    location: pos(0, 0),
                    time_ms: 0
                },
                Effect::PointerFrame
            ]
        );
        assert_eq!(camera_of(&c, a), 0);
        assert_eq!(
            (c.cameras().center(0), c.cameras().zoom(0)),
            (Some(pos(0, 0)), Some(1.0))
        );
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        assert_ne!(a, b);
        assert_eq!(camera_of(&c, b), 1);
        assert_eq!(
            c.cameras().center(0),
            Some(pos(0, 0)),
            "connecting B changed A's camera"
        );
        assert_eq!(c.outputs().len(), 2);
        assert_eq!(
            c.output(b).unwrap().arrangement,
            (1920.0, 0.0),
            "B is arranged right of A"
        );
    }

    /// M5: disconnect unbinds and keeps the camera and its value; reconnecting the same key re-binds
    /// the same camera unchanged with a new OutputId (reconnect is not reconfiguration).
    #[test]
    fn disconnect_keeps_the_camera_and_reconnect_rebinds_it() {
        let persisted =
            PersistedCameras::from_entries(vec![(key("A"), pos(-25, 40), 1.0)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
        let fx = c.output_disconnected(&mut t, a).unwrap();
        // the last output: M10 → NoOutput installs the zero-output grab before the removal
        assert_eq!(
            fx,
            vec![Effect::PointerZeroOutputBegin, Effect::OutputRemoved(a)]
        );
        assert_eq!(c.cameras().len(), 1, "camera destroyed on disconnect");
        assert_eq!(c.cameras().bound_output(0), None);
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
        assert_eq!(
            c.output_disconnected(&mut t, a).err(),
            Some(EntryError::UnknownOutput),
            "repeated disconnect"
        );
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_ne!(a, a2, "OutputId reused");
        assert_eq!(camera_of(&c, a2), 0, "the remembered camera is re-bound");
        assert_eq!(c.cameras().len(), 1);
        assert_eq!(c.cameras().center(0), Some(pos(-25, 40)));
    }

    /// INV-CAMERA-3 rule order: a camera remembered this session wins over the persisted entry.
    #[test]
    fn remembered_camera_wins_over_the_persisted_entry() {
        let persisted = PersistedCameras::from_entries(vec![
            (key("A"), pos(5000, -700), 1.5),
            (key("C"), pos(-123, 456), 0.75),
        ])
        .unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        assert_eq!(
            (
                c.cameras().center(camera_of(&c, a)),
                c.cameras().zoom(camera_of(&c, a))
            ),
            (Some(pos(5000, -700)), Some(1.5))
        );
        assert_eq!(
            (
                c.cameras().center(camera_of(&c, b)),
                c.cameras().zoom(camera_of(&c, b))
            ),
            (Some(pos(0, 0)), Some(1.0))
        );
        let (cc, _) = c.output_connected(&mut t, desc("C")).unwrap();
        assert_eq!(
            c.cameras().center(camera_of(&c, cc)),
            Some(pos(-123, 456)),
            "runtime hotplug takes the same path"
        );
        c.output_disconnected(&mut t, a).unwrap();
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(camera_of(&c, a2), 0);
        assert_eq!(c.cameras().len(), 3, "no new camera for a remembered key");
    }

    /// Runtime 2i (OB-49, transferred from OB-19): two CONNECTED outputs presenting the same
    /// OutputKey each keep their own camera (1:1); the duplicate is not seeded from persistence;
    /// on reconnect the lowest unbound camera with the key is re-bound. Under the
    /// `probe_sealed_key_steal` feature (the sealed 2R rule) this test must fail with "1:1 violated".
    #[test]
    fn concurrent_output_key_collision_keeps_one_to_one() {
        let persisted =
            PersistedCameras::from_entries(vec![(key("A"), pos(77, 88), 1.25)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (ca, cb) = (
            c.cameras().camera_for_output(a),
            c.cameras().camera_for_output(b),
        );
        assert!(
            ca.is_some() && cb.is_some() && ca != cb,
            "1:1 violated: a={ca:?} b={cb:?}"
        );
        // every connected output has exactly one camera, every camera at most one output
        for o in [a, b] {
            let n = (0..c.cameras().len())
                .filter(|&i| c.cameras().bound_output(i) == Some(o))
                .count();
            assert_eq!(n, 1, "1:1 violated: output {o:?} has {n} cameras");
        }
        assert_eq!(
            (
                c.cameras().center(ca.unwrap()),
                c.cameras().zoom(ca.unwrap())
            ),
            (Some(pos(77, 88)), Some(1.25))
        );
        assert_eq!(
            (
                c.cameras().center(cb.unwrap()),
                c.cameras().zoom(cb.unwrap())
            ),
            (Some(pos(0, 0)), Some(1.0)),
            "duplicate seeded from persistence"
        );
        // the two views differ: no aliasing of camera state
        assert_ne!(c.view(a).unwrap().c_eff, c.view(b).unwrap().c_eff);
        c.output_disconnected(&mut t, a).unwrap();
        c.output_disconnected(&mut t, b).unwrap();
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.cameras().camera_for_output(a2),
            ca,
            "reconnect must take the lowest unbound camera"
        );
        let (b2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.cameras().camera_for_output(b2),
            cb,
            "then the next unbound camera"
        );
        assert_eq!(c.cameras().len(), 2);
    }

    /// E1R (EE-4): mode, scale, transform and refresh changes keep the OutputId, the camera
    /// identity and value, and allocate nothing.
    #[test]
    fn reconfiguration_keeps_identity_and_camera() {
        let persisted = PersistedCameras::from_entries(vec![(key("A"), pos(17, -3), 1.0)]).unwrap();
        let (mut c, _i, _s, mut t) = Core::new(persisted);
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        for d in [
            desc_k(key("A"), (2560, 1440), Transform::Normal, 120, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::Normal, 150, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::R90, 150, 60_000),
            desc_k(key("A"), (2560, 1440), Transform::R90, 150, 144_000),
        ] {
            let fx = c.output_reconfigured(&mut t, o, d).unwrap();
            assert_eq!(fx[0], Effect::OutputChanged(o));
            assert_eq!(c.outputs().len(), 1);
            assert_eq!(c.outputs()[0].id, o);
            assert_eq!(camera_of(&c, o), 0);
            assert_eq!(c.cameras().len(), 1);
            assert_eq!(
                (c.cameras().center(0), c.cameras().zoom(0)),
                (Some(pos(17, -3)), Some(1.0))
            );
        }
        let m = c.output(o).unwrap();
        assert_eq!(
            (m.phys, m.view_size(), m.scale, m.refresh_mhz),
            ((2560, 1440), (1440, 2560), 1.25, 144_000)
        );
    }

    /// E1R: unknown and stale ids, foreign tokens, a changed key and an invalid descriptor are
    /// rejected and mutate nothing.
    #[test]
    fn reconfiguration_rejections_mutate_nothing() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let before = (c.outputs().to_vec(), c.pointer(), c.cameras().len());
        let (_other, _, _, mut foreign) = Core::new(PersistedCameras::default());
        assert_eq!(
            c.output_reconfigured(&mut t, OutputId::for_test(99), desc("A"))
                .err(),
            Some(EntryError::UnknownOutput)
        );
        assert_eq!(
            c.output_reconfigured(&mut foreign, o, desc("A")).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            c.output_reconfigured(&mut t, o, desc("B")).err(),
            Some(EntryError::IdentityChanged)
        );
        assert_eq!(
            c.output_reconfigured(
                &mut t,
                o,
                desc_k(key("A"), (0, 0), Transform::Normal, 120, 60_000)
            )
            .err(),
            Some(EntryError::InvalidDescriptor)
        );
        assert_eq!(
            (c.outputs().to_vec(), c.pointer(), c.cameras().len()),
            before
        );
        c.output_disconnected(&mut t, o).unwrap();
        let (o2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_ne!(o, o2);
        assert_eq!(
            c.output_reconfigured(&mut t, o, desc("A")).err(),
            Some(EntryError::UnknownOutput),
            "stale OutputId"
        );
    }

    /// F-RECONF-1: p is clamped into the new extent (half-open, wl_fixed); inside stays.
    #[test]
    fn reconfiguration_clamps_the_pointer() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: o,
                p: (960.0, 540.0)
            }
        );
        // 90° at scale 1.5: logical 720 x 1280 → x clamps to 720 − 2⁻⁸
        c.output_reconfigured(
            &mut t,
            o,
            desc_k(key("A"), (1920, 1080), Transform::R90, 180, 60_000),
        )
        .unwrap();
        let want = PointerState::Located {
            output: o,
            p: (720.0 - 1.0 / 256.0, 540.0),
        };
        assert_eq!(c.pointer(), want);
        c.output_reconfigured(&mut t, o, desc("A")).unwrap();
        assert_eq!(
            c.pointer(),
            want,
            "a pointer inside the new extent is unchanged"
        );
    }

    /// INV-OUTPUT-1 zero outputs: before any connect and after the last disconnect the pointer is
    /// `NoOutput`; nothing is fabricated; cameras persist unbound; the first connect places the
    /// pointer at the new output's centre; a later connect does not move a located pointer.
    #[test]
    fn zero_output_state_is_stable_and_the_first_connect_leaves_it() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        c.output_disconnected(&mut t, a).unwrap();
        assert!(c.outputs().is_empty());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        assert_eq!(c.cameras().bound_output(0), None);
        assert_eq!(
            c.output_disconnected(&mut t, a).err(),
            Some(EntryError::UnknownOutput)
        );
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (b, _) = c
            .output_connected(
                &mut t,
                desc_k(key("B"), (2560, 1440), Transform::R270, 240, 60_000),
            )
            .unwrap();
        // view 1440x2560 at scale 2 → logical 720x1280; centre (360, 640)
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (360.0, 640.0)
            }
        );
        let (a2, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(camera_of(&c, a2), 0);
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (360.0, 640.0)
            }
        );
    }

    /// F-RELOCATE-1: with survivors the pointer moves to the Arrangement-nearest output; with none
    /// it enters NoOutput (the formula's domain excludes the empty set).
    #[test]
    fn relocation_uses_survivors_only() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap(); // pointer (960,540) on A
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap(); // B at x = 1920
        c.output_disconnected(&mut t, a).unwrap();
        // a = (960,540); B's rect is [1920, 3840) × [0, 1080): nearest point x = 1920 → p = (0, 540)
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (0.0, 540.0)
            }
        );
        c.output_disconnected(&mut t, b).unwrap();
        assert_eq!(c.pointer(), PointerState::NoOutput);
    }

    /// The nearer of two survivors wins, and only the interaction output's loss relocates.
    /// Arrangement: A [0,1920), B [1920,3840), C [3840,5760), D [5760,7680); pointer (960,540) on A.
    #[test]
    fn relocation_picks_the_nearest_survivor() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let (b, _) = c.output_connected(&mut t, desc("B")).unwrap();
        let (_c3, _) = c.output_connected(&mut t, desc("C")).unwrap();
        let (d, _) = c.output_connected(&mut t, desc("D")).unwrap();
        c.output_disconnected(&mut t, d).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: a,
                p: (960.0, 540.0)
            },
            "not the interaction output"
        );
        // survivors B (distance 960) and C (distance 2880): B, at its left edge
        c.output_disconnected(&mut t, a).unwrap();
        assert_eq!(
            c.pointer(),
            PointerState::Located {
                output: b,
                p: (0.0, 540.0)
            }
        );
    }

    /// A disconnected output's ProjectionFrame does not survive: no hit-test and no frame for a
    /// stale OutputId.
    #[test]
    fn disconnect_drops_the_output_frame() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let id = c.toplevel_created();
        let tree = |mapped| crate::snapshot::SurfaceTree {
            geometry: crate::geom::LocalRect {
                loc: crate::geom::LocalPoint::default(),
                size: crate::geom::LocalSize { w: 100, h: 100 },
            },
            surfaces: vec![crate::snapshot::SnapshotSurface {
                key: crate::snapshot::tests::key(1),
                role: crate::snapshot::SurfaceRole::Root,
                size: crate::geom::LocalSize { w: 100, h: 100 },
                mapped,
                input: crate::snapshot::InputRegion::Infinite,
            }],
        };
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
        c.render_list(a).unwrap();
        let p = crate::geom::OutputLogicalPoint { x: 970.5, y: 550.5 };
        assert!(c.hit_test(a, p).is_some());
        c.output_disconnected(&mut t, a).unwrap();
        assert_eq!(c.hit_test(a, p), None, "a stale frame survived its output");
        assert_eq!(
            c.render_list(a).err(),
            Some(crate::FrameError::UnknownOutput)
        );
    }

    /// INC2-F07: A ProjectionFrame belongs strictly to the output/view configuration from which it
    /// was built and must not survive output_reconfigured. Before reconfiguration, p_before hits
    /// the frame; reconfiguring mode, transform or scale invalidates the frame so that the stale
    /// frame is not used by a subsequent hit-test.
    #[test]
    fn reconfiguration_drops_the_output_frame() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        let (a, _) = c.output_connected(&mut t, desc("A")).unwrap();
        let id = c.toplevel_created();
        let tree = |mapped| crate::snapshot::SurfaceTree {
            geometry: crate::geom::LocalRect {
                loc: crate::geom::LocalPoint::default(),
                size: crate::geom::LocalSize { w: 100, h: 100 },
            },
            surfaces: vec![crate::snapshot::SnapshotSurface {
                key: crate::snapshot::tests::key(1),
                role: crate::snapshot::SurfaceRole::Root,
                size: crate::geom::LocalSize { w: 100, h: 100 },
                mapped,
                input: crate::snapshot::InputRegion::Infinite,
            }],
        };
        c.toplevel_committed(id, tree(false)).unwrap();
        c.toplevel_committed(id, tree(true)).unwrap();
        c.render_list(a).unwrap();
        // At 1920x1080 scale 1.0, window [0,100)x[0,100) is at OutputLogical [960,1060)x[540,640).
        let p_before = crate::geom::OutputLogicalPoint { x: 970.5, y: 550.5 };
        assert!(c.hit_test(a, p_before).is_some());

        // 1. Mode shrink: 800x600. p_before (970.5, 550.5) is outside the new extent.
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (800, 600), Transform::Normal, 120, 60_000),
        )
        .unwrap();
        assert_eq!(
            c.hit_test(a, p_before),
            None,
            "stale frame survived mode reconfiguration"
        );

        // Populate new frame for 800x600: window is at logical [400,500)x[300,400).
        let p_800 = crate::geom::OutputLogicalPoint { x: 410.5, y: 310.5 };
        assert!(c.hit_test(a, p_800).is_some());

        // 2. Transform change: R90 on 1920x1080 -> view_size (1080, 1920).
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (1920, 1080), Transform::R90, 120, 60_000),
        )
        .unwrap();
        // Window on R90 is centered in 1080x1920: [540,640)x[960,1060). p_800 (410.5, 310.5) misses:
        assert_eq!(
            c.hit_test(a, p_800),
            None,
            "stale frame survived transform reconfiguration"
        );

        // 3. Scale change: scale 2.0 (scale_120 = 240) on 1920x1080 Normal -> logical extent 960x540.
        c.output_reconfigured(
            &mut t,
            a,
            desc_k(key("A"), (1920, 1080), Transform::Normal, 240, 60_000),
        )
        .unwrap();
        assert_eq!(
            c.hit_test(a, p_before),
            None,
            "stale frame survived scale reconfiguration"
        );
    }

    /// An invalid descriptor creates nothing and allocates no id.
    #[test]
    fn invalid_descriptor_is_rejected_without_mutation() {
        let (mut c, _, _, mut t) = Core::new(PersistedCameras::default());
        assert_eq!(
            c.output_connected(
                &mut t,
                desc_k(key("A"), (0, 1080), Transform::Normal, 120, 60_000)
            )
            .err(),
            Some(EntryError::InvalidDescriptor)
        );
        assert!(c.cameras().is_empty() && c.outputs().is_empty());
        assert_eq!(c.pointer(), PointerState::NoOutput);
        let (o, _) = c.output_connected(&mut t, desc("A")).unwrap();
        assert_eq!(
            o,
            OutputId::for_test(1),
            "the rejected connect allocated an id"
        );
    }
}
