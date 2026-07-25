//! Envelope synchronization state and FETCH-response parsing. The
//! per-folder map makes re-scans incremental (CONDSTORE flag deltas +
//! new-UID fetches + UID SEARCH reconciliation) while `scan_envelopes`
//! still streams the full folder, preserving the store's prune-on-done
//! contract.

use std::collections::BTreeMap;

use io_imap::types::core::AString;
use io_imap::types::core::Vec1;
use io_imap::types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName, Section,
};
use io_imap::types::flag::{Flag, FlagFetch};
use io_imap::types::sequence::SequenceSet;

use crate::envelope::summarize_headers;
use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags};

pub(super) const FETCH_WINDOW: usize = 500;
const ENVELOPE_HEADER_FIELDS: &[&str] = &[
    "From",
    "Subject",
    "Date",
    "Message-ID",
    "References",
    "In-Reply-To",
];

/// Session-scoped sync state for one folder.
#[derive(Default)]
pub(super) struct FolderSync {
    pub uid_validity: Option<u32>,
    pub highest_mod_seq: Option<u64>,
    pub envelopes: BTreeMap<u32, EnvelopeSummary>,
}

impl FolderSync {
    pub fn max_uid(&self) -> u32 {
        self.envelopes.keys().next_back().copied().unwrap_or(0)
    }

    pub fn retain_uids(&mut self, live: &[u32]) {
        let live: std::collections::HashSet<u32> = live.iter().copied().collect();
        self.envelopes.retain(|uid, _| live.contains(uid));
    }
}

/// FETCH items for a full envelope load.
pub(super) fn envelope_fetch_items() -> Result<MacroOrMessageDataItemNames<'static>, MailError> {
    let fields: Vec<AString<'static>> = ENVELOPE_HEADER_FIELDS
        .iter()
        .map(|field| {
            AString::try_from((*field).to_owned())
                .map_err(|error| MailError::Backend(format!("header field {field}: {error}")))
        })
        .collect::<Result<_, _>>()?;
    let fields = Vec1::try_from(fields)
        .map_err(|_| MailError::Backend("empty header field list".to_owned()))?;
    Ok(MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
        MessageDataItemName::InternalDate,
        MessageDataItemName::BodyExt {
            section: Some(Section::HeaderFields(None, fields)),
            partial: None,
            peek: true,
        },
    ]))
}

pub(super) fn flags_fetch_items() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
    ])
}

pub(super) fn sequence_range(start: u32, end: u32) -> Result<SequenceSet, MailError> {
    SequenceSet::try_from(format!("{start}:{end}").as_str())
        .map_err(|error| MailError::Backend(format!("sequence {start}:{end}: {error}")))
}

pub(super) fn uid_range_from(start: u32) -> Result<SequenceSet, MailError> {
    SequenceSet::try_from(format!("{start}:*").as_str())
        .map_err(|error| MailError::Backend(format!("uid range {start}:*: {error}")))
}

pub(super) fn single_uid(uid: u32) -> Result<SequenceSet, MailError> {
    SequenceSet::try_from(uid.to_string().as_str())
        .map_err(|error| MailError::Backend(format!("uid {uid}: {error}")))
}

/// One FETCH response entry → `(uid, EnvelopeSummary)`.
pub(super) fn parse_envelope_items(
    items: &[MessageDataItem<'_>],
) -> Option<(u32, EnvelopeSummary)> {
    let uid = uid_of(items)?;
    let flags = flags_of(items);
    let mut header_bytes: &[u8] = &[];
    let mut fallback_date = 0i64;
    for item in items {
        match item {
            MessageDataItem::BodyExt { data, .. } => {
                header_bytes = data.0.as_ref().map(|inner| inner.as_ref()).unwrap_or(&[]);
            }
            MessageDataItem::InternalDate(date) => {
                fallback_date = date.as_ref().timestamp();
            }
            _ => {}
        }
    }
    Some((
        uid,
        summarize_headers(
            header_bytes,
            EnvelopeId::new(uid.to_string()),
            flags,
            fallback_date,
        ),
    ))
}

pub(super) fn uid_of(items: &[MessageDataItem<'_>]) -> Option<u32> {
    items.iter().find_map(|item| match item {
        MessageDataItem::Uid(uid) => Some(uid.get()),
        _ => None,
    })
}

pub(super) fn flags_of(items: &[MessageDataItem<'_>]) -> Flags {
    let mut flags = Flags::default();
    for item in items {
        if let MessageDataItem::Flags(fetched) = item {
            for flag in fetched {
                flags = flags.with(map_flag(flag));
            }
        }
    }
    flags
}

fn map_flag(flag: &FlagFetch<'_>) -> Flags {
    match flag {
        FlagFetch::Flag(Flag::Seen) => Flags::SEEN,
        FlagFetch::Flag(Flag::Answered) => Flags::ANSWERED,
        FlagFetch::Flag(Flag::Flagged) => Flags::FLAGGED,
        FlagFetch::Flag(Flag::Deleted) => Flags::DELETED,
        FlagFetch::Flag(Flag::Draft) => Flags::DRAFT,
        _ => Flags::default(),
    }
}

/// Our Flags bitset → IMAP flag list for `UID STORE FLAGS`.
pub(super) fn imap_flags(flags: Flags) -> Vec<Flag<'static>> {
    [
        (Flags::SEEN, Flag::Seen),
        (Flags::ANSWERED, Flag::Answered),
        (Flags::FLAGGED, Flag::Flagged),
        (Flags::DELETED, Flag::Deleted),
        (Flags::DRAFT, Flag::Draft),
    ]
    .into_iter()
    .filter(|(bit, _)| flags.contains(*bit))
    .map(|(_, flag)| flag)
    .collect()
}

pub(super) fn message_body_items() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![MessageDataItemName::BodyExt {
        section: None,
        partial: None,
        peek: true,
    }])
}

pub(super) fn body_of(items: &[MessageDataItem<'_>]) -> Option<Vec<u8>> {
    items.iter().find_map(|item| match item {
        MessageDataItem::BodyExt { data, .. } => {
            data.0.as_ref().map(|inner| inner.as_ref().to_vec())
        }
        MessageDataItem::Rfc822(data) => data.0.as_ref().map(|inner| inner.as_ref().to_vec()),
        _ => None,
    })
}
