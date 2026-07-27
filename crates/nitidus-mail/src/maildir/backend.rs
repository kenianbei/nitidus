//! The maildir `MailBackend`. Filesystem work runs on the blocking
//! pool so actor threads never stall on IO.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use io_maildir::client::MaildirClient;
use io_maildir::entry::MaildirEntry;
use io_maildir::maildir::{Maildir, MaildirSubdir};

use crate::backend::MailBackend;
use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

use super::{flags, folder_ops, folders, scan};

const SCAN_BATCH_SIZE: usize = 500;

pub struct MaildirBackend {
    root: PathBuf,
    client: Arc<MaildirClient>,
}

impl MaildirBackend {
    pub fn new(root: PathBuf) -> Result<Self, MailError> {
        folders::validate_root(&root)?;
        Ok(Self {
            client: Arc::new(folders::build_client(&root)),
            root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn open(&self, folder: &FolderId) -> Result<Maildir, MailError> {
        self.client
            .load_maildir(folders::folder_path(folder))
            .map_err(|error| MailError::Backend(format!("open folder {folder}: {error}")))
    }
}

impl MailBackend for MaildirBackend {
    async fn list_folders(&mut self) -> Result<Vec<FolderMeta>, MailError> {
        let client = Arc::clone(&self.client);
        run_blocking(move || folders::list_maildirs(&client)).await
    }

    async fn scan_envelopes(
        &mut self,
        folder: &FolderId,
        batches: flume::Sender<Vec<EnvelopeSummary>>,
    ) -> Result<(), MailError> {
        let maildir = self.open(folder)?;
        let entries = {
            let client = Arc::clone(&self.client);
            run_blocking(move || {
                client
                    .list_entries(maildir)
                    .map(|entries| entries.into_iter().collect::<Vec<_>>())
                    .map_err(|error| MailError::Backend(format!("list entries: {error}")))
            })
            .await?
        };
        for chunk in entries.chunks(SCAN_BATCH_SIZE) {
            let chunk = chunk.to_vec();
            let batch = run_blocking(move || Ok(parse_chunk(&chunk))).await?;
            if batches.send_async(batch).await.is_err() {
                return Err(MailError::Cancelled);
            }
        }
        Ok(())
    }

    async fn fetch_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> Result<Vec<u8>, MailError> {
        let path = self.locate(folder, id).await?;
        run_blocking(move || {
            fs::read(&path)
                .map_err(|error| MailError::Backend(format!("read {}: {error}", path.display())))
        })
        .await
    }

    /// Flags always land the message in `cur/`: a message that has been
    /// acted on is no longer new. Upstream's `MaildirFlagsSet` is a
    /// silent no-op for entries in `new/`, so the placement is ours and
    /// only the suffix encoding is theirs (§3.3 finding 11).
    async fn set_flags(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        flags: Flags,
    ) -> Result<(), MailError> {
        let maildir = self.open(folder)?;
        let current = self.locate(folder, id).await?;
        let id = id.clone();
        run_blocking(move || {
            let name = format!("{}:2,{}", id.as_str(), flags::to_maildir(flags));
            let target = PathBuf::from(maildir.cur().as_str()).join(name);
            fs::rename(&current, &target).map_err(|error| {
                MailError::Backend(format!("set flags on {}: {error}", current.display()))
            })
        })
        .await
    }

    async fn create_folder(&mut self, name: &str) -> Result<(), MailError> {
        let client = Arc::clone(&self.client);
        let name = name.to_owned();
        run_blocking(move || folder_ops::create(&client, &name)).await
    }

    async fn delete_folder(&mut self, folder: &FolderId) -> Result<(), MailError> {
        let client = Arc::clone(&self.client);
        let folder = folder.clone();
        run_blocking(move || folder_ops::delete(&client, &folder)).await
    }

    async fn rename_folder(&mut self, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
        let client = Arc::clone(&self.client);
        let folder = folder.clone();
        let new_name = new_name.to_owned();
        run_blocking(move || folder_ops::rename(&client, &folder, &new_name)).await
    }

    async fn delete_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> Result<(), MailError> {
        let path = self.locate(folder, id).await?;
        run_blocking(move || {
            fs::remove_file(&path)
                .map_err(|error| MailError::Backend(format!("delete {}: {error}", path.display())))
        })
        .await
    }

    async fn move_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        target: &FolderId,
    ) -> Result<(), MailError> {
        let source = self.open(folder)?;
        let destination = self.open(target)?;
        let client = Arc::clone(&self.client);
        let id = id.clone();
        run_blocking(move || {
            client
                .r#move(id.as_str(), source, destination, Some(MaildirSubdir::Cur))
                .map_err(|error| MailError::Backend(format!("move {id}: {error}")))
        })
        .await
    }

    async fn append_message(
        &mut self,
        folder: &FolderId,
        bytes: Vec<u8>,
        flags: Flags,
    ) -> Result<(), MailError> {
        let maildir = self.open(folder)?;
        let client = Arc::clone(&self.client);
        run_blocking(move || {
            client
                .store(maildir, MaildirSubdir::Cur, flags::to_maildir(flags), bytes)
                .map(|_delivered| ())
                .map_err(|error| MailError::Backend(format!("append: {error}")))
        })
        .await
    }
}

impl MaildirBackend {
    async fn locate(&self, folder: &FolderId, id: &EnvelopeId) -> Result<PathBuf, MailError> {
        let maildir = self.open(folder)?;
        let client = Arc::clone(&self.client);
        let id = id.clone();
        run_blocking(move || {
            client
                .locate(maildir, id.as_str())
                .map(|(path, _subdir, _flags)| PathBuf::from(path.as_str()))
                .map_err(|error| MailError::Backend(format!("message not found: {id}: {error}")))
        })
        .await
    }
}

async fn run_blocking<T, F>(work: F) -> Result<T, MailError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, MailError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|join_error| MailError::Backend(format!("blocking task failed: {join_error}")))?
}

fn parse_chunk(chunk: &[MaildirEntry]) -> Vec<EnvelopeSummary> {
    chunk
        .iter()
        .filter_map(|entry| match scan::parse_envelope(entry) {
            Ok(envelope) => Some(envelope),
            Err(error) => {
                tracing::warn!("skipping unreadable message {}: {error}", entry.path());
                None
            }
        })
        .collect()
}
