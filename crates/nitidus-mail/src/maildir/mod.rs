//! Hand-rolled maildir support: discovery, envelope scanning, the
//! `:2,` flag protocol, and message access.

mod backend;
mod folder_ops;
pub(crate) mod folders;
mod message;

pub use backend::MaildirBackend;
pub use folders::INBOX;
