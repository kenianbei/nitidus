//! The wizard's shape. Pages are derived from what has been filled in
//! so far, so choosing a provider or an auth method adds and removes
//! steps as data rather than as control flow.

use crate::config::account::{AccountConfig, Auth, Backend, Oauth2Provider, Outgoing};
use crate::overlay::form::{FieldSpec, FormValues, PageSpec, PagesFn, SelectOption};

use super::presets::THUNDERBIRD_CLIENT_ID;

pub(super) const NAME: &str = "name";
pub(super) const EMAIL: &str = "email";
pub(super) const DISPLAY_NAME: &str = "display_name";
pub(super) const PROVIDER: &str = "provider";
pub(super) const AUTH: &str = "auth";
pub(super) const IMAP_HOST: &str = "imap_host";
pub(super) const SMTP_HOST: &str = "smtp_host";
pub(super) const DRAFTS: &str = "drafts";
pub(super) const SENT: &str = "sent";
pub(super) const TRASH: &str = "trash";
pub(super) const ARCHIVE: &str = "archive";
pub(super) const OAUTH_PROVIDER: &str = "oauth_provider";
pub(super) const CLIENT_ID: &str = "client_id";
pub(super) const CLIENT_SECRET: &str = "client_secret";
pub(super) const PASSWORD_CMD: &str = "password_cmd";

pub(super) const GMAIL: &str = "gmail";
pub(super) const OUTLOOK: &str = "outlook";
pub(super) const CUSTOM: &str = "custom";

pub(super) const OAUTH2: &str = "oauth2";
pub(super) const KEYRING: &str = "keyring";
pub(super) const PASSWORD_COMMAND: &str = "password_cmd";

pub(super) const GOOGLE: &str = "google";
pub(super) const MICROSOFT: &str = "microsoft";

/// Everything the form starts from: empty for `:new-account`, drawn
/// from the existing block for `:edit-account`.
#[derive(Clone, Debug, Default)]
pub(super) struct Prefill {
    pub(super) name: String,
    pub(super) email: String,
    pub(super) display_name: String,
    pub(super) provider: String,
    pub(super) auth: String,
    pub(super) imap_host: String,
    pub(super) smtp_host: String,
    pub(super) drafts: String,
    pub(super) sent: String,
    pub(super) trash: String,
    pub(super) archive: String,
    pub(super) oauth_provider: String,
    pub(super) client_id: String,
    pub(super) password_cmd: String,
}

impl Prefill {
    pub(super) fn from_account(account: &AccountConfig) -> Self {
        let imap_host = match &account.backend {
            Some(Backend::Imap(imap)) => imap.host.clone(),
            _ => String::new(),
        };
        let smtp_host = match &account.outgoing {
            Some(Outgoing::Smtp(smtp)) => smtp.host.clone(),
            _ => String::new(),
        };
        Self {
            name: account.name.clone(),
            email: account.email.clone(),
            display_name: account.display_name.clone(),
            provider: provider_of(&imap_host),
            auth: auth_of(&account.auth),
            imap_host,
            smtp_host,
            drafts: account.folders.drafts.clone(),
            sent: account.folders.sent.clone(),
            trash: account.folders.trash.clone(),
            archive: account.folders.archive.clone(),
            oauth_provider: oauth_provider_of(&account.auth),
            client_id: client_id_of(&account.auth),
            password_cmd: password_cmd_of(&account.auth),
        }
    }
}

fn provider_of(imap_host: &str) -> String {
    match imap_host {
        "imap.gmail.com" => GMAIL,
        "outlook.office365.com" => OUTLOOK,
        _ => CUSTOM,
    }
    .to_owned()
}

fn auth_of(auth: &Auth) -> String {
    match auth {
        Auth::Oauth2(_) => OAUTH2,
        Auth::PasswordCmd(_) => PASSWORD_COMMAND,
        _ => KEYRING,
    }
    .to_owned()
}

fn oauth_provider_of(auth: &Auth) -> String {
    match auth {
        Auth::Oauth2(oauth) if oauth.provider == Oauth2Provider::Microsoft => MICROSOFT.to_owned(),
        Auth::Oauth2(_) => GOOGLE.to_owned(),
        _ => String::new(),
    }
}

fn client_id_of(auth: &Auth) -> String {
    match auth {
        Auth::Oauth2(oauth) => oauth.client_id.clone(),
        _ => String::new(),
    }
}

fn password_cmd_of(auth: &Auth) -> String {
    match auth {
        Auth::PasswordCmd(cmd) => cmd.command.clone(),
        _ => String::new(),
    }
}

pub(super) fn pages(prefill: Prefill) -> PagesFn {
    Box::new(move |values| {
        let mut pages = vec![account_page(&prefill), provider_page(&prefill)];
        if values.get(PROVIDER) == CUSTOM {
            pages.push(servers_page(&prefill));
        }
        if let Some(page) = credentials_page(&prefill, values) {
            pages.push(page);
        }
        pages
    })
}

fn account_page(prefill: &Prefill) -> PageSpec {
    PageSpec::new(
        "account",
        "Account",
        vec![
            FieldSpec::text(NAME, "Account name")
                .with_initial(prefill.name.clone())
                .validated(|value| require(value, "account name must not be empty")),
            FieldSpec::text(EMAIL, "Email address")
                .with_initial(prefill.email.clone())
                .validated(|value| {
                    if value.contains('@') {
                        Ok(())
                    } else {
                        Err("email must contain @".to_owned())
                    }
                }),
            FieldSpec::text(DISPLAY_NAME, "Display name")
                .with_initial(prefill.display_name.clone()),
        ],
    )
}

