//! Everything the body understands that is not plain typing: motions,
//! undo, selection, the clipboard. Typing reaches the buffer through the
//! field itself; these arrive as `EditorOp`s so they stay rebindable and
//! keep showing up in the help overlay.

use bevy::prelude::*;
use ratatui_textarea::{CursorMove, Scrolling, TextArea};

use crate::action::{EditorMotion, EditorOp};
use crate::overlay::form::body::lock;
use crate::status::MessageLog;

/// The buffer these commands act on: whichever body field has focus.
///
/// Editing through the shared handle bypasses the resource that owns it,
/// so each edit says out loud that the form changed — that tick is what
/// the renderer reclassifies lines from.
fn with_area<R>(world: &mut World, edit: impl FnOnce(&mut TextArea<'static>) -> R) -> Option<R> {
    let area = crate::overlay::form::focused_body(world)?;
    let outcome = edit(&mut lock(&area));
    crate::overlay::form::touch(world);
    Some(outcome)
}

pub fn dispatch(world: &mut World, op: EditorOp) {
    if matches!(op, EditorOp::Copy | EditorOp::Cut) {
        return copy_or_cut(world, op);
    }
    if op == EditorOp::Paste {
        return paste(world);
    }
    if op == EditorOp::Preview {
        return super::preview::open(world);
    }
    with_area(world, |area| match op {
        EditorOp::Move(motion) => apply_motion(area, motion),
        EditorOp::Undo => {
            area.undo();
        }
        EditorOp::Redo => {
            area.redo();
        }
        EditorOp::SelectToggle => toggle_selection(area),
        EditorOp::SelectAll => area.select_all(),
        EditorOp::DeleteWordBack => {
            area.delete_word();
        }
        EditorOp::DeleteWordForward => {
            area.delete_next_word();
        }
        EditorOp::DeleteLineEnd => {
            area.delete_line_by_end();
        }
        EditorOp::Newline => area.insert_newline(),
        EditorOp::Cut | EditorOp::Copy | EditorOp::Paste | EditorOp::Preview => {}
    });
}

/// The attachment token on the cursor's line, if that line is one.
pub(super) fn token_at_cursor(world: &World) -> Option<super::token::AttachToken> {
    let area = crate::overlay::form::focused_body(world)?;
    let area = lock(&area);
    let row = area.cursor().0;
    super::token::parse(area.lines().get(row)?)
}

/// Puts `text` on a line of its own at the cursor of the named body,
/// focused or not. Returns whether there was a body to take it.
pub(super) fn insert_line_into(world: &mut World, id: &str, text: &str) -> bool {
    let Some(area) = crate::overlay::form::body_field(world, id) else {
        return false;
    };
    {
        let mut area = lock(&area);
        area.move_cursor(CursorMove::End);
        area.insert_newline();
        area.insert_str(text);
    }
    crate::overlay::form::touch(world);
    true
}

/// Drops the token line naming `path` from the live buffer. Returns
/// whether there was a body to edit.
pub(super) fn remove_token_line(world: &mut World, path: &std::path::Path) -> bool {
    with_area(world, |area| {
        let kept = super::token::remove(area.lines(), path);
        replace_all(area, kept);
    })
    .is_some()
}

/// Undo history is preserved: the buffer is rewritten through the normal
/// editing API rather than rebuilt, so the change can be undone.
fn replace_all(area: &mut TextArea<'static>, lines: Vec<String>) {
    area.select_all();
    area.cut();
    area.insert_str(lines.join("\n"));
}

fn toggle_selection(area: &mut TextArea<'static>) {
    if area.is_selecting() {
        area.cancel_selection();
    } else {
        area.start_selection();
    }
}

fn apply_motion(area: &mut TextArea<'static>, motion: EditorMotion) {
    match motion {
        EditorMotion::PageUp => area.scroll(Scrolling::PageUp),
        EditorMotion::PageDown => area.scroll(Scrolling::PageDown),
        other => area.move_cursor(cursor_move(other)),
    }
}

fn cursor_move(motion: EditorMotion) -> CursorMove {
    match motion {
        EditorMotion::Left => CursorMove::Back,
        EditorMotion::Right => CursorMove::Forward,
        EditorMotion::Up => CursorMove::Up,
        EditorMotion::Down => CursorMove::Down,
        EditorMotion::WordForward => CursorMove::WordForward,
        EditorMotion::WordBack => CursorMove::WordBack,
        EditorMotion::LineStart => CursorMove::Head,
        EditorMotion::LineEnd => CursorMove::End,
        EditorMotion::ParagraphForward => CursorMove::ParagraphForward,
        EditorMotion::ParagraphBack => CursorMove::ParagraphBack,
        EditorMotion::Top => CursorMove::Top,
        EditorMotion::Bottom => CursorMove::Bottom,
        // Handled by `apply_motion`; scrolling is not a cursor move.
        EditorMotion::PageUp | EditorMotion::PageDown => CursorMove::InViewport,
    }
}

/// Copy and cut go through the system clipboard as well as the widget's
/// own yank buffer, so text crosses the process boundary.
fn copy_or_cut(world: &mut World, op: EditorOp) {
    let yanked = with_area(world, |area| {
        if op == EditorOp::Cut {
            area.cut();
        } else {
            area.copy();
        }
        area.yank_text()
    })
    .unwrap_or_default();
    if yanked.is_empty() {
        return;
    }
    if let Err(error) = crate::clipboard::set(&yanked) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .warn(format!("clipboard: {error}"), now);
    }
}

/// Prefers the system clipboard, falling back to the widget's yank buffer
/// when no clipboard is reachable (a bare tty, a locked-down session).
fn paste(world: &mut World) {
    let external = crate::clipboard::get();
    with_area(world, |area| {
        if let Some(text) = external.filter(|text| !text.is_empty()) {
            area.set_yank_text(text);
        }
        area.paste();
    });
}
