//! `:new-account` — a guided prompt/picker chain that builds an
//! account, writes it to `config.toml`, stores its secret or chains
//! into the OAuth grant, and registers it live. Known providers fill
//! hosts and folder names from presets; Custom IMAP prompts for
//! everything.

mod presets;

use bevy::prelude::*;
use presets::{Draft, Provider, THUNDERBIRD_CLIENT_ID, apply_gmail, apply_outlook};

use crate::config::account::{
    AccountConfig, Auth, Backend, ImapBackend, Oauth2Auth, Oauth2Flow, Oauth2Provider, Outgoing,
    PasswordCmdAuth, SmtpOutgoing,
};
use crate::config::{Config, keyring};
use crate::index::IndexView;
use crate::overlay::{PickerItem, PickerSpec, open_picker};
use crate::prompt::{PromptRequest, open_prompt};
use crate::screen::Screen;
use crate::status::StatusMessage;

/// `:new-account` — also entered automatically on a zero-account start.
pub fn start(world: &mut World) {
    prompt_name(world, Draft::default());
}

/// `:edit-account` — the same chain, prefilled from the existing block.
pub(super) fn start_edit(world: &mut World, name: &str) {
    let Some(account) = world
        .resource::<Config>()
        .accounts
        .iter()
        .find(|candidate| candidate.name == name)
        .cloned()
    else {
        return;
    };
    let oauth_provider = match &account.auth {
        Auth::Oauth2(oauth) => Some(oauth.provider),
        _ => None,
    };
    prompt_name(
        world,
        Draft {
            account,
            oauth_provider,
            editing: Some(name.to_owned()),
        },
    );
}

fn prompt_name(world: &mut World, draft: Draft) {
    text_step(
        world,
        "Account name: ",
        &draft.account.name.clone(),
        move |world, draft, value| {
            let taken =
                world.resource::<Config>().accounts.iter().any(|account| {
                    account.name == value && draft.editing.as_deref() != Some(&value)
                });
            if value.is_empty() || taken {
                fail(world, "account name must be non-empty and unused");
                return prompt_name(world, draft);
            }
            let mut draft = draft;
            draft.account.name = value;
            prompt_email(world, draft);
        },
        draft.clone(),
    );
}

fn prompt_email(world: &mut World, draft: Draft) {
    text_step(
        world,
        "Email address: ",
        &draft.account.email.clone(),
        move |world, draft, value| {
            if !value.contains('@') {
                fail(world, "email must contain @");
                return prompt_email(world, draft);
            }
            let mut draft = draft;
            draft.account.email = value;
            pick_provider(world, draft);
        },
        draft.clone(),
    );
}

fn pick_provider(world: &mut World, draft: Draft) {
    let items = vec![
        item("Gmail", "imap.gmail.com — OAuth2 or app password"),
        item(
            "Outlook / Office 365",
            "outlook.office365.com — OAuth2 (code flow)",
        ),
        item("Custom IMAP", "any server — prompts for hosts and folders"),
    ];
    open_picker(
        world,
        PickerSpec {
            title: "mail provider".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                let mut draft = draft.clone();
                match picked {
                    0 => {
                        apply_gmail(&mut draft);
                        pick_auth(world, draft, Provider::Gmail);
                    }
                    1 => {
                        apply_outlook(&mut draft);
                        pick_auth(world, draft, Provider::Outlook);
                    }
                    _ => prompt_imap_host(world, draft),
                }
            }),
        },
    );
}

fn prompt_imap_host(world: &mut World, draft: Draft) {
    text_step(
        world,
        "IMAP host: ",
        "",
        move |world, draft, value| {
            if value.is_empty() {
                fail(world, "IMAP host must be non-empty");
                return prompt_imap_host(world, draft);
            }
            let mut draft = draft;
            draft.account.backend = Some(Backend::Imap(ImapBackend {
                host: value.clone(),
                ..Default::default()
            }));
            prompt_smtp_host(world, draft, value);
        },
        draft.clone(),
    );
}

fn prompt_smtp_host(world: &mut World, draft: Draft, prefill: String) {
    text_step(
        world,
        "SMTP host: ",
        &prefill.clone(),
        move |world, draft, value| {
            if value.is_empty() {
                fail(world, "SMTP host must be non-empty");
                return prompt_smtp_host(world, draft, prefill.clone());
            }
            let mut draft = draft;
            draft.account.outgoing = Some(Outgoing::Smtp(SmtpOutgoing {
                host: value,
                ..Default::default()
            }));
            prompt_folders(world, draft, 0);
        },
        draft.clone(),
    );
}

