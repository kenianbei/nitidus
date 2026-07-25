//! Bevy-side wiring for the mail engine: the resource wrappers, the
//! per-frame event drain routing into cache and store, and connection
//! status for the statusline.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use nitidus_mail::cache::CacheWriter;
use nitidus_mail::{AccountId, ConnectionState, FolderId, MailEngine, MailEvent};

use crate::bootstrap::request_sync;
use crate::index::IndexView;
use crate::pager::PagerState;
use crate::screen::Screen;
use crate::status::StatusMessage;
use crate::store::{MailStore, SyncTracker, ThreadSet};

const MAX_EVENTS_PER_FRAME: usize = 64;

pub struct EnginePlugin;

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EngineStatus>();
        app.init_resource::<MailStore>();
        app.init_resource::<SyncTracker>();
        app.init_resource::<ThreadSet>();
        app.init_resource::<PagerState>();
        app.init_resource::<IndexView>();
        app.init_resource::<crate::outbox::OutboxState>();
        app.init_resource::<Screen>();
        app.init_resource::<StartupNotices>();
        app.init_resource::<StatusMessage>();
        app.add_systems(PreUpdate, drain_mail_events);
        app.add_systems(Update, surface_startup_notices);
    }
}

#[derive(Resource)]
pub struct EngineResource(pub MailEngine);

#[derive(Resource)]
pub struct CacheResource(pub CacheWriter);

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupNotices(pub Vec<String>);

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

#[derive(SystemParam)]
struct MailRouting<'w> {
    engine: Option<Res<'w, EngineResource>>,
    cache: Option<Res<'w, CacheResource>>,
    status: ResMut<'w, EngineStatus>,
    store: ResMut<'w, MailStore>,
    tracker: ResMut<'w, SyncTracker>,
    threads: ResMut<'w, ThreadSet>,
    pager: ResMut<'w, PagerState>,
    screen: ResMut<'w, Screen>,
    messages: ResMut<'w, StatusMessage>,
    index_view: ResMut<'w, IndexView>,
    outbox: ResMut<'w, crate::outbox::OutboxState>,
    time: Res<'w, Time>,
}

fn drain_mail_events(mut routing: MailRouting) {
    for _ in 0..MAX_EVENTS_PER_FRAME {
        let Some(event) = routing
            .engine
            .as_deref()
            .and_then(|engine| engine.0.try_recv_event())
        else {
            return;
        };
        if let Some(cache) = routing.cache.as_deref() {
            cache.0.record(&event);
        }
        route_event(&mut routing, event);
    }
}

fn route_event(routing: &mut MailRouting, event: MailEvent) {
    match event {
        MailEvent::Connection { account, state } => routing.status.set(account, state),
        MailEvent::Folders { account, folders } => {
            reanchor_vanished_view(routing, &account, &folders);
            routing.store.set_folders(account, folders);
        }
        MailEvent::EnvelopeBatch {
            account,
            folder,
            job,
            batch,
            done,
        } => {
            if done {
                routing.tracker.finish(&account, &folder, job);
            }
            routing
                .store
                .apply_batch(&account, &folder, job, batch, done);
        }
        MailEvent::FolderChanged { account, folder } => resync_changed(routing, account, folder),
        MailEvent::Threads {
            account,
            folder,
            job,
            rows,
        } => routing.threads.accept(&account, &folder, job, rows),
        MailEvent::SendDone { account, job } => {
            crate::outbox::complete_send(&mut routing.outbox, job);
            let now = routing.time.elapsed_secs_f64();
            routing
                .messages
                .info(format!("{account}: message sent"), now);
        }
        MailEvent::JobFailed {
            account,
            job,
            error,
        } => {
            if let Some(job) = job {
                routing.tracker.fail(job);
                if routing.pager.fail_fetch(job) {
                    *routing.screen = Screen::Index;
                }
                crate::outbox::fail_send(&mut routing.outbox, job);
            }
            let now = routing.time.elapsed_secs_f64();
            routing.messages.warn(format!("{account}: {error}"), now);
        }
        MailEvent::Message {
            account,
            folder,
            id,
            job,
            raw,
        } => routing.pager.receive(account, folder, id, job, raw),
    }
}

/// A folder list that no longer contains the viewed folder (deleted or
/// renamed elsewhere) sends the view back to INBOX.
fn reanchor_vanished_view(
    routing: &mut MailRouting,
    account: &AccountId,
    folders: &[nitidus_mail::FolderMeta],
) {
    let index_view = &mut routing.index_view;
    let is_viewed_account = index_view.account.as_ref() == Some(account);
    if folders.is_empty()
        || !is_viewed_account
        || folders.iter().any(|meta| meta.id == index_view.folder)
    {
        return;
    }
    index_view.folder = FolderId::new(nitidus_mail::maildir::INBOX);
    index_view.selected = None;
    index_view.selected_row = 0;
    index_view.top = 0;
    index_view.collapsed.clear();
    index_view.fold_epoch += 1;
}

