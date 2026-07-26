//! The inline body editor: the compose chain lands in it by default,
//! typing reaches the buffer, bound keys drive the widget, and leaving
//! writes the body back to the session and its crash-survival file.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{AttachPreview, ComposeDir, ComposePlugin, ComposeState, InlineEditor};
use nitidus::config::account::AccountConfig;
use nitidus::config::{Config, EditorKind, RawKeymaps};
use nitidus::index::IndexPlugin;
use nitidus::keymap::{InputMode, Keymaps, Mode};
use nitidus::overlay::OverlayPlugin;
use nitidus::prompt::{PromptPlugin, PromptState};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::screen::Screen;
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use plurimus::{TachyonRegistry, UiEvent};

fn editor_app(compose_dir: &std::path::Path, kind: EditorKind) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.init_resource::<MailStore>();
    app.init_resource::<SyncTracker>();
    let mut config = Config::default();
    config.ui.compose.editor = kind;
    config.accounts.push(AccountConfig {
        name: "local".to_owned(),
        email: "norman@example.com".to_owned(),
        display_name: "Norman".to_owned(),
        ..Default::default()
    });
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(compose_dir.to_path_buf()));
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

fn press_ctrl(app: &mut App, code: KeyCode) {
    route_key(
        app.world_mut(),
        Entity::PLACEHOLDER,
        UiEvent::Key(KeyEvent::new(code, KeyModifiers::CONTROL)),
    )
    .unwrap();
    app.update();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// Runs the To → Subject chain, which lands in the editor by default.
fn compose_into_editor(app: &mut App) {
    press(app, KeyCode::Char('m'));
    type_text(app, "bob@example.com");
    press(app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<PromptState>().label(),
        Some("Subject: ")
    );
    type_text(app, "hello");
    press(app, KeyCode::Enter);
}

fn body(app: &App) -> Vec<String> {
    app.world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body
        .clone()
}

/// The live buffer, for assertions made before leaving the editor.
fn body_now(app: &App) -> Vec<String> {
    app.world().resource::<InlineEditor>().lines().unwrap()
}

#[test]
fn the_compose_chain_lands_in_the_inline_editor() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    assert_eq!(app.world().resource::<Mode>().0, InputMode::Editor);
    assert!(app.world().resource::<InlineEditor>().is_active());
    assert_eq!(*app.world().resource::<Screen>(), Screen::Compose);
}

#[test]
fn external_configuration_keeps_the_editor_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::External);
    compose_into_editor(&mut app);

    assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);
    assert!(
        !app.world().resource::<InlineEditor>().is_active(),
        "ui.compose.editor = external must not open the inline editor"
    );
}

#[test]
fn typing_reaches_the_buffer_and_leaving_writes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "hi there");
    press(&mut app, KeyCode::Esc);

    assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);
    assert!(!app.world().resource::<InlineEditor>().is_active());
    assert!(
        body(&app).iter().any(|line| line.contains("hi there")),
        "the typed text must reach the session: {:?}",
        body(&app)
    );
}

#[test]
fn leaving_writes_the_crash_survival_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);
    type_text(&mut app, "saved to disk");
    press(&mut app, KeyCode::Esc);

    let path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();
    let written = std::fs::read_to_string(path).unwrap();
    assert!(
        written.contains("saved to disk"),
        "the body file must match the buffer: {written:?}"
    );
}

#[test]
fn enter_opens_a_new_line_rather_than_confirming() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "first");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "second");
    press(&mut app, KeyCode::Esc);

    let body = body(&app);
    assert!(body.iter().any(|line| line == "first"), "{body:?}");
    assert!(body.iter().any(|line| line == "second"), "{body:?}");
}

#[test]
fn backspace_deletes_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "abc");
    press(&mut app, KeyCode::Backspace);
    press_ctrl(&mut app, KeyCode::Char('z'));
    press(&mut app, KeyCode::Esc);

    assert!(
        body(&app).iter().any(|line| line.contains("abc")),
        "undo must restore the deleted character: {:?}",
        body(&app)
    );
}

#[test]
fn an_unbound_control_chord_does_not_type_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "ab");
    press_ctrl(&mut app, KeyCode::Char('q'));
    press(&mut app, KeyCode::Esc);

    assert!(
        body(&app).iter().all(|line| !line.contains('q')),
        "an unbound chord must be swallowed, not inserted: {:?}",
        body(&app)
    );
}

#[test]
fn the_editor_owns_keys_that_the_review_screen_binds() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    // `y` sends and `a` attaches on the review screen; in the editor they
    // are just letters.
    type_text(&mut app, "yay");
    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "typing must not have triggered send"
    );
    press(&mut app, KeyCode::Esc);
    assert!(
        body(&app).iter().any(|line| line.contains("yay")),
        "{:?}",
        body(&app)
    );
}

