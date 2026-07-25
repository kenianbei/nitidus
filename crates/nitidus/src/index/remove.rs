//! Message removal verbs: `d`/`:delete` (move to the account's trash;
//! permanent inside it, behind a confirm) and `:move <folder>` — the
//! general filing verb and the delete-recovery path. Removal is
//! optimistic, like flag writes: the store row goes immediately and
//! the next scan reconciles.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, FolderId, MailCommand};

use super::IndexView;
use crate::engine::EngineResource;
use crate::pager::PagerState;
use crate::prompt::{PromptRequest, open_prompt};
use crate::status::StatusMessage;
use crate::store::MailStore;

#[derive(Clone)]
struct RemovalTarget {
    account: AccountId,
    folder: FolderId,
    id: EnvelopeId,
    was_in_pager: bool,
}

pub fn delete_selected(world: &mut World) {
    let Some(target) = current_target(world) else {
        return;
    };
    let trash = FolderId::new(trash_folder(world, target.account.as_str()));
    if target.folder == trash {
        return confirm_permanent(world, target);
    }
    dispatch(world, target, Removal::Move(trash));
}

pub fn move_selected(world: &mut World, destination: &str) {
    let Some(target) = current_target(world) else {
        return;
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let destination = FolderId::new(destination);
    if destination == target.folder {
        return world
            .resource_mut::<StatusMessage>()
            .info("message is already there".to_owned(), now);
    }
    let is_known = world
        .resource::<MailStore>()
        .folders(&target.account)
        .iter()
        .any(|meta| meta.id == destination);
    if !is_known {
        return world
            .resource_mut::<StatusMessage>()
            .warn(format!("unknown folder {destination}"), now);
    }
    dispatch(world, target, Removal::Move(destination));
}

enum Removal {
    Move(FolderId),
    Purge,
}

fn confirm_permanent(world: &mut World, target: RemovalTarget) {
    let request = PromptRequest::new(
        "Delete permanently? (y/n): ",
        Box::new(move |world, answer| {
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                dispatch(world, target, Removal::Purge);
            }
        }),
    );
    open_prompt(world, request);
}

/// Optimistic removal from the view, then the backend command; a
/// `JobFailed` surfaces on the statusline and the next scan restores
/// the truth either way.
fn dispatch(world: &mut World, target: RemovalTarget, removal: Removal) {
    world
        .resource_mut::<MailStore>()
        .remove_envelope(&target.account, &target.folder, &target.id);
    {
        let mut view = world.resource_mut::<IndexView>();
        if view.selected == Some(target.id.clone()) {
            view.selected = None;
        }
    }
    if target.was_in_pager {
        crate::pager::ops::close(world);
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    let (command, notice) = match removal {
        Removal::Move(destination) => (
            MailCommand::MoveMessage {
                folder: target.folder,
                id: target.id,
                target: destination.clone(),
            },
            format!("moved to {destination}"),
        ),
        Removal::Purge => (
            MailCommand::DeleteMessage {
                folder: target.folder,
                id: target.id,
            },
            "deleted permanently".to_owned(),
        ),
    };
    let Some(engine) = world.get_resource::<EngineResource>() else {
        return;
    };
    match engine.0.send(&target.account, command) {
        Ok(()) => world.resource_mut::<StatusMessage>().info(notice, now),
        Err(error) => world
            .resource_mut::<StatusMessage>()
            .warn(format!("remove failed: {error}"), now),
    }
}

/// The pager's open message when it is open, else the index selection.
fn current_target(world: &mut World) -> Option<RemovalTarget> {
    if let Some(open) = world.resource::<PagerState>().open_message() {
        return Some(RemovalTarget {
            account: open.account.clone(),
            folder: open.folder.clone(),
            id: open.id.clone(),
            was_in_pager: true,
        });
    }
    let view = world.resource::<IndexView>();
    Some(RemovalTarget {
        account: view.account.clone()?,
        folder: view.folder.clone(),
        id: view.selected.clone()?,
        was_in_pager: false,
    })
}

fn trash_folder(world: &World, account: &str) -> String {
    world
        .resource::<crate::config::Config>()
        .accounts
        .iter()
        .find(|candidate| candidate.name == account)
        .map(|config| config.folders.trash.clone())
        .unwrap_or_else(|| "Trash".to_owned())
}
