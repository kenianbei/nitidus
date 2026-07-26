//! The inline body editor. A `TextArea` owns the body while
//! `InputMode::Editor` is active; the router still owns the keyboard, so
//! printable keys are typed into the buffer and everything else resolves
//! through the rebindable `editor` context.

use std::sync::{Arc, Mutex, MutexGuard};

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyEvent, KeyModifiers};
use crokey::KeyCombination;
use ratatui_textarea::{CursorMove, Scrolling, TextArea};

use super::{ComposeStage, ComposeState};
use crate::action::{EditorMotion, EditorOp, apply_action};
use crate::keymap::{CONTEXT_EDITOR, InputMode, KeymapMatch, Keymaps, Mode};
use crate::screen::Screen;
use crate::status::StatusMessage;

/// `TextArea` caches its screen map in `RefCell`s, so it is `Send` but not
/// `Sync` — it cannot be a bare bevy resource or widget state. Sharing one
/// behind a mutex satisfies both, and lets the renderer borrow the live
/// editor instead of deep-copying the buffer every frame.
pub(crate) type SharedArea = Arc<Mutex<TextArea<'static>>>;

/// The live editor, present only while editing. Holding the `TextArea`
/// here rather than in `ComposeSession` keeps the session plain data that
/// persistence and the send pipeline can serialize.
#[derive(Resource, Default)]
pub struct InlineEditor(Option<SharedArea>);

impl InlineEditor {
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    /// A handle on the live editor, for the renderer.
    pub(crate) fn shared(&self) -> Option<SharedArea> {
        self.0.clone()
    }

    /// The body as it stands in the buffer, without ending the edit.
    pub fn lines(&self) -> Option<Vec<String>> {
        self.0.as_ref().map(|area| lock(area).lines().to_vec())
    }

    /// Takes `&mut self` even though the mutex would allow shared access:
    /// going through `resource_mut` is what ticks bevy's change detection,
    /// and the renderer reclassifies lines off that tick.
    fn with_mut<R>(&mut self, edit: impl FnOnce(&mut TextArea<'static>) -> R) -> Option<R> {
        self.0.as_ref().map(|area| edit(&mut lock(area)))
    }
}

/// A panicking editor operation would poison the lock; the buffer is
/// still the user's text, so recover it rather than lose the message.
pub(crate) fn lock(area: &SharedArea) -> MutexGuard<'_, TextArea<'static>> {
    area.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Enters the editor with the session body loaded.
pub(super) fn open(world: &mut World) {
    let Some(body) = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.body.clone())
    else {
        return;
    };
    let mut area = TextArea::new(body);
    super::style::apply(world, &mut area);
    world.resource_mut::<InlineEditor>().0 = Some(Arc::new(Mutex::new(area)));
    if let Some(session) = world.resource_mut::<ComposeState>().0.as_mut() {
        session.stage = ComposeStage::Editing;
    }
    *world.resource_mut::<Screen>() = Screen::Compose;
    world.resource_mut::<Mode>().0 = InputMode::Editor;
}

/// Leaves the editor, writing the buffer back to the session and its
/// crash-survival file.
pub(super) fn close(world: &mut World) {
    let Some(area) = world.resource_mut::<InlineEditor>().0.take() else {
        return;
    };
    let lines = lock(&area).lines().to_vec();
    let path = {
        let mut compose = world.resource_mut::<ComposeState>();
        let Some(session) = compose.0.as_mut() else {
            world.resource_mut::<Mode>().0 = InputMode::Normal;
            return;
        };
        session.body = lines;
        session.stage = ComposeStage::Review;
        session.body_path.clone()
    };
    write_body(world, &path);
    world.resource_mut::<Mode>().0 = InputMode::Normal;
}

/// The body file is the crash-survival artifact; a failed write is worth
/// saying out loud, but must not cost the buffer.
fn write_body(world: &mut World, path: &std::path::Path) {
    let Some(body) = world
        .resource::<ComposeState>()
        .session()
        .map(|session| session.body.join("\n"))
    else {
        return;
    };
    if let Err(error) = std::fs::write(path, format!("{body}\n")) {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("could not save the body: {error}"), now);
    }
}

/// Called by the router while the editor owns input. Bound keys win;
/// anything else the text area understands is typed.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let keymaps = world.resource::<Keymaps>();
        keymaps.lookup(CONTEXT_EDITOR, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        apply_action(world, &action);
        return Ok(());
    }
    // Ctrl and Alt combinations are chords, not text: swallow the unbound
    // ones rather than inserting stray characters.
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return Ok(());
    }
    world.resource_mut::<InlineEditor>().with_mut(|area| {
        area.input_without_shortcuts(key);
    });
    Ok(())
}

pub fn dispatch(world: &mut World, op: EditorOp) {
    if op == EditorOp::Done {
        return close(world);
    }
    if matches!(op, EditorOp::Copy | EditorOp::Cut) {
        return copy_or_cut(world, op);
    }
    if op == EditorOp::Paste {
        return paste(world);
    }
    if op == EditorOp::Preview {
        return super::preview::open(world);
    }
    world
        .resource_mut::<InlineEditor>()
        .with_mut(|area| match op {
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
            EditorOp::Done
            | EditorOp::Cut
            | EditorOp::Copy
            | EditorOp::Paste
            | EditorOp::Preview => {}
        });
}

/// The attachment token on the cursor's line, if that line is one.
pub(super) fn token_at_cursor(world: &World) -> Option<super::token::AttachToken> {
    let editor = world.resource::<InlineEditor>();
    let area = editor.0.as_ref()?;
    let area = lock(area);
    let row = area.cursor().0;
    super::token::parse(area.lines().get(row)?)
}

/// Puts `text` on a line of its own at the cursor. Returns whether the
/// editor was open to take it.
pub(super) fn insert_line(world: &mut World, text: &str) -> bool {
    world
        .resource_mut::<InlineEditor>()
        .with_mut(|area| {
            area.move_cursor(CursorMove::End);
            area.insert_newline();
            area.insert_str(text);
        })
        .is_some()
}

/// Drops the token line naming `path` from the live buffer. Returns
/// whether the editor was open to edit.
pub(super) fn remove_token_line(world: &mut World, path: &std::path::Path) -> bool {
    world
        .resource_mut::<InlineEditor>()
        .with_mut(|area| {
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
    let yanked = world
        .resource_mut::<InlineEditor>()
        .with_mut(|area| {
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
            .resource_mut::<StatusMessage>()
            .warn(format!("clipboard: {error}"), now);
    }
}

/// Prefers the system clipboard, falling back to the widget's yank buffer
/// when no clipboard is reachable (a bare tty, a locked-down session).
fn paste(world: &mut World) {
    let external = crate::clipboard::get();
    world.resource_mut::<InlineEditor>().with_mut(|area| {
        if let Some(text) = external.filter(|text| !text.is_empty()) {
            area.set_yank_text(text);
        }
        area.paste();
    });
}
