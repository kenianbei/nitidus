//! The composer's surface: one form, open for as long as the session
//! is, drawn in the reading column beside a reply and over the panes
//! otherwise.
//!
//! The form holds the live answers and `ComposeSession` holds the
//! durable ones; `pull_into_session` runs them together on every change,
//! so persistence, postpone and the send pipeline go on reading one
//! shape.

use std::sync::Arc;

use bevy::prelude::*;
use nitidus_ui_kit::{layer, layout};
use plurimus::LayoutFn;

use super::{ComposeSession, ComposeState};
use crate::config::Config;
use crate::keymap::CONTEXT_COMPOSE;
use crate::overlay::form::{
    ActiveForm, CancelOutcome, FieldSpec, FormPlacement, FormSpec, close, open_form,
};
use crate::panes::{MailPane, mail_layout};

pub(super) const FROM_FIELD: &str = "from";
pub(super) const TO_FIELD: &str = "to";
pub(super) const CC_FIELD: &str = "cc";
pub(super) const BCC_FIELD: &str = "bcc";
pub(super) const SUBJECT_FIELD: &str = "subject";
pub(super) const ATTACH_FIELD: &str = "attachments";
pub(super) const BODY_FIELD: &str = "body";

const NEW_TITLE: &str = "New message";
const REPLY_TITLE: &str = "Reply";
const SEND_LABEL: &str = "Send";
const DISCARD_LABEL: &str = "Discard";
const ADD_ATTACHMENT_LABEL: &str = "Add attachment";
/// Until the first placement reads the real one out of config.
const DEFAULT_MAX_WIDTH: u16 = 100;

/// Opens the composer on a session restored from elsewhere — an undone
/// send, a recalled draft, a recovered crash.
pub(crate) fn reopen_restored(world: &mut World) {
    open(world);
}

/// Opens the composer on the staged session. A reply lands in the body,
/// where there is already a quote to answer; a new message lands in To,
/// which is the first thing it needs.
pub(super) fn open(world: &mut World) {
    let Some((is_reply, fields)) = describe(world) else {
        return;
    };
    let landing = if is_reply { BODY_FIELD } else { TO_FIELD };
    let title = if is_reply { REPLY_TITLE } else { NEW_TITLE };
    let spec = FormSpec::new(
        title,
        SEND_LABEL,
        fields,
        Box::new(|world, _| super::drafts::send_with_checks(world)),
    )
    .cancel_label(DISCARD_LABEL)
    .stepping_enter()
    .in_context(CONTEXT_COMPOSE)
    .focusing(landing)
    .placed(placement(world))
    .with_cancel(|world| {
        super::ops::confirm_discard(world);
        CancelOutcome::Keep
    });
    open_form(world, spec);
}

/// A reply belongs beside the message it answers, so it takes the
/// reading column and leaves the index where it was. A new message has
/// nothing to sit beside and opens over the panes — as does a reply when
/// the column is too narrow to write in.
///
/// The choice lives inside the layout closure so a resize re-decides it
/// without anything having to watch the terminal.
fn placement(world: &World) -> FormPlacement {
    let sidebar_visible = world
        .get_resource::<crate::sidebar::SidebarState>()
        .is_none_or(|sidebar| sidebar.visible);
    let max_width = world
        .get_resource::<Config>()
        .map_or(DEFAULT_MAX_WIDTH, |config| config.ui.pager.max_width);
    let beside_a_message = world
        .resource::<ComposeState>()
        .session()
        .is_some_and(|session| session.reply_source.is_some());
    FormPlacement::Host {
        layout: compose_layout(sidebar_visible, beside_a_message, max_width),
        order: layer::ZOOM,
    }
}

fn compose_layout(sidebar_visible: bool, beside_a_message: bool, max_width: u16) -> LayoutFn {
    let column = mail_layout(MailPane::Reading, sidebar_visible);
    Arc::new(move |area| {
        let pane = column(area);
        if beside_a_message && pane.width >= crate::panes::MIN_PANE_WIDTH {
            return pane;
        }
        layout::centered_capped(*area, max_width, 1)
    })
}

/// The fields, seeded from the session — a recalled draft or a reply
/// arrives with most of them already answered.
fn describe(world: &mut World) -> Option<(bool, Vec<FieldSpec>)> {
    let seed = {
        let compose = world.resource::<ComposeState>();
        let session = compose.session()?;
        (
            session.reply_source.is_some(),
            session.from.clone(),
            session.to.clone(),
            session.cc.clone(),
            session.bcc.clone(),
            session.subject.clone(),
            session
                .attachments
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            session.body.join("\n"),
        )
    };
    let (is_reply, from, to, cc, bcc, subject, attachments, body) = seed;
    let fields = vec![
        FieldSpec::text(FROM_FIELD, "From")
            .read_only()
            .with_initial(from),
        super::ops::address_field(world, TO_FIELD, "To").with_initial(to),
        super::ops::address_field(world, CC_FIELD, "Cc").with_initial(cc),
        super::ops::address_field(world, BCC_FIELD, "Bcc").with_initial(bcc),
        FieldSpec::text(SUBJECT_FIELD, "Subject").with_initial(subject),
        FieldSpec::entries(ATTACH_FIELD, "Attach", ADD_ATTACHMENT_LABEL)
            .with_initial(attachments)
            .activated(super::drafts::activate_attachment),
        FieldSpec::body(BODY_FIELD, "Body")
            .with_initial(body)
            .line_styled(|lines, theme| {
                crate::pager::body::classify_lines(lines)
                    .into_iter()
                    .map(|kind| super::style::line_style(kind, theme))
                    .collect()
            }),
    ];
    Some((is_reply, fields))
}

