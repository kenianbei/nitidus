//! Contact-book mouse: a click in the table pane selects the contact
//! row under the cursor (and focuses the table); the wheel drives the
//! focused pane's cursor.

use bevy::prelude::*;
use plurimus::{UiEvent, WidgetRect};

use super::render::TABLE_PANE_PERCENT;
use super::view::{ContactsView, PaneFocus};
use super::{ContactStore, view};
use crate::mouse::{is_modal_open, local_event};
use crate::screen::Screen;

pub(super) fn handle(world: &mut World, entity: Entity, event: UiEvent) -> Result {
    if *world.resource::<Screen>() != Screen::Contacts || is_modal_open(world) {
        return Ok(());
    }
    let Some(local) = local_event(world, entity, event) else {
        return Ok(());
    };
    if let Some(motion) = local.wheel_motion() {
        view::move_cursor(world, motion);
        return Ok(());
    }
    if !local.is_left_click() || !is_in_table_pane(world, entity, local.column) {
        return Ok(());
    }
    let total = world.resource::<ContactStore>().0.len();
    let mut contacts_view = world.resource_mut::<ContactsView>();
    let row = contacts_view.table_top + usize::from(local.row);
    if row >= total {
        return Ok(());
    }
    contacts_view.focus = PaneFocus::Table;
    contacts_view.selected = row;
    contacts_view.detail_selected = 0;
    contacts_view.detail_top = 0;
    Ok(())
}

fn is_in_table_pane(world: &World, entity: Entity, local_column: u16) -> bool {
    let Some(rect) = world.get::<WidgetRect>(entity).map(|rect| rect.0) else {
        return false;
    };
    local_column < rect.width * TABLE_PANE_PERCENT / 100
}
