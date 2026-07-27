//! Compose through the router: `m` opens one form whose headers and
//! body are all tab stops, what you type reaches the session, Esc asks
//! before discarding, and `$EDITOR` still works as the escape hatch.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{ComposeDir, ComposePlugin, ComposeState, EditorCommand};
use nitidus::config::account::AccountConfig;
use nitidus::config::{Config, RawKeymaps};
use nitidus::index::IndexPlugin;
use nitidus::keymap::Keymaps;
use nitidus::overlay::form::ActiveForm;
use nitidus::overlay::{ActiveOverlay, OverlayPlugin};
use nitidus::router::{RouterPlugin, route_key};
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
    app.add_plugins((RouterPlugin, IndexPlugin, ComposePlugin, OverlayPlugin));
    app.update();
    app
}

fn press(app: &mut App, code: KeyCode) {
    send(app, KeyEvent::from(code));
}

fn press_alt(app: &mut App, code: KeyCode) {
    send(app, KeyEvent::new(code, KeyModifiers::ALT));
}

fn send(app: &mut App, key: KeyEvent) {
    route_key(app.world_mut(), Entity::PLACEHOLDER, UiEvent::Key(key)).unwrap();
    app.update();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// Attaching goes through the file browser in the app; a test puts the
/// path on the row directly.
fn attach(app: &mut App, path: &std::path::Path) {
    let added = nitidus::overlay::form::push_entry(
        app.world_mut(),
        "attachments",
        path.display().to_string(),
    );
    assert!(added, "the attachment row refused {}", path.display());
    app.update();
}

fn value(app: &App, id: &str) -> String {
    app.world().resource::<ActiveForm>().value(id).unwrap()
}

/// `m`, then To, Cc, Bcc, Subject, attachments, body — the tab order,
/// filled in.
fn compose_a_message(app: &mut App) {
    press(app, KeyCode::Char('m'));
    type_text(app, "bob@example.com");
    for _ in 0..3 {
        press(app, KeyCode::Tab);
    }
    type_text(app, "hello");
    press(app, KeyCode::Tab);
    press(app, KeyCode::Tab);
    type_text(app, "body text");
}

#[test]
fn one_form_holds_every_header_and_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());

    compose_a_message(&mut app);

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.to, "bob@example.com");
    assert_eq!(session.subject, "hello");
    assert_eq!(session.from, "Norman <norman@example.com>");
    assert!(
        session.body.iter().any(|line| line.contains("body text")),
        "typing in the body must reach the session: {:?}",
        session.body
    );
    assert!(
        session.body.iter().any(|line| line == "sent from nitidus"),
        "the signature is still there: {:?}",
        session.body
    );
}

/// The tab order is From, To, Cc, Bcc, Subject, body — so a Cc is one
/// stop away rather than a binding you have to remember.
#[test]
fn cc_and_bcc_are_tab_stops_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));

    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "carol@example.com");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "dan@example.com");

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.cc, "carol@example.com");
    assert_eq!(session.bcc, "dan@example.com");
    assert_eq!(session.to, "", "To was left alone");
}

#[test]
fn a_new_message_lands_in_to_and_from_refuses_to_be_typed_into() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));

    type_text(&mut app, "x");
    assert_eq!(value(&app, "to"), "x", "a new message starts in To");

    // Shift-Tab back onto From, which is the account's identity and not
    // an answer the form collects.
    send(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    );
    type_text(&mut app, "zzz");

    assert_eq!(
        value(&app, "from"),
        "Norman <norman@example.com>",
        "From is read-only"
    );
}

#[test]
fn enter_in_a_header_steps_forward_instead_of_sending() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));
    type_text(&mut app, "bob@example.com");

    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "carol@example.com");

    assert_eq!(value(&app, "cc"), "carol@example.com");
    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "Enter must never send from a header field"
    );
}

#[test]
fn enter_in_the_body_breaks_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);

    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "second");

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert!(
        session.body.iter().any(|line| line == "second"),
        "the newline made a line of its own: {:?}",
        session.body
    );
    assert!(app.world().resource::<ComposeState>().is_active());
}

#[test]
fn escape_asks_before_discarding_and_keeps_the_form_open_on_no() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);
    let body_path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();
    assert!(body_path.exists());

    press(&mut app, KeyCode::Esc);
    assert!(
        app.world()
            .resource::<nitidus::overlay::confirm::ActiveConfirm>()
            .is_open(),
        "Esc must ask before discarding"
    );
    press(&mut app, KeyCode::Char('n'));
    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "answering n keeps the session"
    );
    assert!(
        app.world().resource::<ActiveForm>().is_open(),
        "and the form it was written in"
    );
    assert_eq!(value(&app, "subject"), "hello", "with what was typed");

    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('y'));
    assert!(!app.world().resource::<ComposeState>().is_active());
    assert!(
        !app.world().resource::<ActiveForm>().is_open(),
        "discarding closes the composer"
    );
    assert!(!body_path.exists(), "discard must delete the body file");
}

