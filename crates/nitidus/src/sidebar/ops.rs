//! World-mutating sidebar operations: visibility and focus, tree
//! navigation, collapse, folder switching, and the folder CRUD
//! commands.

use bevy::prelude::*;
use nitidus_mail::{AccountId, FolderId, MailCommand};
use plurimus::Widget;

use super::render::SidebarWindow;
use super::tree::{RowKind, SidebarRow};
use super::{SidebarRows, SidebarState, SidebarWidget};
use crate::action::{FoldOp, Motion, PagerOp};
use crate::bootstrap::request_sync;
use crate::engine::EngineResource;
use crate::index::IndexView;
use crate::screen::Screen;
use crate::status::StatusMessage;
use crate::store::SyncTracker;

const FALLBACK_PAGE_ROWS: usize = 10;

pub fn toggle_visible(world: &mut World) {
    let mut state = world.resource_mut::<SidebarState>();
    state.visible = !state.visible;
    if !state.visible {
        state.focused = false;
    }
}

pub fn toggle_focus(world: &mut World) {
    let mut state = world.resource_mut::<SidebarState>();
    state.focused = !state.focused;
    if state.focused {
        state.visible = true;
    }
}

pub fn is_focused(world: &World) -> bool {
    world
        .get_resource::<SidebarState>()
        .is_some_and(|state| state.focused)
}

pub fn move_cursor(world: &mut World, motion: Motion) {
    let viewport = viewport_rows(world);
    let target = {
        let rows = &world.resource::<SidebarRows>().0;
        let state = world.resource::<SidebarState>();
        resolve_motion(rows, state, motion, viewport)
    };
    if let Some(target) = target {
        let mut state = world.resource_mut::<SidebarState>();
        state.selected = target;
        state.top = scrolled_top(state.top, target, viewport);
    }
}

fn resolve_motion(
    rows: &[SidebarRow],
    state: &SidebarState,
    motion: Motion,
    viewport: usize,
) -> Option<usize> {
    let step =
        |from: usize, forward: bool, count: usize| selectable_step(rows, from, forward, count);
    match motion {
        Motion::Next => step(state.selected, true, 1),
        Motion::Prev => step(state.selected, false, 1),
        Motion::NextPage => step(state.selected, true, viewport.saturating_sub(1).max(1)),
        Motion::PrevPage => step(state.selected, false, viewport.saturating_sub(1).max(1)),
        Motion::First => rows.iter().position(SidebarRow::is_selectable),
        Motion::Last => rows.iter().rposition(SidebarRow::is_selectable),
        Motion::Parent => parent_row(rows, state.selected),
    }
}

fn selectable_step(rows: &[SidebarRow], from: usize, forward: bool, count: usize) -> Option<usize> {
    let mut current = from;
    let mut moved = None;
    for _ in 0..count {
        let next = if forward {
            (current + 1..rows.len()).find(|&row| rows[row].is_selectable())
        } else {
            (0..current).rev().find(|&row| rows[row].is_selectable())
        };
        match next {
            Some(next) => {
                current = next;
                moved = Some(next);
            }
            None => break,
        }
    }
    moved
}

fn parent_row(rows: &[SidebarRow], selected: usize) -> Option<usize> {
    let row = rows.get(selected)?;
    let (parent_path, _) = row.path.rsplit_once('/')?;
    rows.iter()
        .position(|candidate| candidate.account == row.account && candidate.path == parent_path)
}

fn scrolled_top(top: usize, selected: usize, viewport: usize) -> usize {
    if selected < top {
        selected
    } else if selected >= top + viewport {
        selected + 1 - viewport
    } else {
        top
    }
}

fn viewport_rows(world: &mut World) -> usize {
    let mut widgets = world.query_filtered::<&mut Widget, With<SidebarWidget>>();
    let height = widgets
        .single_mut(world)
        .ok()
        .and_then(|mut widget| {
            widget
                .get_state_mut::<SidebarWindow>()
                .ok()
                .map(|w| w.last_height)
        })
        .unwrap_or(0);
    if height > 0 {
        usize::from(height)
    } else {
        FALLBACK_PAGE_ROWS
    }
}

/// Enter: folders switch the view; synthetic parents toggle collapse.
pub fn select(world: &mut World) {
    let Some(row) = selected_row(world) else {
        return;
    };
    match row.kind.clone() {
        RowKind::Folder(folder) => open_folder(world, row.account.clone(), folder),
        RowKind::Synthetic => toggle_collapse(world, &row),
        RowKind::AccountHeader => {}
    }
}