const FOLDER_STEPS: [(&str, fn(&mut AccountConfig) -> &mut String); 4] = [
    ("Drafts folder: ", |account| &mut account.folders.drafts),
    ("Sent folder: ", |account| &mut account.folders.sent),
    ("Trash folder: ", |account| &mut account.folders.trash),
    ("Archive folder: ", |account| &mut account.folders.archive),
];

fn prompt_folders(world: &mut World, draft: Draft, step: usize) {
    let Some((label, accessor)) = FOLDER_STEPS.get(step) else {
        return pick_auth(world, draft, Provider::Custom);
    };
    let prefill = accessor(&mut draft.clone().account).clone();
    text_step(
        world,
        label,
        &prefill,
        move |world, draft, value| {
            let mut draft = draft;
            if !value.is_empty() {
                *accessor(&mut draft.account) = value;
            }
            prompt_folders(world, draft, step + 1);
        },
        draft.clone(),
    );
}

fn pick_auth(world: &mut World, draft: Draft, provider: Provider) {
    let items = vec![
        item("OAuth2", "browser or device grant; :authorize runs after"),
        item(
            "Password (keyring)",
            "app password, stored in the OS keyring",
        ),
        item("Password command", "shell command that prints the password"),
    ];
    open_picker(
        world,
        PickerSpec {
            title: "authentication".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                let draft = draft.clone();
                match picked {
                    0 => match draft.oauth_provider {
                        Some(oauth_provider) => {
                            prompt_client_id(world, draft, oauth_provider, provider)
                        }
                        None => pick_oauth_provider(world, draft, provider),
                    },
                    1 => {
                        let mut draft = draft;
                        draft.account.auth = Auth::Keyring;
                        prompt_display_name(world, draft);
                    }
                    _ => prompt_password_cmd(world, draft),
                }
            }),
        },
    );
}

fn pick_oauth_provider(world: &mut World, draft: Draft, provider: Provider) {
    let items = vec![
        item("Google", "accounts.google.com endpoints"),
        item("Microsoft", "login.microsoftonline.com endpoints"),
    ];
    open_picker(
        world,
        PickerSpec {
            title: "oauth2 provider".to_owned(),
            items,
            on_select: Box::new(move |world, picked| {
                let oauth_provider = if picked == 0 {
                    Oauth2Provider::Google
                } else {
                    Oauth2Provider::Microsoft
                };
                prompt_client_id(world, draft.clone(), oauth_provider, provider);
            }),
        },
    );
}

fn prompt_client_id(
    world: &mut World,
    draft: Draft,
    oauth_provider: Oauth2Provider,
    provider: Provider,
) {
    let existing = match &draft.account.auth {
        Auth::Oauth2(oauth) if !oauth.client_id.is_empty() => oauth.client_id.clone(),
        _ => String::new(),
    };
    let prefill = if !existing.is_empty() {
        existing
    } else if oauth_provider == Oauth2Provider::Microsoft {
        THUNDERBIRD_CLIENT_ID.to_owned()
    } else {
        String::new()
    };
    text_step(
        world,
        "OAuth client id: ",
        &prefill,
        move |world, draft, value| {
            if value.is_empty() {
                fail(
                    world,
                    "client id must be non-empty (see design/feature-oauth2-v1.md §5)",
                );
                return prompt_client_id(world, draft, oauth_provider, provider);
            }
            let mut draft = draft;
            let flow = (oauth_provider == Oauth2Provider::Microsoft
                && provider == Provider::Outlook)
                .then_some(Oauth2Flow::Code);
            draft.account.auth = Auth::Oauth2(Oauth2Auth {
                provider: oauth_provider,
                client_id: value,
                client_secret: None,
                flow,
            });
            prompt_client_secret(world, draft);
        },
        draft.clone(),
    );
}

fn prompt_client_secret(world: &mut World, draft: Draft) {
    text_step(
        world,
        "OAuth client secret (Enter for none): ",
        "",
        move |world, draft, value| {
            let mut draft = draft;
            if !value.is_empty()
                && let Auth::Oauth2(oauth) = &mut draft.account.auth
            {
                oauth.client_secret = Some(value);
            }
            prompt_display_name(world, draft);
        },
        draft.clone(),
    );
}

