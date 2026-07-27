//! Help overlay behavior through the router: `?` opens the current
//! context's bindings, Tab toggles to all contexts, Enter executes the
//! selected row, and non-help pickers ignore the scope toggle.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::cmdline::CommandLineState;
use nitidus::config::RawKeymaps;
use nitidus::keymap::Keymaps;
use nitidus::overlay::{ActiveOverlay, OverlayPlugin, PickerItem, PickerSpec, open_picker};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use plurimus::{TachyonRegistry, UiEvent};

fn help_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.add_plugins((RouterPlugin, OverlayPlugin));
    app.update();
    app
}

fn press_code(app: &mut App, code: KeyCode) {
    route_key(
        app.world_mut(),
        Entity::PLACEHOLDER,
        UiEvent::Key(KeyEvent::from(code)),
    )
    .unwrap();
    app.update();
}

fn visible_labels(app: &App) -> Vec<String> {
    app.world()
        .resource::<ActiveOverlay>()
        .visible_items()
        .iter()
        .map(|item| item.label.clone())
        .collect()
}

#[test]
fn question_mark_opens_current_context_bindings_with_summaries() {
    let mut app = help_app();
    press_code(&mut app, KeyCode::Char('?'));

    let overlay = app.world().resource::<ActiveOverlay>();
    assert!(overlay.is_open());
    assert_eq!(overlay.title(), Some("keys — index"));
    let items = overlay.visible_items();
    let fold_all = items
        .iter()
        .find(|item| item.label.contains("fold-all"))
        .expect("index bindings must list fold-all");
    assert!(fold_all.label.starts_with("zM"), "{}", fold_all.label);
    assert_eq!(fold_all.detail.as_deref(), Some("collapse everything"));
    let quit = items
        .iter()
        .find(|item| item.label.contains("quit"))
        .expect("unshadowed globals must appear");
    assert!(
        quit.detail
            .as_deref()
            .unwrap_or_default()
            .ends_with("(global)"),
        "{quit:?}"
    );
    let tab_next = items
        .iter()
        .find(|item| item.label.contains("tab-next"))
        .expect("] tab-next is an unshadowed global now");
    assert!(
        tab_next.label.starts_with(']'),
        "tab switching moved to the brackets: {}",
        tab_next.label
    );
}

#[test]
fn tab_toggles_between_context_and_all_scopes() {
    let mut app = help_app();
    press_code(&mut app, KeyCode::Char('?'));
    press_code(&mut app, KeyCode::Tab);

    let overlay = app.world().resource::<ActiveOverlay>();
    assert_eq!(overlay.title(), Some("keys — all"));
    let labels = visible_labels(&app);
    assert!(
        labels.iter().any(|label| label.starts_with("[pager]")),
        "all-scope must group other contexts: {labels:?}"
    );

    press_code(&mut app, KeyCode::Tab);
    assert_eq!(
        app.world().resource::<ActiveOverlay>().title(),
        Some("keys — index")
    );
}

#[test]
fn enter_executes_the_selected_binding() {
    let mut app = help_app();
    press_code(&mut app, KeyCode::Char('?'));
    for character in "sidebar".chars() {
        press_code(&mut app, KeyCode::Char(character));
    }
    let labels = visible_labels(&app);
    assert!(
        labels
            .first()
            .is_some_and(|label| label.contains("sidebar")),
        "filter must surface the sidebar toggle: {labels:?}"
    );
    let expects_focus = labels
        .first()
        .is_some_and(|label| label.contains("sidebar-focus"));

    press_code(&mut app, KeyCode::Enter);
    assert!(!app.world().resource::<ActiveOverlay>().is_open());
    assert!(
        if expects_focus {
            nitidus::focus::is_focused(app.world(), nitidus::focus::Pane::Folders)
        } else {
            !app.world()
                .resource::<nitidus::sidebar::SidebarState>()
                .visible
        },
        "the selected sidebar command must have executed"
    );
}

#[test]
fn scope_toggle_ignores_non_help_pickers() {
    let mut app = help_app();
    open_picker(
        app.world_mut(),
        PickerSpec {
            title: "fruit".to_owned(),
            items: vec![PickerItem {
                label: "apple".to_owned(),
                detail: None,
            }],
            on_select: Box::new(|_, _| {}),
        },
    );
    app.update();
    press_code(&mut app, KeyCode::Tab);

    let overlay = app.world().resource::<ActiveOverlay>();
    assert!(overlay.is_open());
    assert_eq!(
        overlay.title(),
        Some("fruit"),
        "fruit picker must survive Tab"
    );
}
