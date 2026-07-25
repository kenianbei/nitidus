//! Compiled-in OAuth2 provider endpoints and scopes; the config only
//! names the provider and supplies the client credentials.

use super::account::Oauth2Provider;

pub struct ProviderPreset {
    pub auth_endpoint: &'static str,
    pub token_endpoint: &'static str,
    /// Device-authorization endpoint; the provider's preferred grant
    /// is the device flow when set, the loopback auth-code flow
    /// otherwise.
    pub device_endpoint: Option<&'static str>,
    pub scopes: &'static [&'static str],
    /// Extra authorization-URL query parameters the provider needs.
    pub auth_extras: &'static [(&'static str, &'static str)],
}

/// Google only issues a refresh token for offline access, and only
/// re-issues one when consent is re-prompted.
const GOOGLE_AUTH_EXTRAS: &[(&str, &str)] = &[("access_type", "offline"), ("prompt", "consent")];

/// XOAUTH2 requires the full-mail scope; narrower API scopes are not
/// accepted by the IMAP/SMTP endpoints.
const GOOGLE_SCOPES: &[&str] = &["https://mail.google.com/"];

const MICROSOFT_SCOPES: &[&str] = &[
    "https://outlook.office.com/IMAP.AccessAsUser.All",
    "https://outlook.office.com/SMTP.Send",
    "offline_access",
];

pub fn preset(provider: Oauth2Provider) -> ProviderPreset {
    match provider {
        Oauth2Provider::Google => ProviderPreset {
            auth_endpoint: "https://accounts.google.com/o/oauth2/v2/auth",
            token_endpoint: "https://oauth2.googleapis.com/token",
            device_endpoint: None,
            scopes: GOOGLE_SCOPES,
            auth_extras: GOOGLE_AUTH_EXTRAS,
        },
        Oauth2Provider::Microsoft => ProviderPreset {
            auth_endpoint: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_endpoint: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            device_endpoint: Some(
                "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode",
            ),
            scopes: MICROSOFT_SCOPES,
            auth_extras: &[],
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn preset_endpoints_parse_as_https_urls() {
        for provider in [Oauth2Provider::Google, Oauth2Provider::Microsoft] {
            let preset = preset(provider);
            for endpoint in [
                Some(preset.auth_endpoint),
                Some(preset.token_endpoint),
                preset.device_endpoint,
            ]
            .into_iter()
            .flatten()
            {
                let url = nitidus_mail::oauth::Url::parse(endpoint).unwrap();
                assert_eq!(url.scheme(), "https", "{endpoint}");
            }
            assert!(!preset.scopes.is_empty());
        }
    }
}
