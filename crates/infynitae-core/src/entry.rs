//! Port-carrying entry points (C1.5, INV-CAMERA-1, INV-OUTPUT-2). The only module in which an
//! `Authorized` proof can be minted.
//!
//! INC-0: each entry point checks that its port or token belongs to this Core and does nothing
//! else. Input and shell vocabularies have no variants; the output-lifecycle entries refuse with
//! [`EntryError::Unsupported`] until INC-2.

use crate::output::{OutputDescriptor, OutputId};
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
        #[expect(
            dead_code,
            reason = "INC-0 authority shape; first mint arrives with INC-2"
        )]
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
        _desc: OutputDescriptor,
    ) -> Result<OutputId, EntryError> {
        self.check(token.core_id)?;
        Err(EntryError::Unsupported)
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
    use crate::output::{OutputDescriptor, OutputKey, Transform};
    use crate::{Core, EntryError};

    fn desc() -> OutputDescriptor {
        OutputDescriptor {
            key: OutputKey::for_test(),
            mode: (1920, 1080),
            transform: Transform::Normal,
            scale_120: 120,
            refresh_mhz: 60_000,
        }
    }

    /// A token minted by another Core grants nothing (C1.5, C12.R2.3).
    #[test]
    fn tokens_are_bound_to_their_core() {
        let (mut a, _ia, _sa, mut own) = Core::new();
        let (_b, _ib, _sb, mut foreign) = Core::new();
        assert_eq!(
            a.output_connected(&mut foreign, desc()).err(),
            Some(EntryError::ForeignPort)
        );
        assert_eq!(
            a.output_connected(&mut own, desc()).err(),
            Some(EntryError::Unsupported)
        );
        assert_eq!(
            a.cameras().center(0),
            None,
            "an entry point created a camera"
        );
    }
}
