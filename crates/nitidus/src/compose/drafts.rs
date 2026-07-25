//! Draft operations: attachment add/remove, send-time warnings,
//! postpone to the server drafts folder, and recall back into a live
//! session.

use bevy::prelude::*;
use nitidus_mail::{Flags, FolderId, MailCommand};

use super::{ComposeSession, ComposeState, build, persist};
use crate::engine::EngineResource;
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::prompt::{PromptRequest, open_prompt};
use crate::screen::Screen;
use crate::status::StatusMessage;

const ATTACH_WORDS: &[&str] = &["attach", "attached", "attachment", "attachments"];

pub(super) fn attach_prompt(world: &mut World) {
    let request = PromptRequest::new(
        "Attach file: ",
        Box::new(|world, value| {
            let path = expand_path(&value);
            let now = world.resource::<Time>().elapsed_secs_f64();
            if !path.is_file() {
                world
                    .resource_mut::<StatusMessage>()
                    .warn(format!("not a file: {}", path.display()), now);
                return;
            }
            if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                session.attachments.push(path);
            }
        }),
    );
    open_prompt(world, request);
}

pub(super) fn detach_picker(world: &mut World) {
    let attachments = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.attachments.clone())
        .unwrap_or_default();
    if attachments.is_empty() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .info("no attachments to remove".to_owned(), now);
        return;
    }
    let items = attachments
        .iter()
        .map(|path| PickerItem {
            label: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .to_owned(),
            detail: Some(path.display().to_string()),
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: "remove attachment".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut()
                    && picked < session.attachments.len()
                {
                    session.attachments.remove(picked);
                }
            }),
        },
    );
}

fn expand_path(input: &str) -> std::path::PathBuf {
    let trimmed = input.trim();
    if let Some(stripped) = trimmed.strip_prefix("~/")
        && let Ok(home) = etcetera::home_dir()
    {
        return home.join(stripped);
    }
    std::path::PathBuf::from(trimmed)
}

/// `y` entry: warning chain, then the actual queue.
pub(super) fn send_with_checks(world: &mut World) {
    let Some((subject_empty, needs_attachment)) =
        world.resource::<ComposeState>().session().map(|session| {
            (
                session.subject.trim().is_empty(),
                session.attachments.is_empty() && mentions_attachment(&session.body),
            )
        })
    else {
        return;
    };
    if subject_empty {
        confirm(world, "Send without a subject? (y/n): ", move |world| {
            if needs_attachment {
                confirm_attachment_then_send(world);
            } else {
                super::ops::queue_send(world);
            }
        });
        return;
    }
    if needs_attachment {
        confirm_attachment_then_send(world);
        return;
    }
    super::ops::queue_send(world);
}

fn confirm_attachment_then_send(world: &mut World) {
    confirm(world, "No attachment — send anyway? (y/n): ", |world| {
        super::ops::queue_send(world);
    });
}

fn confirm(world: &mut World, label: &str, then: impl FnOnce(&mut World) + Send + Sync + 'static) {
    let request = PromptRequest::new(
        label,
        Box::new(move |world, answer| {
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                then(world);
            }
        }),
    );
    open_prompt(world, request);
}

/// Attach-words on unquoted body lines while nothing is attached.
pub(super) fn mentions_attachment(body: &[String]) -> bool {
    body.iter()
        .filter(|line| !line.trim_start().starts_with('>'))
        .any(|line| {
            let lower = line.to_ascii_lowercase();
            ATTACH_WORDS.iter().any(|word| lower.contains(word))
        })
}

/// `P`: draft form to the drafts folder, replace any recalled
/// original, clear the local session.
pub(super) fn postpone(world: &mut World) {
    let built = {
        let compose = world.resource::<ComposeState>();
        let Some(session) = compose.session() else {
            return;
        };
        build::build(session, build::BuildMode::Draft)
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let bytes = match built {
        Ok(built) => built.bytes,
        Err(error) => {
            // Draft headers may be legitimately incomplete; an empty To
            // still deserves saving. Rebuild with a placeholder.
            return postpone_unaddressed(world, error, now);
        }
    };
    finish_postpone(world, bytes, now);
}

/// A draft with no valid recipients still saves — the To column is
/// simply empty when recalled.
fn postpone_unaddressed(world: &mut World, error: anyhow::Error, now: f64) {
    let rebuilt = {
        let compose = world.resource::<ComposeState>();
        let Some(session) = compose.session() else {
            return;
        };
        if !session.to.trim().is_empty() {
            // A present-but-unparseable To is a real error.
            world
                .resource_mut::<StatusMessage>()
                .warn(format!("postpone: {error:#}"), now);
            return;
        }
        let mut placeholder = clone_session(session);
        placeholder.to = "draft@localhost".to_owned();
        build::build(&placeholder, build::BuildMode::Draft)
    };
    match rebuilt {
        Ok(built) => finish_postpone(world, built.bytes, now),
        Err(error) => {
            world
                .resource_mut::<StatusMessage>()
                .warn(format!("postpone: {error:#}"), now);
        }
    }
}

fn finish_postpone(world: &mut World, bytes: Vec<u8>, now: f64) {
    let Some(session) = world.resource_mut::<ComposeState>().0.take() else {
        return;
    };
    let drafts_folder = drafts_folder(world, session.account.as_str());
    let Some(engine) = world.get_resource::<EngineResource>() else {
        world.resource_mut::<ComposeState>().0 = Some(session);
        return;
    };
    let append = MailCommand::AppendMessage {
        folder: FolderId::new(&drafts_folder),
        bytes,
        flags: Flags::DRAFT.with(Flags::SEEN),
    };
    if let Err(error) = engine.0.send(&session.account, append) {
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("postpone: {error}"), now);
        world.resource_mut::<ComposeState>().0 = Some(session);
        return;
    }
    if let Some((folder, id)) = &session.draft_source {
        let delete = MailCommand::DeleteMessage {
            folder: folder.clone(),
            id: id.clone(),
        };
        if let Err(error) = engine.0.send(&session.account, delete) {
            tracing::warn!("stale draft removal: {error}");
        }
    }
    persist::remove_sidecar(&session.body_path);
    if let Err(error) = std::fs::remove_file(&session.body_path) {
        tracing::warn!("postpone body cleanup: {error}");
    }
    *world.resource_mut::<Screen>() = Screen::Index;
    world
        .resource_mut::<StatusMessage>()
        .info(format!("draft saved to {drafts_folder}"), now);
}

fn clone_session(session: &ComposeSession) -> ComposeSession {
    ComposeSession {
        account: session.account.clone(),
        from: session.from.clone(),
        to: session.to.clone(),
        cc: session.cc.clone(),
        bcc: session.bcc.clone(),
        subject: session.subject.clone(),
        body_path: session.body_path.clone(),
        body: session.body.clone(),
        stage: session.stage,
        in_reply_to: session.in_reply_to.clone(),
        references: session.references.clone(),
        reply_source: session.reply_source.clone(),
        attachments: session.attachments.clone(),
        draft_source: session.draft_source.clone(),
    }
}

pub(super) fn drafts_folder(world: &World, account: &str) -> String {
    let config = world.resource::<crate::config::Config>();
    config
        .accounts
        .iter()
        .find(|candidate| candidate.name == account)
        .map(|account_config| account_config.folders.drafts.clone())
        .unwrap_or_else(|| "Drafts".to_owned())
}
