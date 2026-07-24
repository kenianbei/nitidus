use bevy::prelude::Resource;

use super::states::ThemeColorStates;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub default: ThemeColorStates,
    pub error: ThemeColorStates,
    pub info: ThemeColorStates,
    pub success: ThemeColorStates,
    pub warning: ThemeColorStates,
}

/// `base` styles the app chrome; `paper` styles raised surfaces
/// (popups, dialogs, buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource)]
pub struct Theme {
    pub base: ThemePalette,
    pub paper: ThemePalette,
}
