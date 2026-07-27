//! The modernized default layout through the router: bracket and
//! number tab switching, the `,` sort family, `*` flag, and `D`
//! permanent-delete confirmation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::config::RawKeymaps;
use nitidus::index::{IndexView, SortKey};
use nitidus::keymap::Keymaps;
use nitidus::overlay::OverlayPlugin;
use nitidus::pager::PagerState;
use nitidus::router::{RouterPlugin, route_key};
use nitidus::shell::Tabs;
use nitidus::sidebar::{RowKind, SidebarRow, SidebarState};
use nitidus::status::MessageLog;
use nitidus::store::MailStore;
use nitidus_mail::{AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, JobId};
use plurimus::{TachyonRegistry, UiEvent};

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.init_resource::<Tabs>();
    app.init_resource::<SidebarState>();
    app.init_resource::<nitidus::sidebar::SidebarRows>();
    app.init_resource::<nitidus::store::SyncTracker>();
    app.init_resource::<MessageLog>();
    app.init_resource::<MailStore>();
    app.init_resource::<IndexView>();
    app.init_resource::<PagerState>();
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.add_plugins((RouterPlugin, OverlayPlugin));
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

fn seed_selection(app: &mut App) {
    let account = AccountId::new("local");
    let folder = FolderId::new("INBOX");
    let envelope = EnvelopeSummary {
        id: EnvelopeId::new("7"),
        subject: "hello".to_owned(),
        from_display: "Ada".to_owned(),
        from_addr: "ada@x.example".to_owned(),
        date_epoch_secs: 1_700_000_000,
        flags: Flags::default(),
        message_id: "m@x".to_owned(),
        references: Vec::new(),
    };
    let world = app.world_mut();
    world.resource_mut::<MailStore>().apply_batch(
        &account,
        &folder,
        JobId(1),
        vec![envelope],
        true,
    );
    let mut view = world.resource_mut::<IndexView>();
    view.account = Some(account);
    view.folder = folder;
    view.selected = Some(EnvelopeId::new("7"));
}

#[test]
fn brackets_and_numbers_switch_tabs() {
    let mut app = harness();
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.world().resource::<Tabs>().active_label(), "contacts");
    assert!(app.world().resource::<Tabs>().is_contacts());

    press(&mut app, KeyCode::Char('['));
    assert_eq!(app.world().resource::<Tabs>().active_label(), "mail");
    assert!(!app.world().resource::<Tabs>().is_contacts());

    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.world().resource::<Tabs>().active_label(), "contacts");
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(app.world().resource::<Tabs>().active_label(), "mail");
}

#[test]
fn comma_sort_family_sets_key_and_reverse() {
    let mut app = harness();
    press(&mut app, KeyCode::Char(','));
    press(&mut app, KeyCode::Char('f'));
    assert_eq!(app.world().resource::<IndexView>().sort.key, SortKey::From);

    press(&mut app, KeyCode::Char(','));
    press(&mut app, KeyCode::Char('r'));
    assert!(app.world().resource::<IndexView>().sort.reverse);

    press(&mut app, KeyCode::Char(','));
    press(&mut app, KeyCode::Char(','));
    let sort = app.world().resource::<IndexView>().sort;
    assert_eq!(sort.key, SortKey::Date);
    assert!(!sort.reverse, ",, resets to the date default");
}

#[test]
fn star_toggles_the_flag() {
    let mut app = harness();
    seed_selection(&mut app);
    press(&mut app, KeyCode::Char('*'));
    let world = app.world();
    let store = world.resource::<MailStore>();
    let envelope = &store.envelopes(&AccountId::new("local"), &FolderId::new("INBOX"))[0];
    assert!(envelope.flags.contains(Flags::FLAGGED));
}

#[test]
fn arrows_move_focus_between_sidebar_index_and_contact_panes() {
    let mut app = harness();
    app.init_resource::<nitidus::contacts::ContactsView>();
    app.update();

    press(&mut app, KeyCode::Left);
    {
        assert!(
            app.world().resource::<SidebarState>().visible
                && nitidus::focus::is_focused(app.world(), nitidus::focus::Pane::Folders),
            "left focuses the sidebar"
        );
    }
    app.world_mut()
        .resource_mut::<nitidus::sidebar::SidebarRows>()
        .0 = vec![SidebarRow {
        account: AccountId::new("local"),
        path: "Archive".to_owned(),
        label: "Archive".to_owned(),
        kind: RowKind::Folder(FolderId::new("Archive")),
        depth: 0,
        has_children: false,
        is_collapsed: false,
        unread: 0,
    }];
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.world().resource::<IndexView>().folder,
        FolderId::new("Archive"),
        "right on a folder row opens that folder"
    );
    assert!(
        !nitidus::focus::is_focused(app.world(), nitidus::focus::Pane::Folders),
        "opening a folder hands focus back to the index"
    );

    app.world_mut().resource_mut::<Tabs>().active = 1;
    app.update();
    press(&mut app, KeyCode::Right);
    assert!(nitidus::focus::is_focused(
        app.world(),
        nitidus::focus::Pane::ContactDetail
    ));
    press(&mut app, KeyCode::Left);
    assert!(nitidus::focus::is_focused(
        app.world(),
        nitidus::focus::Pane::ContactList
    ));
}

#[test]
fn capital_d_confirms_permanent_delete_outside_the_trash() {
    let mut app = harness();
    seed_selection(&mut app);
    press(&mut app, KeyCode::Char('D'));
    let question = app
        .world()
        .resource::<nitidus::overlay::confirm::ActiveConfirm>()
        .question()
        .map(str::to_owned);
    assert!(
        question
            .as_deref()
            .is_some_and(|text| text.contains("permanently")),
        "D must ask before a permanent delete, got {question:?}"
    );
}
