//! Compose flow control: session start, the opening headers form,
//! review-screen operations, and body-preview scrolling.

use bevy::prelude::*;
use plurimus::Widget;

use super::render::{ComposeWidget, ComposeWindow};
use super::{ComposeSession, ComposeState, editor};
use crate::action::{ComposeOp, Motion};
use crate::addresses;
use crate::overlay::form::{FieldSpec, FormSpec, open_form};
use crate::status::MessageLog;

const FALLBACK_PAGE_ROWS: usize = 20;

/// `m` / `:compose`: a new session runs the prompt chain; an existing
/// one resumes at review.
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
            open_headers_form(world);
        }
        Err(error) => notice(world, format!("compose: {error:#}")),
    }
}

const TO_FIELD: &str = "to";
const SUBJECT_FIELD: &str = "subject";

/// Both opening headers on one surface. They used to be two prompts in
/// a chain, where Esc on the second threw away what you typed into the
/// first and there was no way back to it.
fn open_headers_form(world: &mut World) {
    let initial = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.to.clone())
        .unwrap_or_default();
    let spec = FormSpec::new(
        "New message",
        "Write",
        vec![
            address_field(world, TO_FIELD, "To").with_initial(initial),
            FieldSpec::text(SUBJECT_FIELD, "Subject"),
        ],
        Box::new(|world, values| {
            if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                session.to = values.get(TO_FIELD).to_owned();
                session.subject = values.get(SUBJECT_FIELD).to_owned();
            }
            edit_body(world);
        }),
    )
    .with_cancel(Box::new(abandon_new_session));
    open_form(world, spec);
}

/// A recipient field completing against the harvested address index.
pub(super) fn address_field(world: &mut World, id: &'static str, label: &str) -> FieldSpec {
    let candidates = addresses::snapshot_candidates(world);
    FieldSpec::text(id, label).completed(move |segment| addresses::rank(&candidates, segment))
}

/// Esc during the initial chain: nothing typed is worth keeping.
fn abandon_new_session(world: &mut World) {
    delete_session(world);
    notice(world, "compose abandoned");
}

/// `ui.compose.editor` decides which editor the body opens in;
/// `:compose-edit-external` bypasses this so the escape hatch never
/// depends on configuration.
pub(super) fn edit_body(world: &mut World) {
    let inline = world
        .get_resource::<crate::config::Config>()
        .is_none_or(|config| config.ui.compose.editor == crate::config::EditorKind::Inline);
    if inline {
        super::inline::open(world);
    } else {
        editor::edit_body(world);
    }
}

pub fn dispatch(world: &mut World, op: ComposeOp) {
    if !world.resource::<ComposeState>().is_active() {
        notice(world, "no message being composed (m starts one)");
        return;
    }
    match op {
        ComposeOp::EditBody => edit_body(world),
        ComposeOp::EditBodyExternal => editor::edit_body(world),
        ComposeOp::To => prompt_header(world, HeaderField::To),
        ComposeOp::Cc => prompt_header(world, HeaderField::Cc),
        ComposeOp::Bcc => prompt_header(world, HeaderField::Bcc),
        ComposeOp::Subject => prompt_header(world, HeaderField::Subject),
        ComposeOp::Send => super::drafts::send_with_checks(world),
        ComposeOp::Postpone => super::drafts::postpone(world),
        ComposeOp::Attach => super::drafts::attach_prompt(world),
        ComposeOp::Detach => super::drafts::detach_picker(world),
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
    fn title(self) -> &'static str {
        match self {
            Self::To => "To",
            Self::Cc => "Cc",
            Self::Bcc => "Bcc",
            Self::Subject => "Subject",
        }
    }

    fn is_address(self) -> bool {
        matches!(self, Self::To | Self::Cc | Self::Bcc)
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

const HEADER_FIELD: &str = "value";

fn prompt_header(world: &mut World, field: HeaderField) {
    let initial = world
        .resource::<ComposeState>()
        .session()
        .map(|session| field.get(session).to_owned())
        .unwrap_or_default();
    let spec = if field.is_address() {
        address_field(world, HEADER_FIELD, field.title())
    } else {
        FieldSpec::text(HEADER_FIELD, field.title())
    };
    open_form(
        world,
        FormSpec::new(
            field.title(),
            "Set",
            vec![spec.with_initial(initial)],
            Box::new(move |world, values| {
                if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
                    field.set(session, values.get(HEADER_FIELD).to_owned());
                }
            }),
        ),
    );
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

fn confirm_discard(world: &mut World) {
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
    world.resource_mut::<MessageLog>().info(text.into(), now);
}
