//! One authenticated IMAP connection with lazy dialing, reconnect-once
//! retry for transport failures, and selected-mailbox tracking (reset
//! on reconnect, so retried commands re-select before running).

use std::time::Duration;

use io_imap::codec::fragmentizer::Fragmentizer;
use io_imap::coroutine::{ImapCoroutine, ImapYield};
use io_imap::rfc3501::greeting::{ImapGreetingGet, ImapGreetingGetOptions};
use io_imap::rfc3501::login::{ImapLogin, ImapLoginOptions};
use io_imap::rfc3501::select::{
    ImapMailboxSelect, ImapMailboxSelectData, ImapMailboxSelectOptions,
};
use io_imap::rfc3501::starttls::ImapStartTls;
use io_imap::sasl::auth_xoauth2::{ImapAuthXoauth2, ImapAuthXoauth2Options};
use io_imap::types::command::SelectParameter;
use io_imap::types::mailbox::Mailbox;
use io_imap::types::response::Capability;
use secrecy::ExposeSecret;

use super::{ImapConfig, ImapEncryption};
use crate::MailAuth;
use crate::error::MailError;
use crate::imap::pump::{self, PumpError};
use crate::net::{RemoteStream, connect_tcp, upgrade_tls};
use crate::oauth::TokenRefresher;

/// Matches io-imap's own examples; literals larger than this abort the
/// command instead of exhausting memory.
const FRAGMENTIZER_BYTES: u32 = 50 * 1024 * 1024;
const RECONNECT_DELAY: Duration = Duration::from_millis(750);

pub(super) struct ImapSession {
    config: ImapConfig,
    connection: Option<Connection>,
}

pub(super) struct Connection {
    pub stream: RemoteStream,
    pub fragmentizer: Fragmentizer,
    pub capabilities: Vec<Capability<'static>>,
    selected: Option<String>,
}

impl ImapSession {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            config,
            connection: None,
        }
    }

    /// Runs one command coroutine, reconnecting and retrying exactly
    /// once when the transport (not the command) fails.
    pub async fn run<C, T, E>(&mut self, make: impl Fn() -> C) -> Result<T, MailError>
    where
        C: ImapCoroutine<Yield = ImapYield, Return = Result<T, E>>,
        E: std::fmt::Display,
    {
        for attempt in 0..2 {
            let connection = self.ensure_connected().await?;
            match pump::run(&mut connection.stream, &mut connection.fragmentizer, make()).await {
                Ok(value) => return Ok(value),
                Err(PumpError::Command(message)) => return Err(MailError::Backend(message)),
                Err(PumpError::Io(message)) => {
                    self.connection = None;
                    if attempt == 1 {
                        return Err(MailError::Backend(format!("connection lost: {message}")));
                    }
                    tracing::debug!("imap reconnect after transport error: {message}");
                    tokio::time::sleep(RECONNECT_DELAY).await;
                }
            }
        }
        Err(MailError::Backend("imap retry loop exhausted".to_owned()))
    }

    /// SELECTs `mailbox` and returns the untagged response data; the
    /// canonical way scans open a folder. The CONDSTORE parameter goes
    /// only to servers that advertise it — Exchange Online answers
    /// `BAD Command Argument Error` otherwise.
    pub async fn select(&mut self, mailbox: &str) -> Result<ImapMailboxSelectData, MailError> {
        let target = parse_mailbox(mailbox)?;
        self.ensure_connected().await?;
        let parameters = if self.connection.as_ref().is_some_and(|connection| {
            io_imap::has_imap_capability!(connection.capabilities, CondStore)
        }) {
            vec![SelectParameter::CondStore]
        } else {
            Vec::new()
        };
        let data = self
            .run(|| {
                ImapMailboxSelect::new(
                    target.clone(),
                    ImapMailboxSelectOptions {
                        parameters: parameters.clone(),
                    },
                )
            })
            .await?;
        if let Some(connection) = &mut self.connection {
            connection.selected = Some(mailbox.to_owned());
        }
        Ok(data)
    }

    /// Runs a command that requires `mailbox` to be the selected state
    /// (UID FETCH/STORE); reconnects re-select automatically because
    /// `selected` dies with the connection.
    pub async fn run_selected<C, T, E>(
        &mut self,
        mailbox: &str,
        make: impl Fn() -> C,
    ) -> Result<T, MailError>
    where
        C: ImapCoroutine<Yield = ImapYield, Return = Result<T, E>>,
        E: std::fmt::Display,
    {
        let needs_select = self
            .connection
            .as_ref()
            .is_none_or(|connection| connection.selected.as_deref() != Some(mailbox));
        if needs_select {
            self.select(mailbox).await?;
        }
        self.run(make).await
    }

    async fn ensure_connected(&mut self) -> Result<&mut Connection, MailError> {
        if self.connection.is_none() {
            self.connection = Some(connect(&self.config).await?);
        }
        self.connection
            .as_mut()
            .ok_or_else(|| MailError::Backend("imap connection unavailable".to_owned()))
    }
}

