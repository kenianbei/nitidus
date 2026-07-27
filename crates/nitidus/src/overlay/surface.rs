//! The overlay stack: which modal surfaces are open, and in what order
//! they were raised.
//!
//! Before this existed the router carried one gate per surface in a
//! hand-written order, and "what is above what" was a convention nothing
//! enforced. Now the last surface pushed is the top one, it alone takes
//! the keyboard, and the `layer` rung it draws on has to agree.
//!
//! Each surface keeps its own state resource, and that resource stays
//! authoritative: a surface is open when its own state says so. The
//! stack only records order, so a surface that closes without popping
//! (Esc, a callback, a submit) cannot strand the keyboard — `top` drops
//! any entry that is no longer open.

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::KeyEvent;

use crate::keymap::{CONTEXT_CONFIRM, CONTEXT_EXPLORER, CONTEXT_LOG, CONTEXT_PICKER};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Picker,
    Form,
    Explorer,
    AttachPreview,
    Confirm,
    MessageLog,
}

impl Surface {
    const ALL: [Surface; 6] = [
        Surface::Picker,
        Surface::Form,
        Surface::Explorer,
        Surface::AttachPreview,
        Surface::Confirm,
        Surface::MessageLog,
    ];

    fn is_open(self, world: &World) -> bool {
        match self {
            Surface::Picker => world
                .get_resource::<super::ActiveOverlay>()
                .is_some_and(super::ActiveOverlay::is_open),
            Surface::Form => world
                .get_resource::<super::form::ActiveForm>()
                .is_some_and(super::form::ActiveForm::is_open),
            Surface::Explorer => world
                .get_resource::<crate::explorer::ExplorerState>()
                .is_some_and(crate::explorer::ExplorerState::is_open),
            Surface::AttachPreview => world
                .get_resource::<crate::compose::AttachPreview>()
                .is_some_and(crate::compose::AttachPreview::is_open),
            Surface::Confirm => world
                .get_resource::<super::confirm::ActiveConfirm>()
                .is_some_and(super::confirm::ActiveConfirm::is_open),
            Surface::MessageLog => world
                .get_resource::<super::log::LogPanel>()
                .is_some_and(super::log::LogPanel::is_open),
        }
    }

    /// The keymap layers this surface resolves against, most specific
    /// first. Its own key handler and the help overlay both read these,
    /// so what help lists is what will fire. The attach preview closes on
    /// any key and so binds nothing.
    pub(crate) fn key_layers(self, world: &World) -> Vec<&'static str> {
        match self {
            Surface::Picker => vec![CONTEXT_PICKER],
            Surface::Form => super::form::key_layers(world),
            Surface::Explorer => vec![CONTEXT_EXPLORER],
            Surface::AttachPreview => Vec::new(),
            Surface::Confirm => vec![CONTEXT_CONFIRM],
            Surface::MessageLog => vec![CONTEXT_LOG],
        }
    }

    fn handle_key(self, world: &mut World, key: KeyEvent) -> Result {
        match self {
            Surface::Picker => super::picker::handle_key(world, key),
            Surface::Form => super::form::handle_key(world, key),
            Surface::Explorer => crate::explorer::handle_key(world, key),
            Surface::AttachPreview => crate::compose::preview::handle_key(world, key),
            Surface::Confirm => super::confirm::handle_key(world, key),
            Surface::MessageLog => super::log::handle_key(world, key),
        }
    }
}

#[derive(Resource, Default)]
pub struct OverlayStack(Vec<Surface>);

/// Raises `surface` to the top. Re-raising one already on the stack
/// moves it rather than duplicating it, so the stack holds each surface
/// at most once.
pub fn raise(world: &mut World, surface: Surface) {
    let mut stack = world.resource_mut::<OverlayStack>();
    stack.0.retain(|entry| *entry != surface);
    stack.0.push(surface);
}

/// The surface owning the keyboard, dropping any that closed behind the
/// stack's back. A world with no overlay plugins has no stack and no
/// top, which is how base surfaces ask "is anything above me?".
pub fn top(world: &mut World) -> Option<Surface> {
    loop {
        let candidate = *world.get_resource::<OverlayStack>()?.0.last()?;
        if candidate.is_open(world) {
            return Some(candidate);
        }
        world.resource_mut::<OverlayStack>().0.pop();
    }
}

pub fn is_any_open(world: &World) -> bool {
    Surface::ALL.iter().any(|surface| surface.is_open(world))
}

/// The router's single overlay gate: the top surface consumes the key.
pub fn route_key(world: &mut World, key: KeyEvent) -> Option<Result> {
    let surface = top(world)?;
    Some(surface.handle_key(world, key))
}

/// The layers the keyboard is resolving against right now: the top
/// surface's, or the focused pane's when nothing is above it. The help
/// overlay reads this so it can only ever list live bindings.
pub fn key_layers(world: &mut World) -> Vec<&'static str> {
    match top(world) {
        Some(surface) => surface.key_layers(world),
        None => crate::focus::active_layers(world),
    }
}

