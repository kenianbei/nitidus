//! Contact book end to end over a seeded vdir: tab activation, table
//! and detail navigation, statusline counts, and lenient loading.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
use nitidus::action::{Action, apply_action};
use nitidus::config::RawKeymaps;
use nitidus::contacts::{ContactsDir, ContactsPlugin, ContactsStatus, ContactsView, PaneFocus};
use nitidus::engine::StartupNotices;
use nitidus::keymap::Keymaps;
use nitidus::overlay::OverlayPlugin;
use nitidus::prompt::{PromptPlugin, PromptState};
use nitidus::router::{RouterPlugin, route_key};
use nitidus::screen::{MailScreenMemory, Screen};
use nitidus::shell::Tabs;
use nitidus::sidebar::SidebarState;
use nitidus::status::StatusMessage;
use plurimus::{TachyonRegistry, UiEvent};

fn write_contact(dir: &Path, uid: &str, name: &str, email: &str) {
    let card = format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{uid}\r\nFN:{name}\r\nX-CUSTOM;X-PARAM=zig:zag\r\nEMAIL;TYPE=work:{email}\r\nTEL;TYPE=cell:+1-555-0100\r\nEND:VCARD\r\n"
    );
    std::fs::write(dir.join(format!("{uid}.vcf")), card).unwrap();
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

fn read_card(dir: &Path, uid: &str) -> String {
    std::fs::read_to_string(dir.join(format!("{uid}.vcf"))).unwrap()
}

fn harness(seed: impl FnOnce(&Path)) -> (App, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    seed(root.path());

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_non_send_resource(TachyonRegistry::default());
    app.insert_resource(nitidus_ui_kit::theme::tailwind_dark());
    app.insert_resource(ContactsDir(root.path().to_path_buf()));
    app.insert_resource(StartupNotices(Vec::new()));
    app.init_resource::<Tabs>();
    app.init_resource::<Screen>();
    app.init_resource::<MailScreenMemory>();
    app.init_resource::<SidebarState>();
    app.init_resource::<StatusMessage>();
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.add_plugins((RouterPlugin, PromptPlugin, OverlayPlugin, ContactsPlugin));
    app.update();
    (app, root)
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

fn open_contacts(app: &mut App) {
    apply_action(app.world_mut(), &Action::Contacts);
    app.update();
}

#[test]
fn seeded_vdir_loads_sorted_and_navigates() {
    let (mut app, _root) = harness(|dir| {
        write_contact(dir, "uid-c", "Zoe", "zoe@example.com");
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
        write_contact(dir, "uid-b", "Mel", "mel@example.com");
    });
    open_contacts(&mut app);
    assert_eq!(*app.world().resource::<Screen>(), Screen::Contacts);
    assert_eq!(
        *app.world().resource::<ContactsStatus>(),
        ContactsStatus {
            selected: 1,
            total: 3
        }
    );

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.world().resource::<ContactsView>().selected, 1);
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.world().resource::<ContactsView>().selected, 2);
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.world().resource::<ContactsView>().selected, 0);
    assert_eq!(app.world().resource::<ContactsStatus>().selected, 1);
}

#[test]
fn tab_key_toggles_pane_focus_and_detail_cursor_moves() {
    let (mut app, _root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    assert_eq!(
        app.world().resource::<ContactsView>().focus,
        PaneFocus::Table
    );

    press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.world().resource::<ContactsView>().focus,
        PaneFocus::Detail
    );
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.world().resource::<ContactsView>().detail_selected, 1);
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.world().resource::<ContactsView>().detail_selected, 0);

    press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.world().resource::<ContactsView>().focus,
        PaneFocus::Table
    );
}

#[test]
fn moving_the_table_selection_resets_the_detail_cursor() {
    let (mut app, _root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
        write_contact(dir, "uid-b", "Mel", "mel@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.world().resource::<ContactsView>().detail_selected, 1);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.world().resource::<ContactsView>().selected, 1);
    assert_eq!(app.world().resource::<ContactsView>().detail_selected, 0);
}

/// Detail rows for the seeded card, rank-ordered:
/// 0 FN, 1 EMAIL, 2 TEL, 3 X-CUSTOM, 4 UID.
const TEL_ROW_STEPS: usize = 2;

#[test]
fn editing_a_phone_saves_and_preserves_exotic_properties() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Tab);
    for _ in 0..TEL_ROW_STEPS {
        press(&mut app, KeyCode::Char('j'));
    }
    press(&mut app, KeyCode::Char('e'));
    type_text(&mut app, "9");
    press(&mut app, KeyCode::Enter);

    let card = read_card(root.path(), "uid-a");
    assert!(
        card.contains("TEL;TYPE=CELL:+1-555-01009"),
        "the edited value must reach disk with its TYPE intact: {card}"
    );
    assert!(
        card.contains("X-CUSTOM;X-PARAM=zig:zag"),
        "unmodeled properties must survive an edit: {card}"
    );
}

