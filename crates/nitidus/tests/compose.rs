//! Compose flow through the router: `m` runs the To → Subject → editor
//! chain into review, header re-prompts keep values, Esc discards with
//! confirmation, and abandoning mid-chain cleans up the body file.
//!
//! These pin the `$EDITOR` path, so the harness selects it explicitly;
//! the inline editor that ships as the default has its own suite.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{ComposeDir, ComposePlugin, ComposeState, EditorCommand};
use nitidus::config::account::AccountConfig;
use nitidus::config::{Config, RawKeymaps};
use nitidus::index::IndexPlugin;
use nitidus::keymap::Keymaps;
use nitidus::overlay::OverlayPlugin;
use nitidus::prompt::{PromptPlugin, PromptState};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::screen::Screen;
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use plurimus::{TachyonRegistry, UiEvent};

fn compose_app(compose_dir: &std::path::Path) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.init_resource::<MailStore>();
    app.init_resource::<SyncTracker>();
    let mut config = Config::default();
    config.ui.compose.editor = nitidus::config::EditorKind::External;
    config.accounts.push(AccountConfig {
        name: "local".to_owned(),
        email: "norman@example.com".to_owned(),
        display_name: "Norman".to_owned(),
        signature: Some("sent from nitidus".to_owned()),
        ..Default::default()
    });
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(compose_dir.to_path_buf()));
    app.insert_resource(EditorCommand(
        "printf 'typed in the editor\\n' >>".to_owned(),
    ));
    app.add_plugins((
        RouterPlugin,
        IndexPlugin,
        ComposePlugin,
        PromptPlugin,
        OverlayPlugin,
    ));
    app.update();
    app
}

fn press(app: &mut App, code: KeyCode) {
    route_key(
        app.world_mut(),
        Entity::PLACEHOLDER,
        UiEvent::Key(KeyEvent::from(code)),
    )
    .unwrap();
    app.update();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

fn compose_to_review(app: &mut App) {
    press(app, KeyCode::Char('m'));
    assert_eq!(
        app.world().resource::<PromptState>().label(),
        Some("To: "),
        "m must open the To prompt"
    );
    type_text(app, "bob@example.com");
    press(app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<PromptState>().label(),
        Some("Subject: ")
    );
    type_text(app, "hello");
    press(app, KeyCode::Enter);
}

#[test]
fn compose_chain_reaches_review_with_edited_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_to_review(&mut app);

    assert_eq!(*app.world().resource::<Screen>(), Screen::Compose);
    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.to, "bob@example.com");
    assert_eq!(session.subject, "hello");
    assert_eq!(session.from, "Norman <norman@example.com>");
    assert!(
        session
            .body
            .iter()
            .any(|line| line == "typed in the editor"),
        "the editor override must have appended to the body: {:?}",
        session.body
    );
    assert!(
        session.body.iter().any(|line| line == "sent from nitidus"),
        "the signature must be present: {:?}",
        session.body
    );
}

#[test]
fn header_reprompt_starts_from_the_current_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_to_review(&mut app);

    press(&mut app, KeyCode::Char('s'));
    assert_eq!(
        app.world().resource::<PromptState>().label(),
        Some("Subject: ")
    );
    assert_eq!(
        app.world().resource::<PromptState>().value(),
        Some("hello"),
        "re-prompts must start from the existing value"
    );
    type_text(&mut app, " world");
    press(&mut app, KeyCode::Enter);
    let state = app.world().resource::<ComposeState>();
    assert_eq!(state.session().unwrap().subject, "hello world");
    assert_eq!(*app.world().resource::<Screen>(), Screen::Compose);
}

#[test]
fn escape_discards_after_confirmation_and_deletes_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_to_review(&mut app);
    let body_path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();
    assert!(body_path.exists());

    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.world().resource::<PromptState>().label(),
        Some("Discard message? (y/n): ")
    );
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "answering n must keep the session"
    );

    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('y'));
    press(&mut app, KeyCode::Enter);
    assert!(!app.world().resource::<ComposeState>().is_active());
    assert!(!body_path.exists(), "discard must delete the body file");
    assert_eq!(*app.world().resource::<Screen>(), Screen::Index);
}

#[test]
fn escape_mid_chain_abandons_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));
    press(&mut app, KeyCode::Esc);
    assert!(!app.world().resource::<ComposeState>().is_active());
    let leftover = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftover, 0, "abandoning must remove the body file");
}

#[test]
fn m_resumes_an_existing_session_instead_of_starting_over() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_to_review(&mut app);
    let original = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();

    // Simulate leaving via a sidebar folder switch, then return with m.
    *app.world_mut().resource_mut::<Screen>() = Screen::Index;
    app.update();
    press(&mut app, KeyCode::Char('m'));
    let state = app.world().resource::<ComposeState>();
    assert_eq!(
        state.session().unwrap().body_path,
        original,
        "m must resume, not restart"
    );
    assert_eq!(*app.world().resource::<Screen>(), Screen::Compose);
}