pub(super) async fn connect(config: &ImapConfig) -> Result<Connection, MailError> {
    let tcp = connect_tcp(&config.host, config.port).await?;
    let mut fragmentizer = Fragmentizer::new(FRAGMENTIZER_BYTES);
    let mut stream = match config.encryption {
        ImapEncryption::Tls => {
            let mut stream = upgrade_tls(tcp, &config.host).await?;
            read_greeting(&mut stream, &mut fragmentizer).await?;
            stream
        }
        ImapEncryption::None => {
            let mut stream = RemoteStream::Plain(tcp);
            read_greeting(&mut stream, &mut fragmentizer).await?;
            stream
        }
        ImapEncryption::StartTls => {
            let mut plain = RemoteStream::Plain(tcp);
            let leftover = pump::run(&mut plain, &mut fragmentizer, ImapStartTls::new())
                .await
                .map_err(|error| MailError::Backend(format!("starttls: {error}")))?;
            if !leftover.is_empty() {
                tracing::warn!("discarding {} pre-TLS bytes", leftover.len());
            }
            let RemoteStream::Plain(tcp) = plain else {
                return Err(MailError::Backend("starttls stream state".to_owned()));
            };
            upgrade_tls(tcp, &config.host).await?
        }
    };
    let capabilities = authenticate(config, &mut stream, &mut fragmentizer).await?;
    Ok(Connection {
        stream,
        fragmentizer,
        capabilities,
        selected: None,
    })
}

async fn authenticate(
    config: &ImapConfig,
    stream: &mut RemoteStream,
    fragmentizer: &mut Fragmentizer,
) -> Result<Vec<Capability<'static>>, MailError> {
    match &config.auth {
        MailAuth::Login(password) => {
            let login = ImapLogin::new(
                &config.user,
                password.expose_secret(),
                ImapLoginOptions::default(),
            )
            .map_err(|error| {
                MailError::Backend(format!("invalid credentials encoding: {error}"))
            })?;
            pump::run(stream, fragmentizer, login)
                .await
                .map_err(|error| MailError::Backend(format!("login as {}: {error}", config.user)))
        }
        MailAuth::Xoauth2(tokens) => {
            match xoauth2_attempt(config, tokens, stream, fragmentizer).await {
                Ok(capabilities) => Ok(capabilities),
                // A rejected AUTHENTICATE leaves the connection usable;
                // the token may just be stale or revoked server-side.
                Err(PumpError::Command(_)) => {
                    tokens.invalidate();
                    xoauth2_attempt(config, tokens, stream, fragmentizer)
                        .await
                        .map_err(|error| xoauth2_error(&config.user, &error))
                }
                Err(error) => Err(xoauth2_error(&config.user, &error)),
            }
        }
    }
}

async fn xoauth2_attempt(
    config: &ImapConfig,
    tokens: &TokenRefresher,
    stream: &mut RemoteStream,
    fragmentizer: &mut Fragmentizer,
) -> Result<Vec<Capability<'static>>, PumpError> {
    let token = tokens
        .access_token()
        .await
        .map_err(|error| PumpError::Command(error.to_string()))?;
    let auth = ImapAuthXoauth2::new(
        &config.user,
        token.expose_secret(),
        ImapAuthXoauth2Options {
            initial_request: true,
            ..Default::default()
        },
    );
    pump::run(stream, fragmentizer, auth).await
}

fn xoauth2_error(user: &str, error: &PumpError) -> MailError {
    MailError::Backend(format!("xoauth2 as {user}: {error}"))
}

async fn read_greeting(
    stream: &mut RemoteStream,
    fragmentizer: &mut Fragmentizer,
) -> Result<(), MailError> {
    let options = ImapGreetingGetOptions {
        ensure_capabilities: false,
    };
    pump::run(stream, fragmentizer, ImapGreetingGet::new(options))
        .await
        .map(|_greeting| ())
        .map_err(|error| MailError::Backend(format!("greeting: {error}")))
}

pub(super) fn parse_mailbox(name: &str) -> Result<Mailbox<'static>, MailError> {
    Mailbox::try_from(name.to_owned())
        .map_err(|error| MailError::Backend(format!("invalid mailbox name {name:?}: {error}")))
}