#[test]
fn compose_edit_reopens_the_editor_from_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.world().resource::<Mode>().0, InputMode::Normal);

    press(&mut app, KeyCode::Char('e'));
    assert_eq!(app.world().resource::<Mode>().0, InputMode::Editor);
    assert!(app.world().resource::<InlineEditor>().is_active());
}

#[test]
fn word_motions_move_by_word() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "alpha beta");
    press_ctrl(&mut app, KeyCode::Left);
    type_text(&mut app, "X");
    press(&mut app, KeyCode::Esc);

    assert!(
        body(&app).iter().any(|line| line.contains("alpha Xbeta")),
        "Ctrl-Left must land at the start of the last word: {:?}",
        body(&app)
    );
}

#[test]
fn home_and_end_move_within_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "middle");
    press(&mut app, KeyCode::Home);
    type_text(&mut app, ">");
    press(&mut app, KeyCode::End);
    type_text(&mut app, "<");
    press(&mut app, KeyCode::Esc);

    assert!(
        body(&app).iter().any(|line| line.contains(">middle<")),
        "{:?}",
        body(&app)
    );
}

#[test]
fn delete_word_back_removes_the_previous_word() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "keep drop");
    press_ctrl(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Esc);

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
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "original");
    press_ctrl(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "fresh");
    press(&mut app, KeyCode::Esc);

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
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "cutme");
    press_ctrl(&mut app, KeyCode::Char('a'));
    press_ctrl(&mut app, KeyCode::Char('x'));
    assert!(
        body_now(&app).iter().all(|line| !line.contains("cutme")),
        "cut must empty the selection: {:?}",
        body_now(&app)
    );

    press_ctrl(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Esc);
    assert!(
        body(&app).iter().any(|line| line.contains("cutme")),
        "paste must bring the cut text back: {:?}",
        body(&app)
    );
}

#[test]
fn delete_to_end_of_line_keeps_the_head() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, "head tail");
    press(&mut app, KeyCode::Home);
    for _ in 0..5 {
        press(&mut app, KeyCode::Right);
    }
    press_ctrl(&mut app, KeyCode::Char('k'));
    press(&mut app, KeyCode::Esc);

    let lines = body(&app);
    assert!(lines.iter().any(|line| line.contains("head ")), "{lines:?}");
    assert!(lines.iter().all(|line| !line.contains("tail")), "{lines:?}");
}

#[test]
fn a_typed_token_registers_as_an_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press(&mut app, KeyCode::Esc);
    app.update();

    let attachments = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .attachments
        .clone();
    assert_eq!(
        attachments,
        vec![file.clone()],
        "the body is what declares an attachment"
    );
}

#[test]
fn deleting_the_token_deregisters_the_attachment() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, "hi").unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press(&mut app, KeyCode::Esc);
    app.update();
    assert_eq!(attachment_count(&app), 1);

    press(&mut app, KeyCode::Char('e'));
    press_ctrl(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Esc);
    app.update();

    assert_eq!(
        attachment_count(&app),
        0,
        "removing the token must drop the attachment"
    );
}

#[test]
fn a_token_with_attributes_still_registers_and_survives_editing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, "png").unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(
        &mut app,
        &format!("[[attach: {} | width=40]]", file.display()),
    );
    press(&mut app, KeyCode::Esc);
    app.update();

    assert_eq!(attachment_count(&app), 1);
    assert!(
        body(&app).iter().any(|line| line.contains("width=40")),
        "unknown attributes must survive verbatim: {:?}",
        body(&app)
    );
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
fn previewing_a_token_line_opens_the_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, "not really a png").unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press_ctrl(&mut app, KeyCode::Char('p'));

    assert!(
        app.world().resource::<AttachPreview>().is_open(),
        "the token under the cursor must open a preview"
    );

    // Any key dismisses, and input returns to the editor.
    press(&mut app, KeyCode::Char('x'));
    assert!(!app.world().resource::<AttachPreview>().is_open());
    assert_eq!(app.world().resource::<Mode>().0, InputMode::Editor);
}

#[test]
fn previewing_a_line_without_a_token_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

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
    let mut app = editor_app(dir.path(), EditorKind::Inline);
    compose_into_editor(&mut app);

    type_text(&mut app, &format!("[[attach: {}]]", file.display()));
    press_ctrl(&mut app, KeyCode::Char('p'));
    press(&mut app, KeyCode::Char('z'));

    assert!(
        body_now(&app).iter().all(|line| !line.ends_with('z')),
        "the key that closed the overlay must not also type: {:?}",
        body_now(&app)
    );
}
