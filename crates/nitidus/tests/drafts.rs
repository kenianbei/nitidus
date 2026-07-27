//! Drafts end to end: postpone writes a Bcc-and-attachment-preserving
//! draft into the drafts folder, recall reconstructs the session,
//! re-postpone replaces the old draft, crash recovery restores from
//! sidecars, and the send warnings gate `y`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{ComposeDir, ComposePlugin, ComposeState, EditorCommand};
use nitidus::config::account::{AccountConfig, Outgoing, SendmailOutgoing};
use nitidus::config::{Config, RawKeymaps};
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView};
use nitidus::keymap::Keymaps;
use nitidus::outbox::{OutboxDir, OutboxPlugin, SendDelay};
use nitidus::overlay::OverlayPlugin;
use nitidus::overlay::form::ActiveForm;
use nitidus::pager::PagerPlugin;
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::maildir::MaildirBackend;
use nitidus_mail::{AccountId, FolderId, MailEngine};
use plurimus::{TachyonRegistry, UiEvent};

fn make_maildir(root: &Path) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
}

struct Harness {
    _root: tempfile::TempDir,
    mail_root: std::path::PathBuf,
    compose: std::path::PathBuf,
    attachment: std::path::PathBuf,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().unwrap();
    let mail_root = root.path().join("mail");
    make_maildir(&mail_root);
    make_maildir(&mail_root.join(".Drafts"));
    let attachment = root.path().join("plan.txt");
    std::fs::write(&attachment, "the plan").unwrap();
    Harness {
        compose: root.path().join("compose"),
        attachment,
        mail_root,
        _root: root,
    }
}

fn drafts_app(harness: &Harness) -> App {
    let account = AccountId::new("local");
    let mut engine = MailEngine::new(1).unwrap();
    engine.add_account(
        account.clone(),
        MaildirBackend::new(harness.mail_root.clone()).unwrap(),
    );
    engine
        .send(&account, nitidus_mail::MailCommand::ListFolders)
        .unwrap();
    let mut tracker = SyncTracker::default();
    nitidus::bootstrap::request_sync(&engine, &mut tracker, &account, &FolderId::new("INBOX"))
        .unwrap();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.init_resource::<MailStore>();
    let mut config = Config::default();
    // These drive the composer through `EditorCommand`, so they pin
    // the `$EDITOR` path rather than the inline default.
    let mut account_config = AccountConfig {
        name: "local".to_owned(),
        email: "norman@example.com".to_owned(),
        outgoing: Some(Outgoing::Sendmail(SendmailOutgoing {
            command: "true".to_owned(),
        })),
        ..Default::default()
    };
    account_config.folders.drafts = ".Drafts".to_owned();
    account_config.folders.save_sent = false;
    config.accounts.push(account_config);
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(harness.compose.clone()));
    app.insert_resource(OutboxDir(harness._root.path().join("outbox")));
    app.insert_resource(SendDelay(Duration::from_millis(40)));
    app.insert_resource(EditorCommand("true".to_owned()));
    app.insert_resource(EngineResource(engine));
    app.insert_resource(tracker);
    app.add_plugins((
        RouterPlugin,
        IndexPlugin,
        PagerPlugin,
        ComposePlugin,
        OutboxPlugin,
        OverlayPlugin,
        EnginePlugin,
    ));
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

