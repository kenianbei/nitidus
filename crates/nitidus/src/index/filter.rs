//! The shared text matcher behind `/` search and `:limit`:
//! case-insensitive substring over subject and sender. Plain text only
//! — the pattern language is phase 2.

use bevy::prelude::*;
use nitidus_mail::EnvelopeSummary;

use super::IndexView;
use crate::status::StatusMessage;

pub(super) fn matches(envelope: &EnvelopeSummary, needle_lower: &str) -> bool {
    envelope.subject.to_lowercase().contains(needle_lower)
        || envelope.from_display.to_lowercase().contains(needle_lower)
        || envelope.from_addr.to_lowercase().contains(needle_lower)
}

pub(super) fn matches_all(envelope: &EnvelopeSummary, needles_lower: &[String]) -> bool {
    needles_lower.iter().all(|needle| matches(envelope, needle))
}

/// Byte range of the first case-insensitive occurrence of
/// `needle_lower` in `text`, for highlight spans. `None` when
/// lowercasing shifts byte offsets (rare non-ASCII case folds) —
/// skipping the highlight beats mis-slicing a row.
pub(super) fn match_range(text: &str, needle_lower: &str) -> Option<(usize, usize)> {
    if needle_lower.is_empty() {
        return None;
    }
    let lowered = text.to_lowercase();
    if lowered.len() != text.len() {
        return None;
    }
    let start = lowered.find(needle_lower)?;
    let end = start + needle_lower.len();
    (text.is_char_boundary(start) && text.is_char_boundary(end)).then_some((start, end))
}

/// `:limit <text>` — stacks; every needle must match (AND).
pub fn push_limit(world: &mut World, text: &str) {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return;
    }
    let mut view = world.resource_mut::<IndexView>();
    view.limits.push(needle);
    view.filter_epoch += 1;
    let joined = view.limits.join("+");
    let now = world.resource::<Time>().elapsed_secs_f64();
    world
        .resource_mut::<StatusMessage>()
        .info(format!("limit: {joined}"), now);
}

/// `:clear` — drops the limit stack and the retained search query.
pub fn clear_filters(world: &mut World) {
    let mut view = world.resource_mut::<IndexView>();
    if view.limits.is_empty() && view.search.is_none() {
        return;
    }
    view.limits.clear();
    view.search = None;
    view.filter_epoch += 1;
    let now = world.resource::<Time>().elapsed_secs_f64();
    world
        .resource_mut::<StatusMessage>()
        .info("filters cleared".to_owned(), now);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use nitidus_mail::{EnvelopeId, Flags};

    fn envelope(subject: &str, display: &str, addr: &str) -> EnvelopeSummary {
        EnvelopeSummary {
            id: EnvelopeId::new("1"),
            subject: subject.to_owned(),
            from_display: display.to_owned(),
            from_addr: addr.to_owned(),
            date_epoch_secs: 0,
            flags: Flags::default(),
            message_id: String::new(),
            references: Vec::new(),
        }
    }

    #[test]
    fn matches_subject_and_both_from_fields_case_insensitively() {
        let sample = envelope("Quarterly Report", "Ada Lovelace", "ada@x.example");
        assert!(matches(&sample, "quarterly"));
        assert!(matches(&sample, "lovelace"));
        assert!(matches(&sample, "x.example"));
        assert!(!matches(&sample, "nowhere"));
    }

    #[test]
    fn match_range_finds_case_insensitive_byte_ranges() {
        assert_eq!(match_range("Quarterly Report", "report"), Some((10, 16)));
        assert_eq!(match_range("abc", "zzz"), None);
        assert_eq!(match_range("abc", ""), None);
    }

    #[test]
    fn matches_all_requires_every_needle() {
        let sample = envelope("Quarterly Report", "Ada", "ada@x.example");
        let both = ["report".to_owned(), "ada".to_owned()];
        let one_off = ["report".to_owned(), "zed".to_owned()];
        assert!(matches_all(&sample, &both));
        assert!(!matches_all(&sample, &one_off));
    }
}