fn prompt_password_cmd(world: &mut World, draft: Draft) {
    text_step(
        world,
        "Password command: ",
        "",
        move |world, draft, value| {
            if value.is_empty() {
                fail(world, "password command must be non-empty");
                return prompt_password_cmd(world, draft);
            }
            let mut draft = draft;
            draft.account.auth = Auth::PasswordCmd(PasswordCmdAuth { command: value });
            prompt_display_name(world, draft);
        },
        draft.clone(),
    );
}

fn prompt_display_name(world: &mut World, draft: Draft) {
    text_step(
        world,
        "Display name (Enter to skip): ",
        "",
        move |world, draft, value| {
            let mut draft = draft;
            draft.account.display_name = value;
            finalize(world, draft);
        },
        draft.clone(),
    );
}

fn finalize(world: &mut World, draft: Draft) {
    let written = config_file(world).and_then(|path| match &draft.editing {
        Some(original) => crate::config::write::update_account(&path, original, &draft.account),
        None => crate::config::write::append_account(&path, &draft.account),
    });
    let now = world.resource::<Time>().elapsed_secs_f64();
    if let Err(error) = written {
        world
            .resource_mut::<StatusMessage>()
            .warn(format!("account wizard: {error:#}"), now);
        return;
    }
    let name = draft.account.name.clone();
    let needs_grant = matches!(&draft.account.auth, Auth::Oauth2(_))
        && keyring::load_oauth_refresh(&name).is_err();
    let needs_password =
        draft.account.auth == Auth::Keyring && keyring::load_password(&name).is_err();
    let verb = if let Some(original) = &draft.editing {
        super::manage::detach_runtime(world, original);
        world
            .resource_mut::<Config>()
            .accounts
            .retain(|account| &account.name != original);
        "updated"
    } else {
        "added"
    };
    world.resource_mut::<Config>().accounts.push(draft.account);
    switch_active(world, &name);
    world
        .resource_mut::<StatusMessage>()
        .info(format!("account {name} {verb}"), now);
    if needs_password {
        super::set_password(world);
    } else if needs_grant {
        super::oauth::authorize(world);
    } else {
        super::register_live(world, &name);
    }
}

fn switch_active(world: &mut World, name: &str) {
    let mut view = world.resource_mut::<IndexView>();
    view.account = Some(nitidus_mail::AccountId::new(name));
    view.folder = nitidus_mail::FolderId::new("INBOX");
    view.selected = None;
    *world.resource_mut::<Screen>() = Screen::Index;
}

/// A zero-account start lands straight in the wizard.
pub fn enter_on_first_run(world: &mut World) {
    let has_accounts = world
        .get_resource::<Config>()
        .is_none_or(|config| !config.accounts.is_empty());
    if !has_accounts {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world.resource_mut::<StatusMessage>().info(
            "no accounts configured — let's add one (Esc cancels)".to_owned(),
            now,
        );
        start(world);
    }
}

pub(crate) fn config_file(world: &World) -> anyhow::Result<std::path::PathBuf> {
    if let Some(path) = world.get_resource::<super::ConfigFilePath>() {
        return Ok(path.0.clone());
    }
    Ok(crate::dirs::config_dir()?.join(crate::config::CONFIG_FILE_NAME))
}

fn item(label: &str, detail: &str) -> PickerItem {
    PickerItem {
        label: label.to_owned(),
        detail: Some(detail.to_owned()),
    }
}

fn fail(world: &mut World, message: &str) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    world
        .resource_mut::<StatusMessage>()
        .warn(message.to_owned(), now);
}

