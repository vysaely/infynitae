//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! Each entry point first checks that its port or token belongs to this Core. INC-1 state:
//! * `output_connected` connects one output and creates and binds its camera (M4 startup row,
//!   INV-CAMERA-3 rule 3); a second output is refused until INC-2 (multi-output, hotplug,
//!   binding precedence);
//! * `output_reconfigured` and `output_disconnected` refuse with [`EntryError::Unsupported`]
//!   until INC-2;
//! * input and shell vocabularies have no variants yet (INC-3, INC-11).

use crate::output::{OutputDescriptor, OutputId, OutputModel};
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

    pub fn output_connected(
        &mut self,
        token: &mut OutputLifecycleToken,
        desc: OutputDescriptor,
    ) -> Result<OutputId, EntryError> {
        self.check(token.core_id)?;
        if !self.outputs.is_empty() {
            return Err(EntryError::Unsupported);
        }
        let model = OutputModel::from_descriptor(self.output_ids.peek(), &desc)
            .ok_or(EntryError::InvalidDescriptor)?;
        let id = self.output_ids.next();
        self.cameras.create_bound(authority::Authorized::mint(), id);
        self.outputs.push(model);
        Ok(id)
    }

    /// Changes hardware facts of a connected output. Never a camera authority: mints no proof
    /// and calls no `navigation` item (E1R, checked by tools/verify-architecture).
    pub fn output_reconfigured(
        &mut self,
        token: &mut OutputLifecycleToken,
        _output: OutputId,
        _desc: OutputDescriptor,
    ) -> Result<(), EntryError> {
        self.check(token.core_id)?;
        Err(EntryError::Unsupported)
    }

    pub fn output_disconnected(
        &mut self,
        token: &mut OutputLifecycleToken,
        _output: OutputId,
    ) -> Result<(), EntryError> {
        self.check(token.core_id)?;
        Err(EntryError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use crate::geom::CanvasPos;
    use crate::output::{OutputDescriptor, OutputKey, Transform};
    use crate::{Core, EntryError};

    fn desc(mode: (u32, u32), scale_120: u32) -> OutputDescriptor {
        OutputDescriptor {
            key: OutputKey::opaque(),
            mode,
            transform: Transform::Normal,
            scale_120,
            refresh_mhz: 60_000,
        }
    }

    /// A token minted by another Core grants nothing (C1.5, C12.R2.3).
    #[test]
    fn tokens_are_bound_to_their_core() {
        let (mut a, _ia, _sa, _own) = Core::new();
        let (_b, _ib, _sb, mut foreign) = Core::new();
        assert_eq!(
            a.output_connected(&mut foreign, desc((1920, 1080), 120))
                .err(),
            Some(EntryError::ForeignPort)
        );
        assert!(a.cameras().is_empty(), "a foreign token created a camera");
        assert!(a.outputs().is_empty());
    }

    /// M4 startup row: the first connect creates and binds one camera at the default placement,
    /// zoom 1 (INV-CAMERA-3 rule 3). No camera exists before it.
    #[test]
    fn first_connect_creates_and_binds_one_camera() {
        let (mut core, _i, _s, mut token) = Core::new();
        assert!(core.cameras().is_empty());
        let o = core
            .output_connected(&mut token, desc((1920, 1080), 180))
            .unwrap();
        assert_eq!(core.cameras().len(), 1);
        assert_eq!(core.cameras().camera_for_output(o), Some(0));
        assert_eq!(core.cameras().center(0), Some(CanvasPos { x: 0, y: 0 }));
        assert_eq!(core.cameras().zoom(0), Some(1.0));
        assert_eq!(core.outputs().len(), 1);
        assert_eq!(core.outputs()[0].scale, 1.5);
    }

    /// INC-1 scope: a second output is refused and nothing changes (multi-output is INC-2).
    #[test]
    fn second_connect_is_refused_without_mutation() {
        let (mut core, _i, _s, mut token) = Core::new();
        let o = core
            .output_connected(&mut token, desc((1920, 1080), 120))
            .unwrap();
        assert_eq!(
            core.output_connected(&mut token, desc((2560, 1440), 120))
                .err(),
            Some(EntryError::Unsupported)
        );
        assert_eq!(core.cameras().len(), 1);
        assert_eq!(core.outputs().len(), 1);
        assert_eq!(core.outputs()[0].id, o);
    }

    /// An invalid descriptor creates nothing.
    #[test]
    fn invalid_descriptor_is_rejected_without_mutation() {
        let (mut core, _i, _s, mut token) = Core::new();
        assert_eq!(
            core.output_connected(&mut token, desc((0, 1080), 120))
                .err(),
            Some(EntryError::InvalidDescriptor)
        );
        assert!(core.cameras().is_empty());
        assert!(core.outputs().is_empty());
    }
}
