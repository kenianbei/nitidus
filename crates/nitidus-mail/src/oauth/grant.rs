//! The authorization-code grant with PKCE and a loopback redirect
//! listener; ends in a [`Grant`] whose refresh token the caller
//! persists. The device-code grant lives in [`super::device`].

use std::borrow::Cow;
use std::collections::BTreeMap;

use io_oauth::rfc6749::access_token_request::{
    Oauth20AccessTokenRequest, Oauth20AccessTokenRequestParams, Oauth20AccessTokenRequestResult,
};
use io_oauth::rfc6749::auth_request::Oauth20AuthRequestParams;
use io_oauth::rfc6749::auth_response::Oauth20AuthParams;
use io_oauth::rfc6749::issue_access_token::{
    Oauth20AccessTokenResponse, Oauth20AccessTokenSuccessParams,
};
use io_oauth::rfc6749::state::Oauth20State;
use io_oauth::rfc7636::pkce::{Oauth20PkceCodeChallenge, Oauth20PkceCodeVerifier};
use secrecy::SecretString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{PumpStep, Url, connect_endpoint, post_request, pump};
use crate::error::MailError;

const STATE_BYTES: u8 = 43;

/// A completed authorization: the tokens the provider handed back.
pub struct Grant {
    pub refresh_token: SecretString,
    pub access_token: SecretString,
}

#[derive(Clone)]
pub struct CodeFlowConfig {
    pub auth_endpoint: Url,
    pub token_endpoint: Url,
    pub client_id: String,
    pub client_secret: Option<SecretString>,
    pub scopes: Vec<String>,
    /// Provider-specific query parameters (Google needs
    /// `access_type=offline` + `prompt=consent` to issue a refresh
    /// token).
    pub auth_extras: Vec<(String, String)>,
}

/// The authorization-code grant, split so the caller can hand the URL
/// to a browser before blocking on the redirect.
pub struct CodeFlow {
    listener: TcpListener,
    redirect_uri: String,
    state: Oauth20State,
    pkce: Oauth20PkceCodeChallenge,
    config: CodeFlowConfig,
}

