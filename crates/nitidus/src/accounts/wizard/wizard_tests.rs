#![allow(clippy::unwrap_used, clippy::expect_used)]

use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::*;
use crate::config::RawKeymaps;
use crate::config::account::AccountConfig;
use crate::config::keyring::use_mock_keyring;
use crate::keymap::{Keymaps, Mode};
use crate::overlay::form::{ActiveForm, handle_key};

struct Harness {
    app: App,
    config_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    use_mock_keyring();
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.init_resource::<Mode>();
    app.init_resource::<StatusMessage>();
    app.init_resource::<ActiveForm>();
    app.init_resource::<IndexView>();
    app.init_resource::<Screen>();
    app.init_resource::<Config>();
    app.insert_resource(Keymaps::compile(&RawKeymaps::default()).unwrap());
    app.insert_resource(super::super::ConfigFilePath(config_path.clone()));
    Harness {
        app,
        config_path,
        _dir: dir,
    }
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(app.world_mut(), KeyEvent::from(code)).unwrap();
}

fn type_str(app: &mut App, text: &str) {
    for character in text.chars() {
        press(app, KeyCode::Char(character));
    }
}

/// Clears the focused field, then types — the fields carry defaults, so
/// a test that means to override one has to say so.
fn replace(app: &mut App, text: &str) {
    for _ in 0..64 {
        press(app, KeyCode::Backspace);
    }
    type_str(app, text);
}

fn advance(app: &mut App) {
    press(app, KeyCode::PageDown);
}

fn select(app: &mut App, steps: usize) {
    for _ in 0..steps {
        press(app, KeyCode::Right);
    }
}

fn value(app: &App, id: &str) -> String {
    app.world().resource::<ActiveForm>().value(id).unwrap()
}

fn written_config(harness: &Harness) -> Config {
    toml::from_str(&std::fs::read_to_string(&harness.config_path).unwrap()).unwrap()
}

fn fill_account(app: &mut App, name: &str, email: &str, display: &str) {
    replace(app, name);
    press(app, KeyCode::Tab);
    replace(app, email);
    press(app, KeyCode::Tab);
    replace(app, display);
}

#[test]
fn the_form_opens_on_the_account_step_with_the_provider_step_ahead_of_it() {
    let mut harness = harness();
    start(harness.app.world_mut());
    let form = harness.app.world().resource::<ActiveForm>();
    assert_eq!(form.title(), Some("new account"));
    assert_eq!(form.page(), Some(0));
    assert_eq!(
        form.step_titles(),
        vec![
            "Account".to_owned(),
            "Provider".to_owned(),
            "Credentials".to_owned(),
        ],
        "Gmail over OAuth2 is the default, so its credentials step is there"
    );
}

#[test]
fn gmail_with_a_keyring_password_writes_presets_and_chains_the_password_form() {
    let mut harness = harness();
    start(harness.app.world_mut());
    fill_account(&mut harness.app, "wiz-gmail", "wiz@gmail.com", "Wiz Ard");
    advance(&mut harness.app);

    // Provider defaults to Gmail; move to auth and take the keyring.
    press(&mut harness.app, KeyCode::Tab);
    select(&mut harness.app, 1);
    assert_eq!(value(&harness.app, "auth"), "keyring");
    assert_eq!(
        harness.app.world().resource::<ActiveForm>().step_titles(),
        vec!["Account".to_owned(), "Provider".to_owned()],
        "keyring auth drops the credentials step again"
    );
    press(&mut harness.app, KeyCode::Enter);

    let config = written_config(&harness);
    let account = &config.accounts[0];
    assert_eq!(account.name, "wiz-gmail");
    assert_eq!(account.display_name, "Wiz Ard");
    assert!(matches!(&account.backend, Some(Backend::Imap(imap)) if imap.host == "imap.gmail.com"));
    assert_eq!(account.folders.drafts, "[Gmail]/Drafts");
    assert!(!account.folders.save_sent);
    assert_eq!(account.auth, Auth::Keyring);

    let view = harness.app.world().resource::<IndexView>();
    assert_eq!(
        view.account.as_ref().map(|id| id.as_str().to_owned()),
        Some("wiz-gmail".to_owned()),
        "the new account becomes active"
    );
    assert_eq!(
        harness.app.world().resource::<ActiveForm>().title(),
        Some("password — wiz-gmail"),
        "a keyring account without a secret chains into set-password"
    );
    type_str(&mut harness.app, "app-pass");
    press(&mut harness.app, KeyCode::Enter);
    assert!(keyring::load_password("wiz-gmail").is_ok());
}

