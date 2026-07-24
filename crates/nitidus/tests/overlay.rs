//! Picker overlay behavior through the router: filtering, navigation,
//! confirm/cancel, modality (globals must not leak), and rebinding.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nitidus::cmdline::CommandLineState;
use nitidus::config::RawKeymaps;
use nitidus::keymap::Keymaps;
use nitidus::overlay::{ActiveOverlay, OverlayPlugin, PickerItem, PickerSpec, open_picker};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use plurimus::{TachyonRegistry, UiEvent};

#[derive(Resource, Default)]
struct Chosen(Option<usize>);

fn overlay_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.init_resource::<Chosen>();
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.add_plugins((RouterPlugin, OverlayPlugin));
    app.update();
    app
}

fn open_fruit_picker(app: &mut App) {
    let items = ["apple", "banana", "cherry"]
        .into_iter()
        .map(|label| PickerItem {
            label: label.to_owned(),
            detail: None,
        })
        .collect();
    open_picker(
        app.world_mut(),
        PickerSpec {
            title: "fruit".to_owned(),
            items,
            on_select: Box::new(|world, index| {
                world.resource_mut::<Chosen>().0 = Some(index);
            }),
        },
    );
    app.update();
}

fn press(app: &mut App, event: KeyEvent) {
    route_key(app.world_mut(), Entity::PLACEHOLDER, UiEvent::Key(event)).unwrap();
}

fn press_code(app: &mut App, code: KeyCode) {
    press(app, KeyEvent::from(code));
}

fn quit_requested(app: &App) -> bool {
    !app.world().resource::<Messages<AppExit>>().is_empty()
}

#[test]
fn typing_filters_and_enter_selects_original_index() {
    let mut app = overlay_app();
    open_fruit_picker(&mut app);
    for character in "che".chars() {
        press_code(&mut app, KeyCode::Char(character));
    }
    press_code(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<Chosen>().0, Some(2));
    assert!(!app.world().resource::<ActiveOverlay>().is_open());
}

#[test]
fn navigation_moves_selection() {
    let mut app = overlay_app();
    open_fruit_picker(&mut app);
    press_code(&mut app, KeyCode::Down);
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
    );
    press_code(&mut app, KeyCode::Up);
    press_code(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<Chosen>().0, Some(1));
}

#[test]
fn escape_cancels_and_routing_returns_to_normal() {
    let mut app = overlay_app();
    open_fruit_picker(&mut app);
    press_code(&mut app, KeyCode::Esc);
    assert!(!app.world().resource::<ActiveOverlay>().is_open());
    assert_eq!(app.world().resource::<Chosen>().0, None);
    press_code(&mut app, KeyCode::Char('q'));
    assert!(quit_requested(&app), "after close q must reach global quit");
}

#[test]
fn global_bindings_do_not_leak_into_the_picker() {
    let mut app = overlay_app();
    open_fruit_picker(&mut app);
    press_code(&mut app, KeyCode::Char('q'));
    assert!(!quit_requested(&app), "q must filter, not quit");
    press_code(&mut app, KeyCode::Backspace);
    press_code(&mut app, KeyCode::Char(':'));
    assert_eq!(
        app.world().resource::<CommandLineState>().buffer,
        "",
        "colon must filter, not open the command line"
    );
    assert!(app.world().resource::<ActiveOverlay>().is_open());
}

#[test]
fn picker_bindings_are_rebindable() {
    let mut app = overlay_app();
    let raw: RawKeymaps = toml::from_str("[picker]\n\"<C-n>\" = \":next\"\n").unwrap();
    app.insert_resource(Keymaps::compile(&raw).unwrap());
    open_fruit_picker(&mut app);
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
    );
    press_code(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<Chosen>().0, Some(1));
}

#[test]
fn picker_widget_spawns_and_despawns_with_the_overlay() {
    let mut app = overlay_app();
    let widgets_before = app
        .world_mut()
        .query::<&plurimus::Widget>()
        .iter(app.world())
        .count();
    open_fruit_picker(&mut app);
    let widgets_open = app
        .world_mut()
        .query::<&plurimus::Widget>()
        .iter(app.world())
        .count();
    assert_eq!(widgets_open, widgets_before + 1);
    press_code(&mut app, KeyCode::Esc);
    app.update();
    let widgets_closed = app
        .world_mut()
        .query::<&plurimus::Widget>()
        .iter(app.world())
        .count();
    assert_eq!(widgets_closed, widgets_before);
}
