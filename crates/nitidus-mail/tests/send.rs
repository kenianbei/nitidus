//! Outgoing transmission through the engine: SMTP happy path against
//! the scripted server, rejection and auth failures, and the sendmail
//! pipe.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::smtp::{CapturedMessages, SmtpStep, expect, spawn_smtp_server};
use nitidus_mail::send::{
    OutgoingTransport, SendEnvelope, SmtpConfig, SmtpCredentials, SmtpEncryption,
};
use nitidus_mail::{AccountId, JobId, MailEngine, MailEvent};

const EVENT_WAIT: Duration = Duration::from_millis(20);
const EVENT_TRIES: usize = 250;

fn wait_send_result(engine: &MailEngine) -> Result<JobId, String> {
    for _ in 0..EVENT_TRIES {
        match engine.try_recv_event() {
            Some(MailEvent::SendDone { job, .. }) => return Ok(job),
            Some(MailEvent::JobFailed { error, .. }) => return Err(error.to_string()),
            Some(_) | None => std::thread::sleep(EVENT_WAIT),
        }
    }
    panic!("no send outcome arrived");
}

/// The fake server needs a runtime of its own: `MailEngine` owns one
/// too, and dropping it inside an async test context panics.
fn start_server(steps: Vec<SmtpStep>) -> (tokio::runtime::Runtime, u16, CapturedMessages) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let (port, captured) = runtime.block_on(spawn_smtp_server(steps));
    (runtime, port, captured)
}

fn smtp_transport(port: u16, with_auth: bool) -> OutgoingTransport {
    OutgoingTransport::Smtp(SmtpConfig {
        host: "127.0.0.1".to_owned(),
        port,
        encryption: SmtpEncryption::None,
        credentials: with_auth.then(|| SmtpCredentials {
            user: "norman@example.com".to_owned(),
            password: nitidus_mail::SecretString::from("hunter2"),
        }),
    })
}

fn envelope() -> SendEnvelope {
    SendEnvelope {
        from: "norman@example.com".to_owned(),
        recipients: vec!["bob@example.com".to_owned(), "cc@example.com".to_owned()],
    }
}

fn message() -> Vec<u8> {
    b"From: norman@example.com\r\nTo: bob@example.com\r\nSubject: hi\r\n\r\nhello\r\n".to_vec()
}

#[test]
fn smtp_happy_path_delivers_and_reports_send_done() {
    let (_server, port, captured) = start_server(vec![
        expect("EHLO", &["250-fake.example", "250 AUTH PLAIN LOGIN"]),
        expect("AUTH PLAIN", &["235 accepted"]),
        expect("MAIL FROM:<norman@example.com>", &["250 ok"]),
        expect("RCPT TO:<bob@example.com>", &["250 ok"]),
        expect("RCPT TO:<cc@example.com>", &["250 ok"]),
        expect("DATA", &["354 go ahead"]),
        SmtpStep::Data {
            respond: "250 queued",
        },
        expect("QUIT", &["221 bye"]),
    ]);

    let engine = MailEngine::new(1).unwrap();
    let job = engine.next_job();
    engine.submit(
        AccountId::new("test"),
        smtp_transport(port, true),
        envelope(),
        message(),
        job,
    );
    assert_eq!(wait_send_result(&engine), Ok(job));
    let messages = captured.0.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Subject: hi"), "{}", messages[0]);
}

#[test]
fn rejected_recipient_surfaces_job_failed() {
    let (_server, port, _captured) = start_server(vec![
        expect("EHLO", &["250 fake.example"]),
        expect("MAIL FROM", &["250 ok"]),
        expect("RCPT TO", &["550 no such user"]),
    ]);

    let engine = MailEngine::new(1).unwrap();
    let job = engine.next_job();
    engine.submit(
        AccountId::new("test"),
        smtp_transport(port, false),
        envelope(),
        message(),
        job,
    );
    let error = wait_send_result(&engine).unwrap_err();
    assert!(
        error.contains("550") || error.to_lowercase().contains("rcpt"),
        "{error}"
    );
}

#[test]
fn auth_failure_names_the_user() {
    let (_server, port, _captured) = start_server(vec![
        expect("EHLO", &["250-fake.example", "250 AUTH PLAIN"]),
        expect("AUTH PLAIN", &["535 bad credentials"]),
    ]);

    let engine = MailEngine::new(1).unwrap();
    let job = engine.next_job();
    engine.submit(
        AccountId::new("test"),
        smtp_transport(port, true),
        envelope(),
        message(),
        job,
    );
    let error = wait_send_result(&engine).unwrap_err();
    assert!(error.contains("norman@example.com"), "{error}");
}

#[test]
fn sendmail_pipe_receives_recipients_and_message() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("fake-sendmail.sh");
    let out = dir.path().join("out");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s ' \"$@\" > '{args}'\ncat > '{msg}'\n",
            args = out.with_extension("args").display(),
            msg = out.with_extension("msg").display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let engine = MailEngine::new(1).unwrap();
    let job = engine.next_job();
    engine.submit(
        AccountId::new("test"),
        OutgoingTransport::Sendmail {
            command: script.display().to_string(),
        },
        envelope(),
        message(),
        job,
    );
    assert_eq!(wait_send_result(&engine), Ok(job));
    let args = std::fs::read_to_string(out.with_extension("args")).unwrap();
    assert!(
        args.contains("bob@example.com") && args.contains("cc@example.com"),
        "{args}"
    );
    let body = std::fs::read_to_string(out.with_extension("msg")).unwrap();
    assert!(body.contains("Subject: hi"), "{body}");
}

#[test]
fn failing_sendmail_command_reports_its_exit() {
    let engine = MailEngine::new(1).unwrap();
    let job = engine.next_job();
    engine.submit(
        AccountId::new("test"),
        OutgoingTransport::Sendmail {
            command: "exit 3".to_owned(),
        },
        envelope(),
        message(),
        job,
    );
    let error = wait_send_result(&engine).unwrap_err();
    assert!(error.contains("exit"), "{error}");
}
