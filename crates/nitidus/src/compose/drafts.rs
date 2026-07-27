//! Draft operations: attachment add/remove, send-time warnings,
//! postpone to the server drafts folder, and recall back into a live
//! session.

use bevy::prelude::*;
use nitidus_mail::{Flags, FolderId, MailCommand};

use super::{ComposeSession, ComposeState, build, persist};
use crate::engine::EngineResource;
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::status::MessageLog;

const ATTACH_WORDS: &[&str] = &["attach", "attached", "attachment", "attachments"];

/// Attaching writes a token into the body rather than a side list: the
/// body is what declares an attachment, so the token is the thing the
/// user can see, move, and delete.
const ATTACH_FIELD: &str = "path";

pub(super) fn attach_prompt(world: &mut World) {
    crate::overlay::form::open_form(
        world,
        crate::overlay::form::FormSpec::new(
            "Attach",
            "Attach",
            vec![
                crate::overlay::form::FieldSpec::text(ATTACH_FIELD, "File").validated(|value| {
                    if expand_path(value).is_file() {
                        return Ok(());
                    }
                    Err("no such file".to_owned())
                }),
            ],
            Box::new(|world, values| {
                let path = expand_path(values.get(ATTACH_FIELD));
                insert_token(world, &super::token::AttachToken::new(path).render());
            }),
        ),
    );
}

/// Into the editor at the cursor when one is open, otherwise onto the end
/// of the staged body.
fn insert_token(world: &mut World, token: &str) {
    if super::inline::insert_line(world, token) {
        return;
    }
    if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
        session.body.push(token.to_owned());
        let outcome = session.write_body();
        report_body_write(world, outcome);
    }
}

fn report_body_write(world: &mut World, outcome: std::io::Result<()>) {
    if let Err(error) = outcome {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .warn(format!("could not save the body: {error}"), now);
    }
}

pub(super) fn detach_picker(world: &mut World) {
    let attachments = current_body(world)
        .map(|body| super::token::paths(&body))
        .unwrap_or_default();
    if attachments.is_empty() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
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
                let Some(path) = attachments.get(picked).cloned() else {
                    return;
                };
                remove_token(world, &path);
            }),
        },
    );
}

/// The body as it stands: the live buffer while editing, else the staged
/// session body.
fn current_body(world: &World) -> Option<Vec<String>> {
    world.resource::<super::InlineEditor>().lines().or_else(|| {
        world
            .resource::<ComposeState>()
            .session()
            .map(|s| s.body.clone())
    })
}

fn remove_token(world: &mut World, path: &std::path::Path) {
    if super::inline::remove_token_line(world, path) {
        return;
    }
    if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
        session.body = super::token::remove(&session.body, path);
        let outcome = session.write_body();
        report_body_write(world, outcome);
    }
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
        confirm(
            world,
            "Send without a subject",
            "This message has no subject. Send it anyway?",
            move |world| {
                if needs_attachment {
                    confirm_attachment_then_send(world);
                } else {
                    super::ops::queue_send(world);
                }
            },
        );
        return;
    }
    if needs_attachment {
        confirm_attachment_then_send(world);
        return;
    }
    super::ops::queue_send(world);
}

fn confirm_attachment_then_send(world: &mut World) {
    confirm(
        world,
        "No attachment",
        "The body mentions an attachment but nothing is attached. Send anyway?",
        super::ops::queue_send,
    );
}

fn confirm(
    world: &mut World,
    title: &str,
    question: &str,
    then: impl FnOnce(&mut World) + Send + Sync + 'static,
) {
    crate::overlay::open_confirm(
        world,
        crate::overlay::ConfirmSpec::new(title, question, "Send", Box::new(then)),
    );
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
                .resource_mut::<MessageLog>()
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
                .resource_mut::<MessageLog>()
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
            .resource_mut::<MessageLog>()
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
    world
        .resource_mut::<MessageLog>()
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
