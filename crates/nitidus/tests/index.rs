//! Index behavior through the public API: selection and motion, sort
//! changes, statusline position, and the optimistic flag path against a
//! real maildir.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use nitidus::action::{Action, FlagOp, FoldOp, Motion, apply_action};
use nitidus::bootstrap::register_accounts;
use nitidus::config::Config;
use nitidus::config::account::{AccountConfig, Backend, MaildirBackend};
use nitidus::engine::{EnginePlugin, EngineResource};
use nitidus::index::{IndexPlugin, IndexStatus, IndexView, SortKey, SortMode};
use nitidus::status::StatusMessage;
use nitidus::store::{MailStore, SyncTracker};
use nitidus_mail::{AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, JobId, MailEngine};

fn envelope(id: &str, subject: &str, date: i64) -> EnvelopeSummary {
    EnvelopeSummary {
        id: EnvelopeId::new(id),
        subject: subject.to_owned(),
        from_display: "Alice".to_owned(),
        from_addr: "alice@example.com".to_owned(),
        date_epoch_secs: date,
        flags: Flags::default(),
        message_id: format!("{id}@example"),
        references: Vec::new(),
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
    app.init_resource::<nitidus::keymap::Mode>();
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
    assert_eq!(
        status(&app),
        IndexStatus {
            selected: 1,
            total: 3,
            folder: "INBOX".to_owned(),
            folder_total: 3,
            limits: String::new()
        }
    );
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
    assert_eq!(
        status(&app),
        IndexStatus {
            selected: 0,
            total: 0,
            folder: "INBOX".to_owned(),
            folder_total: 0,
            limits: String::new()
        },
        "a configured account shows its viewed folder even before folders load"
    );
}

fn envelope_from(id: &str, subject: &str, display: &str, addr: &str, date: i64) -> EnvelopeSummary {
    EnvelopeSummary {
        from_display: display.to_owned(),
        from_addr: addr.to_owned(),
        ..envelope(id, subject, date)
    }
}

#[test]
fn limits_stack_filter_counts_and_clear_restores() {
    let mut app = index_app(
        account_config("local"),
        vec![
            envelope_from("a", "quarterly report", "Ada", "ada@x.example", 300),
            envelope_from("b", "report card", "Zed", "zed@y.example", 200),
            envelope_from("c", "lunch plans", "Ada", "ada@x.example", 100),
        ],
    );

    apply_action(app.world_mut(), &Action::Limit("report".to_owned()));
    app.update();
    let limited = status(&app);
    assert_eq!(limited.total, 2, "one limit filters to matching rows");
    assert_eq!(limited.folder_total, 3);
    assert_eq!(limited.limits, "report");

    apply_action(app.world_mut(), &Action::Limit("ada".to_owned()));
    app.update();
    let stacked = status(&app);
    assert_eq!(stacked.total, 1, "stacked limits AND together");
    assert_eq!(stacked.limits, "report+ada");
    assert_eq!(selected_id(&app).as_deref(), Some("a"));

    apply_action(app.world_mut(), &Action::ClearFilters);
    app.update();
    let cleared = status(&app);
    assert_eq!(cleared.total, 3);
    assert_eq!(cleared.limits, "");
}

#[test]
fn limits_suspend_threading_and_clear_restores_it() {
    let mut parent = envelope("p", "root subject", 300);
    parent.message_id = "p@example".to_owned();
    let mut child = envelope("c", "Re: root subject", 200);
    child.message_id = "c@example".to_owned();
    child.references = vec!["p@example".to_owned()];
    let mut app = index_app(account_config("local"), vec![parent, child]);
    apply_action(app.world_mut(), &Action::ToggleThreads);
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }

    apply_action(app.world_mut(), &Action::Limit("root".to_owned()));
    app.update();
    assert_eq!(
        status(&app).total,
        2,
        "both rows match; limited view is flat"
    );

    apply_action(app.world_mut(), &Action::ClearFilters);
    app.update();
    assert_eq!(status(&app).total, 2);
    assert!(
        app.world().resource::<IndexView>().threaded,
        "threading preference survives the limit round-trip"
    );
}

fn search_key(app: &mut App, code: bevy_ratatui::crossterm::event::KeyCode) {
    nitidus::index::search::handle_key(
        app.world_mut(),
        bevy_ratatui::crossterm::event::KeyEvent::from(code),
    )
    .unwrap();
    app.update();
}

fn search_type(app: &mut App, text: &str) {
    for character in text.chars() {
        search_key(
            app,
            bevy_ratatui::crossterm::event::KeyCode::Char(character),
        );
    }
}

fn searchable_app() -> App {
    index_app(
        account_config("local"),
        vec![
            envelope("a", "alpha subject", 300),
            envelope("b", "beta subject", 200),
            envelope("c", "gamma beta", 100),
        ],
    )
}