fn wait_for(app: &mut App, mut is_done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..400 {
        app.update();
        if is_done(app.world()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// To, Bcc, Subject and an attachment, walked in tab order. Attaching
/// itself goes through the file browser in the app; here the path joins
/// the attachment row directly.
fn stage_with_attachment(app: &mut App, harness: &Harness) {
    press(app, KeyCode::Char('m'));
    assert!(
        app.world().resource::<ActiveForm>().is_open(),
        "m must open the composer"
    );
    type_text(app, "bob@example.com");
    press(app, KeyCode::Tab);
    press(app, KeyCode::Tab);
    type_text(app, "secret@example.com");
    press(app, KeyCode::Tab);
    type_text(app, "draft test");
    assert!(nitidus::overlay::form::push_entry(
        app.world_mut(),
        "attachments",
        harness.attachment.display().to_string(),
    ));
    app.update();
    let state = app.world().resource::<ComposeState>();
    assert_eq!(state.session().unwrap().attachments.len(), 1);
}

fn drafts_dir_count(harness: &Harness) -> usize {
    std::fs::read_dir(harness.mail_root.join(".Drafts/cur")).map_or(0, |entries| entries.count())
}

fn switch_to_drafts(app: &mut App) {
    {
        let mut view = app.world_mut().resource_mut::<IndexView>();
        view.folder = FolderId::new(".Drafts");
        view.selected = None;
    }
    let account = AccountId::new("local");
    let world = app.world_mut();
    let mut tracker = world.remove_resource::<SyncTracker>().unwrap();
    {
        let engine = world.resource::<EngineResource>();
        nitidus::bootstrap::request_sync(
            &engine.0,
            &mut tracker,
            &account,
            &FolderId::new(".Drafts"),
        )
        .unwrap();
    }
    world.insert_resource(tracker);
    assert!(
        wait_for(app, |world| world.resource::<IndexStatus>().total >= 1),
        "drafts folder never loaded"
    );
}

#[test]
fn postpone_recall_round_trip_preserves_everything() {
    let harness = harness();
    let mut app = drafts_app(&harness);
    stage_with_attachment(&mut app, &harness);

    press_alt(&mut app, KeyCode::Char('p'));
    assert!(!app.world().resource::<ComposeState>().is_active());
    assert!(
        wait_for(&mut app, |_| drafts_dir_count(&harness) == 1),
        "the draft never landed"
    );
    let leftover = std::fs::read_dir(&harness.compose).unwrap().count();
    assert_eq!(leftover, 0, "postpone must clean the local session files");

    switch_to_drafts(&mut app);
    press(&mut app, KeyCode::Char('e'));
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<ComposeState>().is_active()
        }),
        "recall never restored the session"
    );
    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.to, "bob@example.com");
    assert_eq!(session.bcc, "secret@example.com", "Bcc must round-trip");
    assert_eq!(session.subject, "draft test");
    assert_eq!(session.attachments.len(), 1, "attachment must round-trip");
    assert!(session.attachments[0].exists());
    assert!(session.draft_source.is_some());

    // Re-postpone replaces rather than accumulates.
    press_alt(&mut app, KeyCode::Char('p'));
    std::thread::sleep(Duration::from_millis(100));
    app.update();
    assert!(
        wait_for(&mut app, |_| drafts_dir_count(&harness) == 1),
        "re-postpone must replace the old draft, found {}",
        drafts_dir_count(&harness)
    );
}

#[test]
fn crash_recovery_restores_the_session_from_the_sidecar() {
    let harness = harness();
    {
        let mut app = drafts_app(&harness);
        stage_with_attachment(&mut app, &harness);
        // Simulated crash: drop the app without discard/postpone.
    }
    let mut app = drafts_app(&harness);
    assert!(!app.world().resource::<ComposeState>().is_active());

    nitidus::compose::recover(app.world_mut());
    app.update();
    let state = app.world().resource::<ComposeState>();
    let session = state.session().expect("recover must restore the session");
    assert_eq!(session.to, "bob@example.com");
    assert_eq!(session.bcc, "secret@example.com");
    assert_eq!(session.subject, "draft test");
    assert_eq!(session.attachments.len(), 1);
    assert!(app.world().resource::<ComposeState>().is_active());
}

#[test]
fn empty_subject_warning_gates_send_and_decline_keeps_review() {
    let harness = harness();
    let mut app = drafts_app(&harness);
    press(&mut app, KeyCode::Char('m'));
    type_text(&mut app, "bob@example.com"); // subject left empty

    press_alt(&mut app, KeyCode::Enter);
    let question = app
        .world()
        .resource::<nitidus::overlay::confirm::ActiveConfirm>()
        .question()
        .map(str::to_owned);
    assert!(
        question
            .as_deref()
            .is_some_and(|text| text.contains("subject")),
        "sending without a subject must ask about it, got {question:?}"
    );
    press(&mut app, KeyCode::Char('n'));
    assert!(
        app.world().resource::<ComposeState>().is_active(),
        "declining must keep the session"
    );
    assert!(app.world().resource::<ComposeState>().is_active());
}

#[test]
fn forgotten_attachment_warning_fires_on_unquoted_mentions() {
    let harness = harness();
    let script = harness._root.path().join("editor.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf 'see the attached file\\n' >> \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let mut app = drafts_app(&harness);
    app.insert_resource(EditorCommand(script.display().to_string()));
    press(&mut app, KeyCode::Char('m'));
    type_text(&mut app, "bob@example.com");
    for _ in 0..3 {
        press(&mut app, KeyCode::Tab);
    }
    type_text(&mut app, "has subject");
    press_alt(&mut app, KeyCode::Char('e'));

    press_alt(&mut app, KeyCode::Enter);
    let question = app
        .world()
        .resource::<nitidus::overlay::confirm::ActiveConfirm>()
        .question()
        .map(str::to_owned);
    assert!(
        question
            .as_deref()
            .is_some_and(|text| text.contains("attachment")),
        "a body mentioning an attachment must ask, got {question:?}"
    );
    press(&mut app, KeyCode::Char('y'));
    assert!(
        !app.world().resource::<ComposeState>().is_active(),
        "accepting must queue the send"
    );
}
