//! `:authorize` — interactive OAuth2 grants for the active account,
//! run on the mail runtime and reported back through a channel drained
//! each frame.

use std::sync::{Mutex, mpsc};

use bevy::prelude::*;
use nitidus_mail::SecretString;
use nitidus_mail::oauth::Url;
use nitidus_mail::oauth::device::{DeviceFlowConfig, DevicePrompt, authorize_device};
use nitidus_mail::oauth::grant::{CodeFlow, CodeFlowConfig, Grant};

use super::active_account;
use crate::config::account::{AccountConfig, Auth, Oauth2Flow};
use crate::config::{keyring, presets};
use crate::engine::EngineResource;
use crate::status::StatusMessage;

pub enum OauthEvent {
    BrowserPrompt(String),
    DevicePrompt(DevicePrompt),
    Granted {
        account: String,
        refresh_token: SecretString,
    },
    Failed {
        account: String,
        error: String,
    },
}

#[derive(Resource)]
pub struct OauthChannel {
    sender: mpsc::Sender<OauthEvent>,
    receiver: Mutex<mpsc::Receiver<OauthEvent>>,
}

impl Default for OauthChannel {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Mutex::new(receiver),
        }
    }
}

pub(crate) enum FlowSpec {
    Code(CodeFlowConfig),
    Device(DeviceFlowConfig),
}

/// `:authorize` — starts the provider's grant flow for the active
/// account; progress and the outcome arrive as [`OauthEvent`]s.
pub fn authorize(world: &mut World) {
    let Some(account) = active_account(world) else {
        return;
    };
    let spec = {
        let config = world.resource::<crate::config::Config>();
        config
            .accounts
            .iter()
            .find(|candidate| candidate.name == account)
            .map(build_flow)
    };
    let now = world.resource::<Time>().elapsed_secs_f64();
    match spec {
        None => world
            .resource_mut::<StatusMessage>()
            .warn(format!("unknown account {account}"), now),
        Some(Err(error)) => world
            .resource_mut::<StatusMessage>()
            .warn(format!("authorize: {error:#}"), now),
        Some(Ok(spec)) => {
            let sender = world.resource::<OauthChannel>().sender.clone();
            let Some(engine) = world.get_resource::<EngineResource>() else {
                return;
            };
            let handle = engine.0.runtime_handle();
            match spec {
                FlowSpec::Code(config) => spawn_code_flow(&handle, config, sender, account),
                FlowSpec::Device(config) => spawn_device_flow(&handle, config, sender, account),
            }
            world
                .resource_mut::<StatusMessage>()
                .info("authorization started".to_owned(), now);
        }
    }
}

pub(crate) fn build_flow(account: &AccountConfig) -> anyhow::Result<FlowSpec> {
    let Auth::Oauth2(oauth) = &account.auth else {
        anyhow::bail!("account {:?} does not use oauth2 auth", account.name)
    };
    let preset = presets::preset(oauth.provider);
    let token_endpoint = Url::parse(preset.token_endpoint)?;
    let scopes = preset.scopes.iter().map(|s| (*s).to_owned()).collect();
    let wants_device = match oauth.flow {
        Some(Oauth2Flow::Device) => true,
        Some(Oauth2Flow::Code) => false,
        None => preset.device_endpoint.is_some(),
    };
    if wants_device {
        let device = preset.device_endpoint.ok_or_else(|| {
            anyhow::anyhow!("provider {:?} has no device-grant endpoint", oauth.provider)
        })?;
        return Ok(FlowSpec::Device(DeviceFlowConfig {
            device_endpoint: Url::parse(device)?,
            token_endpoint,
            client_id: oauth.client_id.clone(),
            scopes,
        }));
    }
    Ok(FlowSpec::Code(CodeFlowConfig {
        auth_endpoint: Url::parse(preset.auth_endpoint)?,
        token_endpoint,
        client_id: oauth.client_id.clone(),
        client_secret: oauth.client_secret.clone().map(SecretString::from),
        scopes,
        auth_extras: preset
            .auth_extras
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }))
}

pub(crate) fn spawn_code_flow(
    handle: &nitidus_mail::RuntimeHandle,
    config: CodeFlowConfig,
    sender: mpsc::Sender<OauthEvent>,
    account: String,
) {
    handle.spawn(async move {
        let outcome = run_code_flow(config, &sender).await;
        report(&sender, account, outcome);
    });
}

async fn run_code_flow(
    config: CodeFlowConfig,
    sender: &mpsc::Sender<OauthEvent>,
) -> Result<Grant, nitidus_mail::MailError> {
    let (flow, url) = CodeFlow::start(config).await?;
    let _ = sender.send(OauthEvent::BrowserPrompt(url.to_string()));
    // Tests drive the loopback redirect themselves; opening a real
    // browser from `cargo test` would be hostile.
    #[cfg(not(test))]
    open_browser(url.as_str());
    flow.finish().await
}

