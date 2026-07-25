//! Scripted in-memory backend for tests, future UI harnesses, and an
//! offline demo mode.

use std::collections::HashMap;
use std::time::Duration;

use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

#[derive(Default)]
pub struct MockBackend {
    folders: Vec<FolderMeta>,
    envelopes: HashMap<FolderId, Vec<EnvelopeSummary>>,
    batch_size: usize,
    batch_delay: Duration,
    fail_scan: bool,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            batch_size: 100,
            ..Self::default()
        }
    }

    pub fn with_folder(mut self, name: &str, envelope_count: usize) -> Self {
        let id = FolderId::new(name);
        let envelopes = generate_envelopes(&id, envelope_count);
        self.folders.push(FolderMeta {
            id: id.clone(),
            name: name.to_owned(),
            unread: 0,
            total: u32::try_from(envelope_count).unwrap_or(u32::MAX),
        });
        self.envelopes.insert(id, envelopes);
        self
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    pub fn with_batch_delay(mut self, delay: Duration) -> Self {
        self.batch_delay = delay;
        self
    }

    pub fn with_failing_scan(mut self) -> Self {
        self.fail_scan = true;
        self
    }
}

/// Every third message replies to its predecessor, so scripted folders
/// contain small threads for threading tests.
pub fn generate_envelopes(folder: &FolderId, count: usize) -> Vec<EnvelopeSummary> {
    (0..count)
        .map(|index| EnvelopeSummary {
            id: EnvelopeId::new(format!("{folder}-{index}")),
            subject: format!("Message {index}"),
            from_display: "Mock Sender".to_owned(),
            from_addr: "mock@example.com".to_owned(),
            date_epoch_secs: 1_700_000_000 + index as i64,
            flags: Flags::default(),
            message_id: format!("{folder}-{index}@mock"),
            references: if index % 3 == 0 {
                Vec::new()
            } else {
                vec![format!("{folder}-{}@mock", index - 1)]
            },
        })
        .collect()
}

impl crate::backend::MailBackend for MockBackend {
    async fn list_folders(&mut self) -> Result<Vec<FolderMeta>, MailError> {
        Ok(self.folders.clone())
    }

    async fn scan_envelopes(
        &mut self,
        folder: &FolderId,
        batches: flume::Sender<Vec<EnvelopeSummary>>,
    ) -> Result<(), MailError> {
        if self.fail_scan {
            return Err(MailError::Backend("scripted scan failure".to_owned()));
        }
        let envelopes = self
            .envelopes
            .get(folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?
            .clone();
        for chunk in envelopes.chunks(self.batch_size) {
            if !self.batch_delay.is_zero() {
                tokio::time::sleep(self.batch_delay).await;
            }
            if batches.send_async(chunk.to_vec()).await.is_err() {
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
        Ok(format!("Subject: mock message {id} in {folder}\r\n\r\nbody\r\n").into_bytes())
    }

    async fn set_flags(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        flags: Flags,
    ) -> Result<(), MailError> {
        let envelopes = self
            .envelopes
            .get_mut(folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?;
        for envelope in envelopes.iter_mut().filter(|e| &e.id == id) {
            envelope.flags = flags;
        }
        Ok(())
    }

    async fn create_folder(&mut self, name: &str) -> Result<(), MailError> {
        let id = FolderId::new(name);
        if self.folders.iter().any(|meta| meta.id == id) {
            return Err(MailError::Backend(format!("folder already exists: {name}")));
        }
        self.folders.push(FolderMeta {
            id: id.clone(),
            name: name.to_owned(),
            unread: 0,
            total: 0,
        });
        self.envelopes.insert(id, Vec::new());
        Ok(())
    }

    async fn delete_folder(&mut self, folder: &FolderId) -> Result<(), MailError> {
        let is_empty = self.envelopes.get(folder).is_some_and(Vec::is_empty);
        if !is_empty {
            return Err(MailError::Backend(format!(
                "folder missing or not empty, refusing to delete: {folder}"
            )));
        }
        self.folders.retain(|meta| &meta.id != folder);
        self.envelopes.remove(folder);
        Ok(())
    }

    async fn delete_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> Result<(), MailError> {
        let envelopes = self
            .envelopes
            .get_mut(folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?;
        envelopes.retain(|envelope| &envelope.id != id);
        Ok(())
    }

    async fn move_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        target: &FolderId,
    ) -> Result<(), MailError> {
        let source = self
            .envelopes
            .get_mut(folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?;
        let position = source
            .iter()
            .position(|envelope| &envelope.id == id)
            .ok_or_else(|| MailError::Backend(format!("no such message: {id}")))?;
        let moved = source.remove(position);
        self.envelopes
            .get_mut(target)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {target}")))?
            .push(moved);
        Ok(())
    }

    async fn append_message(
        &mut self,
        folder: &FolderId,
        bytes: Vec<u8>,
        flags: Flags,
    ) -> Result<(), MailError> {
        let envelopes = self
            .envelopes
            .get_mut(folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?;
        let index = envelopes.len();
        let mut envelope = crate::envelope::summarize_headers(
            &bytes,
            EnvelopeId::new(format!("{folder}-appended-{index}")),
            flags,
            0,
        );
        envelope.flags = flags;
        envelopes.push(envelope);
        Ok(())
    }

    async fn rename_folder(&mut self, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
        let meta = self
            .folders
            .iter_mut()
            .find(|meta| &meta.id == folder)
            .ok_or_else(|| MailError::Backend(format!("no such folder: {folder}")))?;
        let new_id = FolderId::new(new_name);
        meta.id = new_id.clone();
        meta.name = new_name.to_owned();
        if let Some(envelopes) = self.envelopes.remove(folder) {
            self.envelopes.insert(new_id, envelopes);
        }
        Ok(())
    }
}
