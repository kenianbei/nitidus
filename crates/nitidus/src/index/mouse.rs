//! Index mouse: click selects the row under the cursor (a click on the
//! selected row opens it), the wheel moves the cursor, and motion
//! tracks the hovered row for the theme's hovered state.

use bevy::prelude::*;
use plurimus::{UiEvent, UiHovered, Widget};

use super::{IndexWidget, IndexWindowState};
use crate::mouse::{is_modal_open, local_event};

pub(super) fn handle(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    if crate::shell::on_contacts(world) || is_modal_open(world) {
        return Ok(());
    }
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        super::move_cursor(world, motion);
        return Ok(());
    }
    let Some(row) = absolute_row(world, entity, local.row) else {
        return Ok(());
    };
    if local.is_move() {
        return set_hover(world, entity, Some(row));
    }
    if local.is_left_click() {
        set_hover(world, entity, None)?;
        super::ops::click_row(world, row);
    }
    Ok(())
}

fn absolute_row(world: &World, entity: Entity, local_row: u16) -> Option<usize> {
    let state = world
        .get::<Widget>(entity)?
        .get_state::<IndexWindowState>()
        .ok()?;
    Some(state.window_top + usize::from(local_row))
}

fn set_hover(world: &mut World, entity: Entity, row: Option<usize>) -> Result {
    let Some(mut widget) = world.get_mut::<Widget>(entity) else {
        return Ok(());
    };
    let state = widget.get_state_mut::<IndexWindowState>()?;
    if state.hovered_row != row {
        state.hovered_row = row;
    }
    Ok(())
}

/// The pointer left the widget (plurimus removed `UiHovered`): drop
/// the row highlight.
pub(super) fn clear_departed_hover(
    mut widgets: Query<&mut Widget, (With<IndexWidget>, Without<UiHovered>)>,
) {
    for mut widget in &mut widgets {
        let has_hover = widget
            .get_state::<IndexWindowState>()
            .is_ok_and(|state| state.hovered_row.is_some());
        if has_hover && let Ok(state) = widget.get_state_mut::<IndexWindowState>() {
            state.hovered_row = None;
        }
    }
}
