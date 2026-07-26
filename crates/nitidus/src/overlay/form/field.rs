//! One live field. Text fields edit through tui-prompts (the bottom
//! prompt's engine, so masking comes free); select fields hold an index
//! into their spec's options, which is why cycling cannot drift out of
//! range.

use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_prompts::{FocusState, State, TextState};

use super::spec::{FieldSpec, SelectOption};
use super::state::Cursor;

pub(super) struct FieldRuntime {
    pub(super) spec: FieldSpec,
    editor: FieldEditor,
}

enum FieldEditor {
    Text(TextState<'static>),
    Select(usize),
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
        } else {
            let mut text = TextState::new().with_value(initial.to_owned());
            text.move_end();
            FieldEditor::Text(text)
        };
        Self { spec, editor }
    }

    pub(super) fn value(&self) -> String {
        match &self.editor {
            FieldEditor::Text(text) => text.value().to_owned(),
            FieldEditor::Select(selected) => self
                .spec
                .options()
                .get(*selected)
                .map(|option| option.value.clone())
                .unwrap_or_default(),
        }
    }

    pub(super) fn selected(&self) -> Option<&SelectOption> {
        match &self.editor {
            FieldEditor::Text(_) => None,
            FieldEditor::Select(selected) => self.spec.options().get(*selected),
        }
    }

    pub(super) fn cursor(&self) -> usize {
        match &self.editor {
            FieldEditor::Text(text) => text.position(),
            FieldEditor::Select(_) => 0,
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

    /// Anything the `form` keymap left unbound. Enter and Esc are refused
    /// outright: `State::handle_key_event` treats them as submit and
    /// abort, which belong to the form. Reports whether anything changed.
    pub(super) fn edit(&mut self, key: KeyEvent) -> bool {
        let FieldEditor::Text(text) = &mut self.editor else {
            return false;
        };
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            return false;
        }
        let is_printable = matches!(key.code, KeyCode::Char(_))
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if !is_printable && !matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
            return false;
        }
        text.handle_key_event(key);
        true
    }

    /// Left and Right mean "move the caret" in a text field and "take
    /// the previous or next option" in a select. Reports whether the
    /// value changed, which is what drives page re-derivation.
    pub(super) fn move_cursor(&mut self, cursor: Cursor) -> bool {
        match &mut self.editor {
            FieldEditor::Text(text) => {
                match cursor {
                    Cursor::Left => text.move_left(),
                    Cursor::Right => text.move_right(),
                }
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
        }
    }

    #[cfg(test)]
    pub(super) fn status(&self) -> Option<tui_prompts::Status> {
        match &self.editor {
            FieldEditor::Text(text) => Some(text.status()),
            FieldEditor::Select(_) => None,
        }
    }
}
