//! Destructive questions as a modal surface rather than a bottom-row
//! y/n prompt: the question, room for context about what is about to
//! happen, and two buttons.
//!
//! Focus starts on Cancel and `Esc` cancels, so a reflexive `Enter` on a
//! confirmation that appeared unexpectedly never destroys anything. `y`
//! and `n` stay bound for the muscle memory the prompt left behind.
//!
//! It sits at `layer::MODAL` because it is the one surface that opens
//! *above* another — a picker's callback can raise it.

mod entity;
mod render;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;
use crokey::KeyCombination;

use crate::action::ConfirmOp;
use crate::keymap::{KeymapMatch, Keymaps};
use crate::overlay::surface::Surface;

pub type ConfirmFn = Box<dyn FnOnce(&mut World) + Send + Sync>;

const CANCEL_LABEL: &str = "Cancel";

pub struct ConfirmSpec {
    pub title: String,
    pub question: String,
    /// Context lines under the question — what exactly is being acted
    /// on. Empty is fine when the question already says it.
    pub detail: Vec<String>,
    /// The affirmative button's label: "Delete", "Discard", "Send".
    pub confirm_label: String,
    pub on_confirm: ConfirmFn,
}

impl ConfirmSpec {
    pub fn new(
        title: impl Into<String>,
        question: impl Into<String>,
        confirm_label: impl Into<String>,
        on_confirm: ConfirmFn,
    ) -> Self {
        Self {
            title: title.into(),
            question: question.into(),
            detail: Vec::new(),
            confirm_label: confirm_label.into(),
            on_confirm,
        }
    }

    pub fn with_detail(mut self, detail: Vec<String>) -> Self {
        self.detail = detail;
        self
    }
}

struct ConfirmState {
    title: String,
    question: String,
    detail: Vec<String>,
    confirm_label: String,
    /// Index into `button_labels`; starts on Cancel by design.
    focused: usize,
    on_confirm: ConfirmFn,
}

impl ConfirmState {
    fn button_labels(&self) -> [String; 2] {
        [CANCEL_LABEL.to_owned(), self.confirm_label.clone()]
    }

    fn accepts(&self) -> bool {
        self.focused == 1
    }
}

#[derive(Resource, Default)]
pub struct ActiveConfirm(Option<ConfirmState>);

impl ActiveConfirm {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }

    pub fn question(&self) -> Option<&str> {
        self.0.as_ref().map(|state| state.question.as_str())
    }
}

pub struct ConfirmPlugin;

impl Plugin for ConfirmPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveConfirm>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.add_systems(
            Update,
            (
                entity::sync_confirm_entities,
                entity::sync_interaction,
                entity::refresh_confirm,
            )
                .chain(),
        );
    }
}

pub fn open_confirm(world: &mut World, spec: ConfirmSpec) {
    world.resource_mut::<ActiveConfirm>().0 = Some(ConfirmState {
        title: spec.title,
        question: spec.question,
        detail: spec.detail,
        confirm_label: spec.confirm_label,
        focused: 0,
        on_confirm: spec.on_confirm,
    });
    super::surface::raise(world, super::surface::Surface::Confirm);
}

/// Exact single-key `confirm` bindings only — no chord waits and no
/// global fallback, matching every other modal.
pub fn handle_key(world: &mut World, key: KeyEvent) -> Result {
    let outcome = {
        let layers = Surface::Confirm.key_layers(world);
        let keymaps = world.resource::<Keymaps>();
        keymaps.resolve_layered(&layers, &[KeyCombination::from(key)])
    };
    if let KeymapMatch::Exact(action) = outcome {
        crate::action::apply_action(world, &action);
    }
    Ok(())
}

pub fn dispatch(world: &mut World, op: ConfirmOp) {
    match op {
        ConfirmOp::Accept => accept(world),
        ConfirmOp::FocusNext => move_focus(world, 1),
        ConfirmOp::FocusPrev => move_focus(world, -1),
    }
}

/// `Enter`: whichever button is highlighted.
pub(super) fn activate(world: &mut World) {
    if world
        .resource::<ActiveConfirm>()
        .0
        .as_ref()
        .is_some_and(ConfirmState::accepts)
    {
        return accept(world);
    }
    cancel(world);
}