/// Whether the composer's own form is the one on screen. Another form —
/// the account wizard, say — can be open with no session behind it.
pub(super) fn is_open(world: &World) -> bool {
    world
        .get_resource::<ActiveForm>()
        .and_then(ActiveForm::context)
        == Some(CONTEXT_COMPOSE)
}

/// Copies the form's answers into the session. Runs every frame the form
/// changes, and again before anything that reads the session — a command
/// firing in the same frame as a keystroke must not act on stale text.
pub(super) fn pull_into_session(world: &mut World) {
    if !is_open(world) {
        return;
    }
    let Some(values) = world.get_resource::<ActiveForm>().map(|form| {
        [
            form.value(TO_FIELD).unwrap_or_default(),
            form.value(CC_FIELD).unwrap_or_default(),
            form.value(BCC_FIELD).unwrap_or_default(),
            form.value(SUBJECT_FIELD).unwrap_or_default(),
            form.value(ATTACH_FIELD).unwrap_or_default(),
            form.value(BODY_FIELD).unwrap_or_default(),
        ]
    }) else {
        return;
    };
    let [to, cc, bcc, subject, attachments, body] = values;
    let attachments: Vec<std::path::PathBuf> = attachments
        .lines()
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();
    let body = body_lines(&body);
    let body_changed = {
        let mut compose = world.resource_mut::<ComposeState>();
        let Some(session) = compose.0.as_mut() else {
            return;
        };
        if !changed(session, &to, &cc, &bcc, &subject, &body) && session.attachments == attachments
        {
            return;
        }
        let body_changed = session.body != body;
        session.to = to;
        session.cc = cc;
        session.bcc = bcc;
        session.subject = subject;
        session.attachments = attachments;
        session.body = body;
        body_changed
    };
    // Only the body has a file of its own; the headers ride the sidecar.
    if body_changed {
        world.resource_mut::<BodyFile>().dirty = true;
        write_due_body(world);
    }
}

/// A body of one empty line is an empty body: the field always holds a
/// line for the caret to sit on, while a session with nothing in it
/// holds no lines at all. Without this they differ forever and every
/// frame looks like a change.
fn body_lines(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    value.split('\n').map(str::to_owned).collect()
}

/// Reads go through `Deref`, so only a real change ticks the resource —
/// which would otherwise mark the session dirty on every frame.
fn changed(
    session: &ComposeSession,
    to: &str,
    cc: &str,
    bcc: &str,
    subject: &str,
    body: &[String],
) -> bool {
    session.to != to
        || session.cc != cc
        || session.bcc != bcc
        || session.subject != subject
        || session.body != body
}

/// How often the body reaches disk while it is being typed into. The
/// buffer is the truth; the file is the copy a crash leaves behind, and
/// rewriting it on every keystroke is a syscall per character.
const BODY_WRITE_INTERVAL_SECS: f64 = 0.25;

/// Tracks what the body file still owes the buffer.
#[derive(Resource)]
pub(super) struct BodyFile {
    dirty: bool,
    last_write_secs: f64,
}

impl Default for BodyFile {
    fn default() -> Self {
        Self {
            dirty: false,
            // The first change writes at once; the interval only ever
            // holds back the ones behind it.
            last_write_secs: f64::NEG_INFINITY,
        }
    }
}

/// Writes the body file when it is owed one and the interval has
/// passed. Runs every frame; the flush points call `flush_body`.
pub(super) fn write_due_body(world: &mut World) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    let file = world.resource::<BodyFile>();
    if !file.dirty || now - file.last_write_secs < BODY_WRITE_INTERVAL_SECS {
        return;
    }
    flush_body(world);
}

/// Writes the body file now, whatever the interval says. Anything that
/// reads the file rather than the buffer — send, postpone, `$EDITOR` —
/// goes through here first.
pub(crate) fn flush_body(world: &mut World) {
    if !world.resource::<BodyFile>().dirty {
        return;
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    {
        let mut file = world.resource_mut::<BodyFile>();
        file.dirty = false;
        file.last_write_secs = now;
    }
    write_body(world);
}

/// The body file is the crash-survival artifact; a failed write is worth
/// saying out loud, but must not cost the buffer.
fn write_body(world: &mut World) {
    let outcome = world
        .resource::<ComposeState>()
        .session()
        .map(ComposeSession::write_body);
    if let Some(Err(error)) = outcome {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<crate::status::MessageLog>()
            .warn(format!("could not save the body: {error}"), now);
    }
}

/// Rebuilds the form from the session — after `$EDITOR` has rewritten
/// the body underneath it.
pub(super) fn reopen(world: &mut World) {
    if !is_open(world) {
        return;
    }
    close(world);
    open(world);
}

/// Closes the composer's form, leaving any other surface alone.
pub(super) fn dismiss(world: &mut World) {
    if is_open(world) {
        close(world);
    }
}

/// The form and the session run together on every change. Exclusive
/// because the pull touches both resources and the body file.
pub(super) fn sync_session(world: &mut World) {
    if world.is_resource_changed::<ActiveForm>() {
        pull_into_session(world);
    }
}
