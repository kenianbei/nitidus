//! The folder sidebar: per-account folder trees with unread counts in a
//! left column of the content region. Selection, collapse, and folder
//! switching live here; the index and pager swap between the full
//! content rect and the main column as visibility changes.

mod mouse;
mod ops;
mod render;
mod tree;

pub use ops::{
    fold, folder_create, folder_delete, folder_rename, move_cursor, select, toggle_focus,
    toggle_visible,
};
pub use tree::{AccountSection, FolderEntry, RowKind, SidebarRow};

use std::collections::HashSet;

use bevy::prelude::*;
use nitidus_mail::{AccountId, FolderMeta};
use plurimus::{Widget, WidgetLayout};

use crate::config::Config;
use crate::panes::{MailPane, PaneBudget, mail_layout};
use crate::store::{MailStore, SyncTracker};

pub(crate) const INBOX_NAME: &str = nitidus_mail::maildir::INBOX;
/// Gmail's label-mirror namespace starts collapsed; it is rarely what
/// the user is looking for.
const DEFAULT_COLLAPSED_PREFIX: &str = "[Gmail]";

pub struct SidebarPlugin;

impl Plugin for SidebarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SidebarState>();
        app.init_resource::<SidebarRows>();
        app.init_resource::<crate::focus::PaneFocus>();
        app.add_systems(Startup, spawn_sidebar);
        app.add_systems(
            Update,
            (
                refresh_rows,
                mouse::clear_departed_hover,
                render::refresh_sidebar,
                apply_visibility,
            )
                .chain(),
        );
    }
}

#[derive(Resource)]
pub struct SidebarState {
    pub visible: bool,
    pub selected: usize,
    pub top: usize,
    pub collapsed: HashSet<(AccountId, String)>,
    /// Accounts whose default collapse state has been applied once.
    seeded: HashSet<AccountId>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            visible: true,
            selected: 0,
            top: 0,
            collapsed: HashSet::new(),
            seeded: HashSet::new(),
        }
    }
}

/// The current display rows; rebuilt when folders, sync state, or
/// collapse state change — never on cursor movement alone.
#[derive(Resource, Default)]
pub struct SidebarRows(pub Vec<SidebarRow>);

#[derive(Component)]
pub struct SidebarWidget;

fn spawn_sidebar(config: Res<Config>, mut commands: Commands) {
    commands.spawn((
        SidebarWidget,
        Widget::from_render_fn_with_state(render::render_sidebar, render::SidebarWindow::default()),
        WidgetLayout::from(mail_layout(
            MailPane::Folders,
            PaneBudget::new(true, config.ui.index.list_width()),
        )),
        plurimus::UiActions::new(vec![plurimus::UiInputBinding::mouse_passthrough(
            mouse::handle,
        )]),
        plurimus::UiHoverable,
    ));
}

fn refresh_rows(
    config: Res<Config>,
    store: Res<MailStore>,
    tracker: Res<SyncTracker>,
    mut state: ResMut<SidebarState>,
    mut rows: ResMut<SidebarRows>,
) {
    if !(store.is_changed() || state.is_changed() || tracker.is_changed()) {
        return;
    }
    let sections = build_sections(&config, &store, &tracker);
    // Seeding and clamping are derived bookkeeping; tracked writes here
    // would re-trigger this system every frame.
    let cached = state.bypass_change_detection();
    seed_default_collapse(cached, &sections);
    let built = tree::build_rows(&sections, &cached.collapsed);
    if built != rows.0 {
        rows.0 = built;
    }
    clamp_selection(cached, &rows.0);
}

fn build_sections(
    config: &Config,
    store: &MailStore,
    tracker: &SyncTracker,
) -> Vec<AccountSection> {
    config
        .accounts
        .iter()
        .filter_map(|account_config| {
            let account = AccountId::new(&account_config.name);
            let folders = store.folders(&account);
            if folders.is_empty() {
                return None;
            }
            let entries = folders
                .iter()
                .map(|meta| FolderEntry {
                    id: meta.id.clone(),
                    path: meta.name.clone(),
                    unread: effective_unread(store, tracker, &account, meta),
                })
                .collect();
            Some(AccountSection {
                account,
                label: account_config.name.clone(),
                entries,
            })
        })
        .collect()
}

/// Store-derived counts for folders synced or hydrated this session
/// (they follow optimistic flag edits); discovery snapshots otherwise.
fn effective_unread(
    store: &MailStore,
    tracker: &SyncTracker,
    account: &AccountId,
    meta: &FolderMeta,
) -> u32 {
    let envelopes = store.envelopes(account, &meta.id);
    if envelopes.is_empty() && !tracker.is_tracked(account, &meta.id) {
        return meta.unread;
    }
    let unread = envelopes
        .iter()
        .filter(|envelope| !envelope.flags.contains(nitidus_mail::Flags::SEEN))
        .count();
    u32::try_from(unread).unwrap_or(u32::MAX)
}

fn seed_default_collapse(state: &mut SidebarState, sections: &[AccountSection]) {
    for section in sections {
        if !state.seeded.insert(section.account.clone()) {
            continue;
        }
        let has_gmail_namespace = section
            .entries
            .iter()
            .any(|entry| entry.path.starts_with(DEFAULT_COLLAPSED_PREFIX));
        if has_gmail_namespace {
            state
                .collapsed
                .insert((section.account.clone(), DEFAULT_COLLAPSED_PREFIX.to_owned()));
        }
    }
}

/// Rebuilds can remove the selected row (collapse, folder deletion);
/// clamp to the nearest selectable row.
fn clamp_selection(state: &mut SidebarState, rows: &[SidebarRow]) {
    if rows.is_empty() {
        state.selected = 0;
        state.top = 0;
        return;
    }
    let limit = rows.len() - 1;
    state.selected = state.selected.min(limit);
    if !rows[state.selected].is_selectable() {
        let fallback = (state.selected..=limit)
            .chain((0..state.selected).rev())
            .find(|&row| rows[row].is_selectable());
        state.selected = fallback.unwrap_or(0);
    }
    state.top = state.top.min(state.selected);
}

/// Re-columns the message and reading panes when the sidebar comes or
/// goes; the sidebar widget draws nothing while hidden.
fn apply_visibility(
    (state, config): (Res<SidebarState>, Res<Config>),
    mut last_visible: Local<Option<bool>>,
    mut commands: Commands,
    messages: Query<Entity, With<crate::index::IndexWidget>>,
    reading: Query<Entity, With<crate::pager::PagerWidget>>,
) {
    if *last_visible == Some(state.visible) {
        return;
    }
    *last_visible = Some(state.visible);
    let budget = PaneBudget::new(state.visible, config.ui.index.list_width());
    for (entity, pane) in messages
        .iter()
        .map(|entity| (entity, MailPane::Messages))
        .chain(reading.iter().map(|entity| (entity, MailPane::Reading)))
    {
        commands
            .entity(entity)
            .insert(WidgetLayout::from(mail_layout(pane, budget)));
    }
}
