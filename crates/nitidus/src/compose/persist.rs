//! Crash-safe session sidecars: a `<body-stem>.toml` beside every
//! compose body, rewritten whenever the session changes and removed
//! with it. Startup counts orphaned pairs; `:recover` restores the
//! newest into a full session at review.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, FolderId};
use serde::{Deserialize, Serialize};

use super::{ComposeSession, ComposeStage, ComposeState, ReplySource};
use crate::screen::Screen;
use crate::status::StatusMessage;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub account: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub reply_source: Option<(String, String, String)>,
    #[serde(default)]
    pub attachments: Vec<PathBuf>,
    #[serde(default)]
    pub draft_source: Option<(String, String)>,
}

pub(super) fn sidecar_path(body_path: &Path) -> PathBuf {
    body_path.with_extension("toml")
}

impl SessionMeta {
    fn of(session: &ComposeSession) -> Self {
        Self {
            account: session.account.as_str().to_owned(),
            from: session.from.clone(),
            to: session.to.clone(),
            cc: session.cc.clone(),
            bcc: session.bcc.clone(),
            subject: session.subject.clone(),
            in_reply_to: session.in_reply_to.clone(),
            references: session.references.clone(),
            reply_source: session.reply_source.as_ref().map(|source| {
                (
                    source.account.as_str().to_owned(),
                    source.folder.as_str().to_owned(),
                    source.id.as_str().to_owned(),
                )
            }),
            attachments: session.attachments.clone(),
            draft_source: session
                .draft_source
                .as_ref()
                .map(|(folder, id)| (folder.as_str().to_owned(), id.as_str().to_owned())),
        }
    }

    fn into_session(self, body_path: PathBuf) -> ComposeSession {
        let mut session = ComposeSession {
            account: AccountId::new(&self.account),
            from: self.from,
            to: self.to,
            cc: self.cc,
            bcc: self.bcc,
            subject: self.subject,
            body_path,
            body: Vec::new(),
            stage: ComposeStage::Review,
            in_reply_to: self.in_reply_to,
            references: self.references,
            reply_source: self.reply_source.map(|(account, folder, id)| ReplySource {
                account: AccountId::new(account),
                folder: FolderId::new(folder),
                id: EnvelopeId::new(id),
            }),
            attachments: self.attachments,
            draft_source: self
                .draft_source
                .map(|(folder, id)| (FolderId::new(folder), EnvelopeId::new(id))),
        };
        session.reload_body();
        session
    }
}

/// Rewrites the sidecar whenever the session mutates; every field
/// change flows through `ComposeState`, so change detection covers all
/// mutation points at once.
pub(super) fn persist_session(compose: Res<ComposeState>) {
    if !compose.is_changed() {
        return;
    }
    let Some(session) = compose.session() else {
        return;
    };
    let meta = SessionMeta::of(session);
    let path = sidecar_path(&session.body_path);
    let write = toml::to_string(&meta)
        .map_err(anyhow::Error::from)
        .and_then(|serialized| std::fs::write(&path, serialized).map_err(Into::into));
    if let Err(error) = write {
        tracing::warn!("session sidecar {}: {error:#}", path.display());
    }
}

pub(super) fn remove_sidecar(body_path: &Path) {
    let path = sidecar_path(body_path);
    if let Err(error) = std::fs::remove_file(&path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("removing sidecar {}: {error}", path.display());
    }
}

/// Meta paths of orphaned sessions, newest first.
pub(super) fn orphans(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("toml")
                && path.with_extension("md").exists()
        })
        .collect();
    found.sort();
    found.reverse();
    found
}

pub(super) fn notice_orphans(world: &mut World) {
    let Ok(directory) = super::compose_directory(world) else {
        return;
    };
    let count = orphans(&directory).len();
    if count > 0 {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world.resource_mut::<StatusMessage>().info(
            format!("{count} unfinished draft(s) — :recover restores the newest"),
            now,
        );
    }
}

/// `:recover` — restores the newest orphan into a live session.
pub fn recover(world: &mut World) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    if world.resource::<ComposeState>().is_active() {
        world.resource_mut::<StatusMessage>().warn(
            "a message is already being composed (m resumes it)".to_owned(),
            now,
        );
        return;
    }
    let Ok(directory) = super::compose_directory(world) else {
        return;
    };
    let Some(meta_path) = orphans(&directory).into_iter().next() else {
        world
            .resource_mut::<StatusMessage>()
            .info("no unfinished drafts to recover".to_owned(), now);
        return;
    };
    let restored = std::fs::read_to_string(&meta_path)
        .map_err(anyhow::Error::from)
        .and_then(|content| toml::from_str::<SessionMeta>(&content).map_err(Into::into));
    match restored {
        Ok(meta) => {
            let session = meta.into_session(meta_path.with_extension("md"));
            world.resource_mut::<ComposeState>().0 = Some(session);
            *world.resource_mut::<Screen>() = Screen::Compose;
            world
                .resource_mut::<StatusMessage>()
                .info("draft recovered — back to review".to_owned(), now);
        }
        Err(error) => {
            world
                .resource_mut::<StatusMessage>()
                .warn(format!("recover {}: {error:#}", meta_path.display()), now);
        }
    }
}
