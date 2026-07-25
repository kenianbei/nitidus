//! Compiled keymaps: per-context tries mapping key sequences to actions.
//! Compiled once at startup from built-in defaults overlaid by the
//! user's keys.toml; compilation failures exit like config errors.

use std::collections::HashMap;

use anyhow::{Context, bail};
use bevy::prelude::Resource;
use crokey::KeyCombination;

use crate::action::{Action, parse_command};
use crate::config::{RawKeymaps, parse_key_sequence};

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Normal,
    CommandLine,
}

/// The active input mode. A plain resource (not bevy `States`) so mode
/// switches apply synchronously — burst input arriving within one frame
/// must route each key against the mode the previous key produced.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode(pub InputMode);

pub const CONTEXT_GLOBAL: &str = "global";
pub const CONTEXT_INDEX: &str = "index";
pub const CONTEXT_PAGER: &str = "pager";
pub const CONTEXT_PICKER: &str = "picker";
pub const CONTEXT_SIDEBAR: &str = "sidebar";

/// Contexts accepted in keys.toml. Screens activate theirs as they land;
/// binding an inactive context is allowed, a typo is not.
pub const KNOWN_CONTEXTS: &[&str] = &[
    CONTEXT_GLOBAL,
    CONTEXT_INDEX,
    CONTEXT_PAGER,
    CONTEXT_PICKER,
    CONTEXT_SIDEBAR,
    "compose",
    "contacts",
    "command_line",
];

const DEFAULT_GLOBAL_BINDINGS: &[(&str, &str)] = &[
    ("q", ":quit"),
    (":", ":command-line"),
    ("<Tab>", ":tab-next"),
    ("<BackTab>", ":tab-prev"),
];

const DEFAULT_INDEX_BINDINGS: &[(&str, &str)] = &[
    ("j", ":next"),
    ("k", ":prev"),
    ("<Down>", ":next"),
    ("<Up>", ":prev"),
    ("<PageDown>", ":next-page"),
    ("<PageUp>", ":prev-page"),
    ("gg", ":first"),
    ("G", ":last"),
    ("N", ":toggle-read"),
    ("F", ":toggle-flag"),
    ("T", ":threads"),
    ("za", ":fold"),
    ("zM", ":fold-all"),
    ("zR", ":unfold-all"),
    ("P", ":parent"),
    ("<Enter>", ":view"),
    ("b", ":sidebar"),
    ("<Tab>", ":sidebar-focus"),
];

const DEFAULT_PAGER_BINDINGS: &[(&str, &str)] = &[
    ("q", ":close"),
    ("j", ":next"),
    ("k", ":prev"),
    ("<Down>", ":next"),
    ("<Up>", ":prev"),
    ("<Space>", ":next-page"),
    ("<PageDown>", ":next-page"),
    ("<PageUp>", ":prev-page"),
    ("gg", ":first"),
    ("G", ":last"),
    ("J", ":next-message"),
    ("K", ":prev-message"),
    ("H", ":headers"),
    ("S", ":skip-quoted"),
    ("]", ":next-part"),
    ("[", ":prev-part"),
    ("s", ":save-part"),
    ("o", ":open-part"),
    ("l", ":links"),
    ("b", ":sidebar"),
    ("<Tab>", ":sidebar-focus"),
];

const DEFAULT_SIDEBAR_BINDINGS: &[(&str, &str)] = &[
    ("j", ":next"),
    ("k", ":prev"),
    ("<Down>", ":next"),
    ("<Up>", ":prev"),
    ("<PageDown>", ":next-page"),
    ("<PageUp>", ":prev-page"),
    ("gg", ":first"),
    ("G", ":last"),
    ("P", ":parent"),
    ("<Enter>", ":view"),
    ("za", ":fold"),
    ("zM", ":fold-all"),
    ("zR", ":unfold-all"),
    ("b", ":sidebar"),
    ("<Tab>", ":sidebar-focus"),
    ("<Esc>", ":sidebar-focus"),
    ("c", ":command-line folder-create"),
    ("r", ":command-line folder-rename"),
    ("D", ":command-line folder-delete"),
];

