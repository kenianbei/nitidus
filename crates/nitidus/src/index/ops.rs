//! World-mutating index operations, called from `apply_action`:
//! cursor motion, sort changes, and optimistic flag writes.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, Flags, FolderId, MailCommand};
use plurimus::Widget;

use super::{IndexOrder, IndexView, IndexWidget, IndexWindowState, SortMode, current_envelopes, view};
use crate::action::{FlagOp, Motion};
use crate::engine::EngineResource;
use crate::status::StatusMessage;
use crate::store::MailStore;

/// Page size when nothing has rendered yet (headless tests, first frame).
const FALLBACK_PAGE_ROWS: usize = 10;

pub fn move_cursor(world: &mut World, motion: Motion) {
    let page = viewport_rows(world).saturating_sub(1).max(1);
    let new_id = {
        let index_view = world.resource::<IndexView>();
        let store = world.resource::<MailStore>();
        let order = &world.resource::<IndexOrder>().order;
        let envelopes = current_envelopes(store, index_view);
        let Some(row) = view::resolve_selection(index_view, envelopes, order) else {
            return;
        };
        let new_row = view::apply_motion(row, order.len(), page, motion);
        order
            .get(new_row)
            .map(|&index| envelopes[index as usize].id.clone())
    };
    if new_id.is_some() {
        world.resource_mut::<IndexView>().selected = new_id;
    }
}

pub fn set_sort(world: &mut World, mode: SortMode) {
    world.resource_mut::<IndexView>().sort = mode;
}

pub fn flag_selected(world: &mut World, flag: Flags, op: FlagOp) {
    let index_view = world.resource::<IndexView>();
    let (Some(account), Some(id)) = (index_view.account.clone(), index_view.selected.clone())
    else {
        return;
    };
    let folder = index_view.folder.clone();
    let current = world
        .resource::<MailStore>()
        .envelopes(&account, &folder)
        .iter()
        .find(|envelope| envelope.id == id)
        .map(|envelope| envelope.flags);
    let Some(current) = current else { return };
    let updated = match op {
        FlagOp::Set => current.with(flag),
        FlagOp::Clear => current.without(flag),
        FlagOp::Toggle if current.contains(flag) => current.without(flag),
        FlagOp::Toggle => current.with(flag),
    };
    if updated == current {
        return;
    }
    world
        .resource_mut::<MailStore>()
        .set_flags(&account, &folder, &id, updated);
    send_flag_write(world, &account, folder, id, updated);
}

fn send_flag_write(
    world: &mut World,
    account: &AccountId,
    folder: FolderId,
    id: EnvelopeId,
    flags: Flags,
) {
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    if let Err(error) = engine
        .0
        .send(account, MailCommand::SetFlags { folder, id, flags })
    {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("flag write failed: {error}"), now);
    }
}

fn viewport_rows(world: &mut World) -> usize {
    let mut widgets = world.query_filtered::<&Widget, With<IndexWidget>>();
    widgets
        .iter(world)
        .find_map(|widget| {
            widget
                .get_state::<IndexWindowState>()
                .ok()
                .map(|state| usize::from(state.last_height))
        })
        .filter(|&height| height > 0)
        .unwrap_or(FALLBACK_PAGE_ROWS)
}
