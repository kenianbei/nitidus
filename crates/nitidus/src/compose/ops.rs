//! Compose flow control: session start, the To → Subject prompt chain,
//! review-screen operations, and body-preview scrolling.

use bevy::prelude::*;
use plurimus::Widget;

use super::render::{ComposeWidget, ComposeWindow};
use super::{ComposeSession, ComposeState, editor};
use crate::action::{ComposeOp, Motion};
use crate::prompt::{PromptRequest, open_prompt};
use crate::screen::Screen;
use crate::status::StatusMessage;

const FALLBACK_PAGE_ROWS: usize = 20;

/// `m` / `:compose`: a new session runs the prompt chain; an existing
/// one resumes at review.
pub fn start_compose(world: &mut World) {
    if world.resource::<ComposeState>().is_active() {
        *world.resource_mut::<Screen>() = Screen::Compose;
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
    match ComposeSession::create(&account_config, &directory) {
        Ok(session) => {
            world.resource_mut::<ComposeState>().0 = Some(session);
            prompt_to(world);
        }
        Err(error) => notice(world, format!("compose: {error:#}")),
    }
}

fn prompt_to(world: &mut World) {
    let request = PromptRequest::new(
        "To: ",
        Box::new(|world, value| {
            if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                session.to = value;
            }
            prompt_initial_subject(world);
        }),
    )
    .with_cancel(Box::new(abandon_new_session));
    open_prompt(world, request);
}

fn prompt_initial_subject(world: &mut World) {
    let request = PromptRequest::new(
        "Subject: ",
        Box::new(|world, value| {
            if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                session.subject = value;
            }
            editor::edit_body(world);
        }),
    )
    .with_cancel(Box::new(abandon_new_session));
    open_prompt(world, request);
}

/// Esc during the initial chain: nothing typed is worth keeping.
fn abandon_new_session(world: &mut World) {
    delete_session(world);
    notice(world, "compose abandoned");
}

pub fn dispatch(world: &mut World, op: ComposeOp) {
    if !world.resource::<ComposeState>().is_active() {
        notice(world, "no message being composed (m starts one)");
        return;
    }
    match op {
        ComposeOp::EditBody => editor::edit_body(world),
        ComposeOp::To => prompt_header(world, HeaderField::To),
        ComposeOp::Cc => prompt_header(world, HeaderField::Cc),
        ComposeOp::Bcc => prompt_header(world, HeaderField::Bcc),
        ComposeOp::Subject => prompt_header(world, HeaderField::Subject),
        ComposeOp::Send => send(world),
        ComposeOp::Postpone => notice(world, "postpone lands with 1c.17 — message kept"),
        ComposeOp::Discard => confirm_discard(world),
    }
}

#[derive(Clone, Copy)]
enum HeaderField {
    To,
    Cc,
    Bcc,
    Subject,
}

impl HeaderField {
    fn label(self) -> &'static str {
        match self {
            Self::To => "To: ",
            Self::Cc => "Cc: ",
            Self::Bcc => "Bcc: ",
            Self::Subject => "Subject: ",
        }
    }

    fn get(self, session: &ComposeSession) -> &str {
        match self {
            Self::To => &session.to,
            Self::Cc => &session.cc,
            Self::Bcc => &session.bcc,
            Self::Subject => &session.subject,
        }
    }

    fn set(self, session: &mut ComposeSession, value: String) {
        match self {
            Self::To => session.to = value,
            Self::Cc => session.cc = value,
            Self::Bcc => session.bcc = value,
            Self::Subject => session.subject = value,
        }
    }
}

fn prompt_header(world: &mut World, field: HeaderField) {
    let initial = world
        .resource::<ComposeState>()
        .session()
        .map(|session| field.get(session).to_owned())
        .unwrap_or_default();
    let request = PromptRequest::new(
        field.label(),
        Box::new(move |world, value| {
            if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                field.set(session, value);
            }
        }),
    )
    .with_initial(initial);
    open_prompt(world, request);
}

/// `y`: build, queue with the undo window, and drop back to the index
/// — the session dissolves into the outbox entry until sent or undone.
fn send(world: &mut World) {
    let built = {
        let compose = world.resource::<ComposeState>();
        let Some(session) = compose.session() else {
            return;
        };
        super::build::build(session)
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
            *world.resource_mut::<Screen>() = Screen::Index;
            let seconds = world.resource::<crate::outbox::SendDelay>().0.as_secs();
            notice(world, format!("sending in {seconds}s — z undoes"));
        }
        Err(error) => {
            world.resource_mut::<ComposeState>().0 = Some(session);
            notice(world, format!("queue failed: {error:#}"));
        }
    }
}

fn confirm_discard(world: &mut World) {
    let request = PromptRequest::new(
        "Discard message? (y/n): ",
        Box::new(|world, answer| {
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                delete_session(world);
                notice(world, "message discarded");
            }
        }),
    );
    open_prompt(world, request);
}

fn delete_session(world: &mut World) {
    if let Some(session) = world.resource_mut::<ComposeState>().0.take()
        && let Err(error) = std::fs::remove_file(&session.body_path)
    {
        tracing::warn!(
            "could not remove compose body {}: {error}",
            session.body_path.display()
        );
    }
    *world.resource_mut::<Screen>() = Screen::Index;
}

pub fn scroll(world: &mut World, motion: Motion) {
    let mut widgets = world.query_filtered::<&mut Widget, With<ComposeWidget>>();
    let Ok(mut widget) = widgets.single_mut(world) else {
        return;
    };
    let Ok(window) = widget.get_state_mut::<ComposeWindow>() else {
        return;
    };
    let height = usize::from(window.viewport_rows());
    let page = if height > 1 {
        height - 1
    } else {
        FALLBACK_PAGE_ROWS
    };
    let max_scroll = window.line_count().saturating_sub(height.max(1));
    window.scroll = match motion {
        Motion::Next => (window.scroll + 1).min(max_scroll),
        Motion::Prev => window.scroll.saturating_sub(1),
        Motion::NextPage => (window.scroll + page).min(max_scroll),
        Motion::PrevPage => window.scroll.saturating_sub(page),
        Motion::First => 0,
        Motion::Last => max_scroll,
        Motion::Parent => window.scroll,
    };
}

fn notice(world: &mut World, text: impl Into<String>) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world.resource_mut::<StatusMessage>().info(text.into(), now);
}
