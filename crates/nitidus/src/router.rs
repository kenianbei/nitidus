//! The key router: one global passthrough resolves each key against the
//! active keymap trie the moment it arrives, so bursts of input route
//! correctly across synchronous mode switches.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crokey::{KeyCombination, KeyCombinationFormat};
use plurimus::{UiActions, UiEvent, UiInputBinding, Widget};

use crate::action::apply_action;
use crate::keymap::{
    CONTEXT_COMPOSE, CONTEXT_CONTACTS, CONTEXT_INDEX, CONTEXT_PAGER, CONTEXT_SIDEBAR, InputMode,
    KeymapMatch, Keymaps, Mode,
};
use crate::screen::Screen;
use crate::status::{StatusMessage, expire_status_messages};

const CHORD_TIMEOUT_SECS: f64 = 0.5;

pub struct RouterPlugin;

impl Plugin for RouterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mode>();
        app.init_resource::<PendingKeys>();
        app.init_resource::<StatusMessage>();
        app.init_resource::<crate::overlay::ActiveOverlay>();
        app.init_resource::<crate::explorer::ExplorerState>();
        app.init_resource::<crate::addresses::AddressIndex>();
        app.init_resource::<crate::prompt::PromptState>();
        app.init_resource::<crate::sidebar::SidebarState>();
        app.init_resource::<Screen>();
        app.add_systems(Startup, spawn_router);
        app.add_systems(Update, (expire_pending, expire_status_messages));
    }
}

#[derive(Resource, Default)]
pub struct PendingKeys {
    keys: Vec<KeyCombination>,
    last_press_secs: f64,
}

impl PendingKeys {
    pub fn hint(&self) -> Option<String> {
        if self.keys.is_empty() {
            None
        } else {
            Some(format_keys(&self.keys))
        }
    }
}

/// Implicit shift: `M` renders as `M`, not `Shift-m` — matches how
/// bindings are written in keys.toml.
pub fn format_keys(keys: &[KeyCombination]) -> String {
    let format = KeyCombinationFormat::default().with_implicit_shift();
    keys.iter().map(|key| format.to_string(*key)).collect()
}

fn spawn_router(mut commands: Commands) {
    commands.spawn((
        Widget::from_render_fn(|_, _| Ok(())),
        UiActions::new(vec![UiInputBinding::key_passthrough(route_key).global()]),
    ));
}

pub fn route_key(world: &mut World, _entity: Entity, event: UiEvent) -> Result {
    let UiEvent::Key(key) = event else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        world.write_message(AppExit::Success);
        return Ok(());
    }
    if world.resource::<Mode>().0 == InputMode::CommandLine {
        return crate::cmdline::handle_key(world, key);
    }
    if world.resource::<Mode>().0 == InputMode::Prompt {
        return crate::prompt::handle_key(world, key);
    }
    if world.resource::<Mode>().0 == InputMode::Search {
        return crate::index::search::handle_key(world, key);
    }
    if world.resource::<crate::overlay::ActiveOverlay>().is_open() {
        return crate::overlay::handle_key(world, key);
    }
    if world.resource::<crate::explorer::ExplorerState>().is_open() {
        return crate::explorer::handle_key(world, key);
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut pending = world.resource_mut::<PendingKeys>();
    pending.keys.push(KeyCombination::from(key));
    pending.last_press_secs = now;
    resolve_now(world, now);
    Ok(())
}

