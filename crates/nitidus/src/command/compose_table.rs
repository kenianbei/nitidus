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
        name: "compose-edit-external",
        summary: "edit the body in $EDITOR",
        aliases: &[],
        parse: |args| {
            no_args(
                "compose-edit-external",
                args,
                Action::ComposeAction(ComposeOp::EditBodyExternal),
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
        name: "attach-insert",
        summary: "place the attachment at the cursor",
        aliases: &[],
        parse: |args| {
            no_args(
                "attach-insert",
                args,
                Action::ComposeAction(ComposeOp::AttachInsert),
            )
        },
    },
    CommandSpec {
        name: "detach",
        summary: "remove the picked attachment",
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
