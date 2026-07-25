//! In-process scripted SMTP server: line-oriented steps, plus a DATA
//! step that swallows the message until the dot terminator and hands
//! the collected bytes back for assertions.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::sync::Arc;
use std::sync::Mutex;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub enum SmtpStep {
    /// Expect one command line containing `expect`; reply with the
    /// given lines.
    Expect {
        expect: &'static str,
        respond: &'static [&'static str],
    },
    /// Consume message bytes until the `.` terminator, then reply.
    Data { respond: &'static str },
}

pub fn expect(expect: &'static str, respond: &'static [&'static str]) -> SmtpStep {
    SmtpStep::Expect { expect, respond }
}

/// Captured DATA payloads, for message-content assertions.
#[derive(Clone, Default)]
pub struct CapturedMessages(pub Arc<Mutex<Vec<String>>>);

pub async fn spawn_smtp_server(steps: Vec<SmtpStep>) -> (u16, CapturedMessages) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured = CapturedMessages::default();
    let capture = captured.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        serve(stream, steps, capture).await;
    });
    (port, captured)
}

async fn serve(mut stream: TcpStream, steps: Vec<SmtpStep>, captured: CapturedMessages) {
    send_line(&mut stream, "220 fake ESMTP ready").await;
    let mut pending = Vec::new();
    for step in &steps {
        match step {
            SmtpStep::Expect { expect, respond } => {
                let line = read_line(&mut stream, &mut pending).await;
                assert!(
                    line.contains(expect),
                    "smtp server expected {expect:?} in {line:?}"
                );
                for response in *respond {
                    send_line(&mut stream, response).await;
                }
            }
            SmtpStep::Data { respond } => {
                let mut message = Vec::new();
                loop {
                    let line = read_line(&mut stream, &mut pending).await;
                    if line == "." {
                        break;
                    }
                    message.extend_from_slice(line.as_bytes());
                    message.extend_from_slice(b"\r\n");
                }
                captured
                    .0
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&message).into_owned());
                send_line(&mut stream, respond).await;
            }
        }
    }
}

async fn send_line(stream: &mut TcpStream, line: &str) {
    stream
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .unwrap();
}

async fn read_line(stream: &mut TcpStream, pending: &mut Vec<u8>) -> String {
    loop {
        if let Some(position) = pending.windows(2).position(|window| window == b"\r\n") {
            let line: Vec<u8> = pending.drain(..position + 2).collect();
            return String::from_utf8_lossy(&line[..line.len() - 2]).into_owned();
        }
        let mut buffer = [0u8; 4096];
        let n = stream.read(&mut buffer).await.unwrap();
        assert!(n > 0, "smtp client closed mid-script");
        pending.extend_from_slice(&buffer[..n]);
    }
}
