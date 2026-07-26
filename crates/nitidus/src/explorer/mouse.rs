//! Explorer mouse: the wheel moves the selection, a click selects the
//! row under the cursor; the row math mirrors `render_explorer`.

use bevy::prelude::*;
use plurimus::{UiEvent, WidgetRect};

use super::{ExplorerState, scrolled_window_top};
use crate::action::Motion;
use crate::mouse::local_event;

pub(super) fn handle(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    if !world.resource::<ExplorerState>().is_open() {
        return Ok(());
    }
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        let input = match motion {
            Motion::Prev => ratatui_explorer::Input::Up,
            _ => ratatui_explorer::Input::Down,
        };
        forward_input(world, input);
        return Ok(());
    }
    if !local.is_left_click() || local.row == 0 {
        return Ok(());
    }
    let Some(row) = clicked_row(world, entity, usize::from(local.row - 1)) else {
        return Ok(());
    };
    let mut state = world.resource_mut::<ExplorerState>();
    if let Some(active) = state.0.as_mut() {
        active.explorer.set_selected_idx(row);
    }
    Ok(())
}

fn forward_input(world: &mut World, input: ratatui_explorer::Input) {
    let mut state = world.resource_mut::<ExplorerState>();
    if let Some(active) = state.0.as_mut()
        && let Err(error) = active.explorer.handle(input)
    {
        tracing::warn!("explorer mouse input failed: {error}");
    }
}

/// Border row 0 excluded by the caller; `offset` is rows below it.
fn clicked_row(world: &World, entity: Entity, offset: usize) -> Option<usize> {
    let viewport = usize::from(
        world
            .get::<WidgetRect>(entity)
            .map_or(0, |rect| rect.0.height.saturating_sub(2)),
    )
    .max(1);
    let state = world.resource::<ExplorerState>();
    let active = state.0.as_ref()?;
    let total = active.explorer.files().len();
    let row = scrolled_window_top(active.explorer.selected_idx(), viewport, total) + offset;
    (row < total).then_some(row)
}
