//! Reply/reply-all/forward seeding: pure functions turning the raw
//! original message plus the account identity into pre-filled session
//! fields — addressing, subject prefixes, threading headers, and the
//! quoted or forwarded body block.

use bevy::prelude::*;
use mail_parser::{Message as ParsedMessage, MessageParser};
use nitidus_mail::{AccountId, EnvelopeId, FolderId};

use super::{ComposeSession, ComposeState, ReplySource, ops};
use crate::config::account::AccountConfig;
const FORWARD_FIELD: &str = "to";
use crate::status::MessageLog;

const QUOTE_PREFIX: &str = "> ";
const FORWARD_MARKER: &str = "---------- Forwarded message ----------";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyKind {
    Reply,
    ReplyAll,
    Forward,
}

/// Everything a reply pre-fills; pure output of `seed`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplySeed {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

pub fn seed(kind: ReplyKind, raw: &[u8], account: &AccountConfig) -> ReplySeed {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ReplySeed::default();
    };
    match kind {
        ReplyKind::Reply => reply_seed(&message, false, account),
        ReplyKind::ReplyAll => reply_seed(&message, true, account),
        ReplyKind::Forward => forward_seed(&message),
    }
}

fn reply_seed(message: &ParsedMessage, all: bool, account: &AccountConfig) -> ReplySeed {
    let target = address_line(message.reply_to())
        .unwrap_or_else(|| address_line(message.from()).unwrap_or_default());
    let (to, cc) = if all {
        let mut to_list = vec![target.clone()];
        to_list.extend(addresses_of(message.to()));
        (
            join_without_self(to_list, account),
            join_without_self(addresses_of(message.cc()), account),
        )
    } else {
        (target, String::new())
    };
    let message_id = message.message_id().unwrap_or_default().to_owned();
    let mut references = reference_ids(message);
    if !message_id.is_empty() {
        references.push(message_id.clone());
    }
    ReplySeed {
        to,
        cc,
        subject: prefixed_subject("Re:", &["re:"], message.subject().unwrap_or_default()),
        body: quoted_body(message),
        in_reply_to: (!message_id.is_empty()).then_some(message_id),
        references,
    }
}

fn forward_seed(message: &ParsedMessage) -> ReplySeed {
    let mut block = format!("\n{FORWARD_MARKER}\n");
    for (name, value) in [
        ("From", address_line(message.from())),
        ("Date", message.date().map(|date| date.to_rfc822())),
        ("Subject", message.subject().map(str::to_owned)),
        ("To", address_line(message.to())),
    ] {
        if let Some(value) = value {
            block.push_str(&format!("{name}: {value}\n"));
        }
    }
    block.push('\n');
    block.push_str(&body_text(message));
    ReplySeed {
        subject: prefixed_subject(
            "Fwd:",
            &["fwd:", "fw:"],
            message.subject().unwrap_or_default(),
        ),
        body: block,
        ..ReplySeed::default()
    }
}

/// `On <date>, <who> wrote:` plus the `> `-quoted text part.
fn quoted_body(message: &ParsedMessage) -> String {
    let who = address_line(message.from()).unwrap_or_else(|| "someone".to_owned());
    let date = message
        .date()
        .map(|date| date.to_rfc822())
        .unwrap_or_else(|| "an earlier date".to_owned());
    let mut quoted = format!("On {date}, {who} wrote:\n");
    for line in body_text(message).lines() {
        quoted.push_str(QUOTE_PREFIX);
        quoted.push_str(line);
        quoted.push('\n');
    }
    quoted
}

fn body_text(message: &ParsedMessage) -> String {
    message
        .body_text(0)
        .map(|text| text.into_owned())
        .unwrap_or_default()
}

