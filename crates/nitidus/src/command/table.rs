//! The command table: every named command with its summary, aliases,
//! and parser. Purely declarative — behavior lives in `Action`.

use nitidus_mail::Flags;

use super::{CommandSpec, flag_action, named_arg, no_args};
use crate::action::{Action, FlagOp, FoldOp, Motion, PagerOp, SidebarOp};
use crate::index::SortMode;

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "quit",
        summary: "exit nitidus",
        aliases: &["q"],
        parse: |args| no_args("quit", args, Action::Quit),
    },
    CommandSpec {
        name: "command-line",
        summary: "open the command line",
        aliases: &[],
        parse: |args| Ok(Action::OpenCommandLine(args.to_owned())),
    },
    CommandSpec {
        name: "tab-next",
        summary: "switch to the next tab",
        aliases: &[],
        parse: |args| no_args("tab-next", args, Action::TabNext),
    },
    CommandSpec {
        name: "tab-prev",
        summary: "switch to the previous tab",
        aliases: &[],
        parse: |args| no_args("tab-prev", args, Action::TabPrev),
    },
    CommandSpec {
        name: "echo",
        summary: "show a message in the statusline",
        aliases: &[],
        parse: |args| Ok(Action::Echo(args.to_owned())),
    },
    CommandSpec {
        name: "next",
        summary: "move down / scroll forward",
        aliases: &[],
        parse: |args| no_args("next", args, Action::Cursor(Motion::Next)),
    },
    CommandSpec {
        name: "prev",
        summary: "move up / scroll back",
        aliases: &[],
        parse: |args| no_args("prev", args, Action::Cursor(Motion::Prev)),
    },
    CommandSpec {
        name: "next-page",
        summary: "forward one page",
        aliases: &[],
        parse: |args| no_args("next-page", args, Action::Cursor(Motion::NextPage)),
    },
    CommandSpec {
        name: "prev-page",
        summary: "back one page",
        aliases: &[],
        parse: |args| no_args("prev-page", args, Action::Cursor(Motion::PrevPage)),
    },
    CommandSpec {
        name: "first",
        summary: "jump to the top",
        aliases: &[],
        parse: |args| no_args("first", args, Action::Cursor(Motion::First)),
    },
    CommandSpec {
        name: "last",
        summary: "jump to the bottom",
        aliases: &[],
        parse: |args| no_args("last", args, Action::Cursor(Motion::Last)),
    },
    CommandSpec {
        name: "sort",
        summary: "set the index sort: :sort <key> [-r]",
        aliases: &[],
        parse: |args| Ok(Action::Sort(SortMode::parse(args)?)),
    },
    CommandSpec {
        name: "read",
        summary: "mark the selection read",
        aliases: &[],
        parse: |args| no_args("read", args, flag_action(Flags::SEEN, FlagOp::Set)),
    },
    CommandSpec {
        name: "unread",
        summary: "mark the selection unread",
        aliases: &[],
        parse: |args| no_args("unread", args, flag_action(Flags::SEEN, FlagOp::Clear)),
    },
    CommandSpec {
        name: "flag",
        summary: "flag the selection",
        aliases: &[],
        parse: |args| no_args("flag", args, flag_action(Flags::FLAGGED, FlagOp::Set)),
    },
    CommandSpec {
        name: "unflag",
        summary: "unflag the selection",
        aliases: &[],
        parse: |args| no_args("unflag", args, flag_action(Flags::FLAGGED, FlagOp::Clear)),
    },
    CommandSpec {
        name: "toggle-read",
        summary: "toggle read state",
        aliases: &[],
        parse: |args| {
            no_args(
                "toggle-read",
                args,
                flag_action(Flags::SEEN, FlagOp::Toggle),
            )
        },
    },
    CommandSpec {
        name: "toggle-flag",
        summary: "toggle the flag",
        aliases: &[],
        parse: |args| {
            no_args(
                "toggle-flag",
                args,
                flag_action(Flags::FLAGGED, FlagOp::Toggle),
            )
        },
    },
    CommandSpec {
        name: "threads",
        summary: "toggle threaded view",
        aliases: &[],
        parse: |args| no_args("threads", args, Action::ToggleThreads),
    },
    CommandSpec {
        name: "fold",
        summary: "collapse or expand the selection",
        aliases: &[],
        parse: |args| no_args("fold", args, Action::Fold(FoldOp::Toggle)),
    },
    CommandSpec {
        name: "fold-all",
        summary: "collapse everything",
        aliases: &[],
        parse: |args| no_args("fold-all", args, Action::Fold(FoldOp::CollapseAll)),
    },
    CommandSpec {
        name: "unfold-all",
        summary: "expand everything",
        aliases: &[],
        parse: |args| no_args("unfold-all", args, Action::Fold(FoldOp::ExpandAll)),
    },
    CommandSpec {
        name: "parent",
        summary: "jump to the parent",
        aliases: &[],
        parse: |args| no_args("parent", args, Action::Cursor(Motion::Parent)),
    },
    CommandSpec {
        name: "confirm",
        summary: "confirm the picker selection",
        aliases: &[],
        parse: |args| no_args("confirm", args, Action::OverlayConfirm),
    },
    CommandSpec {
        name: "cancel",
        summary: "close the picker",
        aliases: &[],
        parse: |args| no_args("cancel", args, Action::OverlayCancel),
    },
    CommandSpec {
        name: "view",
        summary: "open the selection",
        aliases: &[],
        parse: |args| no_args("view", args, Action::View),
    },
    CommandSpec {
        name: "close",
        summary: "close the pager",
        aliases: &[],
        parse: |args| no_args("close", args, Action::Pager(PagerOp::Close)),
    },
    CommandSpec {
        name: "next-message",
        summary: "open the next message",
        aliases: &[],
        parse: |args| no_args("next-message", args, Action::Pager(PagerOp::NextMessage)),
    },
    CommandSpec {
        name: "prev-message",
        summary: "open the previous message",
        aliases: &[],
        parse: |args| no_args("prev-message", args, Action::Pager(PagerOp::PrevMessage)),
    },
    CommandSpec {
        name: "headers",
        summary: "toggle full headers",
        aliases: &[],
        parse: |args| no_args("headers", args, Action::Pager(PagerOp::ToggleHeaders)),
    },
    CommandSpec {
        name: "skip-quoted",
        summary: "skip past the quoted block",
        aliases: &[],
        parse: |args| no_args("skip-quoted", args, Action::Pager(PagerOp::SkipQuoted)),
    },
    CommandSpec {
        name: "next-part",
        summary: "next message part",
        aliases: &[],
        parse: |args| no_args("next-part", args, Action::Pager(PagerOp::NextPart)),
    },
    CommandSpec {
        name: "prev-part",
        summary: "previous message part",
        aliases: &[],
        parse: |args| no_args("prev-part", args, Action::Pager(PagerOp::PrevPart)),
    },
    CommandSpec {
        name: "save-part",
        summary: "save an attachment",
        aliases: &[],
        parse: |args| no_args("save-part", args, Action::Pager(PagerOp::SavePart)),
    },
    CommandSpec {
        name: "open-part",
        summary: "open an attachment externally",
        aliases: &[],
        parse: |args| no_args("open-part", args, Action::Pager(PagerOp::OpenPart)),
    },
    CommandSpec {
        name: "links",
        summary: "list links in this part",
        aliases: &[],
        parse: |args| no_args("links", args, Action::Pager(PagerOp::Links)),
    },
    CommandSpec {
        name: "sidebar",
        summary: "show or hide the sidebar",
        aliases: &[],
        parse: |args| no_args("sidebar", args, Action::Sidebar(SidebarOp::ToggleVisible)),
    },
    CommandSpec {
        name: "sidebar-focus",
        summary: "focus or leave the sidebar",
        aliases: &[],
        parse: |args| {
            no_args(
                "sidebar-focus",
                args,
                Action::Sidebar(SidebarOp::ToggleFocus),
            )
        },
    },
    CommandSpec {
        name: "folder-create",
        summary: "create a folder: :folder-create <path>",
        aliases: &[],
        parse: |args| named_arg("folder-create", args, Action::FolderCreate(args.to_owned())),
    },
    CommandSpec {
        name: "folder-rename",
        summary: "rename the sidebar-selected folder",
        aliases: &[],
        parse: |args| named_arg("folder-rename", args, Action::FolderRename(args.to_owned())),
    },
    CommandSpec {
        name: "help",
        summary: "show key bindings",
        aliases: &[],
        parse: |args| no_args("help", args, Action::Help),
    },
    CommandSpec {
        name: "help-scope",
        summary: "toggle help between this context and all",
        aliases: &[],
        parse: |args| no_args("help-scope", args, Action::HelpScope),
    },
    CommandSpec {
        name: "set-password",
        summary: "store the active account's password in the OS keyring",
        aliases: &[],
        parse: |args| no_args("set-password", args, Action::SetPassword),
    },
    CommandSpec {
        name: "delete-password",
        summary: "remove the active account's keyring password",
        aliases: &[],
        parse: |args| no_args("delete-password", args, Action::DeletePassword),
    },
    CommandSpec {
        name: "folder-delete",
        summary: "delete the selected empty folder",
        aliases: &[],
        parse: |args| no_args("folder-delete", args, Action::FolderDelete),
    },
];
