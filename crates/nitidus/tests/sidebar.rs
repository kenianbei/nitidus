//! Sidebar behavior over a real maildir: tree rows with unread counts,
//! folder switching with lazy sync, live badge updates, and the folder
//! CRUD commands.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use nitidus::action::{Action, FlagOp, Motion, apply_action};
use nitidus::config::Config;
use nitidus::config::account::AccountConfig;
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView};
use nitidus::overlay::OverlayPlugin;
use nitidus::pager::PagerPlugin;
use nitidus::sidebar::{RowKind, SidebarPlugin, SidebarRows, SidebarState};
use nitidus::status::StatusMessage;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::maildir::MaildirBackend;
use nitidus_mail::{AccountId, Flags, FolderId, MailCommand, MailEngine};

fn make_maildir(root: &Path) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
}

fn write_message(dir: &Path, name: &str, subject: &str) {
    std::fs::write(
        dir.join(name),
        format!(
            "From: Alice <alice@x.com>\r\nSubject: {subject}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\n\r\nbody\r\n"
        ),
    )
    .unwrap();
}

/// INBOX with one unread message; `.Work` with one read and one unread.
fn corpus(root: &Path) {
    make_maildir(root);
    write_message(root, "new/fresh.host", "inbox unread");
    let work = root.join(".Work");
    make_maildir(&work);
    write_message(&work, "cur/seen.host:2,S", "work read");
    write_message(&work, "new/unseen.host", "work unread");
}

fn sidebar_app(root: &Path) -> App {
    let account = AccountId::new("local");
    let mut engine = MailEngine::new(1).unwrap();
    engine.add_account(
        account.clone(),
        MaildirBackend::new(root.to_path_buf()).unwrap(),
    );
    engine.send(&account, MailCommand::ListFolders).unwrap();
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
    app.add_plugins((
        IndexPlugin,
        PagerPlugin,
        SidebarPlugin,
        OverlayPlugin,
        EnginePlugin,
    ));
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

fn wait_rows(app: &mut App, count: usize) {
    assert!(
        wait_for(app, |world| world.resource::<SidebarRows>().0.len()
            == count),
        "sidebar never reached {count} rows: {:?}",
        row_labels(app)
    );
}

fn row_labels(app: &App) -> Vec<String> {
    app.world()
        .resource::<SidebarRows>()
        .0
        .iter()
        .map(|row| row.label.clone())
        .collect()
}

fn select_row_with_label(app: &mut App, label: &str) {
    let target = app
        .world()
        .resource::<SidebarRows>()
        .0
        .iter()
        .position(|row| row.label == label)
        .unwrap_or_else(|| panic!("no row labelled {label} in {:?}", row_labels(app)));
    app.world_mut().resource_mut::<SidebarState>().selected = target;
}

#[test]
fn tree_shows_folders_with_two_source_unread_counts() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<SidebarRows>().0[0].unread == 1
        }),
        "INBOX unread never derived from the synced store"
    );

    let rows = &app.world().resource::<SidebarRows>().0;
    assert_eq!(rows[0].label, "INBOX");
    assert!(matches!(rows[0].kind, RowKind::Folder(_)));
    assert_eq!(
        rows[1].unread, 1,
        "unsynced Work folder shows the discovery snapshot"
    );
}

#[test]
fn selecting_a_folder_switches_the_index_and_syncs_lazily() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);

    apply_action(
        app.world_mut(),
        &Action::Sidebar(nitidus::action::SidebarOp::ToggleFocus),
    );
    assert!(app.world().resource::<SidebarState>().focused);
    apply_action(app.world_mut(), &Action::Cursor(Motion::Last));
    apply_action(app.world_mut(), &Action::View);

    let world = app.world();
    let index_view = world.resource::<IndexView>();
    assert_eq!(index_view.folder, FolderId::new(".Work"));
    assert!(
        !world.resource::<SidebarState>().focused,
        "selecting a folder returns focus to the index"
    );
    assert!(
        wait_for(&mut app, |world| world.resource::<IndexStatus>().total == 2),
        "the selected folder never synced its two messages"
    );
    assert_eq!(app.world().resource::<IndexStatus>().folder, "Work");
}

#[test]
fn optimistic_flag_edit_updates_the_unread_badge() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);
    assert!(
        wait_for(&mut app, |world| world.resource::<IndexStatus>().total == 1),
        "INBOX never synced"
    );

    apply_action(
        app.world_mut(),
        &Action::Flag {
            flag: Flags::SEEN,
            op: FlagOp::Set,
        },
    );
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<SidebarRows>().0[0].unread == 0
        }),
        "the INBOX badge did not follow the optimistic read"
    );
}

#[test]
fn folder_create_rename_and_empty_delete_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);

    apply_action(app.world_mut(), &Action::FolderCreate("Scratch".to_owned()));
    assert!(
        wait_for(&mut app, |world| {
            world
                .resource::<SidebarRows>()
                .0
                .iter()
                .any(|row| row.label == "Scratch")
        }),
        "created folder never appeared: {:?}",
        row_labels(&app)
    );

    select_row_with_label(&mut app, "Scratch");
    apply_action(app.world_mut(), &Action::FolderRename("Notes".to_owned()));
    assert!(
        wait_for(&mut app, |world| {
            let rows = &world.resource::<SidebarRows>().0;
            rows.iter().any(|row| row.label == "Notes")
                && !rows.iter().any(|row| row.label == "Scratch")
        }),
        "rename never landed: {:?}",
        row_labels(&app)
    );

    select_row_with_label(&mut app, "Notes");
    apply_action(app.world_mut(), &Action::FolderDelete);
    assert!(
        wait_for(&mut app, |world| {
            !world
                .resource::<SidebarRows>()
                .0
                .iter()
                .any(|row| row.label == "Notes")
        }),
        "empty folder was not deleted: {:?}",
        row_labels(&app)
    );
}

#[test]
fn deleting_a_non_empty_folder_is_refused_with_a_warning() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);

    select_row_with_label(&mut app, "Work");
    apply_action(app.world_mut(), &Action::FolderDelete);
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<StatusMessage>().current().is_some()
        }),
        "refusal never surfaced a warning"
    );
    app.update();
    assert!(
        row_labels(&app).contains(&"Work".to_owned()),
        "non-empty folder must survive a delete attempt"
    );
    assert!(tmp.path().join(".Work").is_dir());
}

#[test]
fn deleting_the_viewed_folder_reanchors_to_inbox() {
    let tmp = tempfile::tempdir().unwrap();
    corpus(tmp.path());
    let mut app = sidebar_app(tmp.path());
    wait_rows(&mut app, 2);

    apply_action(app.world_mut(), &Action::FolderCreate("Temp".to_owned()));
    assert!(
        wait_for(&mut app, |world| {
            world
                .resource::<SidebarRows>()
                .0
                .iter()
                .any(|row| row.label == "Temp")
        }),
        "created folder never appeared"
    );

    select_row_with_label(&mut app, "Temp");
    apply_action(
        app.world_mut(),
        &Action::Sidebar(nitidus::action::SidebarOp::ToggleFocus),
    );
    apply_action(app.world_mut(), &Action::View);
    assert_eq!(
        app.world().resource::<IndexView>().folder,
        FolderId::new(".Temp")
    );

    select_row_with_label(&mut app, "Temp");
    apply_action(app.world_mut(), &Action::FolderDelete);
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<IndexView>().folder == FolderId::new("INBOX")
        }),
        "view never reanchored to INBOX after its folder vanished"
    );
}
