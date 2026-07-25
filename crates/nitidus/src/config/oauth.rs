//! Turns an account's auth config into the engine-side `MailAuth`:
//! password sources resolve through `secrets`, oauth2 builds a
//! `TokenRefresher` backed by the keyring refresh-token entry.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use nitidus_mail::oauth::{OauthClient, TokenRefresher, Url};
use nitidus_mail::{MailAuth, SecretString};

use super::account::{AccountConfig, Auth, Oauth2Auth};
use super::{keyring, presets, secrets};

pub fn resolve_auth(account: &AccountConfig, config_dir: &Path) -> anyhow::Result<MailAuth> {
    match &account.auth {
        Auth::Oauth2(oauth) => build_xoauth2(&account.name, oauth),
        password_source => Ok(MailAuth::Login(secrets::resolve_password(
            password_source,
            config_dir,
            &account.name,
        )?)),
    }
}

fn build_xoauth2(account_name: &str, oauth: &Oauth2Auth) -> anyhow::Result<MailAuth> {
    let refresh_token = keyring::load_oauth_refresh(account_name)?;
    let preset = presets::preset(oauth.provider);
    let token_endpoint = Url::parse(preset.token_endpoint)
        .with_context(|| format!("token endpoint for {:?}", oauth.provider))?;
    let client = OauthClient {
        token_endpoint,
        client_id: oauth.client_id.clone(),
        client_secret: oauth.client_secret.clone().map(SecretString::from),
    };
    let account = account_name.to_owned();
    let persist = Box::new(move |token: &SecretString| {
        if let Err(error) = keyring::store_oauth_refresh(&account, token) {
            tracing::warn!("persisting rotated refresh token for {account}: {error:#}");
        }
    });
    Ok(MailAuth::Xoauth2(Arc::new(TokenRefresher::new(
        client,
        refresh_token,
        persist,
    ))))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::super::account::Oauth2Provider;
    use super::*;

    fn oauth_account(name: &str) -> AccountConfig {
        AccountConfig {
            name: name.to_owned(),
            email: format!("{name}@example.com"),
            auth: Auth::Oauth2(Oauth2Auth {
                provider: Oauth2Provider::Google,
                client_id: "client-123".to_owned(),
                client_secret: None,
                flow: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn missing_grant_resolves_to_the_authorize_notice() {
        keyring::use_mock_keyring();
        let error = match resolve_auth(&oauth_account("oauth-ungran"), Path::new("/nonexistent")) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("resolution must fail without a stored grant"),
        };
        assert!(error.contains(":authorize"), "{error}");
    }

    #[test]
    fn stored_grant_builds_the_xoauth2_refresher() {
        keyring::use_mock_keyring();
        keyring::store_oauth_refresh("oauth-granted", &SecretString::from("refresh-x")).unwrap();
        let auth =
            resolve_auth(&oauth_account("oauth-granted"), Path::new("/nonexistent")).unwrap();
        assert!(matches!(auth, MailAuth::Xoauth2(_)));
    }
}