/// Folders never scanned this session stay lazy: their first view will
/// trigger the scan that also picks up this change.
fn resync_changed(routing: &mut MailRouting, account: AccountId, folder: FolderId) {
    let Some(engine) = routing.engine.as_deref() else {
        return;
    };
    // Watched changes also refresh the folder list, keeping sidebar
    // unread snapshots current for folders not synced this session.
    if let Err(error) = engine
        .0
        .send(&account, nitidus_mail::MailCommand::ListFolders)
    {
        tracing::warn!("folder-list refresh after change failed: {error}");
    }
    if !routing.tracker.is_tracked(&account, &folder) {
        return;
    }
    if let Err(error) = request_sync(&engine.0, &mut routing.tracker, &account, &folder) {
        tracing::warn!("re-sync of {folder} after change failed: {error}");
    }
}

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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::path::Path;
    use std::time::Duration;

    use nitidus_mail::mock::MockBackend;
    use nitidus_mail::{MailCommand, maildir};

    use super::*;
    use crate::bootstrap::register_accounts;
    use crate::config::Config;
    use crate::config::account::{AccountConfig, Backend};

    fn engine_app(engine: MailEngine, tracker: SyncTracker) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(EngineResource(engine));
        app.add_plugins(EnginePlugin);
        app.insert_resource(tracker);
        app
    }

    fn update_until(app: &mut App, mut is_done: impl FnMut(&World) -> bool) -> bool {
        for _ in 0..400 {
            app.update();
            if is_done(app.world()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn drains_connection_events_into_status() {
        let mut engine = MailEngine::new(1).unwrap();
        engine.add_account(AccountId::new("mock"), MockBackend::new());
        let mut app = engine_app(engine, SyncTracker::default());
        assert!(
            update_until(&mut app, |world| {
                world.resource::<EngineStatus>().summary().as_deref() == Some("1/1")
            }),
            "engine status never reached 1/1"
        );
    }

    #[test]
    fn mock_scan_fills_store_and_marks_folder_synced() {
        let account = AccountId::new("mock");
        let inbox = FolderId::new("INBOX");
        let mut engine = MailEngine::new(1).unwrap();
        engine.add_account(account.clone(), MockBackend::new().with_folder("INBOX", 5));
        engine.send(&account, MailCommand::ListFolders).unwrap();
        let mut tracker = SyncTracker::default();
        request_sync(&engine, &mut tracker, &account, &inbox).unwrap();

        let mut app = engine_app(engine, tracker);
        assert!(
            update_until(&mut app, |world| {
                world
                    .resource::<MailStore>()
                    .envelopes(&account, &inbox)
                    .len()
                    == 5
            }),
            "store never received the scanned envelopes"
        );
        let world = app.world();
        assert_eq!(world.resource::<MailStore>().folders(&account).len(), 1);
        let tracker = world.resource::<SyncTracker>();
        assert!(tracker.is_tracked(&account, &inbox));
        assert_eq!(tracker.in_flight_job(&account, &inbox), None);
    }

    #[test]
    fn failed_scan_surfaces_status_warning() {
        let account = AccountId::new("mock");
        let mut engine = MailEngine::new(1).unwrap();
        engine.add_account(account.clone(), MockBackend::new().with_failing_scan());
        let mut tracker = SyncTracker::default();
        request_sync(&engine, &mut tracker, &account, &FolderId::new("INBOX")).unwrap();

        let mut app = engine_app(engine, tracker);
        assert!(
            update_until(&mut app, |world| {
                world.resource::<StatusMessage>().current().is_some()
            }),
            "scan failure never reached the status message"
        );
        let tracker = app.world().resource::<SyncTracker>();
        assert!(!tracker.is_tracked(&account, &FolderId::new("INBOX")));
    }

    fn deliver(root: &Path, name: &str) {
        let body = format!(
            "From: A <a@example.com>\r\nSubject: {name}\r\nDate: Thu, 15 Feb 2024 12:00:00 +0000\r\n\r\nx\r\n"
        );
        std::fs::write(root.join("new").join(name), body).unwrap();
    }

    #[test]
    fn external_delivery_resyncs_into_store() {
        let tmp = tempfile::tempdir().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(tmp.path().join(sub)).unwrap();
        }
        deliver(tmp.path(), "first.host");

        let mut config = Config::default();
        config.accounts.push(AccountConfig {
            name: "local".to_owned(),
            backend: Some(Backend::Maildir(crate::config::account::MaildirBackend {
                path: tmp.path().to_path_buf(),
            })),
            ..Default::default()
        });
        let mut engine = MailEngine::new(1).unwrap();
        let mut tracker = SyncTracker::default();
        let mut notices = Vec::new();
        register_accounts(&mut engine, &config, &mut tracker, &mut notices).unwrap();
        assert!(notices.is_empty(), "{notices:?}");

        let account = AccountId::new("local");
        let inbox = FolderId::new(maildir::INBOX);
        let mut app = engine_app(engine, tracker);
        assert!(
            update_until(&mut app, |world| {
                world
                    .resource::<MailStore>()
                    .envelopes(&account, &inbox)
                    .len()
                    == 1
            }),
            "initial scan never reached the store"
        );

        deliver(tmp.path(), "second.host");
        assert!(
            update_until(&mut app, |world| {
                world
                    .resource::<MailStore>()
                    .envelopes(&account, &inbox)
                    .len()
                    == 2
            }),
            "external delivery never re-synced into the store"
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
