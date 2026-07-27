//! Pager behavior over a real maildir: open/mark-read/close, adjacent
//! navigation, part switching, attachment save, link picker, and fetch
//! failure fallback.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use nitidus::action::{Action, Motion, PagerOp, apply_action};
use nitidus::config::Config;
use nitidus::config::account::AccountConfig;
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView};
use nitidus::overlay::{ActiveOverlay, OverlayPlugin};
use nitidus::pager::{PagerPlugin, PagerState, PagerStatus, ReadingZoom, SaveDir};
use nitidus::status::{MessageLog, Severity};
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

fn html_message() -> String {
    concat!(
        "From: Carol <carol@x.com>\r\n",
        "Subject: newsletter\r\n",
        "Date: Wed, 22 Jul 2026 09:00:00 +0000\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: text/html\r\n",
        "\r\n",
        "<p>Read <strong>this</strong>: <a href=\"https://example.com/story\">the story</a></p>\r\n",
        "<img src=\"https://tracker.example/pixel.gif\">\r\n",
    )
    .to_owned()
}

/// Engine without a watcher, so tests control the filesystem freely.
fn pager_app(root: &Path) -> App {
    let account = AccountId::new("local");
    let mut engine = MailEngine::new(1).unwrap();
    engine.add_account(
        account.clone(),
        MaildirBackend::new(root.to_path_buf()).unwrap(),
    );
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
    app.init_resource::<MessageLog>();
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

fn reading_focused(world: &World) -> bool {
    nitidus::focus::is_focused(world, nitidus::focus::Pane::Reading)
}

fn open_selected_and_wait(app: &mut App) {
    apply_action(app.world_mut(), &Action::View);
    assert!(
        reading_focused(app.world()),
        "opening focuses the reading pane"
    );
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
    assert!(!app.world().resource::<PagerState>().is_open());
    assert!(app.world().resource::<IndexView>().selected.is_some());
}

#[test]
fn adjacent_message_navigation_stays_in_pager() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(
        tmp.path().join("cur/newer.host:2,S"),
        simple_message("newer"),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("cur/older.host:2,S"),
        simple_message("older"),
    )
    .unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 2);

    open_selected_and_wait(&mut app);
    let first = app
        .world()
        .resource::<PagerState>()
        .open_id()
        .unwrap()
        .clone();
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
    assert!(reading_focused(app.world()));
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
    let overlay = app.world().resource::<ActiveOverlay>();
    assert!(overlay.is_open());
    let items = overlay.visible_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "https://example.com/page");
    assert_eq!(items[0].detail, None, "plain-text links carry no detail");
}

#[test]
fn html_part_links_list_anchors_with_labels() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/html.host:2,S"), html_message()).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Links));
    let overlay = app.world().resource::<ActiveOverlay>();
    assert!(overlay.is_open());
    let items = overlay.visible_items();
    assert_eq!(items.len(), 1, "tracker img must not appear as a link");
    assert_eq!(items[0].label, "the story");
    assert_eq!(
        items[0].detail.as_deref(),
        Some("https://example.com/story")
    );
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
    assert!(reading_focused(app.world()));
    assert!(
        wait_for(&mut app, |world| !reading_focused(world)),
        "a failed fetch must hand focus back to the message list"
    );
    assert!(!app.world().resource::<PagerState>().is_loading());
    assert!(
        app.world()
            .resource::<MessageLog>()
            .entries()
            .last()
            .is_some_and(|entry| entry.severity == Severity::Warning),
        "failure must surface a warning"
    );
}

/// The reading pane holds its own message, so the cursor and the pane
/// can disagree — which is what makes reading explicit rather than a
/// fetch per keystroke.
#[test]
fn arrowing_the_index_neither_fetches_nor_disturbs_the_reading_pane() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    for index in 0..3 {
        std::fs::write(
            tmp.path().join(format!("cur/m{index}.host:2,S")),
            simple_message(&format!("subject {index}")),
        )
        .unwrap();
    }
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 3);

    open_selected_and_wait(&mut app);
    let loaded = app
        .world()
        .resource::<PagerState>()
        .open_id()
        .cloned()
        .unwrap();

    // Back out to the list, then browse it while the pane keeps its
    // message.
    apply_action(app.world_mut(), &Action::FocusLeft);
    assert!(!reading_focused(app.world()));
    apply_action(app.world_mut(), &Action::Cursor(Motion::Next));
    apply_action(app.world_mut(), &Action::Cursor(Motion::Next));
    app.update();

    assert_eq!(
        app.world().resource::<PagerState>().open_id(),
        Some(&loaded),
        "moving the cursor must not swap what is being read"
    );
    assert!(
        !app.world().resource::<PagerState>().is_loading(),
        "moving the cursor must not start a fetch"
    );
    assert_ne!(
        app.world().resource::<IndexView>().selected.as_ref(),
        Some(&loaded),
        "the cursor has genuinely moved off the loaded message"
    );
}

