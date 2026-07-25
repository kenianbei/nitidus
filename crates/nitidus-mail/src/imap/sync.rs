//! The scan strategy: first sight of a folder (or a UIDVALIDITY bump)
//! streams a full windowed fetch; later scans apply CONDSTORE flag
//! deltas, fetch new UIDs, reconcile expunges via UID SEARCH, and then
//! stream the merged map in full — so consumers always see a complete
//! folder and prune-on-done stays correct.

use io_imap::rfc3501::fetch::{ImapMessageFetch, ImapMessageFetchOptions};
use io_imap::rfc3501::search::{ImapMessageSearch, ImapMessageSearchOptions};
use io_imap::types::command::FetchModifier;
use io_imap::types::core::Vec1;
use io_imap::types::search::SearchKey;

use super::backend::ImapBackend;
use super::envelopes::{
    FolderSync, envelope_fetch_items, flags_fetch_items, flags_of, parse_envelope_items, uid_of,
    uid_range_from,
};
use crate::error::MailError;
use crate::types::{EnvelopeSummary, FolderId};

pub(super) async fn scan(
    backend: &mut ImapBackend,
    folder: &FolderId,
    batches: &flume::Sender<Vec<EnvelopeSummary>>,
) -> Result<(), MailError> {
    let data = backend.session.select(folder.as_str()).await?;
    let uid_validity = data.uid_validity.map(|value| value.get());
    let highest_mod_seq = data.highest_mod_seq;
    let exists = data.exists.unwrap_or(0);

    let known = backend.folders.get(folder);
    let is_incremental = known.is_some_and(|state| {
        !state.envelopes.is_empty() && state.uid_validity == uid_validity && uid_validity.is_some()
    });

    let mut state = if is_incremental {
        let mut state = backend.folders.remove(folder).unwrap_or_default();
        refresh_incremental(backend, folder, &mut state).await?;
        stream_full(&state, batches).await?;
        state
    } else {
        backend.fetch_full(folder, exists, batches).await?
    };
    state.uid_validity = uid_validity;
    state.highest_mod_seq = highest_mod_seq;
    backend.folders.insert(folder.clone(), state);

    batches
        .send_async(Vec::new())
        .await
        .map_err(|_| MailError::Cancelled)
}

async fn refresh_incremental(
    backend: &mut ImapBackend,
    folder: &FolderId,
    state: &mut FolderSync,
) -> Result<(), MailError> {
    apply_flag_deltas(backend, folder, state).await?;
    fetch_new_uids(backend, folder, state).await?;
    reconcile_expunges(backend, folder, state).await?;
    Ok(())
}

/// `UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE <modseq>)`.
async fn apply_flag_deltas(
    backend: &mut ImapBackend,
    folder: &FolderId,
    state: &mut FolderSync,
) -> Result<(), MailError> {
    let Some(mod_seq) = state.highest_mod_seq.and_then(std::num::NonZeroU64::new) else {
        return Ok(());
    };
    let range = uid_range_from(1)?;
    let fetched = backend
        .session
        .run_selected(folder.as_str(), || {
            ImapMessageFetch::new(
                range.clone(),
                flags_fetch_items(),
                ImapMessageFetchOptions {
                    uid: true,
                    modifiers: vec![FetchModifier::ChangedSince(mod_seq)],
                },
            )
        })
        .await?;
    for entry in fetched.values() {
        if let Some(uid) = uid_of(entry.as_ref())
            && let Some(envelope) = state.envelopes.get_mut(&uid)
        {
            envelope.flags = flags_of(entry.as_ref());
        }
    }
    Ok(())
}

async fn fetch_new_uids(
    backend: &mut ImapBackend,
    folder: &FolderId,
    state: &mut FolderSync,
) -> Result<(), MailError> {
    let from = state.max_uid() + 1;
    let range = uid_range_from(from)?;
    let items = envelope_fetch_items()?;
    let fetched = backend
        .session
        .run_selected(folder.as_str(), || {
            ImapMessageFetch::new(
                range.clone(),
                items.clone(),
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )
        })
        .await?;
    for entry in fetched.values() {
        // `<max>:*` always matches the last message, so re-fetches of
        // an already known UID are expected and harmless.
        if let Some((uid, envelope)) = parse_envelope_items(entry.as_ref())
            && uid >= from
        {
            state.envelopes.insert(uid, envelope);
        }
    }
    Ok(())
}

/// `UID SEARCH ALL` names every live UID; anything we know that the
/// server no longer lists was expunged.
async fn reconcile_expunges(
    backend: &mut ImapBackend,
    folder: &FolderId,
    state: &mut FolderSync,
) -> Result<(), MailError> {
    let live = backend
        .session
        .run_selected(folder.as_str(), || {
            ImapMessageSearch::new(
                Vec1::from(SearchKey::All),
                ImapMessageSearchOptions { uid: true },
            )
        })
        .await?;
    let live: Vec<u32> = live.into_iter().map(|uid| uid.get()).collect();
    state.retain_uids(&live);
    Ok(())
}

async fn stream_full(
    state: &FolderSync,
    batches: &flume::Sender<Vec<EnvelopeSummary>>,
) -> Result<(), MailError> {
    let envelopes: Vec<EnvelopeSummary> = state.envelopes.values().cloned().collect();
    for chunk in envelopes.chunks(super::envelopes::FETCH_WINDOW) {
        if batches.send_async(chunk.to_vec()).await.is_err() {
            return Err(MailError::Cancelled);
        }
    }
    Ok(())
}
