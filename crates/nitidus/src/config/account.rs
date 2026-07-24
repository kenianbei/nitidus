//! Account definitions. These carry references to credentials (commands,
//! keyring entries, OAuth providers) — never secret material itself.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_IMAP_PORT: u16 = 993;
const DEFAULT_SMTP_PORT: u16 = 587;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountConfig {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub backend: Option<Backend>,
    pub outgoing: Option<Outgoing>,
    pub auth: Auth,
    pub folders: Folders,
    pub signature: Option<String>,
    pub signature_file: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Maildir(MaildirBackend),
    Imap(ImapBackend),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaildirBackend {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImapBackend {
    pub host: String,
    pub port: u16,
    pub encryption: Encryption,
}

impl Default for ImapBackend {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: DEFAULT_IMAP_PORT,
            encryption: Encryption::Tls,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outgoing {
    Smtp(SmtpOutgoing),
    Sendmail(SendmailOutgoing),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SmtpOutgoing {
    pub host: String,
    pub port: u16,
    pub encryption: Encryption,
}

impl Default for SmtpOutgoing {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: DEFAULT_SMTP_PORT,
            encryption: Encryption::Starttls,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SendmailOutgoing {
    pub command: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Encryption {
    #[default]
    Tls,
    Starttls,
    None,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    #[default]
    Keyring,
    PasswordCmd(PasswordCmdAuth),
    Oauth2(Oauth2Auth),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordCmdAuth {
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Oauth2Auth {
    pub provider: Oauth2Provider,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Oauth2Provider {
    Google,
    Microsoft,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Folders {
    pub drafts: String,
    pub sent: String,
    pub trash: String,
    pub archive: String,
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            drafts: "Drafts".to_owned(),
            sent: "Sent".to_owned(),
            trash: "Trash".to_owned(),
            archive: "Archive".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_full_imap_account() {
        let account: AccountConfig = toml::from_str(
            r#"
            name = "work"
            email = "norman@example.com"
            display_name = "Norman"
            aliases = ["n@example.com"]
            backend = { imap = { host = "imap.example.com" } }
            outgoing = { smtp = { host = "smtp.example.com", port = 465, encryption = "tls" } }
            auth = { oauth2 = { provider = "google" } }
            [folders]
            archive = "All Mail"
            "#,
        )
        .unwrap();
        assert_eq!(
            account.backend,
            Some(Backend::Imap(ImapBackend {
                host: "imap.example.com".to_owned(),
                port: 993,
                encryption: Encryption::Tls,
            }))
        );
        assert_eq!(account.folders.archive, "All Mail");
        assert_eq!(account.folders.drafts, "Drafts");
        assert_eq!(
            account.auth,
            Auth::Oauth2(Oauth2Auth {
                provider: Oauth2Provider::Google
            })
        );
    }

    #[test]
    fn parses_maildir_sendmail_password_cmd_account() {
        let account: AccountConfig = toml::from_str(
            r#"
            name = "local"
            email = "moz@localhost"
            backend = { maildir = { path = "~/Mail" } }
            outgoing = { sendmail = { command = "/usr/bin/msmtp" } }
            auth = { password_cmd = { command = "pass show mail" } }
            "#,
        )
        .unwrap();
        assert!(matches!(account.backend, Some(Backend::Maildir(_))));
        assert!(matches!(account.outgoing, Some(Outgoing::Sendmail(_))));
        assert!(matches!(account.auth, Auth::PasswordCmd(_)));
    }

    #[test]
    fn rejects_unknown_account_field() {
        let result = toml::from_str::<AccountConfig>("name = \"a\"\npasword = \"typo\"\n");
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("pasword"),
            "error should name the typo: {message}"
        );
    }

    #[test]
    fn auth_defaults_to_keyring() {
        let account: AccountConfig = toml::from_str("name = \"a\"").unwrap();
        assert_eq!(account.auth, Auth::Keyring);
    }
}
