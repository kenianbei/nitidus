//! Provider connection presets for the wizard: hosts, folders, and
//! OAuth defaults learned from the live Gmail and O365 smokes.

use crate::config::account::{
    AccountConfig, Backend, Encryption, ImapBackend, Outgoing, SmtpOutgoing,
};

/// Mozilla's public Thunderbird client id — the ecosystem-standard
/// registration for Microsoft tenants, consented wherever Thunderbird
/// is supported.
pub(super) const THUNDERBIRD_CLIENT_ID: &str = "9e5f94bc-e8a4-4e73-b8be-63364c29d753";

#[derive(Clone, Default)]
pub(super) struct Draft {
    pub(super) account: AccountConfig,
    /// `Some(original name)` when the form is editing rather than creating.
    pub(super) editing: Option<String>,
}

pub(super) fn apply_gmail(draft: &mut Draft) {
    draft.account.backend = Some(Backend::Imap(ImapBackend {
        host: "imap.gmail.com".to_owned(),
        ..Default::default()
    }));
    draft.account.outgoing = Some(Outgoing::Smtp(SmtpOutgoing {
        host: "smtp.gmail.com".to_owned(),
        port: 465,
        encryption: Encryption::Tls,
    }));
    let folders = &mut draft.account.folders;
    folders.drafts = "[Gmail]/Drafts".to_owned();
    folders.sent = "[Gmail]/Sent Mail".to_owned();
    folders.trash = "[Gmail]/Trash".to_owned();
    folders.archive = "[Gmail]/All Mail".to_owned();
    folders.save_sent = false;
}

pub(super) fn apply_outlook(draft: &mut Draft) {
    draft.account.backend = Some(Backend::Imap(ImapBackend {
        host: "outlook.office365.com".to_owned(),
        ..Default::default()
    }));
    draft.account.outgoing = Some(Outgoing::Smtp(SmtpOutgoing {
        host: "outlook.office365.com".to_owned(),
        ..Default::default()
    }));
    let folders = &mut draft.account.folders;
    folders.sent = "Sent Items".to_owned();
    folders.trash = "Deleted Items".to_owned();
    folders.save_sent = false;
}
