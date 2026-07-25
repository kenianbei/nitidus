//! Marking for batch operations: sticky `<Space>` marks, a `v` visual
//! range, `t` whole-thread marks, `Esc` clears. The marked set a batch
//! verb consumes is the sticky marks plus the live visual range, in
//! visible order. Marks are per-view working state: switching folders
//! clears them (durable tags are phase 2).

use std::collections::HashSet;

use bevy::prelude::*;
use nitidus_mail::{EnvelopeId, FolderId};

use super::{IndexOrder, IndexView, current_envelopes};
use crate::action::Motion;
use crate::store::MailStore;

/// `<Space>` — toggle the selection's mark and advance one row.
pub fn toggle_mark(world: &mut World) {
    let Some(id) = world.resource::<IndexView>().selected.clone() else {
        return;
    };
    {
        let mut view = world.resource_mut::<IndexView>();
        if !view.marked.remove(&id) {
            view.marked.insert(id);
        }
    }
    super::move_cursor(world, Motion::Next);
}

/// `v` — anchor a visual range at the selection; `v` again drops it.
pub fn toggle_visual(world: &mut World) {
    let mut view = world.resource_mut::<IndexView>();
    view.visual_anchor = match view.visual_anchor {
        Some(_) => None,
        None => Some(view.selected_row),
    };
}

/// `Esc` — clear every mark and the visual anchor.
pub fn unmark_all(world: &mut World) {
    let mut view = world.resource_mut::<IndexView>();
    view.marked.clear();
    view.visual_anchor = None;
}

/// `t` — mark the selection's whole thread (unmark when the thread is
/// already fully marked).
pub fn mark_thread(world: &mut World) {
    let thread_ids = selected_thread_ids(world);
    if thread_ids.is_empty() {
        return;
    }
    let mut view = world.resource_mut::<IndexView>();
    let all_marked = thread_ids.iter().all(|id| view.marked.contains(id));
    for id in thread_ids {
        if all_marked {
            view.marked.remove(&id);
        } else {
            view.marked.insert(id);
        }
    }
}

/// The set a batch verb consumes: sticky marks ∪ visual range, in
/// visible order. Empty means "use the single selection".
pub fn batch_ids(world: &World) -> Vec<EnvelopeId> {
    let view = world.resource::<IndexView>();
    let Some(order) = world.get_resource::<IndexOrder>() else {
        return Vec::new();
    };
    let store = world.resource::<MailStore>();
    let envelopes = current_envelopes(store, view);
    let visual = visual_rows(view);
    order
        .entries
        .iter()
        .enumerate()
        .filter_map(|(row, entry)| {
            let envelope = envelopes.get(entry.index as usize)?;
            let in_visual = visual.as_ref().is_some_and(|range| range.contains(&row));
            (in_visual || view.marked.contains(&envelope.id)).then(|| envelope.id.clone())
        })
        .collect()
}

/// Row range of the live visual selection, if any.
pub(super) fn visual_rows(view: &IndexView) -> Option<std::ops::RangeInclusive<usize>> {
    let anchor = view.visual_anchor?;
    Some(anchor.min(view.selected_row)..=anchor.max(view.selected_row))
}

/// Threaded mode: the contiguous depth-block around the selection.
/// Flat mode: everything sharing the selection's reference chain.
fn selected_thread_ids(world: &World) -> Vec<EnvelopeId> {
    let view = world.resource::<IndexView>();
    let Some(order) = world.get_resource::<IndexOrder>() else {
        return Vec::new();
    };
    let store = world.resource::<MailStore>();
    let envelopes = current_envelopes(store, view);
    if view.threaded {
        let Some(block) = thread_block(order, view.selected_row) else {
            return Vec::new();
        };
        return block
            .filter_map(|row| {
                order
                    .entries
                    .get(row)
                    .and_then(|entry| envelopes.get(entry.index as usize))
                    .map(|envelope| envelope.id.clone())
            })
            .collect();
    }
    let Some(selected) = view
        .selected
        .as_ref()
        .and_then(|id| envelopes.iter().find(|envelope| &envelope.id == id))
    else {
        return Vec::new();
    };
    let mut chain: HashSet<&str> = selected.references.iter().map(String::as_str).collect();
    if !selected.message_id.is_empty() {
        chain.insert(&selected.message_id);
    }
    envelopes
        .iter()
        .filter(|envelope| {
            (!envelope.message_id.is_empty() && chain.contains(envelope.message_id.as_str()))
                || envelope
                    .references
                    .iter()
                    .any(|reference| chain.contains(reference.as_str()))
        })
        .map(|envelope| envelope.id.clone())
        .collect()
}

/// Walk out to the depth-0 root, then forward to the next root.
fn thread_block(order: &IndexOrder, selected_row: usize) -> Option<std::ops::Range<usize>> {
    let entries = &order.entries;
    entries.get(selected_row)?;
    let start = (0..=selected_row)
        .rev()
        .find(|&row| entries[row].depth == 0)?;
    let end = (selected_row + 1..entries.len())
        .find(|&row| entries[row].depth == 0)
        .unwrap_or(entries.len());
    Some(start..end)
}

/// Marks are per-folder working state — a folder switch clears them.
/// Reads go through `as_ref` so an unchanged frame flags nothing.
pub(super) fn clear_marks_on_folder_change(
    mut view: ResMut<IndexView>,
    mut last_folder: Local<Option<FolderId>>,
) {
    let current = view.as_ref();
    if last_folder.as_ref() == Some(&current.folder) {
        return;
    }
    let had_marks = !current.marked.is_empty() || current.visual_anchor.is_some();
    *last_folder = Some(current.folder.clone());
    if !had_marks {
        return;
    }
    view.marked.clear();
    view.visual_anchor = None;
}
