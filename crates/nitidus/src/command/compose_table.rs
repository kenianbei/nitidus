//! Compose and send commands — 1c grows here (send pipeline, replies,
//! drafts) without crowding the read-path table.

use super::{CommandSpec, no_args};
use crate::action::{Action, ComposeOp};

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
        name: "postpone",
        summary: "save the message as a draft",
        aliases: &[],
        parse: |args| no_args("postpone", args, Action::ComposeAction(ComposeOp::Postpone)),
    },
    CommandSpec {
        name: "discard",
        summary: "discard the staged message",
        aliases: &[],
        parse: |args| no_args("discard", args, Action::ComposeAction(ComposeOp::Discard)),
    },
];
