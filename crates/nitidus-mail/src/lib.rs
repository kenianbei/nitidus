//! Mail engine for nitidus: mail I/O, sync, and the backend abstraction.
//! This crate must never depend on bevy — it runs on its own tokio
//! runtime and talks to the UI through channels.

mod actor;
mod backend;
pub mod cache;
mod command;
mod engine;
mod envelope;
mod error;
mod event;
pub mod imap;
pub mod maildir;
pub mod message;
pub(crate) mod net;
pub mod send;
pub mod thread;
mod types;
mod watch;

#[cfg(feature = "mock")]
pub mod mock;

pub use backend::MailBackend;
pub use command::MailCommand;
pub use engine::MailEngine;
pub use error::MailError;
pub use event::MailEvent;
pub use secrecy::{ExposeSecret, SecretString};
pub use types::{
    AccountId, ConnectionState, EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta, JobId,
};
