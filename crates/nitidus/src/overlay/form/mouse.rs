//! Form mouse handling. These run inside plurimus's `run_world_intents`,
//! the same step the key router runs in, so a click moves focus on the
//! synchronous path rather than a frame later.

use bevy::prelude::*;
use plurimus::UiEvent;

use super::entity::{FormButtonControl, FormFieldControl, FormStepControl};
use super::state::Focus;
use super::{ActiveForm, activate, go_to_page};
use crate::mouse::local_event;

pub(super) fn handle_field(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if !local.is_left_click() {
        return Ok(());
    }
    let Some(&FormFieldControl(index)) = world.get::<FormFieldControl>(entity) else {
        return Ok(());
    };
    set_focus(world, Focus::Field(index));
    Ok(())
}

/// Buttons fire on release, and only when the pointer is still over
/// them — plurimus routes the release to whichever control captured the
/// press, wherever it ended up.
pub(super) fn handle_button(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    let Some(&FormButtonControl(index)) = world.get::<FormButtonControl>(entity) else {
        return Ok(());
    };
    if local.is_left_click() {
        set_focus(world, Focus::Button(index));
        return Ok(());
    }
    if local.is_left_release() {
        set_focus(world, Focus::Button(index));
        activate(world);
    }
    Ok(())
}

fn set_focus(world: &mut World, focus: Focus) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.set_focus(focus);
    }
}

/// Clicking a step jumps to it. An unreached step carries `UiDisabled`,
/// so plurimus does not route the click here at all.
pub(super) fn handle_step(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if !local.is_left_click() {
        return Ok(());
    }
    let Some(&FormStepControl(index)) = world.get::<FormStepControl>(entity) else {
        return Ok(());
    };
    go_to_page(world, index);
    Ok(())
}
