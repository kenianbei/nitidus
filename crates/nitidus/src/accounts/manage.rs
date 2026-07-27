//! `:edit-account` / `:remove-account`: pick an account, then re-run
//! the wizard prefilled or tear the account down everywhere — config
//! file, `Config` resource, engine, store, sync tracker, cache.
//! Keyring secrets are kept: removal is not revocation.

use bevy::prelude::*;
use nitidus_mail::AccountId;

use super::wizard;
use crate::config::Config;
use crate::engine::{CacheResource, EngineResource};
use crate::index::IndexView;
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::status::MessageLog;
use crate::store::{MailStore, SyncTracker};

pub fn edit_account(world: &mut World) {
    pick_account(world, "edit account", |world, name| {
        wizard::start_edit(world, &name);
    });
}

pub fn remove_account(world: &mut World) {
    pick_account(world, "remove account", |world, name| {
        let detail = name.clone();
        crate::overlay::open_confirm(
            world,
            crate::overlay::ConfirmSpec::new(
                "Remove account",
                "Remove this account?",
                "Remove",
                Box::new(move |world| perform_removal(world, &name)),
            )
            .with_detail(vec![detail]),
        );
    });
}

fn pick_account(
    world: &mut World,
    title: &str,
    then: impl Fn(&mut World, String) + Send + Sync + 'static,
) {
    let names: Vec<String> = world
        .resource::<Config>()
        .accounts
        .iter()
        .map(|account| account.name.clone())
        .collect();
    if names.is_empty() {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world.resource_mut::<MessageLog>().info(
            "no accounts configured — :new-account adds one".to_owned(),
            now,
        );
        return;
    }
    let items = world
        .resource::<Config>()
        .accounts
        .iter()
        .map(|account| PickerItem {
            label: account.name.clone(),
            detail: Some(account.email.clone()),
        })
        .collect();
    open_picker(
        world,
        PickerSpec {
            title: title.to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                if let Some(name) = names.get(picked) {
                    then(world, name.clone());
                }
            }),
        },
    );
}

fn perform_removal(world: &mut World, name: &str) {
    let removed = wizard::config_file(world)
        .and_then(|path| crate::config::write::remove_account(&path, name));
    let now = world.resource::<Time>().elapsed_secs_f64();
    if let Err(error) = removed {
        world
            .resource_mut::<MessageLog>()
            .warn(format!("remove-account: {error:#}"), now);
        return;
    }
    world
        .resource_mut::<Config>()
        .accounts
        .retain(|account| account.name != name);
    detach_runtime(world, name);
    fall_back_active(world, name);
    world.resource_mut::<MessageLog>().info(
        format!("account {name} removed (its keyring secrets were kept)"),
        now,
    );
}

/// Tears the account out of every runtime structure; shared by removal
/// and by edits that rename an account.
pub(super) fn detach_runtime(world: &mut World, name: &str) {
    let id = AccountId::new(name);
    if let Some(mut engine) = world.get_resource_mut::<EngineResource>() {
        engine.0.remove_account(&id);
    }
    if let Some(mut store) = world.get_resource_mut::<MailStore>() {
        store.remove_account(&id);
    }
    if let Some(mut tracker) = world.get_resource_mut::<SyncTracker>() {
        tracker.remove_account(&id);
    }
    if let Some(cache) = world.get_resource::<CacheResource>() {
        cache.0.purge_account(&id);
    }
}

