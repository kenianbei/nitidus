//! Draft recall: fetch the selected draft from the drafts folder and
//! reconstruct a full compose session — headers (Bcc included), body,
//! and attachment parts materialized back to disk.

use bevy::prelude::*;
use mail_parser::{MessageParser, MimeHeaders};
use nitidus_mail::{AccountId, EnvelopeId, FolderId};

use super::{ComposeSession, ComposeState};
use crate::status::MessageLog;

/// `e` / `:recall` — only in the account's drafts folder.
pub fn recall_selected(world: &mut World) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    if world.resource::<ComposeState>().is_active() {
        world.resource_mut::<MessageLog>().warn(
            "a message is already being composed (m resumes it)".to_owned(),
            now,
        );
        return;
    }
    let index_view = world.resource::<crate::index::IndexView>();
    let (Some(account), folder) = (index_view.account.clone(), index_view.folder.clone()) else {
        return;
    };
    let drafts = super::drafts::drafts_folder(world, account.as_str());
    if folder.as_str() != drafts {
        world
            .resource_mut::<MessageLog>()
            .info(format!("recall works in the drafts folder ({drafts})"), now);
        return;
    }
    super::intent::fetch_selected(world, super::intent::IntentPurpose::Recall);
}

/// Rebuilds a session from fetched draft bytes: headers (Bcc
/// included), body, and materialized attachment files.
pub(crate) fn recall_from_raw(
    world: &mut World,
    source: (AccountId, FolderId, EnvelopeId),
    raw: &[u8],
) {
    let Some(account_config) = super::composing_account(world) else {
        return;
    };
    let directory = match super::compose_directory(world) {
        Ok(directory) => directory,
        Err(error) => return warn(world, format!("recall: {error:#}")),
    };
    let Some(message) = MessageParser::default().parse(raw) else {
        return warn(world, "recall: unparseable draft".to_owned());
    };
    let session = match materialize(&message, &account_config, &directory, &source) {
        Ok(session) => session,
        Err(error) => return warn(world, format!("recall: {error:#}")),
    };
    world.resource_mut::<ComposeState>().0 = Some(session);
    super::form::reopen_restored(world);
    let now = world.resource::<Time>().elapsed_secs_f64();
    world
        .resource_mut::<MessageLog>()
        .info("draft recalled".to_owned(), now);
}

fn materialize(
    message: &mail_parser::Message,
    account_config: &crate::config::account::AccountConfig,
    directory: &std::path::Path,
    source: &(AccountId, FolderId, EnvelopeId),
) -> anyhow::Result<ComposeSession> {
    std::fs::create_dir_all(directory)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let stem = format!("{stamp}-{}", std::process::id());
    let body_path = directory.join(format!("{stem}.md"));
    let body = message
        .body_text(0)
        .map(|text| text.into_owned())
        .unwrap_or_default();

    let mut attachments = Vec::new();
    for (index, part) in message.attachments().enumerate() {
        let name = part
            .attachment_name()
            .unwrap_or("attachment")
            .replace(['/', '\\'], "_");
        let path = directory.join(format!("{stem}-att{index}-{name}"));
        std::fs::write(&path, part.contents())?;
        attachments.push(path);
    }

    // The recalled parts land on the attachment row; the body keeps
    // whatever placement tokens it was written with.
    let body: Vec<String> = body.lines().map(str::to_owned).collect();
    std::fs::write(&body_path, format!("{}\n", body.join("\n")))?;

    let line = |header: Option<&mail_parser::Address<'_>>| {
        super::reply::address_line(header).unwrap_or_default()
    };
    Ok(ComposeSession {
        account: source.0.clone(),
        from: super::from_identity(account_config),
        to: line(message.to()),
        cc: line(message.cc()),
        bcc: line(message.bcc()),
        subject: message.subject().unwrap_or_default().to_owned(),
        body_path,
        body,
        in_reply_to: message
            .in_reply_to()
            .as_text_list()
            .unwrap_or_default()
            .first()
            .map(|id| id.as_ref().to_owned()),
        references: message
            .references()
            .as_text_list()
            .unwrap_or_default()
            .iter()
            .map(|id| id.as_ref().to_owned())
            .collect(),
        reply_source: None,
        attachments,
        draft_source: Some((source.1.clone(), source.2.clone())),
    })
}

fn warn(world: &mut World, text: String) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<MessageLog>().warn(text, now);
}
