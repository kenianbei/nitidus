//! Index view state: sort modes, the display permutation, and
//! identity-based selection that survives re-syncs and re-sorts.

use anyhow::bail;
use bevy::prelude::Resource;
use nitidus_mail::{AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, maildir};

use crate::action::Motion;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Date,
    From,
    Subject,
    Unread,
    Flagged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SortMode {
    pub key: SortKey,
    pub reverse: bool,
}

impl SortMode {
    /// `":sort [key] [-r]"`; no arguments resets to the date default.
    pub fn parse(args: &str) -> anyhow::Result<SortMode> {
        let mut mode = SortMode::default();
        for token in args.split_whitespace() {
            match token {
                "-r" => mode.reverse = true,
                "date" => mode.key = SortKey::Date,
                "from" => mode.key = SortKey::From,
                "subject" => mode.key = SortKey::Subject,
                "unread" => mode.key = SortKey::Unread,
                "flagged" => mode.key = SortKey::Flagged,
                other => {
                    bail!("unknown sort key {other:?} (date, from, subject, unread, flagged)")
                }
            }
        }
        Ok(mode)
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct IndexView {
    pub account: Option<AccountId>,
    pub folder: FolderId,
    pub selected: Option<EnvelopeId>,
    /// Last resolved row — the clamp anchor when the selected id
    /// disappears from the folder.
    pub selected_row: usize,
    pub top: usize,
    pub sort: SortMode,
    pub threaded: bool,
    /// Collapsed thread roots; keyed by envelope id so folds survive
    /// re-threads.
    pub collapsed: std::collections::HashSet<EnvelopeId>,
    /// Bumped by fold/threading toggles so the order rebuild can react
    /// without rebuilding on every cursor move.
    pub fold_epoch: u64,
}

impl Default for IndexView {
    fn default() -> Self {
        Self {
            account: None,
            folder: FolderId::new(maildir::INBOX),
            selected: None,
            selected_row: 0,
            top: 0,
            sort: SortMode::default(),
            threaded: false,
            collapsed: std::collections::HashSet::new(),
            fold_epoch: 0,
        }
    }
}

/// Display permutation over the store's date-desc slice. The date key
/// rides the store order for free; the others pay a stable sort, so
/// equal keys keep the date-desc order.
pub fn compute_order(envelopes: &[EnvelopeSummary], sort: SortMode) -> Vec<u32> {
    let mut order: Vec<u32> = (0..envelopes.len() as u32).collect();
    match sort.key {
        SortKey::Date => {}
        SortKey::From => {
            order.sort_by_cached_key(|&i| envelopes[i as usize].from_display.to_lowercase());
        }
        SortKey::Subject => {
            order.sort_by_cached_key(|&i| envelopes[i as usize].subject.to_lowercase());
        }
        SortKey::Unread => {
            order.sort_by_key(|&i| envelopes[i as usize].flags.contains(Flags::SEEN));
        }
        SortKey::Flagged => {
            order.sort_by_key(|&i| !envelopes[i as usize].flags.contains(Flags::FLAGGED));
        }
    }
    if sort.reverse {
        order.reverse();
    }
    order
}

/// Follows the selected id into the current entry list; a vanished id
/// clamps to the anchored position instead.
pub(super) fn resolve_selection(
    view: &IndexView,
    envelopes: &[EnvelopeSummary],
    entries: &[super::thread_view::OrderEntry],
) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    if let Some(id) = &view.selected
        && let Some(row) = entries
            .iter()
            .position(|entry| &envelopes[entry.index as usize].id == id)
    {
        return Some(row);
    }
    Some(view.selected_row.min(entries.len() - 1))
}

/// Row arithmetic for uniform motions; `Parent` is thread-structural
/// and handled by `ops::move_to_parent` before reaching here.
pub fn apply_motion(row: usize, total: usize, page: usize, motion: Motion) -> usize {
    let last = total.saturating_sub(1);
    match motion {
        Motion::Next => (row + 1).min(last),
        Motion::Prev => row.saturating_sub(1),
        Motion::NextPage => (row + page).min(last),
        Motion::PrevPage => row.saturating_sub(page),
        Motion::First => 0,
        Motion::Last => last,
        Motion::Parent => row,
    }
}

/// Minimal scroll to keep the selection inside the viewport.
pub fn scrolled_top(top: usize, selected: usize, height: usize) -> usize {
    if height == 0 {
        return top;
    }
    if selected < top {
        selected
    } else if selected >= top + height {
        selected + 1 - height
    } else {
        top
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn envelope(id: &str, from: &str, subject: &str, date: i64, flags: Flags) -> EnvelopeSummary {
        EnvelopeSummary {
            id: EnvelopeId::new(id),
            subject: subject.to_owned(),
            from_display: from.to_owned(),
            from_addr: format!("{from}@example.com"),
            date_epoch_secs: date,
            flags,
            message_id: format!("{id}@example"),
            references: Vec::new(),
        }
    }

    fn fixture() -> Vec<EnvelopeSummary> {
        vec![
            envelope("newest", "carol", "beta", 300, Flags::default().with(Flags::SEEN)),
            envelope("middle", "alice", "alpha", 200, Flags::default()),
            envelope(
                "oldest",
                "bob",
                "gamma",
                100,
                Flags::default().with(Flags::SEEN).with(Flags::FLAGGED),
            ),
        ]
    }

    fn sorted_ids(envelopes: &[EnvelopeSummary], sort: SortMode) -> Vec<&str> {
        compute_order(envelopes, sort)
            .iter()
            .map(|&i| envelopes[i as usize].id.as_str())
            .collect()
    }

    #[test]
    fn date_sort_is_store_order_and_reversible() {
        let envelopes = fixture();
        let date = SortMode::default();
        assert_eq!(sorted_ids(&envelopes, date), vec!["newest", "middle", "oldest"]);
        let reversed = SortMode {
            reverse: true,
            ..date
        };
        assert_eq!(
            sorted_ids(&envelopes, reversed),
            vec!["oldest", "middle", "newest"]
        );
    }

    #[test]
    fn from_subject_and_flag_sorts_order_correctly() {
        let envelopes = fixture();
        let by = |key| SortMode {
            key,
            reverse: false,
        };
        assert_eq!(
            sorted_ids(&envelopes, by(SortKey::From)),
            vec!["middle", "oldest", "newest"]
        );
        assert_eq!(
            sorted_ids(&envelopes, by(SortKey::Subject)),
            vec!["middle", "newest", "oldest"]
        );
        assert_eq!(
            sorted_ids(&envelopes, by(SortKey::Unread)),
            vec!["middle", "newest", "oldest"],
            "unseen first, then store date order"
        );
        assert_eq!(
            sorted_ids(&envelopes, by(SortKey::Flagged)),
            vec!["oldest", "newest", "middle"]
        );
    }

    #[test]
    fn parses_sort_arguments() {
        assert_eq!(SortMode::parse("").unwrap(), SortMode::default());
        assert_eq!(
            SortMode::parse("subject -r").unwrap(),
            SortMode {
                key: SortKey::Subject,
                reverse: true
            }
        );
        assert!(SortMode::parse("size").is_err());
    }

    #[test]
    fn selection_follows_id_across_reorder() {
        let envelopes = fixture();
        let mut view = IndexView {
            selected: Some(EnvelopeId::new("oldest")),
            ..IndexView::default()
        };
        let date_entries = crate::index::thread_view::flat_entries(&envelopes, SortMode::default());
        assert_eq!(resolve_selection(&view, &envelopes, &date_entries), Some(2));
        view.sort = SortMode {
            key: SortKey::Flagged,
            reverse: false,
        };
        let flag_entries = crate::index::thread_view::flat_entries(&envelopes, view.sort);
        assert_eq!(resolve_selection(&view, &envelopes, &flag_entries), Some(0));
    }

    #[test]
    fn vanished_selection_clamps_to_anchor() {
        let envelopes = fixture();
        let entries = crate::index::thread_view::flat_entries(&envelopes, SortMode::default());
        let view = IndexView {
            selected: Some(EnvelopeId::new("gone")),
            selected_row: 7,
            ..IndexView::default()
        };
        assert_eq!(resolve_selection(&view, &envelopes, &entries), Some(2));
        assert_eq!(resolve_selection(&view, &[], &[]), None);
    }

    #[test]
    fn motions_clamp_at_both_ends() {
        assert_eq!(apply_motion(0, 10, 5, Motion::Prev), 0);
        assert_eq!(apply_motion(9, 10, 5, Motion::Next), 9);
        assert_eq!(apply_motion(2, 10, 5, Motion::NextPage), 7);
        assert_eq!(apply_motion(8, 10, 5, Motion::NextPage), 9);
        assert_eq!(apply_motion(3, 10, 5, Motion::PrevPage), 0);
        assert_eq!(apply_motion(5, 10, 5, Motion::First), 0);
        assert_eq!(apply_motion(5, 10, 5, Motion::Last), 9);
        assert_eq!(apply_motion(0, 0, 5, Motion::Next), 0);
    }

    #[test]
    fn scrolling_keeps_selection_visible() {
        assert_eq!(scrolled_top(0, 5, 10), 0);
        assert_eq!(scrolled_top(0, 12, 10), 3);
        assert_eq!(scrolled_top(20, 5, 10), 5);
        assert_eq!(scrolled_top(3, 3, 10), 3);
        assert_eq!(scrolled_top(3, 9, 0), 3);
    }
}
