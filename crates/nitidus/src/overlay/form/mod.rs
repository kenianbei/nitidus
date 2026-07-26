//! Modal forms: a set of tab-focusable fields with Cancel and a primary
//! button, drawn at `layer::OVERLAY` above whatever screen is beneath.
//!
//! Keyboard input stays on the router, resolved against the rebindable
//! `form` context, with unbound printables typing into the focused
//! field — so global bindings never leak through a modal.
//!
//! Focus is owned here rather than by plurimus. `PlurimusUiPlugin`
//! chains its PreUpdate systems `collect_key_actions → run_world_intents
//! → apply_focus_intents`, so the router drains every key in a frame
//! before a `UiFocusMessage` is applied; a Tab followed by more keys in
//! one frame would otherwise all land in the pre-Tab field. `ActiveForm`
//! moves focus synchronously and `entity::mirror_focus` pushes the
//! result outward for styling and hit-testing.

mod entity;
mod field;
mod geometry;
mod interaction;
mod mouse;
mod render;
mod spec;
mod state;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use crokey::KeyCombination;
use plurimus::UiFocusMessage;

pub use spec::{FieldSpec, FormMode, FormSpec, FormValues, PageSpec, PagesFn, SelectOption};

use crate::action::FormOp;
use crate::keymap::{CONTEXT_FORM, KeymapMatch, Keymaps};
use state::{ButtonRole, Cursor, Focus, FormState};

pub struct FormPlugin;

impl Plugin for FormPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveForm>();
        // Idempotent with PlurimusUiPlugin's registration; keeps this
        // plugin usable in headless test apps.
        app.add_message::<UiFocusMessage>();
        app.add_systems(
            Update,
            (
                entity::sync_form_entities,
                entity::mirror_focus,
                entity::refresh_form,
                interaction::sync_interaction,
            )
                .chain(),
        );
    }
}

#[derive(Resource, Default)]
pub struct ActiveForm(Option<FormState>);

impl ActiveForm {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }

    pub fn title(&self) -> Option<&str> {
        self.0.as_ref().map(|state| state.title.as_str())
    }

    /// The value of a field by id — the handle tests and callers use to
    /// inspect a form without reaching into its runtime.
    pub fn value(&self, id: &str) -> Option<String> {
        self.0
            .as_ref()
            .map(|state| state.values().get(id).to_owned())
    }

    /// The zero-based index of the page on screen.
    pub fn page(&self) -> Option<usize> {
        self.0.as_ref().map(FormState::page)
    }

    /// Step titles in order, for callers and tests that want to see the
    /// shape a form derived without reaching into its runtime.
    pub fn step_titles(&self) -> Vec<String> {
        self.0.as_ref().map_or_else(Vec::new, |state| {
            state.steps().into_iter().map(|(title, _)| title).collect()
        })
    }

    pub fn message(&self) -> Option<&str> {
        self.0.as_ref().and_then(FormState::message)
    }

    fn state(&self) -> Option<&FormState> {
        self.0.as_ref()
    }

    fn state_mut(&mut self) -> Option<&mut FormState> {
        self.0.as_mut()
    }
}

pub fn open_form(world: &mut World, spec: FormSpec) {
    world.resource_mut::<ActiveForm>().0 = Some(FormState::new(spec));
}

/// Runs `on_cancel` and closes. Safe to call when nothing is open.
fn cancel(world: &mut World) {
    let Some(mut state) = world.resource_mut::<ActiveForm>().0.take() else {
        return;
    };
    if let Some(on_cancel) = state.take_cancel() {
        on_cancel(world);
    }
}

/// Validates, then submits. A failing validator keeps the form open with
/// the offending field focused, so nothing is lost on a typo.
fn submit(world: &mut World) {
    {
        let mut form = world.resource_mut::<ActiveForm>();
        let Some(state) = form.state_mut() else {
            return;
        };
        if !state.validate_all() {
            return;
        }
    }
    let Some(mut state) = world.resource_mut::<ActiveForm>().0.take() else {
        return;
    };
    let values = state.values();
    if let Some(on_submit) = state.take_submit() {
        on_submit(world, values);
    }
}

/// Enter: the focused button if one has focus, otherwise the page's
/// primary action — so Enter always does what the highlighted button
/// says. During creation that is Next until the last page.
fn activate(world: &mut World) {
    let role = {
        let form = world.resource::<ActiveForm>();
        let Some(state) = form.state() else {
            return;
        };
        match state.focus() {
            Focus::Button(index) => state.role_at(index),
            Focus::Field(_) => Some(ButtonRole::Primary),
        }
    };
    match role {
        Some(ButtonRole::Cancel) => cancel(world),
        Some(ButtonRole::Back) => prev_page(world),
        Some(ButtonRole::Primary) => primary(world),
        None => {}
    }
}

fn primary(world: &mut World) {
    let advances = world
        .resource::<ActiveForm>()
        .state()
        .is_some_and(FormState::primary_advances);
    if advances {
        next_page(world);
    } else {
        submit(world);
    }
}

fn next_page(world: &mut World) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.next_page();
    }
}

fn prev_page(world: &mut World) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.prev_page();
    }
}

pub fn go_to_page(world: &mut World, page: usize) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.go_to_page(page);
    }
}

pub fn dispatch(world: &mut World, op: FormOp) {
    match op {
        FormOp::FocusNext => move_focus(world, true),
        FormOp::FocusPrev => move_focus(world, false),
        FormOp::Activate => activate(world),
        FormOp::Cancel => cancel(world),
        FormOp::Left => move_cursor(world, Cursor::Left),
        FormOp::Right => move_cursor(world, Cursor::Right),
        FormOp::NextPage => next_page(world),
        FormOp::PrevPage => prev_page(world),
    }
}

fn move_focus(world: &mut World, forward: bool) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.move_focus(forward);
    }
}

fn move_cursor(world: &mut World, cursor: Cursor) {
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.move_cursor(cursor);
    }
}

/// Exact single-key `form` bindings win; everything else printable
/// edits the focused field. No chord waits and no global fallback, by
/// design — the picker precedent.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let keymaps = world.resource::<Keymaps>();
        keymaps.lookup(CONTEXT_FORM, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        crate::action::apply_action(world, &action);
        return Ok(());
    }
    if let Some(state) = world.resource_mut::<ActiveForm>().state_mut() {
        state.edit_focused(key);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
