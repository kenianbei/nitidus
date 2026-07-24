//! Folder discovery. The account root is itself a maildir (INBOX); any
//! child directory containing `cur`, `new`, and `tmp` is a folder.
//! Maildir++ dot-names (`.Archive.2024`) decode to display paths
//! (`Archive/2024`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::MailError;
use crate::types::{FolderId, FolderMeta};

pub const INBOX: &str = "INBOX";

pub fn validate_root(root: &Path) -> Result<(), MailError> {
    if is_maildir(root) {
        Ok(())
    } else {
        Err(MailError::Backend(format!(
            "not a maildir (missing cur/new/tmp): {}",
            root.display()
        )))
    }
}

pub fn discover(root: &Path) -> Result<Vec<FolderMeta>, MailError> {
    validate_root(root)?;
    let mut folders = vec![folder_meta(root, FolderId::new(INBOX), INBOX.to_owned())];
    let entries = fs::read_dir(root)
        .map_err(|error| MailError::Backend(format!("read {}: {error}", root.display())))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_maildir(&path) {
            continue;
        }
        if let Some(dir_name) = path.file_name().and_then(|name| name.to_str()) {
            folders.push(folder_meta(
                &path,
                FolderId::new(dir_name),
                display_name(dir_name),
            ));
        }
    }
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

/// Maps a folder id back to its directory. INBOX is the root itself.
pub fn folder_dir(root: &Path, folder: &FolderId) -> PathBuf {
    if folder.as_str() == INBOX {
        root.to_path_buf()
    } else {
        root.join(folder.as_str())
    }
}

fn folder_meta(dir: &Path, id: FolderId, name: String) -> FolderMeta {
    let unread = count_files(&dir.join("new"));
    let cur = count_files(&dir.join("cur"));
    FolderMeta {
        id,
        name,
        unread,
        total: unread + cur,
    }
}

fn is_maildir(dir: &Path) -> bool {
    dir.is_dir() && dir.join("cur").is_dir() && dir.join("new").is_dir() && dir.join("tmp").is_dir()
}

fn count_files(dir: &Path) -> u32 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn display_name(dir_name: &str) -> String {
    match dir_name.strip_prefix('.') {
        Some(dotted) => dotted.replace('.', "/"),
        None => dir_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn decodes_maildir_plus_plus_names() {
        assert_eq!(display_name(".Archive.2024"), "Archive/2024");
        assert_eq!(display_name(".Sent"), "Sent");
        assert_eq!(display_name("Archive"), "Archive");
    }

    #[test]
    fn inbox_maps_to_root() {
        let root = Path::new("/mail");
        assert_eq!(folder_dir(root, &FolderId::new(INBOX)), root);
        assert_eq!(
            folder_dir(root, &FolderId::new(".Sent")),
            root.join(".Sent")
        );
    }
}