/// Resolves after every key push, so the buffer is only ever one key
/// deeper than the last resolution — whole-buffer lookup is incremental.
fn resolve_now(world: &mut World, now: f64) {
    let outcome = {
        let context = if world.resource::<crate::sidebar::SidebarState>().focused {
            CONTEXT_SIDEBAR
        } else {
            match world.resource::<Screen>() {
                Screen::Index => CONTEXT_INDEX,
                Screen::Pager => CONTEXT_PAGER,
                Screen::Compose => CONTEXT_COMPOSE,
                Screen::Contacts => CONTEXT_CONTACTS,
            }
        };
        let keymaps = world.resource::<Keymaps>();
        let pending = world.resource::<PendingKeys>();
        keymaps.resolve_layered(context, &pending.keys)
    };
    match outcome {
        KeymapMatch::Exact(action) => {
            world.resource_mut::<PendingKeys>().keys.clear();
            apply_action(world, &action);
        }
        KeymapMatch::Prefix => {}
        KeymapMatch::Unbound => {
            let keys = std::mem::take(&mut world.resource_mut::<PendingKeys>().keys);
            world
                .resource_mut::<StatusMessage>()
                .warn(format!("unbound: {}", format_keys(&keys)), now);
        }
    }
}

fn expire_pending(time: Res<Time>, mut pending: ResMut<PendingKeys>) {
    if !pending.keys.is_empty()
        && time.elapsed_secs_f64() - pending.last_press_secs > CHORD_TIMEOUT_SECS
    {
        pending.keys.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::KeyEvent;

    use super::*;
    use crate::cmdline::CommandLineState;
    use crate::config::RawKeymaps;
    use crate::shell::Tabs;

    fn router_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Tabs>();
        app.init_resource::<CommandLineState>();
        app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
        app.add_plugins(RouterPlugin);
        app.update();
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        let event = UiEvent::Key(KeyEvent::from(code));
        route_key(app.world_mut(), Entity::PLACEHOLDER, event).unwrap();
    }

    fn press_str(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn exact_match_applies_action_and_clears() {
        let mut app = router_app();
        press(&mut app, KeyCode::Char('q'));
        assert!(app.world().resource::<PendingKeys>().keys.is_empty());
        assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    }

    #[test]
    fn prefix_keeps_pending_and_exposes_hint() {
        let mut app = router_app();
        let raw: RawKeymaps = toml::from_str("[global]\n\"gg\" = \":tab-prev\"\n").unwrap();
        app.insert_resource(Keymaps::compile(&raw).unwrap());
        press(&mut app, KeyCode::Char('g'));
        let pending = app.world().resource::<PendingKeys>();
        assert_eq!(pending.hint().unwrap(), "g");
    }

    #[test]
    fn unbound_clears_and_warns() {
        let mut app = router_app();
        press(&mut app, KeyCode::Char('x'));
        assert!(app.world().resource::<PendingKeys>().keys.is_empty());
        let status = app.world().resource::<StatusMessage>();
        assert!(status.current().unwrap().0.contains("unbound"));
    }

    #[test]
    fn tab_toggles_sidebar_focus_shadowing_global_tab_next() {
        let mut app = router_app();
        app.world_mut().resource_mut::<Tabs>().labels =
            vec!["mail".to_owned(), "contacts".to_owned()];
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.world().resource::<Tabs>().active,
            0,
            "the index-context Tab binding must shadow the global tab-next"
        );
        assert!(
            app.world()
                .resource::<crate::sidebar::SidebarState>()
                .focused
        );
        press(&mut app, KeyCode::Tab);
        assert!(
            !app.world()
                .resource::<crate::sidebar::SidebarState>()
                .focused,
            "Tab in the sidebar context must return focus"
        );
    }

    #[test]
    fn burst_command_line_input_routes_across_mode_switch() {
        let mut app = router_app();
        press_str(&mut app, ":echo hi");
        assert_eq!(app.world().resource::<Mode>().0, InputMode::CommandLine);
        assert_eq!(app.world().resource::<CommandLineState>().buffer, "echo hi");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);
        let status = app.world().resource::<StatusMessage>();
        assert_eq!(status.current().unwrap().0, "hi");
    }

    #[test]
    fn burst_quit_command_exits() {
        let mut app = router_app();
        press_str(&mut app, ":quit");
        press(&mut app, KeyCode::Enter);
        assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    }
}
