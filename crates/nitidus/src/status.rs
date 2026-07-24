//! Ephemeral statusline messages with severity and a time-to-live.

use bevy::prelude::*;

const STATUS_MESSAGE_TTL_SECS: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct StatusMessage {
    current: Option<(String, Severity)>,
    expires_at_secs: f64,
}

impl StatusMessage {
    pub fn info(&mut self, text: String, now_secs: f64) {
        self.set(text, Severity::Info, now_secs);
    }

    pub fn warn(&mut self, text: String, now_secs: f64) {
        self.set(text, Severity::Warning, now_secs);
    }

    pub fn error(&mut self, text: String, now_secs: f64) {
        self.set(text, Severity::Error, now_secs);
    }

    fn set(&mut self, text: String, severity: Severity, now_secs: f64) {
        self.current = Some((text, severity));
        self.expires_at_secs = now_secs + STATUS_MESSAGE_TTL_SECS;
    }

    pub fn current(&self) -> Option<(&str, Severity)> {
        self.current
            .as_ref()
            .map(|(text, severity)| (text.as_str(), *severity))
    }

    fn expired(&self, now_secs: f64) -> bool {
        self.current.is_some() && now_secs >= self.expires_at_secs
    }
}

pub fn expire_status_messages(time: Res<Time>, mut status: ResMut<StatusMessage>) {
    if status.expired(time.elapsed_secs_f64()) {
        status.current = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn message_expires_after_ttl() {
        let mut status = StatusMessage::default();
        status.info("hello".to_owned(), 10.0);
        assert_eq!(status.current(), Some(("hello", Severity::Info)));
        assert!(!status.expired(10.0 + STATUS_MESSAGE_TTL_SECS - 0.1));
        assert!(status.expired(10.0 + STATUS_MESSAGE_TTL_SECS));
    }

    #[test]
    fn empty_status_never_expires() {
        assert!(!StatusMessage::default().expired(f64::MAX));
    }
}
