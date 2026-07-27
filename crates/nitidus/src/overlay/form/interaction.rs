//! Pointer markers onto the form's own control views. The visual
//! vocabulary itself is shared — see `overlay::interaction`.

use bevy::prelude::*;
use plurimus::{UiDisabled, UiHovered, UiPressed, Widget};

pub(super) use crate::overlay::interaction::{Interaction, Visual};

use super::render::{ButtonView, FieldView};

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
