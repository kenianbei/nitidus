//! The command vocabulary: every operation is a named command string
//! that parses to an `Action`. Keybindings, the command line, and future
//! macros all share this one parser.

use anyhow::{Context, bail};
use bevy::app::AppExit;
use bevy::prelude::*;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher};

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
];

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
