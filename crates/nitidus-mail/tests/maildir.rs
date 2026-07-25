//! Maildir backend tests over real tempdir fixtures: discovery,
//! scanning, flag renames, fetch, and change watching.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nitidus_mail::maildir::MaildirBackend;
use nitidus_mail::{
    AccountId, EnvelopeId, Flags, FolderId, MailBackend, MailCommand, MailEngine, MailError,
    MailEvent,
};

fn make_maildir(dir: &Path) {
    for sub in ["cur", "new", "tmp"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
}

fn write_message(dir: &Path, sub: &str, name: &str, subject: &str) -> PathBuf {
    let path = dir.join(sub).join(name);
    let body = format!(
        "From: Alice Example <alice@example.com>\r\nSubject: {subject}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\nMessage-ID: <{name}@test>\r\n\r\nbody text\r\n"
    );
    fs::write(&path, body).unwrap();
    path
}

fn fixture_root() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    make_maildir(&tmp.path().join(".Archive.2024"));
    make_maildir(&tmp.path().join("Drafts"));
    fs::create_dir_all(tmp.path().join("not-a-folder")).unwrap();
    tmp
}

#[tokio::test]
async fn discovers_folders_across_layouts() {
    let tmp = fixture_root();
    write_message(tmp.path(), "new", "m1.host", "hello");
    write_message(tmp.path(), "cur", "m2.host:2,S", "seen");
    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    let folders = backend.list_folders().await.unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["Archive/2024", "Drafts", "INBOX"]);
    let inbox = folders.iter().find(|f| f.name == "INBOX").unwrap();
    assert_eq!(inbox.unread, 1);
    assert_eq!(inbox.total, 2);
}

#[tokio::test]
async fn rejects_non_maildir_root() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        MaildirBackend::new(tmp.path().to_path_buf()),
        Err(MailError::Backend(_))
    ));
}

#[tokio::test]
async fn scans_envelopes_with_flags_and_headers() {
    let tmp = fixture_root();
    write_message(tmp.path(), "new", "fresh.host", "unread one");
    write_message(tmp.path(), "cur", "old.host:2,FS", "flagged seen");
    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    let (tx, rx) = flume::unbounded();
    backend
        .scan_envelopes(&FolderId::new("INBOX"), tx)
        .await
        .unwrap();
    let mut envelopes: Vec<_> = rx.drain().flatten().collect();
    envelopes.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    assert_eq!(envelopes.len(), 2);

    let fresh = &envelopes[0];
    assert_eq!(fresh.id.as_str(), "fresh.host");
    assert_eq!(fresh.subject, "unread one");
    assert_eq!(fresh.from_addr, "alice@example.com");
    assert_eq!(fresh.from_display, "Alice Example");
    assert!(!fresh.flags.contains(Flags::SEEN));
    assert_eq!(fresh.date_epoch_secs, 1_707_998_400);

    let old = &envelopes[1];
    assert!(old.flags.contains(Flags::SEEN));
    assert!(old.flags.contains(Flags::FLAGGED));
}

#[tokio::test]
async fn set_flags_renames_and_moves_out_of_new() {
    let tmp = fixture_root();
    write_message(tmp.path(), "new", "movable.host", "to be seen");
    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    let id = EnvelopeId::new("movable.host");
    backend
        .set_flags(
            &FolderId::new("INBOX"),
            &id,
            Flags::default().with(Flags::SEEN),
        )
        .await
        .unwrap();
    assert!(tmp.path().join("cur/movable.host:2,S").exists());
    assert!(!tmp.path().join("new/movable.host").exists());

    let raw = backend
        .fetch_message(&FolderId::new("INBOX"), &id)
        .await
        .unwrap();
    assert!(String::from_utf8(raw).unwrap().contains("to be seen"));
}

#[tokio::test]
async fn dropped_receiver_cancels_scan() {
    let tmp = fixture_root();
    for index in 0..10 {
        write_message(tmp.path(), "cur", &format!("m{index}.host:2,S"), "x");
    }
    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    let (tx, rx) = flume::bounded(0);
    drop(rx);
    assert!(matches!(
        backend.scan_envelopes(&FolderId::new("INBOX"), tx).await,
        Err(MailError::Cancelled)
    ));
}

#[test]
fn watcher_emits_one_folder_changed_per_burst() {
    let tmp = fixture_root();
    let mut engine = MailEngine::new(1).unwrap();
    let account = AccountId::new("local");
    engine.add_account(
        account.clone(),
        MaildirBackend::new(tmp.path().to_path_buf()).unwrap(),
    );
    engine.watch_maildir(account, tmp.path().to_path_buf());
    std::thread::sleep(Duration::from_millis(300));

    write_message(tmp.path(), "new", "delivered-1.host", "one");
    write_message(tmp.path(), "new", "delivered-2.host", "two");

    let mut changed = Vec::new();
    for _ in 0..300 {
        match engine.try_recv_event() {
            Some(MailEvent::FolderChanged { folder, .. }) => changed.push(folder),
            Some(_other) => {}
            None => std::thread::sleep(Duration::from_millis(20)),
        }
        if !changed.is_empty() {
            std::thread::sleep(Duration::from_millis(200));
            while let Some(event) = engine.try_recv_event() {
                if let MailEvent::FolderChanged { folder, .. } = event {
                    changed.push(folder);
                }
            }
            break;
        }
    }
    assert_eq!(
        changed,
        vec![FolderId::new("INBOX")],
        "two writes in one burst must coalesce to one INBOX event"
    );
    let _keep_alive = engine.send(&AccountId::new("local"), MailCommand::ListFolders);
}

#[tokio::test]
async fn append_message_delivers_into_cur_with_flags() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    let sent_dir = tmp.path().join(".Sent");
    make_maildir(&sent_dir);

    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    backend
        .append_message(
            &FolderId::new(".Sent"),
            b"From: me@x.com\r\nSubject: sent copy\r\n\r\nhello\r\n".to_vec(),
            Flags::SEEN,
        )
        .await
        .unwrap();

    let files: Vec<_> = fs::read_dir(sent_dir.join("cur"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().into_string().unwrap();
    assert!(name.ends_with(":2,S"), "seen flag expected: {name}");
    assert!(
        fs::read_dir(sent_dir.join("tmp")).unwrap().next().is_none(),
        "tmp must be empty after delivery"
    );
    let (batch_tx, batch_rx) = flume::unbounded();
    backend
        .scan_envelopes(&FolderId::new(".Sent"), batch_tx)
        .await
        .unwrap();
    let envelopes: Vec<_> = batch_rx.drain().flatten().collect();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].subject, "sent copy");
    assert!(envelopes[0].flags.contains(Flags::SEEN));
}

#[tokio::test]
async fn delete_message_removes_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    make_maildir(tmp.path());
    write_message(tmp.path(), "cur", "victim.host:2,S", "goes away");

    let mut backend = MaildirBackend::new(tmp.path().to_path_buf()).unwrap();
    backend
        .delete_message(&FolderId::new("INBOX"), &EnvelopeId::new("victim.host"))
        .await
        .unwrap();
    assert!(
        fs::read_dir(tmp.path().join("cur"))
            .unwrap()
            .next()
            .is_none(),
        "the message file must be gone"
    );
    assert!(
        backend
            .delete_message(&FolderId::new("INBOX"), &EnvelopeId::new("victim.host"))
            .await
            .is_err(),
        "deleting a missing message errors"
    );
}
