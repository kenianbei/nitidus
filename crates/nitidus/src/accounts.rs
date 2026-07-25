//! Account-level operations: keyring secret management for the active
//! account.

use bevy::prelude::*;

use crate::config::secrets;
use crate::index::IndexView;
use crate::prompt::{PromptRequest, open_prompt};
use crate::status::StatusMessage;

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
            match secrets::store_password(&account, &secret) {
                Ok(()) => status.info(format!("keyring secret stored for {account}"), now),
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
    match secrets::delete_password(&account) {
        Ok(()) => status.info(format!("keyring secret removed for {account}"), now),
        Err(error) => status.warn(format!("delete-password: {error:#}"), now),
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
    use crate::config::secrets::use_mock_keyring;
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
        secrets::store_password("accounts-delete-test", "gone-soon").unwrap();
        delete_password(app.world_mut());
        let lookup = keyring_core::Entry::new("nitidus", "accounts-delete-test")
            .unwrap()
            .get_password();
        assert!(matches!(lookup, Err(keyring_core::Error::NoEntry)));
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
