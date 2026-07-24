//! Builds the display entry list: flat mode wraps the sort permutation;
//! threaded mode walks `ThreadRow`s, resolves ids against the current
//! store slice, applies the sort to whole threads, and filters
//! collapsed subtrees down to their root.

use std::collections::HashSet;

use bevy::prelude::*;
use nitidus_mail::thread::ThreadRow;
use nitidus_mail::{EnvelopeId, EnvelopeSummary, Flags};

use super::view::{self, SortKey, SortMode};
use super::{IndexOrder, IndexView, current_envelopes};
use crate::engine::EngineResource;
use crate::store::{MailStore, SyncTracker, ThreadSet};

/// Requests a JWZ recompute whenever the viewed folder's id set has
/// changed and no scan is mid-flight (scan completion bumps the
/// generation, so this fires once per settle, not per batch).
pub(super) fn refresh_threads(
    index_view: Res<IndexView>,
    store: Res<MailStore>,
    tracker: Res<SyncTracker>,
    engine: Option<Res<EngineResource>>,
    mut threads: ResMut<ThreadSet>,
) {
    if !index_view.threaded {
        return;
    }
    let (Some(account), Some(engine)) = (&index_view.account, engine) else {
        return;
    };
    if tracker.in_flight_job(account, &index_view.folder).is_some() {
        return;
    }
    let generation = store.structure_generation(account, &index_view.folder);
    if !threads.needs_compute(account, &index_view.folder, generation) {
        return;
    }
    let job = engine.0.next_job();
    threads.begin(account.clone(), index_view.folder.clone(), job, generation);
    engine.0.compute_threads(
        account.clone(),
        index_view.folder.clone(),
        store.envelopes(account, &index_view.folder).to_vec(),
        job,
    );
}

pub(super) fn refresh_order(
    store: Res<MailStore>,
    index_view: Res<IndexView>,
    threads: Res<ThreadSet>,
    mut order: ResMut<IndexOrder>,
) {
    let key = (index_view.sort, index_view.threaded, index_view.fold_epoch);
    if !store.is_changed() && !threads.is_changed() && order.for_key == Some(key) {
        return;
    }
    let envelopes = current_envelopes(&store, &index_view);
    order.entries = build_entries(&index_view, &store, &threads, envelopes);
    order.for_key = Some(key);
}

