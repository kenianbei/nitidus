//! Pager behavior over a real maildir: open/mark-read/close, adjacent
//! navigation, part switching, attachment save, link picker, and fetch
//! failure fallback.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use nitidus::action::{Action, PagerOp, apply_action};
use nitidus::config::Config;
use nitidus::config::account::AccountConfig;
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView};
use nitidus::overlay::{ActiveOverlay, OverlayPlugin};
use nitidus::pager::{PagerPlugin, PagerState, PagerStatus, SaveDir};
use nitidus::screen::Screen;
use nitidus::status::StatusMessage;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::maildir::MaildirBackend;
use nitidus_mail::{AccountId, Flags, FolderId, MailEngine};

fn make_maildir(root: &Path) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
}

fn simple_message(subject: &str) -> String {
    format!(
        "From: Alice <alice@x.com>\r\nSubject: {subject}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\n\r\nplain body\r\n"
    )
}

fn rich_message() -> String {
    concat!(
        "From: Bob <bob@x.com>\r\n",
        "To: Norman <n@x.com>\r\n",
        "Subject: rich\r\n",
        "Date: Tue, 21 Jul 2026 09:00:00 +0000\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
        "\r\n",
        "--outer\r\n",
        "Content-Type: multipart/alternative; boundary=\"inner\"\r\n",
        "\r\n",
        "--inner\r\n",
        "Content-Type: text/plain\r\n",
        "\r\n",
        "> quoted context\r\n",
        "see https://example.com/page for details\r\n",
        "--inner\r\n",
        "Content-Type: text/html\r\n",
        "\r\n",
        "<p>hi</p>\r\n",
        "--inner--\r\n",
        "--outer\r\n",
        "Content-Type: application/pdf\r\n",
        "Content-Disposition: attachment; filename=\"doc.pdf\"\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "JVBERi0xLjQ=\r\n",
        "--outer--\r\n",
    )
    .to_owned()
}

/// Engine without a watcher, so tests control the filesystem freely.
fn pager_app(root: &Path) -> App {
    let account = AccountId::new("local");
    let mut engine = MailEngine::new(1).unwrap();
    engine.add_account(account.clone(), MaildirBackend::new(root.to_path_buf()).unwrap());
    let mut tracker = SyncTracker::default();
    nitidus::bootstrap::request_sync(&engine, &mut tracker, &account, &FolderId::new("INBOX"))
        .unwrap();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(plurimus::TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    let mut config = Config::default();
    config.accounts.push(AccountConfig {
        name: "local".to_owned(),
        ..Default::default()
    });
    app.insert_resource(config);
    app.init_resource::<MailStore>();
    app.init_resource::<StatusMessage>();
    app.insert_resource(EngineResource(engine));
    app.add_plugins((IndexPlugin, PagerPlugin, OverlayPlugin, EnginePlugin));
    app.insert_resource(tracker);
    app.update();
    app
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

fn wait_total(app: &mut App, total: usize) {
    assert!(
        wait_for(app, |world| world.resource::<IndexStatus>().total == total),
        "store never reached {total} messages"
    );
}

fn open_selected_and_wait(app: &mut App) {
    apply_action(app.world_mut(), &Action::View);
    assert_eq!(*app.world().resource::<Screen>(), Screen::Pager);
    assert!(
        wait_for(app, |world| world.resource::<PagerState>().is_open()),
        "message never arrived in the pager"
    );
}

#[test]
fn view_opens_marks_read_and_close_returns() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("new/only.host"), simple_message("hello")).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);

    open_selected_and_wait(&mut app);
    let world = app.world();
    let account = AccountId::new("local");
    let inbox = FolderId::new("INBOX");
    assert!(
        world.resource::<MailStore>().envelopes(&account, &inbox)[0]
            .flags
            .contains(Flags::SEEN),
        "opening must mark read optimistically"
    );

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Close));
    assert_eq!(*app.world().resource::<Screen>(), Screen::Index);
    assert!(!app.world().resource::<PagerState>().is_open());
    assert!(app.world().resource::<IndexView>().selected.is_some());
}

#[test]
fn adjacent_message_navigation_stays_in_pager() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/newer.host:2,S"), simple_message("newer")).unwrap();
    std::fs::write(tmp.path().join("cur/older.host:2,S"), simple_message("older")).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 2);

    open_selected_and_wait(&mut app);
    let first = app.world().resource::<PagerState>().open_id().unwrap().clone();
    apply_action(app.world_mut(), &Action::Pager(PagerOp::NextMessage));
    assert!(
        wait_for(&mut app, |world| {
            world
                .resource::<PagerState>()
                .open_id()
                .is_some_and(|id| id != &first)
        }),
        "J never opened the adjacent message"
    );
    assert_eq!(*app.world().resource::<Screen>(), Screen::Pager);
}

#[test]
fn part_switcher_cycles_and_reports_in_status() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/rich.host:2,S"), rich_message()).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    app.update();
    assert_eq!(
        app.world().resource::<PagerStatus>().part.as_deref(),
        Some("text/plain 1/2")
    );
    apply_action(app.world_mut(), &Action::Pager(PagerOp::NextPart));
    app.update();
    assert_eq!(
        app.world().resource::<PagerStatus>().part.as_deref(),
        Some("text/html 2/2")
    );
}

#[test]
fn save_part_writes_the_attachment_into_save_dir() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/rich.host:2,S"), rich_message()).unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let mut app = pager_app(tmp.path());
    app.insert_resource(SaveDir(downloads.path().to_path_buf()));
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::Pager(PagerOp::SavePart));
    let saved = downloads.path().join("doc.pdf");
    assert_eq!(std::fs::read(&saved).unwrap(), b"%PDF-1.4");

    apply_action(app.world_mut(), &Action::Pager(PagerOp::SavePart));
    assert!(
        downloads.path().join("doc(1).pdf").exists(),
        "second save must uniquify"
    );
}

#[test]
fn links_command_opens_the_picker_with_extracted_urls() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/rich.host:2,S"), rich_message()).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Links));
    assert!(app.world().resource::<ActiveOverlay>().is_open());
}

#[test]
fn failed_fetch_falls_back_to_the_index_with_a_warning() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    let path = tmp.path().join("cur/gone.host:2,S");
    std::fs::write(&path, simple_message("about to vanish")).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);

    std::fs::remove_file(&path).unwrap();
    apply_action(app.world_mut(), &Action::View);
    assert_eq!(*app.world().resource::<Screen>(), Screen::Pager);
    assert!(
        wait_for(&mut app, |world| {
            *world.resource::<Screen>() == Screen::Index
        }),
        "failed fetch never returned to the index"
    );
    assert!(!app.world().resource::<PagerState>().is_loading());
    let status = app.world().resource::<StatusMessage>();
    assert!(status.current().is_some(), "failure must surface a warning");
}
