use bevy::prelude::Resource;
use ratatui::style::Style;

use super::states::ThemeColorStates;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemePalette {
    pub default: ThemeColorStates,
    pub error: ThemeColorStates,
    pub info: ThemeColorStates,
    pub success: ThemeColorStates,
    pub warning: ThemeColorStates,
}

/// Patches composed over a message-index row's base style, so a theme
/// owns row appearance without the renderer hardcoding modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeIndexStyles {
    pub unseen: Style,
    pub flagged: Style,
    pub deleted: Style,
    pub marked: Style,
    /// The row whose message is in the reading pane, which is not
    /// always the row under the cursor.
    pub reading: Style,
    /// Banding for alternate rows; the base every other patch composes
    /// over, so it must stay below every interaction state.
    pub stripe: Style,
    /// Typographic hierarchy inside one card: the sender line leads,
    /// the date line recedes. Flag roles patch over both, so state
    /// always outranks emphasis.
    pub sender: Style,
    pub date: Style,
}

/// `base` styles the app chrome; `paper` styles raised surfaces
/// (popups, dialogs, buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource)]
pub struct Theme {
    pub base: ThemePalette,
    pub paper: ThemePalette,
    pub index: ThemeIndexStyles,
}
