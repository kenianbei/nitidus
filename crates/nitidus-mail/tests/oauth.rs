//! TokenRefresher behavior against an in-process scripted token
//! server: refresh-on-demand, caching, expiry, rotation persistence,
//! and denial surfacing.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use nitidus_mail::oauth::{OauthClient, TokenRefresher};
use nitidus_mail::{ExposeSecret, SecretString};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type CapturedBodies = Arc<Mutex<Vec<String>>>;

fn json_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn grant(access: &str, expires_in: u32, rotated: Option<&str>) -> String {
    let rotation = rotated
        .map(|token| format!(",\"refresh_token\":\"{token}\""))
        .unwrap_or_default();
    json_response(
        "200 OK",
        &format!(
            "{{\"access_token\":\"{access}\",\"token_type\":\"Bearer\",\"expires_in\":{expires_in}{rotation}}}"
        ),
    )
}

/// Serves one scripted response per successive connection, capturing
/// each request body.
async fn spawn_token_server(responses: Vec<String>) -> (u16, CapturedBodies) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured: CapturedBodies = Arc::default();
    let bodies = captured.clone();
    tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body = read_request_body(&mut stream).await;
            bodies.lock().unwrap().push(body);
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (port, captured)
}

async fn read_request_body(stream: &mut tokio::net::TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let n = stream.read(&mut buffer).await.unwrap();
        raw.extend_from_slice(&buffer[..n]);
        let text = String::from_utf8_lossy(&raw);
        if let Some(head_end) = text.find("\r\n\r\n") {
            let advertised: usize = text
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if raw.len() >= head_end + 4 + advertised {
                return text[head_end + 4..].to_owned();
            }
        }
    }
}

fn client(port: u16) -> OauthClient {
    OauthClient {
        token_endpoint: nitidus_mail::oauth::Url::parse(&format!("http://127.0.0.1:{port}/token"))
            .unwrap(),
        client_id: "nitidus-test".to_owned(),
        client_secret: None,
    }
}

fn refresher(port: u16, rotated: CapturedBodies) -> TokenRefresher {
    TokenRefresher::new(
        client(port),
        SecretString::from("refresh-1"),
        Box::new(move |token| {
            rotated
                .lock()
                .unwrap()
                .push(token.expose_secret().to_owned());
        }),
    )
}

#[tokio::test]
async fn refresh_fetches_once_and_serves_from_cache() {
    let (port, requests) = spawn_token_server(vec![grant("access-a", 3600, None)]).await;
    let tokens = refresher(port, Arc::default());

    let first = tokens.access_token().await.unwrap();
    let second = tokens.access_token().await.unwrap();
    assert_eq!(first.expose_secret(), "access-a");
    assert_eq!(second.expose_secret(), "access-a");

    let bodies = requests.lock().unwrap();
    assert_eq!(bodies.len(), 1, "second call must come from the cache");
    assert!(bodies[0].contains("grant_type=refresh_token"), "{bodies:?}");
    assert!(bodies[0].contains("refresh_token=refresh-1"), "{bodies:?}");
    assert!(bodies[0].contains("client_id=nitidus-test"), "{bodies:?}");
}

#[tokio::test]
async fn expired_token_refreshes_again() {
    // expires_in 60 minus the 60s staleness margin → stale immediately.
    let (port, requests) =
        spawn_token_server(vec![grant("short", 60, None), grant("fresh", 3600, None)]).await;
    let tokens = refresher(port, Arc::default());

    assert_eq!(
        tokens.access_token().await.unwrap().expose_secret(),
        "short"
    );
    assert_eq!(
        tokens.access_token().await.unwrap().expose_secret(),
        "fresh"
    );
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn rotated_refresh_token_is_persisted_and_used() {
    let (port, requests) = spawn_token_server(vec![
        grant("access-a", 3600, Some("refresh-2")),
        grant("access-b", 3600, None),
    ])
    .await;
    let rotated: CapturedBodies = Arc::default();
    let tokens = refresher(port, rotated.clone());

    tokens.access_token().await.unwrap();
    assert_eq!(rotated.lock().unwrap().as_slice(), ["refresh-2"]);

    tokens.invalidate();
    tokens.access_token().await.unwrap();
    let bodies = requests.lock().unwrap();
    assert!(
        bodies[1].contains("refresh_token=refresh-2"),
        "the rotated token must be used next: {bodies:?}"
    );
}

#[tokio::test]
async fn denied_refresh_surfaces_the_grant_error() {
    let (port, _requests) = spawn_token_server(vec![json_response(
        "400 Bad Request",
        "{\"error\":\"invalid_grant\"}",
    )])
    .await;
    let tokens = refresher(port, Arc::default());

    let error = tokens.access_token().await.unwrap_err().to_string();
    assert!(error.to_lowercase().contains("invalid"), "{error}");
}

#[tokio::test]
async fn fixed_refresher_serves_without_network_and_survives_invalidate() {
    let tokens = TokenRefresher::fixed(SecretString::from("static-token"));
    assert_eq!(
        tokens.access_token().await.unwrap().expose_secret(),
        "static-token"
    );
    tokens.invalidate();
    assert_eq!(
        tokens.access_token().await.unwrap().expose_secret(),
        "static-token"
    );
}
