//! Backend-shared envelope summarization: one place turns raw message
//! headers into an `EnvelopeSummary`, so maildir files and IMAP header
//! fetches decode subjects, addresses, and reference chains identically.

use mail_parser::MessageParser;

use crate::types::{EnvelopeId, EnvelopeSummary, Flags};

pub(crate) fn summarize_headers(
    raw_headers: &[u8],
    id: EnvelopeId,
    flags: Flags,
    fallback_date_epoch_secs: i64,
) -> EnvelopeSummary {
    let parsed = MessageParser::default().parse(raw_headers);
    let (subject, from_display, from_addr, date, message_id, references) = match &parsed {
        Some(message) => (
            message.subject().unwrap_or("(no subject)").to_owned(),
            message
                .from()
                .and_then(|from| from.first())
                .and_then(|addr| addr.name())
                .unwrap_or_default()
                .to_owned(),
            message
                .from()
                .and_then(|from| from.first())
                .and_then(|addr| addr.address())
                .unwrap_or_default()
                .to_owned(),
            message.date().map(|date| date.to_timestamp()),
            message.message_id().unwrap_or_default().to_owned(),
            parse_references(message),
        ),
        None => (
            "(unparseable message)".to_owned(),
            String::new(),
            String::new(),
            None,
            String::new(),
            Vec::new(),
        ),
    };
    EnvelopeSummary {
        id,
        subject,
        from_display,
        from_addr,
        date_epoch_secs: date.unwrap_or(fallback_date_epoch_secs),
        flags,
        message_id,
        references,
    }
}

/// `References` ids oldest-first; `In-Reply-To` stands in when the
/// chain header is absent.
pub(crate) fn parse_references(message: &mail_parser::Message) -> Vec<String> {
    let references: Vec<String> = message
        .references()
        .as_text_list()
        .unwrap_or_default()
        .iter()
        .map(|id| id.as_ref().to_owned())
        .collect();
    if !references.is_empty() {
        return references;
    }
    message
        .in_reply_to()
        .as_text_list()
        .unwrap_or_default()
        .iter()
        .map(|id| id.as_ref().to_owned())
        .collect()
}