#[test]
fn custom_imap_grows_a_servers_step_and_takes_a_password_command() {
    let mut harness = harness();
    start(harness.app.world_mut());
    fill_account(&mut harness.app, "wiz-custom", "me@custom.net", "");
    advance(&mut harness.app);

    select(&mut harness.app, 2); // Custom IMAP
    assert_eq!(
        harness.app.world().resource::<ActiveForm>().step_titles(),
        vec![
            "Account".to_owned(),
            "Provider".to_owned(),
            "Servers".to_owned(),
            "Credentials".to_owned(),
        ],
        "Custom IMAP adds hosts and folders ahead of the OAuth defaults"
    );
    press(&mut harness.app, KeyCode::Tab);
    select(&mut harness.app, 2); // password command

    advance(&mut harness.app);
    replace(&mut harness.app, "mail.custom.net");
    press(&mut harness.app, KeyCode::Tab);
    replace(&mut harness.app, "smtp.custom.net");
    press(&mut harness.app, KeyCode::Tab);
    press(&mut harness.app, KeyCode::Tab); // keep the Drafts default
    replace(&mut harness.app, "Outbox");

    advance(&mut harness.app);
    replace(&mut harness.app, "pass show custom");
    press(&mut harness.app, KeyCode::Enter);

    let config = written_config(&harness);
    let account = &config.accounts[0];
    assert!(
        matches!(&account.outgoing, Some(Outgoing::Smtp(smtp)) if smtp.host == "smtp.custom.net")
    );
    assert!(
        matches!(&account.backend, Some(Backend::Imap(imap)) if imap.host == "mail.custom.net")
    );
    assert_eq!(account.folders.sent, "Outbox");
    assert_eq!(
        account.folders.drafts, "Drafts",
        "untouched defaults survive"
    );
    assert_eq!(account.folders.trash, "Trash");
    assert!(matches!(&account.auth, Auth::PasswordCmd(cmd) if cmd.command == "pass show custom"));
}

#[test]
fn outlook_with_oauth_pins_the_code_flow_and_the_thunderbird_client_id() {
    let mut harness = harness();
    // A grant already on file, so finishing does not chain into
    // :authorize — that flow is feature-oauth2-v1's to test.
    keyring::store_oauth_refresh("wiz-o365", &"refresh".to_owned().into()).unwrap();
    start(harness.app.world_mut());
    fill_account(&mut harness.app, "wiz-o365", "me@contoso.com", "");
    advance(&mut harness.app);

    select(&mut harness.app, 1); // Outlook
    assert_eq!(value(&harness.app, "auth"), "oauth2", "the default");
    advance(&mut harness.app);
    assert_eq!(
        value(&harness.app, "client_id"),
        presets::THUNDERBIRD_CLIENT_ID,
        "a Microsoft tenant gets the registration its consent screen knows"
    );
    press(&mut harness.app, KeyCode::Enter);

    let config = written_config(&harness);
    let Auth::Oauth2(oauth) = &config.accounts[0].auth else {
        panic!(
            "expected an oauth2 account, got {:?}",
            config.accounts[0].auth
        );
    };
    assert_eq!(oauth.provider, Oauth2Provider::Microsoft);
    assert_eq!(oauth.flow, Some(Oauth2Flow::Code));
    assert_eq!(oauth.client_secret, None, "an empty secret stays absent");
}

