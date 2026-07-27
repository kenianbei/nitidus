//! Reply machinery end to end: reply from the pager pre-fills and
//! threads, reply from the index fetches via the intent, forward
//! prompts To, and a sent reply appends the Sent copy and marks the
//! source answered (or skips the copy when save_sent is off).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::action::{Action, apply_action};
use nitidus::cmdline::CommandLineState;
use nitidus::compose::{ComposeDir, ComposePlugin, ComposeState, EditorCommand, InlineEditor};
use nitidus::config::account::{AccountConfig, Outgoing, SendmailOutgoing};
use nitidus::config::{Config, RawKeymaps};
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus};
use nitidus::keymap::{InputMode, Keymaps, Mode};
use nitidus::outbox::{OutboxDir, OutboxPlugin, OutboxState, SendDelay};
use nitidus::overlay::OverlayPlugin;
use nitidus::overlay::form::ActiveForm;
use nitidus::pager::{PagerPlugin, PagerState};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::maildir::MaildirBackend;
use nitidus_mail::{AccountId, Flags, FolderId, MailEngine};
use plurimus::{TachyonRegistry, UiEvent};

const ORIGINAL: &str = "From: Alice <alice@x.com>\r\n\
To: Norman <norman@example.com>\r\n\
Subject: project plan\r\n\
Date: Mon, 08 Apr 2024 20:52:42 -0700\r\n\
Message-ID: <orig-1@x.com>\r\n\r\n\
original body line\r\n";

fn make_maildir(root: &Path) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
}

struct Harness {
    _root: tempfile::TempDir,
    mail_root: std::path::PathBuf,
    outbox: std::path::PathBuf,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().unwrap();
    let mail_root = root.path().join("mail");
    make_maildir(&mail_root);
    make_maildir(&mail_root.join(".Sent"));
    std::fs::write(mail_root.join("cur/orig.host:2,"), ORIGINAL).unwrap();
    Harness {
        outbox: root.path().join("outbox"),
        mail_root,
        _root: root,
    }
}

fn reply_app(harness: &Harness, save_sent: bool) -> App {
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
    let mut account_config = AccountConfig {
        name: "local".to_owned(),
        email: "norman@example.com".to_owned(),
        display_name: "Norman".to_owned(),
        outgoing: Some(Outgoing::Sendmail(SendmailOutgoing {
            command: "true".to_owned(),
        })),
        ..Default::default()
    };
    account_config.folders.sent = ".Sent".to_owned();
    account_config.folders.save_sent = save_sent;
    config.accounts.push(account_config);
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(ComposeDir(harness._root.path().join("compose")));
    app.insert_resource(OutboxDir(harness.outbox.clone()));
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
    route_key(
        app.world_mut(),
        Entity::PLACEHOLDER,
        UiEvent::Key(KeyEvent::from(code)),
    )
    .unwrap();
    app.update();
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

fn wait_loaded(app: &mut App) {
    assert!(
        wait_for(app, |world| world.resource::<IndexStatus>().total == 1),
        "INBOX never loaded"
    );
}

fn open_in_pager(app: &mut App) {
    apply_action(app.world_mut(), &Action::View);
    assert!(
        wait_for(app, |world| world.resource::<PagerState>().is_open()),
        "message never opened"
    );
}

#[test]
fn reply_from_pager_prefills_threads_and_skips_prompts() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);
    open_in_pager(&mut app);

    press(&mut app, KeyCode::Char('r'));
    assert!(app.world().resource::<ComposeState>().is_active());
    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.to, "Alice <alice@x.com>");
    assert_eq!(session.subject, "Re: project plan");
    assert_eq!(session.in_reply_to.as_deref(), Some("orig-1@x.com"));
    assert_eq!(session.references, vec!["orig-1@x.com"]);
    assert!(
        session
            .body
            .iter()
            .any(|line| line == "> original body line"),
        "{:?}",
        session.body
    );
    assert!(
        !app.world().resource::<ActiveForm>().is_open(),
        "replies skip the headers form"
    );
}

