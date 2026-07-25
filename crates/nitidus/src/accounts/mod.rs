//! Account-level operations: keyring secret management and OAuth2
//! grants for the active account.

pub mod manage;
pub mod oauth;
pub mod wizard;

use bevy::prelude::*;
use nitidus_mail::AccountId;

use crate::config::keyring;
use crate::engine::EngineResource;
use crate::index::IndexView;
use crate::prompt::{PromptRequest, open_prompt};
use crate::status::StatusMessage;
use crate::store::SyncTracker;

/// Where account mutations write; tests point it at a temp file, the
/// app leaves it unset and falls back to the real config directory.
#[derive(Resource)]
pub struct ConfigFilePath(pub std::path::PathBuf);

pub struct AccountsPlugin;

impl Plugin for AccountsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<oauth::OauthChannel>();
        app.add_systems(Update, oauth::drain_oauth_events);
        app.add_systems(PostStartup, wizard::enter_on_first_run);
    }
}

/// `:set-password` — masked prompt, stored in the OS keyring.
pub fn set_password(world: &mut World) {
    let Some(account) = active_account(world) else {
        return;
    };
    let label = format!("Password for {account}: ");
    let request = PromptRequest::new(
        label,
        Box::new(move |world, secret| {
            let now = world.resource::<Time>().elapsed_secs_f64();
            let mut status = world.resource_mut::<StatusMessage>();
            if secret.is_empty() {
                status.warn("empty password not stored".to_owned(), now);
                return;
            }
            match keyring::store_password(&account, &secret) {
                Ok(()) => {
                    status.info(format!("keyring secret stored for {account}"), now);
                    register_live(world, &account);
                }
                Err(error) => status.warn(format!("set-password: {error:#}"), now),
            }
        }),
    )
    .masked();
    open_prompt(world, request);
}

