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
}