/// Only single-key bindings are meaningful here: the picker resolves
/// one key at a time because unbound printables type into the filter.
const DEFAULT_PICKER_BINDINGS: &[(&str, &str)] = &[
    ("<Down>", ":next"),
    ("<Up>", ":prev"),
    ("<C-j>", ":next"),
    ("<C-k>", ":prev"),
    ("<Enter>", ":confirm"),
    ("<Esc>", ":cancel"),
];

#[derive(Debug, Default)]
struct TrieNode {
    action: Option<Action>,
    children: HashMap<KeyCombination, TrieNode>,
}

#[derive(Debug, Default, Resource)]
pub struct Keymaps {
    contexts: HashMap<String, TrieNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapMatch {
    Exact(Action),
    Prefix,
    Unbound,
}

impl Keymaps {
    pub fn compile(raw: &RawKeymaps) -> anyhow::Result<Self> {
        let mut keymaps = Self::default();
        for (sequence, command) in DEFAULT_GLOBAL_BINDINGS {
            keymaps.bind(CONTEXT_GLOBAL, sequence, command)?;
        }
        for (sequence, command) in DEFAULT_INDEX_BINDINGS {
            keymaps.bind(CONTEXT_INDEX, sequence, command)?;
        }
        for (sequence, command) in DEFAULT_PAGER_BINDINGS {
            keymaps.bind(CONTEXT_PAGER, sequence, command)?;
        }
        for (sequence, command) in DEFAULT_PICKER_BINDINGS {
            keymaps.bind(CONTEXT_PICKER, sequence, command)?;
        }
        for (sequence, command) in DEFAULT_SIDEBAR_BINDINGS {
            keymaps.bind(CONTEXT_SIDEBAR, sequence, command)?;
        }
        for (context, bindings) in &raw.0 {
            if !KNOWN_CONTEXTS.contains(&context.as_str()) {
                bail!(
                    "unknown keymap context [{context}] (known: {})",
                    KNOWN_CONTEXTS.join(", ")
                );
            }
            for (sequence, command) in bindings {
                keymaps
                    .apply_user_binding(context, sequence, command)
                    .with_context(|| format!("binding {sequence:?} in [{context}]"))?;
            }
        }
        Ok(keymaps)
    }

    fn apply_user_binding(
        &mut self,
        context: &str,
        sequence: &str,
        command: &str,
    ) -> anyhow::Result<()> {
        if command.is_empty() {
            self.unbind(context, sequence)
        } else {
            self.bind(context, sequence, command)
        }
    }

    fn bind(&mut self, context: &str, sequence: &str, command: &str) -> anyhow::Result<()> {
        let action = parse_command(command)?;
        let keys = parse_key_sequence(sequence)?;
        let mut node = self.contexts.entry(context.to_owned()).or_default();
        for key in keys {
            node = node.children.entry(key).or_default();
        }
        node.action = Some(action);
        Ok(())
    }

    fn unbind(&mut self, context: &str, sequence: &str) -> anyhow::Result<()> {
        let keys = parse_key_sequence(sequence)?;
        let Some(mut node) = self.contexts.get_mut(context) else {
            return Ok(());
        };
        for key in &keys {
            match node.children.get_mut(key) {
                Some(next) => node = next,
                None => return Ok(()),
            }
        }
        node.action = None;
        Ok(())
    }

    /// Context bindings shadow global ones: a context exact fires first,
    /// a prefix in either layer waits for the chord, and only then may a
    /// global exact fire.
    pub fn resolve_layered(&self, context: &str, keys: &[KeyCombination]) -> KeymapMatch {
        let scoped = self.lookup(context, keys);
        if matches!(scoped, KeymapMatch::Exact(_)) {
            return scoped;
        }
        let global = self.lookup(CONTEXT_GLOBAL, keys);
        match (scoped, global) {
            (KeymapMatch::Prefix, _) | (_, KeymapMatch::Prefix) => KeymapMatch::Prefix,
            (_, global) => global,
        }
    }

