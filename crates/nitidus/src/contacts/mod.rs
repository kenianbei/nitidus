//! The contact book tab: vdir-loaded contacts in a table pane beside a
//! detail pane, with prompt-chain property editors saving atomically
//! through the vdir store.

mod add;
mod detail;
mod draw;
mod edit;
mod mutate;
mod photo;
mod render;
mod transfer;
mod view;

pub use add::{add_property, delete_selected_contact, new_contact};
pub use edit::{edit_selected, edit_selected_raw, remove_selected_property};
pub use photo::{PhotoPicker, set_photo};
pub use transfer::{export_contacts, import_contacts};
pub use view::{ContactsView, PaneFocus, move_cursor, toggle_focus};

use std::path::PathBuf;

use bevy::prelude::*;
use nitidus_contacts::{ContactBook, load_dir};
use nitidus_ui_kit::layout;
use plurimus::{Widget, WidgetLayout};

use crate::engine::StartupNotices;

const CONTACTS_SUBDIR: &str = "contacts";

pub struct ContactsPlugin;

impl Plugin for ContactsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContactStore>();
        app.init_resource::<ContactsView>();
        app.init_resource::<ContactsStatus>();
        app.add_systems(Startup, (load_book, spawn_contacts));
        app.add_systems(Update, render::refresh_contacts);
    }
}

/// Where the vdir lives; tests override, the app resolves the XDG data
/// tier on startup.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct ContactsDir(pub PathBuf);

#[derive(Resource, Default)]
pub struct ContactStore(pub ContactBook);

/// Selected position / total for the statusline (1-based; zero total
/// means hide).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactsStatus {
    pub selected: usize,
    pub total: usize,
}

#[derive(Component)]
pub struct ContactsWidget;

pub fn default_contacts_dir() -> anyhow::Result<PathBuf> {
    Ok(crate::dirs::data_dir()?.join(CONTACTS_SUBDIR))
}

fn load_book(
    mut commands: Commands,
    dir: Option<Res<ContactsDir>>,
    notices: Option<ResMut<StartupNotices>>,
) {
    let mut reports = Vec::new();
    let dir_path = match &dir {
        Some(dir) => dir.0.clone(),
        None => match default_contacts_dir() {
            Ok(resolved) => {
                commands.insert_resource(ContactsDir(resolved.clone()));
                resolved
            }
            Err(error) => {
                push_notices(notices, vec![format!("contacts: {error:#}")]);
                return;
            }
        },
    };
    match load_dir(&dir_path) {
        Ok((contacts, issues)) => {
            commands.insert_resource(ContactStore(ContactBook::from_contacts(contacts)));
            reports.extend(
                issues
                    .iter()
                    .map(|issue| format!("contacts: {}: {}", issue.file, issue.problem)),
            );
        }
        Err(error) => reports.push(format!("contacts: {}: {error}", dir_path.display())),
    }
    push_notices(notices, reports);
}

fn push_notices(notices: Option<ResMut<StartupNotices>>, reports: Vec<String>) {
    if reports.is_empty() {
        return;
    }
    match notices {
        Some(mut startup) => startup.0.extend(reports),
        None => {
            for report in reports {
                tracing::warn!("{report}");
            }
        }
    }
}

fn spawn_contacts(mut commands: Commands) {
    commands.spawn((
        ContactsWidget,
        Widget::from_render_fn_with_state(draw::render_contacts, render::ContactsWindow::default()),
        WidgetLayout::from(layout::content_layout()),
    ));
}