/// The active view cannot point at a removed account; fall back to the
/// first remaining one.
fn fall_back_active(world: &mut World, removed: &str) {
    let replacement = world
        .resource::<Config>()
        .accounts
        .first()
        .map(|account| AccountId::new(&account.name));
    let mut view = world.resource_mut::<IndexView>();
    if view.account.as_ref().map(AccountId::as_str) == Some(removed) {
        view.account = replacement;
        view.folder = nitidus_mail::FolderId::new("INBOX");
        view.selected = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};

    use super::*;
    use crate::config::account::AccountConfig;
    use crate::config::keyring::use_mock_keyring;
    use crate::keymap::Mode;
    use crate::overlay::ActiveOverlay;

    struct Harness {
        app: App,
        config_path: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    fn account(name: &str) -> AccountConfig {
        AccountConfig {
            name: name.to_owned(),
            email: format!("{name}@example.com"),
            ..Default::default()
        }
    }

    fn harness(names: &[&str]) -> Harness {
        use_mock_keyring();
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let mut config = Config::default();
        for name in names {
            crate::config::write::append_account(&config_path, &account(name)).unwrap();
            config.accounts.push(account(name));
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<Mode>();
        app.init_resource::<MessageLog>();
        app.init_resource::<ActiveOverlay>();
        app.init_resource::<crate::overlay::surface::OverlayStack>();
        app.init_resource::<crate::overlay::form::ActiveForm>();
        app.init_resource::<crate::overlay::confirm::ActiveConfirm>();
        app.insert_resource(
            crate::keymap::Keymaps::compile(&crate::config::RawKeymaps::default()).unwrap(),
        );
        app.insert_resource(IndexView {
            account: names.first().map(|name| AccountId::new(*name)),
            ..Default::default()
        });
        app.insert_resource(config);
        app.insert_resource(super::super::ConfigFilePath(config_path.clone()));
        Harness {
            app,
            config_path,
            _dir: dir,
        }
    }

    fn answer(app: &mut App, key: char) {
        crate::overlay::confirm::handle_key(app.world_mut(), KeyEvent::from(KeyCode::Char(key)))
            .unwrap();
    }

    fn pick(app: &mut App, index: usize) {
        for _ in 0..index {
            crate::overlay::move_selection(app.world_mut(), crate::action::Motion::Next);
        }
        crate::overlay::picker::confirm(app.world_mut());
    }

    #[test]
    fn remove_account_tears_down_config_and_falls_back_active() {
        let mut harness = harness(&["alpha", "beta"]);
        remove_account(harness.app.world_mut());
        pick(&mut harness.app, 0); // alpha — the active one
        assert!(
            harness
                .app
                .world()
                .resource::<crate::overlay::confirm::ActiveConfirm>()
                .is_open(),
            "removing an account must ask first"
        );
        answer(&mut harness.app, 'y');

        let content = std::fs::read_to_string(&harness.config_path).unwrap();
        assert!(!content.contains("alpha"), "{content}");
        assert!(content.contains("beta"), "{content}");
        let config = harness.app.world().resource::<Config>();
        assert_eq!(config.accounts.len(), 1);
        let view = harness.app.world().resource::<IndexView>();
        assert_eq!(
            view.account.as_ref().map(|id| id.as_str().to_owned()),
            Some("beta".to_owned()),
            "the active view falls back to the remaining account"
        );
    }

    #[test]
    fn declining_the_confirm_keeps_the_account() {
        let mut harness = harness(&["solo"]);
        remove_account(harness.app.world_mut());
        pick(&mut harness.app, 0);
        answer(&mut harness.app, 'n');
        assert_eq!(harness.app.world().resource::<Config>().accounts.len(), 1);
        assert!(
            std::fs::read_to_string(&harness.config_path)
                .unwrap()
                .contains("solo")
        );
    }

    fn form_key(app: &mut App, code: KeyCode) {
        crate::overlay::form::handle_key(app.world_mut(), KeyEvent::from(code)).unwrap();
    }

    fn form_type(app: &mut App, text: &str) {
        for character in text.chars() {
            form_key(app, KeyCode::Char(character));
        }
    }

    #[test]
    fn edit_account_updates_the_block_in_place() {
        let mut harness = harness(&["editable"]);
        edit_account(harness.app.world_mut());
        pick(&mut harness.app, 0);

        // The picker chooses which account; the form then opens prefilled
        // with every step reachable, so one field can be changed alone.
        let form = harness
            .app
            .world()
            .resource::<crate::overlay::form::ActiveForm>();
        assert_eq!(form.title(), Some("edit account — editable"));
        assert_eq!(form.value("name").unwrap(), "editable");

        form_key(&mut harness.app, KeyCode::Tab); // email
        form_key(&mut harness.app, KeyCode::Tab); // display name
        form_type(&mut harness.app, "Edited Name");

        crate::overlay::form::go_to_page(harness.app.world_mut(), 2);
        form_type(&mut harness.app, "mail.new-host.net");
        form_key(&mut harness.app, KeyCode::Tab);
        form_type(&mut harness.app, "smtp.new-host.net");
        form_key(&mut harness.app, KeyCode::Enter);
        assert_eq!(
            harness
                .app
                .world()
                .resource::<crate::overlay::form::ActiveForm>()
                .title(),
            Some("password — editable"),
            "a keyring account with no stored secret chains into set-password"
        );

        let content = std::fs::read_to_string(&harness.config_path).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.accounts.len(), 1, "{content}");
        assert_eq!(config.accounts[0].display_name, "Edited Name");
        assert!(
            matches!(&config.accounts[0].backend, Some(crate::config::account::Backend::Imap(imap)) if imap.host == "mail.new-host.net")
        );
        let resource = harness.app.world().resource::<Config>();
        assert_eq!(resource.accounts.len(), 1);
        assert_eq!(resource.accounts[0].display_name, "Edited Name");
    }
}
