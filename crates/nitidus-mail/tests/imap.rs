//! IMAP backend behavior against the in-process scripted server:
//! folder listing, full and incremental scans, message fetch, flag
//! writes, folder ops, auth failure, and reconnect-once.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{ImapScript, fetch_envelope_lines, login_ok, spawn_server, step};
use nitidus_mail::imap::{ImapBackend, ImapConfig, ImapEncryption};
use nitidus_mail::{EnvelopeId, EnvelopeSummary, Flags, FolderId, MailBackend, MailError};

fn config(port: u16) -> ImapConfig {
    ImapConfig {
        host: "127.0.0.1".to_owned(),
        port,
        encryption: ImapEncryption::None,
        user: "norman@example.com".to_owned(),
        password: "hunter2".to_owned(),
    }
}

fn alice_headers(subject: &str, message_id: &str) -> String {
    format!(
        "From: Alice <alice@x.com>\r\nSubject: {subject}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\nMessage-ID: <{message_id}>\r\n\r\n"
    )
}

fn select_lines(exists: u32, uid_validity: u32, mod_seq: u64) -> Vec<&'static str> {
    // Leaked to 'static for script convenience; tests only.
    let lines = vec![
        format!("* {exists} EXISTS"),
        format!("* OK [UIDVALIDITY {uid_validity}] UIDs valid"),
        format!("* OK [HIGHESTMODSEQ {mod_seq}] modseq"),
        "{tag} OK [READ-WRITE] SELECT completed".to_owned(),
    ];
    lines
        .into_iter()
        .map(|line| Box::leak(line.into_boxed_str()) as &'static str)
        .collect()
}

async fn scan(
    backend: &mut ImapBackend,
    folder: &FolderId,
) -> Result<Vec<EnvelopeSummary>, MailError> {
    let (batch_tx, batch_rx) = flume::unbounded();
    backend.scan_envelopes(folder, batch_tx).await?;
    Ok(batch_rx.drain().flatten().collect())
}

#[tokio::test]
async fn lists_folders_skipping_noselect_with_status_counts() {
    let port = spawn_server(vec![ImapScript::new(vec![
        login_ok(),
        step(
            "LIST",
            &[
                "* LIST (\\HasNoChildren) \"/\" \"Work\"",
                "* LIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\"",
                "* LIST (\\HasNoChildren) \"/\" \"[Gmail]/Sent Mail\"",
                "* LIST (\\HasNoChildren) \"/\" \"INBOX\"",
                "{tag} OK LIST completed",
            ],
        ),
        step(
            "STATUS",
            &[
                "* STATUS \"INBOX\" (MESSAGES 4 UNSEEN 2)",
                "{tag} OK STATUS",
            ],
        ),
        step(
            "STATUS",
            &["* STATUS \"Work\" (MESSAGES 1 UNSEEN 1)", "{tag} OK STATUS"],
        ),
        step(
            "STATUS",
            &[
                "* STATUS \"[Gmail]/Sent Mail\" (MESSAGES 9 UNSEEN 0)",
                "{tag} OK STATUS",
            ],
        ),
    ])])
    .await;

    let mut backend = ImapBackend::new(config(port));
    let folders = backend.list_folders().await.unwrap();
    let names: Vec<&str> = folders.iter().map(|meta| meta.name.as_str()).collect();
    assert_eq!(names, vec!["INBOX", "Work", "[Gmail]/Sent Mail"]);
    assert_eq!(folders[0].unread, 2);
    assert_eq!(folders[0].total, 4);
    assert_eq!(folders[1].unread, 1);
    assert_eq!(folders[2].total, 9);
}

#[tokio::test]
async fn full_scan_streams_envelopes_and_rescan_is_incremental() {
    let mut first_scan = vec![login_ok(), step("SELECT", &select_lines(2, 99, 10))];
    let mut fetch_lines: Vec<String> = Vec::new();
    fetch_lines.extend(fetch_envelope_lines(
        1,
        101,
        "\\Seen",
        &alice_headers("first", "m1@x"),
    ));
    fetch_lines.extend(fetch_envelope_lines(
        2,
        102,
        "",
        &alice_headers("second", "m2@x"),
    ));
    fetch_lines.push("{tag} OK FETCH completed".to_owned());
    let fetch_refs: Vec<&str> = fetch_lines.iter().map(String::as_str).collect();
    first_scan.push(step("FETCH 1:2", &fetch_refs));

    // Re-scan: SELECT again, flag delta marks 102 seen, no new UIDs
    // (the 103:* fetch legitimately re-answers with the last message),
    // SEARCH still lists both.
    let flag_delta_line = "* 2 FETCH (UID 102 FLAGS (\\Seen))";
    let second_scan = vec![
        step("SELECT", &select_lines(2, 99, 12)),
        step(
            "CHANGEDSINCE",
            &[flag_delta_line, "{tag} OK FETCH completed"],
        ),
        step("UID FETCH 103:*", &["{tag} OK FETCH completed"]),
        step(
            "UID SEARCH",
            &["* SEARCH 101 102", "{tag} OK SEARCH completed"],
        ),
    ];

    let mut steps = first_scan;
    steps.extend(second_scan);
    let port = spawn_server(vec![ImapScript::new(steps)]).await;

    let mut backend = ImapBackend::new(config(port));
    let inbox = FolderId::new("INBOX");

    let envelopes = scan(&mut backend, &inbox).await.unwrap();
    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].id, EnvelopeId::new("101"));
    assert_eq!(envelopes[0].subject, "first");
    assert!(envelopes[0].flags.contains(Flags::SEEN));
    assert_eq!(envelopes[1].message_id, "m2@x");
    assert!(!envelopes[1].flags.contains(Flags::SEEN));

    let rescanned = scan(&mut backend, &inbox).await.unwrap();
    assert_eq!(rescanned.len(), 2);
    assert!(
        rescanned[1].flags.contains(Flags::SEEN),
        "CHANGEDSINCE flag delta must apply to the cached envelope"
    );
}

