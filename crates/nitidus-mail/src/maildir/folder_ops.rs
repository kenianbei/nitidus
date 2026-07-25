//! Folder create/delete/rename on a Maildir++ tree. Display paths
//! (`Archive/2024`) encode to dot-names (`.Archive.2024`); the dot is
//! the on-disk separator, so it cannot appear inside a path component.
//! Deletion refuses non-empty folders and folders with children — no
//! destructive path exists here by design.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::MailError;
use crate::types::FolderId;

use super::folders::{INBOX, folder_dir, is_maildir};

pub fn encode_dot_name(name: &str) -> Result<String, MailError> {
    let components: Vec<&str> = name.split('/').collect();
    let is_valid = !components.is_empty()
        && components
            .iter()
            .all(|component| !component.is_empty() && !component.contains('.'));
    if !is_valid {
        return Err(MailError::Backend(format!(
            "invalid folder name {name:?}: components must be non-empty and free of '.'"
        )));
    }
    Ok(format!(".{}", components.join(".")))
}

pub fn create(root: &Path, name: &str) -> Result<(), MailError> {
    let dir = root.join(encode_dot_name(name)?);
    if dir.exists() {
        return Err(MailError::Backend(format!("folder already exists: {name}")));
    }
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(dir.join(sub)).map_err(|error| {
            MailError::Backend(format!("create {}: {error}", dir.join(sub).display()))
        })?;
    }
    Ok(())
}

pub fn delete(root: &Path, folder: &FolderId) -> Result<(), MailError> {
    let dir = require_existing(root, folder, "delete")?;
    if message_count(&dir)? > 0 {
        return Err(MailError::Backend(format!(
            "folder not empty, refusing to delete: {folder}"
        )));
    }
    if !children_of(root, folder)?.is_empty() {
        return Err(MailError::Backend(format!(
            "folder has child folders, refusing to delete: {folder}"
        )));
    }
    fs::remove_dir_all(&dir)
        .map_err(|error| MailError::Backend(format!("delete {}: {error}", dir.display())))
}

pub fn rename(root: &Path, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
    require_existing(root, folder, "rename")?;
    let new_dot = encode_dot_name(new_name)?;
    let mut moves = vec![(folder.as_str().to_owned(), new_dot.clone())];
    for child in children_of(root, folder)? {
        let suffix = child[folder.as_str().len()..].to_owned();
        moves.push((child, format!("{new_dot}{suffix}")));
    }
    for (_, to) in &moves {
        if root.join(to).exists() {
            return Err(MailError::Backend(format!("folder already exists: {to}")));
        }
    }
    for (from, to) in &moves {
        fs::rename(root.join(from), root.join(to))
            .map_err(|error| MailError::Backend(format!("rename {from} -> {to}: {error}")))?;
    }
    Ok(())
}

fn require_existing(root: &Path, folder: &FolderId, op: &str) -> Result<PathBuf, MailError> {
    if folder.as_str() == INBOX {
        return Err(MailError::Backend(format!("cannot {op} INBOX")));
    }
    let dir = folder_dir(root, folder);
    if !is_maildir(&dir) {
        return Err(MailError::Backend(format!("no such folder: {folder}")));
    }
    Ok(dir)
}

/// Direct and transitive Maildir++ children (`.A.x`, `.A.x.y` of `.A`).
fn children_of(root: &Path, folder: &FolderId) -> Result<Vec<String>, MailError> {
    let prefix = format!("{}.", folder.as_str());
    let entries = fs::read_dir(root)
        .map_err(|error| MailError::Backend(format!("read {}: {error}", root.display())))?;
    Ok(entries
        .flatten()
        .filter(|entry| is_maildir(&entry.path()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|dir_name| dir_name.starts_with(&prefix))
        .collect())
}

fn message_count(dir: &Path) -> Result<usize, MailError> {
    let mut count = 0;
    for sub in ["cur", "new"] {
        let sub_dir = dir.join(sub);
        let entries = fs::read_dir(&sub_dir)
            .map_err(|error| MailError::Backend(format!("read {}: {error}", sub_dir.display())))?;
        count += entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .count();
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn make_maildir(dir: &Path) {
        for sub in ["cur", "new", "tmp"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
    }

    fn root() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        make_maildir(tmp.path());
        tmp
    }

    #[test]
    fn encodes_display_paths_to_dot_names() {
        assert_eq!(encode_dot_name("Sent").unwrap(), ".Sent");
        assert_eq!(encode_dot_name("Archive/2024").unwrap(), ".Archive.2024");
        assert!(encode_dot_name("a.b").is_err(), "dots are the separator");
        assert!(encode_dot_name("a//b").is_err(), "empty component");
        assert!(encode_dot_name("").is_err());
    }

    #[test]
    fn create_makes_a_maildir_and_refuses_duplicates() {
        let tmp = root();
        create(tmp.path(), "Projects/nitidus").unwrap();
        assert!(is_maildir(&tmp.path().join(".Projects.nitidus")));
        let duplicate = create(tmp.path(), "Projects/nitidus");
        assert!(duplicate.is_err(), "{duplicate:?}");
    }

    #[test]
    fn delete_refuses_inbox_missing_nonempty_and_parents() {
        let tmp = root();
        assert!(delete(tmp.path(), &FolderId::new(INBOX)).is_err());
        assert!(delete(tmp.path(), &FolderId::new(".Ghost")).is_err());

        create(tmp.path(), "Full").unwrap();
        fs::write(tmp.path().join(".Full/cur/msg.host:2,S"), "x").unwrap();
        assert!(delete(tmp.path(), &FolderId::new(".Full")).is_err());

        create(tmp.path(), "Parent").unwrap();
        create(tmp.path(), "Parent/Child").unwrap();
        assert!(delete(tmp.path(), &FolderId::new(".Parent")).is_err());
    }

    #[test]
    fn delete_removes_an_empty_leaf() {
        let tmp = root();
        create(tmp.path(), "Scratch").unwrap();
        delete(tmp.path(), &FolderId::new(".Scratch")).unwrap();
        assert!(!tmp.path().join(".Scratch").exists());
    }

    #[test]
    fn rename_moves_the_folder_and_its_children() {
        let tmp = root();
        create(tmp.path(), "Old").unwrap();
        create(tmp.path(), "Old/Sub").unwrap();
        fs::write(tmp.path().join(".Old.Sub/cur/msg.host:2,S"), "x").unwrap();

        rename(tmp.path(), &FolderId::new(".Old"), "New/Name").unwrap();
        assert!(!tmp.path().join(".Old").exists());
        assert!(is_maildir(&tmp.path().join(".New.Name")));
        assert!(
            tmp.path().join(".New.Name.Sub/cur/msg.host:2,S").is_file(),
            "child folder contents must move with the parent"
        );
    }

    #[test]
    fn rename_refuses_inbox_and_existing_targets() {
        let tmp = root();
        create(tmp.path(), "A").unwrap();
        create(tmp.path(), "B").unwrap();
        assert!(rename(tmp.path(), &FolderId::new(INBOX), "C").is_err());
        assert!(rename(tmp.path(), &FolderId::new(".A"), "B").is_err());
    }
}
