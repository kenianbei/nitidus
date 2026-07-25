//! The IMAP `MailBackend` (io-imap coroutines pumped over tokio +
//! rustls): folder listing, streaming envelope sync with session-scoped
//! incremental re-scans, message fetch, flag writes, folder ops, and an
//! engine-level INBOX watch via IDLE.

mod backend;
mod envelopes;
mod folders;
mod pump;
mod session;
mod sync;
mod watch;

pub use backend::ImapBackend;

pub(crate) use crate::maildir::INBOX;

/// Connection parameters resolved by the app; `nitidus-mail` never
/// reads config files or runs password commands itself.
#[derive(Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub encryption: ImapEncryption,
    pub user: String,
    pub password: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImapEncryption {
    Tls,
    StartTls,
    /// Plaintext — exists for in-process test servers; logged loudly.
    None,
}
