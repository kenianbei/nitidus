//! The crash-safe outbox: queued sends as `<stem>.eml` + `<stem>.toml`
//! pairs under the state dir, held through the undo window, submitted
//! to the engine at expiry, and cleaned up on `SendDone`. Undo deletes
//! the pair and restores the full compose session (the compose body
//! file survives until transmission succeeds).

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use nitidus_mail::send::SendEnvelope;
use nitidus_mail::{AccountId, JobId};
use serde::{Deserialize, Serialize};

use crate::compose::{ComposeSession, ComposeStage, ComposeState};
use crate::screen::Screen;
use crate::status::StatusMessage;

mod delivery;

pub use delivery::{after_send, fail_send};

const OUTBOX_DIR_NAME: &str = "outbox";
pub const SEND_DELAY: Duration = Duration::from_secs(10);
/// A failed or unresolvable entry parks this far in the future — no
/// per-frame retry loop, but startup picks it up again.
pub(crate) const RETRY_PARK_MS: u128 = 3_600_000;

pub struct OutboxPlugin;

impl Plugin for OutboxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OutboxState>();
        app.init_resource::<SendDelay>();
        app.add_systems(Startup, delivery::scan_outbox);
        app.add_systems(Update, delivery::tick_outbox);
    }
}

/// The undo window; a resource so tests can shrink it.
#[derive(Resource)]
pub struct SendDelay(pub Duration);

impl Default for SendDelay {
    fn default() -> Self {
        Self(SEND_DELAY)
    }
}

/// Where queued sends live; overridable for tests.
#[derive(Resource)]
pub struct OutboxDir(pub PathBuf);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutboxMeta {
    pub account: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body_path: PathBuf,
    pub envelope_from: String,
    pub recipients: Vec<String>,
    pub send_at_epoch_ms: u128,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    /// `account/folder/id` of the message being answered.
    #[serde(default)]
    pub reply_source: Option<(String, String, String)>,
    #[serde(default = "default_save_sent")]
    pub save_sent: bool,
    #[serde(default)]
    pub sent_folder: String,
}

fn default_save_sent() -> bool {
    true
}

pub struct PendingSend {
    pub stem: String,
    pub eml_path: PathBuf,
    pub meta_path: PathBuf,
    pub meta: OutboxMeta,
    pub submitted: Option<JobId>,
}

#[derive(Resource, Default)]
pub struct OutboxState(pub(crate) Vec<PendingSend>);

impl OutboxState {
    pub fn pending_count(&self) -> usize {
        self.0.len()
    }

    /// Milliseconds until the next unsubmitted entry departs.
    pub fn countdown_ms(&self) -> Option<u128> {
        let now = epoch_ms();
        self.0
            .iter()
            .filter(|entry| entry.submitted.is_none())
            .map(|entry| entry.meta.send_at_epoch_ms.saturating_sub(now))
            .min()
    }

    pub fn is_sending(&self) -> bool {
        self.0.iter().any(|entry| entry.submitted.is_some())
    }
}

pub(crate) fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

pub(crate) fn outbox_directory(world: &World) -> anyhow::Result<PathBuf> {
    match world.get_resource::<OutboxDir>() {
        Some(directory) => Ok(directory.0.clone()),
        None => Ok(crate::dirs::state_dir()?.join(OUTBOX_DIR_NAME)),
    }
}

/// Queues a built message; the compose session dissolves into the
/// outbox entry until sent or undone.
pub fn queue(
    world: &mut World,
    session: &ComposeSession,
    envelope: &SendEnvelope,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let directory = outbox_directory(world)?;
    std::fs::create_dir_all(&directory)?;
    let delay = world.resource::<SendDelay>().0;
    let stem = format!("{}-{}", epoch_ms(), std::process::id());
    let eml_path = directory.join(format!("{stem}.eml"));
    let meta_path = directory.join(format!("{stem}.toml"));
    let (save_sent, sent_folder) = delivery::sent_policy(world, session.account.as_str());
    let meta = OutboxMeta {
        account: session.account.as_str().to_owned(),
        from: session.from.clone(),
        to: session.to.clone(),
        cc: session.cc.clone(),
        bcc: session.bcc.clone(),
        subject: session.subject.clone(),
        body_path: session.body_path.clone(),
        envelope_from: envelope.from.clone(),
        recipients: envelope.recipients.clone(),
        send_at_epoch_ms: epoch_ms() + delay.as_millis(),
        in_reply_to: session.in_reply_to.clone(),
        references: session.references.clone(),
        reply_source: session.reply_source.as_ref().map(|source| {
            (
                source.account.as_str().to_owned(),
                source.folder.as_str().to_owned(),
                source.id.as_str().to_owned(),
            )
        }),
        save_sent,
        sent_folder,
    };
    std::fs::write(&eml_path, bytes)?;
    std::fs::write(&meta_path, toml::to_string(&meta)?)?;
    world.resource_mut::<OutboxState>().0.push(PendingSend {
        stem,
        eml_path,
        meta_path,
        meta,
        submitted: None,
    });
    Ok(())
}

/// `z`: the most recent unsubmitted entry returns to a full compose
/// session; the files (except the body) are removed.
pub fn undo_send(world: &mut World) {
    let entry = {
        let mut outbox = world.resource_mut::<OutboxState>();
        let position = outbox.0.iter().rposition(|entry| entry.submitted.is_none());
        position.map(|position| outbox.0.remove(position))
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let Some(entry) = entry else {
        world
            .resource_mut::<StatusMessage>()
            .info("nothing to undo".to_owned(), now);
        return;
    };
    remove_file_logged(&entry.eml_path);
    remove_file_logged(&entry.meta_path);
    let mut session = ComposeSession {
        account: AccountId::new(&entry.meta.account),
        from: entry.meta.from.clone(),
        to: entry.meta.to.clone(),
        cc: entry.meta.cc.clone(),
        bcc: entry.meta.bcc.clone(),
        subject: entry.meta.subject.clone(),
        body_path: entry.meta.body_path.clone(),
        body: Vec::new(),
        stage: ComposeStage::Review,
        in_reply_to: entry.meta.in_reply_to.clone(),
        references: entry.meta.references.clone(),
        reply_source: entry
            .meta
            .reply_source
            .as_ref()
            .map(|(account, folder, id)| crate::compose::ReplySource {
                account: AccountId::new(account),
                folder: nitidus_mail::FolderId::new(folder),
                id: nitidus_mail::EnvelopeId::new(id),
            }),
    };
    session.reload_body();
    world.resource_mut::<ComposeState>().0 = Some(session);
    *world.resource_mut::<Screen>() = Screen::Compose;
    world
        .resource_mut::<StatusMessage>()
        .info("send undone — back to review".to_owned(), now);
}

/// Removes and returns the entry for a completed job; the caller runs
/// `after_send` (sent copy, answered flag, file cleanup).
pub fn take_completed(outbox: &mut OutboxState, job: JobId) -> Option<PendingSend> {
    let position = outbox
        .0
        .iter()
        .position(|entry| entry.submitted == Some(job))?;
    Some(outbox.0.remove(position))
}

pub(super) fn remove_file_logged(path: &std::path::Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("outbox cleanup {}: {error}", path.display());
    }
}
