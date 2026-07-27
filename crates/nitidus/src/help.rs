//! The help overlay: a searchable picker over the bindings that work
//! right now (or, toggled, every context), each row carrying its key
//! sequence, command, and summary. Enter executes the selected row, so
//! help doubles as a command palette.
//!
//! "Right now" means the whole layer stack the keyboard is resolving
//! against, not one context: over the composer that is the body's
//! editor keys, the form's, and the composer's own — and none of the
//! globals, which a form does not fall through to.

use bevy::prelude::*;

use crate::action::{apply_action, parse_command};
use crate::command::describe;
use crate::keymap::{CONTEXT_GLOBAL, HelpRow, Keymaps};
use crate::overlay::{ActiveOverlay, PickerItem, PickerSpec, open_picker};

const TITLE_PREFIX: &str = "keys — ";
const ALL_TITLE: &str = "keys — all";
const LAYER_SEPARATOR: &str = " · ";
const KEY_COLUMN_WIDTH: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpScope {
    /// The active context plus non-shadowed globals.
    Current,
    /// Every context, grouped in `KNOWN_CONTEXTS` order.
    All,
}

pub fn open(world: &mut World, scope: HelpScope) {
    let layers = crate::overlay::surface::key_layers(world);
    let rows = {
        let keymaps = world.resource::<Keymaps>();
        match scope {
            HelpScope::Current => keymaps.help_rows(&layers),
            HelpScope::All => keymaps.all_help_rows(),
        }
    };
    let title = match scope {
        HelpScope::Current => current_title(&layers),
        HelpScope::All => ALL_TITLE.to_owned(),
    };
    let primary = layers.first().copied().unwrap_or(CONTEXT_GLOBAL);
    let commands: Vec<String> = rows.iter().map(|row| row.command.clone()).collect();
    let items = rows
        .iter()
        .map(|row| picker_item(row, scope, primary))
        .collect();
    open_picker(
        world,
        PickerSpec {
            title,
            items,
            on_select: Box::new(move |world, picked| {
                if let Ok(action) = parse_command(&commands[picked]) {
                    apply_action(world, &action);
                }
            }),
        },
    );
}

/// `<Tab>` in the picker: flips help between current-context and
/// all-contexts; a no-op for every other picker.
pub fn toggle_scope(world: &mut World) {
    let Some(title) = world.resource::<ActiveOverlay>().title().map(str::to_owned) else {
        return;
    };
    let next = if title == ALL_TITLE {
        HelpScope::Current
    } else if title.starts_with(TITLE_PREFIX) {
        HelpScope::All
    } else {
        return;
    };
    crate::overlay::picker::close(world);
    open(world, next);
}

/// Globals answer everywhere, so naming them tells the user nothing;
/// what places them is the stack above.
fn current_title(layers: &[&str]) -> String {
    let named: Vec<&str> = layers
        .iter()
        .copied()
        .filter(|layer| *layer != CONTEXT_GLOBAL)
        .collect();
    if named.is_empty() {
        return format!("{TITLE_PREFIX}{CONTEXT_GLOBAL}");
    }
    format!("{TITLE_PREFIX}{}", named.join(LAYER_SEPARATOR))
}

fn picker_item(row: &HelpRow, scope: HelpScope, primary: &str) -> PickerItem {
    let command = row.command.trim_start_matches(':');
    let label = match scope {
        HelpScope::Current => format!("{:<KEY_COLUMN_WIDTH$} {command}", row.keys),
        HelpScope::All => format!(
            "[{}] {:<KEY_COLUMN_WIDTH$} {command}",
            row.context, row.keys
        ),
    };
    let summary = describe(&row.command).unwrap_or_default();
    let layer =
        (scope == HelpScope::Current && row.context != primary).then_some(row.context.as_str());
    let detail = match (summary, layer) {
        ("", None) => String::new(),
        ("", Some(layer)) => format!("({layer})"),
        (summary, None) => summary.to_owned(),
        (summary, Some(layer)) => format!("{summary} ({layer})"),
    };
    PickerItem {
        label,
        detail: (!detail.is_empty()).then_some(detail),
    }
}