/// `:delete-password` — removes the active account's keyring entry.
pub fn delete_password(world: &mut World) {
    let Some(account) = active_account(world) else {
        return;
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match keyring::delete_password(&account) {
        Ok(()) => status.info(format!("keyring secret removed for {account}"), now),
        Err(error) => status.warn(format!("delete-password: {error:#}"), now),
    }
}

/// Registers a configured-but-unconnected account on the running
/// engine — idempotent, and silent unless it acts. The wizard,
/// `:set-password`, and a landed OAuth grant all finish through this,
/// which is what retired the old "restart to connect" contract.
pub fn register_live(world: &mut World, name: &str) {
    let id = AccountId::new(name);
    let is_registered = world
        .get_resource::<EngineResource>()
        .is_none_or(|engine| engine.0.has_account(&id));
    if is_registered {
        return;
    }
    let Some(account) = world
        .resource::<crate::config::Config>()
        .accounts
        .iter()
        .find(|candidate| candidate.name == name)
        .cloned()
    else {
        return;
    };
    let Some(mut tracker) = world.remove_resource::<SyncTracker>() else {
        return;
    };
    let outcome = {
        let mut engine = world.resource_mut::<EngineResource>();
        crate::bootstrap::register_one(&mut engine.0, &mut tracker, &account)
    };
    world.insert_resource(tracker);
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match outcome {
        Ok(()) => status.info(format!("{name} connected — syncing INBOX"), now),
        Err(error) => status.warn(format!("{name}: {error:#}"), now),
    }
}

/// `:deauthorize` — removes the active account's OAuth grant.
pub fn deauthorize(world: &mut World) {
    let Some(account) = active_account(world) else {
        return;
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match keyring::delete_oauth_refresh(&account) {
        Ok(()) => status.info(format!("oauth grant removed for {account}"), now),
        Err(error) => status.warn(format!("deauthorize: {error:#}"), now),
    }
}

fn active_account(world: &mut World) -> Option<String> {
    let account = world
        .resource::<IndexView>()
        .account
        .as_ref()
        .map(|account| account.as_str().to_owned());
    if account.is_none() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<StatusMessage>()
            .warn("no active account".to_owned(), now);
    }
    account
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};
    use nitidus_mail::AccountId;

    use super::*;
    use crate::config::keyring::use_mock_keyring;
    use crate::keymap::Mode;
    use crate::prompt::{PromptState, handle_key};

    fn accounts_app(account: &str) -> App {
        use_mock_keyring();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Mode>();
        app.init_resource::<PromptState>();
        app.init_resource::<StatusMessage>();
        app.insert_resource(IndexView {
            account: Some(AccountId::new(account)),
            ..Default::default()
        });
        app
    }

    fn type_and_submit(app: &mut App, text: &str) {
        for character in text.chars() {
            handle_key(app.world_mut(), KeyEvent::from(KeyCode::Char(character))).unwrap();
        }
        handle_key(app.world_mut(), KeyEvent::from(KeyCode::Enter)).unwrap();
    }

    #[test]
    fn set_password_prompt_stores_into_the_keyring() {
        let mut app = accounts_app("accounts-set-test");
        set_password(app.world_mut());
        assert_eq!(
            app.world().resource::<PromptState>().label(),
            Some("Password for accounts-set-test: ")
        );
        type_and_submit(&mut app, "s3cret!");
        let stored = keyring_core::Entry::new("nitidus", "accounts-set-test")
            .unwrap()
            .get_password()
            .unwrap();
        assert_eq!(stored, "s3cret!");
    }

    /// Regression: the command line used to reset the mode to Normal
    /// AFTER the action ran, clobbering the prompt that
    /// `:set-password` opens — the typed secret then leaked into
    /// normal-mode key handling.
    #[test]
    fn cmdline_set_password_lands_in_the_masked_prompt() {
        let mut app = accounts_app("accounts-cmdline-test");
        app.init_resource::<crate::cmdline::CommandLineState>();
        app.world_mut().resource_mut::<Mode>().0 = crate::keymap::InputMode::CommandLine;
        for character in "set-password".chars() {
            crate::cmdline::handle_key(app.world_mut(), KeyEvent::from(KeyCode::Char(character)))
                .unwrap();
        }
        crate::cmdline::handle_key(app.world_mut(), KeyEvent::from(KeyCode::Enter)).unwrap();
        assert_eq!(
            app.world().resource::<Mode>().0,
            crate::keymap::InputMode::Prompt,
            "the prompt mode must survive the command line closing"
        );
        type_and_submit(&mut app, "via-cmdline");
        let stored = keyring_core::Entry::new("nitidus", "accounts-cmdline-test")
            .unwrap()
            .get_password()
            .unwrap();
        assert_eq!(stored, "via-cmdline");
    }

    #[test]
    fn delete_password_removes_the_stored_entry() {
        let mut app = accounts_app("accounts-delete-test");
        keyring::store_password("accounts-delete-test", "gone-soon").unwrap();
        delete_password(app.world_mut());
        let lookup = keyring_core::Entry::new("nitidus", "accounts-delete-test")
            .unwrap()
            .get_password();
        assert!(matches!(lookup, Err(keyring_core::Error::NoEntry)));
    }

    #[test]
    fn register_live_connects_a_configured_maildir_account() {
        let maildir = tempfile::tempdir().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(maildir.path().join(sub)).unwrap();
        }
        let mut app = accounts_app("live-reg");
        let mut config = crate::config::Config::default();
        config.accounts.push(crate::config::account::AccountConfig {
            name: "live-reg".to_owned(),
            email: "live@example.com".to_owned(),
            backend: Some(crate::config::account::Backend::Maildir(
                crate::config::account::MaildirBackend {
                    path: maildir.path().to_path_buf(),
                },
            )),
            ..Default::default()
        });
        app.insert_resource(config);
        app.init_resource::<SyncTracker>();
        app.insert_resource(EngineResource(nitidus_mail::MailEngine::new(1).unwrap()));

        register_live(app.world_mut(), "live-reg");
        let engine = app.world().resource::<EngineResource>();
        assert!(engine.0.has_account(&AccountId::new("live-reg")));
        assert!(
            engine
                .0
                .send(
                    &AccountId::new("live-reg"),
                    nitidus_mail::MailCommand::ListFolders
                )
                .is_ok()
        );

        // Idempotent: a second call must not error or double-register.
        register_live(app.world_mut(), "live-reg");
        assert_eq!(
            app.world()
                .resource::<EngineResource>()
                .0
                .accounts()
                .count(),
            1
        );
    }

    #[test]
    fn deauthorize_removes_the_oauth_grant() {
        let mut app = accounts_app("accounts-deauth-test");
        keyring::store_oauth_refresh(
            "accounts-deauth-test",
            &nitidus_mail::SecretString::from("grant"),
        )
        .unwrap();
        deauthorize(app.world_mut());
        assert!(keyring::load_oauth_refresh("accounts-deauth-test").is_err());
    }

    #[test]
    fn empty_submission_stores_nothing() {
        let mut app = accounts_app("accounts-empty-test");
        set_password(app.world_mut());
        type_and_submit(&mut app, "");
        let lookup = keyring_core::Entry::new("nitidus", "accounts-empty-test")
            .unwrap()
            .get_password();
        assert!(matches!(lookup, Err(keyring_core::Error::NoEntry)));
    }
}
