//! The command-string vocabulary: every named command, its parser, and
//! fuzzy completion. Keybindings, the command line, and future macros
//! all share this one table.

use anyhow::{Context, bail};
use nitidus_mail::Flags;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher};

use crate::action::{Action, FlagOp, FoldOp, Motion, PagerOp, SidebarOp};
use crate::index::SortMode;

struct CommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    parse: fn(&str) -> anyhow::Result<Action>,
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "quit",
        aliases: &["q"],
        parse: |args| no_args("quit", args, Action::Quit),
    },
    CommandSpec {
        name: "command-line",
        aliases: &[],
        parse: |args| Ok(Action::OpenCommandLine(args.to_owned())),
    },
    CommandSpec {
        name: "tab-next",
        aliases: &[],
        parse: |args| no_args("tab-next", args, Action::TabNext),
    },
    CommandSpec {
        name: "tab-prev",
        aliases: &[],
        parse: |args| no_args("tab-prev", args, Action::TabPrev),
    },
    CommandSpec {
        name: "echo",
        aliases: &[],
        parse: |args| Ok(Action::Echo(args.to_owned())),
    },
    CommandSpec {
        name: "next",
        aliases: &[],
        parse: |args| no_args("next", args, Action::Cursor(Motion::Next)),
    },
    CommandSpec {
        name: "prev",
        aliases: &[],
        parse: |args| no_args("prev", args, Action::Cursor(Motion::Prev)),
    },
    CommandSpec {
        name: "next-page",
        aliases: &[],
        parse: |args| no_args("next-page", args, Action::Cursor(Motion::NextPage)),
    },
    CommandSpec {
        name: "prev-page",
        aliases: &[],
        parse: |args| no_args("prev-page", args, Action::Cursor(Motion::PrevPage)),
    },
    CommandSpec {
        name: "first",
        aliases: &[],
        parse: |args| no_args("first", args, Action::Cursor(Motion::First)),
    },
    CommandSpec {
        name: "last",
        aliases: &[],
        parse: |args| no_args("last", args, Action::Cursor(Motion::Last)),
    },
    CommandSpec {
        name: "sort",
        aliases: &[],
        parse: |args| Ok(Action::Sort(SortMode::parse(args)?)),
    },
    CommandSpec {
        name: "read",
        aliases: &[],
        parse: |args| no_args("read", args, flag_action(Flags::SEEN, FlagOp::Set)),
    },
    CommandSpec {
        name: "unread",
        aliases: &[],
        parse: |args| no_args("unread", args, flag_action(Flags::SEEN, FlagOp::Clear)),
    },
    CommandSpec {
        name: "flag",
        aliases: &[],
        parse: |args| no_args("flag", args, flag_action(Flags::FLAGGED, FlagOp::Set)),
    },
    CommandSpec {
        name: "unflag",
        aliases: &[],
        parse: |args| no_args("unflag", args, flag_action(Flags::FLAGGED, FlagOp::Clear)),
    },
    CommandSpec {
        name: "toggle-read",
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
        aliases: &[],
        parse: |args| no_args("threads", args, Action::ToggleThreads),
    },
    CommandSpec {
        name: "fold",
        aliases: &[],
        parse: |args| no_args("fold", args, Action::Fold(FoldOp::Toggle)),
    },
    CommandSpec {
        name: "fold-all",
        aliases: &[],
        parse: |args| no_args("fold-all", args, Action::Fold(FoldOp::CollapseAll)),
    },
    CommandSpec {
        name: "unfold-all",
        aliases: &[],
        parse: |args| no_args("unfold-all", args, Action::Fold(FoldOp::ExpandAll)),
    },
    CommandSpec {
        name: "parent",
        aliases: &[],
        parse: |args| no_args("parent", args, Action::Cursor(Motion::Parent)),
    },
    CommandSpec {
        name: "confirm",
        aliases: &[],
        parse: |args| no_args("confirm", args, Action::OverlayConfirm),
    },
    CommandSpec {
        name: "cancel",
        aliases: &[],
        parse: |args| no_args("cancel", args, Action::OverlayCancel),
    },
    CommandSpec {
        name: "view",
        aliases: &[],
        parse: |args| no_args("view", args, Action::View),
    },
    CommandSpec {
        name: "close",
        aliases: &[],
        parse: |args| no_args("close", args, Action::Pager(PagerOp::Close)),
    },
    CommandSpec {
        name: "next-message",
        aliases: &[],
        parse: |args| no_args("next-message", args, Action::Pager(PagerOp::NextMessage)),
    },
    CommandSpec {
        name: "prev-message",
        aliases: &[],
        parse: |args| no_args("prev-message", args, Action::Pager(PagerOp::PrevMessage)),
    },
    CommandSpec {
        name: "headers",
        aliases: &[],
        parse: |args| no_args("headers", args, Action::Pager(PagerOp::ToggleHeaders)),
    },
    CommandSpec {
        name: "skip-quoted",
        aliases: &[],
        parse: |args| no_args("skip-quoted", args, Action::Pager(PagerOp::SkipQuoted)),
    },
    CommandSpec {
        name: "next-part",
        aliases: &[],
        parse: |args| no_args("next-part", args, Action::Pager(PagerOp::NextPart)),
    },
    CommandSpec {
        name: "prev-part",
        aliases: &[],
        parse: |args| no_args("prev-part", args, Action::Pager(PagerOp::PrevPart)),
    },
    CommandSpec {
        name: "save-part",
        aliases: &[],
        parse: |args| no_args("save-part", args, Action::Pager(PagerOp::SavePart)),
    },
    CommandSpec {
        name: "open-part",
        aliases: &[],
        parse: |args| no_args("open-part", args, Action::Pager(PagerOp::OpenPart)),
    },
    CommandSpec {
        name: "links",
        aliases: &[],
        parse: |args| no_args("links", args, Action::Pager(PagerOp::Links)),
    },
    CommandSpec {
        name: "sidebar",
        aliases: &[],
        parse: |args| no_args("sidebar", args, Action::Sidebar(SidebarOp::ToggleVisible)),
    },
    CommandSpec {
        name: "sidebar-focus",
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
        aliases: &[],
        parse: |args| named_arg("folder-create", args, Action::FolderCreate(args.to_owned())),
    },
    CommandSpec {
        name: "folder-rename",
        aliases: &[],
        parse: |args| named_arg("folder-rename", args, Action::FolderRename(args.to_owned())),
    },
    CommandSpec {
        name: "folder-delete",
        aliases: &[],
        parse: |args| no_args("folder-delete", args, Action::FolderDelete),
    },
];