impl CodeFlow {
    /// Binds the loopback listener and returns the URL to open in the
    /// user's browser.
    pub async fn start(config: CodeFlowConfig) -> Result<(Self, Url), MailError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| MailError::Backend(format!("loopback listener: {error}")))?;
        let port = listener
            .local_addr()
            .map_err(|error| MailError::Backend(format!("loopback address: {error}")))?
            .port();
        // "localhost", not "127.0.0.1": Entra matches the loopback
        // redirect host literally (AADSTS50011), and Google accepts
        // either. The listener still binds the v4 loopback; browsers
        // fall back from ::1 on connection-refused.
        let redirect_uri = format!("http://localhost:{port}/");
        let state = url_safe_state();
        let pkce = Oauth20PkceCodeChallenge::default();
        let params = Oauth20AuthRequestParams {
            client_id: Cow::Borrowed(&config.client_id),
            redirect_uri: Some(Cow::Borrowed(&redirect_uri)),
            scope: config
                .scopes
                .iter()
                .map(|s| Cow::Borrowed(s.as_str()))
                .collect(),
            state: Some(Cow::Borrowed(&state)),
            pkce_code_challenge: Some(Cow::Borrowed(&pkce)),
            extras: config
                .auth_extras
                .iter()
                .map(|(key, value)| (Cow::Borrowed(key.as_str()), Cow::Borrowed(value.as_str())))
                .collect::<BTreeMap<_, _>>(),
        };
        let url = params.build_url(&config.auth_endpoint);
        Ok((
            Self {
                listener,
                redirect_uri,
                state,
                pkce,
                config,
            },
            url,
        ))
    }

    /// Blocks until the browser redirect delivers the code, then
    /// exchanges it at the token endpoint.
    pub async fn finish(self) -> Result<Grant, MailError> {
        let code = self.wait_for_code().await?;
        let params = Oauth20AccessTokenRequestParams {
            code: Cow::Owned(code),
            redirect_uri: Some(Cow::Borrowed(self.redirect_uri.as_str())),
            client_id: Cow::Borrowed(&self.config.client_id),
            client_secret: self.config.client_secret.clone(),
            pkce_code_verifier: Some(Cow::Borrowed(&self.pkce.verifier)),
        };
        let request = post_request(&self.config.token_endpoint);
        let mut stream = connect_endpoint(&self.config.token_endpoint).await?;
        let mut coroutine = Oauth20AccessTokenRequest::new(request, params);
        let response = pump(&mut stream, |arg| {
            Ok(match coroutine.resume(arg) {
                Oauth20AccessTokenRequestResult::Ok(response) => PumpStep::Done(response),
                Oauth20AccessTokenRequestResult::WantsRead => PumpStep::Read,
                Oauth20AccessTokenRequestResult::WantsWrite(bytes) => PumpStep::Write(bytes),
                Oauth20AccessTokenRequestResult::Err(error) => {
                    return Err(MailError::Backend(format!("code exchange: {error}")));
                }
            })
        })
        .await?;
        grant_from(response)
    }

    /// Serves loopback requests until one carries the authorization
    /// response; stray requests (favicon probes) get a 404.
    async fn wait_for_code(&self) -> Result<String, MailError> {
        loop {
            let (mut browser, _) = self
                .listener
                .accept()
                .await
                .map_err(|error| MailError::Backend(format!("loopback accept: {error}")))?;
            let Some(target) = read_request_target(&mut browser).await else {
                continue;
            };
            if !target.starts_with("/?") {
                respond(&mut browser, "404 Not Found", "nothing here").await;
                continue;
            }
            let url = match Url::parse(&format!("http://127.0.0.1{target}")) {
                Ok(url) => url,
                Err(_) => continue,
            };
            let outcome = Oauth20AuthParams::from(&url).validate(Some(&self.state));
            match outcome {
                Ok(code) => {
                    respond(
                        &mut browser,
                        "200 OK",
                        "nitidus is authorized — you can close this tab.",
                    )
                    .await;
                    return Ok(code.into_owned());
                }
                Err(error) => {
                    respond(
                        &mut browser,
                        "200 OK",
                        "authorization failed — see nitidus.",
                    )
                    .await;
                    return Err(MailError::Backend(format!("authorization: {error}")));
                }
            }
        }
    }
}

/// Entra rejects states containing HTML-dangerous characters
/// (AADSTS90013), so the state is drawn from PKCE's unreserved
/// alphabet instead of io-oauth's full printable-ASCII range.
fn url_safe_state() -> Oauth20State {
    let seed = Oauth20PkceCodeVerifier::new(STATE_BYTES);
    let text = String::from_utf8_lossy(seed.expose()).into_owned();
    let deserializer = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&text);
    // Unreserved characters are a subset of VSCHAR, so this cannot
    // fail; the fallback only satisfies the no-panic rule.
    serde::Deserialize::deserialize(deserializer).unwrap_or_default()
}

pub(crate) fn grant_from(response: Oauth20AccessTokenResponse) -> Result<Grant, MailError> {
    let success: Oauth20AccessTokenSuccessParams = response.map_err(|denied| {
        MailError::Backend(format!("authorization denied: {:?}", denied.error))
    })?;
    let refresh_token = success.refresh_token.ok_or_else(|| {
        MailError::Backend(
            "provider returned no refresh token — check offline access / consent parameters"
                .to_owned(),
        )
    })?;
    Ok(Grant {
        refresh_token,
        access_token: success.access_token,
    })
}

/// First-line target of a loopback HTTP request; `None` drops requests
/// this flow does not understand.
async fn read_request_target(browser: &mut TcpStream) -> Option<String> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let n = browser.read(&mut buffer).await.ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..n]);
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&raw);
    let mut request_line = head.lines().next()?.split_whitespace();
    let _method = request_line.next()?;
    request_line.next().map(str::to_owned)
}

async fn respond(browser: &mut TcpStream, status: &str, body: &str) {
    let page = format!("<html><body><p>{body}</p></body></html>");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    if let Err(error) = browser.write_all(response.as_bytes()).await {
        tracing::debug!("loopback response: {error}");
    }
}