#[test]
fn an_empty_name_holds_the_form_on_the_account_step() {
    let mut harness = harness();
    start(harness.app.world_mut());
    advance(&mut harness.app);
    let form = harness.app.world().resource::<ActiveForm>();
    assert_eq!(form.page(), Some(0), "a broken step is not walked past");
    assert_eq!(form.message(), Some("account name must not be empty"));
}

#[test]
fn an_email_without_an_at_sign_is_refused() {
    let mut harness = harness();
    start(harness.app.world_mut());
    replace(&mut harness.app, "wiz");
    press(&mut harness.app, KeyCode::Tab);
    replace(&mut harness.app, "not-an-email");
    advance(&mut harness.app);
    assert_eq!(
        harness.app.world().resource::<ActiveForm>().message(),
        Some("email must contain @")
    );
}

#[test]
fn a_duplicate_name_is_refused_rather_than_overwriting_the_other_account() {
    let mut harness = harness();
    harness
        .app
        .world_mut()
        .resource_mut::<Config>()
        .accounts
        .push(AccountConfig {
            name: "taken".to_owned(),
            ..Default::default()
        });
    start(harness.app.world_mut());
    fill_account(&mut harness.app, "taken", "me@x.example", "");
    advance(&mut harness.app);
    press(&mut harness.app, KeyCode::Tab);
    select(&mut harness.app, 1); // keyring
    press(&mut harness.app, KeyCode::Enter);

    assert!(
        !harness.config_path.exists(),
        "nothing may be written for a name already in use"
    );
    assert_eq!(harness.app.world().resource::<Config>().accounts.len(), 1);
}

#[test]
fn editing_prefills_every_step_and_reaches_them_all_at_once() {
    let mut harness = harness();
    let account = AccountConfig {
        name: "existing".to_owned(),
        email: "me@custom.net".to_owned(),
        backend: Some(Backend::Imap(ImapBackend {
            host: "mail.custom.net".to_owned(),
            ..Default::default()
        })),
        outgoing: Some(Outgoing::Smtp(SmtpOutgoing {
            host: "smtp.custom.net".to_owned(),
            ..Default::default()
        })),
        auth: Auth::Keyring,
        ..Default::default()
    };
    crate::config::write::append_account(&harness.config_path, &account).unwrap();
    harness
        .app
        .world_mut()
        .resource_mut::<Config>()
        .accounts
        .push(account);
    start_edit(harness.app.world_mut(), "existing");

    let form = harness.app.world().resource::<ActiveForm>();
    assert_eq!(form.title(), Some("edit account — existing"));
    assert_eq!(form.value("name").unwrap(), "existing");
    assert_eq!(form.value("provider").unwrap(), "custom");
    assert_eq!(form.value("imap_host").unwrap(), "mail.custom.net");

    // The Servers step is reachable straight away rather than by walking.
    crate::overlay::form::go_to_page(harness.app.world_mut(), 2);
    assert_eq!(harness.app.world().resource::<ActiveForm>().page(), Some(2));
    replace(&mut harness.app, "imap2.custom.net");
    press(&mut harness.app, KeyCode::Enter);

    let config = written_config(&harness);
    assert_eq!(config.accounts.len(), 1, "editing replaces, never appends");
    assert!(
        matches!(&config.accounts[0].backend, Some(Backend::Imap(imap)) if imap.host == "imap2.custom.net")
    );
    assert_eq!(
        config.accounts[0].email, "me@custom.net",
        "untouched fields survive"
    );
}

#[test]
fn zero_account_start_opens_the_form() {
    let mut harness = harness();
    enter_on_first_run(harness.app.world_mut());
    assert_eq!(
        harness.app.world().resource::<ActiveForm>().title(),
        Some("new account")
    );
}