/// Threaded mode falls back to the flat list until rows exist — the
/// first computation lands within a frame or two.
fn build_entries(
    index_view: &IndexView,
    store: &MailStore,
    threads: &ThreadSet,
    envelopes: &[EnvelopeSummary],
) -> Vec<OrderEntry> {
    if index_view.threaded
        && let Some(account) = &index_view.account
        && let Some(rows) = threads.rows(account, &index_view.folder)
    {
        return threaded_entries(
            rows,
            envelopes,
            index_view.sort,
            &index_view.collapsed,
            |id| store.position_of(account, &index_view.folder, id),
        );
    }
    flat_entries(envelopes, index_view.sort)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct OrderEntry {
    pub index: u32,
    pub depth: u8,
    /// Number of hidden descendants; non-zero only on a collapsed root.
    pub collapsed_children: u32,
}

pub(super) fn flat_entries(envelopes: &[EnvelopeSummary], sort: SortMode) -> Vec<OrderEntry> {
    view::compute_order(envelopes, sort)
        .into_iter()
        .map(|index| OrderEntry {
            index,
            depth: 0,
            collapsed_children: 0,
        })
        .collect()
}

/// One thread = one contiguous run of rows sharing a root (the JWZ
/// output guarantees contiguity). Vanished ids drop out; a re-thread is
/// already queued whenever that can happen.
pub(super) fn threaded_entries(
    rows: &[ThreadRow],
    envelopes: &[EnvelopeSummary],
    sort: SortMode,
    collapsed: &HashSet<EnvelopeId>,
    position_of: impl Fn(&EnvelopeId) -> Option<usize>,
) -> Vec<OrderEntry> {
    let mut threads: Vec<&[ThreadRow]> = Vec::new();
    let mut start = 0;
    for end in 1..=rows.len() {
        if end == rows.len() || rows[end].root != rows[start].root {
            threads.push(&rows[start..end]);
            start = end;
        }
    }
    sort_threads(&mut threads, envelopes, sort, &position_of);
    let mut entries = Vec::with_capacity(rows.len());
    for thread in threads {
        push_thread(thread, collapsed, &position_of, &mut entries);
    }
    entries
}

fn push_thread(
    thread: &[ThreadRow],
    collapsed: &HashSet<EnvelopeId>,
    position_of: &impl Fn(&EnvelopeId) -> Option<usize>,
    entries: &mut Vec<OrderEntry>,
) {
    let Some(root) = thread.first() else { return };
    if collapsed.contains(&root.root) {
        if let Some(position) = position_of(&root.id) {
            let hidden = thread[1..]
                .iter()
                .filter(|row| position_of(&row.id).is_some())
                .count();
            entries.push(OrderEntry {
                index: position as u32,
                depth: 0,
                collapsed_children: hidden as u32,
            });
        }
        return;
    }
    for row in thread {
        if let Some(position) = position_of(&row.id) {
            entries.push(OrderEntry {
                index: position as u32,
                depth: row.depth,
                collapsed_children: 0,
            });
        }
    }
}

/// Threads sort as units: date by the newest message, from/subject by
/// the root, unread/flagged when any message in the thread matches.
fn sort_threads(
    threads: &mut [&[ThreadRow]],
    envelopes: &[EnvelopeSummary],
    sort: SortMode,
    position_of: &impl Fn(&EnvelopeId) -> Option<usize>,
) {
    let resolve = |id: &EnvelopeId| position_of(id).map(|position| &envelopes[position]);
    match sort.key {
        SortKey::Date => threads.sort_by_key(|thread| {
            std::cmp::Reverse(
                thread
                    .iter()
                    .filter_map(|row| resolve(&row.id))
                    .map(|envelope| envelope.date_epoch_secs)
                    .max()
                    .unwrap_or(i64::MIN),
            )
        }),
        SortKey::From => threads.sort_by_cached_key(|thread| {
            thread
                .first()
                .and_then(|row| resolve(&row.id))
                .map(|envelope| envelope.from_display.to_lowercase())
                .unwrap_or_default()
        }),
        SortKey::Subject => threads.sort_by_cached_key(|thread| {
            thread
                .first()
                .and_then(|row| resolve(&row.id))
                .map(|envelope| envelope.subject.to_lowercase())
                .unwrap_or_default()
        }),
        SortKey::Unread => threads.sort_by_key(|thread| {
            !thread
                .iter()
                .filter_map(|row| resolve(&row.id))
                .any(|envelope| !envelope.flags.contains(Flags::SEEN))
        }),
        SortKey::Flagged => threads.sort_by_key(|thread| {
            !thread
                .iter()
                .filter_map(|row| resolve(&row.id))
                .any(|envelope| envelope.flags.contains(Flags::FLAGGED))
        }),
    }
    if sort.reverse {
        threads.reverse();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_mail::thread::compute_thread_rows;

    use super::*;

    fn envelope(id: &str, date: i64, message_id: &str, references: &[&str]) -> EnvelopeSummary {
        EnvelopeSummary {
            id: EnvelopeId::new(id),
            subject: format!("subject {id}"),
            from_display: String::new(),
            from_addr: String::new(),
            date_epoch_secs: date,
            flags: Flags::default(),
            message_id: message_id.to_owned(),
            references: references.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    /// Date-desc like the store guarantees: reply(300), lone(200),
    /// root(100). Threaded order: the thread (newest 300) first, walked
    /// root-then-reply, then lone.
    fn fixture() -> Vec<EnvelopeSummary> {
        vec![
            envelope("reply", 300, "re@x", &["r@x"]),
            envelope("lone", 200, "l@x", &[]),
            envelope("root", 100, "r@x", &[]),
        ]
    }

    fn ids_of(entries: &[OrderEntry], envelopes: &[EnvelopeSummary]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| envelopes[entry.index as usize].id.as_str().to_owned())
            .collect()
    }

    fn position_in(envelopes: &[EnvelopeSummary]) -> impl Fn(&EnvelopeId) -> Option<usize> + '_ {
        |id| envelopes.iter().position(|envelope| &envelope.id == id)
    }

    #[test]
    fn threaded_entries_walk_threads_with_depth() {
        let envelopes = fixture();
        let rows = compute_thread_rows(&envelopes);
        let entries = threaded_entries(
            &rows,
            &envelopes,
            SortMode::default(),
            &HashSet::new(),
            position_in(&envelopes),
        );
        assert_eq!(ids_of(&entries, &envelopes), vec!["root", "reply", "lone"]);
        assert_eq!(entries[0].depth, 0);
        assert_eq!(entries[1].depth, 1);
        assert_eq!(entries[2].depth, 0);
    }

    #[test]
    fn collapsed_thread_shows_only_its_root_with_count() {
        let envelopes = fixture();
        let rows = compute_thread_rows(&envelopes);
        let collapsed: HashSet<EnvelopeId> = [EnvelopeId::new("root")].into();
        let entries = threaded_entries(
            &rows,
            &envelopes,
            SortMode::default(),
            &collapsed,
            position_in(&envelopes),
        );
        assert_eq!(ids_of(&entries, &envelopes), vec!["root", "lone"]);
        assert_eq!(entries[0].collapsed_children, 1);
        assert_eq!(entries[1].collapsed_children, 0);
    }

    #[test]
    fn vanished_ids_drop_out_of_the_display() {
        let envelopes = fixture();
        let rows = compute_thread_rows(&envelopes);
        let shrunk: Vec<EnvelopeSummary> =
            envelopes.iter().filter(|e| e.id.as_str() != "reply").cloned().collect();
        let entries = threaded_entries(
            &rows,
            &shrunk,
            SortMode::default(),
            &HashSet::new(),
            position_in(&shrunk),
        );
        assert_eq!(
            ids_of(&entries, &shrunk),
            vec!["lone", "root"],
            "reply gone: its thread re-keys to the root's date (100) below lone (200)"
        );
    }

    #[test]
    fn subject_sort_orders_whole_threads_by_root() {
        let envelopes = vec![
            envelope("zeta-root", 100, "z@x", &[]),
            envelope("zeta-reply", 400, "zr@x", &["z@x"]),
            envelope("alpha-lone", 200, "a@x", &[]),
        ];
        let rows = compute_thread_rows(&envelopes);
        let entries = threaded_entries(
            &rows,
            &envelopes,
            SortMode {
                key: SortKey::Subject,
                reverse: false,
            },
            &HashSet::new(),
            position_in(&envelopes),
        );
        assert_eq!(
            ids_of(&entries, &envelopes),
            vec!["alpha-lone", "zeta-root", "zeta-reply"],
            "the thread moves as a unit under its root's subject"
        );
    }

    #[test]
    fn flat_entries_match_the_sort_permutation() {
        let envelopes = fixture();
        let entries = flat_entries(&envelopes, SortMode::default());
        assert_eq!(ids_of(&entries, &envelopes), vec!["reply", "lone", "root"]);
        assert!(entries.iter().all(|entry| entry.depth == 0));
    }
}
