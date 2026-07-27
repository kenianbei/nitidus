//! Removal verbs end to end over a maildir account: `d` moves to
//! trash (permanent inside it), `a` archives, `:move` files, undo
//! restores, and pager verbs advance or close per `ui.pager`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::cmdline::CommandLineState;
use nitidus::config::Config;
use nitidus::config::RawKeymaps;
use nitidus::config::account::AccountConfig;
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView};
use nitidus::keymap::Keymaps;
use nitidus::overlay::OverlayPlugin;
use nitidus::pager::{PagerPlugin, PagerState};
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

fn write_message(root: &Path, name: &str, subject: &str) {
    let body = format!(
        "From: Alice <alice@x.com>\r\nSubject: {subject}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\nMessage-ID: <{name}@x>\r\n\r\nbody\r\n"
    );
    std::fs::write(root.join("cur").join(format!("{name}:2,")), body).unwrap();
}

struct Harness {
    app: App,
    mail_root: std::path::PathBuf,
    _root: tempfile::TempDir,
}

fn harness() -> Harness {
    let root = tempfile::tempdir().unwrap();
    let mail_root = root.path().join("mail");
    make_maildir(&mail_root);
    make_maildir(&mail_root.join(".Trash"));
    make_maildir(&mail_root.join(".Archive"));
    write_message(&mail_root, "first.host", "first mail");
    write_message(&mail_root, "second.host", "second mail");

    let account = AccountId::new("local");
    let mut engine = MailEngine::new(1).unwrap();
    engine.add_account(
        account.clone(),
        MaildirBackend::new(mail_root.clone()).unwrap(),
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
        ..Default::default()
    };
    account_config.folders.trash = ".Trash".to_owned();
    account_config.folders.archive = ".Archive".to_owned();
    config.accounts.push(account_config);
    app.insert_resource(config);
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(EngineResource(engine));
    app.insert_resource(tracker);
    app.insert_resource(nitidus::index::OpDelay(Duration::ZERO));
    app.add_plugins((
        RouterPlugin,
        IndexPlugin,
        PagerPlugin,
        OverlayPlugin,
        EnginePlugin,
    ));
    app.update();
    Harness {
        app,
        mail_root,
        _root: root,
    }
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

fn wait_inbox(app: &mut App, total: usize) {
    assert!(
        wait_for(app, |world| world.resource::<IndexStatus>().total == total),
        "INBOX never reached {total} messages"
    );
}

fn file_count(root: &Path, folder: &str) -> usize {
    std::fs::read_dir(root.join(folder).join("cur")).map_or(0, |entries| entries.count())
}

fn switch_folder(app: &mut App, folder: &str, expected: usize) {
    {
        let mut view = app.world_mut().resource_mut::<IndexView>();
        view.folder = FolderId::new(folder);
        view.selected = None;
    }
    let account = AccountId::new("local");
    let world = app.world_mut();
    let mut tracker = world.remove_resource::<SyncTracker>().unwrap();
    {
        let engine = world.resource::<EngineResource>();
        nitidus::bootstrap::request_sync(&engine.0, &mut tracker, &account, &FolderId::new(folder))
            .unwrap();
    }
    world.insert_resource(tracker);
    assert!(
        wait_for(app, |world| world.resource::<IndexStatus>().total
            == expected),
        "{folder} never reached {expected} messages"
    );
}

#[test]
fn delete_moves_the_selection_to_trash() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);

    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 1),
        "the message never landed in trash"
    );
    assert_eq!(file_count(&harness.mail_root, ""), 1, "one message remains");
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        1,
        "the store row is optimistically gone"
    );
}

#[test]
fn delete_inside_trash_confirms_and_declining_keeps() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 1),
        "setup: message must reach trash"
    );

    switch_folder(&mut harness.app, ".Trash", 1);
    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        harness
            .app
            .world()
            .resource::<nitidus::overlay::confirm::ActiveConfirm>()
            .is_open(),
        "a permanent delete inside the trash must ask first"
    );
    press(&mut harness.app, KeyCode::Char('n'));
    std::thread::sleep(Duration::from_millis(50));
    harness.app.update();
    assert_eq!(file_count(&harness.mail_root, ".Trash"), 1, "decline keeps");

    press(&mut harness.app, KeyCode::Char('d'));
    type_text(&mut harness.app, "y");
    press(&mut harness.app, KeyCode::Enter);
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 0),
        "confirm must purge the message"
    );
}

#[test]
fn archive_moves_the_selection_to_the_configured_folder() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);

    press(&mut harness.app, KeyCode::Char('a'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Archive"
        ) == 1),
        "the message never landed in the archive"
    );
    assert_eq!(file_count(&harness.mail_root, ""), 1, "one message remains");
}

#[test]
fn batch_archive_moves_every_marked_message() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);

    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char('a'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Archive"
        ) == 2),
        "both marked messages must reach the archive"
    );
    assert_eq!(file_count(&harness.mail_root, ""), 0);
}