#[test]
fn incremental_search_jumps_live_and_esc_restores() {
    use bevy_ratatui::crossterm::event::KeyCode;
    let mut app = searchable_app();
    assert_eq!(selected_id(&app).as_deref(), Some("a"));

    apply_action(app.world_mut(), &Action::SearchStart);
    search_type(&mut app, "beta");
    assert_eq!(selected_id(&app).as_deref(), Some("b"), "live jump");

    search_type(&mut app, "x");
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("a"),
        "an unmatched query returns to the origin"
    );
    search_key(&mut app, KeyCode::Backspace);
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("b"),
        "backspace re-jumps"
    );

    search_key(&mut app, KeyCode::Esc);
    assert_eq!(selected_id(&app).as_deref(), Some("a"), "Esc restores");
    assert_eq!(app.world().resource::<IndexView>().search, None);
}

#[test]
fn accepted_search_repeats_with_wrap_in_both_directions() {
    use bevy_ratatui::crossterm::event::KeyCode;
    let mut app = searchable_app();
    apply_action(app.world_mut(), &Action::SearchStart);
    search_type(&mut app, "beta");
    search_key(&mut app, KeyCode::Enter);
    assert_eq!(selected_id(&app).as_deref(), Some("b"));

    apply_action(app.world_mut(), &Action::SearchNext);
    app.update();
    assert_eq!(selected_id(&app).as_deref(), Some("c"));
    apply_action(app.world_mut(), &Action::SearchNext);
    app.update();
    assert_eq!(selected_id(&app).as_deref(), Some("b"), "next wraps");
    apply_action(app.world_mut(), &Action::SearchPrev);
    app.update();
    assert_eq!(selected_id(&app).as_deref(), Some("c"), "prev wraps back");

    apply_action(app.world_mut(), &Action::ClearFilters);
    apply_action(app.world_mut(), &Action::SearchNext);
    app.update();
    let (message, _) = app.world().resource::<StatusMessage>().current().unwrap();
    assert_eq!(message, "no search (press /)");
}

#[test]
fn search_operates_within_the_active_limit() {
    use bevy_ratatui::crossterm::event::KeyCode;
    let mut app = searchable_app();
    apply_action(app.world_mut(), &Action::Limit("beta".to_owned()));
    app.update();
    assert_eq!(status(&app).total, 2);

    apply_action(app.world_mut(), &Action::SearchStart);
    search_type(&mut app, "gamma");
    search_key(&mut app, KeyCode::Enter);
    assert_eq!(selected_id(&app).as_deref(), Some("c"));
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

fn threaded_fixture() -> Vec<EnvelopeSummary> {
    let mut root = envelope("root", "the thread root", 100);
    root.message_id = "r@x".to_owned();
    let mut reply = envelope("reply", "Re: the thread root", 300);
    reply.message_id = "re@x".to_owned();
    reply.references = vec!["r@x".to_owned()];
    let lone = envelope("lone", "unrelated", 200);
    vec![root, reply, lone]
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

#[test]
fn threading_folds_and_parent_jump_work_end_to_end() {
    let mut app = index_app(account_config("local"), threaded_fixture());
    app.insert_resource(EngineResource(MailEngine::new(1).unwrap()));
    app.add_plugins(EnginePlugin);
    app.update();
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("reply"),
        "flat date order first"
    );
    assert_eq!(status(&app).selected, 1);

    apply_action(app.world_mut(), &Action::ToggleThreads);
    assert!(
        wait_for(&mut app, |world| {
            world.resource::<IndexStatus>().selected == 2
        }),
        "threaded order never arrived (selection should sit at row 2 of root,reply,lone)"
    );
    assert_eq!(selected_id(&app).as_deref(), Some("reply"));

    apply_action(app.world_mut(), &Action::Cursor(Motion::Parent));
    app.update();
    assert_eq!(selected_id(&app).as_deref(), Some("root"));
    assert_eq!(status(&app).selected, 1);

    apply_action(app.world_mut(), &Action::Fold(FoldOp::Toggle));
    app.update();
    apply_action(app.world_mut(), &Action::Cursor(Motion::Next));
    app.update();
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("lone"),
        "next from a collapsed root must skip its hidden reply"
    );

    apply_action(app.world_mut(), &Action::Fold(FoldOp::ExpandAll));
    app.update();
    apply_action(app.world_mut(), &Action::Cursor(Motion::Prev));
    app.update();
    assert_eq!(
        selected_id(&app).as_deref(),
        Some("reply"),
        "expanding must reveal the reply row again"
    );

    apply_action(app.world_mut(), &Action::ToggleThreads);
    app.update();
    assert_eq!(selected_id(&app).as_deref(), Some("reply"));
    assert_eq!(status(&app).selected, 1, "back to flat date order");
}
