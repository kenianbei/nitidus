//! Peek: `[ui.pager] mark_read` defers SEEN. Opening arms a timer for
//! the opened message; the tick fires it only while that message is
//! still open; closing or switching messages disarms or re-arms.

use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeId, FolderId};

use super::PagerState;
use crate::config::{Config, MarkRead};

#[derive(Resource, Default)]
pub struct PeekTimer(Option<ArmedPeek>);

struct ArmedPeek {
    account: AccountId,
    folder: FolderId,
    id: EnvelopeId,
    due_secs: f64,
}

pub(super) struct PeekTarget {
    pub account: AccountId,
    pub folder: FolderId,
    pub id: EnvelopeId,
}

/// Runs on every pager open; a re-open for another message replaces
/// any armed timer.
pub(super) fn arm(world: &mut World, target: PeekTarget) {
    match world.resource::<Config>().ui.pager.mark_read {
        MarkRead::Open => {
            crate::index::mark_seen(world, &target.account, &target.folder, &target.id);
            world.resource_mut::<PeekTimer>().0 = None;
        }
        MarkRead::Never => world.resource_mut::<PeekTimer>().0 = None,
        MarkRead::After(delay) => {
            let now = world.resource::<Time>().elapsed_secs_f64();
            world.resource_mut::<PeekTimer>().0 = Some(ArmedPeek {
                account: target.account,
                folder: target.folder,
                id: target.id,
                due_secs: now + delay.as_secs_f64(),
            });
        }
    }
}

pub(super) fn disarm(world: &mut World) {
    world.resource_mut::<PeekTimer>().0 = None;
}

/// Exclusive: the fire path writes the store and the engine.
pub(super) fn tick_peek(world: &mut World) {
    let is_due = {
        let Some(armed) = &world.resource::<PeekTimer>().0 else {
            return;
        };
        armed.due_secs <= world.resource::<Time>().elapsed_secs_f64()
    };
    if !is_due {
        return;
    }
    let Some(armed) = world.resource_mut::<PeekTimer>().0.take() else {
        return;
    };
    let still_open = world.resource::<PagerState>().open_id() == Some(&armed.id);
    if !still_open {
        return;
    }
    crate::index::mark_seen(world, &armed.account, &armed.folder, &armed.id);
}
