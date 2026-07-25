//! OAuth2 access-token supply for XOAUTH2 authentication: a cached
//! access token, refreshed through io-oauth over the shared transport
//! when stale, with rotated refresh tokens handed back to the caller
//! for persistence. Interactive grants live in [`grant`] and
//! [`device`].

pub mod device;
pub mod grant;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use io_http::rfc9110::request::HttpRequest;
use io_oauth::rfc6749::refresh_access_token::{
    Oauth20AccessTokenRefresh, Oauth20AccessTokenRefreshParams, Oauth20AccessTokenRefreshResult,
};
use secrecy::SecretString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub use url::Url;

use crate::error::MailError;
use crate::net::{RemoteStream, connect_tcp, upgrade_tls};

/// Refresh this long before the server-reported expiry.
const STALENESS_MARGIN: Duration = Duration::from_secs(60);
/// Access tokens whose response carried no `expires_in`.
const FALLBACK_LIFETIME: Duration = Duration::from_secs(30 * 60);
const READ_BUFFER_BYTES: usize = 16 * 1024;

/// Everything needed to talk to a provider's token endpoint.
#[derive(Clone)]
pub struct OauthClient {
    pub token_endpoint: Url,
    pub client_id: String,
    pub client_secret: Option<SecretString>,
}

/// Called with the new refresh token whenever the provider rotates it.
pub type PersistRefreshToken = Box<dyn Fn(&SecretString) + Send + Sync>;

/// Serves the current access token to connecting sessions.
pub struct TokenRefresher {
    client: Option<OauthClient>,
    refresh_token: Mutex<Option<SecretString>>,
    persist: Option<PersistRefreshToken>,
    access: Mutex<Option<CachedAccess>>,
    /// Serializes refreshes so parallel IMAP/SMTP connects do one
    /// round-trip, not two.
    refresh_gate: tokio::sync::Mutex<()>,
}

struct CachedAccess {
    token: SecretString,
    expires_at: Option<Instant>,
}

