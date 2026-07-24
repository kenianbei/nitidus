//! The command vocabulary: every operation is a named command string
//! that parses to an `Action`. Keybindings, the command line, and future
//! macros all share this one parser.

use anyhow::{Context, bail};
use bevy::app::AppExit;
use bevy::prelude::*;
use nitidus_mail::Flags;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher};

use crate::index::{self, SortMode};
use crate::keymap::{InputMode, Mode};
use crate::shell::Tabs;
use crate::status::StatusMessage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    OpenCommandLine,
    TabNext,
    TabPrev,
    Echo(String),
    Cursor(Motion),
    Sort(SortMode),
    Flag { flag: Flags, op: FlagOp },
    ToggleThreads,
    Fold(FoldOp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Next,
    Prev,
    NextPage,
    PrevPage,
    First,
    Last,
    Parent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagOp {
    Set,
    Clear,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldOp {
    Toggle,
    CollapseAll,
    ExpandAll,
}

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
        parse: |args| no_args("command-line", args, Action::OpenCommandLine),
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
        parse: |args| no_args("toggle-read", args, flag_action(Flags::SEEN, FlagOp::Toggle)),
    },
    CommandSpec {
        name: "toggle-flag",
        aliases: &[],
        parse: |args| no_args("toggle-flag", args, flag_action(Flags::FLAGGED, FlagOp::Toggle)),
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

/// Applies an action immediately. Direct world mutation (rather than a
/// message hop) keeps mode switches synchronous for burst input.
pub fn apply_action(world: &mut World, action: &Action) {
    match action {
        Action::Quit => {
            world.write_message(AppExit::Success);
        }
        Action::OpenCommandLine => world.resource_mut::<Mode>().0 = InputMode::CommandLine,
        Action::TabNext => world.resource_mut::<Tabs>().rotate(1),
        Action::TabPrev => world.resource_mut::<Tabs>().rotate(-1),
        Action::Echo(text) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<StatusMessage>()
                .info(text.clone(), now);
        }
        Action::Cursor(motion) => index::move_cursor(world, *motion),
        Action::Sort(mode) => index::set_sort(world, *mode),
        Action::Flag { flag, op } => index::flag_selected(world, *flag, *op),
        Action::ToggleThreads => index::toggle_threads(world),
        Action::Fold(op) => index::fold(world, *op),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_known_commands_with_and_without_colon() {
        assert_eq!(parse_command(":quit").unwrap(), Action::Quit);
        assert_eq!(parse_command("quit").unwrap(), Action::Quit);
        assert_eq!(parse_command(":q").unwrap(), Action::Quit);
        assert_eq!(parse_command(":tab-next").unwrap(), Action::TabNext);
        assert_eq!(parse_command(":tab-prev").unwrap(), Action::TabPrev);
        assert_eq!(
            parse_command(":command-line").unwrap(),
            Action::OpenCommandLine
        );
    }

    #[test]
    fn echo_keeps_its_arguments() {
        assert_eq!(
            parse_command(":echo hello world").unwrap(),
            Action::Echo("hello world".to_owned())
        );
        assert_eq!(parse_command(":echo").unwrap(), Action::Echo(String::new()));
    }

    #[test]
    fn parses_cursor_sort_and_flag_commands() {
        use crate::index::{SortKey, SortMode};
        assert_eq!(
            parse_command(":next").unwrap(),
            Action::Cursor(Motion::Next)
        );
        assert_eq!(
            parse_command(":prev-page").unwrap(),
            Action::Cursor(Motion::PrevPage)
        );
        assert_eq!(parse_command(":last").unwrap(), Action::Cursor(Motion::Last));
        assert_eq!(
            parse_command(":sort from -r").unwrap(),
            Action::Sort(SortMode {
                key: SortKey::From,
                reverse: true
            })
        );
        assert_eq!(
            parse_command(":toggle-read").unwrap(),
            Action::Flag {
                flag: Flags::SEEN,
                op: FlagOp::Toggle
            }
        );
        assert_eq!(
            parse_command(":unflag").unwrap(),
            Action::Flag {
                flag: Flags::FLAGGED,
                op: FlagOp::Clear
            }
        );
        assert!(parse_command(":sort sideways").is_err());
    }

    #[test]
    fn unknown_and_empty_commands_error_with_context() {
        let message = parse_command(":frobnicate").unwrap_err().to_string();
        assert!(message.contains("frobnicate"), "{message}");
        assert!(parse_command("").is_err());
        assert!(parse_command(":").is_err());
    }

    #[test]
    fn extra_arguments_on_no_arg_commands_error() {
        let message = format!("{:#}", parse_command(":quit now").unwrap_err());
        assert!(message.contains("no arguments"), "{message}");
    }

    #[test]
    fn completion_ranks_fuzzy_matches() {
        let all = complete_command("");
        assert_eq!(all.len(), COMMANDS.len());
        let tab = complete_command("tb");
        assert!(tab.contains(&"tab-next".to_owned()), "{tab:?}");
        assert!(complete_command("zzz").is_empty());
    }
}
