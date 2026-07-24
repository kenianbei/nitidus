//! Which screen owns the content region. Drives both widget visibility
//! and the router's Normal-mode keymap context.

use bevy::prelude::Resource;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    #[default]
    Index,
    Pager,
}