#[test]
fn n_edit_prefills_every_component_so_enter_through_preserves_them() {
    let (mut app, root) = harness(|dir| {
        let card = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:uid-n\r\nFN:Ada Lovelace\r\nN:Lovelace;Ada;Augusta;Countess;\r\nEND:VCARD\r\n";
        std::fs::write(dir.join("uid-n.vcf"), card).unwrap();
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('e'));
    for _ in 0..5 {
        press(&mut app, KeyCode::Enter);
    }
    let card = read_card(root.path(), "uid-n");
    assert!(
        card.contains("N:Lovelace;Ada;Augusta;Countess;"),
        "keeping every prefill must not lose components: {card}"
    );
}

#[test]
fn add_flow_files_a_typed_email() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Char('a'));
    type_text(&mut app, "email");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "work");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "ada@work.example");
    press(&mut app, KeyCode::Enter);

    let card = read_card(root.path(), "uid-a");
    assert!(
        card.contains("EMAIL;TYPE=WORK:ada@work.example"),
        "the added email must reach disk: {card}"
    );
}

#[test]
fn raw_editor_rejects_uid_replacement_and_reprompts() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('G'));
    press(&mut app, KeyCode::Char('E'));
    let prefill = app
        .world()
        .resource::<PromptState>()
        .value()
        .unwrap()
        .to_owned();
    assert_eq!(prefill, "UID:uid-a", "the raw editor prefills the line");
    for _ in 0..prefill.chars().count() {
        press(&mut app, KeyCode::Backspace);
    }
    type_text(&mut app, "UID:sneaky");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.world().resource::<PromptState>().is_open(),
        "a rejected line must re-prompt"
    );
    assert!(
        read_card(root.path(), "uid-a").contains("UID:uid-a"),
        "the uid must be untouched"
    );
}

#[test]
fn removing_a_property_rewrites_the_file() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Tab);
    for _ in 0..TEL_ROW_STEPS {
        press(&mut app, KeyCode::Char('j'));
    }
    press(&mut app, KeyCode::Char('x'));
    let card = read_card(root.path(), "uid-a");
    assert!(
        !card.contains("TEL"),
        "the removed property is gone: {card}"
    );
}

#[test]
fn new_contact_chain_creates_a_file_and_selects_it() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Bob");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.world().resource::<ContactsStatus>().total, 2);
    assert_eq!(
        app.world().resource::<ContactsView>().selected,
        1,
        "the new contact (Bob, after Ada) is selected"
    );
    let files = std::fs::read_dir(root.path()).unwrap().count();
    assert_eq!(files, 2, "one new .vcf file");
}

#[test]
fn delete_contact_confirms_and_removes_the_file() {
    let (mut app, root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
        write_contact(dir, "uid-b", "Mel", "mel@example.com");
    });
    open_contacts(&mut app);
    press(&mut app, KeyCode::Char('D'));
    type_text(&mut app, "n");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<ContactsStatus>().total,
        2,
        "declining keeps the contact"
    );

    press(&mut app, KeyCode::Char('D'));
    type_text(&mut app, "y");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<ContactsStatus>().total, 1);
    assert!(
        !root.path().join("uid-a.vcf").exists(),
        "Ada's file is gone"
    );
    assert!(root.path().join("uid-b.vcf").exists(), "Mel's file remains");
}

#[test]
fn malformed_files_become_startup_notices() {
    let (app, _root) = harness(|dir| {
        write_contact(dir, "uid-a", "Ada", "ada@example.com");
        std::fs::write(dir.join("broken.vcf"), "not a vcard").unwrap();
    });
    let notices = &app.world().resource::<StartupNotices>().0;
    assert_eq!(notices.len(), 1, "exactly one notice: {notices:?}");
    assert!(
        notices[0].contains("broken.vcf"),
        "the notice names the file: {notices:?}"
    );
    assert_eq!(app.world().resource::<ContactsStatus>().total, 1);
}