#[test]
fn m_resumes_an_existing_session_instead_of_starting_over() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);
    let original = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();

    press(&mut app, KeyCode::Char('m'));

    let state = app.world().resource::<ComposeState>();
    assert_eq!(
        state.session().unwrap().body_path,
        original,
        "m must resume, not restart"
    );
}

/// `$EDITOR` rewrites the body file behind the form's back, so the form
/// has to be rebuilt from the session afterwards.
#[test]
fn the_external_editor_still_edits_the_body_and_its_text_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);

    press_alt(&mut app, KeyCode::Char('e'));

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert!(
        session
            .body
            .iter()
            .any(|line| line == "typed in the editor"),
        "the editor appended to the body: {:?}",
        session.body
    );
    assert!(
        value(&app, "body").contains("typed in the editor"),
        "and the form shows what the editor wrote"
    );
}

/// The commands that are not on a button are discoverable through help
/// rather than a border that crops them. F1 reaches it from inside the
/// form and lists what will actually fire — the composer's own commands
/// and the form's, but no globals, which a form never falls through to.
#[test]
fn f1_opens_help_on_the_bindings_the_composer_answers() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));

    press(&mut app, KeyCode::F(1));

    let overlay = app.world().resource::<ActiveOverlay>();
    assert!(overlay.is_open());
    assert_eq!(overlay.title(), Some("keys — form · compose"));
    let labels: Vec<String> = overlay
        .visible_items()
        .iter()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        labels.iter().any(|label| label.contains("postpone")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|label| label.contains("form-focus-next")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("quit")),
        "a global that cannot fire must not be listed: {labels:?}"
    );
}

/// `~` opens help everywhere else, and must not here: it is a printable
/// the composer has to be able to type.
#[test]
fn the_tilde_help_key_still_types_into_a_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));

    press(&mut app, KeyCode::Char('~'));

    assert!(!app.world().resource::<ActiveOverlay>().is_open());
    assert_eq!(value(&app, "to"), "~");
}

/// Attachments are a row of their own between Subject and the body:
/// one tab stop, stepped through with Left and Right.
#[test]
fn the_attachment_row_holds_what_is_attached() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("one.txt");
    let second = dir.path().join("two.txt");
    std::fs::write(&first, "1").unwrap();
    std::fs::write(&second, "2").unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));

    attach(&mut app, &first);
    attach(&mut app, &second);

    let state = app.world().resource::<ComposeState>();
    assert_eq!(
        state.session().unwrap().attachments,
        vec![first.clone(), second.clone()],
        "the row is what declares an attachment"
    );
}

/// The placement is separate from the attachment: Alt-i puts a token
/// where the caret is, and the file stays attached either way.
#[test]
fn alt_i_places_the_picked_attachment_at_the_caret() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);
    attach(&mut app, &file);

    press_alt(&mut app, KeyCode::Char('i'));

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert!(
        session
            .body
            .iter()
            .any(|line| line.contains("[[attach:") && line.contains("notes.txt")),
        "the token marks where it belongs: {:?}",
        session.body
    );
    assert_eq!(session.attachments, vec![file], "and it stays attached");
}

/// Detaching takes the file off the row and its placement out of the
/// body with it.
#[test]
fn alt_d_detaches_the_picked_attachment_and_its_token() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = compose_app(dir.path());
    compose_a_message(&mut app);
    attach(&mut app, &file);
    press_alt(&mut app, KeyCode::Char('i'));

    press_alt(&mut app, KeyCode::Char('d'));

    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert!(session.attachments.is_empty(), "the row is empty again");
    assert!(
        session.body.iter().all(|line| !line.contains("[[attach:")),
        "and the placement went with it: {:?}",
        session.body
    );
}

/// Delete on the row does what Alt-d does, without leaving the field.
#[test]
fn delete_on_the_attachment_row_detaches() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = compose_app(dir.path());
    press(&mut app, KeyCode::Char('m'));
    attach(&mut app, &file);
    // From, To, Cc, Bcc, Subject, attachments: four tabs from To.
    for _ in 0..4 {
        press(&mut app, KeyCode::Tab);
    }

    press(&mut app, KeyCode::Delete);

    let state = app.world().resource::<ComposeState>();
    assert!(state.session().unwrap().attachments.is_empty());
}
