//! The key router: one global passthrough resolves each key against the
//! active keymap trie the moment it arrives, so bursts of input route
//! correctly across synchronous mode switches.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crokey::{KeyCombination, KeyCombinationFormat};
use plurimus::{UiActions, UiEvent, UiInputBinding, Widget};

use crate::action::apply_action;
use crate::keymap::{InputMode, KeymapMatch, Keymaps, Mode};
use crate::status::{MessageLog, expire_status_messages};

const CHORD_TIMEOUT_SECS: f64 = 0.5;

pub struct RouterPlugin;

impl Plugin for RouterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mode>();
        app.init_resource::<PendingKeys>();
        app.init_resource::<MessageLog>();
        app.init_resource::<crate::overlay::ActiveOverlay>();
        app.init_resource::<crate::overlay::form::ActiveForm>();
        app.init_resource::<crate::explorer::ExplorerState>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.init_resource::<crate::addresses::AddressIndex>();
        app.init_resource::<crate::compose::AttachPreview>();
        app.init_resource::<crate::sidebar::SidebarState>();
        app.init_resource::<crate::focus::PaneFocus>();
        app.init_resource::<crate::shell::Tabs>();
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
    if world.resource::<Mode>().0 == InputMode::Search {
        return crate::index::search::handle_key(world, key);
    }
    if let Some(handled) = crate::overlay::surface::route_key(world, key) {
        return handled;
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
        let context = crate::focus::active_layers(world);
        let keymaps = world.resource::<Keymaps>();
        let pending = world.resource::<PendingKeys>();
        keymaps.resolve_layered(&context, &pending.keys)
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
                .resource_mut::<MessageLog>()
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
        let log = app.world().resource::<MessageLog>();
        assert!(
            log.entries()
                .last()
                .is_some_and(|entry| entry.text.contains("unbound")),
            "an unbound key must be reported"
        );
        assert_eq!(
            log.current(),
            None,
            "a warning belongs in a toast, not the status row"
        );
    }

    #[test]
    fn tab_toggles_sidebar_focus_and_never_switches_tabs() {
        let mut app = router_app();
        app.world_mut().resource_mut::<Tabs>().labels =
            vec!["mail".to_owned(), "contacts".to_owned()];
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.world().resource::<Tabs>().active,
            0,
            "Tab is the local focus key, never tab switching"
        );
        assert!(crate::focus::is_focused(
            app.world(),
            crate::focus::Pane::Folders
        ));
        press(&mut app, KeyCode::Tab);
        assert!(
            !crate::focus::is_focused(app.world(), crate::focus::Pane::Folders),
            "Tab in the sidebar context must return focus"
        );
    }

    /// Focus is stored per tab, so a focused mail pane cannot select the
    /// sidebar context while the contact book is on screen — the leak the
    /// old global `SidebarState.focused` flag had to be cleared to avoid.
    #[test]
    fn a_focused_mail_pane_does_not_claim_the_contacts_context() {
        let mut app = router_app();
        crate::focus::focus(app.world_mut(), crate::focus::Pane::Folders);
        app.world_mut().resource_mut::<Tabs>().active = 1;

        press(&mut app, KeyCode::Tab);

        assert!(
            crate::focus::is_focused(app.world(), crate::focus::Pane::ContactDetail),
            "Tab must resolve against the contacts context and move the detail focus"
        );
        assert!(
            crate::focus::is_focused(app.world(), crate::focus::Pane::Folders),
            "the mail tab's own focus must survive untouched"
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
        assert_eq!(app.world().resource::<MessageLog>().current(), Some("hi"));
    }

    #[test]
    fn burst_quit_command_exits() {
        let mut app = router_app();
        press_str(&mut app, ":quit");
        press(&mut app, KeyCode::Enter);
        assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    }
}
