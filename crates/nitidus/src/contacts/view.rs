//! Selection and pane-focus state for the contact book: a table cursor,
//! a detail cursor, and which pane the motions steer.

use bevy::prelude::*;

use super::ContactStore;
use super::detail;
use crate::action::Motion;
use crate::index::apply_motion;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneFocus {
    #[default]
    Table,
    Detail,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactsView {
    pub selected: usize,
    pub detail_selected: usize,
    pub focus: PaneFocus,
    pub table_top: usize,
    pub detail_top: usize,
    /// Viewport heights fed back from the last render, for page motions
    /// and scroll clamping.
    pub(super) table_viewport: usize,
    pub(super) detail_viewport: usize,
}

pub fn move_cursor(world: &mut World, motion: Motion) {
    let (table_total, detail_total) = pane_totals(world);
    let mut view = world.resource_mut::<ContactsView>();
    match view.focus {
        PaneFocus::Table => {
            if table_total == 0 {
                return;
            }
            let page = view.table_viewport.max(1);
            view.selected = apply_motion(view.selected, table_total, page, motion);
            // A new contact starts the detail cursor at the top.
            view.detail_selected = 0;
            view.detail_top = 0;
        }
        PaneFocus::Detail => {
            if detail_total == 0 {
                return;
            }
            let page = view.detail_viewport.max(1);
            view.detail_selected = apply_motion(view.detail_selected, detail_total, page, motion);
        }
    }
}

pub fn toggle_focus(world: &mut World) {
    let mut view = world.resource_mut::<ContactsView>();
    view.focus = match view.focus {
        PaneFocus::Table => PaneFocus::Detail,
        PaneFocus::Detail => PaneFocus::Table,
    };
}

fn pane_totals(world: &World) -> (usize, usize) {
    let book = &world.resource::<ContactStore>().0;
    let view = world.resource::<ContactsView>();
    let detail_total = book
        .get(view.selected)
        .map_or(0, |contact| detail::build_rows(contact).len());
    (book.len(), detail_total)
}
