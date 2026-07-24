//! Commands flow from the UI into account actors.

use crate::types::{EnvelopeId, Flags, FolderId, JobId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailCommand {
    ListFolders,
    SyncEnvelopes {
        folder: FolderId,
        job: JobId,
    },
    FetchMessage {
        folder: FolderId,
        id: EnvelopeId,
        job: JobId,
    },
    SetFlags {
        folder: FolderId,
        id: EnvelopeId,
        flags: Flags,
    },
    Cancel(JobId),
    Shutdown,
}
