//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! Each entry point first checks that its port or token belongs to this Core. INC-2 state:
//! * the three output-lifecycle entries implement M4/M5 for any number of outputs (startup and
//!   hotplug alike, reconfiguration, disconnection, the zero-output state);
//! * input and shell vocabularies have no variants yet (INC-3, INC-11).

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

    pub fn physical_input(
        &mut self,
        port: &mut InputPort,
        ev: PhysicalEvent,
    ) -> Result<(), EntryError> {
        self.check(port.core_id)?;
        match ev {}
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
        if self.pointer == PointerState::NoOutput {
            self.pointer = pointer::startup(&model);
        }
        self.outputs.push(model);
        let mut fx = vec![Effect::OutputAdded(id)];
        fx.extend(self.visibility_pass());
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
        if let PointerState::Located { output: o, p } = self.pointer
            && o == output
        {
            self.pointer = match self.outputs.split_first() {
                Some((first, rest)) => pointer::relocate(&lost, p, first, rest),
                None => PointerState::NoOutput,
            };
        }
        let mut fx = self.visibility_pass();
        fx.push(Effect::OutputRemoved(output));
        Ok(fx)
    }
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
        assert_eq!(fx, vec![Effect::OutputAdded(a)]);
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
        assert_eq!(fx, vec![Effect::OutputRemoved(a)]);
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
