//! Compose flow control: starting a session, the operations its
//! commands dispatch to, and tearing it down again.

use bevy::prelude::*;

use super::{ComposeSession, ComposeState, editor};
use crate::action::ComposeOp;
use crate::addresses;
use crate::overlay::form::FieldSpec;
use crate::status::MessageLog;

/// `m` / `:compose`: a new session opens the composer; an existing one
/// is already open, and says so.
pub fn start_compose(world: &mut World) {
    start_compose_with(world, None);
}

/// The `:compose-to` bridge: a fresh composition with To prefilled.
pub fn start_compose_to(world: &mut World, to: String) {
    start_compose_with(world, Some(to));
}

fn start_compose_with(world: &mut World, to: Option<String>) {
    if world.resource::<ComposeState>().is_active() {
        notice(world, "resuming the staged message (Esc discards)");
        return;
    }
    let Some(account_config) = super::composing_account(world) else {
        notice(world, "no account configured to compose from");
        return;
    };
    let directory = match super::compose_directory(world) {
        Ok(directory) => directory,
        Err(error) => return notice(world, format!("compose: {error:#}")),
    };
    match ComposeSession::create(&account_config, &directory, "") {
        Ok(mut session) => {
            session.to = to.unwrap_or_default();
            world.resource_mut::<ComposeState>().0 = Some(session);
            super::form::open(world);
        }
        Err(error) => notice(world, format!("compose: {error:#}")),
    }
}

/// A recipient field completing against the harvested address index.
pub(super) fn address_field(world: &mut World, id: &'static str, label: &str) -> FieldSpec {
    let candidates = addresses::snapshot_candidates(world);
    FieldSpec::text(id, label).completed(move |segment| addresses::rank(&candidates, segment))
}

pub fn dispatch(world: &mut World, op: ComposeOp) {
    if !world.resource::<ComposeState>().is_active() {
        notice(world, "no message being composed (m starts one)");
        return;
    }
    super::form::pull_into_session(world);
    match op {
        ComposeOp::EditBodyExternal => editor::edit_body(world),
        ComposeOp::Send => super::drafts::send_with_checks(world),
        ComposeOp::Postpone => super::drafts::postpone(world),
        ComposeOp::Attach => super::drafts::attach_prompt(world),
        ComposeOp::AttachInsert => super::drafts::insert_selected(world),
        ComposeOp::Detach => super::drafts::detach_selected(world),
        ComposeOp::Discard => confirm_discard(world),
    }
}

/// The actual queue step, entered after the warning chain passes.
pub(super) fn queue_send(world: &mut World) {
    let built = {
        let compose = world.resource::<ComposeState>();
        let Some(session) = compose.session() else {
            return;
        };
        super::build::build(session, super::build::BuildMode::Send)
    };
    let built = match built {
        Ok(built) => built,
        Err(error) => return notice(world, format!("send: {error:#}")),
    };
    let session = match world.resource_mut::<ComposeState>().0.take() {
        Some(session) => session,
        None => return,
    };
    match crate::outbox::queue(world, &session, &built.envelope, &built.bytes) {
        Ok(()) => {
            super::form::dismiss(world);
            crate::addresses::harvest_recipients(world, &[&session.to, &session.cc, &session.bcc]);
            let seconds = world.resource::<crate::outbox::SendDelay>().0.as_secs();
            notice(world, format!("sending in {seconds}s — z undoes"));
        }
        Err(error) => {
            world.resource_mut::<ComposeState>().0 = Some(session);
            notice(world, format!("queue failed: {error:#}"));
        }
    }
}

pub(super) fn confirm_discard(world: &mut World) {
    let subject = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.subject.clone())
        .filter(|subject| !subject.trim().is_empty());
    crate::overlay::open_confirm(
        world,
        crate::overlay::ConfirmSpec::new(
            "Discard",
            "Discard this message?",
            "Discard",
            Box::new(|world| {
                delete_session(world);
                super::form::dismiss(world);
                notice(world, "message discarded");
            }),
        )
        .with_detail(subject.into_iter().collect()),
    );
}

fn delete_session(world: &mut World) {
    if let Some(session) = world.resource_mut::<ComposeState>().0.take() {
        super::persist::remove_sidecar(&session.body_path);
        if let Err(error) = std::fs::remove_file(&session.body_path) {
            tracing::warn!(
                "could not remove compose body {}: {error}",
                session.body_path.display()
            );
        }
    }
}

fn notice(world: &mut World, text: impl Into<String>) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<MessageLog>().info(text.into(), now);
}
