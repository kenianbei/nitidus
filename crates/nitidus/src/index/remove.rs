//! Message removal verbs: `d`/`:delete` (move to the account's trash;
//! permanent inside it, behind a confirm) and `:move <folder>` — the
//! general filing verb and the delete-recovery path. Removal is
//! optimistic, like flag writes: the store row goes immediately and
//! the next scan reconciles.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, FolderId, MailCommand};

use super::IndexView;
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
    if let Some(batch) = batch_targets(world) {
        let trash = FolderId::new(trash_folder(world, batch.account.as_str()));
        if batch.folder == trash {
            return confirm_batch_purge(world, batch);
        }
        return dispatch_batch(world, batch, Removal::Move(trash));
    }
    let Some(target) = current_target(world) else {
        return;
    };
    let trash = FolderId::new(trash_folder(world, target.account.as_str()));
    if target.folder == trash {
        return confirm_permanent(world, target);
    }
    dispatch(world, target, Removal::Move(trash));
}

/// `D`/`:delete-permanent` — the confirmed purge, from any folder.
pub fn delete_permanent_selected(world: &mut World) {
    if let Some(batch) = batch_targets(world) {
        return confirm_batch_purge(world, batch);
    }
    let Some(target) = current_target(world) else {
        return;
    };
    confirm_permanent(world, target);
}

pub fn move_selected(world: &mut World, destination: &str) {
    if let Some(batch) = batch_targets(world) {
        let destination = FolderId::new(destination);
        if !is_known_folder(world, &batch.account, &destination) {
            let now = world.resource::<Time>().elapsed_secs_f64();
            return world
                .resource_mut::<StatusMessage>()
                .warn(format!("unknown folder {destination}"), now);
        }
        return dispatch_batch(world, batch, Removal::Move(destination));
    }
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
    if !is_known_folder(world, &target.account, &destination) {
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

fn is_known_folder(world: &World, account: &AccountId, destination: &FolderId) -> bool {
    world
        .resource::<MailStore>()
        .folders(account)
        .iter()
        .any(|meta| meta.id == *destination)
}

/// The marked set with its view coordinates; `None` means no marks —
/// verbs fall back to the single selection.
struct BatchTarget {
    account: AccountId,
    folder: FolderId,
    ids: Vec<EnvelopeId>,
}

fn batch_targets(world: &World) -> Option<BatchTarget> {
    let ids = super::marks::batch_ids(world);
    if ids.is_empty() {
        return None;
    }
    let view = world.resource::<super::IndexView>();
    Some(BatchTarget {
        account: view.account.clone()?,
        folder: view.folder.clone(),
        ids,
    })
}

fn confirm_batch_purge(world: &mut World, batch: BatchTarget) {
    let request = PromptRequest::new(
        format!("Delete {} permanently? (y/n): ", batch.ids.len()),
        Box::new(move |world, answer| {
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                dispatch_batch(world, batch, Removal::Purge);
            }
        }),
    );
    open_prompt(world, request);
}

/// One staged op for the whole set: rows out instantly, one `z`
/// undoes them all, marks consumed.
fn dispatch_batch(world: &mut World, batch: BatchTarget, removal: Removal) {
    let mut commands = Vec::new();
    let mut restore = Vec::new();
    for id in &batch.ids {
        let removed = world
            .resource::<MailStore>()
            .envelopes(&batch.account, &batch.folder)
            .iter()
            .find(|envelope| &envelope.id == id)
            .cloned();
        let Some(envelope) = removed else { continue };
        world
            .resource_mut::<MailStore>()
            .remove_envelope(&batch.account, &batch.folder, id);
        commands.push(match &removal {
            Removal::Move(destination) => MailCommand::MoveMessage {
                folder: batch.folder.clone(),
                id: id.clone(),
                target: destination.clone(),
            },
            Removal::Purge => MailCommand::DeleteMessage {
                folder: batch.folder.clone(),
                id: id.clone(),
            },
        });
        restore.push((batch.folder.clone(), envelope));
    }
    {
        let mut view = world.resource_mut::<super::IndexView>();
        view.selected = None;
        view.marked.clear();
        view.visual_anchor = None;
    }
    let count = commands.len();
    let notice = match &removal {
        Removal::Move(destination) => format!("moved {count} to {destination}"),
        Removal::Purge => format!("deleted {count} permanently"),
    };
    super::staged::stage(
        world,
        super::staged::StageRequest {
            account: batch.account,
            commands,
            restore,
            notice,
        },
    );
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
/// Optimistic removal, then a *staged* backend command: the row is out
/// of the view instantly, the engine waits out the undo window.
fn dispatch(world: &mut World, target: RemovalTarget, removal: Removal) {
    let removed = world
        .resource::<MailStore>()
        .envelopes(&target.account, &target.folder)
        .iter()
        .find(|envelope| envelope.id == target.id)
        .cloned();
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
    let (command, notice) = match removal {
        Removal::Move(destination) => (
            MailCommand::MoveMessage {
                folder: target.folder.clone(),
                id: target.id,
                target: destination.clone(),
            },
            format!("moved to {destination}"),
        ),
        Removal::Purge => (
            MailCommand::DeleteMessage {
                folder: target.folder.clone(),
                id: target.id,
            },
            "deleted permanently".to_owned(),
        ),
    };
    super::staged::stage(
        world,
        super::staged::StageRequest {
            account: target.account,
            commands: vec![command],
            restore: removed
                .map(|envelope| (target.folder, envelope))
                .into_iter()
                .collect(),
            notice,
        },
    );
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
