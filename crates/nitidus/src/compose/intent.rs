//! The index-reply intent: fetch the selection with a remembered
//! kind, park the arriving raw message, and consume it into a compose
//! session on the next frame.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, FolderId, JobId};

use super::reply::{ReplyKind, start_from_raw};
use crate::status::StatusMessage;

/// Fetch-then-reply from the index: the intent parks until the raw
/// message arrives through the engine drain.
#[derive(Resource, Default)]
pub struct ReplyIntent(pub(crate) Option<PendingReply>);

pub(crate) struct PendingReply {
    pub kind: ReplyKind,
    pub job: JobId,
    pub source: (AccountId, FolderId, EnvelopeId),
    pub raw: Option<Vec<u8>>,
}

impl ReplyIntent {
    /// Claims an arriving message for a pending reply; true when the
    /// pager should not receive it.
    pub fn claim(&mut self, job: JobId, raw: &[u8]) -> bool {
        match self.0.as_mut() {
            Some(pending) if pending.job == job => {
                pending.raw = Some(raw.to_vec());
                true
            }
            _ => false,
        }
    }

    pub fn abandon(&mut self, job: JobId) {
        if self.0.as_ref().is_some_and(|pending| pending.job == job) {
            self.0 = None;
        }
    }
}

/// Consumes a fulfilled intent into a compose session.
pub(crate) fn consume_reply_intent(world: &mut World) {
    let ready = {
        let mut intent = world.resource_mut::<ReplyIntent>();
        match intent.0.as_ref() {
            Some(pending) if pending.raw.is_some() => intent.0.take(),
            _ => None,
        }
    };
    if let Some(pending) = ready
        && let Some(raw) = pending.raw
    {
        start_from_raw(world, pending.kind, pending.source, &raw);
    }
}

/// No open message: fetch the index selection with a remembered kind.
pub(super) fn fetch_selected_for_reply(world: &mut World, kind: ReplyKind) {
    let index_view = world.resource::<crate::index::IndexView>();
    let (Some(account), Some(id)) = (index_view.account.clone(), index_view.selected.clone())
    else {
        return;
    };
    let folder = index_view.folder.clone();
    let Some(engine) = world.get_resource::<crate::engine::EngineResource>() else {
        return;
    };
    let job = engine.0.next_job();
    let command = nitidus_mail::MailCommand::FetchMessage {
        folder: folder.clone(),
        id: id.clone(),
        job,
    };
    if let Err(error) = engine.0.send(&account, command) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("fetch for reply failed: {error}"), now);
        return;
    }
    world.resource_mut::<ReplyIntent>().0 = Some(PendingReply {
        kind,
        job,
        source: (account, folder, id),
        raw: None,
    });
}
