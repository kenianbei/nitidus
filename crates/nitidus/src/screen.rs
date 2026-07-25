//! Which screen owns the content region. Drives both widget visibility
//! and the router's Normal-mode keymap context.

use bevy::prelude::Resource;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    Compose,
    Contacts,
    #[default]
    Index,
    Pager,
}

/// The mail-tab screen (Index or Pager) to restore when tabbing back
/// from the contacts tab.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailScreenMemory(pub Screen);

impl Default for MailScreenMemory {
    fn default() -> Self {
        Self(Screen::Index)
    }
}