pub(crate) fn spawn_device_flow(
    handle: &nitidus_mail::RuntimeHandle,
    config: DeviceFlowConfig,
    sender: mpsc::Sender<OauthEvent>,
    account: String,
) {
    handle.spawn(async move {
        let prompt_sender = sender.clone();
        let outcome = authorize_device(config, move |prompt| {
            let _ = prompt_sender.send(OauthEvent::DevicePrompt(prompt));
        })
        .await;
        report(&sender, account, outcome);
    });
}

fn report(
    sender: &mpsc::Sender<OauthEvent>,
    account: String,
    outcome: Result<Grant, nitidus_mail::MailError>,
) {
    let event = match outcome {
        Ok(grant) => OauthEvent::Granted {
            account,
            refresh_token: grant.refresh_token,
        },
        Err(error) => OauthEvent::Failed {
            account,
            error: error.to_string(),
        },
    };
    let _ = sender.send(event);
}

#[cfg(not(test))]
fn open_browser(url: &str) {
    let spawned = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(error) = spawned {
        tracing::warn!("xdg-open failed ({error}); open the URL manually");
    }
}

pub fn drain_oauth_events(world: &mut World) {
    loop {
        let event = {
            let channel = world.resource::<OauthChannel>();
            let Ok(receiver) = channel.receiver.lock() else {
                return;
            };
            match receiver.try_recv() {
                Ok(event) => event,
                Err(_) => return,
            }
        };
        apply_event(world, event);
    }
}

#[cfg(test)]
pub(crate) fn oauth_sender(world: &World) -> mpsc::Sender<OauthEvent> {
    world.resource::<OauthChannel>().sender.clone()
}

