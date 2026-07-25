//! The help overlay: a searchable picker over the bindings that work
//! right now (or, toggled, every context), each row carrying its key
//! sequence, command, and summary. Enter executes the selected row, so
//! help doubles as a command palette.

use bevy::prelude::*;

use crate::action::{apply_action, parse_command};
use crate::command::describe;
use crate::keymap::{
    CONTEXT_GLOBAL, CONTEXT_INDEX, CONTEXT_PAGER, CONTEXT_SIDEBAR, HelpRow, Keymaps,
};
use crate::overlay::{ActiveOverlay, PickerItem, PickerSpec, open_picker};
use crate::screen::Screen;

const TITLE_PREFIX: &str = "keys — ";
const ALL_TITLE: &str = "keys — all";
const KEY_COLUMN_WIDTH: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpScope {
    /// The active context plus non-shadowed globals.
    Current,
    /// Every context, grouped in `KNOWN_CONTEXTS` order.
    All,
}

pub fn open(world: &mut World, scope: HelpScope) {
    let context = active_context(world);
    let rows = {
        let keymaps = world.resource::<Keymaps>();
        match scope {
            HelpScope::Current => keymaps.help_rows(context),
            HelpScope::All => keymaps.all_help_rows(),
        }
    };
    let title = match scope {
        HelpScope::Current => format!("{TITLE_PREFIX}{context}"),
        HelpScope::All => ALL_TITLE.to_owned(),
    };
    let commands: Vec<String> = rows.iter().map(|row| row.command.clone()).collect();
    let items = rows.iter().map(|row| picker_item(row, scope)).collect();
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
    crate::overlay::close(world);
    open(world, next);
}

fn picker_item(row: &HelpRow, scope: HelpScope) -> PickerItem {
    let command = row.command.trim_start_matches(':');
    let label = match scope {
        HelpScope::Current => format!("{:<KEY_COLUMN_WIDTH$} {command}", row.keys),
        HelpScope::All => format!(
            "[{}] {:<KEY_COLUMN_WIDTH$} {command}",
            row.context, row.keys
        ),
    };
    let summary = describe(&row.command).unwrap_or_default();
    let detail = if scope == HelpScope::Current && row.context == CONTEXT_GLOBAL {
        format!("{summary} (global)")
    } else {
        summary.to_owned()
    };
    PickerItem {
        label,
        detail: (!detail.is_empty()).then_some(detail),
    }
}

/// Mirrors the router's context choice: a focused sidebar wins, then
/// the active screen.
fn active_context(world: &World) -> &'static str {
    if crate::sidebar::is_focused(world) {
        return CONTEXT_SIDEBAR;
    }
    match world.get_resource::<Screen>().copied().unwrap_or_default() {
        Screen::Pager => CONTEXT_PAGER,
        Screen::Index => CONTEXT_INDEX,
    }
}
