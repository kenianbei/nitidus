//! Folder discovery over LIST + STATUS, and the display-name mapping:
//! raw mailbox names stay the folder ids so commands round-trip, while
//! display names decode modified-UTF-7 and normalize the hierarchy
//! delimiter to `/` for the sidebar tree.

use io_imap::rfc3501::list::ImapMailboxList;
use io_imap::rfc3501::status::ImapMailboxStatus;
use io_imap::types::flag::FlagNameAttribute;
use io_imap::types::mailbox::{ListMailbox, Mailbox};
use io_imap::types::status::{StatusDataItem, StatusDataItemName};

use super::session::{ImapSession, parse_mailbox};
use crate::error::MailError;
use crate::maildir::INBOX;
use crate::types::{FolderId, FolderMeta};

pub(super) const DEFAULT_DELIMITER: char = '/';

pub(super) async fn list_folders(
    session: &mut ImapSession,
) -> Result<(Vec<FolderMeta>, char), MailError> {
    let reference = parse_mailbox("")?;
    let wildcard = ListMailbox::try_from("*".to_owned())
        .map_err(|error| MailError::Backend(format!("list wildcard: {error}")))?;
    let listing = session
        .run(|| ImapMailboxList::new(reference.clone(), wildcard.clone()))
        .await?;

    let mut delimiter = DEFAULT_DELIMITER;
    let mut selectable = Vec::new();
    for (mailbox, folder_delimiter, attributes) in listing {
        if let Some(quoted) = folder_delimiter {
            delimiter = quoted.inner();
        }
        if attributes
            .iter()
            .any(|attribute| matches!(attribute, FlagNameAttribute::Noselect))
        {
            continue;
        }
        selectable.push(mailbox_name(&mailbox));
    }
    order_inbox_first(&mut selectable);

    let mut folders = Vec::with_capacity(selectable.len());
    for raw in selectable {
        let (total, unread) = folder_counts(session, &raw).await?;
        folders.push(FolderMeta {
            id: FolderId::new(&raw),
            name: display_name(&raw, delimiter),
            unread,
            total,
        });
    }
    Ok((folders, delimiter))
}

async fn folder_counts(session: &mut ImapSession, raw: &str) -> Result<(u32, u32), MailError> {
    let mailbox = parse_mailbox(raw)?;
    let items = session
        .run(|| {
            ImapMailboxStatus::new(
                mailbox.clone(),
                [StatusDataItemName::Messages, StatusDataItemName::Unseen].as_slice(),
            )
        })
        .await?;
    let mut total = 0;
    let mut unread = 0;
    for item in items {
        match item {
            StatusDataItem::Messages(count) => total = count,
            StatusDataItem::Unseen(count) => unread = count,
            _ => {}
        }
    }
    Ok((total, unread))
}

pub(super) fn mailbox_name(mailbox: &Mailbox<'_>) -> String {
    match mailbox {
        Mailbox::Inbox => INBOX.to_owned(),
        Mailbox::Other(other) => String::from_utf8_lossy(other.as_ref()).into_owned(),
    }
}

/// Modified-UTF-7 decoded, delimiter normalized to `/`.
pub(super) fn display_name(raw: &str, delimiter: char) -> String {
    let decoded = decode_utf7(raw);
    if delimiter == '/' {
        decoded
    } else {
        decoded.replace(delimiter, "/")
    }
}

/// utf7-imap panics on malformed encodings, and mailbox names come
/// from the network — an invalid name must display raw, not kill the
/// actor.
fn decode_utf7(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_owned();
    }
    let owned = raw.to_owned();
    std::panic::catch_unwind(|| utf7_imap::decode_utf7_imap(owned))
        .unwrap_or_else(|_| raw.to_owned())
}

/// Inverse of `display_name` for CREATE/RENAME arguments.
pub(super) fn encode_name(display: &str, delimiter: char) -> String {
    let with_delimiter = if delimiter == '/' {
        display.to_owned()
    } else {
        display.replace('/', &delimiter.to_string())
    };
    utf7_imap::encode_utf7_imap(with_delimiter)
}

fn order_inbox_first(names: &mut Vec<String>) {
    names.sort_by(|a, b| {
        let a_inbox = a.eq_ignore_ascii_case(INBOX);
        let b_inbox = b.eq_ignore_ascii_case(INBOX);
        b_inbox.cmp(&a_inbox).then_with(|| a.cmp(b))
    });
    names.dedup();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn display_names_normalize_delimiters_and_survive_bad_utf7() {
        assert_eq!(display_name("[Gmail]/Sent Mail", '/'), "[Gmail]/Sent Mail");
        assert_eq!(display_name("Archive.2024", '.'), "Archive/2024");
        assert_eq!(
            display_name("&invalid=utf7", '/'),
            "&invalid=utf7",
            "malformed encodings must fall back to the raw name"
        );
    }

    #[test]
    fn encode_name_round_trips_display_names() {
        assert_eq!(encode_name("Archive/2024", '.'), "Archive.2024");
        for name in ["收件", "收件/子", "Ünïcode/Bränch"] {
            for delimiter in ['/', '.'] {
                assert_eq!(
                    display_name(&encode_name(name, delimiter), delimiter),
                    name,
                    "round trip failed for {name:?} with {delimiter:?}"
                );
            }
        }
    }

    #[test]
    fn inbox_sorts_first_case_insensitively() {
        let mut names = vec!["Work".to_owned(), "inbox".to_owned(), "Archive".to_owned()];
        order_inbox_first(&mut names);
        assert_eq!(names, vec!["inbox", "Archive", "Work"]);
    }
}