fn addresses_of(header: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    header
        .map(|address| {
            address
                .iter()
                .filter_map(|addr| {
                    let email = addr.address()?;
                    Some(match addr.name() {
                        Some(name) => format!("{name} <{email}>"),
                        None => email.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn address_line(header: Option<&mail_parser::Address<'_>>) -> Option<String> {
    let list = addresses_of(header);
    (!list.is_empty()).then(|| list.join(", "))
}

/// Drops entries whose bare address matches the account email or an
/// alias (case-insensitive), then joins.
fn join_without_self(entries: Vec<String>, account: &AccountConfig) -> String {
    let is_self = |entry: &str| {
        let bare = entry
            .rsplit_once('<')
            .and_then(|(_, rest)| rest.strip_suffix('>'))
            .unwrap_or(entry)
            .trim()
            .to_ascii_lowercase();
        bare == account.email.to_ascii_lowercase()
            || account
                .aliases
                .iter()
                .any(|alias| alias.to_ascii_lowercase() == bare)
    };
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|entry| !is_self(entry))
        .filter(|entry| seen.insert(entry.to_ascii_lowercase()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn prefixed_subject(prefix: &str, existing: &[&str], subject: &str) -> String {
    let trimmed = subject.trim();
    let lower = trimmed.to_ascii_lowercase();
    if existing
        .iter()
        .any(|candidate| lower.starts_with(candidate))
    {
        trimmed.to_owned()
    } else {
        format!("{prefix} {trimmed}")
    }
}

fn reference_ids(message: &ParsedMessage) -> Vec<String> {
    let references: Vec<String> = message
        .references()
        .as_text_list()
        .unwrap_or_default()
        .iter()
        .map(|id| id.as_ref().to_owned())
        .collect();
    if !references.is_empty() {
        return references;
    }
    message
        .in_reply_to()
        .as_text_list()
        .unwrap_or_default()
        .iter()
        .map(|id| id.as_ref().to_owned())
        .collect()
}

/// Builds and installs the session from raw message bytes; replies go
/// straight to the editor, forward asks for To first.
pub fn start_from_raw(
    world: &mut World,
    kind: ReplyKind,
    source: (AccountId, FolderId, EnvelopeId),
    raw: &[u8],
) {
    if world.resource::<ComposeState>().is_active() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world.resource_mut::<MessageLog>().warn(
            "a message is already being composed (m resumes it)".to_owned(),
            now,
        );
        return;
    }
    let Some(account_config) = super::composing_account(world) else {
        return;
    };
    let seed = seed(kind, raw, &account_config);
    let directory = match super::compose_directory(world) {
        Ok(directory) => directory,
        Err(error) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<MessageLog>()
                .warn(format!("reply: {error:#}"), now);
            return;
        }
    };
    let session = ComposeSession::create(&account_config, &directory, &seed.body);
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<MessageLog>()
                .warn(format!("reply: {error:#}"), now);
            return;
        }
    };
    session.to = seed.to;
    session.cc = seed.cc;
    session.subject = seed.subject;
    session.in_reply_to = seed.in_reply_to;
    session.references = seed.references;
    if kind != ReplyKind::Forward {
        session.reply_source = Some(ReplySource {
            account: source.0,
            folder: source.1,
            id: source.2,
        });
    }
    world.resource_mut::<ComposeState>().0 = Some(session);
    if kind == ReplyKind::Forward {
        let field = ops::address_field(world, FORWARD_FIELD, "To");
        crate::overlay::form::open_form(
            world,
            crate::overlay::form::FormSpec::new(
                "Forward to",
                "Write",
                vec![field],
                Box::new(|world, values| {
                    if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                        session.to = values.get(FORWARD_FIELD).to_owned();
                    }
                    ops::edit_body(world);
                }),
            ),
        );
    } else {
        ops::edit_body(world);
    }
}

/// Entry from the pager: the open message is the source.
pub fn start_reply(world: &mut World, kind: ReplyKind) {
    let source = {
        let pager = world.resource::<crate::pager::PagerState>();
        pager.open_message().map(|open| {
            (
                (open.account.clone(), open.folder.clone(), open.id.clone()),
                open.raw.clone(),
            )
        })
    };
    match source {
        Some((source, raw)) => start_from_raw(world, kind, source, &raw),
        None => super::intent::fetch_selected_for_reply(world, kind),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const RAW: &[u8] = b"From: Alice <alice@x.com>\r\n\
To: Norman <norman@example.com>, Bob <bob@x.com>\r\n\
Cc: Carol <carol@x.com>, norman.alias@example.com\r\n\
Reply-To: alice-lists@x.com\r\n\
Subject: Re: project plan\r\n\
Date: Mon, 8 Apr 2024 20:52:42 -0700\r\n\
Message-ID: <orig-2@x.com>\r\n\
References: <orig-1@x.com>\r\n\r\n\
first line\r\nsecond line\r\n";

    fn account() -> AccountConfig {
        AccountConfig {
            email: "norman@example.com".to_owned(),
            aliases: vec!["Norman.Alias@example.com".to_owned()],
            ..Default::default()
        }
    }

    #[test]
    fn reply_targets_reply_to_and_threads() {
        let seed = seed(ReplyKind::Reply, RAW, &account());
        assert_eq!(seed.to, "alice-lists@x.com");
        assert_eq!(seed.cc, "");
        assert_eq!(seed.subject, "Re: project plan", "no double Re:");
        assert_eq!(seed.in_reply_to.as_deref(), Some("orig-2@x.com"));
        assert_eq!(seed.references, vec!["orig-1@x.com", "orig-2@x.com"]);
        assert!(seed.body.starts_with("On "), "{}", seed.body);
        assert!(
            seed.body.contains("> first line\n> second line"),
            "{}",
            seed.body
        );
    }

    #[test]
    fn reply_all_merges_and_drops_self_and_aliases() {
        let seed = seed(ReplyKind::ReplyAll, RAW, &account());
        assert_eq!(seed.to, "alice-lists@x.com, Bob <bob@x.com>");
        assert_eq!(seed.cc, "Carol <carol@x.com>");
    }

    #[test]
    fn forward_prefixes_and_inlines_the_original() {
        let seed = seed(ReplyKind::Forward, RAW, &account());
        assert_eq!(seed.subject, "Fwd: Re: project plan");
        assert!(seed.to.is_empty());
        assert_eq!(seed.in_reply_to, None, "forwards do not thread");
        assert!(seed.body.contains(FORWARD_MARKER));
        assert!(
            seed.body.contains("From: Alice <alice@x.com>"),
            "{}",
            seed.body
        );
        assert!(seed.body.contains("first line"), "{}", seed.body);
    }

    #[test]
    fn subject_prefixing_handles_variants() {
        assert_eq!(prefixed_subject("Re:", &["re:"], "hello"), "Re: hello");
        assert_eq!(prefixed_subject("Re:", &["re:"], "RE: hello"), "RE: hello");
        assert_eq!(prefixed_subject("Fwd:", &["fwd:", "fw:"], "FW: x"), "FW: x");
    }
}
