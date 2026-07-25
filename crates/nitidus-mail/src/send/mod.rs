//! Outgoing transmission: one envelope + message through SMTP
//! (io-smtp coroutines over the shared net stream) or a sendmail-style
//! pipe. The engine spawns `transmit` on its runtime and reports
//! through `SendDone`/`JobFailed`.

mod pump;
mod sendmail;
mod smtp;

use crate::error::MailError;

/// Connection parameters resolved by the app — no config-file types
/// here, mirroring `ImapConfig`.
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub encryption: SmtpEncryption,
    /// `None` sends unauthenticated (local relays).
    pub credentials: Option<SmtpCredentials>,
}

#[derive(Clone)]
pub struct SmtpCredentials {
    pub user: String,
    pub password: secrecy::SecretString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpEncryption {
    Tls,
    StartTls,
    /// Plaintext — exists for in-process test servers; logged loudly.
    None,
}

#[derive(Clone)]
pub enum OutgoingTransport {
    Smtp(SmtpConfig),
    Sendmail { command: String },
}

/// The SMTP envelope, independent of the message headers (Bcc lives
/// here and only here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendEnvelope {
    pub from: String,
    pub recipients: Vec<String>,
}

pub(crate) async fn transmit(
    transport: &OutgoingTransport,
    envelope: &SendEnvelope,
    message: Vec<u8>,
) -> Result<(), MailError> {
    if envelope.recipients.is_empty() {
        return Err(MailError::Backend("no recipients in envelope".to_owned()));
    }
    match transport {
        OutgoingTransport::Smtp(config) => smtp::transmit(config, envelope, message).await,
        OutgoingTransport::Sendmail { command } => {
            sendmail::transmit(command, envelope, message).await
        }
    }
}