fn flag_action(flag: Flags, op: FlagOp) -> Action {
    Action::Flag { flag, op }
}

fn no_args(name: &str, args: &str, action: Action) -> anyhow::Result<Action> {
    if args.is_empty() {
        Ok(action)
    } else {
        bail!("{name} takes no arguments")
    }
}

fn named_arg(name: &str, args: &str, action: Action) -> anyhow::Result<Action> {
    if args.is_empty() {
        bail!("{name} needs a folder name")
    } else {
        Ok(action)
    }
}

pub fn parse_command(input: &str) -> anyhow::Result<Action> {
    let stripped = input.trim();
    let stripped = stripped.strip_prefix(':').unwrap_or(stripped).trim();
    if stripped.is_empty() {
        bail!("empty command");
    }
    let (name, args) = match stripped.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (stripped, ""),
    };
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
        .ok_or_else(|| anyhow::anyhow!("unknown command: {name:?}"))?;
    (spec.parse)(args).with_context(|| format!("in command {name:?}"))
}

/// Fuzzy completion over command names, best match first.
pub fn complete_command(input: &str) -> Vec<String> {
    let names = COMMANDS.iter().map(|spec| spec.name);
    if input.is_empty() {
        return names.map(str::to_owned).collect();
    }
    let mut matcher = Matcher::new(MatcherConfig::DEFAULT);
    Pattern::parse(input, CaseMatching::Ignore, Normalization::Smart)
        .match_list(names, &mut matcher)
        .into_iter()
        .map(|(name, _)| name.to_owned())
        .collect()
}
