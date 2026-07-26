//! Sidebar mouse: click selects the row with Enter semantics (folders
//! open, groups toggle), the wheel moves the cursor, motion tracks a
//! hovered row.

use bevy::prelude::*;
use plurimus::{UiEvent, UiHovered, Widget};

use super::render::SidebarWindow;
use super::{SidebarRows, SidebarState, SidebarWidget};
use crate::mouse::{is_modal_open, local_event};

pub(super) fn handle(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    let hidden = {
        let window = world
            .get::<Widget>(entity)
            .and_then(|widget| widget.get_state::<SidebarWindow>().ok());
        !window.is_some_and(SidebarWindow::is_visible)
    };
    if hidden || is_modal_open(world) {
        return Ok(());
    }
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        super::move_cursor(world, motion);
        return Ok(());
    }
    let row = {
        let window = world.get::<Widget>(entity).map(|widget| {
            widget
                .get_state::<SidebarWindow>()
                .map(SidebarWindow::top)
                .unwrap_or_default()
        });
        window.unwrap_or_default() + usize::from(local.row)
    };
    if local.is_move() {
        return set_hover(world, entity, Some(row));
    }
    if local.is_left_click() && row < world.resource::<SidebarRows>().0.len() {
        set_hover(world, entity, None)?;
        world.resource_mut::<SidebarState>().selected = row;
        super::select(world);
    }
    Ok(())
}

fn set_hover(world: &mut World, entity: Entity, row: Option<usize>) -> Result {
    let Some(mut widget) = world.get_mut::<Widget>(entity) else {
        return Ok(());
    };
    let window = widget.get_state_mut::<SidebarWindow>()?;
    window.set_hovered(row);
    Ok(())
}

/// The pointer left the sidebar: drop the row highlight.
pub(super) fn clear_departed_hover(
    mut widgets: Query<&mut Widget, (With<SidebarWidget>, Without<UiHovered>)>,
) {
    for mut widget in &mut widgets {
        let has_hover = widget
            .get_state::<SidebarWindow>()
            .is_ok_and(SidebarWindow::has_hover);
        if has_hover && let Ok(window) = widget.get_state_mut::<SidebarWindow>() {
            window.set_hovered(None);
        }
    }
}
