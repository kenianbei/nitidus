//! Raw keymap parsing and key-notation validation. Binding semantics
//! (the trie, actions) belong to the action router; this module only
//! guarantees that keys.toml contains well-formed sequences.

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use bevy::prelude::Resource;
use crokey::KeyCombination;
use serde::{Deserialize, Serialize};

/// context → key sequence → command string, mirroring keys.toml.
#[derive(Clone, Debug, Default, PartialEq, Eq, Resource, Deserialize, Serialize)]
#[serde(transparent)]
pub struct RawKeymaps(pub BTreeMap<String, BTreeMap<String, String>>);

impl RawKeymaps {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (context, bindings) in &self.0 {
            for sequence in bindings.keys() {
                parse_key_sequence(sequence)
                    .with_context(|| format!("invalid key sequence {sequence:?} in [{context}]"))?;
            }
        }
        Ok(())
    }
}

/// Parses aerc-style notation into key combinations: bare characters are
/// one key each (`"dd"` → `d`, `d`); angle groups name specials and
/// modifiers (`"<C-x>"`, `"<Enter>"`, `"g<Tab>"`).
pub fn parse_key_sequence(sequence: &str) -> anyhow::Result<Vec<KeyCombination>> {
    if sequence.is_empty() {
        bail!("empty key sequence");
    }
    let mut combinations = Vec::new();
    let mut chars = sequence.chars();
    while let Some(ch) = chars.next() {
        let token = match ch {
            '<' => collect_group(&mut chars)?,
            '>' => bail!("unmatched '>'"),
            _ => ch.to_string(),
        };
        combinations.push(parse_token(&token)?);
    }
    Ok(combinations)
}

fn collect_group(chars: &mut std::str::Chars) -> anyhow::Result<String> {
    let group: String = chars.by_ref().take_while(|&c| c != '>').collect();
    if group.is_empty() {
        bail!("empty '<>' group");
    }
    Ok(group)
}

fn parse_token(token: &str) -> anyhow::Result<KeyCombination> {
    let normalized = normalize_token(token);
    let parsed = crokey::parse(&normalized)
        .map_err(|error| anyhow::anyhow!("unrecognized key {token:?}: {error}"))?;
    Ok(fold_back_tab(parsed))
}

/// A terminal reports Shift-Tab as `BackTab`, never as Tab with a shift
/// modifier — so `<S-Tab>`, the spelling everyone reaches for, would
/// compile to a combination no key event can ever match. Fold it onto
/// the one crossterm actually delivers, which is also what `<BackTab>`
/// parses to, so both spellings mean the same binding.
fn fold_back_tab(combination: KeyCombination) -> KeyCombination {
    use bevy_ratatui::crossterm::event::{KeyCode, KeyModifiers};

    // Shift and nothing else: `<C-S-Tab>` is a different key that
    // terminals do report as Tab with modifiers, and must survive.
    let is_shift_tab = combination.codes == crokey::OneToThree::One(KeyCode::Tab)
        && combination.modifiers == KeyModifiers::SHIFT;
    if is_shift_tab {
        return KeyCombination::new(KeyCode::BackTab, KeyModifiers::SHIFT);
    }
    combination
}

fn normalize_token(token: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = token;
    loop {
        match rest.split_at_checked(2) {
            Some(("C-", tail)) => {
                parts.push("ctrl".to_owned());
                rest = tail;
            }
            Some(("A-", tail)) => {
                parts.push("alt".to_owned());
                rest = tail;
            }
            Some(("S-", tail)) => {
                parts.push("shift".to_owned());
                rest = tail;
            }
            _ => break,
        }
    }
    parts.push(normalize_key_name(rest));
    parts.join("-")
}