fn provider_page(prefill: &Prefill) -> PageSpec {
    PageSpec::new(
        "provider",
        "Provider",
        vec![
            FieldSpec::select(
                PROVIDER,
                "Mail provider",
                vec![
                    SelectOption::new(GMAIL, "Gmail")
                        .with_detail("imap.gmail.com — OAuth2 or app password"),
                    SelectOption::new(OUTLOOK, "Outlook / Office 365")
                        .with_detail("outlook.office365.com — OAuth2 (code flow)"),
                    SelectOption::new(CUSTOM, "Custom IMAP")
                        .with_detail("any server — asks for hosts and folders"),
                ],
            )
            .with_initial(prefill.provider.clone()),
            FieldSpec::select(
                AUTH,
                "Authentication",
                vec![
                    SelectOption::new(OAUTH2, "OAuth2")
                        .with_detail("browser or device grant; :authorize runs after"),
                    SelectOption::new(KEYRING, "Password (keyring)")
                        .with_detail("app password, stored in the OS keyring"),
                    SelectOption::new(PASSWORD_COMMAND, "Password command")
                        .with_detail("shell command that prints the password"),
                ],
            )
            .with_initial(prefill.auth.clone()),
        ],
    )
}

fn servers_page(prefill: &Prefill) -> PageSpec {
    let folders = crate::config::account::Folders::default();
    PageSpec::new(
        "servers",
        "Servers",
        vec![
            FieldSpec::text(IMAP_HOST, "IMAP host")
                .with_initial(prefill.imap_host.clone())
                .validated(|value| require(value, "IMAP host must not be empty")),
            FieldSpec::text(SMTP_HOST, "SMTP host")
                .with_initial(prefill.smtp_host.clone())
                .validated(|value| require(value, "SMTP host must not be empty")),
            folder_field(DRAFTS, "Drafts folder", &prefill.drafts, folders.drafts),
            folder_field(SENT, "Sent folder", &prefill.sent, folders.sent),
            folder_field(TRASH, "Trash folder", &prefill.trash, folders.trash),
            folder_field(ARCHIVE, "Archive folder", &prefill.archive, folders.archive),
        ],
    )
}

fn folder_field(
    id: &'static str,
    label: &'static str,
    prefill: &str,
    fallback: String,
) -> FieldSpec {
    let initial = if prefill.is_empty() {
        fallback
    } else {
        prefill.to_owned()
    };
    FieldSpec::text(id, label).with_initial(initial)
}

/// Keyring auth needs nothing more, so its page does not exist at all —
/// the strip shows one fewer step rather than an empty one.
fn credentials_page(prefill: &Prefill, values: &FormValues) -> Option<PageSpec> {
    match values.get(AUTH) {
        OAUTH2 => Some(oauth_page(prefill, values)),
        PASSWORD_COMMAND => Some(PageSpec::new(
            "credentials",
            "Credentials",
            vec![
                FieldSpec::text(PASSWORD_CMD, "Password command")
                    .with_initial(prefill.password_cmd.clone())
                    .validated(|value| require(value, "password command must not be empty")),
            ],
        )),
        _ => None,
    }
}

/// A known provider implies its OAuth endpoints, so only Custom IMAP is
/// asked which ones to use.
fn oauth_page(prefill: &Prefill, values: &FormValues) -> PageSpec {
    let mut fields = Vec::new();
    if values.get(PROVIDER) == CUSTOM {
        fields.push(
            FieldSpec::select(
                OAUTH_PROVIDER,
                "OAuth provider",
                vec![
                    SelectOption::new(GOOGLE, "Google")
                        .with_detail("accounts.google.com endpoints"),
                    SelectOption::new(MICROSOFT, "Microsoft")
                        .with_detail("login.microsoftonline.com endpoints"),
                ],
            )
            .with_initial(prefill.oauth_provider.clone()),
        );
    }
    fields.push(
        FieldSpec::text(CLIENT_ID, "OAuth client id")
            .with_initial(client_id_initial(prefill, values))
            .validated(|value| {
                require(
                    value,
                    "client id must not be empty (see design/feature-oauth2-v1.md §5)",
                )
            }),
    );
    fields.push(FieldSpec::text(CLIENT_SECRET, "Client secret").masked());
    PageSpec::new("credentials", "Credentials", fields)
}

/// Microsoft tenants get Thunderbird's public client id, which is the
/// registration their consent screens already know.
fn client_id_initial(prefill: &Prefill, values: &FormValues) -> String {
    if !prefill.client_id.is_empty() {
        return prefill.client_id.clone();
    }
    if resolved_oauth_provider(values) == MICROSOFT {
        return THUNDERBIRD_CLIENT_ID.to_owned();
    }
    String::new()
}

pub(super) fn resolved_oauth_provider(values: &FormValues) -> &'static str {
    match values.get(PROVIDER) {
        GMAIL => GOOGLE,
        OUTLOOK => MICROSOFT,
        _ => {
            if values.get(OAUTH_PROVIDER) == MICROSOFT {
                MICROSOFT
            } else {
                GOOGLE
            }
        }
    }
}

fn require(value: &str, message: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(message.to_owned())
    } else {
        Ok(())
    }
}
