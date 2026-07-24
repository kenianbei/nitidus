//! Bevy-side wiring for the mail engine: the resource wrapper, the
//! per-frame event drain, and connection status for the statusline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use bevy::prelude::*;
use nitidus_mail::maildir::{self, MaildirBackend};
use nitidus_mail::{AccountId, ConnectionState, FolderId, MailCommand, MailEngine, MailEvent};

use crate::config::Config;
use crate::config::account::Backend;
use crate::status::StatusMessage;

const MAX_EVENTS_PER_FRAME: usize = 64;

pub struct EnginePlugin;

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EngineStatus>();
        app.init_resource::<StartupNotices>();
        app.init_resource::<StatusMessage>();
        app.add_systems(PreUpdate, drain_mail_events);
        app.add_systems(Update, surface_startup_notices);
    }
}

/// Registers every configured account with the engine; returns
/// user-facing notices for accounts that cannot run yet.
pub fn register_accounts(engine: &mut MailEngine, config: &Config) -> anyhow::Result<Vec<String>> {
    let mut notices = Vec::new();
    for account in &config.accounts {
        match &account.backend {
            Some(Backend::Maildir(settings)) => {
                register_maildir(engine, &account.name, &settings.path)
                    .with_context(|| format!("account {:?}", account.name))?;
            }
            Some(Backend::Imap(_)) => {
                notices.push(format!("{}: imap not yet supported", account.name));
            }
            None => notices.push(format!("{}: no backend configured", account.name)),
        }
    }
    Ok(notices)
}

fn register_maildir(engine: &mut MailEngine, name: &str, path: &Path) -> anyhow::Result<()> {
    let root = expand_home(path)?;
    let backend = MaildirBackend::new(root.clone())?;
    let id = AccountId::new(name);
    engine.add_account(id.clone(), backend);
    engine.watch_maildir(id.clone(), root);
    engine.send(&id, MailCommand::ListFolders)?;
    let job = engine.next_job();
    engine.send(
        &id,
        MailCommand::SyncEnvelopes {
            folder: FolderId::new(maildir::INBOX),
            job,
        },
    )?;
    Ok(())
}

fn expand_home(path: &Path) -> anyhow::Result<PathBuf> {
    match path.strip_prefix("~") {
        Ok(stripped) => {
            let home = etcetera::home_dir().context("cannot resolve home dir for ~ expansion")?;
            Ok(home.join(stripped))
        }
        Err(_) => Ok(path.to_path_buf()),
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupNotices(pub Vec<String>);

fn surface_startup_notices(
    notices: Res<StartupNotices>,
    mut status: ResMut<StatusMessage>,
    time: Res<Time>,
    mut surfaced: Local<bool>,
) {
    if *surfaced || notices.0.is_empty() {
        return;
    }
    *surfaced = true;
    status.warn(notices.0.join("; "), time.elapsed_secs_f64());
}

#[derive(Resource)]
pub struct EngineResource(pub MailEngine);

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStatus {
    accounts: BTreeMap<AccountId, ConnectionState>,
}

impl EngineStatus {
    pub fn set(&mut self, account: AccountId, state: ConnectionState) {
        self.accounts.insert(account, state);
    }

    pub fn summary(&self) -> Option<String> {
        if self.accounts.is_empty() {
            return None;
        }
        let connected = self
            .accounts
            .values()
            .filter(|state| **state == ConnectionState::Connected)
            .count();
        Some(format!("{connected}/{}", self.accounts.len()))
    }
}

fn drain_mail_events(engine: Option<Res<EngineResource>>, mut status: ResMut<EngineStatus>) {
    let Some(engine) = engine else { return };
    for _ in 0..MAX_EVENTS_PER_FRAME {
        let Some(event) = engine.0.try_recv_event() else {
            return;
        };
        match event {
            MailEvent::Connection { account, state } => status.set(account, state),
            other => tracing::debug!(event = ?other, "mail event (unrouted until MailStore)"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_mail::mock::MockBackend;

    use super::*;

    #[test]
    fn drains_connection_events_into_status() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut engine = MailEngine::new(1).unwrap();
        let account = AccountId::new("mock");
        engine.add_account(account.clone(), MockBackend::new());
        app.insert_resource(EngineResource(engine));
        app.add_plugins(EnginePlugin);

        for _ in 0..200 {
            app.update();
            let status = app.world().resource::<EngineStatus>();
            if status.summary().as_deref() == Some("1/1") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!(
            "engine status never reached 1/1: {:?}",
            app.world().resource::<EngineStatus>()
        );
    }

    #[test]
    fn registers_maildir_account_and_reaches_connected() {
        let tmp = tempfile::tempdir().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(tmp.path().join(sub)).unwrap();
        }
        let mut config = Config::default();
        config.accounts.push(crate::config::account::AccountConfig {
            name: "local".to_owned(),
            backend: Some(Backend::Maildir(crate::config::account::MaildirBackend {
                path: tmp.path().to_path_buf(),
            })),
            ..Default::default()
        });
        let mut engine = MailEngine::new(1).unwrap();
        let notices = register_accounts(&mut engine, &config).unwrap();
        assert!(notices.is_empty(), "{notices:?}");

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(EngineResource(engine));
        app.add_plugins(EnginePlugin);
        for _ in 0..200 {
            app.update();
            if app.world().resource::<EngineStatus>().summary().as_deref() == Some("1/1") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("maildir account never connected");
    }

    #[test]
    fn unsupported_accounts_produce_notices_not_errors() {
        let mut config = Config::default();
        config.accounts.push(crate::config::account::AccountConfig {
            name: "work".to_owned(),
            backend: Some(Backend::Imap(crate::config::account::ImapBackend::default())),
            ..Default::default()
        });
        let mut engine = MailEngine::new(1).unwrap();
        let notices = register_accounts(&mut engine, &config).unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("imap"), "{notices:?}");
    }

    #[test]
    fn missing_maildir_path_fails_registration() {
        let mut config = Config::default();
        config.accounts.push(crate::config::account::AccountConfig {
            name: "broken".to_owned(),
            backend: Some(Backend::Maildir(crate::config::account::MaildirBackend {
                path: "/definitely/not/a/maildir".into(),
            })),
            ..Default::default()
        });
        let mut engine = MailEngine::new(1).unwrap();
        let message = format!("{:#}", register_accounts(&mut engine, &config).unwrap_err());
        assert!(message.contains("broken"), "{message}");
    }

    #[test]
    fn summary_is_none_without_accounts() {
        assert_eq!(EngineStatus::default().summary(), None);
    }

    #[test]
    fn summary_counts_connected_accounts() {
        let mut status = EngineStatus::default();
        status.set(AccountId::new("a"), ConnectionState::Connected);
        status.set(AccountId::new("b"), ConnectionState::Connecting);
        assert_eq!(status.summary().as_deref(), Some("1/2"));
    }
}