fn apply_event(world: &mut World, event: OauthEvent) {
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut status = world.resource_mut::<StatusMessage>();
    match event {
        OauthEvent::BrowserPrompt(url) => {
            status.info(format!("authorize in the browser — {url}"), now);
        }
        OauthEvent::DevicePrompt(prompt) => {
            status.info(
                format!(
                    "authorize: enter code {} at {}",
                    prompt.user_code, prompt.verification_uri
                ),
                now,
            );
        }
        OauthEvent::Granted {
            account,
            refresh_token,
        } => match keyring::store_oauth_refresh(&account, &refresh_token) {
            Ok(()) => status.info(format!("{account} authorized — restart to connect"), now),
            Err(error) => status.warn(format!("storing grant for {account}: {error:#}"), now),
        },
        OauthEvent::Failed { account, error } => {
            status.warn(format!("authorize {account}: {error}"), now);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::time::Duration;

    use nitidus_mail::MailEngine;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::config::account::{Oauth2Auth, Oauth2Provider};
    use crate::config::keyring::use_mock_keyring;

    fn oauth_app() -> App {
        use_mock_keyring();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<StatusMessage>();
        app.add_plugins(super::super::AccountsPlugin);
        app.update();
        app
    }

    fn json_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    /// One scripted response per successive connection.
    fn spawn_http_server(runtime: &tokio::runtime::Runtime, responses: Vec<String>) -> u16 {
        runtime.block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(async move {
                for response in responses {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut buffer = [0u8; 8192];
                    let mut raw = Vec::new();
                    loop {
                        let n = stream.read(&mut buffer).await.unwrap();
                        raw.extend_from_slice(&buffer[..n]);
                        let text = String::from_utf8_lossy(&raw).into_owned();
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
                                break;
                            }
                        }
                    }
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            });
            port
        })
    }

    fn server_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    fn wait_for(app: &mut App, mut is_done: impl FnMut(&World) -> bool) -> bool {
        for _ in 0..600 {
            app.update();
            if is_done(app.world()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn status_text(world: &World) -> String {
        world
            .resource::<StatusMessage>()
            .current()
            .map(|(text, _)| text.to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn device_flow_polls_until_granted_and_stores_the_refresh_token() {
        let server = server_runtime();
        let port = spawn_http_server(
            &server,
            vec![
                json_response(
                    "200 OK",
                    "{\"device_code\":\"dev-1\",\"user_code\":\"ABCD-EFGH\",\
                     \"verification_uri\":\"https://example.com/device\",\
                     \"expires_in\":300,\"interval\":0}",
                ),
                json_response("400 Bad Request", "{\"error\":\"authorization_pending\"}"),
                json_response(
                    "200 OK",
                    "{\"access_token\":\"acc-1\",\"token_type\":\"Bearer\",\
                     \"expires_in\":3600,\"refresh_token\":\"ref-device\"}",
                ),
            ],
        );
        let mut app = oauth_app();
        let engine = MailEngine::new(1).unwrap();
        let config = DeviceFlowConfig {
            device_endpoint: Url::parse(&format!("http://127.0.0.1:{port}/device")).unwrap(),
            token_endpoint: Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap(),
            client_id: "client-dev".to_owned(),
            scopes: vec!["mail".to_owned()],
        };
        spawn_device_flow(
            &engine.runtime_handle(),
            config,
            oauth_sender(app.world()),
            "oauth-device-e2e".to_owned(),
        );

        assert!(
            wait_for(&mut app, |world| {
                status_text(world).contains("ABCD-EFGH")
                    || keyring::load_oauth_refresh("oauth-device-e2e").is_ok()
            }),
            "the device prompt never surfaced"
        );
        assert!(
            wait_for(&mut app, |_| keyring::load_oauth_refresh(
                "oauth-device-e2e"
            )
            .is_ok()),
            "the grant never landed in the keyring"
        );
        let stored = keyring::load_oauth_refresh("oauth-device-e2e").unwrap();
        assert_eq!(
            nitidus_mail::ExposeSecret::expose_secret(&stored),
            "ref-device"
        );
        assert!(
            wait_for(&mut app, |world| status_text(world)
                .contains("restart to connect")),
            "the granted notice never surfaced"
        );
    }

    #[test]
    fn code_flow_exchanges_the_driven_redirect_and_stores_the_grant() {
        let server = server_runtime();
        let port = spawn_http_server(
            &server,
            vec![json_response(
                "200 OK",
                "{\"access_token\":\"acc-2\",\"token_type\":\"Bearer\",\
                 \"expires_in\":3600,\"refresh_token\":\"ref-code\"}",
            )],
        );
        let mut app = oauth_app();
        let engine = MailEngine::new(1).unwrap();
        let config = CodeFlowConfig {
            auth_endpoint: Url::parse("https://auth.example.com/authorize").unwrap(),
            token_endpoint: Url::parse(&format!("http://127.0.0.1:{port}/token")).unwrap(),
            client_id: "client-code".to_owned(),
            client_secret: None,
            scopes: vec!["mail".to_owned()],
            auth_extras: vec![("prompt".to_owned(), "consent".to_owned())],
        };
        spawn_code_flow(
            &engine.runtime_handle(),
            config,
            oauth_sender(app.world()),
            "oauth-code-e2e".to_owned(),
        );

        assert!(
            wait_for(&mut app, |world| {
                status_text(world).contains("authorize in the browser")
            }),
            "the browser prompt never surfaced"
        );
        let prompt = status_text(app.world());
        let auth_url = Url::parse(prompt.split(" — ").nth(1).unwrap()).unwrap();
        assert!(prompt.contains("prompt=consent"), "{prompt}");
        let query: std::collections::HashMap<String, String> = auth_url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let redirect = query.get("redirect_uri").unwrap();
        let state = query.get("state").unwrap();

        // Play the browser: deliver the code to the loopback listener,
        // percent-encoding the (arbitrary-byte) state faithfully.
        let mut redirect_url = Url::parse(redirect).unwrap();
        redirect_url
            .query_pairs_mut()
            .append_pair("code", "code-42")
            .append_pair("state", state);
        server.block_on(async {
            let target = redirect_url;
            let mut stream = tokio::net::TcpStream::connect((
                target.host_str().unwrap(),
                target.port().unwrap(),
            ))
            .await
            .unwrap();
            let path = format!("{}?{}", target.path(), target.query().unwrap());
            stream
                .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut sink = Vec::new();
            let _ = stream.read_to_end(&mut sink).await;
        });

        assert!(
            wait_for(&mut app, |_| keyring::load_oauth_refresh("oauth-code-e2e")
                .is_ok()),
            "the grant never landed in the keyring"
        );
        let stored = keyring::load_oauth_refresh("oauth-code-e2e").unwrap();
        assert_eq!(
            nitidus_mail::ExposeSecret::expose_secret(&stored),
            "ref-code"
        );
    }

    #[test]
    fn build_flow_picks_the_provider_grant() {
        let mut account = AccountConfig {
            name: "flows".to_owned(),
            auth: Auth::Oauth2(Oauth2Auth {
                provider: Oauth2Provider::Google,
                client_id: "id".to_owned(),
                client_secret: None,
                flow: None,
            }),
            ..Default::default()
        };
        assert!(matches!(build_flow(&account), Ok(FlowSpec::Code(_))));
        account.auth = Auth::Oauth2(Oauth2Auth {
            provider: Oauth2Provider::Microsoft,
            client_id: "id".to_owned(),
            client_secret: None,
            flow: None,
        });
        assert!(matches!(build_flow(&account), Ok(FlowSpec::Device(_))));
        account.auth = Auth::Oauth2(Oauth2Auth {
            provider: Oauth2Provider::Microsoft,
            client_id: "id".to_owned(),
            client_secret: None,
            flow: Some(Oauth2Flow::Code),
        });
        assert!(
            matches!(build_flow(&account), Ok(FlowSpec::Code(_))),
            "flow = \"code\" must override the microsoft device default"
        );
        account.auth = Auth::Oauth2(Oauth2Auth {
            provider: Oauth2Provider::Google,
            client_id: "id".to_owned(),
            client_secret: None,
            flow: Some(Oauth2Flow::Device),
        });
        assert!(
            build_flow(&account).is_err(),
            "google has no device endpoint to force"
        );
        account.auth = Auth::Keyring;
        assert!(build_flow(&account).is_err());
    }
}
