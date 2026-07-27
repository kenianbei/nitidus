//! Maildir support over `io-maildir`: the Maildir++ layout, envelope
//! scanning through a bounded header window, and a guard layer in front
//! of the unguarded folder coroutines.

mod backend;
mod flags;
mod folder_ops;
pub(crate) mod folders;
mod scan;

pub use backend::MaildirBackend;
pub use folders::INBOX;
