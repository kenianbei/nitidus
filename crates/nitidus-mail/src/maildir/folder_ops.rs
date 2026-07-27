//! Validation and refusal in front of `io-maildir`'s folder coroutines,
//! which are unguarded by design: `MaildirCreate` is idempotent,
//! `MaildirDelete` is a recursive remove, and `MaildirRename` moves one
//! directory without its Maildir++ children. No destructive path exists
//! here by design.

use std::fs;
use std::path::{Path, PathBuf};

use io_maildir::client::MaildirClient;
use io_maildir::path::MaildirPath;

use crate::error::MailError;
use crate::types::FolderId;

use super::folders::{INBOX, folder_path, is_maildir};

/// `MaildirStore::resolve` does the dot-encoding; it does no
/// validation, so the component rules stay here. The dot is the on-disk
/// separator, so it cannot appear inside a component.
pub fn validate_name(name: &str) -> Result<MaildirPath, MailError> {
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
    Ok(MaildirPath::from(name))
}

pub fn create(client: &MaildirClient, name: &str) -> Result<(), MailError> {
    let path = validate_name(name)?;
    if Path::new(client.store.resolve(&path).as_str()).exists() {
        return Err(MailError::Backend(format!("folder already exists: {name}")));
    }
    client
        .create_maildir(path)
        .map_err(|error| MailError::Backend(format!("create {name}: {error}")))
}

pub fn delete(client: &MaildirClient, folder: &FolderId) -> Result<(), MailError> {
    let dir = require_existing(client, folder, "delete")?;
    if message_count(&dir)? > 0 {
        return Err(MailError::Backend(format!(
            "folder not empty, refusing to delete: {folder}"
        )));
    }
    if !children_of(client, folder)?.is_empty() {
        return Err(MailError::Backend(format!(
            "folder has child folders, refusing to delete: {folder}"
        )));
    }
    client
        .delete_maildir(folder_path(folder))
        .map_err(|error| MailError::Backend(format!("delete {folder}: {error}")))
}

/// Their rename moves one directory, so the Maildir++ children are
/// renamed explicitly alongside the parent.
pub fn rename(client: &MaildirClient, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
    require_existing(client, folder, "rename")?;
    let new_dot = dot_name_of(&validate_name(new_name)?);
    let mut moves = vec![(folder.as_str().to_owned(), new_dot.clone())];
    for child in children_of(client, folder)? {
        let suffix = child[folder.as_str().len()..].to_owned();
        moves.push((child, format!("{new_dot}{suffix}")));
    }
    let root = root_of(client);
    for (_, to) in &moves {
        if root.join(to).exists() {
            return Err(MailError::Backend(format!("folder already exists: {to}")));
        }
    }
    for (from, to) in &moves {
        client
            .rename_maildir(logical_of(from), logical_of(to))
            .map_err(|error| MailError::Backend(format!("rename {from} -> {to}: {error}")))?;
    }
    Ok(())
}

fn require_existing(
    client: &MaildirClient,
    folder: &FolderId,
    op: &str,
) -> Result<PathBuf, MailError> {
    if folder.as_str() == INBOX {
        return Err(MailError::Backend(format!("cannot {op} INBOX")));
    }
    let dir = PathBuf::from(client.store.resolve(&folder_path(folder)).as_str());
    if !is_maildir(&dir) {
        return Err(MailError::Backend(format!("no such folder: {folder}")));
    }
    Ok(dir)
}

/// Direct and transitive Maildir++ children (`.A.x`, `.A.x.y` of `.A`).
fn children_of(client: &MaildirClient, folder: &FolderId) -> Result<Vec<String>, MailError> {
    let root = root_of(client);
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

fn root_of(client: &MaildirClient) -> &Path {
    Path::new(client.store.root.as_str())
}

fn logical_of(dot_name: &str) -> MaildirPath {
    folder_path(&FolderId::new(dot_name))
}

fn dot_name_of(path: &MaildirPath) -> String {
    format!(".{}", path.as_str().replace('/', "."))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::maildir::folders::build_client;

    fn make_maildir(dir: &Path) {
        for sub in ["cur", "new", "tmp"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
    }

    fn root() -> (tempfile::TempDir, MaildirClient) {
        let tmp = tempfile::tempdir().unwrap();
        make_maildir(tmp.path());
        let client = build_client(tmp.path());
        (tmp, client)
    }

    #[test]
    fn encodes_display_paths_to_dot_names() {
        assert_eq!(dot_name_of(&validate_name("Sent").unwrap()), ".Sent");
        assert_eq!(
            dot_name_of(&validate_name("Archive/2024").unwrap()),
            ".Archive.2024"
        );
        assert!(validate_name("a.b").is_err(), "dots are the separator");
        assert!(validate_name("a//b").is_err(), "empty component");
        assert!(validate_name("").is_err());
    }

    #[test]
    fn create_makes_a_maildir_and_refuses_duplicates() {
        let (tmp, client) = root();
        create(&client, "Projects/nitidus").unwrap();
        assert!(is_maildir(&tmp.path().join(".Projects.nitidus")));
        let duplicate = create(&client, "Projects/nitidus");
        assert!(duplicate.is_err(), "{duplicate:?}");
    }

    #[test]
    fn delete_refuses_inbox_missing_nonempty_and_parents() {
        let (tmp, client) = root();
        assert!(delete(&client, &FolderId::new(INBOX)).is_err());
        assert!(delete(&client, &FolderId::new(".Ghost")).is_err());

        create(&client, "Full").unwrap();
        fs::write(tmp.path().join(".Full/cur/msg.host:2,S"), "x").unwrap();
        assert!(delete(&client, &FolderId::new(".Full")).is_err());

        create(&client, "Parent").unwrap();
        create(&client, "Parent/Child").unwrap();
        assert!(delete(&client, &FolderId::new(".Parent")).is_err());
    }

    #[test]
    fn delete_removes_an_empty_leaf() {
        let (tmp, client) = root();
        create(&client, "Scratch").unwrap();
        delete(&client, &FolderId::new(".Scratch")).unwrap();
        assert!(!tmp.path().join(".Scratch").exists());
    }

    #[test]
    fn rename_moves_the_folder_and_its_children() {
        let (tmp, client) = root();
        create(&client, "Old").unwrap();
        create(&client, "Old/Sub").unwrap();
        fs::write(tmp.path().join(".Old.Sub/cur/msg.host:2,S"), "x").unwrap();

        rename(&client, &FolderId::new(".Old"), "New/Name").unwrap();
        assert!(!tmp.path().join(".Old").exists());
        assert!(is_maildir(&tmp.path().join(".New.Name")));
        assert!(
            tmp.path().join(".New.Name.Sub/cur/msg.host:2,S").is_file(),
            "child folder contents must move with the parent"
        );
    }

    #[test]
    fn rename_refuses_inbox_and_existing_targets() {
        let (_tmp, client) = root();
        create(&client, "A").unwrap();
        create(&client, "B").unwrap();
        assert!(rename(&client, &FolderId::new(INBOX), "C").is_err());
        assert!(rename(&client, &FolderId::new(".A"), "B").is_err());
    }
}