#[test]
fn closing_the_reading_pane_returns_focus_to_the_message_list() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(
        tmp.path().join("cur/one.host:2,S"),
        simple_message("only one"),
    )
    .unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Close));

    assert!(!reading_focused(app.world()));
    assert!(!app.world().resource::<PagerState>().is_open());
}

#[test]
fn zooming_raises_the_reading_pane_and_closing_returns_to_the_list() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    for index in 0..2 {
        std::fs::write(
            tmp.path().join(format!("cur/m{index}.host:2,S")),
            simple_message(&format!("subject {index}")),
        )
        .unwrap();
    }
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 2);
    open_selected_and_wait(&mut app);
    let selected = app.world().resource::<IndexView>().selected.clone();

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Zoom));
    app.update();
    assert!(app.world().resource::<ReadingZoom>().is_zoomed());

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Close));
    app.update();

    assert!(
        !app.world().resource::<ReadingZoom>().is_zoomed(),
        "closing must leave the overlay as well as the message"
    );
    assert!(!reading_focused(app.world()));
    assert_eq!(
        app.world().resource::<IndexView>().selected,
        selected,
        "the list selection survives the overlay"
    );
}

#[test]
fn the_zoomed_pane_stays_below_a_picker_opened_from_it() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/rich.host:2,S"), rich_message()).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Zoom));
    apply_action(app.world_mut(), &Action::Pager(PagerOp::Links));
    app.update();

    assert!(
        app.world().resource::<ActiveOverlay>().is_open(),
        "a picker must still open over the zoomed pane"
    );
    let orders: Vec<i32> = app
        .world_mut()
        .query::<&plurimus::WidgetOrder>()
        .iter(app.world())
        .map(|order| order.0)
        .collect();
    assert!(
        orders.contains(&nitidus_ui_kit::layer::ZOOM),
        "the zoomed pane sits on its own rung, got {orders:?}"
    );
    assert!(
        orders
            .iter()
            .any(|order| *order > nitidus_ui_kit::layer::ZOOM),
        "and the picker draws above it, got {orders:?}"
    );
}

/// `Z` on a row nobody has opened has nothing to enlarge unless it
/// loads the message first.
#[test]
fn zooming_from_the_message_list_opens_the_selected_message() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(
        tmp.path().join("cur/unread.host:2,S"),
        simple_message("never opened"),
    )
    .unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    assert!(
        !app.world().resource::<PagerState>().is_open(),
        "setup: nothing has been read yet"
    );

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Zoom));

    assert!(app.world().resource::<ReadingZoom>().is_zoomed());
    assert!(
        wait_for(&mut app, |world| world.resource::<PagerState>().is_open()),
        "zooming must load the selected message"
    );
    assert!(reading_focused(app.world()));
}

#[test]
fn zooming_an_empty_folder_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    let mut app = pager_app(tmp.path());
    app.update();

    apply_action(app.world_mut(), &Action::Pager(PagerOp::Zoom));

    assert!(
        !app.world().resource::<ReadingZoom>().is_zoomed(),
        "there is no message to enlarge"
    );
}

/// Re-opening what the pane already holds should not go back to the
/// network for it.
#[test]
fn reopening_the_loaded_message_does_not_refetch() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    std::fs::write(tmp.path().join("cur/one.host:2,S"), simple_message("one")).unwrap();
    let mut app = pager_app(tmp.path());
    wait_total(&mut app, 1);
    open_selected_and_wait(&mut app);

    apply_action(app.world_mut(), &Action::FocusLeft);
    apply_action(app.world_mut(), &Action::View);

    assert!(
        !app.world().resource::<PagerState>().is_loading(),
        "the message is already in the pane; nothing should be in flight"
    );
    assert!(
        reading_focused(app.world()),
        "focus still moves to the pane"
    );
}
