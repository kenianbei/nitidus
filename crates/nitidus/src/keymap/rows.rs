//! Binding enumeration for the help overlay: walk the tries into
//! crokey-formatted rows, merge layers shadow-aware, or group every
//! context in `KNOWN_CONTEXTS` order.

use crokey::KeyCombination;

use super::{KNOWN_CONTEXTS, Keymaps, TrieNode};

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

    /// The rows that resolve right now, given the layer stack the router
    /// walks — most specific first. A sequence an earlier layer answers
    /// shadows every later spelling of it, exactly as `resolve_layered`
    /// decides which one fires.
    pub fn help_rows(&self, layers: &[&str]) -> Vec<HelpRow> {
        let mut shadowed = std::collections::HashSet::new();
        let mut rows = Vec::new();
        for layer in layers {
            for row in self.bindings(layer) {
                if shadowed.insert(row.keys.clone()) {
                    rows.push(HelpRow::new(layer, row));
                }
            }
        }
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
