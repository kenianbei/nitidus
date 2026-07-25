//! The SMTP submission flow: greeting → EHLO → optional STARTTLS
//! upgrade (with a fresh EHLO) → optional AUTH PLAIN → MAIL/RCPT/DATA
//! via the composite send coroutine → QUIT.

use std::borrow::Cow;

use io_smtp::message::SmtpMessageSend;
use io_smtp::rfc3207::starttls::SmtpStartTls;
use io_smtp::rfc5321::ehlo::SmtpEhlo;
use io_smtp::rfc5321::greeting::SmtpGreetingGet;
use io_smtp::rfc5321::quit::SmtpQuit;
use io_smtp::rfc5321::{
    SmtpDomain, SmtpEhloDomain, SmtpForwardPath, SmtpLocalPart, SmtpMailbox, SmtpReversePath,
};
use io_smtp::sasl::auth_plain::{SmtpAuthPlain, SmtpAuthPlainOptions};
use secrecy::SecretString;

use super::pump::run;
use super::{SendEnvelope, SmtpConfig, SmtpEncryption};
use crate::error::MailError;
use crate::net::{RemoteStream, connect_tcp, upgrade_tls};

/// The identity we announce in EHLO; submission servers do not resolve
/// it, and the account's real domain is not necessarily ours to claim.
const EHLO_DOMAIN: &str = "localhost";

pub(super) async fn transmit(
    config: &SmtpConfig,
    envelope: &SendEnvelope,
    message: Vec<u8>,
) -> Result<(), MailError> {
    if config.encryption == SmtpEncryption::None {
        tracing::warn!("plaintext SMTP connection to {}", config.host);
    }
    let mut stream = connect(config).await?;
    authenticate(config, &mut stream).await?;

    let reverse_path = SmtpReversePath::SmtpMailbox(parse_mailbox(&envelope.from)?);
    let forward_paths = envelope
        .recipients
        .iter()
        .map(|recipient| Ok(SmtpForwardPath(parse_mailbox(recipient)?)))
        .collect::<Result<Vec<_>, MailError>>()?;
    run(
        &mut stream,
        SmtpMessageSend::new(reverse_path, forward_paths, message),
    )
    .await?;
    let _quit = run(&mut stream, SmtpQuit::new()).await;
    Ok(())
}

async fn connect(config: &SmtpConfig) -> Result<RemoteStream, MailError> {
    let tcp = connect_tcp(&config.host, config.port).await?;
    match config.encryption {
        SmtpEncryption::Tls => {
            let mut stream = upgrade_tls(tcp, &config.host).await?;
            run(&mut stream, SmtpGreetingGet::new()).await?;
            run(&mut stream, SmtpEhlo::new(ehlo_domain())).await?;
            Ok(stream)
        }
        SmtpEncryption::None => {
            let mut stream = RemoteStream::Plain(tcp);
            run(&mut stream, SmtpGreetingGet::new()).await?;
            run(&mut stream, SmtpEhlo::new(ehlo_domain())).await?;
            Ok(stream)
        }
        SmtpEncryption::StartTls => {
            let mut plain = RemoteStream::Plain(tcp);
            run(&mut plain, SmtpGreetingGet::new()).await?;
            run(&mut plain, SmtpEhlo::new(ehlo_domain())).await?;
            let leftover = run(&mut plain, SmtpStartTls::new()).await?;
            if !leftover.is_empty() {
                tracing::warn!("discarding {} pre-TLS smtp bytes", leftover.len());
            }
            let RemoteStream::Plain(tcp) = plain else {
                return Err(MailError::Backend("starttls stream state".to_owned()));
            };
            let mut stream = upgrade_tls(tcp, &config.host).await?;
            run(&mut stream, SmtpEhlo::new(ehlo_domain())).await?;
            Ok(stream)
        }
    }
}

async fn authenticate(config: &SmtpConfig, stream: &mut RemoteStream) -> Result<(), MailError> {
    let Some(credentials) = &config.credentials else {
        return Ok(());
    };
    let password = SecretString::from(credentials.password.clone());
    let auth = SmtpAuthPlain::new(
        &credentials.user,
        &password,
        ehlo_domain(),
        SmtpAuthPlainOptions {
            initial_request: true,
            ensure_capabilities: false,
        },
    );
    run(stream, auth)
        .await
        .map_err(|error| MailError::Backend(format!("smtp auth as {}: {error}", credentials.user)))
}

fn ehlo_domain() -> SmtpEhloDomain<'static> {
    SmtpEhloDomain::SmtpDomain(SmtpDomain(Cow::Borrowed(EHLO_DOMAIN)))
}

fn parse_mailbox(address: &str) -> Result<SmtpMailbox<'static>, MailError> {
    let trimmed = address.trim();
    let (local, domain) = trimmed
        .rsplit_once('@')
        .filter(|(local, domain)| !local.is_empty() && !domain.is_empty())
        .ok_or_else(|| MailError::Backend(format!("not an address: {address:?}")))?;
    Ok(SmtpMailbox {
        local_part: SmtpLocalPart(Cow::Owned(local.to_owned())),
        domain: SmtpEhloDomain::SmtpDomain(SmtpDomain(Cow::Owned(domain.to_owned()))),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_addresses_and_rejects_garbage() {
        let mailbox = parse_mailbox(" alice@example.com ").unwrap();
        assert_eq!(mailbox.local_part.0, "alice");
        assert!(parse_mailbox("nonsense").is_err());
        assert!(parse_mailbox("@example.com").is_err());
        assert!(parse_mailbox("alice@").is_err());
    }
}
