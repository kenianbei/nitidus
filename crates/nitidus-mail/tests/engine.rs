//! End-to-end engine tests over the mock backend: the full
//! command → actor → event loop, cancellation, failure, backpressure.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use nitidus_mail::mock::MockBackend;
use nitidus_mail::{
    AccountId, ConnectionState, FolderId, MailCommand, MailEngine, MailError, MailEvent,
};

const EVENT_WAIT: Duration = Duration::from_millis(20);
const EVENT_TRIES: usize = 250;

fn engine_with(backend: MockBackend) -> (MailEngine, AccountId) {
    let mut engine = MailEngine::new(1).unwrap();
    let account = AccountId::new("test");
    engine.add_account(account.clone(), backend);
    (engine, account)
}

fn wait_event(engine: &MailEngine) -> MailEvent {
    for _ in 0..EVENT_TRIES {
        if let Some(event) = engine.try_recv_event() {
            return event;
        }
        std::thread::sleep(EVENT_WAIT);
    }
    panic!(
        "no event arrived within {:?}",
        EVENT_WAIT * EVENT_TRIES as u32
    );
}

fn wait_connected(engine: &MailEngine) {
    match wait_event(engine) {
        MailEvent::Connection {
            state: ConnectionState::Connected,
            ..
        } => {}
        other => panic!("expected Connected, got {other:?}"),
    }
}

#[test]
fn actor_reports_connection_lifecycle() {
    let (engine, account) = engine_with(MockBackend::new());
    wait_connected(&engine);
    engine.send(&account, MailCommand::Shutdown).unwrap();
    match wait_event(&engine) {
        MailEvent::Connection {
            state: ConnectionState::Disconnected,
            ..
        } => {}
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

#[test]
fn lists_folders_from_backend() {
    let (engine, account) = engine_with(MockBackend::new().with_folder("INBOX", 3));
    wait_connected(&engine);
    engine.send(&account, MailCommand::ListFolders).unwrap();
    match wait_event(&engine) {
        MailEvent::Folders { folders, .. } => {
            assert_eq!(folders.len(), 1);
            assert_eq!(folders[0].name, "INBOX");
            assert_eq!(folders[0].total, 3);
        }
        other => panic!("expected Folders, got {other:?}"),
    }
}

#[test]
fn streams_envelopes_in_batches_with_terminal_event() {
    let backend = MockBackend::new()
        .with_folder("INBOX", 25)
        .with_batch_size(10);
    let (engine, account) = engine_with(backend);
    wait_connected(&engine);
    let job = engine.next_job();
    engine
        .send(
            &account,
            MailCommand::SyncEnvelopes {
                folder: FolderId::new("INBOX"),
                job,
            },
        )
        .unwrap();
    let mut total = 0;
    let mut batches = 0;
    loop {
        match wait_event(&engine) {
            MailEvent::EnvelopeBatch {
                batch,
                done,
                job: event_job,
                ..
            } => {
                assert_eq!(event_job, job);
                total += batch.len();
                if done {
                    break;
                }
                batches += 1;
            }
            other => panic!("expected EnvelopeBatch, got {other:?}"),
        }
    }
    assert_eq!(total, 25);
    assert_eq!(batches, 3, "25 envelopes in batches of 10 = 3 batches");
}

#[test]
fn cancel_stops_a_scan_mid_stream() {
    let backend = MockBackend::new()
        .with_folder("INBOX", 1000)
        .with_batch_size(10)
        .with_batch_delay(Duration::from_millis(10));
    let (engine, account) = engine_with(backend);
    wait_connected(&engine);
    let job = engine.next_job();
    engine
        .send(
            &account,
            MailCommand::SyncEnvelopes {
                folder: FolderId::new("INBOX"),
                job,
            },
        )
        .unwrap();
    let first = wait_event(&engine);
    assert!(
        matches!(first, MailEvent::EnvelopeBatch { .. }),
        "{first:?}"
    );
    engine.send(&account, MailCommand::Cancel(job)).unwrap();
    let mut seen_after_cancel = 0;
    loop {
        match wait_event(&engine) {
            MailEvent::JobFailed {
                job: Some(failed),
                error: MailError::Cancelled,
                ..
            } => {
                assert_eq!(failed, job);
                break;
            }
            MailEvent::EnvelopeBatch { done: false, .. } => {
                seen_after_cancel += 1;
                assert!(
                    seen_after_cancel < 5,
                    "scan kept streaming long after cancel"
                );
            }
            other => panic!("expected cancellation, got {other:?}"),
        }
    }
}

#[test]
fn failing_scan_reports_job_failed() {
    let backend = MockBackend::new()
        .with_folder("INBOX", 5)
        .with_failing_scan();
    let (engine, account) = engine_with(backend);
    wait_connected(&engine);
    let job = engine.next_job();
    engine
        .send(
            &account,
            MailCommand::SyncEnvelopes {
                folder: FolderId::new("INBOX"),
                job,
            },
        )
        .unwrap();
    match wait_event(&engine) {
        MailEvent::JobFailed {
            job: Some(failed),
            error: MailError::Backend(_),
            ..
        } => assert_eq!(failed, job),
        other => panic!("expected JobFailed, got {other:?}"),
    }
}

#[test]
fn slow_consumer_loses_no_envelopes() {
    let backend = MockBackend::new()
        .with_folder("INBOX", 500)
        .with_batch_size(5);
    let (engine, account) = engine_with(backend);
    wait_connected(&engine);
    let job = engine.next_job();
    engine
        .send(
            &account,
            MailCommand::SyncEnvelopes {
                folder: FolderId::new("INBOX"),
                job,
            },
        )
        .unwrap();
    let mut total = 0;
    loop {
        std::thread::sleep(Duration::from_millis(1));
        match wait_event(&engine) {
            MailEvent::EnvelopeBatch { batch, done, .. } => {
                total += batch.len();
                if done {
                    break;
                }
            }
            other => panic!("expected EnvelopeBatch, got {other:?}"),
        }
    }
    assert_eq!(total, 500);
}

#[test]
fn unknown_account_send_errors() {
    let (engine, _account) = engine_with(MockBackend::new());
    let missing = AccountId::new("missing");
    assert!(matches!(
        engine.send(&missing, MailCommand::ListFolders),
        Err(MailError::UnknownAccount(_))
    ));
}
