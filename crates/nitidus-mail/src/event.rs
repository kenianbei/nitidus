//! Events flow from account actors back to the UI drain.

use crate::error::MailError;
use crate::thread::ThreadRow;
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
    /// An outgoing submission completed successfully.
    SendDone {
        account: AccountId,
        job: JobId,
    },
    /// A folder's contents changed on disk outside our control
    /// (external delivery, another client). Consumers re-sync.
    FolderChanged {
        account: AccountId,
        folder: FolderId,
    },
    /// Result of a `compute_threads` job (or, later, a server THREAD
    /// response — the event is backend-agnostic).
    Threads {
        account: AccountId,
        folder: FolderId,
        job: JobId,
        rows: Vec<ThreadRow>,
    },
}
