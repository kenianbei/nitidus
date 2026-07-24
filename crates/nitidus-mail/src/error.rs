//! Engine errors are data: they cross the event channel and surface in
//! the UI, they never panic the engine.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("backend error: {0}")]
    Backend(String),
    #[error("job cancelled")]
    Cancelled,
    #[error("engine channel closed")]
    ChannelClosed,
    #[error("unknown account: {0}")]
    UnknownAccount(String),
    #[error("failed to start engine runtime")]
    Runtime(#[from] std::io::Error),
}

impl Clone for MailError {
    fn clone(&self) -> Self {
        match self {
            Self::Backend(message) => Self::Backend(message.clone()),
            Self::Cancelled => Self::Cancelled,
            Self::ChannelClosed => Self::ChannelClosed,
            Self::UnknownAccount(account) => Self::UnknownAccount(account.clone()),
            Self::Runtime(error) => Self::Backend(format!("runtime: {error}")),
        }
    }
}