#[tokio::test]
async fn uidvalidity_change_forces_a_full_refetch() {
    let mut steps = vec![login_ok(), step("SELECT", &select_lines(1, 7, 5))];
    let mut fetch = fetch_envelope_lines(1, 50, "", &alice_headers("old", "old@x"));
    fetch.push("{tag} OK FETCH completed".to_owned());
    let fetch_refs: Vec<&str> = fetch.iter().map(String::as_str).collect();
    steps.push(step("FETCH 1 ", &fetch_refs));

    steps.push(step("SELECT", &select_lines(1, 8, 5)));
    let mut refetch = fetch_envelope_lines(1, 1, "", &alice_headers("new", "new@x"));
    refetch.push("{tag} OK FETCH completed".to_owned());
    let refetch_refs: Vec<&str> = refetch.iter().map(String::as_str).collect();
    steps.push(step("FETCH 1 ", &refetch_refs));

    let port = spawn_server(vec![ImapScript::new(steps)]).await;
    let mut backend = ImapBackend::new(config(port));
    let inbox = FolderId::new("INBOX");

    let first = scan(&mut backend, &inbox).await.unwrap();
    assert_eq!(first[0].id, EnvelopeId::new("50"));
    let second = scan(&mut backend, &inbox).await.unwrap();
    assert_eq!(
        second[0].id,
        EnvelopeId::new("1"),
        "a UIDVALIDITY bump must discard session state and refetch"
    );
}

#[tokio::test]
async fn fetches_message_body_and_stores_flags() {
    let body = "Subject: hi\r\n\r\nbody text\r\n";
    let fetch_body = format!(
        "* 1 FETCH (UID 44 BODY[] {{{len}}}\r\n{body})",
        len = body.len()
    );
    let mut body_lines: Vec<String> = fetch_body.split("\r\n").map(str::to_owned).collect();
    body_lines.push("{tag} OK FETCH completed".to_owned());
    let body_refs: Vec<&str> = body_lines.iter().map(String::as_str).collect();

    let port = spawn_server(vec![ImapScript::new(vec![
        login_ok(),
        step("SELECT", &select_lines(1, 3, 1)),
        step("UID FETCH 44", &body_refs),
        step(
            "UID STORE 44 FLAGS.SILENT (\\Seen \\Flagged)",
            &["{tag} OK STORE completed"],
        ),
    ])])
    .await;

    let mut backend = ImapBackend::new(config(port));
    let inbox = FolderId::new("INBOX");
    let raw = backend
        .fetch_message(&inbox, &EnvelopeId::new("44"))
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&raw), body);

    backend
        .set_flags(
            &inbox,
            &EnvelopeId::new("44"),
            Flags::SEEN.with(Flags::FLAGGED),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn folder_ops_create_rename_and_guarded_delete() {
    let port = spawn_server(vec![ImapScript::new(vec![
        login_ok(),
        step("CREATE", &["{tag} OK CREATE completed"]),
        step("RENAME", &["{tag} OK RENAME completed"]),
        step("SELECT", &select_lines(3, 5, 1)),
    ])])
    .await;

    let mut backend = ImapBackend::new(config(port));
    backend.create_folder("Projects/nitidus").await.unwrap();
    backend
        .rename_folder(&FolderId::new("Projects"), "Archive")
        .await
        .unwrap();

    let refusal = backend.delete_folder(&FolderId::new("Full")).await;
    assert!(
        refusal.unwrap_err().to_string().contains("not empty"),
        "delete must refuse a folder with messages"
    );
    assert!(
        backend
            .delete_folder(&FolderId::new("INBOX"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn login_failure_surfaces_a_backend_error() {
    let port = spawn_server(vec![ImapScript::new(vec![step(
        "LOGIN",
        &["{tag} NO [AUTHENTICATIONFAILED] bad credentials"],
    )])])
    .await;

    let mut backend = ImapBackend::new(config(port));
    let error = backend.list_folders().await.unwrap_err().to_string();
    assert!(error.to_lowercase().contains("login"), "{error}");
}

#[tokio::test]
async fn transport_failure_reconnects_and_retries_once() {
    let first = ImapScript::new(vec![login_ok()]);
    let second = ImapScript::new(vec![
        login_ok(),
        step(
            "LIST",
            &[
                "* LIST (\\HasNoChildren) \"/\" \"INBOX\"",
                "{tag} OK LIST completed",
            ],
        ),
        step(
            "STATUS",
            &[
                "* STATUS \"INBOX\" (MESSAGES 0 UNSEEN 0)",
                "{tag} OK STATUS",
            ],
        ),
    ]);
    let port = spawn_server(vec![first, second]).await;

    let mut backend = ImapBackend::new(config(port));
    let folders = backend.list_folders().await.unwrap();
    assert_eq!(
        folders.len(),
        1,
        "LIST must succeed on the second connection"
    );
}

#[tokio::test]
async fn append_message_runs_the_append_command() {
    let body = "From: me@x.com\r\n\r\nsent\r\n";
    let port = spawn_server(vec![ImapScript::new(vec![
        login_ok(),
        step(
            "APPEND \"[Gmail]/Sent Mail\" (\\Seen)",
            &["+ go ahead", "{tag} OK APPEND completed"],
        ),
    ])])
    .await;

    let mut backend = ImapBackend::new(config(port));
    backend
        .append_message(
            &FolderId::new("[Gmail]/Sent Mail"),
            body.as_bytes().to_vec(),
            Flags::SEEN,
        )
        .await
        .unwrap();
}
