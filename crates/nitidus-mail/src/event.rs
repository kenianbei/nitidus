//! Events flow from account actors back to the UI drain.

use crate::error::MailError;
use crate::types::{
    AccountId, ConnectionState, EnvelopeId, EnvelopeSummary, FolderId, FolderMeta, JobId,
};

#[derive(Clone, Debug)]
pub enum MailEvent {
    Connection {
        account: AccountId,
        state: ConnectionState,
    },
    Folders {
        account: AccountId,
        folders: Vec<FolderMeta>,
    },
    EnvelopeBatch {
        account: AccountId,
        folder: FolderId,
        job: JobId,
        batch: Vec<EnvelopeSummary>,
        done: bool,
    },
    Message {
        account: AccountId,
        folder: FolderId,
        id: EnvelopeId,
        job: JobId,
        raw: Vec<u8>,
    },
    JobFailed {
        account: AccountId,
        job: Option<JobId>,
        error: MailError,
    },
}
