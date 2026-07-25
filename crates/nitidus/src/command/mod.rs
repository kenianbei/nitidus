//! The command-string vocabulary: every named command, its parser, and
//! fuzzy completion. Keybindings, the command line, and future macros
//! all share this one table.

use anyhow::{Context, bail};
use nitidus_mail::Flags;

mod compose_table;
mod table;

use compose_table::COMPOSE_COMMANDS;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher};
use table::COMMANDS;

use crate::action::{Action, FlagOp};

struct CommandSpec {
    name: &'static str,
    summary: &'static str,
    aliases: &'static [&'static str],
    parse: fn(&str) -> anyhow::Result<Action>,
}

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
    let spec = commands()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
        .ok_or_else(|| anyhow::anyhow!("unknown command: {name:?}"))?;
    (spec.parse)(args).with_context(|| format!("in command {name:?}"))
}

/// One-line summary for a command input (arguments ignored).
pub fn describe(input: &str) -> Option<&'static str> {
    let stripped = input.trim();
    let stripped = stripped.strip_prefix(':').unwrap_or(stripped).trim();
    let name = stripped.split_whitespace().next()?;
    commands()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
        .map(|spec| spec.summary)
}

fn commands() -> impl Iterator<Item = &'static CommandSpec> {
    COMMANDS.iter().chain(COMPOSE_COMMANDS.iter())
}

/// Fuzzy completion over command names, best match first.
pub fn complete_command(input: &str) -> Vec<String> {
    let names = commands().map(|spec| spec.name);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn describe_resolves_names_aliases_and_ignores_arguments() {
        assert_eq!(describe(":fold-all"), Some("collapse everything"));
        assert_eq!(describe("q"), describe("quit"));
        assert_eq!(
            describe(":command-line folder-create"),
            Some("open the command line")
        );
        assert_eq!(describe(":frobnicate"), None);
        assert_eq!(describe(""), None);
    }
}
