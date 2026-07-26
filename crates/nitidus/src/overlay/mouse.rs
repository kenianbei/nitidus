//! Picker mouse: click selects the row under the cursor, the wheel
//! moves the selection, motion tracks a hovered row.

use bevy::prelude::*;
use plurimus::{UiEvent, UiHovered, Widget, WidgetRect};

use super::render::PickerWindow;
use super::{ActiveOverlay, PickerWidget};
use crate::mouse::local_event;

pub(super) fn handle(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    if !world.resource::<ActiveOverlay>().is_open() {
        return Ok(());
    }
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        super::move_selection(world, motion);
        return Ok(());
    }
    let row = row_at(world, entity, local.raw.row);
    if local.is_move() {
        return set_hover(world, entity, row);
    }
    if local.is_left_click()
        && let Some(row) = row
    {
        set_hover(world, entity, None)?;
        super::select_row(world, row);
    }
    Ok(())
}

/// Maps a screen y onto the picker's scrolled row list; the geometry
/// mirrors the renderer's exactly.
fn row_at(world: &World, entity: Entity, y: u16) -> Option<usize> {
    let rect = world.get::<WidgetRect>(entity)?.0;
    let window = world
        .get::<Widget>(entity)?
        .get_state::<PickerWindow>()
        .ok()?;
    let geometry = window.row_window(rect);
    if y < geometry.first_row_y {
        return None;
    }
    let offset = usize::from(y - geometry.first_row_y);
    (offset < geometry.visible).then_some(geometry.top + offset)
}

fn set_hover(world: &mut World, entity: Entity, row: Option<usize>) -> Result {
    let Some(mut widget) = world.get_mut::<Widget>(entity) else {
        return Ok(());
    };
    let window = widget.get_state_mut::<PickerWindow>()?;
    window.set_hovered(row);
    Ok(())
}

/// The pointer left the picker: drop the row highlight.
pub(super) fn clear_departed_hover(
    mut widgets: Query<&mut Widget, (With<PickerWidget>, Without<UiHovered>)>,
) {
    for mut widget in &mut widgets {
        let has_hover = widget
            .get_state::<PickerWindow>()
            .is_ok_and(PickerWindow::has_hover);
        if has_hover && let Ok(window) = widget.get_state_mut::<PickerWindow>() {
            window.set_hovered(None);
        }
    }
}
