//! Staged destructive dispatch: rows leave the store instantly, the
//! engine commands wait out an undo window. `z` cancels the newest
//! staged op and restores its rows; expiry dispatches for real; app
//! exit flushes — a staged op is never dropped.

use std::time::Duration;

use bevy::app::AppExit;
use bevy::prelude::*;
use nitidus_mail::{AccountId, EnvelopeSummary, FolderId, MailCommand};

use crate::engine::EngineResource;
use crate::status::StatusMessage;
use crate::store::MailStore;

pub const OP_DELAY: Duration = Duration::from_secs(5);

/// The undo window; a resource so tests can shrink it.
#[derive(Resource)]
pub struct OpDelay(pub Duration);

impl Default for OpDelay {
    fn default() -> Self {
        Self(OP_DELAY)
    }
}

pub struct StagedOp {
    account: AccountId,
    commands: Vec<MailCommand>,
    restore: Vec<(FolderId, EnvelopeSummary)>,
    due_at_secs: f64,
    notice: String,
}

/// Pending ops, oldest first; undo pops the newest (LIFO).
#[derive(Resource, Default)]
pub struct StagedOps(Vec<StagedOp>);

impl StagedOps {
    pub fn pending(&self) -> usize {
        self.0.len()
    }
}

pub struct StageRequest {
    pub account: AccountId,
    pub commands: Vec<MailCommand>,
    pub restore: Vec<(FolderId, EnvelopeSummary)>,
    /// e.g. "deleted 3" — the statusline appends "— z undoes".
    pub notice: String,
}

pub fn stage(world: &mut World, request: StageRequest) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    let delay = world.resource::<OpDelay>().0.as_secs_f64();
    let text = format!("{} — z undoes", request.notice);
    world.resource_mut::<StagedOps>().0.push(StagedOp {
        account: request.account,
        commands: request.commands,
        restore: request.restore,
        due_at_secs: now + delay,
        notice: request.notice,
    });
    world.resource_mut::<StatusMessage>().info(text, now);
}

/// `z` — cancel the newest staged op, restoring its rows. Returns
/// false when nothing is staged (the caller falls back to undo-send).
pub fn undo_last(world: &mut World) -> bool {
    let Some(op) = world.resource_mut::<StagedOps>().0.pop() else {
        return false;
    };
    let restored = op.restore.len();
    for (folder, envelope) in op.restore {
        world
            .resource_mut::<MailStore>()
            .restore_envelope(&op.account, &folder, envelope);
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    world
        .resource_mut::<StatusMessage>()
        .info(format!("undid {} ({restored} restored)", op.notice), now);
    true
}

/// Dispatches every op whose window has expired.
pub(super) fn tick_staged(
    time: Res<Time>,
    mut staged: ResMut<StagedOps>,
    engine: Option<Res<EngineResource>>,
) {
    if staged.as_ref().0.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if !staged.as_ref().0.iter().any(|op| op.due_at_secs <= now) {
        return;
    }
    let due: Vec<StagedOp> = {
        let ops = &mut staged.0;
        let mut kept = Vec::new();
        let mut expired = Vec::new();
        for op in ops.drain(..) {
            if op.due_at_secs <= now {
                expired.push(op);
            } else {
                kept.push(op);
            }
        }
        *ops = kept;
        expired
    };
    for op in due {
        dispatch(&engine, op);
    }
}

/// Exit must not lose staged work: dispatch everything immediately.
pub(super) fn flush_on_exit(
    mut exits: MessageReader<AppExit>,
    mut staged: ResMut<StagedOps>,
    engine: Option<Res<EngineResource>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    for op in staged.0.drain(..) {
        dispatch(&engine, op);
    }
}

fn dispatch(engine: &Option<Res<EngineResource>>, op: StagedOp) {
    let Some(engine) = engine else {
        tracing::warn!("no engine; dropping staged op: {}", op.notice);
        return;
    };
    for command in op.commands {
        if let Err(error) = engine.0.send(&op.account, command) {
            tracing::warn!("staged {} failed to dispatch: {error}", op.notice);
        }
    }
}
