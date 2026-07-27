//! The send flow end to end: y queues the built message with an undo
//! window, z restores the session, expiry submits through the engine
//! (sendmail transport), success cleans every file up, failure parks
//! the entry, and startup recovers queued sends.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{ComposeDir, ComposePlugin, ComposeState, EditorCommand};
use nitidus::config::account::{AccountConfig, Outgoing, SendmailOutgoing};
use nitidus::config::{Config, RawKeymaps};
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::IndexPlugin;
use nitidus::keymap::Keymaps;
use nitidus::outbox::{OutboxDir, OutboxPlugin, OutboxState, SendDelay};
use nitidus::overlay::OverlayPlugin;
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::MailEngine;
use plurimus::{TachyonRegistry, UiEvent};

struct Dirs {
    _root: tempfile::TempDir,
    compose: std::path::PathBuf,
    outbox: std::path::PathBuf,
    captured: std::path::PathBuf,
}

fn dirs() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let compose = root.path().join("compose");
    let outbox = root.path().join("outbox");
    let captured = root.path().join("captured.msg");
    Dirs {
        compose,
        outbox,
        captured,
        _root: root,
    }
}

fn sendmail_script(dirs: &Dirs, body: &str) -> String {
    let script = dirs._root.path().join("sendmail.sh");
    std::fs::write(
        &script,
        body.replace("{OUT}", &dirs.captured.display().to_string()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    script.display().to_string()
}

fn send_app(dirs: &Dirs, sendmail_command: &str, delay: Duration) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<CommandLineState>();
    app.init_resource::<MailStore>();
    app.init_resource::<SyncTracker>();
    let mut config = Config::default();
    // Driven through `EditorCommand`, so pin the `$EDITOR` path.
    config.ui.compose.editor = nitidus::config::EditorKind::External;
    config.accounts.push(AccountConfig {
        name: "local".to_owned(),
        email: "norman@example.com".to_owned(),
        outgoing: Some(Outgoing::Sendmail(SendmailOutgoing {
            command: sendmail_command.to_owned(),
        })),
        ..Default::default()
    });
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(dirs.compose.clone()));
    app.insert_resource(OutboxDir(dirs.outbox.clone()));
    app.insert_resource(SendDelay(delay));
    app.insert_resource(EditorCommand("true".to_owned()));
    app.insert_resource(EngineResource(MailEngine::new(1).unwrap()));
    app.add_plugins((
        RouterPlugin,
        IndexPlugin,
        ComposePlugin,
        OutboxPlugin,
        OverlayPlugin,
        EnginePlugin,
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

fn stage_message(app: &mut App) {
    press(app, KeyCode::Char('m'));
    type_text(app, "bob@example.com");
    press(app, KeyCode::Tab);
    type_text(app, "outbox test");
    press(app, KeyCode::Enter);
    assert!(app.world().resource::<ComposeState>().is_active());
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

fn outbox_files(directory: &Path) -> usize {
    std::fs::read_dir(directory).map_or(0, |entries| entries.count())
}

#[test]
fn y_queues_with_countdown_and_z_restores_the_session() {
    let dirs = dirs();
    let mut app = send_app(&dirs, "true", Duration::from_secs(600));
    stage_message(&mut app);
    let body_path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();

    press(&mut app, KeyCode::Char('y'));
    assert!(!app.world().resource::<ComposeState>().is_active());
    assert!(!app.world().resource::<ComposeState>().is_active());
    assert_eq!(outbox_files(&dirs.outbox), 2, "eml + toml pair expected");
    assert!(body_path.exists(), "body survives while queued");
    assert!(
        app.world()
            .resource::<OutboxState>()
            .countdown_ms()
            .is_some()
    );

    press(&mut app, KeyCode::Char('z'));
    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.to, "bob@example.com");
    assert_eq!(session.subject, "outbox test");
    assert_eq!(session.body_path, body_path);
    assert!(app.world().resource::<ComposeState>().is_active());
    assert_eq!(outbox_files(&dirs.outbox), 0);
}

#[test]
fn expiry_submits_via_sendmail_and_cleans_everything_up() {
    let dirs = dirs();
    let script = sendmail_script(&dirs, "#!/bin/sh\ncat > '{OUT}'\n");
    let mut app = send_app(&dirs, &script, Duration::from_millis(50));
    stage_message(&mut app);
    let body_path = app
        .world()
        .resource::<ComposeState>()
        .session()
        .unwrap()
        .body_path
        .clone();

    press(&mut app, KeyCode::Char('y'));
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<OutboxState>().pending_count() == 0
        }),
        "the entry never completed"
    );
    let sent = std::fs::read_to_string(&dirs.captured).unwrap();
    assert!(sent.contains("Subject: outbox test"), "{sent}");
    assert!(sent.contains("bob@example.com"), "{sent}");
    assert_eq!(outbox_files(&dirs.outbox), 0, "outbox pair must be removed");
    assert!(!body_path.exists(), "body file removed after success");
}

#[test]
fn failed_send_parks_the_entry_with_files_intact() {
    let dirs = dirs();
    let mut app = send_app(&dirs, "exit 3", Duration::from_millis(50));
    stage_message(&mut app);

    press(&mut app, KeyCode::Char('y'));
    assert!(
        wait_for(&mut app, |world| {
            let outbox = world.resource::<OutboxState>();
            outbox.pending_count() == 1 && !outbox.is_sending()
        }),
        "the failed entry should return to a parked pending state"
    );
    assert_eq!(outbox_files(&dirs.outbox), 2, "files stay for retry");
}

#[test]
fn startup_recovers_queued_entries() {
    let dirs = dirs();
    std::fs::create_dir_all(&dirs.outbox).unwrap();
    std::fs::write(dirs.outbox.join("42-1.eml"), "From: a@x\r\n\r\nhi\r\n").unwrap();
    std::fs::write(
        dirs.outbox.join("42-1.toml"),
        concat!(
            "account = \"local\"\n",
            "from = \"Norman <norman@example.com>\"\n",
            "to = \"bob@example.com\"\n",
            "cc = \"\"\n",
            "bcc = \"\"\n",
            "subject = \"recovered\"\n",
            "body_path = \"/nonexistent/body.md\"\n",
            "envelope_from = \"norman@example.com\"\n",
            "recipients = [\"bob@example.com\"]\n",
            "send_at_epoch_ms = 99999999999999\n",
        ),
    )
    .unwrap();

    let app = send_app(&dirs, "true", Duration::from_secs(600));
    assert_eq!(
        app.world().resource::<OutboxState>().pending_count(),
        1,
        "startup must recover the queued pair"
    );
}