impl TokenRefresher {
    /// A refresher that always serves the same token — for tests and
    /// callers that manage refresh themselves.
    pub fn fixed(token: SecretString) -> Self {
        Self {
            client: None,
            refresh_token: Mutex::new(None),
            persist: None,
            access: Mutex::new(Some(CachedAccess {
                token,
                expires_at: None,
            })),
            refresh_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub fn new(
        client: OauthClient,
        refresh_token: SecretString,
        persist: PersistRefreshToken,
    ) -> Self {
        Self {
            client: Some(client),
            refresh_token: Mutex::new(Some(refresh_token)),
            persist: Some(persist),
            access: Mutex::new(None),
            refresh_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn access_token(&self) -> Result<SecretString, MailError> {
        if let Some(token) = self.cached() {
            return Ok(token);
        }
        let Some(client) = self.client.clone() else {
            return Err(MailError::Backend("no access token available".to_owned()));
        };
        let _gate = self.refresh_gate.lock().await;
        if let Some(token) = self.cached() {
            return Ok(token);
        }
        self.refresh(&client).await
    }

    /// Drops the cached token after a server-side rejection so the
    /// next attempt refreshes. A no-op without a refresh client —
    /// clearing a fixed token would only turn a clean rejection into a
    /// missing-token error.
    pub fn invalidate(&self) {
        if self.client.is_some()
            && let Ok(mut access) = self.access.lock()
        {
            *access = None;
        }
    }

    fn cached(&self) -> Option<SecretString> {
        let access = self.access.lock().ok()?;
        let cached = access.as_ref()?;
        match cached.expires_at {
            Some(expires_at) if Instant::now() >= expires_at => None,
            _ => Some(cached.token.clone()),
        }
    }

    async fn refresh(&self, client: &OauthClient) -> Result<SecretString, MailError> {
        let refresh_token = self
            .refresh_token
            .lock()
            .map_err(|_| MailError::Backend("refresh token poisoned".to_owned()))?
            .clone()
            .ok_or_else(|| MailError::Backend("no refresh token available".to_owned()))?;
        let mut params =
            Oauth20AccessTokenRefreshParams::new(&client.client_id, refresh_token.clone());
        params.client_secret = client.client_secret.clone();
        let request = post_request(&client.token_endpoint);
        let mut stream = connect_endpoint(&client.token_endpoint).await?;
        let mut coroutine = Oauth20AccessTokenRefresh::new(request, params);
        let granted = pump(&mut stream, |arg| {
            Ok(match coroutine.resume(arg) {
                Oauth20AccessTokenRefreshResult::Ok(response) => PumpStep::Done(response),
                Oauth20AccessTokenRefreshResult::WantsRead => PumpStep::Read,
                Oauth20AccessTokenRefreshResult::WantsWrite(bytes) => PumpStep::Write(bytes),
                Oauth20AccessTokenRefreshResult::Err(error) => {
                    return Err(MailError::Backend(format!("token refresh: {error}")));
                }
            })
        })
        .await?
        .map_err(|denied| {
            MailError::Backend(format!("token refresh denied: {:?}", denied.error))
        })?;
        Ok(self.store_grant(granted))
    }

    fn store_grant(
        &self,
        granted: io_oauth::rfc6749::issue_access_token::Oauth20AccessTokenSuccessParams,
    ) -> SecretString {
        let lifetime = granted
            .expires_in
            .map_or(FALLBACK_LIFETIME, |seconds| {
                Duration::from_secs(seconds as u64)
            })
            .saturating_sub(STALENESS_MARGIN);
        let token = granted.access_token.clone();
        if let Ok(mut access) = self.access.lock() {
            *access = Some(CachedAccess {
                token: token.clone(),
                expires_at: Some(Instant::now() + lifetime),
            });
        }
        if let Some(rotated) = granted.refresh_token {
            if let Ok(mut refresh) = self.refresh_token.lock() {
                *refresh = Some(rotated.clone());
            }
            if let Some(persist) = &self.persist {
                persist(&rotated);
            }
        }
        token
    }
}

pub(crate) fn post_request(endpoint: &Url) -> HttpRequest {
    HttpRequest {
        method: "POST".to_owned(),
        url: endpoint.clone(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

/// Dials the endpoint host: TLS for https, plain TCP for http (which
/// exists for in-process test servers, mirroring `Encryption::None`).
pub(crate) async fn connect_endpoint(endpoint: &Url) -> Result<RemoteStream, MailError> {
    let host = endpoint
        .host_str()
        .ok_or_else(|| MailError::Backend(format!("token endpoint has no host: {endpoint}")))?;
    let is_tls = endpoint.scheme() == "https";
    let default_port = if is_tls { 443 } else { 80 };
    let tcp = connect_tcp(host, endpoint.port().unwrap_or(default_port)).await?;
    if is_tls {
        upgrade_tls(tcp, host).await
    } else {
        Ok(RemoteStream::Plain(tcp))
    }
}

/// One step of a resumed sans-IO coroutine, normalized across the
/// per-flow result enums.
pub(crate) enum PumpStep<T> {
    Done(T),
    Read,
    Write(Vec<u8>),
}

/// Drives any io-oauth coroutine over the stream; `step` maps the
/// flow-specific resume result into a [`PumpStep`].
pub(crate) async fn pump<T>(
    stream: &mut RemoteStream,
    mut step: impl FnMut(Option<&[u8]>) -> Result<PumpStep<T>, MailError>,
) -> Result<T, MailError> {
    let mut buffer = [0u8; READ_BUFFER_BYTES];
    let mut input: Option<usize> = None;
    loop {
        let arg = input.take().map(|n| &buffer[..n]);
        match step(arg)? {
            PumpStep::Done(value) => return Ok(value),
            PumpStep::Write(bytes) => write_all(stream, &bytes).await?,
            PumpStep::Read => input = Some(read_some(stream, &mut buffer).await?),
        }
    }
}

pub(crate) async fn write_all(stream: &mut RemoteStream, bytes: &[u8]) -> Result<(), MailError> {
    stream
        .write_all(bytes)
        .await
        .map_err(|error| MailError::Backend(format!("oauth write: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| MailError::Backend(format!("oauth flush: {error}")))
}

pub(crate) async fn read_some(
    stream: &mut RemoteStream,
    buffer: &mut [u8],
) -> Result<usize, MailError> {
    let n = stream
        .read(buffer)
        .await
        .map_err(|error| MailError::Backend(format!("oauth read: {error}")))?;
    if n == 0 {
        return Err(MailError::Backend("oauth connection closed".to_owned()));
    }
    Ok(n)
}

impl std::fmt::Debug for TokenRefresher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenRefresher")
            .field("has_client", &self.client.is_some())
            .finish_non_exhaustive()
    }
}
