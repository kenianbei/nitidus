//! Keyring storage under one service: a password entry per account,
//! and an OAuth refresh-token entry at `<account>/oauth-refresh` so
//! `:delete-password` and `:deauthorize` stay independent.

use anyhow::{Context, bail};
use nitidus_mail::{ExposeSecret, SecretString};

/// One password entry per account: Gmail-style app passwords cover
/// both IMAP and SMTP, and the config has a single `auth` per account.
const KEYRING_SERVICE: &str = "nitidus";

const OAUTH_REFRESH_SUFFIX: &str = "/oauth-refresh";

pub fn load_password(account_name: &str) -> anyhow::Result<SecretString> {
    match entry(account_name)?.get_password() {
        Ok(secret) => Ok(SecretString::from(secret)),
        Err(keyring_core::Error::NoEntry) => {
            bail!("no keyring secret for {account_name} — :set-password stores one")
        }
        Err(error) => bail!(
            "keyring for {account_name}: {error} — :set-password stores one, \
             or switch auth to password_file/password_cmd"
        ),
    }
}

/// `:set-password` — writes the account's keyring entry.
pub fn store_password(account_name: &str, secret: &str) -> anyhow::Result<()> {
    entry(account_name)?
        .set_password(secret)
        .with_context(|| format!("storing keyring secret for {account_name}"))
}

/// `:delete-password` — removes the account's keyring entry.
pub fn delete_password(account_name: &str) -> anyhow::Result<()> {
    match entry(account_name)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring_core::Error::NoEntry) => {
            bail!("no keyring secret stored for {account_name}")
        }
        Err(error) => {
            Err(error).with_context(|| format!("deleting keyring secret for {account_name}"))
        }
    }
}

pub fn load_oauth_refresh(account_name: &str) -> anyhow::Result<SecretString> {
    let user = format!("{account_name}{OAUTH_REFRESH_SUFFIX}");
    match entry(&user)?.get_password() {
        Ok(secret) => Ok(SecretString::from(secret)),
        Err(keyring_core::Error::NoEntry) => {
            bail!("no oauth grant for {account_name} — :authorize connects it")
        }
        Err(error) => bail!("keyring for {account_name}: {error}"),
    }
}

pub fn store_oauth_refresh(account_name: &str, token: &SecretString) -> anyhow::Result<()> {
    let user = format!("{account_name}{OAUTH_REFRESH_SUFFIX}");
    entry(&user)?
        .set_password(token.expose_secret())
        .with_context(|| format!("storing oauth refresh token for {account_name}"))
}

/// `:deauthorize` — removes the account's refresh-token entry.
pub fn delete_oauth_refresh(account_name: &str) -> anyhow::Result<()> {
    let user = format!("{account_name}{OAUTH_REFRESH_SUFFIX}");
    match entry(&user)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring_core::Error::NoEntry) => {
            bail!("no oauth grant stored for {account_name}")
        }
        Err(error) => {
            Err(error).with_context(|| format!("deleting oauth grant for {account_name}"))
        }
    }
}

fn entry(user: &str) -> anyhow::Result<keyring_core::Entry> {
    ensure_keyring_store()?;
    keyring_core::Entry::new(KEYRING_SERVICE, user)
        .with_context(|| format!("opening keyring entry for {user}"))
}

/// Connects the process-wide default store to the Secret Service on
/// first use; tests pre-install the mock store instead.
fn ensure_keyring_store() -> anyhow::Result<()> {
    if keyring_core::get_default_store().is_some() {
        return Ok(());
    }
    let store = zbus_secret_service_keyring_store::Store::new()
        .context("connecting to the Secret Service (is a keyring daemon running?)")?;
    keyring_core::set_default_store(store);
    Ok(())
}

/// Process-global mock store so no test ever reaches the real OS
/// keyring; every keyring-touching test calls this first.
#[cfg(test)]
pub(crate) fn use_mock_keyring() {
    static MOCK_KEYRING: std::sync::Once = std::sync::Once::new();
    MOCK_KEYRING.call_once(|| {
        #[allow(clippy::expect_used)]
        keyring_core::set_default_store(
            keyring_core::mock::Store::new().expect("mock store never fails"),
        );
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_mail::ExposeSecret as _;

    use super::*;

    #[test]
    fn keyring_secret_round_trips() {
        use_mock_keyring();
        store_password("keyring-round-trip", "k3yr1ng").unwrap();
        let secret = load_password("keyring-round-trip").unwrap();
        assert_eq!(secret.expose_secret(), "k3yr1ng");
    }

    #[test]
    fn missing_keyring_entry_names_set_password() {
        use_mock_keyring();
        let message = load_password("keyring-absent").unwrap_err().to_string();
        assert!(message.contains(":set-password"), "{message}");
    }

    #[test]
    fn oauth_refresh_entry_is_separate_from_the_password() {
        use_mock_keyring();
        store_password("oauth-sep", "pass").unwrap();
        store_oauth_refresh("oauth-sep", &SecretString::from("refresh")).unwrap();
        delete_password("oauth-sep").unwrap();
        let survives = load_oauth_refresh("oauth-sep").unwrap();
        assert_eq!(survives.expose_secret(), "refresh");
        delete_oauth_refresh("oauth-sep").unwrap();
        let message = load_oauth_refresh("oauth-sep").unwrap_err().to_string();
        assert!(message.contains(":authorize"), "{message}");
    }
}
