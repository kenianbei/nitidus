//! The maildir `MailBackend`. Filesystem work runs on the blocking
//! pool so actor threads never stall on IO.

use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::MailBackend;
use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

use super::{folder_ops, folders, message};

const SCAN_BATCH_SIZE: usize = 500;

pub struct MaildirBackend {
    root: PathBuf,
}

impl MaildirBackend {
    pub fn new(root: PathBuf) -> Result<Self, MailError> {
        folders::validate_root(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl MailBackend for MaildirBackend {
    async fn list_folders(&mut self) -> Result<Vec<FolderMeta>, MailError> {
        let root = self.root.clone();
        run_blocking(move || folders::discover(&root)).await
    }

    async fn scan_envelopes(
        &mut self,
        folder: &FolderId,
        batches: flume::Sender<Vec<EnvelopeSummary>>,
    ) -> Result<(), MailError> {
        let dir = folders::folder_dir(&self.root, folder);
        let files = {
            let dir = dir.clone();
            run_blocking(move || list_message_files(&dir)).await?
        };
        for chunk in files.chunks(SCAN_BATCH_SIZE) {
            let chunk = chunk.to_vec();
            let batch = run_blocking(move || parse_chunk(&chunk)).await?;
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
        let dir = folders::folder_dir(&self.root, folder);
        let id = id.clone();
        run_blocking(move || {
            let path = message::find_message(&dir, &id)?;
            fs::read(&path)
                .map_err(|error| MailError::Backend(format!("read {}: {error}", path.display())))
        })
        .await
    }

    async fn set_flags(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        flags: Flags,
    ) -> Result<(), MailError> {
        let dir = folders::folder_dir(&self.root, folder);
        let id = id.clone();
        run_blocking(move || {
            let current = message::find_message(&dir, &id)?;
            message::rename_with_flags(&dir, &current, &id, flags)?;
            Ok(())
        })
        .await
    }

    async fn create_folder(&mut self, name: &str) -> Result<(), MailError> {
        let root = self.root.clone();
        let name = name.to_owned();
        run_blocking(move || folder_ops::create(&root, &name)).await
    }

    async fn delete_folder(&mut self, folder: &FolderId) -> Result<(), MailError> {
        let root = self.root.clone();
        let folder = folder.clone();
        run_blocking(move || folder_ops::delete(&root, &folder)).await
    }

    async fn rename_folder(&mut self, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
        let root = self.root.clone();
        let folder = folder.clone();
        let new_name = new_name.to_owned();
        run_blocking(move || folder_ops::rename(&root, &folder, &new_name)).await
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

fn list_message_files(folder_dir: &Path) -> Result<Vec<(PathBuf, bool)>, MailError> {
    let mut files = Vec::new();
    for (sub, in_new) in [("new", true), ("cur", false)] {
        let dir = folder_dir.join(sub);
        let entries = fs::read_dir(&dir)
            .map_err(|error| MailError::Backend(format!("read {}: {error}", dir.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                files.push((path, in_new));
            }
        }
    }
    Ok(files)
}

fn parse_chunk(chunk: &[(PathBuf, bool)]) -> Result<Vec<EnvelopeSummary>, MailError> {
    let mut batch = Vec::with_capacity(chunk.len());
    for (path, in_new) in chunk {
        match message::parse_envelope(path, *in_new) {
            Ok(envelope) => batch.push(envelope),
            Err(error) => tracing::warn!("skipping unreadable message {}: {error}", path.display()),
        }
    }
    Ok(batch)
}
