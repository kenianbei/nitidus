//! The IMAP `MailBackend`: one command connection, per-folder session
//! sync state, and full-folder streaming that satisfies the store's
//! prune-on-done contract while re-scans stay incremental.

use std::collections::HashMap;

use io_imap::rfc3501::append::{ImapMessageAppend, ImapMessageAppendOptions};
use io_imap::rfc3501::create::ImapMailboxCreate;
use io_imap::rfc3501::delete::ImapMailboxDelete;
use io_imap::rfc3501::expunge::ImapMailboxExpunge;
use io_imap::rfc3501::fetch::{ImapMessageFetch, ImapMessageFetchOptions};
use io_imap::rfc3501::rename::ImapMailboxRename;
use io_imap::rfc3501::store::{ImapMessageStoreOptions, ImapMessageStoreSilent};
use io_imap::types::flag::StoreType;

use super::envelopes::{
    FETCH_WINDOW, FolderSync, body_of, envelope_fetch_items, imap_flags, message_body_items,
    parse_envelope_items, sequence_range, single_uid,
};
use super::folders::{self, DEFAULT_DELIMITER, encode_name};
use super::session::{ImapSession, parse_mailbox};
use super::sync;
use super::{INBOX, ImapConfig};
use crate::backend::MailBackend;
use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

pub struct ImapBackend {
    pub(super) session: ImapSession,
    pub(super) folders: HashMap<FolderId, FolderSync>,
    pub(super) delimiter: char,
}

impl ImapBackend {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            session: ImapSession::new(config),
            folders: HashMap::new(),
            delimiter: DEFAULT_DELIMITER,
        }
    }

    async fn fetch_window(
        &mut self,
        folder: &FolderId,
        start: u32,
        end: u32,
    ) -> Result<Vec<(u32, EnvelopeSummary)>, MailError> {
        let range = sequence_range(start, end)?;
        let items = envelope_fetch_items()?;
        let fetched = self
            .session
            .run_selected(folder.as_str(), || {
                ImapMessageFetch::new(
                    range.clone(),
                    items.clone(),
                    ImapMessageFetchOptions::default(),
                )
            })
            .await?;
        Ok(fetched
            .values()
            .filter_map(|entry| parse_envelope_items(entry.as_ref()))
            .collect())
    }

    fn require_empty_guard(total: u32, folder: &FolderId) -> Result<(), MailError> {
        if total > 0 {
            return Err(MailError::Backend(format!(
                "folder not empty, refusing to delete: {folder}"
            )));
        }
        Ok(())
    }
}

impl MailBackend for ImapBackend {
    async fn list_folders(&mut self) -> Result<Vec<FolderMeta>, MailError> {
        let (folders, delimiter) = folders::list_folders(&mut self.session).await?;
        self.delimiter = delimiter;
        Ok(folders)
    }

    async fn scan_envelopes(
        &mut self,
        folder: &FolderId,
        batches: flume::Sender<Vec<EnvelopeSummary>>,
    ) -> Result<(), MailError> {
        sync::scan(self, folder, &batches).await
    }

    async fn fetch_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> Result<Vec<u8>, MailError> {
        let uid = parse_uid(id)?;
        let set = single_uid(uid)?;
        let fetched = self
            .session
            .run_selected(folder.as_str(), || {
                ImapMessageFetch::new(
                    set.clone(),
                    message_body_items(),
                    ImapMessageFetchOptions {
                        uid: true,
                        ..Default::default()
                    },
                )
            })
            .await?;
        fetched
            .values()
            .find_map(|entry| body_of(entry.as_ref()))
            .ok_or_else(|| MailError::Backend(format!("message {id} not found in {folder}")))
    }

    async fn set_flags(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        flags: Flags,
    ) -> Result<(), MailError> {
        let uid = parse_uid(id)?;
        let set = single_uid(uid)?;
        let imap_flags = imap_flags(flags);
        self.session
            .run_selected(folder.as_str(), || {
                ImapMessageStoreSilent::new(
                    set.clone(),
                    StoreType::Replace,
                    imap_flags.clone(),
                    ImapMessageStoreOptions { uid: true },
                )
            })
            .await?;
        if let Some(state) = self.folders.get_mut(folder)
            && let Some(envelope) = state.envelopes.get_mut(&uid)
        {
            envelope.flags = flags;
        }
        Ok(())
    }