/// `:confirm` from a surface's own keymap context. Forms are absent by
/// design — their primary action is `:form-activate`, which knows which
/// button is focused.
pub fn confirm(world: &mut World) {
    match top(world) {
        Some(Surface::Picker) => super::picker::confirm(world),
        Some(Surface::Explorer) => crate::explorer::confirm(world),
        Some(Surface::AttachPreview) => crate::compose::preview::close(world),
        Some(Surface::Confirm) => super::confirm::activate(world),
        Some(Surface::MessageLog) => super::log::close(world),
        Some(Surface::Form) | None => {}
    }
}

pub fn cancel(world: &mut World) {
    match top(world) {
        Some(Surface::Picker) => super::picker::close(world),
        Some(Surface::Explorer) => crate::explorer::close(world),
        Some(Surface::AttachPreview) => crate::compose::preview::close(world),
        Some(Surface::Confirm) => super::confirm::cancel(world),
        Some(Surface::MessageLog) => super::log::close(world),
        Some(Surface::Form) | None => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy::app::AppExit;
    use bevy_ratatui::crossterm::event::KeyCode;

    use super::*;
    use crate::overlay::{PickerItem, PickerSpec, open_picker};

    fn press(app: &mut App, code: KeyCode) {
        route_key(app.world_mut(), KeyEvent::from(code));
    }

    fn stack_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<OverlayStack>();
        app.init_resource::<super::super::ActiveOverlay>();
        app.init_resource::<super::super::form::ActiveForm>();
        app.init_resource::<crate::explorer::ExplorerState>();
        app.init_resource::<crate::compose::AttachPreview>();
        app.init_resource::<crate::overlay::confirm::ActiveConfirm>();
        app.init_resource::<crate::overlay::log::LogPanel>();
        app.init_resource::<crate::status::MessageLog>();
        app
    }

    fn a_picker(world: &mut World) {
        open_picker(
            world,
            PickerSpec {
                title: "pick".to_owned(),
                items: vec![PickerItem {
                    label: "one".to_owned(),
                    detail: None,
                }],
                on_select: Box::new(|_, _| {}),
            },
        );
    }

    fn an_explorer(world: &mut World) {
        crate::explorer::open_explorer(
            world,
            crate::explorer::ExplorerRequest {
                title: "files".to_owned(),
                extensions: &[],
                start_dir: None,
                on_pick: Box::new(|_, _| {}),
            },
        );
    }

    #[test]
    fn an_empty_stack_has_no_top() {
        let mut app = stack_app();
        assert_eq!(top(app.world_mut()), None);
        assert!(!is_any_open(app.world()));
    }

    #[test]
    fn the_last_surface_raised_owns_the_keyboard() {
        let mut app = stack_app();
        let world = app.world_mut();
        a_picker(world);
        an_explorer(world);

        assert_eq!(top(world), Some(Surface::Explorer));
    }

    #[test]
    fn closing_the_top_surface_hands_the_keyboard_back_to_the_one_beneath() {
        let mut app = stack_app();
        let world = app.world_mut();
        a_picker(world);
        an_explorer(world);

        crate::explorer::close(world);

        assert_eq!(
            top(world),
            Some(Surface::Picker),
            "a surface that closes without popping must not strand the keyboard"
        );
    }

    /// The explorer used to hardcode its keys and forward the rest to
    /// ratatui-explorer, so a key it did not know did nothing and no
    /// `keys.toml` could reach it. It now resolves its own context.
    #[test]
    fn the_explorer_context_is_rebindable() {
        let mut app = stack_app();
        let raw: crate::config::RawKeymaps =
            toml::from_str("[explorer]\n\"x\" = \":cancel\"\n").unwrap();
        app.insert_resource(crate::keymap::Keymaps::compile(&raw).unwrap());
        an_explorer(app.world_mut());

        press(&mut app, KeyCode::Char('x'));

        assert!(
            !crate::explorer::ExplorerState::is_open(app.world().resource()),
            "a user-bound explorer key must reach the surface"
        );
    }

    #[test]
    fn global_bindings_do_not_leak_through_the_explorer() {
        let mut app = stack_app();
        app.insert_resource(
            crate::keymap::Keymaps::compile(&crate::config::RawKeymaps::default()).unwrap(),
        );
        an_explorer(app.world_mut());

        // `q` is the global quit; over a modal it must do nothing.
        press(&mut app, KeyCode::Char('q'));

        assert!(
            app.world().resource::<Messages<AppExit>>().is_empty(),
            "a modal must not fall through to global bindings"
        );
        assert!(crate::explorer::ExplorerState::is_open(
            app.world().resource()
        ));
    }

    #[test]
    fn raising_a_surface_twice_does_not_duplicate_it() {
        let mut app = stack_app();
        let world = app.world_mut();
        a_picker(world);
        raise(world, Surface::Picker);

        assert_eq!(world.resource::<OverlayStack>().0, vec![Surface::Picker]);
    }
}