#[test]
fn archive_to_an_unknown_folder_warns_without_staging() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness.app.world_mut().resource_mut::<Config>().accounts[0]
        .folders
        .archive = ".Missing".to_owned();

    press(&mut harness.app, KeyCode::Char('a'));
    assert_eq!(
        harness
            .app
            .world()
            .resource::<nitidus::index::StagedOps>()
            .pending(),
        0,
        "nothing may be staged toward a folder the store does not know"
    );
    assert_eq!(harness.app.world().resource::<IndexStatus>().total, 2);
}

#[test]
fn undo_restores_the_row_before_anything_reaches_disk() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .insert_resource(nitidus::index::OpDelay(Duration::from_secs(600)));

    press(&mut harness.app, KeyCode::Char('d'));
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        1,
        "the row leaves the view instantly"
    );
    std::thread::sleep(Duration::from_millis(30));
    harness.app.update();
    assert_eq!(
        file_count(&harness.mail_root, ".Trash"),
        0,
        "the backend command is staged, not sent"
    );

    press(&mut harness.app, KeyCode::Char('z'));
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        2,
        "undo restores the row"
    );
    assert_eq!(file_count(&harness.mail_root, ""), 2, "nothing moved");
    assert_eq!(
        harness
            .app
            .world()
            .resource::<nitidus::index::StagedOps>()
            .pending(),
        0
    );
}

#[test]
fn undo_is_lifo_across_staged_ops() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .insert_resource(nitidus::index::OpDelay(Duration::from_secs(600)));

    press(&mut harness.app, KeyCode::Char('d'));
    press(&mut harness.app, KeyCode::Char('d'));
    assert_eq!(harness.app.world().resource::<IndexStatus>().total, 0);

    press(&mut harness.app, KeyCode::Char('z'));
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        1,
        "one op undone at a time, newest first"
    );
    press(&mut harness.app, KeyCode::Char('z'));
    assert_eq!(harness.app.world().resource::<IndexStatus>().total, 2);

    press(&mut harness.app, KeyCode::Char('z'));
    harness.app.update();
    // Nothing staged: z falls back to undo-send, which reports
    // "nothing to undo" rather than touching the store.
    assert_eq!(harness.app.world().resource::<IndexStatus>().total, 2);
}

#[test]
fn batch_delete_trashes_every_marked_row_and_one_undo_restores_all() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .insert_resource(nitidus::index::OpDelay(Duration::from_secs(600)));

    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char('d'));
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        0,
        "both marked rows leave the view"
    );
    assert!(
        harness
            .app
            .world()
            .resource::<nitidus::index::IndexView>()
            .marked
            .is_empty(),
        "the batch verb consumes the marks"
    );

    press(&mut harness.app, KeyCode::Char('z'));
    assert_eq!(
        harness.app.world().resource::<IndexStatus>().total,
        2,
        "one undo restores the whole batch"
    );
    assert_eq!(file_count(&harness.mail_root, ""), 2);
}

#[test]
fn batch_delete_expiry_moves_every_file() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 2),
        "expiry dispatches the whole batch"
    );
}

#[test]
fn batch_permanent_delete_confirms_with_the_count() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char('D'));
    let question = harness
        .app
        .world()
        .resource::<nitidus::overlay::confirm::ActiveConfirm>()
        .question()
        .map(str::to_owned);
    assert!(
        question.as_deref().is_some_and(|text| text.contains('2')),
        "the question must name how many are going, got {question:?}"
    );
    press(&mut harness.app, KeyCode::Char('y'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(&harness.mail_root, "")
            == 0),
        "confirmed batch purge removes both files"
    );
    assert_eq!(file_count(&harness.mail_root, ".Trash"), 0, "no trash copy");
}

#[test]
fn batch_move_files_every_marked_row() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    assert!(
        wait_for(&mut harness.app, |world| {
            !world
                .resource::<MailStore>()
                .folders(&AccountId::new("local"))
                .is_empty()
        }),
        "the folder list never arrived"
    );
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(' '));
    press(&mut harness.app, KeyCode::Char(':'));
    type_text(&mut harness.app, "move .Archive");
    press(&mut harness.app, KeyCode::Enter);
    assert!(
        wait_for(&mut harness.app, |_| {
            file_count(&harness.mail_root, ".Archive") == 2
        }),
        "both marked rows land in the archive"
    );
}

#[test]
fn app_exit_flushes_staged_ops_to_the_backend() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .insert_resource(nitidus::index::OpDelay(Duration::from_secs(600)));

    press(&mut harness.app, KeyCode::Char('d'));
    assert_eq!(file_count(&harness.mail_root, ".Trash"), 0);

    harness
        .app
        .world_mut()
        .write_message(bevy::app::AppExit::Success);
    harness.app.update();
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 1),
        "exit must dispatch the staged op, never drop it"
    );
}

