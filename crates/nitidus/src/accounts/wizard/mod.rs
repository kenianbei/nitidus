//! `:new-account` — a stepped overlay form that builds an account,
//! writes it to `config.toml`, stores its secret or chains into the
//! OAuth grant, and registers it live. Known providers fill hosts and
//! folder names from presets; Custom IMAP grows a Servers step.
//!
//! `:edit-account` opens the same form in edit mode, prefilled, with
//! every step reachable at once — changing one host does not mean
//! walking the whole flow again.

mod fields;
mod presets;

use bevy::prelude::*;
use presets::{Draft, apply_gmail, apply_outlook};

use crate::config::account::{
    Auth, Backend, ImapBackend, Oauth2Auth, Oauth2Flow, Oauth2Provider, Outgoing, PasswordCmdAuth,
    SmtpOutgoing,
};
use crate::config::{Config, keyring};
use crate::index::IndexView;
use crate::overlay::form::{FormSpec, FormValues, open_form};
use crate::status::MessageLog;

use fields::{Prefill, resolved_oauth_provider};

/// `:new-account` — also entered automatically on a zero-account start.
pub fn start(world: &mut World) {
    open_account_form(world, Prefill::default(), None);
}

/// `:edit-account` — the same form, prefilled and freely navigable.
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
    open_account_form(
        world,
        Prefill::from_account(&account),
        Some(name.to_owned()),
    );
}

fn open_account_form(world: &mut World, prefill: Prefill, editing: Option<String>) {
    let title = match &editing {
        Some(name) => format!("edit account — {name}"),
        None => "new account".to_owned(),
    };
    let primary = if editing.is_some() { "Save" } else { "Create" };
    let existing = editing.clone();
    let spec = FormSpec::paged(
        title,
        primary,
        fields::pages(prefill),
        Box::new(move |world, values| finalize(world, &values, existing)),
    );
    let spec = if editing.is_some() {
        spec.editing()
    } else {
        spec
    };
    open_form(world, spec);
}

fn finalize(world: &mut World, values: &FormValues, editing: Option<String>) {
    let name = values.get(fields::NAME);
    let taken = world
        .resource::<Config>()
        .accounts
        .iter()
        .any(|account| account.name == name && editing.as_deref() != Some(name));
    if taken {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world
            .resource_mut::<MessageLog>()
            .warn(format!("account name {name} is already used"), now);
        return;
    }
    let draft = build_draft(values, editing);
    write_and_register(world, draft);
}

fn build_draft(values: &FormValues, editing: Option<String>) -> Draft {
    let mut draft = Draft {
        editing,
        ..Default::default()
    };
    draft.account.name = values.get(fields::NAME).to_owned();
    draft.account.email = values.get(fields::EMAIL).to_owned();
    draft.account.display_name = values.get(fields::DISPLAY_NAME).to_owned();
    apply_provider(&mut draft, values);
    apply_auth(&mut draft, values);
    draft
}

fn apply_provider(draft: &mut Draft, values: &FormValues) {
    match values.get(fields::PROVIDER) {
        fields::GMAIL => apply_gmail(draft),
        fields::OUTLOOK => apply_outlook(draft),
        _ => apply_custom(draft, values),
    }
}

fn apply_custom(draft: &mut Draft, values: &FormValues) {
    draft.account.backend = Some(Backend::Imap(ImapBackend {
        host: values.get(fields::IMAP_HOST).to_owned(),
        ..Default::default()
    }));
    draft.account.outgoing = Some(Outgoing::Smtp(SmtpOutgoing {
        host: values.get(fields::SMTP_HOST).to_owned(),
        ..Default::default()
    }));
    let folders = &mut draft.account.folders;
    folders.drafts = values.get(fields::DRAFTS).to_owned();
    folders.sent = values.get(fields::SENT).to_owned();
    folders.trash = values.get(fields::TRASH).to_owned();
    folders.archive = values.get(fields::ARCHIVE).to_owned();
}

fn apply_auth(draft: &mut Draft, values: &FormValues) {
    draft.account.auth = match values.get(fields::AUTH) {
        fields::OAUTH2 => oauth_auth(values),
        fields::PASSWORD_COMMAND => Auth::PasswordCmd(PasswordCmdAuth {
            command: values.get(fields::PASSWORD_CMD).to_owned(),
        }),
        _ => Auth::Keyring,
    };
}

/// Microsoft's Outlook registration only permits the browser code flow,
/// so that pairing pins it rather than falling back to the device grant.
fn oauth_auth(values: &FormValues) -> Auth {
    let provider = if resolved_oauth_provider(values) == fields::MICROSOFT {
        Oauth2Provider::Microsoft
    } else {
        Oauth2Provider::Google
    };
    let flow = (provider == Oauth2Provider::Microsoft
        && values.get(fields::PROVIDER) == fields::OUTLOOK)
        .then_some(Oauth2Flow::Code);
    let secret = values.get(fields::CLIENT_SECRET);
    Auth::Oauth2(Oauth2Auth {
        provider,
        client_id: values.get(fields::CLIENT_ID).to_owned(),
        client_secret: (!secret.is_empty()).then(|| secret.to_owned()),
        flow,
    })
}

fn write_and_register(world: &mut World, draft: Draft) {
    let written = config_file(world).and_then(|path| match &draft.editing {
        Some(original) => crate::config::write::update_account(&path, original, &draft.account),
        None => crate::config::write::append_account(&path, &draft.account),
    });
    let now = world.resource::<Time>().elapsed_secs_f64();
    if let Err(error) = written {
        world
            .resource_mut::<MessageLog>()
            .warn(format!("account wizard: {error:#}"), now);
        return;
    }
    let name = draft.account.name.clone();
    let needs_grant = matches!(&draft.account.auth, Auth::Oauth2(_))
        && keyring::load_oauth_refresh(&name).is_err();
    let needs_password =
        draft.account.auth == Auth::Keyring && keyring::load_password(&name).is_err();
    let verb = replace_existing(world, &draft);
    world.resource_mut::<Config>().accounts.push(draft.account);
    switch_active(world, &name);
    world
        .resource_mut::<MessageLog>()
        .info(format!("account {name} {verb}"), now);
    if needs_password {
        super::set_password(world);
    } else if needs_grant {
        super::oauth::authorize(world);
    } else {
        super::register_live(world, &name);
    }
}

fn replace_existing(world: &mut World, draft: &Draft) -> &'static str {
    let Some(original) = &draft.editing else {
        return "added";
    };
    super::manage::detach_runtime(world, original);
    world
        .resource_mut::<Config>()
        .accounts
        .retain(|account| &account.name != original);
    "updated"
}

fn switch_active(world: &mut World, name: &str) {
    let mut view = world.resource_mut::<IndexView>();
    view.account = Some(nitidus_mail::AccountId::new(name));
    view.folder = nitidus_mail::FolderId::new("INBOX");
    view.selected = None;
}

pub fn enter_on_first_run(world: &mut World) {
    let has_accounts = world
        .get_resource::<Config>()
        .is_none_or(|config| !config.accounts.is_empty());
    if !has_accounts {
        let now = world.resource::<Time>().elapsed_secs_f64();
        world.resource_mut::<MessageLog>().info(
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

#[cfg(test)]
#[path = "wizard_tests.rs"]
mod wizard_tests;
