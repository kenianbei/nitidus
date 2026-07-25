//! Compose and send commands — 1c grows here (send pipeline, replies,
//! drafts) without crowding the read-path table.

use super::{CommandSpec, no_args};
use crate::action::{Action, ComposeOp};
use crate::compose::ReplyKind;

pub(super) const COMPOSE_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "compose",
        summary: "compose a new message",
        aliases: &["m"],
        parse: |args| no_args("compose", args, Action::Compose),
    },
    CommandSpec {
        name: "compose-edit",
        summary: "edit the body in $EDITOR",
        aliases: &[],
        parse: |args| {
            no_args(
                "compose-edit",
                args,
                Action::ComposeAction(ComposeOp::EditBody),
            )
        },
    },
    CommandSpec {
        name: "compose-to",
        summary: "edit the To header",
        aliases: &[],
        parse: |args| no_args("compose-to", args, Action::ComposeAction(ComposeOp::To)),
    },
    CommandSpec {
        name: "compose-cc",
        summary: "edit the Cc header",
        aliases: &[],
        parse: |args| no_args("compose-cc", args, Action::ComposeAction(ComposeOp::Cc)),
    },
    CommandSpec {
        name: "compose-bcc",
        summary: "edit the Bcc header",
        aliases: &[],
        parse: |args| no_args("compose-bcc", args, Action::ComposeAction(ComposeOp::Bcc)),
    },
    CommandSpec {
        name: "compose-subject",
        summary: "edit the Subject header",
        aliases: &[],
        parse: |args| {
            no_args(
                "compose-subject",
                args,
                Action::ComposeAction(ComposeOp::Subject),
            )
        },
    },
    CommandSpec {
        name: "send",
        summary: "send the staged message",
        aliases: &[],
        parse: |args| no_args("send", args, Action::ComposeAction(ComposeOp::Send)),
    },
    CommandSpec {
        name: "attach",
        summary: "attach a file to the message",
        aliases: &[],
        parse: |args| no_args("attach", args, Action::ComposeAction(ComposeOp::Attach)),
    },
    CommandSpec {
        name: "detach",
        summary: "remove an attachment",
        aliases: &[],
        parse: |args| no_args("detach", args, Action::ComposeAction(ComposeOp::Detach)),
    },
    CommandSpec {
        name: "recall",
        summary: "edit the selected draft",
        aliases: &[],
        parse: |args| no_args("recall", args, Action::Recall),
    },
    CommandSpec {
        name: "recover",
        summary: "restore the newest unfinished draft",
        aliases: &[],
        parse: |args| no_args("recover", args, Action::Recover),
    },
    CommandSpec {
        name: "postpone",
        summary: "save the message as a draft",
        aliases: &[],
        parse: |args| no_args("postpone", args, Action::ComposeAction(ComposeOp::Postpone)),
    },
    CommandSpec {
        name: "reply",
        summary: "reply to the message",
        aliases: &[],
        parse: |args| no_args("reply", args, Action::Reply(ReplyKind::Reply)),
    },
    CommandSpec {
        name: "reply-all",
        summary: "reply to everyone on the message",
        aliases: &[],
        parse: |args| no_args("reply-all", args, Action::Reply(ReplyKind::ReplyAll)),
    },
    CommandSpec {
        name: "forward",
        summary: "forward the message",
        aliases: &[],
        parse: |args| no_args("forward", args, Action::Reply(ReplyKind::Forward)),
    },
    CommandSpec {
        name: "undo-send",
        summary: "pull the queued message back to review",
        aliases: &[],
        parse: |args| no_args("undo-send", args, Action::UndoSend),
    },
    CommandSpec {
        name: "discard",
        summary: "discard the staged message",
        aliases: &[],
        parse: |args| no_args("discard", args, Action::ComposeAction(ComposeOp::Discard)),
    },
];
