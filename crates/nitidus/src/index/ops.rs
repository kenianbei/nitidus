//! World-mutating index operations, called from `apply_action`:
//! cursor motion, sort changes, and optimistic flag writes.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, Flags, FolderId, MailCommand};
use plurimus::Widget;

use super::{
    IndexOrder, IndexView, IndexWidget, IndexWindowState, SortMode, current_envelopes, view,
};
use crate::action::{FlagOp, FoldOp, Motion};
use crate::engine::EngineResource;
use crate::status::StatusMessage;
use crate::store::{MailStore, ThreadSet};

/// Page size when nothing has rendered yet (headless tests, first frame).
const FALLBACK_PAGE_ROWS: usize = 10;

pub fn move_cursor(world: &mut World, motion: Motion) {
    if matches!(motion, Motion::Parent) {
        return move_to_parent(world);
    }
    let page = viewport_rows(world).saturating_sub(1).max(1);
    let new_id = {
        let index_view = world.resource::<IndexView>();
        let store = world.resource::<MailStore>();
        // Minimal harnesses route keys without the order resource.
        let Some(order) = world.get_resource::<IndexOrder>() else {
            return;
        };
        let entries = &order.entries;
        let envelopes = current_envelopes(store, index_view);
        let Some(row) = view::resolve_selection(index_view, envelopes, entries) else {
            return;
        };
        let new_row = view::apply_motion(row, entries.len(), page, motion);
        entries
            .get(new_row)
            .map(|entry| envelopes[entry.index as usize].id.clone())
    };
    if new_id.is_some() {
        world.resource_mut::<IndexView>().selected = new_id;
    }
}

fn move_to_parent(world: &mut World) {
    let parent = {
        let index_view = world.resource::<IndexView>();
        let threads = world.resource::<ThreadSet>();
        let (Some(account), Some(selected)) = (&index_view.account, &index_view.selected) else {
            return;
        };
        threads
            .rows(account, &index_view.folder)
            .and_then(|rows| rows.iter().find(|row| &row.id == selected))
            .and_then(|row| row.parent.clone())
    };
    if parent.is_some() {
        world.resource_mut::<IndexView>().selected = parent;
    }
}

pub fn set_sort(world: &mut World, mode: SortMode) {
    world.resource_mut::<IndexView>().sort = mode;
}

/// `,r` — flip the current sort's direction without changing its key.
pub fn reverse_sort(world: &mut World) {
    let mut index_view = world.resource_mut::<IndexView>();
    index_view.sort.reverse = !index_view.sort.reverse;
}

pub fn toggle_threads(world: &mut World) {
    let mut index_view = world.resource_mut::<IndexView>();
    index_view.threaded = !index_view.threaded;
    index_view.fold_epoch += 1;
}

pub fn fold(world: &mut World, op: FoldOp) {
    match op {
        FoldOp::Toggle => toggle_selected_fold(world),
        FoldOp::CollapseAll => set_all_folds(world, true),
        FoldOp::ExpandAll => set_all_folds(world, false),
    }
}

/// Collapsing keeps the cursor meaningful by moving it to the root the
/// selection just disappeared into.
fn toggle_selected_fold(world: &mut World) {
    let root = {
        let index_view = world.resource::<IndexView>();
        let threads = world.resource::<ThreadSet>();
        let (Some(account), Some(selected)) = (&index_view.account, &index_view.selected) else {
            return;
        };
        threads
            .rows(account, &index_view.folder)
            .and_then(|rows| rows.iter().find(|row| &row.id == selected))
            .map(|row| row.root.clone())
    };
    let Some(root) = root else { return };
    let mut index_view = world.resource_mut::<IndexView>();
    if !index_view.collapsed.remove(&root) {
        index_view.collapsed.insert(root.clone());
        index_view.selected = Some(root);
    }
    index_view.fold_epoch += 1;
}

fn set_all_folds(world: &mut World, collapse: bool) {
    let roots = {
        let index_view = world.resource::<IndexView>();
        let threads = world.resource::<ThreadSet>();
        let Some(account) = &index_view.account else {
            return;
        };
        if !collapse {
            Vec::new()
        } else {
            threads
                .rows(account, &index_view.folder)
                .map(|rows| {
                    rows.iter()
                        .filter(|row| row.depth == 0 && row.has_children)
                        .map(|row| row.root.clone())
                        .collect()
                })
                .unwrap_or_default()
        }
    };
    let mut index_view = world.resource_mut::<IndexView>();
    index_view.collapsed = roots.into_iter().collect();
    index_view.fold_epoch += 1;
}

pub fn flag_selected(world: &mut World, flag: Flags, op: FlagOp) {
    let batch = super::marks::batch_ids(world);
    if !batch.is_empty() {
        return flag_batch(world, flag, op, batch);
    }
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

/// Mouse: select the clicked visible row; a click on the row that is
/// already selected opens it.
pub(super) fn click_row(world: &mut World, row: usize) {
    let clicked = {
        let index_view = world.resource::<IndexView>();
        let Some(order) = world.get_resource::<IndexOrder>() else {
            return;
        };
        let envelopes = current_envelopes(world.resource::<MailStore>(), index_view);
        order
            .entries
            .get(row)
            .and_then(|entry| envelopes.get(entry.index as usize))
            .map(|envelope| envelope.id.clone())
    };
    let Some(id) = clicked else { return };
    if world.resource::<IndexView>().selected.as_ref() == Some(&id) {
        return crate::pager::open_selected(world);
    }
    let mut index_view = world.resource_mut::<IndexView>();
    index_view.selected = Some(id);
    index_view.selected_row = row;
}

/// Marks one exact message SEEN, ignoring batch marks — the pager's
/// mark-read path targets precisely the opened message.
pub(crate) fn mark_seen(
    world: &mut World,
    account: &AccountId,
    folder: &FolderId,
    id: &EnvelopeId,
) {
    let current = world
        .resource::<MailStore>()
        .envelopes(account, folder)
        .iter()
        .find(|envelope| &envelope.id == id)
        .map(|envelope| envelope.flags);
    let Some(current) = current else { return };
    if current.contains(Flags::SEEN) {
        return;
    }
    let updated = current.with(Flags::SEEN);
    world
        .resource_mut::<MailStore>()
        .set_flags(account, folder, id, updated);
    send_flag_write(world, account, folder.clone(), id.clone(), updated);
}

/// Toggles resolve per message; flags are their own undo, so the
/// writes go immediately and the marks are consumed.
fn flag_batch(world: &mut World, flag: Flags, op: FlagOp, ids: Vec<nitidus_mail::EnvelopeId>) {
    let index_view = world.resource::<IndexView>();
    let Some(account) = index_view.account.clone() else {
        return;
    };
    let folder = index_view.folder.clone();
    for id in ids {
        let current = world
            .resource::<MailStore>()
            .envelopes(&account, &folder)
            .iter()
            .find(|envelope| envelope.id == id)
            .map(|envelope| envelope.flags);
        let Some(current) = current else { continue };
        let updated = match op {
            FlagOp::Set => current.with(flag),
            FlagOp::Clear => current.without(flag),
            FlagOp::Toggle if current.contains(flag) => current.without(flag),
            FlagOp::Toggle => current.with(flag),
        };
        if updated == current {
            continue;
        }
        world
            .resource_mut::<MailStore>()
            .set_flags(&account, &folder, &id, updated);
        send_flag_write(world, &account, folder.clone(), id, updated);
    }
    super::marks::unmark_all(world);
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
