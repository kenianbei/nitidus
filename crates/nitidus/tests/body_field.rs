//! The composer's body field: typing reaches the buffer, the bound keys
//! drive the widget, and every keystroke reaches the session and its
//! crash-survival file without anything having to be closed first.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{AttachPreview, ComposeDir, ComposePlugin, ComposeState};
use nitidus::config::account::AccountConfig;
use nitidus::config::{Config, RawKeymaps};
use nitidus::index::IndexPlugin;
use nitidus::keymap::Keymaps;
use nitidus::overlay::OverlayPlugin;
use nitidus::overlay::form::ActiveForm;
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use plurimus::{TachyonRegistry, UiEvent};

fn editor_app(compose_dir: &std::path::Path) -> App {
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
        ..Default::default()
    });
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(compose_dir.to_path_buf()));
    app.add_plugins((RouterPlugin, IndexPlugin, ComposePlugin, OverlayPlugin));
    app.update();
    app
}

fn press(app: &mut App, code: KeyCode) {
    send(app, KeyEvent::from(code));
}

fn press_ctrl(app: &mut App, code: KeyCode) {
    send(app, KeyEvent::new(code, KeyModifiers::CONTROL));
}

fn press_alt(app: &mut App, code: KeyCode) {
    send(app, KeyEvent::new(code, KeyModifiers::ALT));
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

fn send(app: &mut App, key: KeyEvent) {
    route_key(app.world_mut(), Entity::PLACEHOLDER, UiEvent::Key(key)).unwrap();
    app.update();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// Opens the composer and tabs from To down to the body, past Cc, Bcc,
/// Subject and the attachment row.
fn compose_into_the_body(app: &mut App) {
    press(app, KeyCode::Char('m'));
    assert!(app.world().resource::<ActiveForm>().is_open());
    type_text(app, "bob@example.com");
    for _ in 0..5 {
        press(app, KeyCode::Tab);
    }
}

/// The session's body, which the form writes through to on every change.
fn body(app: &App) -> Vec<String> {
    app.world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body
        .clone()
}

fn body_file(app: &App) -> String {
    let path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();
    std::fs::read_to_string(path).unwrap()
}

fn attachment_count(app: &App) -> usize {
    app.world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .attachments
        .len()
}

#[test]
fn typing_reaches_the_session_without_leaving_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "hi there");

    assert!(
        body(&app).iter().any(|line| line.contains("hi there")),
        "the typed text must reach the session: {:?}",
        body(&app)
    );
}

/// The buffer is the truth and the file is the copy a crash leaves
/// behind: the first change reaches it at once, and the burst behind
/// that one catches up a beat later rather than costing a write per
/// character.
#[test]
fn the_first_change_reaches_the_crash_survival_file_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "x");

    assert!(
        body_file(&app).contains('x'),
        "the first keystroke must not wait: {:?}",
        body_file(&app)
    );
}

#[test]
fn the_rest_of_a_burst_catches_up_within_the_write_interval() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);
    type_text(&mut app, "saved to disk");

    std::thread::sleep(Duration::from_millis(300));
    app.update();

    assert!(
        body_file(&app).contains("saved to disk"),
        "the file must catch up with the buffer: {:?}",
        body_file(&app)
    );
}

#[test]
fn enter_opens_a_new_line_rather_than_confirming() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "first");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "second");

    let body = body(&app);
    assert!(body.iter().any(|line| line == "first"), "{body:?}");
    assert!(body.iter().any(|line| line == "second"), "{body:?}");
}

#[test]
fn backspace_deletes_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "abc");
    press(&mut app, KeyCode::Backspace);
    press_ctrl(&mut app, KeyCode::Char('z'));

    assert!(
        body(&app).iter().any(|line| line.contains("abc")),
        "undo must restore the deleted character: {:?}",
        body(&app)
    );
}

#[test]
fn an_unbound_control_chord_does_not_type_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "ab");
    press_ctrl(&mut app, KeyCode::Char('q'));

    assert!(
        body(&app).iter().all(|line| !line.contains('q')),
        "an unbound chord must be swallowed, not inserted: {:?}",
        body(&app)
    );
}

/// The composer's own commands are all Alt chords precisely so that
/// every letter stays a letter.
#[test]
fn the_letters_the_composer_used_to_bind_are_now_just_letters() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "yay");

    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "typing must not have triggered send"
    );
    assert!(
        body(&app).iter().any(|line| line.contains("yay")),
        "{:?}",
        body(&app)
    );
}

#[test]
fn word_motions_move_by_word() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "alpha beta");
    press_ctrl(&mut app, KeyCode::Left);
    type_text(&mut app, "X");

    assert!(
        body(&app).iter().any(|line| line.contains("alpha Xbeta")),
        "Ctrl-Left must land at the start of the last word: {:?}",
        body(&app)
    );
}

