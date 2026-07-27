//! One live field. Text fields edit through tui-prompts (the bottom
//! prompt's engine, so masking comes free); select fields hold an index
//! into their spec's options, which is why cycling cannot drift out of
//! range.

use std::sync::Arc;

use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::CursorMove;
use tui_prompts::{FocusState, State, TextState};

use super::body::{SharedArea, area_from, lock};
use super::spec::{FieldSpec, SelectOption};
use super::state::Cursor;

fn split_entries(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Completion works on one address at a time: everything after the last
/// comma.
fn active_segment(value: &str) -> &str {
    value
        .rsplit_once(',')
        .map_or(value, |(_, segment)| segment)
        .trim_start()
}

fn replace_active_segment(value: &str, candidate: &str) -> String {
    match value.rsplit_once(',') {
        Some((prefix, _)) => format!("{prefix}, {candidate}"),
        None => candidate.to_owned(),
    }
}

pub(super) struct FieldRuntime {
    pub(super) spec: FieldSpec,
    editor: FieldEditor,
    /// Candidates for the segment under the caret, refreshed on every
    /// edit; `cycle` freezes a position in them while stepping.
    candidates: Vec<String>,
    cycle: Option<usize>,
}

enum FieldEditor {
    Text(TextState<'static>),
    Select(usize),
    Body(SharedArea),
    Entries {
        entries: Vec<String>,
        selected: usize,
    },
}

impl FieldRuntime {
    /// `initial` is a starting value for a text field and the name of an
    /// option for a select; an unknown option selects the first.
    pub(super) fn new(spec: FieldSpec, initial: &str) -> Self {
        let editor = if spec.is_select() {
            let selected = spec
                .options()
                .iter()
                .position(|option| option.value == initial)
                .unwrap_or(0);
            FieldEditor::Select(selected)
        } else if spec.is_body() {
            FieldEditor::Body(area_from(initial))
        } else if spec.is_entries() {
            FieldEditor::Entries {
                entries: split_entries(initial),
                selected: 0,
            }
        } else {
            let mut text = TextState::new().with_value(initial.to_owned());
            text.move_end();
            FieldEditor::Text(text)
        };
        let mut field = Self {
            spec,
            editor,
            candidates: Vec::new(),
            cycle: None,
        };
        field.refresh_candidates();
        field
    }

    pub(super) fn candidates(&self) -> &[String] {
        &self.candidates
    }

    pub(super) fn cycle(&self) -> Option<usize> {
        self.cycle
    }

    /// Steps through the frozen candidate list, rewriting only the
    /// segment being typed. The list is deliberately not recomputed
    /// mid-cycle: matching against the value just inserted would strand
    /// the cycle after one step.
    pub(super) fn cycle_candidate(&mut self, forward: bool) -> bool {
        if self.candidates.is_empty() {
            return false;
        }
        let count = self.candidates.len();
        let next = match self.cycle {
            Some(current) if forward => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None if forward => 0,
            None => count - 1,
        };
        self.cycle = Some(next);
        let FieldEditor::Text(text) = &mut self.editor else {
            return false;
        };
        let replaced = replace_active_segment(text.value(), &self.candidates[next]);
        *text = TextState::new().with_value(replaced);
        text.move_end();
        *text.focus_state_mut() = FocusState::Focused;
        true
    }

    fn refresh_candidates(&mut self) {
        let Some(complete) = self.spec.complete.clone() else {
            return;
        };
        let value = self.value();
        self.candidates = complete(active_segment(&value));
        self.cycle = None;
    }

    pub(super) fn value(&self) -> String {
        match &self.editor {
            FieldEditor::Text(text) => text.value().to_owned(),
            FieldEditor::Body(area) => lock(area).lines().join("\n"),
            FieldEditor::Entries { entries, .. } => entries.join("\n"),
            FieldEditor::Select(selected) => self
                .spec
                .options()
                .get(*selected)
                .map(|option| option.value.clone())
                .unwrap_or_default(),
        }
    }

    /// The live buffer, for the renderer and for the editing commands
    /// that reach past `edit` — motions, undo, the clipboard.
    pub(super) fn area(&self) -> Option<SharedArea> {
        match &self.editor {
            FieldEditor::Body(area) => Some(Arc::clone(area)),
            _ => None,
        }
    }

    /// The entries and which one is picked, for the renderer and for
    /// whatever acts on the selection.
    pub(super) fn entries(&self) -> Option<(&[String], usize)> {
        match &self.editor {
            FieldEditor::Entries { entries, selected } => Some((entries, *selected)),
            _ => None,
        }
    }

    pub(super) fn selected_entry(&self) -> Option<&String> {
        let (entries, selected) = self.entries()?;
        entries.get(selected)
    }

    pub(super) fn push_entry(&mut self, entry: String) -> bool {
        let FieldEditor::Entries { entries, selected } = &mut self.editor else {
            return false;
        };
        if entries.contains(&entry) {
            return false;
        }
        entries.push(entry);
        *selected = entries.len() - 1;
        true
    }

    /// Drops the selection, leaving the one that took its place picked.
    pub(super) fn remove_selected_entry(&mut self) -> Option<String> {
        let FieldEditor::Entries { entries, selected } = &mut self.editor else {
            return None;
        };
        if *selected >= entries.len() {
            return None;
        }
        let removed = entries.remove(*selected);
        *selected = selected.saturating_sub(usize::from(*selected >= entries.len()));
        Some(removed)
    }

    pub(super) fn selected(&self) -> Option<&SelectOption> {
        match &self.editor {
            FieldEditor::Select(selected) => self.spec.options().get(*selected),
            _ => None,
        }
    }

    pub(super) fn cursor(&self) -> usize {
        match &self.editor {
            FieldEditor::Text(text) => text.position(),
            _ => 0,
        }
    }

    pub(super) fn set_focused(&mut self, focused: bool) {
        if let FieldEditor::Text(text) = &mut self.editor {
            *text.focus_state_mut() = if focused {
                FocusState::Focused
            } else {
                FocusState::Unfocused
            };
        }
    }

    /// A body draws its own caret, so an unfocused one has to hide it —
    /// two visible cursors on a form is one too many.
    pub(super) fn apply_theme(&self, theme: &nitidus_ui_kit::theme::Theme, focused: bool) {
        let Some(area) = self.area() else {
            return;
        };
        let mut area = lock(&area);
        let base = theme.base.default.normal.style();
        area.set_style(base);
        area.set_cursor_line_style(ratatui::style::Style::default());
        area.set_selection_style(theme.base.info.selected.style());
        area.set_wrap_mode(ratatui_textarea::WrapMode::Word);
        area.set_cursor_style(if focused {
            theme.base.default.selected.style()
        } else {
            base
        });
    }

    /// What the spec says each of this body's lines should look like.
    pub(super) fn line_styles(
        &self,
        theme: &nitidus_ui_kit::theme::Theme,
    ) -> Vec<Option<ratatui::style::Style>> {
        let Some(style) = self.spec.body_style() else {
            return Vec::new();
        };
        let Some(area) = self.area() else {
            return Vec::new();
        };
        let lines = lock(&area).lines().to_vec();
        style(&lines, theme)
    }

    /// Anything the `form` keymap left unbound. Enter, Esc and Tab are
    /// refused outright: they mean newline, cancel and focus, and every
    /// one of those belongs to a layer above the field. Reports whether
    /// anything changed.
    pub(super) fn edit(&mut self, key: KeyEvent) -> bool {
        if self.spec.read_only || matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Tab) {
            return false;
        }
        let is_printable = matches!(key.code, KeyCode::Char(_))
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if !is_printable && !matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
            return false;
        }
        match &mut self.editor {
            FieldEditor::Text(text) => {
                text.handle_key_event(key);
                self.refresh_candidates();
                true
            }
            FieldEditor::Body(area) => {
                lock(area).input_without_shortcuts(key);
                true
            }
            // Delete is how an entry leaves the row; nothing is typed
            // into one.
            FieldEditor::Entries { .. } => self.remove_selected_entry().is_some(),
            FieldEditor::Select(_) => false,
        }
    }

    /// Left and Right mean "move the caret" in a text field and "take
    /// the previous or next option" in a select. Reports whether the
    /// value changed, which is what drives page re-derivation.
    pub(super) fn move_cursor(&mut self, cursor: Cursor) -> bool {
        if self.spec.read_only {
            return false;
        }
        match &mut self.editor {
            FieldEditor::Text(text) => {
                match cursor {
                    Cursor::Left => text.move_left(),
                    Cursor::Right => text.move_right(),
                }
                false
            }
            FieldEditor::Body(area) => {
                let motion = match cursor {
                    Cursor::Left => CursorMove::Back,
                    Cursor::Right => CursorMove::Forward,
                };
                lock(area).move_cursor(motion);
                false
            }
            FieldEditor::Select(selected) => {
                let count = self.spec.options().len();
                if count == 0 {
                    return false;
                }
                *selected = match cursor {
                    Cursor::Right => (*selected + 1) % count,
                    Cursor::Left => (*selected + count - 1) % count,
                };
                true
            }
            FieldEditor::Entries { entries, selected } => {
                let count = entries.len();
                if count == 0 {
                    return false;
                }
                *selected = match cursor {
                    Cursor::Right => (*selected + 1) % count,
                    Cursor::Left => (*selected + count - 1) % count,
                };
                false
            }
        }
    }

    #[cfg(test)]
    pub(super) fn status(&self) -> Option<tui_prompts::Status> {
        match &self.editor {
            FieldEditor::Text(text) => Some(text.status()),
            _ => None,
        }
    }
}