    /// An exact match fires immediately even if longer bindings share the
    /// prefix — predictability over vim-style ambiguity waits.
    pub fn lookup(&self, context: &str, keys: &[KeyCombination]) -> KeymapMatch {
        let Some(mut node) = self.contexts.get(context) else {
            return KeymapMatch::Unbound;
        };
        for key in keys {
            match node.children.get(key) {
                Some(next) => node = next,
                None => return KeymapMatch::Unbound,
            }
        }
        match &node.action {
            Some(action) => KeymapMatch::Exact(action.clone()),
            None if node.children.is_empty() => KeymapMatch::Unbound,
            None => KeymapMatch::Prefix,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::*;

    fn keys(sequence: &str) -> Vec<KeyCombination> {
        parse_key_sequence(sequence).unwrap()
    }

    fn raw(context: &str, bindings: &[(&str, &str)]) -> RawKeymaps {
        let map: BTreeMap<String, String> = bindings
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        RawKeymaps(BTreeMap::from([(context.to_owned(), map)]))
    }

    #[test]
    fn defaults_bind_quit_and_command_line() {
        let keymaps = Keymaps::compile(&RawKeymaps::default()).unwrap();
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("q")),
            KeymapMatch::Exact(Action::Quit)
        );
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys(":")),
            KeymapMatch::Exact(Action::OpenCommandLine(String::new()))
        );
    }

    #[test]
    fn user_binding_overrides_default() {
        let keymaps = Keymaps::compile(&raw("global", &[("q", ":tab-next")])).unwrap();
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("q")),
            KeymapMatch::Exact(Action::TabNext)
        );
    }

    #[test]
    fn empty_command_unbinds_a_default() {
        let keymaps = Keymaps::compile(&raw("global", &[("q", "")])).unwrap();
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("q")),
            KeymapMatch::Unbound
        );
    }

    #[test]
    fn multi_key_sequences_report_prefix_then_exact() {
        let keymaps = Keymaps::compile(&raw("global", &[("gg", ":tab-prev")])).unwrap();
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("g")),
            KeymapMatch::Prefix
        );
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("gg")),
            KeymapMatch::Exact(Action::TabPrev)
        );
        assert_eq!(
            keymaps.lookup(CONTEXT_GLOBAL, &keys("gx")),
            KeymapMatch::Unbound
        );
    }

    #[test]
    fn index_defaults_resolve_through_layering() {
        let keymaps = Keymaps::compile(&RawKeymaps::default()).unwrap();
        assert!(matches!(
            keymaps.resolve_layered(CONTEXT_INDEX, &keys("j")),
            KeymapMatch::Exact(Action::Cursor(crate::action::Motion::Next))
        ));
        assert_eq!(
            keymaps.resolve_layered(CONTEXT_INDEX, &keys("q")),
            KeymapMatch::Exact(Action::Quit),
            "global bindings must fall through"
        );
    }

    #[test]
    fn context_binding_shadows_global() {
        let keymaps = Keymaps::compile(&raw("index", &[("q", ":tab-next")])).unwrap();
        assert_eq!(
            keymaps.resolve_layered(CONTEXT_INDEX, &keys("q")),
            KeymapMatch::Exact(Action::TabNext)
        );
    }

    #[test]
    fn context_prefix_outweighs_global_exact() {
        let keymaps = Keymaps::compile(&raw("global", &[("g", ":tab-next")])).unwrap();
        assert_eq!(
            keymaps.resolve_layered(CONTEXT_INDEX, &keys("g")),
            KeymapMatch::Prefix,
            "gg in index must make bare g wait for the chord"
        );
        assert_eq!(
            keymaps.resolve_layered("contacts", &keys("g")),
            KeymapMatch::Exact(Action::TabNext),
            "contexts without the chord fire the global immediately"
        );
    }

    #[test]
    fn unknown_context_is_a_compile_error() {
        let message = Keymaps::compile(&raw("indx", &[("q", ":quit")]))
            .unwrap_err()
            .to_string();
        assert!(message.contains("indx"), "{message}");
    }

    #[test]
    fn future_contexts_compile_without_being_active() {
        assert!(Keymaps::compile(&raw("pager", &[("q", ":quit")])).is_ok());
    }

    #[test]
    fn bad_command_in_binding_names_the_binding() {
        let message = format!(
            "{:#}",
            Keymaps::compile(&raw("global", &[("x", ":frobnicate")])).unwrap_err()
        );
        assert!(message.contains("frobnicate"), "{message}");
        assert!(message.contains('x'), "{message}");
    }
}