#[test]
fn reply_from_index_fetches_via_the_intent() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);

    press(&mut app, KeyCode::Char('r'));
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<ComposeState>().is_active()
        }),
        "the intent never produced a session"
    );
    let state = app.world().resource::<ComposeState>();
    assert_eq!(state.session().unwrap().subject, "Re: project plan");
    assert!(
        !app.world().resource::<PagerState>().is_open(),
        "the pager must not open for an index reply"
    );
}

#[test]
fn forward_prompts_for_to_with_inline_block() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);
    open_in_pager(&mut app);

    press(&mut app, KeyCode::Char('f'));
    assert!(
        app.world().resource::<ActiveForm>().is_open(),
        "forward must ask for the recipient"
    );
    for character in "dave@example.com".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    let state = app.world().resource::<ComposeState>();
    let session = state.session().unwrap();
    assert_eq!(session.subject, "Fwd: project plan");
    assert_eq!(session.to, "dave@example.com");
    assert!(
        session
            .body
            .iter()
            .any(|line| line.contains("Forwarded message")),
        "{:?}",
        session.body
    );
}

fn send_reply_and_wait(app: &mut App) {
    press(app, KeyCode::Char('r'));
    assert!(wait_for(app, |world| {
        world.resource::<ComposeState>().is_active()
    }));
    // A reply opens the editor, where `y` is a letter; leave it first so
    // the review screen takes the send.
    press(app, KeyCode::Esc);
    press(app, KeyCode::Char('y'));
    assert!(
        wait_for(app, |world| {
            world.resource::<OutboxState>().pending_count() == 0
                && !world.resource::<ComposeState>().is_active()
        }),
        "the reply never finished sending"
    );
}

#[test]
fn sent_reply_appends_copy_and_marks_answered() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);
    send_reply_and_wait(&mut app);

    let sent_dir = harness.mail_root.join(".Sent/cur");
    assert!(
        wait_for(&mut app, |_| {
            std::fs::read_dir(&sent_dir).map_or(0, |entries| entries.count()) == 1
        }),
        "sent copy never landed in .Sent"
    );
    let sent_file = std::fs::read_dir(&sent_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let content = std::fs::read_to_string(sent_file.path()).unwrap();
    assert!(content.contains("Subject: Re: project plan"), "{content}");
    assert!(content.contains("In-Reply-To: <orig-1@x.com>"), "{content}");

    assert!(
        wait_for(&mut app, |world| {
            world
                .resource::<MailStore>()
                .envelopes(&AccountId::new("local"), &FolderId::new("INBOX"))
                .first()
                .is_some_and(|envelope| envelope.flags.contains(Flags::ANSWERED))
        }),
        "the source message never gained the answered flag"
    );
}

#[test]
fn save_sent_false_skips_the_copy() {
    let harness = harness();
    let mut app = reply_app(&harness, false);
    wait_loaded(&mut app);
    send_reply_and_wait(&mut app);

    std::thread::sleep(Duration::from_millis(100));
    app.update();
    let sent_count =
        std::fs::read_dir(harness.mail_root.join(".Sent/cur")).map_or(0, |entries| entries.count());
    assert_eq!(sent_count, 0, "save_sent = false must skip the Sent copy");
}

/// Every route into a body — new, reply, reply-all, forward — has to
/// honour `ui.compose.editor`. Replies once called the external editor
/// directly and ignored it.
#[test]
fn replying_opens_the_inline_editor() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);
    open_in_pager(&mut app);

    press(&mut app, KeyCode::Char('r'));

    assert_eq!(
        app.world().resource::<Mode>().0,
        InputMode::Editor,
        "a reply must land in the inline editor like a new message does"
    );
    assert!(app.world().resource::<InlineEditor>().is_active());
    assert!(
        app.world()
            .resource::<InlineEditor>()
            .lines()
            .unwrap()
            .iter()
            .any(|line| line == "> original body line"),
        "the quoted reply must be in the buffer"
    );
}

#[test]
fn forwarding_opens_the_inline_editor_after_the_to_prompt() {
    let harness = harness();
    let mut app = reply_app(&harness, true);
    wait_loaded(&mut app);
    open_in_pager(&mut app);

    press(&mut app, KeyCode::Char('f'));
    assert!(app.world().resource::<ActiveForm>().is_open());
    for character in "bob@x.com".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.world().resource::<Mode>().0,
        InputMode::Editor,
        "forward must reach the editor once the To prompt is answered"
    );
}