fn normalize_key_name(name: &str) -> String {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        // crokey's parser lowercases its input, so an uppercase letter
        // must be spelled shift-<lower> to match the shift-normalized
        // combination crossterm delivers for a typed capital.
        (Some(c), None) if c.is_ascii_uppercase() => {
            format!("shift-{}", c.to_ascii_lowercase())
        }
        _ => match name {
            "PgUp" => "pageup".to_owned(),
            "PgDn" => "pagedown".to_owned(),
            "Del" => "delete".to_owned(),
            other if other.chars().count() > 1 => other.to_ascii_lowercase(),
            other => other.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn bare_characters_become_one_combination_each() {
        assert_eq!(parse_key_sequence("dd").unwrap().len(), 2);
        assert_eq!(parse_key_sequence("gg").unwrap().len(), 2);
        assert_eq!(parse_key_sequence("q").unwrap().len(), 1);
    }

    #[test]
    fn angle_groups_parse_specials_and_modifiers() {
        assert_eq!(parse_key_sequence("<Enter>").unwrap().len(), 1);
        assert_eq!(parse_key_sequence("<C-x>").unwrap().len(), 1);
        assert_eq!(parse_key_sequence("<C-S-Tab>").unwrap().len(), 1);
        assert_eq!(parse_key_sequence("g<Tab>").unwrap().len(), 2);
        assert_eq!(parse_key_sequence("<PgUp>").unwrap().len(), 1);
    }

    #[test]
    fn uppercase_letters_match_shifted_key_events() {
        use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let parsed = parse_key_sequence("Z").unwrap();
        let typed = KeyCombination::from(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT));
        assert_eq!(parsed, vec![typed]);
    }

    /// A terminal never sends Tab-with-shift, so an `<S-Tab>` binding
    /// taken literally would be unreachable in every context.
    #[test]
    fn shift_tab_matches_the_back_tab_a_terminal_actually_sends() {
        use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let pressed = KeyCombination::from(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(parse_key_sequence("<S-Tab>").unwrap(), vec![pressed]);
        assert_eq!(
            parse_key_sequence("<BackTab>").unwrap(),
            parse_key_sequence("<S-Tab>").unwrap(),
            "both spellings must mean one binding"
        );
    }

    #[test]
    fn a_plain_tab_binding_is_left_alone() {
        use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let pressed = KeyCombination::from(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(parse_key_sequence("<Tab>").unwrap(), vec![pressed]);
        assert_ne!(
            parse_key_sequence("<Tab>").unwrap(),
            parse_key_sequence("<S-Tab>").unwrap()
        );
    }

    /// Ctrl-Shift-Tab is a distinct key a terminal really does report as
    /// Tab with modifiers, so folding must not swallow its Ctrl.
    #[test]
    fn ctrl_shift_tab_keeps_both_modifiers() {
        use bevy_ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let parsed = parse_key_sequence("<C-S-Tab>").unwrap();
        assert_eq!(
            parsed,
            vec![KeyCombination::new(
                KeyCode::Tab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )]
        );
    }

    #[test]
    fn malformed_sequences_are_rejected() {
        assert!(parse_key_sequence("").is_err());
        assert!(parse_key_sequence("<>").is_err());
        assert!(parse_key_sequence("a>").is_err());
        assert!(parse_key_sequence("<NoSuchKey>").is_err());
    }

    #[test]
    fn keymaps_validate_reports_context_and_sequence() {
        let mut bindings = BTreeMap::new();
        bindings.insert("<Bogus>".to_owned(), ":next".to_owned());
        let keymaps = RawKeymaps(BTreeMap::from([("index".to_owned(), bindings)]));
        let message = format!("{:#}", keymaps.validate().unwrap_err());
        assert!(message.contains("index"), "missing context: {message}");
        assert!(message.contains("<Bogus>"), "missing sequence: {message}");
    }

    #[test]
    fn keymaps_parse_from_toml_shape() {
        let keymaps: RawKeymaps = toml::from_str(
            "[index]\n\"dd\" = \":delete\"\n\"gg\" = \":select 0\"\n[pager]\n\"q\" = \":close\"\n",
        )
        .unwrap();
        assert_eq!(keymaps.0["index"]["dd"], ":delete");
        assert!(keymaps.validate().is_ok());
    }
}
