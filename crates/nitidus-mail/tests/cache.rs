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
        message_id: format!("{id}@example"),
        references: Vec::new(),
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
                vec![
                    envelope("kept", 100, Flags::default()),
                    envelope("gone", 200, Flags::default()),
                ],
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
fn v1_database_migrates_in_place_preserving_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE folders (
                     account TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL,
                     unread INTEGER NOT NULL, total INTEGER NOT NULL,
                     PRIMARY KEY (account, id)) STRICT;
                 CREATE TABLE envelopes (
                     account TEXT NOT NULL, folder TEXT NOT NULL, id TEXT NOT NULL,
                     subject TEXT NOT NULL, from_display TEXT NOT NULL,
                     from_addr TEXT NOT NULL, date_epoch_secs INTEGER NOT NULL,
                     flags INTEGER NOT NULL, seen_job INTEGER NOT NULL,
                     PRIMARY KEY (account, folder, id)) STRICT;
                 CREATE INDEX envelopes_by_folder_date
                     ON envelopes (account, folder, date_epoch_secs DESC);
                 INSERT INTO envelopes VALUES
                     ('local', 'INBOX', 'kept', 'survived', 'A', 'a@x', 42, 1, 1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    }

    let cache = MailCache::open(&path).unwrap();
    let envelopes = cache
        .load_envelopes(&AccountId::new("local"), &FolderId::new("INBOX"))
        .unwrap();
    assert_eq!(envelopes.len(), 1, "v1 rows must survive the migration");
    assert_eq!(envelopes[0].subject, "survived");
    assert_eq!(envelopes[0].message_id, "");
    assert!(envelopes[0].references.is_empty());
}

#[test]
fn references_roundtrip_through_the_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let account = AccountId::new("local");
    let inbox = FolderId::new("INBOX");
    let mut reply = envelope("reply", 100, Flags::default());
    reply.message_id = "reply@x".to_owned();
    reply.references = vec!["root@x".to_owned(), "mid@x".to_owned()];
    record_all(
        &path,
        &[batch_event(&account, &inbox, 1, vec![reply], true)],
    );

    let cache = MailCache::open(&path).unwrap();
    let envelopes = cache.load_envelopes(&account, &inbox).unwrap();
    assert_eq!(envelopes[0].message_id, "reply@x");
    assert_eq!(envelopes[0].references, vec!["root@x", "mid@x"]);
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

#[test]
fn purge_account_drops_only_that_accounts_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let doomed = AccountId::new("doomed");
    let kept = AccountId::new("kept");
    let inbox = FolderId::new("INBOX");
    let writer = MailCache::open(&path).unwrap().into_writer();
    for account in [&doomed, &kept] {
        writer.record(&folders_event(account, &["INBOX"]));
        writer.record(&batch_event(
            account,
            &inbox,
            1,
            vec![envelope("kept-mail", 100, Flags::default())],
            true,
        ));
    }
    writer.purge_account(&doomed);
    writer.close();

    let cache = MailCache::open(&path).unwrap();
    assert!(cache.load_folders(&doomed).unwrap().is_empty());
    assert!(cache.load_envelopes(&doomed, &inbox).unwrap().is_empty());
    assert_eq!(cache.load_folders(&kept).unwrap().len(), 1);
    assert_eq!(cache.load_envelopes(&kept, &inbox).unwrap().len(), 1);
}

#[test]
fn harvest_accumulates_uses_and_fills_display_names() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mail.db");
    let entry = |display: &str, uses: u32, seen: i64| nitidus_mail::cache::HarvestedAddress {
        addr: "kj@nasa.example".to_owned(),
        display: display.to_owned(),
        uses,
        last_seen_epoch: seen,
    };

    let writer = MailCache::open(&path).unwrap().into_writer();
    writer.harvest(vec![entry("", 1, 100)]);
    writer.harvest(vec![entry("Katherine Johnson", 2, 50)]);
    writer.close();

    let loaded = MailCache::open(&path).unwrap().load_addresses().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].addr, "kj@nasa.example");
    assert_eq!(
        loaded[0].display, "Katherine Johnson",
        "a later display name fills the blank"
    );
    assert_eq!(loaded[0].uses, 3, "uses accumulate across harvests");
    assert_eq!(
        loaded[0].last_seen_epoch, 100,
        "the newest sighting wins even when harvested later"
    );
}