pub(super) fn cancel(world: &mut World) {
    world.resource_mut::<ActiveConfirm>().0 = None;
}

/// Closes before running the callback, so a chained confirmation opens
/// onto a clear stack rather than above its own predecessor.
fn accept(world: &mut World) {
    let Some(state) = world.resource_mut::<ActiveConfirm>().0.take() else {
        return;
    };
    (state.on_confirm)(world);
}

pub(super) fn focus_button(world: &mut World, index: usize) {
    if let Some(state) = world.resource_mut::<ActiveConfirm>().0.as_mut() {
        state.focused = index.min(1);
    }
}

fn move_focus(world: &mut World, delta: isize) {
    if let Some(state) = world.resource_mut::<ActiveConfirm>().0.as_mut() {
        state.focused = (state.focused as isize + delta).rem_euclid(2) as usize;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::config::RawKeymaps;

    #[derive(Resource, Default)]
    struct Fired(bool);

    fn confirm_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ActiveConfirm>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.init_resource::<Fired>();
        app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
        app
    }

    fn ask(world: &mut World) {
        open_confirm(
            world,
            ConfirmSpec::new(
                "Delete",
                "Delete this message permanently?",
                "Delete",
                Box::new(|world| world.resource_mut::<Fired>().0 = true),
            ),
        );
    }

    fn press(app: &mut App, code: KeyCode) {
        handle_key(app.world_mut(), KeyEvent::from(code)).unwrap();
    }

    fn fired(app: &App) -> bool {
        app.world().resource::<Fired>().0
    }

    /// The property the whole surface exists for: a confirmation that
    /// appears while the user is typing must not be destroyable by the
    /// keystroke already on its way.
    #[test]
    fn enter_on_first_open_declines_rather_than_destroying() {
        let mut app = confirm_app();
        ask(app.world_mut());

        press(&mut app, KeyCode::Enter);

        assert!(
            !fired(&app),
            "Enter must land on Cancel, not the affirmative"
        );
        assert!(
            !app.world().resource::<ActiveConfirm>().is_open(),
            "and it must still dismiss the question"
        );
    }

    #[test]
    fn moving_focus_to_the_affirmative_makes_enter_accept() {
        let mut app = confirm_app();
        ask(app.world_mut());

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);

        assert!(fired(&app));
    }

    #[test]
    fn y_accepts_and_n_declines_whatever_is_focused() {
        let mut app = confirm_app();
        ask(app.world_mut());
        press(&mut app, KeyCode::Char('n'));
        assert!(!fired(&app));
        assert!(!app.world().resource::<ActiveConfirm>().is_open());

        ask(app.world_mut());
        press(&mut app, KeyCode::Char('y'));
        assert!(fired(&app), "y accepts from the Cancel button too");
    }

    #[test]
    fn esc_declines() {
        let mut app = confirm_app();
        ask(app.world_mut());

        press(&mut app, KeyCode::Esc);

        assert!(!fired(&app));
        assert!(!app.world().resource::<ActiveConfirm>().is_open());
    }

    #[test]
    fn focus_wraps_both_ways() {
        let mut app = confirm_app();
        ask(app.world_mut());

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);

        assert!(!fired(&app), "two moves return to Cancel");
    }

    /// A chained question (send-without-subject into send-without-
    /// attachment) must replace its predecessor, not stack on it.
    #[test]
    fn accepting_closes_before_the_callback_runs() {
        let mut app = confirm_app();
        open_confirm(
            app.world_mut(),
            ConfirmSpec::new(
                "First",
                "First?",
                "Yes",
                Box::new(|world| {
                    assert!(
                        !world.resource::<ActiveConfirm>().is_open(),
                        "the callback must see a clear surface"
                    );
                    ask(world);
                }),
            ),
        );

        press(&mut app, KeyCode::Char('y'));

        assert_eq!(
            app.world().resource::<ActiveConfirm>().question(),
            Some("Delete this message permanently?"),
            "the second question replaces the first"
        );
    }
}
