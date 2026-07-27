//! Draft operations: attachment add/remove, send-time warnings,
//! postpone to the server drafts folder, and recall back into a live
//! session.

use bevy::prelude::*;
use nitidus_mail::{Flags, FolderId, MailCommand};

use super::{ComposeSession, ComposeState, build, persist};
use crate::engine::EngineResource;
use crate::status::MessageLog;

const ATTACH_WORDS: &[&str] = &["attach", "attached", "attachment", "attachments"];

/// Attaching is a file browse, not a prompt: the composer is itself a
/// form, and a second form would take the first one's place. What comes
/// back joins the attachment row, which is what declares an attachment.
pub(super) fn attach_prompt(world: &mut World) {
    crate::explorer::open_explorer(
        world,
        crate::explorer::ExplorerRequest {
            title: "Attach".to_owned(),
            extensions: &[],
            start_dir: None,
            on_pick: Box::new(|world, path| {
                let name = path.display().to_string();
                if !crate::overlay::form::push_entry(world, super::form::ATTACH_FIELD, name) {
                    notice(world, "already attached");
                }
            }),
        },
    );
}

/// Enter on the attachment row: with nothing on it, the offer to add
/// the first; otherwise a look at what is picked.
pub(super) fn activate_attachment(world: &mut World) {
    match selected_attachment(world) {
        Some(path) => super::preview::open_path(world, &path),
        None => attach_prompt(world),
    }
}

fn selected_attachment(world: &World) -> Option<std::path::PathBuf> {
    crate::overlay::form::selected_entry(world, super::form::ATTACH_FIELD)
        .map(std::path::PathBuf::from)
}

/// `:attach-insert`: puts the picked attachment where the caret is, so
/// the body says where it belongs. The file is attached either way —
/// the token is a placement, not the attachment itself.
pub(super) fn insert_selected(world: &mut World) {
    let Some(path) = selected_attachment(world) else {
        return notice(world, "nothing attached to place");
    };
    let token = super::token::AttachToken::new(path).render();
    if !super::editing::insert_line_into(world, super::form::BODY_FIELD, &token) {
        notice(world, "no body to place it in");
    }
}

/// `:detach`: the picked attachment leaves the row, and any token
/// naming it leaves the body with it.
pub(super) fn detach_selected(world: &mut World) {
    let Some(removed) =
        crate::overlay::form::remove_selected_entry(world, super::form::ATTACH_FIELD)
    else {
        return notice(world, "no attachments to remove");
    };
    let path = std::path::PathBuf::from(&removed);
    super::editing::remove_token_line(world, &path);
    notice(world, format!("detached {removed}"));
}

fn notice(world: &mut World, text: impl Into<String>) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<MessageLog>().info(text.into(), now);
}

/// The send entry point: warning chain, then the actual queue.
pub(super) fn send_with_checks(world: &mut World) {
    super::form::flush_body(world);
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
    super::form::flush_body(world);
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
    super::form::dismiss(world);
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