#[test]
fn home_and_end_move_within_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "middle");
    press(&mut app, KeyCode::Home);
    type_text(&mut app, ">");
    press(&mut app, KeyCode::End);
    type_text(&mut app, "<");

    assert!(
        body(&app).iter().any(|line| line.contains(">middle<")),
        "{:?}",
        body(&app)
    );
}

#[test]
fn delete_word_back_removes_the_previous_word() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "keep drop");
    press_ctrl(&mut app, KeyCode::Backspace);

    let lines = body(&app);
    assert!(lines.iter().any(|line| line.contains("keep")), "{lines:?}");
    assert!(
        lines.iter().all(|line| !line.contains("drop")),
        "the word before the cursor must be gone: {lines:?}"
    );
}

#[test]
fn select_all_then_typing_replaces_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "original");
    press_ctrl(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "fresh");

    let lines = body(&app);
    assert!(lines.iter().any(|line| line.contains("fresh")), "{lines:?}");
    assert!(
        lines.iter().all(|line| !line.contains("original")),
        "the selection must have been replaced: {lines:?}"
    );
}

#[test]
fn cut_removes_the_selection_and_paste_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "cutme");
    press_ctrl(&mut app, KeyCode::Char('a'));
    press_ctrl(&mut app, KeyCode::Char('x'));
    assert!(
        body(&app).iter().all(|line| !line.contains("cutme")),
        "cut must empty the selection: {:?}",
        body(&app)
    );

    press_ctrl(&mut app, KeyCode::Char('v'));
    assert!(
        body(&app).iter().any(|line| line.contains("cutme")),
        "paste must bring the cut text back: {:?}",
        body(&app)
    );
}

#[test]
fn delete_to_end_of_line_keeps_the_head() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "head tail");
    press(&mut app, KeyCode::Home);
    for _ in 0..5 {
        press(&mut app, KeyCode::Right);
    }
    press_ctrl(&mut app, KeyCode::Char('k'));

    let lines = body(&app);
    assert!(lines.iter().any(|line| line.contains("head ")), "{lines:?}");
    assert!(lines.iter().all(|line| !line.contains("tail")), "{lines:?}");
}

/// A token is a placement, not the attachment itself: what is attached
/// is what the attachment row holds.
#[test]
fn a_typed_token_attaches_nothing_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    app.update();

    assert_eq!(
        attachment_count(&app),
        0,
        "attaching is done on the attachment row, not by typing"
    );
}

#[test]
fn an_attached_file_survives_deleting_its_token() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);
    attach(&mut app, &file);
    press_alt(&mut app, KeyCode::Char('i'));
    assert!(
        body(&app).iter().any(|line| line.contains("[[attach:")),
        "the placement token went into the body: {:?}",
        body(&app)
    );

    press_ctrl(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Backspace);
    app.update();

    assert_eq!(
        attachment_count(&app),
        1,
        "removing a placement must not detach the file"
    );
}

#[test]
fn a_token_with_attributes_survives_editing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, "png").unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(
        &mut app,
        &format!("[[attach: {} | width=40]]", file.display()),
    );
    app.update();

    assert!(
        body(&app).iter().any(|line| line.contains("width=40")),
        "unknown attributes must survive verbatim: {:?}",
        body(&app)
    );
}

#[test]
fn previewing_a_token_line_opens_the_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, "not really a png").unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press_ctrl(&mut app, KeyCode::Char('p'));

    assert!(
        app.world().resource::<AttachPreview>().is_open(),
        "the token under the cursor must open a preview"
    );

    // Any key dismisses, and input returns to the body.
    press(&mut app, KeyCode::Char('x'));
    assert!(!app.world().resource::<AttachPreview>().is_open());
    type_text(&mut app, "!");
    assert!(
        body(&app).iter().any(|line| line.ends_with('!')),
        "the body has the keyboard again: {:?}",
        body(&app)
    );
}

#[test]
fn previewing_a_line_without_a_token_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, "just prose");
    press_ctrl(&mut app, KeyCode::Char('p'));

    assert!(
        !app.world().resource::<AttachPreview>().is_open(),
        "prose is not an attachment"
    );
}

#[test]
fn a_dismissing_key_is_not_typed_into_the_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, "x").unwrap();
    let mut app = editor_app(dir.path());
    compose_into_the_body(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press_ctrl(&mut app, KeyCode::Char('p'));
    press(&mut app, KeyCode::Char('z'));

    assert!(
        body(&app).iter().all(|line| !line.ends_with('z')),
        "the key that closed the overlay must not also type: {:?}",
        body(&app)
    );
}
