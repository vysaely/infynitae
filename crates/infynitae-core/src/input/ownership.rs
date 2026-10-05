//! OBJ-25: the button ownership table (INV-INPUT-1). Every pressed button has exactly one owner,
//! decided at press; its release follows that owner, whatever happened in between (output
//! removal, the zero-output state, the end of an interaction).

/// Who owns a pressed button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Delivered to the client (Smithay's implicit grab follows it).
    Client,
    /// Consumed by the compositor, press and release.
    Compositor,
}

#[derive(Debug, Default)]
pub struct Ownership {
    pressed: Vec<(u32, Owner)>,
}

impl Ownership {
    pub fn owner(&self, button: u32) -> Option<Owner> {
        self.pressed
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, o)| *o)
    }

    /// INV-INPUT-2: whether the client owns any pressed button.
    pub fn client_owns_any(&self) -> bool {
        self.pressed.iter().any(|(_, o)| *o == Owner::Client)
    }

    /// Records the owner decided at press. A button or key already pressed keeps its first owner.
    pub(crate) fn press(&mut self, button: u32, owner: Owner) -> bool {
        if self.owner(button).is_some() {
            return false;
        }
        self.pressed.push((button, owner));
        true
    }

    /// The owner the release follows; `None` for a button or key that was never pressed.
    pub(crate) fn release(&mut self, button: u32) -> Option<Owner> {
        let i = self.pressed.iter().position(|(b, _)| *b == button)?;
        Some(self.pressed.remove(i).1)
    }

    pub fn pressed(&self) -> &[(u32, Owner)] {
        &self.pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_follows_the_press_owner_and_a_repeat_press_is_ignored() {
        let mut t = Ownership::default();
        assert!(t.press(1, Owner::Client));
        assert!(!t.press(1, Owner::Compositor), "a repeat press re-decided");
        assert!(t.press(2, Owner::Compositor));
        assert!(t.client_owns_any());
        assert_eq!(t.release(1), Some(Owner::Client));
        assert!(!t.client_owns_any());
        assert_eq!(t.release(1), None);
        assert_eq!(t.release(2), Some(Owner::Compositor));
        assert!(t.pressed().is_empty());
    }
}
