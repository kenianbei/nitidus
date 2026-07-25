//! File-level maildir operations: envelope parsing from a header
//! window, `:2,` flag suffixes, and the flag-rename protocol.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags};

const HEADER_WINDOW_BYTES: usize = 64 * 1024;
const FLAG_SEPARATOR: &str = ":2,";

pub fn parse_envelope(path: &Path, in_new: bool) -> Result<EnvelopeSummary, MailError> {
    let file_name = file_name_of(path)?;
    let (unique, flags) = split_flags(&file_name);
    let window = read_header_window(path)?;
    let effective_flags = if in_new {
        flags.without(Flags::SEEN)
    } else {
        flags
    };
    Ok(crate::envelope::summarize_headers(
        &window,
        EnvelopeId::new(unique),
        effective_flags,
        mtime_epoch(path),
    ))
}

/// Classic maildir delivery: write to `tmp/`, rename into `cur/`
/// with the flag suffix. The unique name includes nanos + pid so
/// concurrent deliveries never collide.
pub fn deliver(folder_dir: &Path, bytes: &[u8], flags: Flags) -> Result<EnvelopeId, MailError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let unique = format!("{nanos}.{}.nitidus", std::process::id());
    let tmp = folder_dir.join("tmp").join(&unique);
    fs::write(&tmp, bytes)
        .map_err(|error| MailError::Backend(format!("write {}: {error}", tmp.display())))?;
    let target = folder_dir
        .join("cur")
        .join(format!("{unique}{FLAG_SEPARATOR}{}", flag_suffix(flags)));
    fs::rename(&tmp, &target)
        .map_err(|error| MailError::Backend(format!("deliver {}: {error}", target.display())))?;
    Ok(EnvelopeId::new(unique))
}

/// Finds the file for a maildir unique name in `cur/` then `new/`.
pub fn find_message(folder_dir: &Path, id: &EnvelopeId) -> Result<PathBuf, MailError> {
    for sub in ["cur", "new"] {
        let dir = folder_dir.join(sub);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let (unique, _) = split_flags(name);
            if unique == id.as_str() {
                return Ok(entry.path());
            }
        }
    }
    Err(MailError::Backend(format!(
        "message not found: {id} in {}",
        folder_dir.display()
    )))
}

/// Renames to the new flag suffix; flagged messages always land in
/// `cur/` (a message with flags is no longer "new").
pub fn rename_with_flags(
    folder_dir: &Path,
    current: &Path,
    id: &EnvelopeId,
    flags: Flags,
) -> Result<PathBuf, MailError> {
    let target = folder_dir.join("cur").join(format!(
        "{}{}{}",
        id.as_str(),
        FLAG_SEPARATOR,
        flag_suffix(flags)
    ));
    fs::rename(current, &target)
        .map_err(|error| MailError::Backend(format!("rename {}: {error}", current.display())))?;
    Ok(target)
}

pub fn split_flags(file_name: &str) -> (&str, Flags) {
    match file_name.split_once(FLAG_SEPARATOR) {
        Some((unique, suffix)) => (unique, parse_flag_suffix(suffix)),
        None => (file_name, Flags::default()),
    }
}

fn parse_flag_suffix(suffix: &str) -> Flags {
    let mut flags = Flags::default();
    for c in suffix.chars() {
        flags = match c {
            'D' => flags.with(Flags::DRAFT),
            'F' => flags.with(Flags::FLAGGED),
            'R' => flags.with(Flags::ANSWERED),
            'S' => flags.with(Flags::SEEN),
            'T' => flags.with(Flags::DELETED),
            _ => flags,
        };
    }
    flags
}

/// Maildir flag letters must be ASCII-sorted: D, F, R, S, T.
fn flag_suffix(flags: Flags) -> String {
    let mut suffix = String::new();
    for (flag, letter) in [
        (Flags::DRAFT, 'D'),
        (Flags::FLAGGED, 'F'),
        (Flags::ANSWERED, 'R'),
        (Flags::SEEN, 'S'),
        (Flags::DELETED, 'T'),
    ] {
        if flags.contains(flag) {
            suffix.push(letter);
        }
    }
    suffix
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

fn file_name_of(path: &Path) -> Result<String, MailError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| MailError::Backend(format!("invalid file name: {}", path.display())))
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
    fn splits_unique_name_and_flags() {
        let (unique, flags) = split_flags("1700000000.abc123.host:2,FS");
        assert_eq!(unique, "1700000000.abc123.host");
        assert!(flags.contains(Flags::FLAGGED));
        assert!(flags.contains(Flags::SEEN));
        assert!(!flags.contains(Flags::DELETED));

        let (unique, flags) = split_flags("1700000000.abc123.host");
        assert_eq!(unique, "1700000000.abc123.host");
        assert_eq!(flags, Flags::default());
    }

    #[test]
    fn flag_suffix_is_ascii_sorted() {
        let flags = Flags::default()
            .with(Flags::SEEN)
            .with(Flags::DRAFT)
            .with(Flags::ANSWERED);
        assert_eq!(flag_suffix(flags), "DRS");
    }
}
