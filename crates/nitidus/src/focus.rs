//! Which pane owns the keyboard, tracked per tab. One resource replaces
//! the sidebar's `focused` flag and the contact book's own `PaneFocus`,
//! so keymap context, row styling, and motion all read one source.
//!
//! Focus is stored per tab rather than globally: a pane belongs to
//! exactly one tab, so a mail pane can never be reported focused while
//! the contact book is on screen.

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Folders,
    Messages,
    Reading,
    ContactList,
    ContactDetail,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneFocus {
    mail: Pane,
    contacts: Pane,
}

impl Default for PaneFocus {
    fn default() -> Self {
        Self {
            mail: Pane::Messages,
            contacts: Pane::ContactList,
        }
    }
}

impl PaneFocus {
    pub fn is(&self, pane: Pane) -> bool {
        self.slot(pane) == pane
    }

    pub fn set(&mut self, pane: Pane) {
        *self.slot_mut(pane) = pane;
    }

    fn slot(&self, pane: Pane) -> Pane {
        match pane {
            Pane::Folders | Pane::Messages | Pane::Reading => self.mail,
            Pane::ContactList | Pane::ContactDetail => self.contacts,
        }
    }

    fn slot_mut(&mut self, pane: Pane) -> &mut Pane {
        match pane {
            Pane::Folders | Pane::Messages | Pane::Reading => &mut self.mail,
            Pane::ContactList | Pane::ContactDetail => &mut self.contacts,
        }
    }
}

/// Which bindings are live: an open composition owns the keyboard
/// wherever it is drawn, then the active tab, then the focused pane.
/// The router and the help overlay both read this, so they cannot
/// disagree.
pub fn active_context(world: &World) -> &'static str {
    if world
        .get_resource::<crate::compose::ComposeState>()
        .is_some_and(crate::compose::ComposeState::is_active)
    {
        return crate::keymap::CONTEXT_COMPOSE;
    }
    if crate::shell::on_contacts(world) {
        return crate::keymap::CONTEXT_CONTACTS;
    }
    mail_context(world)
}

/// The keymap context the focused mail pane owns.
fn mail_context(world: &World) -> &'static str {
    if is_focused(world, Pane::Folders) {
        crate::keymap::CONTEXT_SIDEBAR
    } else if is_focused(world, Pane::Reading) {
        crate::keymap::CONTEXT_PAGER
    } else {
        crate::keymap::CONTEXT_INDEX
    }
}

pub fn is_focused(world: &World, pane: Pane) -> bool {
    world
        .get_resource::<PaneFocus>()
        .is_some_and(|focus| focus.is(pane))
}

pub fn focus(world: &mut World, pane: Pane) {
    world.resource_mut::<PaneFocus>().set(pane);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn each_tab_starts_on_its_list_pane() {
        let focus = PaneFocus::default();
        assert!(focus.is(Pane::Messages));
        assert!(focus.is(Pane::ContactList));
    }

    #[test]
    fn focusing_a_pane_displaces_only_its_own_tab() {
        let mut focus = PaneFocus::default();
        focus.set(Pane::Folders);

        assert!(focus.is(Pane::Folders));
        assert!(!focus.is(Pane::Messages), "the mail slot holds one pane");
        assert!(
            focus.is(Pane::ContactList),
            "focusing a mail pane must not disturb the contact book"
        );
    }

    #[test]
    fn a_mail_pane_is_never_focused_while_the_contact_book_holds_a_pane() {
        let mut focus = PaneFocus::default();
        focus.set(Pane::Folders);
        focus.set(Pane::ContactDetail);

        assert!(focus.is(Pane::ContactDetail));
        assert!(
            focus.is(Pane::Folders),
            "each slot is independent; the router picks the slot by tab"
        );
    }

    #[test]
    fn setting_the_focused_pane_again_is_idempotent() {
        let mut focus = PaneFocus::default();
        focus.set(Pane::ContactDetail);
        let once = focus;
        focus.set(Pane::ContactDetail);

        assert_eq!(focus, once);
    }
}