pub fn fold(world: &mut World, op: FoldOp) {
    match op {
        FoldOp::Toggle => {
            if let Some(row) = selected_row(world) {
                toggle_collapse(world, &row);
            }
        }
        FoldOp::CollapseAll => {
            let parents: Vec<(AccountId, String)> = world
                .resource::<SidebarRows>()
                .0
                .iter()
                .filter(|row| row.has_children)
                .map(|row| (row.account.clone(), row.path.clone()))
                .collect();
            world
                .resource_mut::<SidebarState>()
                .collapsed
                .extend(parents);
        }
        FoldOp::ExpandAll => world.resource_mut::<SidebarState>().collapsed.clear(),
    }
}

fn selected_row(world: &World) -> Option<SidebarRow> {
    let state = world.resource::<SidebarState>();
    world
        .resource::<SidebarRows>()
        .0
        .get(state.selected)
        .cloned()
}

fn toggle_collapse(world: &mut World, row: &SidebarRow) {
    if !row.has_children {
        return;
    }
    let key = (row.account.clone(), row.path.clone());
    let mut state = world.resource_mut::<SidebarState>();
    if !state.collapsed.remove(&key) {
        state.collapsed.insert(key);
    }
}

/// Switching abandons the outgoing folder's in-flight scan; the target
/// folder syncs lazily on first view (`request_sync` supersedes on
/// return visits).
fn open_folder(world: &mut World, account: AccountId, folder: FolderId) {
    cancel_outgoing_scan(world);
    if world.resource::<crate::pager::PagerState>().is_open() {
        crate::pager::dispatch(world, PagerOp::Close);
    }
    {
        let mut index_view = world.resource_mut::<IndexView>();
        index_view.account = Some(account.clone());
        index_view.folder = folder.clone();
        index_view.selected = None;
        index_view.selected_row = 0;
        index_view.top = 0;
        index_view.collapsed.clear();
        index_view.fold_epoch += 1;
    }
    *world.resource_mut::<Screen>() = Screen::Index;
    world.resource_mut::<SidebarState>().focused = false;
    sync_if_untracked(world, &account, &folder);
}

fn cancel_outgoing_scan(world: &mut World) {
    let index_view = world.resource::<IndexView>();
    let (Some(account), folder) = (index_view.account.clone(), index_view.folder.clone()) else {
        return;
    };
    let Some(job) = world
        .resource::<SyncTracker>()
        .in_flight_job(&account, &folder)
    else {
        return;
    };
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    if engine.0.send(&account, MailCommand::Cancel(job)).is_ok() {
        world.resource_mut::<SyncTracker>().fail(job);
    }
}

fn sync_if_untracked(world: &mut World, account: &AccountId, folder: &FolderId) {
    if world.resource::<SyncTracker>().is_tracked(account, folder) {
        return;
    }
    world.resource_scope(|world, mut tracker: Mut<SyncTracker>| {
        let Some(engine) = world.get_resource::<EngineResource>() else {
            return;
        };
        if let Err(error) = request_sync(&engine.0, &mut tracker, account, folder) {
            tracing::warn!("sync of {folder} on folder switch failed: {error}");
        }
    });
}

pub fn folder_create(world: &mut World, name: &str) {
    let Some(account) = target_account(world) else {
        return;
    };
    send_folder_command(
        world,
        &account,
        MailCommand::CreateFolder {
            name: name.to_owned(),
        },
    );
}

pub fn folder_rename(world: &mut World, new_name: &str) {
    let Some((account, folder)) = target_folder(world) else {
        return;
    };
    send_folder_command(
        world,
        &account,
        MailCommand::RenameFolder {
            folder,
            new_name: new_name.to_owned(),
        },
    );
}

pub fn folder_delete(world: &mut World) {
    let Some((account, folder)) = target_folder(world) else {
        return;
    };
    send_folder_command(world, &account, MailCommand::DeleteFolder { folder });
}

/// Folder commands act on the sidebar selection, falling back to the
/// viewed account for `create`.
fn target_account(world: &mut World) -> Option<AccountId> {
    selected_row(world)
        .map(|row| row.account)
        .or_else(|| world.resource::<IndexView>().account.clone())
}

fn target_folder(world: &mut World) -> Option<(AccountId, FolderId)> {
    match selected_row(world) {
        Some(SidebarRow {
            account,
            kind: RowKind::Folder(folder),
            ..
        }) => Some((account, folder)),
        _ => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world
                .resource_mut::<StatusMessage>()
                .warn("select a folder in the sidebar first".to_owned(), now);
            None
        }
    }
}

fn send_folder_command(world: &mut World, account: &AccountId, command: MailCommand) {
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    if let Err(error) = engine.0.send(account, command) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("folder command failed: {error}"), now);
    }
}
