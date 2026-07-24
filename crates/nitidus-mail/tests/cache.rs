//! Envelope cache tests over real tempdir databases: warm-read
//! roundtrips, stale-row pruning, folder replacement, and downgrade
//! refusal.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use nitidus_mail::cache::{CacheError, MailCache};
use nitidus_mail::{
    AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta, JobId, MailEvent,
};

fn envelope(id: &str, date: i64, flags: Flags) -> EnvelopeSummary {
    EnvelopeSummary {
        id: EnvelopeId::new(id),
        subject: format!("subject {id}"),
        from_display: "Alice Example".to_owned(),
        from_addr: "alice@example.com".to_owned(),
        date_epoch_secs: date,
        flags,
    }
}

fn folders_event(account: &AccountId, names: &[&str]) -> MailEvent {
    MailEvent::Folders {
        account: account.clone(),
        folders: names
            .iter()
            .map(|name| FolderMeta {
                id: FolderId::new(*name),
                name: (*name).to_owned(),
                unread: 1,
                total: 2,
            })
            .collect(),
    }
}

fn batch_event(
    account: &AccountId,
    folder: &FolderId,
    job: u64,
    batch: Vec<EnvelopeSummary>,
    done: bool,
) -> MailEvent {
    MailEvent::EnvelopeBatch {
        account: account.clone(),
        folder: folder.clone(),
        job: JobId(job),
        batch,
        done,
    }
}

fn record_all(path: &Path, events: &[MailEvent]) {
    let writer = MailCache::open(path).unwrap().into_writer();
    for event in events {
        writer.record(event);
    }
    writer.close();
}

#[test]
fn roundtrip_preserves_folders_envelopes_and_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let account = AccountId::new("local");
    let inbox = FolderId::new("INBOX");
    record_all(
        &path,
        &[
            folders_event(&account, &["INBOX"]),
            batch_event(
                &account,
                &inbox,
                1,
                vec![
                    envelope("older", 100, Flags::default().with(Flags::SEEN)),
                    envelope("newer", 200, Flags::default()),
                ],
                true,
            ),
        ],
    );

    let cache = MailCache::open(&path).unwrap();
    let folders = cache.load_folders(&account).unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "INBOX");
    assert_eq!(folders[0].unread, 1);

    let envelopes = cache.load_envelopes(&account, &inbox).unwrap();
    let ids: Vec<&str> = envelopes.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["newer", "older"], "must load date-descending");
    assert!(envelopes[1].flags.contains(Flags::SEEN));
    assert!(!envelopes[0].flags.contains(Flags::SEEN));
    assert_eq!(envelopes[1].subject, "subject older");
    assert_eq!(envelopes[1].from_addr, "alice@example.com");
}

#[test]
fn done_batch_prunes_rows_from_earlier_scans() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let account = AccountId::new("local");
    let inbox = FolderId::new("INBOX");
    record_all(
        &path,
        &[
            batch_event(
                &account,
                &inbox,
                1,
                vec![envelope("kept", 100, Flags::default()), envelope("gone", 200, Flags::default())],
                true,
            ),
            batch_event(
                &account,
                &inbox,
                2,
                vec![envelope("kept", 100, Flags::default())],
                true,
            ),
        ],
    );

    let cache = MailCache::open(&path).unwrap();
    let envelopes = cache.load_envelopes(&account, &inbox).unwrap();
    let ids: Vec<&str> = envelopes.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["kept"], "rescan must remove rows it did not see");
}

#[test]
fn folder_replacement_drops_removed_folders_and_their_envelopes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let account = AccountId::new("local");
    let archive = FolderId::new("Archive");
    record_all(
        &path,
        &[
            folders_event(&account, &["INBOX", "Archive"]),
            batch_event(
                &account,
                &archive,
                1,
                vec![envelope("archived", 100, Flags::default())],
                true,
            ),
            folders_event(&account, &["INBOX"]),
        ],
    );

    let cache = MailCache::open(&path).unwrap();
    let names: Vec<String> = cache
        .load_folders(&account)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, vec!["INBOX"]);
    assert!(cache.load_envelopes(&account, &archive).unwrap().is_empty());
}

#[test]
fn newer_schema_is_refused_not_migrated() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
    }
    assert!(matches!(
        MailCache::open(&path),
        Err(CacheError::NewerSchema)
    ));
}

#[test]
fn open_rejects_unreadable_database_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    std::fs::write(&path, "this is not a sqlite database, not even close").unwrap();
    assert!(matches!(
        MailCache::open(&path),
        Err(CacheError::Database(_))
    ));
}
