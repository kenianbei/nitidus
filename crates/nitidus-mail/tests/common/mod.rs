//! In-process scripted IMAP server: accepts one connection, sends the
//! greeting, then walks a list of steps — each expects a substring of
//! the next client command line and replies with its responses, `{tag}`
//! substituted with the command's tag. Deterministic, plaintext, no
//! network beyond loopback.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

pub mod smtp;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct ImapScript {
    pub greeting: String,
    pub steps: Vec<ScriptStep>,
}

pub struct ScriptStep {
    /// Substring the incoming command must contain (tag excluded).
    pub expect: String,
    /// Substring the incoming command must NOT contain.
    pub forbid: Option<String>,
    /// Response lines; `{tag}` is replaced with the client's tag.
    pub respond: Vec<String>,
}

pub fn step(expect: &str, respond: &[&str]) -> ScriptStep {
    ScriptStep {
        expect: expect.to_owned(),
        forbid: None,
        respond: respond.iter().map(|line| (*line).to_owned()).collect(),
    }
}

pub fn step_forbidding(expect: &str, forbid: &str, respond: &[&str]) -> ScriptStep {
    ScriptStep {
        forbid: Some(forbid.to_owned()),
        ..step(expect, respond)
    }
}

pub fn login_ok() -> ScriptStep {
    step("LOGIN", &["{tag} OK LOGIN completed"])
}

/// A `* {seq} FETCH` response whose BODY[HEADER.FIELDS …] literal is
/// `headers`, split on CRLF so the scripted server reproduces the
/// exact byte stream.
pub fn fetch_envelope_lines(seq: u32, uid: u32, flags: &str, headers: &str) -> Vec<String> {
    let response = format!(
        "* {seq} FETCH (UID {uid} FLAGS ({flags}) \
BODY[HEADER.FIELDS (FROM SUBJECT DATE MESSAGE-ID REFERENCES IN-REPLY-TO)] \
{{{len}}}\r\n{headers})",
        len = headers.len(),
    );
    response.split("\r\n").map(str::to_owned).collect()
}

impl ImapScript {
    pub fn new(steps: Vec<ScriptStep>) -> Self {
        Self {
            greeting: "* OK scripted server ready".to_owned(),
            steps,
        }
    }
}

/// Binds a loopback listener and serves one script per successive
/// connection (reconnect tests use several); returns the port. Panics
/// (failing the test) when a command does not match the next step.
pub async fn spawn_server(scripts: Vec<ImapScript>) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for script in scripts {
            let (stream, _) = listener.accept().await.unwrap();
            serve(stream, script).await;
        }
    });
    port
}

async fn serve(mut stream: TcpStream, script: ImapScript) {
    send_line(&mut stream, &script.greeting).await;
    let mut pending = Vec::new();
    for script_step in &script.steps {
        let line = read_line(&mut stream, &mut pending).await;
        let (tag, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        assert!(
            rest.contains(&script_step.expect),
            "scripted server expected {:?} in command {line:?}",
            script_step.expect
        );
        if let Some(forbid) = &script_step.forbid {
            assert!(
                !rest.contains(forbid),
                "scripted server forbids {forbid:?} in command {line:?}"
            );
        }
        for response in &script_step.respond {
            send_line(&mut stream, &response.replace("{tag}", tag)).await;
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
        assert!(n > 0, "client closed mid-script");
        pending.extend_from_slice(&buffer[..n]);
    }
}
