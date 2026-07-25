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
    /// Folder ops reply with a refreshed `Folders` event on success and
    /// `JobFailed` on error; `name` values are display paths.
    CreateFolder {
        name: String,
    },
    DeleteFolder {
        folder: FolderId,
    },
    RenameFolder {
        folder: FolderId,
        new_name: String,
    },
    Cancel(JobId),
    Shutdown,
}
