//! Bevy-side wiring for the mail engine: the resource wrapper, the
//! per-frame event drain, and connection status for the statusline.

use std::collections::BTreeMap;

use bevy::prelude::*;
use nitidus_mail::{AccountId, ConnectionState, MailEngine, MailEvent};

const MAX_EVENTS_PER_FRAME: usize = 64;

pub struct EnginePlugin;

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EngineStatus>();
        app.add_systems(PreUpdate, drain_mail_events);
    }
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
