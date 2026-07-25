//! Engine-level maildir change watching. Lives outside the backend
//! because an actor's `&mut backend` cannot host a long-running watch.
//! Raw notify events coalesce per folder within a quiet window, so a
//! delivery's tmp-write + rename burst emits one `FolderChanged`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::engine::MailEngine;
use crate::event::MailEvent;
use crate::maildir::folders;
use crate::types::{AccountId, FolderId};

const QUIET_WINDOW: Duration = Duration::from_millis(500);

impl MailEngine {
    /// Watches every folder's `new/` and `cur/` (non-recursively — new
    /// folders created after startup are not watched until restart).
    pub fn watch_maildir(&mut self, account: AccountId, root: PathBuf) {
        let events = self.events_sender();
        let id = account.clone();
        let handle = self.runtime_handle().spawn(async move {
            if let Err(error) = run_watcher(account.clone(), &root, events).await {
                tracing::warn!("maildir watcher for {account} stopped: {error}");
            }
        });
        self.track_watcher(id, handle);
    }
}

async fn run_watcher(
    account: AccountId,
    root: &Path,
    events: flume::Sender<MailEvent>,
) -> Result<(), String> {
    let watched = watchable_dirs(root).map_err(|error| error.to_string())?;
    let (raw_tx, raw_rx) = flume::unbounded::<notify::Event>();
    let mut watcher = notify::recommended_watcher(move |result| {
        if let Ok(event) = result {
            let _sent = raw_tx.send(event);
        }
    })
    .map_err(|error| error.to_string())?;
    for dir in &watched {
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("watch {}: {error}", dir.display()))?;
    }
    forward_coalesced(account, root, &raw_rx, &events).await;
    Ok(())
}

async fn forward_coalesced(
    account: AccountId,
    root: &Path,
    raw: &flume::Receiver<notify::Event>,
    events: &flume::Sender<MailEvent>,
) {
    while let Ok(first) = raw.recv_async().await {
        let mut changed = HashSet::new();
        collect_folders(root, &first, &mut changed);
        while let Ok(Ok(event)) = tokio::time::timeout(QUIET_WINDOW, raw.recv_async()).await {
            collect_folders(root, &event, &mut changed);
        }
        for folder in changed {
            let event = MailEvent::FolderChanged {
                account: account.clone(),
                folder,
            };
            let _sent = events.send_async(event).await;
        }
    }
}

fn collect_folders(root: &Path, event: &notify::Event, changed: &mut HashSet<FolderId>) {
    for path in &event.paths {
        if let Some(folder) = folder_of_path(root, path) {
            changed.insert(folder);
        }
    }
}

/// Maps `<root>[/<folder>]/{new,cur}/<file>` back to its folder id.
fn folder_of_path(root: &Path, path: &Path) -> Option<FolderId> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let first = components.next()?.as_os_str().to_str()?;
    if first == "new" || first == "cur" {
        return Some(FolderId::new(folders::INBOX));
    }
    let second = components.next()?.as_os_str().to_str()?;
    if second == "new" || second == "cur" {
        Some(FolderId::new(first))
    } else {
        None
    }
}

fn watchable_dirs(root: &Path) -> Result<Vec<PathBuf>, crate::error::MailError> {
    let mut dirs = Vec::new();
    for folder in folders::discover(root)? {
        let dir = folders::folder_dir(root, &folder.id);
        dirs.push(dir.join("new"));
        dirs.push(dir.join("cur"));
    }
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn maps_paths_to_folders() {
        let root = Path::new("/mail");
        assert_eq!(
            folder_of_path(root, Path::new("/mail/new/msg")),
            Some(FolderId::new("INBOX"))
        );
        assert_eq!(
            folder_of_path(root, Path::new("/mail/.Sent/cur/msg")),
            Some(FolderId::new(".Sent"))
        );
        assert_eq!(folder_of_path(root, Path::new("/mail/tmp/msg")), None);
        assert_eq!(folder_of_path(root, Path::new("/elsewhere/new/m")), None);
    }
}
