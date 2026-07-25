//! Binding enumeration for the help overlay: walk the tries into
//! crokey-formatted rows, merge layers shadow-aware, or group every
//! context in `KNOWN_CONTEXTS` order.

use crokey::KeyCombination;

use super::{CONTEXT_GLOBAL, KNOWN_CONTEXTS, Keymaps, TrieNode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingRow {
    pub keys: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpRow {
    pub context: String,
    pub keys: String,
    pub command: String,
}

impl HelpRow {
    fn new(context: &str, row: BindingRow) -> Self {
        Self {
            context: context.to_owned(),
            keys: row.keys,
            command: row.command,
        }
    }
}

fn walk_bindings(node: &TrieNode, prefix: &mut Vec<KeyCombination>, rows: &mut Vec<BindingRow>) {
    if let Some(command) = &node.command {
        rows.push(BindingRow {
            keys: crate::router::format_keys(prefix),
            command: command.clone(),
        });
    }
    for (key, child) in &node.children {
        prefix.push(*key);
        walk_bindings(child, prefix, rows);
        prefix.pop();
    }
}

impl Keymaps {
    /// Every binding in one context, crokey-formatted, sorted by
    /// sequence.
    pub fn bindings(&self, context: &str) -> Vec<BindingRow> {
        let mut rows = Vec::new();
        if let Some(root) = self.contexts.get(context) {
            walk_bindings(root, &mut Vec::new(), &mut rows);
        }
        rows.sort_by(|a, b| a.keys.cmp(&b.keys));
        rows
    }

    /// The rows that resolve right now in `context`: its own bindings
    /// plus globals whose sequences the context does not shadow.
    pub fn help_rows(&self, context: &str) -> Vec<HelpRow> {
        let scoped = self.bindings(context);
        let shadowed: std::collections::HashSet<String> =
            scoped.iter().map(|row| row.keys.clone()).collect();
        let mut rows: Vec<HelpRow> = scoped
            .into_iter()
            .map(|row| HelpRow::new(context, row))
            .collect();
        rows.extend(
            self.bindings(CONTEXT_GLOBAL)
                .into_iter()
                .filter(|row| !shadowed.contains(&row.keys))
                .map(|row| HelpRow::new(CONTEXT_GLOBAL, row)),
        );
        rows
    }

    /// Every context's rows in `KNOWN_CONTEXTS` order.
    pub fn all_help_rows(&self) -> Vec<HelpRow> {
        KNOWN_CONTEXTS
            .iter()
            .flat_map(|context| {
                self.bindings(context)
                    .into_iter()
                    .map(|row| HelpRow::new(context, row))
            })
            .collect()
    }
}