    async fn create_folder(&mut self, name: &str) -> Result<(), MailError> {
        let mailbox = parse_mailbox(&encode_name(name, self.delimiter))?;
        self.session
            .run(|| ImapMailboxCreate::new(mailbox.clone()))
            .await
    }

    async fn delete_folder(&mut self, folder: &FolderId) -> Result<(), MailError> {
        if folder.as_str().eq_ignore_ascii_case(INBOX) {
            return Err(MailError::Backend("cannot delete INBOX".to_owned()));
        }
        let data = self.session.select(folder.as_str()).await?;
        Self::require_empty_guard(data.exists.unwrap_or(0), folder)?;
        let mailbox = parse_mailbox(folder.as_str())?;
        self.session
            .run(|| ImapMailboxDelete::new(mailbox.clone()))
            .await?;
        self.folders.remove(folder);
        Ok(())
    }

    /// `\Deleted` + whole-folder EXPUNGE — scoped to draft
    /// replacement, where stray deleted-flag collateral is acceptable.
    async fn delete_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> Result<(), MailError> {
        let uid = parse_uid(id)?;
        let set = single_uid(uid)?;
        let deleted = vec![io_imap::types::flag::Flag::Deleted];
        self.session
            .run_selected(folder.as_str(), || {
                ImapMessageStoreSilent::new(
                    set.clone(),
                    StoreType::Add,
                    deleted.clone(),
                    ImapMessageStoreOptions { uid: true },
                )
            })
            .await?;
        self.session
            .run_selected(folder.as_str(), ImapMailboxExpunge::new)
            .await
            .map(|_expunged| ())?;
        if let Some(state) = self.folders.get_mut(folder) {
            state.envelopes.remove(&uid);
        }
        Ok(())
    }

    async fn append_message(
        &mut self,
        folder: &FolderId,
        bytes: Vec<u8>,
        flags: Flags,
    ) -> Result<(), MailError> {
        let mailbox = parse_mailbox(folder.as_str())?;
        let options = ImapMessageAppendOptions {
            flags: super::envelopes::imap_flags(flags),
            ..Default::default()
        };
        self.session
            .run(|| ImapMessageAppend::new(mailbox.clone(), bytes.clone(), options.clone()))
            .await
            .map(|_uidplus| ())
    }

    async fn rename_folder(&mut self, folder: &FolderId, new_name: &str) -> Result<(), MailError> {
        if folder.as_str().eq_ignore_ascii_case(INBOX) {
            return Err(MailError::Backend("cannot rename INBOX".to_owned()));
        }
        let from = parse_mailbox(folder.as_str())?;
        let to = parse_mailbox(&encode_name(new_name, self.delimiter))?;
        self.session
            .run(|| ImapMailboxRename::new(from.clone(), to.clone()))
            .await?;
        self.folders.remove(folder);
        Ok(())
    }
}

pub(super) fn parse_uid(id: &EnvelopeId) -> Result<u32, MailError> {
    id.as_str()
        .parse::<u32>()
        .map_err(|_| MailError::Backend(format!("not an IMAP uid: {id}")))
}

impl ImapBackend {
    pub(super) async fn fetch_full(
        &mut self,
        folder: &FolderId,
        exists: u32,
        batches: &flume::Sender<Vec<EnvelopeSummary>>,
    ) -> Result<FolderSync, MailError> {
        let mut state = FolderSync::default();
        let mut start = 1u32;
        while start <= exists {
            let end = (start + FETCH_WINDOW as u32 - 1).min(exists);
            let window = self.fetch_window(folder, start, end).await?;
            let batch: Vec<EnvelopeSummary> = window
                .iter()
                .map(|(_, envelope)| envelope.clone())
                .collect();
            for (uid, envelope) in window {
                state.envelopes.insert(uid, envelope);
            }
            if !batch.is_empty() && batches.send_async(batch).await.is_err() {
                return Err(MailError::Cancelled);
            }
            start = end + 1;
        }
        Ok(state)
    }
}
