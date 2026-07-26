//! Maps plurimus's interaction markers onto a visual variant, so every
//! control on an overlay styles itself the same way from one system
//! rather than each widget wiring up its own.
//!
//! Two writers share a control's view state and never overlap: this
//! module owns what the pointer knows, and `refresh_form` owns keyboard
//! focus, which the form — not plurimus — decides.

use bevy::prelude::*;
use nitidus_ui_kit::theme::{ThemeColorStates, ThemeColors};
use plurimus::{UiDisabled, UiHovered, UiPressed, Widget};

use super::render::{ButtonView, FieldView};

/// What the pointer is doing to a control this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Interaction {
    pub(super) hovered: bool,
    pub(super) pressed: bool,
    pub(super) disabled: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Visual {
    #[default]
    Normal,
    Hovered,
    Focused,
    Pressed,
    Disabled,
}

impl Visual {
    /// Focus outranks hover deliberately: the user must be able to see
    /// what Enter will hit, even with the pointer resting elsewhere on
    /// the form.
    pub(super) fn resolve(focused: bool, interaction: Interaction) -> Self {
        if interaction.disabled {
            Self::Disabled
        } else if interaction.pressed {
            Self::Pressed
        } else if focused {
            Self::Focused
        } else if interaction.hovered {
            Self::Hovered
        } else {
            Self::Normal
        }
    }

    pub(super) fn colors(self, states: &ThemeColorStates) -> ThemeColors {
        match self {
            Self::Normal => states.normal,
            Self::Hovered => states.hovered,
            Self::Focused => states.focused,
            Self::Pressed => states.pressed,
            Self::Disabled => states.disabled,
        }
    }
}

/// Reads the markers plurimus derived from pointer hit-testing and
/// stores them on whichever control view the entity carries.
pub(super) fn sync_interaction(
    mut controls: Query<(&mut Widget, Has<UiHovered>, Has<UiPressed>, Has<UiDisabled>)>,
) {
    for (mut widget, hovered, pressed, disabled) in &mut controls {
        let interaction = Interaction {
            hovered,
            pressed,
            disabled,
        };
        if let Ok(button) = widget.get_state_mut::<ButtonView>() {
            button.interaction = interaction;
            continue;
        }
        if let Ok(field) = widget.get_state_mut::<FieldView>() {
            field.interaction = interaction;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const HOVERED: Interaction = Interaction {
        hovered: true,
        pressed: false,
        disabled: false,
    };

    #[test]
    fn quiet_controls_are_normal() {
        assert_eq!(
            Visual::resolve(false, Interaction::default()),
            Visual::Normal
        );
    }

    #[test]
    fn focus_outranks_hover() {
        assert_eq!(Visual::resolve(true, HOVERED), Visual::Focused);
        assert_eq!(Visual::resolve(false, HOVERED), Visual::Hovered);
    }

    #[test]
    fn press_outranks_focus_and_disabled_outranks_everything() {
        let pressed = Interaction {
            pressed: true,
            ..HOVERED
        };
        assert_eq!(Visual::resolve(true, pressed), Visual::Pressed);
        let disabled = Interaction {
            disabled: true,
            ..pressed
        };
        assert_eq!(Visual::resolve(true, disabled), Visual::Disabled);
    }

    #[test]
    fn every_variant_maps_to_its_own_theme_slot() {
        let states = ThemeColorStates::derive(ThemeColors::new(
            nitidus_ui_kit::theme::ThemeColor::new(30, 41, 59),
            nitidus_ui_kit::theme::ThemeColor::new(203, 213, 225),
        ));
        assert_eq!(Visual::Normal.colors(&states), states.normal);
        assert_eq!(Visual::Hovered.colors(&states), states.hovered);
        assert_eq!(Visual::Focused.colors(&states), states.focused);
        assert_eq!(Visual::Pressed.colors(&states), states.pressed);
        assert_eq!(Visual::Disabled.colors(&states), states.disabled);
    }
}
