//! Where everything the app wants to tell the user goes.
//!
//! One call site vocabulary — `info`, `warn`, `error` — and one policy
//! deciding what happens to each, rather than every caller picking a
//! destination for itself. Every message lands in a bounded log; on top of that,
//! `Info` shows briefly on the statusline and the two louder severities
//! surface as toasts, which stack, wrap, and are hard to miss.
//!
//! The log is the durable record: the statusline and toasts both expire,
//! so `:messages` is how you read what scrolled past.

use std::collections::VecDeque;

use bevy::prelude::*;

const STATUS_MESSAGE_TTL_SECS: f64 = 4.0;
/// Deep enough to cover a session's worth of scrollback, bounded so a
/// chatty sync cannot grow it without limit. Not persisted across runs.
const LOG_CAPACITY: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    /// `Info` is quiet enough for the statusline; anything louder gets a
    /// toast, which survives longer and does not compete with the
    /// chord hint for one row.
    fn toasts(self) -> bool {
        !matches!(self, Severity::Info)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub severity: Severity,
    pub text: String,
}

#[derive(Resource, Default)]
pub struct MessageLog {
    entries: VecDeque<LogEntry>,
    /// The statusline's transient line: `Info` only, and only until it
    /// expires.
    current: Option<String>,
    expires_at_secs: f64,
    /// Written but not yet handed to the toast engine.
    pending: Vec<LogEntry>,
}

impl MessageLog {
    pub fn info(&mut self, text: String, now_secs: f64) {
        self.push(text, Severity::Info, now_secs);
    }

    pub fn warn(&mut self, text: String, now_secs: f64) {
        self.push(text, Severity::Warning, now_secs);
    }

    pub fn error(&mut self, text: String, now_secs: f64) {
        self.push(text, Severity::Error, now_secs);
    }

    fn push(&mut self, text: String, severity: Severity, now_secs: f64) {
        let entry = LogEntry { severity, text };
        if severity.toasts() {
            self.pending.push(entry.clone());
        } else {
            self.current = Some(entry.text.clone());
            self.expires_at_secs = now_secs + STATUS_MESSAGE_TTL_SECS;
        }
        if self.entries.len() == LOG_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// The statusline's current line, if one is live.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Newest last, which is how the log panel reads.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn take_pending(&mut self) -> Vec<LogEntry> {
        std::mem::take(&mut self.pending)
    }

    fn expired(&self, now_secs: f64) -> bool {
        self.current.is_some() && now_secs >= self.expires_at_secs
    }
}

pub fn expire_status_messages(time: Res<Time>, mut log: ResMut<MessageLog>) {
    if log.expired(time.elapsed_secs_f64()) {
        log.current = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn texts(log: &MessageLog) -> Vec<&str> {
        log.entries().map(|entry| entry.text.as_str()).collect()
    }

    #[test]
    fn info_reaches_the_statusline_and_expires_after_its_ttl() {
        let mut log = MessageLog::default();
        log.info("synced".to_owned(), 10.0);

        assert_eq!(log.current(), Some("synced"));
        assert!(!log.expired(10.0 + STATUS_MESSAGE_TTL_SECS - 0.1));
        assert!(log.expired(10.0 + STATUS_MESSAGE_TTL_SECS));
    }

    #[test]
    fn warnings_and_errors_toast_instead_of_taking_the_statusline() {
        let mut log = MessageLog::default();
        log.warn("fetch failed".to_owned(), 1.0);
        log.error("send failed".to_owned(), 2.0);

        assert_eq!(
            log.current(),
            None,
            "the louder severities must not compete for the status row"
        );
        let pending = log.take_pending();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].severity, Severity::Warning);
        assert_eq!(pending[1].severity, Severity::Error);
    }

    #[test]
    fn info_is_logged_but_never_queued_for_a_toast() {
        let mut log = MessageLog::default();
        log.info("synced".to_owned(), 1.0);

        assert!(log.take_pending().is_empty());
        assert_eq!(texts(&log), vec!["synced"]);
    }

    #[test]
    fn pending_toasts_are_handed_over_once() {
        let mut log = MessageLog::default();
        log.warn("first".to_owned(), 1.0);

        assert_eq!(log.take_pending().len(), 1);
        assert!(
            log.take_pending().is_empty(),
            "a drained toast must not surface twice"
        );
    }

    #[test]
    fn every_severity_lands_in_the_log() {
        let mut log = MessageLog::default();
        log.info("a".to_owned(), 1.0);
        log.warn("b".to_owned(), 2.0);
        log.error("c".to_owned(), 3.0);

        assert_eq!(texts(&log), vec!["a", "b", "c"], "newest last");
    }

    #[test]
    fn the_log_bounds_itself_by_evicting_the_oldest() {
        let mut log = MessageLog::default();
        for index in 0..LOG_CAPACITY + 3 {
            log.info(index.to_string(), index as f64);
        }

        assert_eq!(log.entries().len(), LOG_CAPACITY);
        assert_eq!(
            texts(&log).first().copied(),
            Some("3"),
            "the three oldest must have been evicted"
        );
    }

    #[test]
    fn an_empty_log_never_expires() {
        assert!(!MessageLog::default().expired(f64::MAX));
    }
}
