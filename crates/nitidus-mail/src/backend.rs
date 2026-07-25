//! The backend abstraction every mail source implements. Backends are
//! used generically (one actor monomorphized per backend type), so
//! methods use RPITIT futures with explicit `Send` bounds and no boxing.

use std::future::Future;

use crate::error::MailError;
use crate::types::{EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

/// Contract for `scan_envelopes`: batches stream through the sender so
/// channel capacity applies backpressure inside the backend. When the
/// receiving side disconnects (job cancelled), the backend must stop
/// and return `MailError::Cancelled`.
pub trait MailBackend: Send + 'static {
    fn list_folders(&mut self) -> impl Future<Output = Result<Vec<FolderMeta>, MailError>> + Send;

    fn scan_envelopes(
        &mut self,
        folder: &FolderId,
        batches: flume::Sender<Vec<EnvelopeSummary>>,
    ) -> impl Future<Output = Result<(), MailError>> + Send;

    fn fetch_message(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
    ) -> impl Future<Output = Result<Vec<u8>, MailError>> + Send;

    fn set_flags(
        &mut self,
        folder: &FolderId,
        id: &EnvelopeId,
        flags: Flags,
    ) -> impl Future<Output = Result<(), MailError>> + Send;

    /// `name` is a display path (`Archive/2024`); each backend encodes
    /// it into its own folder-id scheme.
    fn create_folder(&mut self, name: &str) -> impl Future<Output = Result<(), MailError>> + Send;

    /// Must refuse non-empty folders and folders with children — the UI
    /// deliberately has no destructive-delete path.
    fn delete_folder(
        &mut self,
        folder: &FolderId,
    ) -> impl Future<Output = Result<(), MailError>> + Send;

    fn rename_folder(
        &mut self,
        folder: &FolderId,
        new_name: &str,
    ) -> impl Future<Output = Result<(), MailError>> + Send;
}
