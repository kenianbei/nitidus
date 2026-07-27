//! Body editing commands. Every operation on a body field that is not
//! plain typing goes through this table, so it is rebindable and shows
//! up in the help overlay like everything else.

use super::{CommandSpec, no_args};
use crate::action::{Action, EditorMotion, EditorOp};

pub(super) const EDITOR_COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "editor-preview",
        summary: "preview the attachment on this line",
        aliases: &[],
        parse: |args| no_args("editor-preview", args, Action::Editor(EditorOp::Preview)),
    },
    CommandSpec {
        name: "editor-newline",
        summary: "break the line",
        aliases: &[],
        parse: |args| no_args("editor-newline", args, Action::Editor(EditorOp::Newline)),
    },
    CommandSpec {
        name: "editor-left",
        summary: "move left",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-left",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Left)),
            )
        },
    },
    CommandSpec {
        name: "editor-right",
        summary: "move right",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-right",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Right)),
            )
        },
    },
    CommandSpec {
        name: "editor-up",
        summary: "move up",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-up",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Up)),
            )
        },
    },
    CommandSpec {
        name: "editor-down",
        summary: "move down",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-down",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Down)),
            )
        },
    },
    CommandSpec {
        name: "editor-word-forward",
        summary: "move a word forward",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-word-forward",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::WordForward)),
            )
        },
    },
    CommandSpec {
        name: "editor-word-back",
        summary: "move a word back",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-word-back",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::WordBack)),
            )
        },
    },
    CommandSpec {
        name: "editor-line-start",
        summary: "move to the start of the line",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-line-start",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::LineStart)),
            )
        },
    },
    CommandSpec {
        name: "editor-line-end",
        summary: "move to the end of the line",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-line-end",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::LineEnd)),
            )
        },
    },
    CommandSpec {
        name: "editor-paragraph-forward",
        summary: "move a paragraph forward",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-paragraph-forward",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::ParagraphForward)),
            )
        },
    },
    CommandSpec {
        name: "editor-paragraph-back",
        summary: "move a paragraph back",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-paragraph-back",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::ParagraphBack)),
            )
        },
    },
    CommandSpec {
        name: "editor-page-up",
        summary: "scroll up a page",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-page-up",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::PageUp)),
            )
        },
    },
    CommandSpec {
        name: "editor-page-down",
        summary: "scroll down a page",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-page-down",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::PageDown)),
            )
        },
    },
    CommandSpec {
        name: "editor-top",
        summary: "move to the top of the body",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-top",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Top)),
            )
        },
    },
    CommandSpec {
        name: "editor-bottom",
        summary: "move to the end of the body",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-bottom",
                args,
                Action::Editor(EditorOp::Move(EditorMotion::Bottom)),
            )
        },
    },
    CommandSpec {
        name: "editor-undo",
        summary: "undo the last edit",
        aliases: &[],
        parse: |args| no_args("editor-undo", args, Action::Editor(EditorOp::Undo)),
    },
    CommandSpec {
        name: "editor-redo",
        summary: "redo the last edit",
        aliases: &[],
        parse: |args| no_args("editor-redo", args, Action::Editor(EditorOp::Redo)),
    },
    CommandSpec {
        name: "editor-select",
        summary: "start or cancel a selection",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-select",
                args,
                Action::Editor(EditorOp::SelectToggle),
            )
        },
    },
    CommandSpec {
        name: "editor-select-all",
        summary: "select the whole body",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-select-all",
                args,
                Action::Editor(EditorOp::SelectAll),
            )
        },
    },
    CommandSpec {
        name: "editor-cut",
        summary: "cut the selection",
        aliases: &[],
        parse: |args| no_args("editor-cut", args, Action::Editor(EditorOp::Cut)),
    },
    CommandSpec {
        name: "editor-copy",
        summary: "copy the selection",
        aliases: &[],
        parse: |args| no_args("editor-copy", args, Action::Editor(EditorOp::Copy)),
    },
    CommandSpec {
        name: "editor-paste",
        summary: "paste from the clipboard",
        aliases: &[],
        parse: |args| no_args("editor-paste", args, Action::Editor(EditorOp::Paste)),
    },
    CommandSpec {
        name: "editor-delete-word-back",
        summary: "delete the word before the cursor",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-delete-word-back",
                args,
                Action::Editor(EditorOp::DeleteWordBack),
            )
        },
    },
    CommandSpec {
        name: "editor-delete-word-forward",
        summary: "delete the word after the cursor",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-delete-word-forward",
                args,
                Action::Editor(EditorOp::DeleteWordForward),
            )
        },
    },
    CommandSpec {
        name: "editor-delete-line-end",
        summary: "delete to the end of the line",
        aliases: &[],
        parse: |args| {
            no_args(
                "editor-delete-line-end",
                args,
                Action::Editor(EditorOp::DeleteLineEnd),
            )
        },
    },
];
