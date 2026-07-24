//! Index behavior through the public API: selection and motion, sort
//! changes, statusline position, and the optimistic flag path against a
//! real maildir.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use nitidus::action::{Action, FlagOp, Motion, apply_action};
use nitidus::bootstrap::register_accounts;
use nitidus::config::Config;
use nitidus::config::account::{AccountConfig, Backend, MaildirBackend};
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView, SortKey, SortMode};
use nitidus::status::StatusMessage;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::{
    AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, JobId, MailEngine,
};

fn envelope(id: &str, subject: &str, date: i64) -> EnvelopeSummary {
    EnvelopeSummary {
        id: EnvelopeId::new(id),
        subject: subject.to_owned(),
        from_display: "Alice".to_owned(),
        from_addr: "alice@example.com".to_owned(),
        date_epoch_secs: date,
        flags: Flags::default(),
    }
}

fn account_config(name: &str) -> Config {
    let mut config = Config::default();
    config.accounts.push(AccountConfig {
        name: name.to_owned(),
        ..Default::default()
    });
    config
}

fn index_app(config: Config, envelopes: Vec<EnvelopeSummary>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(plurimus::TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.insert_resource(config);
    let mut store = MailStore::default();
    if !envelopes.is_empty() {
        store.apply_batch(
            &AccountId::new("local"),
            &FolderId::new("INBOX"),
            JobId(1),
            envelopes,
            true,
        );
    }
    app.insert_resource(store);
    app.init_resource::<SyncTracker>();
    app.init_resource::<StatusMessage>();
    app.add_plugins(IndexPlugin);
    app.update();
    app
}

fn status(app: &App) -> IndexStatus {
    app.world().resource::<IndexStatus>().clone()
}

fn selected_id(app: &App) -> Option<String> {
    app.world()
        .resource::<IndexView>()
        .selected
        .as_ref()
        .map(|id| id.as_str().to_owned())
}

#[test]
fn selection_defaults_to_first_and_motions_move_it() {
    let envelopes = vec![
        envelope("newest", "one", 300),
        envelope("middle", "two", 200),
        envelope("oldest", "three", 100),
    ];
    let mut app = index_app(account_config("local"), envelopes);
    assert_eq!(status(&app), IndexStatus { selected: 1, total: 3 });
    assert_eq!(selected_id(&app).as_deref(), Some("newest"));

    apply_action(app.world_mut(), &Action::Cursor(Motion::Next));
    app.update();
    assert_eq!(status(&app).selected, 2);

    apply_action(app.world_mut(), &Action::Cursor(Motion::Last));
    app.update();
    assert_eq!(status(&app).selected, 3);
    assert_eq!(selected_id(&app).as_deref(), Some("oldest"));

    apply_action(app.world_mut(), &Action::Cursor(Motion::Prev));
    app.update();
    assert_eq!(status(&app).selected, 2);
    assert_eq!(selected_id(&app).as_deref(), Some("middle"));
}

#[test]
fn sort_change_reorders_but_selection_follows_the_id() {
    let envelopes = vec![
        envelope("newest", "zzz", 300),
        envelope("middle", "aaa", 200),
        envelope("oldest", "mmm", 100),
    ];
    let mut app = index_app(account_config("local"), envelopes);
    assert_eq!(selected_id(&app).as_deref(), Some("newest"));

    let by_subject = SortMode {
        key: SortKey::Subject,
        reverse: false,
    };
    apply_action(app.world_mut(), &Action::Sort(by_subject));
    app.update();
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("newest"),
        "sorting must not move the selection off its message"
    );
    assert_eq!(
        status(&app).selected,
        3,
        "the selected message sorts last by subject"
    );
}

#[test]
fn empty_configurations_report_zero_status() {
    let app = index_app(Config::default(), Vec::new());
    assert_eq!(status(&app), IndexStatus::default());
    assert_eq!(selected_id(&app), None);

    let app = index_app(account_config("local"), Vec::new());
    assert_eq!(status(&app), IndexStatus::default());
}

fn make_maildir(root: &Path) {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
}

#[test]
fn flag_toggle_is_optimistic_and_renames_the_maildir_file() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    let body = "From: A <a@example.com>\r\nSubject: hello\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\n\r\nx\r\n";
    std::fs::write(tmp.path().join("new/msg1.host"), body).unwrap();

    let mut config = account_config("local");
    config.accounts[0].backend = Some(Backend::Maildir(MaildirBackend {
        path: tmp.path().to_path_buf(),
    }));
    let mut engine = MailEngine::new(1).unwrap();
    let mut tracker = SyncTracker::default();
    register_accounts(&mut engine, &config, &mut tracker, &mut Vec::new()).unwrap();

    let mut app = index_app(config, Vec::new());
    app.insert_resource(EngineResource(engine));
    app.insert_resource(tracker);
    app.add_plugins(EnginePlugin);

    for _ in 0..400 {
        app.update();
        if status(&app).total == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(status(&app).total, 1, "scan never reached the index");

    apply_action(
        app.world_mut(),
        &Action::Flag {
            flag: Flags::FLAGGED,
            op: FlagOp::Toggle,
        },
    );
    let account = AccountId::new("local");
    let inbox = FolderId::new("INBOX");
    let store = app.world().resource::<MailStore>();
    assert!(
        store.envelopes(&account, &inbox)[0]
            .flags
            .contains(Flags::FLAGGED),
        "store must update before the backend write lands"
    );

    let flagged_path = tmp.path().join("cur/msg1.host:2,F");
    for _ in 0..400 {
        if flagged_path.exists() {
            return;
        }
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("maildir file was never renamed with the flag suffix");
}
