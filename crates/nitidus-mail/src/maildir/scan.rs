//! Envelope parsing from a bounded header window. `io-maildir`'s only
//! body reader is a full `fs::read` of every entry, so this stays ours:
//! a 64 KB window keeps the memory ceiling flat across large mailboxes
//! (`refactor-himalaya-sync-v1` §3.3 finding 7).

use std::fs;
use std::io::Read;
use std::path::Path;

use io_maildir::entry::MaildirEntry;

use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags};

use super::flags;

const HEADER_WINDOW_BYTES: usize = 64 * 1024;
const NEW_SUBDIR: &str = "new";

pub fn parse_envelope(entry: &MaildirEntry) -> Result<EnvelopeSummary, MailError> {
    let path = Path::new(entry.path().as_str());
    let id = entry
        .id()
        .ok_or_else(|| MailError::Backend(format!("invalid file name: {}", path.display())))?;
    let window = read_header_window(path)?;
    Ok(crate::envelope::summarize_headers(
        &window,
        EnvelopeId::new(id),
        effective_flags(entry),
        mtime_epoch(path),
    ))
}

/// A message still in `new/` has not been seen, whatever its suffix says.
fn effective_flags(entry: &MaildirEntry) -> Flags {
    let parsed = flags::from_maildir(&entry.flags());
    if is_in_new(entry) {
        parsed.without(Flags::SEEN)
    } else {
        parsed
    }
}

pub fn is_in_new(entry: &MaildirEntry) -> bool {
    entry.path().parent().is_some_and(|parent| {
        Path::new(parent)
            .file_name()
            .is_some_and(|name| name == NEW_SUBDIR)
    })
}

fn read_header_window(path: &Path) -> Result<Vec<u8>, MailError> {
    let file = fs::File::open(path)
        .map_err(|error| MailError::Backend(format!("open {}: {error}", path.display())))?;
    let mut window = Vec::with_capacity(8 * 1024);
    let mut taken = file.take(HEADER_WINDOW_BYTES as u64);
    taken
        .read_to_end(&mut window)
        .map_err(|error| MailError::Backend(format!("read {}: {error}", path.display())))?;
    Ok(window)
}

fn mtime_epoch(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn reads_the_id_and_flags_out_of_the_file_name() {
        let entry = MaildirEntry::from_path("/mail/cur/1700000000.abc.host:2,FS");
        assert_eq!(entry.id(), Some("1700000000.abc.host"));
        assert_eq!(
            flags::from_maildir(&entry.flags()),
            Flags::FLAGGED.with(Flags::SEEN)
        );
    }

    #[test]
    fn messages_in_new_are_never_seen() {
        let unseen = MaildirEntry::from_path("/mail/new/1700000000.abc.host:2,S");
        assert!(is_in_new(&unseen));
        assert!(!effective_flags(&unseen).contains(Flags::SEEN));

        let seen = MaildirEntry::from_path("/mail/cur/1700000000.abc.host:2,S");
        assert!(!is_in_new(&seen));
        assert!(effective_flags(&seen).contains(Flags::SEEN));
    }
}