#[test]
fn move_command_files_to_a_named_folder() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    assert!(
        wait_for(&mut harness.app, |world| {
            !world
                .resource::<MailStore>()
                .folders(&AccountId::new("local"))
                .is_empty()
        }),
        "the folder list never arrived"
    );

    press(&mut harness.app, KeyCode::Char(':'));
    type_text(&mut harness.app, "move .Archive");
    press(&mut harness.app, KeyCode::Enter);
    assert!(
        wait_for(&mut harness.app, |_| {
            file_count(&harness.mail_root, ".Archive") == 1
        }),
        "the message never landed in the archive"
    );

    // Unknown folders are refused before anything is touched.
    press(&mut harness.app, KeyCode::Char(':'));
    type_text(&mut harness.app, "move .Nowhere");
    press(&mut harness.app, KeyCode::Enter);
    harness.app.update();
    assert_eq!(harness.app.world().resource::<IndexStatus>().total, 1);
}

fn open_pager(app: &mut App) {
    press(app, KeyCode::Enter);
    assert!(
        wait_for(app, |world| world.resource::<PagerState>().is_open()),
        "the pager never opened"
    );
}

fn inbox_flags(world: &World, id: &nitidus_mail::EnvelopeId) -> Option<nitidus_mail::Flags> {
    world
        .resource::<MailStore>()
        .envelopes(&AccountId::new("local"), &FolderId::new("INBOX"))
        .iter()
        .find(|envelope| &envelope.id == id)
        .map(|envelope| envelope.flags)
}

#[test]
fn pager_delete_with_advance_off_closes_and_trashes() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .world_mut()
        .resource_mut::<Config>()
        .ui
        .pager
        .advance = false;

    open_pager(&mut harness.app);
    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |_| file_count(
            &harness.mail_root,
            ".Trash"
        ) == 1),
        "the open message never reached trash"
    );
    assert!(!harness.app.world().resource::<PagerState>().is_open());
}

#[test]
fn pager_delete_advances_to_the_next_message_and_closes_on_the_last() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);

    open_pager(&mut harness.app);
    let first = harness
        .app
        .world()
        .resource::<PagerState>()
        .open_id()
        .unwrap()
        .clone();
    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |world| {
            let pager = world.resource::<PagerState>();
            pager.open_id().is_some_and(|id| id != &first)
        }),
        "the pager never advanced to the next message"
    );
    assert!(nitidus::focus::is_focused(
        harness.app.world(),
        nitidus::focus::Pane::Reading
    ));

    press(&mut harness.app, KeyCode::Char('d'));
    assert!(
        wait_for(&mut harness.app, |world| {
            !world.resource::<PagerState>().is_open()
        }),
        "deleting the last message must fall back to closing"
    );
    assert!(!harness.app.world().resource::<PagerState>().is_open());
}

#[test]
fn mark_read_open_flags_the_message_once_its_fetch_lands() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    let first = harness
        .app
        .world()
        .resource::<IndexView>()
        .selected
        .clone()
        .unwrap();
    assert!(
        !inbox_flags(harness.app.world(), &first)
            .unwrap()
            .contains(nitidus_mail::Flags::SEEN),
        "setup: the message starts unread"
    );

    open_pager(&mut harness.app);

    assert!(
        wait_for(&mut harness.app, |world| {
            inbox_flags(world, &first)
                .is_some_and(|flags| flags.contains(nitidus_mail::Flags::SEEN))
        }),
        "a message that arrived in the reading pane must be marked read"
    );
}

#[test]
fn mark_read_never_leaves_the_message_unread_in_the_pane_and_zoomed() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    harness
        .app
        .world_mut()
        .resource_mut::<Config>()
        .ui
        .pager
        .mark_read = nitidus::config::MarkRead::Never;
    let first = harness
        .app
        .world()
        .resource::<IndexView>()
        .selected
        .clone()
        .unwrap();

    open_pager(&mut harness.app);
    press(&mut harness.app, KeyCode::Char('Z'));
    std::thread::sleep(Duration::from_millis(50));
    harness.app.update();

    assert!(
        !inbox_flags(harness.app.world(), &first)
            .unwrap()
            .contains(nitidus_mail::Flags::SEEN),
        "never must hold in the pane and in the overlay alike"
    );
}

#[test]
fn single_target_flag_toggle_advances_the_cursor() {
    let mut harness = harness();
    wait_inbox(&mut harness.app, 2);
    assert_eq!(harness.app.world().resource::<IndexStatus>().selected, 1);

    press(&mut harness.app, KeyCode::Char('u'));
    assert!(
        wait_for(&mut harness.app, |world| {
            world.resource::<IndexStatus>().selected == 2
        }),
        "toggling read on a single message must advance the cursor"
    );
}
