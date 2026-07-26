//! A form's live state: the derived page list, the values collected so
//! far, where focus sits, and what the validators had to say.
//!
//! Values live in one id-keyed map rather than in the field runtimes, so
//! what you typed survives a page switch and a re-derivation. Runtimes
//! exist only for the page on screen.

use bevy_ratatui::crossterm::event::KeyEvent;

use super::field::FieldRuntime;
use super::spec::{CancelFn, FormMode, FormSpec, FormValues, PageSpec, PagesFn, SubmitFn};

const CANCEL_LABEL: &str = "Cancel";
const BACK_LABEL: &str = "Back";
const NEXT_LABEL: &str = "Next";
/// Seeding a page's defaults can reveal another page, which has its own
/// defaults. Two rounds cover every shape here; the cap only stops a
/// pathological `PagesFn` that never settles.
const DERIVATION_ROUNDS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Field(usize),
    Button(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ButtonRole {
    Cancel,
    Back,
    Primary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepState {
    Current,
    Reached,
    Unreached,
}

pub(super) struct FormState {
    pub(super) title: String,
    mode: FormMode,
    primary_label: String,
    pages_fn: PagesFn,
    pages: Vec<PageSpec>,
    page: usize,
    /// Highest page reached during creation; gates jumping ahead.
    reached: usize,
    values: FormValues,
    pub(super) fields: Vec<FieldRuntime>,
    focus: Focus,
    message: Option<String>,
    error_field: Option<usize>,
    /// Bumped whenever the control set changes, so the entity layer
    /// knows to respawn rather than diffing components itself.
    generation: u64,
    on_submit: Option<SubmitFn>,
    on_cancel: Option<CancelFn>,
}

impl FormState {
    pub(super) fn new(spec: FormSpec) -> Self {
        let values = FormValues::default();
        let pages = (spec.pages)(&values);
        let mut state = Self {
            title: spec.title,
            mode: spec.mode,
            primary_label: spec.primary_label,
            pages_fn: spec.pages,
            pages,
            page: 0,
            reached: 0,
            values,
            fields: Vec::new(),
            focus: Focus::Button(0),
            message: None,
            error_field: None,
            generation: 0,
            on_submit: Some(spec.on_submit),
            on_cancel: Some(spec.on_cancel),
        };
        state.converge_pages();
        state.rebuild_fields();
        state
    }

    /// A spec's `initial` value is part of the form's answer even if the
    /// user never visits that page — and a page that only appears after
    /// a branch is chosen still gets its defaults. An id already in the
    /// map is left alone: the form's answer outranks the spec's default.
    fn seed_initial_values(&mut self) {
        let seeds: Vec<(&'static str, String)> = self
            .pages
            .iter()
            .flat_map(|page| page.fields.iter())
            .filter(|field| !self.values.contains(field.id))
            .map(|field| (field.id, field.resolved_initial()))
            .filter(|(_, value)| !value.is_empty())
            .collect();
        for (id, value) in seeds {
            self.values.set(id, value);
        }
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn focus(&self) -> Focus {
        self.focus
    }

    pub(super) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub(super) fn error_field(&self) -> Option<usize> {
        self.error_field
    }

    pub(super) fn page(&self) -> usize {
        self.page
    }

    pub(super) fn has_strip(&self) -> bool {
        self.pages.len() > 1
    }

    pub(super) fn steps(&self) -> Vec<(String, StepState)> {
        self.pages
            .iter()
            .enumerate()
            .map(|(index, page)| (page.title.clone(), self.step_state(index)))
            .collect()
    }

    fn step_state(&self, index: usize) -> StepState {
        if index == self.page {
            StepState::Current
        } else if self.can_reach(index) {
            StepState::Reached
        } else {
            StepState::Unreached
        }
    }

    /// Editing reaches every page at once; creating only reaches what it
    /// has already walked through.
    fn can_reach(&self, index: usize) -> bool {
        index < self.pages.len() && (self.mode == FormMode::Edit || index <= self.reached)
    }

    fn is_last_page(&self) -> bool {
        self.page + 1 >= self.pages.len()
    }

    /// During creation the primary action advances until the last page;
    /// editing saves from wherever you are.
    pub(super) fn primary_advances(&self) -> bool {
        self.mode == FormMode::Create && !self.is_last_page()
    }

    pub(super) fn buttons(&self) -> Vec<(ButtonRole, String)> {
        let mut buttons = vec![(ButtonRole::Cancel, CANCEL_LABEL.to_owned())];
        if self.page > 0 {
            buttons.push((ButtonRole::Back, BACK_LABEL.to_owned()));
        }
        let primary = if self.primary_advances() {
            NEXT_LABEL.to_owned()
        } else {
            self.primary_label.clone()
        };
        buttons.push((ButtonRole::Primary, primary));
        buttons
    }

    pub(super) fn button_labels(&self) -> Vec<String> {
        self.buttons().into_iter().map(|(_, label)| label).collect()
    }

    pub(super) fn role_at(&self, index: usize) -> Option<ButtonRole> {
        self.buttons().get(index).map(|(role, _)| *role)
    }

    /// The current page's live values merged over everything collected
    /// from the pages not on screen.
    pub(super) fn values(&self) -> FormValues {
        let mut values = self.values.clone();
        for field in &self.fields {
            values.set(field.spec.id, field.value());
        }
        values
    }

    fn commit(&mut self) {
        self.values = self.values();
    }

    /// Rebuilding keeps focus on the same *field*, not the same index —
    /// cycling a select that reveals a page must not yank the caret out
    /// from under the key the user is still pressing.
    fn rebuild_fields(&mut self) {
        let held = self.focused_field_id();
        let values = self.values.clone();
        self.fields = self.pages.get(self.page).map_or_else(Vec::new, |page| {
            page.fields
                .iter()
                .map(|spec| FieldRuntime::new(spec.clone(), values.get(spec.id)))
                .collect()
        });
        self.generation += 1;
        self.set_focus(self.landing_focus(held));
    }

    fn focused_field_id(&self) -> Option<&'static str> {
        let Focus::Field(index) = self.focus else {
            return None;
        };
        self.fields.get(index).map(|field| field.spec.id)
    }

    fn landing_focus(&self, held: Option<&'static str>) -> Focus {
        if self.fields.is_empty() {
            return Focus::Button(self.buttons().len().saturating_sub(1));
        }
        let kept = held.and_then(|id| {
            self.fields
                .iter()
                .position(|field| field.spec.id == id)
                .map(Focus::Field)
        });
        kept.unwrap_or(Focus::Field(0))
    }

    /// Derives the page list, then seeds whatever defaults that revealed,
    /// until neither changes. Seeding a page's defaults can bring another
    /// page into existence, and that page has defaults of its own.
    fn converge_pages(&mut self) {
        for _ in 0..DERIVATION_ROUNDS {
            self.pages = (self.pages_fn)(&self.values);
            let before = self.values.clone();
            self.seed_initial_values();
            if self.values == before {
                return;
            }
        }
    }

    /// Re-derives the page list after a value change. The control set is
    /// only rebuilt when its *shape* changes, so typing never churns
    /// entities or resets a cursor.
    fn resync(&mut self) {
        self.commit();
        let pages_before = page_ids(&self.pages);
        let fields_before = field_ids(self.pages.get(self.page));
        self.converge_pages();
        let last = self.pages.len().saturating_sub(1);
        self.page = self.page.min(last);
        self.reached = self.reached.min(last);
        if page_ids(&self.pages) != pages_before
            || field_ids(self.pages.get(self.page)) != fields_before
        {
            self.rebuild_fields();
        }
    }

    pub(super) fn set_focus(&mut self, focus: Focus) {
        if !self.is_reachable(focus) {
            return;
        }
        self.focus = focus;
        for (index, field) in self.fields.iter_mut().enumerate() {
            field.set_focused(focus == Focus::Field(index));
        }
    }

    fn is_reachable(&self, focus: Focus) -> bool {
        match focus {
            Focus::Field(index) => index < self.fields.len(),
            Focus::Button(index) => index < self.buttons().len(),
        }
    }

    /// Tab order is fields, then the buttons left to right, wrapping in
    /// both directions.
    pub(super) fn move_focus(&mut self, forward: bool) {
        let stops = self.fields.len() + self.buttons().len();
        if stops == 0 {
            return;
        }
        let current = self.focus_index();
        let next = if forward {
            (current + 1) % stops
        } else {
            (current + stops - 1) % stops
        };
        self.set_focus(self.focus_at(next));
    }

    fn focus_index(&self) -> usize {
        match self.focus {
            Focus::Field(index) => index,
            Focus::Button(index) => self.fields.len() + index,
        }
    }

    fn focus_at(&self, index: usize) -> Focus {
        match index.checked_sub(self.fields.len()) {
            Some(button) => Focus::Button(button),
            None => Focus::Field(index),
        }
    }

    pub(super) fn edit_focused(&mut self, key: KeyEvent) {
        let Focus::Field(index) = self.focus else {
            return;
        };
        let changed = self
            .fields
            .get_mut(index)
            .is_some_and(|field| field.edit(key));
        if changed {
            self.clear_error();
            self.resync();
        }
    }

    pub(super) fn move_cursor(&mut self, cursor: Cursor) {
        let Focus::Field(index) = self.focus else {
            return;
        };
        let changed = self
            .fields
            .get_mut(index)
            .is_some_and(|field| field.move_cursor(cursor));
        if changed {
            self.clear_error();
            self.resync();
        }
    }

    pub(super) fn go_to_page(&mut self, page: usize) -> bool {
        if page == self.page || !self.can_reach(page) {
            return false;
        }
        self.commit();
        self.page = page;
        self.clear_error();
        self.focus = Focus::Button(0);
        self.rebuild_fields();
        true
    }

    /// Advancing during creation validates the page you are leaving, so
    /// a step is never walked past in a broken state.
    pub(super) fn next_page(&mut self) -> bool {
        if self.is_last_page() {
            return false;
        }
        if self.mode == FormMode::Create && !self.validate_page(self.page) {
            return false;
        }
        self.reached = self.reached.max(self.page + 1);
        let target = self.page + 1;
        self.go_to_page(target)
    }

    pub(super) fn prev_page(&mut self) -> bool {
        match self.page.checked_sub(1) {
            Some(target) => self.go_to_page(target),
            None => false,
        }
    }

    /// True when the whole form is valid. Otherwise the first offending
    /// field takes focus, switching pages if it lives on another.
    pub(super) fn validate_all(&mut self) -> bool {
        let values = self.values();
        let failure = self.pages.iter().enumerate().find_map(|(page, spec)| {
            first_failure(spec, &values).map(|(field, why)| (page, field, why))
        });
        let Some((page, field, why)) = failure else {
            self.clear_error();
            return true;
        };
        if page != self.page {
            self.commit();
            self.page = page;
            self.reached = self.reached.max(page);
            self.rebuild_fields();
        }
        self.error_field = Some(field);
        self.message = Some(why);
        self.set_focus(Focus::Field(field));
        false
    }

    fn validate_page(&mut self, page: usize) -> bool {
        let values = self.values();
        let failure = self
            .pages
            .get(page)
            .and_then(|spec| first_failure(spec, &values));
        match failure {
            Some((field, why)) => {
                self.error_field = Some(field);
                self.message = Some(why);
                self.set_focus(Focus::Field(field));
                false
            }
            None => {
                self.clear_error();
                true
            }
        }
    }

    fn clear_error(&mut self) {
        self.error_field = None;
        self.message = None;
    }

    pub(super) fn take_submit(&mut self) -> Option<SubmitFn> {
        self.on_submit.take()
    }

    pub(super) fn take_cancel(&mut self) -> Option<CancelFn> {
        self.on_cancel.take()
    }
}

fn first_failure(page: &PageSpec, values: &FormValues) -> Option<(usize, String)> {
    page.fields.iter().enumerate().find_map(|(index, field)| {
        let validate = field.validate.as_ref()?;
        validate(values.get(field.id)).err().map(|why| (index, why))
    })
}

fn page_ids(pages: &[PageSpec]) -> Vec<&'static str> {
    pages.iter().map(|page| page.id).collect()
}

fn field_ids(page: Option<&PageSpec>) -> Vec<&'static str> {
    page.map_or_else(Vec::new, |page| {
        page.fields.iter().map(|field| field.id).collect()
    })
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