/// One text prompt in the chain: `apply` receives the trimmed value
/// and the draft, and decides what opens next.
fn text_step(
    world: &mut World,
    label: &str,
    prefill: &str,
    apply: impl FnOnce(&mut World, Draft, String) + Send + Sync + 'static,
    draft: Draft,
) {
    let request = PromptRequest::new(
        label,
        Box::new(move |world, value| apply(world, draft, value.trim().to_owned())),
    )
    .with_initial(prefill);
    open_prompt(world, request);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::{KeyCode, KeyEvent};

    use super::*;
    use crate::action::Motion;
    use crate::config::keyring::use_mock_keyring;
    use crate::keymap::Mode;
    use crate::overlay::ActiveOverlay;
    use crate::prompt::PromptState;

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
        app.init_resource::<PromptState>();
        app.init_resource::<StatusMessage>();
        app.init_resource::<ActiveOverlay>();
        app.init_resource::<IndexView>();
        app.init_resource::<Screen>();
        app.init_resource::<Config>();
        app.insert_resource(super::super::ConfigFilePath(config_path.clone()));
        Harness {
            app,
            config_path,
            _dir: dir,
        }
    }

    fn type_submit(app: &mut App, text: &str) {
        for character in text.chars() {
            crate::prompt::handle_key(app.world_mut(), KeyEvent::from(KeyCode::Char(character)))
                .unwrap();
        }
        crate::prompt::handle_key(app.world_mut(), KeyEvent::from(KeyCode::Enter)).unwrap();
    }

    fn pick(app: &mut App, index: usize) {
        for _ in 0..index {
            crate::overlay::move_selection(app.world_mut(), Motion::Next);
        }
        crate::overlay::confirm(app.world_mut());
    }

    fn prompt_label(app: &App) -> String {
        app.world()
            .resource::<PromptState>()
            .label()
            .unwrap_or_default()
            .to_owned()
    }

    fn written_config(harness: &Harness) -> Config {
        toml::from_str(&std::fs::read_to_string(&harness.config_path).unwrap()).unwrap()
    }

    #[test]
    fn gmail_password_path_writes_presets_and_chains_the_password_prompt() {
        let mut harness = harness();
        start(harness.app.world_mut());
        type_submit(&mut harness.app, "wiz-gmail");
        type_submit(&mut harness.app, "wiz@gmail.com");
        assert_eq!(
            harness.app.world().resource::<ActiveOverlay>().title(),
            Some("mail provider")
        );
        pick(&mut harness.app, 0);
        pick(&mut harness.app, 1); // password (keyring)
        type_submit(&mut harness.app, "Wiz Ard"); // display name

        let config = written_config(&harness);
        let account = &config.accounts[0];
        assert_eq!(account.name, "wiz-gmail");
        assert_eq!(account.display_name, "Wiz Ard");
        assert!(
            matches!(&account.backend, Some(Backend::Imap(imap)) if imap.host == "imap.gmail.com")
        );
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
            prompt_label(&harness.app),
            "Password for wiz-gmail: ",
            "a keyring account without a secret chains into set-password"
        );
        type_submit(&mut harness.app, "app-pass");
        assert!(keyring::load_password("wiz-gmail").is_ok());
    }

    #[test]
    fn custom_path_prompts_hosts_and_folders_and_password_command() {
        let mut harness = harness();
        start(harness.app.world_mut());
        type_submit(&mut harness.app, "wiz-custom");
        type_submit(&mut harness.app, "me@custom.net");
        pick(&mut harness.app, 2); // custom imap
        type_submit(&mut harness.app, "mail.custom.net");
        assert_eq!(prompt_label(&harness.app), "SMTP host: ");
        type_submit(&mut harness.app, ""); // keep the prefilled imap host
        type_submit(&mut harness.app, ""); // drafts default
        // Replace the prefilled "Sent": clear it, then type the override.
        for _ in 0.."Sent".len() {
            crate::prompt::handle_key(harness.app.world_mut(), KeyEvent::from(KeyCode::Backspace))
                .unwrap();
        }
        type_submit(&mut harness.app, "Outbox");
        type_submit(&mut harness.app, ""); // trash default
        type_submit(&mut harness.app, ""); // archive default
        pick(&mut harness.app, 2); // password command
        type_submit(&mut harness.app, "pass show custom");
        type_submit(&mut harness.app, ""); // display name skip

        let config = written_config(&harness);
        let account = &config.accounts[0];
        assert!(
            matches!(&account.outgoing, Some(Outgoing::Smtp(smtp)) if smtp.host == "mail.custom.net")
        );
        assert_eq!(account.folders.sent, "Outbox");
        assert_eq!(account.folders.drafts, "Drafts");
        assert!(
            matches!(&account.auth, Auth::PasswordCmd(cmd) if cmd.command == "pass show custom")
        );
    }

    #[test]
    fn duplicate_name_reprompts_instead_of_advancing() {
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
        type_submit(&mut harness.app, "taken");
        assert_eq!(prompt_label(&harness.app), "Account name: ");
    }

    #[test]
    fn zero_account_start_enters_the_wizard() {
        let mut harness = harness();
        enter_on_first_run(harness.app.world_mut());
        assert_eq!(prompt_label(&harness.app), "Account name: ");
    }
}
