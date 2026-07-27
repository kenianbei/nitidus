//! The Maildir++ layout decision, in one place: how a store is built,
//! and how our `FolderId` translates to `io-maildir`'s logical
//! `MaildirPath`. `FolderId` stays the on-disk directory name
//! (`.Archive.2024`), which the cache and config already persist; the
//! logical name (`Archive/2024`) is what the store resolves against.

use std::fs;
use std::path::{Path, PathBuf};

use io_maildir::client::MaildirClient;
use io_maildir::maildir::Maildir;
use io_maildir::path::{MaildirFsPath, MaildirPath};
use io_maildir::store::MaildirStore;

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

/// Maildir++ is the layout we promise for mbsync/offlineimap
/// compatibility, so undotted sibling directories are not folders.
pub fn build_store(root: &Path) -> MaildirStore {
    MaildirStore {
        root: MaildirFsPath::new(root.to_string_lossy().into_owned()),
        maildirpp: true,
    }
}

pub fn build_client(root: &Path) -> MaildirClient {
    let mut client = MaildirClient::new(MaildirFsPath::new(root.to_string_lossy().into_owned()));
    client.store = build_store(root);
    client
}

pub fn list_maildirs(client: &MaildirClient) -> Result<Vec<FolderMeta>, MailError> {
    let maildirs = client
        .list_maildirs()
        .map_err(|error| MailError::Backend(format!("list maildirs: {error}")))?;
    let mut folders: Vec<FolderMeta> = maildirs
        .iter()
        .map(|maildir| folder_meta(&client.store, maildir))
        .collect();
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

/// The `new`/`cur` directories the filesystem watcher subscribes to.
pub fn watched_dirs(root: &Path) -> Result<Vec<PathBuf>, MailError> {
    let client = build_client(root);
    let mut dirs = Vec::new();
    for folder in list_maildirs(&client)? {
        let dir = PathBuf::from(folder_dir(&client.store, &folder.id).as_str());
        dirs.push(dir.join("new"));
        dirs.push(dir.join("cur"));
    }
    Ok(dirs)
}

/// `INBOX` is the store root, so it maps to the empty logical path.
pub fn folder_path(folder: &FolderId) -> MaildirPath {
    if folder.as_str() == INBOX {
        return MaildirPath::default();
    }
    MaildirPath::from(decode_dot_name(folder.as_str()))
}

pub fn folder_dir(store: &MaildirStore, folder: &FolderId) -> MaildirFsPath {
    store.resolve(&folder_path(folder))
}

fn folder_meta(store: &MaildirStore, maildir: &Maildir) -> FolderMeta {
    let is_root = maildir.path() == &store.root;
    let (id, name) = if is_root {
        (FolderId::new(INBOX), INBOX.to_owned())
    } else {
        let dir_name = maildir.name().unwrap_or_default();
        let logical = store
            .relative(maildir.path())
            .map(|path| path.as_str().to_owned())
            .unwrap_or_else(|| decode_dot_name(dir_name));
        (FolderId::new(dir_name), logical)
    };
    let unread = count_files(&maildir.new());
    let cur = count_files(&maildir.cur());
    FolderMeta {
        id,
        name,
        unread,
        total: unread + cur,
    }
}

pub(super) fn is_maildir(dir: &Path) -> bool {
    dir.is_dir() && dir.join("cur").is_dir() && dir.join("new").is_dir() && dir.join("tmp").is_dir()
}

fn count_files(dir: &MaildirFsPath) -> u32 {
    fs::read_dir(dir.as_str())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn decode_dot_name(dir_name: &str) -> String {
    match dir_name.strip_prefix('.') {
        Some(dotted) => dotted.replace('.', "/"),
        None => dir_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn store() -> MaildirStore {
        MaildirStore {
            root: MaildirFsPath::new("/mail"),
            maildirpp: true,
        }
    }

    #[test]
    fn inbox_resolves_to_the_store_root() {
        assert_eq!(
            folder_dir(&store(), &FolderId::new(INBOX)).as_str(),
            "/mail"
        );
    }

    #[test]
    fn dot_names_round_trip_through_the_logical_path() {
        assert_eq!(
            folder_dir(&store(), &FolderId::new(".Archive.2024")).as_str(),
            "/mail/.Archive.2024"
        );
        assert_eq!(
            folder_dir(&store(), &FolderId::new(".Sent")).as_str(),
            "/mail/.Sent"
        );
    }

    #[test]
    fn decodes_maildir_plus_plus_names() {
        assert_eq!(decode_dot_name(".Archive.2024"), "Archive/2024");
        assert_eq!(decode_dot_name(".Sent"), "Sent");
        assert_eq!(decode_dot_name("Archive"), "Archive");
    }
}
