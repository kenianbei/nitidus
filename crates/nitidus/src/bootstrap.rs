//! Startup assembly of the mail side: open the envelope cache (never
//! fatally — it is deletable by contract), warm-load `MailStore`,
//! register accounts, and kick off the eager INBOX scans.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use nitidus_mail::cache::{CacheError, CacheWriter, MailCache};
use nitidus_mail::maildir::{self, MaildirBackend};
use nitidus_mail::{AccountId, FolderId, MailCommand, MailEngine};

use crate::config::Config;
use crate::config::account::Backend;
use crate::dirs;
use crate::store::{MailStore, SyncTracker};

const CACHE_DB_FILE: &str = "mail.db";

pub struct EngineSetup {
    pub engine: MailEngine,
    pub cache: Option<CacheWriter>,
    pub store: MailStore,
    pub tracker: SyncTracker,
    pub notices: Vec<String>,
}

pub fn bootstrap(config: &Config) -> anyhow::Result<EngineSetup> {
    let mut notices = Vec::new();
    let mut store = MailStore::default();
    let cache = open_default_cache(&mut notices);
    if let Some(cache) = &cache {
        warm_load(cache, config, &mut store);
    }
    let mut engine = MailEngine::new(config.accounts.len())?;
    let mut tracker = SyncTracker::default();
    register_accounts(&mut engine, config, &mut tracker, &mut notices)?;
    Ok(EngineSetup {
        engine,
        cache: cache.map(MailCache::into_writer),
        store,
        tracker,
        notices,
    })
}

/// (Re-)requests a folder scan, cancelling any in-flight scan of the
/// same folder — the single entry point for eager, lazy-first-view, and
/// change-triggered syncs.
pub fn request_sync(
    engine: &MailEngine,
    tracker: &mut SyncTracker,
    account: &AccountId,
    folder: &FolderId,
) -> Result<(), nitidus_mail::MailError> {
    if let Some(job) = tracker.in_flight_job(account, folder) {
        engine.send(account, MailCommand::Cancel(job))?;
    }
    let job = engine.next_job();
    engine.send(
        account,
        MailCommand::SyncEnvelopes {
            folder: folder.clone(),
            job,
        },
    )?;
    tracker.begin(account.clone(), folder.clone(), job);
    Ok(())
}

/// Registers every configured account with the engine; returns
/// user-facing notices for accounts that cannot run yet.
pub fn register_accounts(
    engine: &mut MailEngine,
    config: &Config,
    tracker: &mut SyncTracker,
    notices: &mut Vec<String>,
) -> anyhow::Result<()> {
    for account in &config.accounts {
        match &account.backend {
            Some(Backend::Maildir(settings)) => {
                register_maildir(engine, tracker, &account.name, &settings.path)
                    .with_context(|| format!("account {:?}", account.name))?;
            }
            Some(Backend::Imap(_)) => {
                notices.push(format!("{}: imap not yet supported", account.name));
            }
            None => notices.push(format!("{}: no backend configured", account.name)),
        }
    }
    Ok(())
}

fn register_maildir(
    engine: &mut MailEngine,
    tracker: &mut SyncTracker,
    name: &str,
    path: &Path,
) -> anyhow::Result<()> {
    let root = expand_home(path)?;
    let backend = MaildirBackend::new(root.clone())?;
    let id = AccountId::new(name);
    engine.add_account(id.clone(), backend);
    engine.watch_maildir(id.clone(), root);
    engine.send(&id, MailCommand::ListFolders)?;
    request_sync(engine, tracker, &id, &FolderId::new(maildir::INBOX))?;
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

fn open_default_cache(notices: &mut Vec<String>) -> Option<MailCache> {
    let dir = match dirs::cache_dir() {
        Ok(dir) => dir,
        Err(error) => {
            notices.push(format!("mail cache unavailable: {error:#}"));
            return None;
        }
    };
    if let Err(error) = fs::create_dir_all(&dir) {
        notices.push(format!("mail cache unavailable: {error}"));
        return None;
    }
    open_cache_at(&dir.join(CACHE_DB_FILE), notices)
}

fn open_cache_at(path: &Path, notices: &mut Vec<String>) -> Option<MailCache> {
    match MailCache::open(path) {
        Ok(cache) => Some(cache),
        Err(CacheError::NewerSchema) => {
            notices.push("mail cache was written by a newer nitidus; running without cache".into());
            None
        }
        Err(error) => {
            tracing::warn!("recreating mail cache after open failure: {error}");
            remove_cache_files(path);
            match MailCache::open(path) {
                Ok(cache) => Some(cache),
                Err(error) => {
                    notices.push(format!("mail cache unavailable: {error}"));
                    None
                }
            }
        }
    }
}

fn remove_cache_files(path: &Path) {
    for sidecar_suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(sidecar_suffix);
        if let Err(error) = fs::remove_file(PathBuf::from(&file))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("could not remove stale cache file: {error}");
        }
    }
}

/// Cache read failures degrade to a cold start for that account only.
fn warm_load(cache: &MailCache, config: &Config, store: &mut MailStore) {
    for account in &config.accounts {
        let id = AccountId::new(&account.name);
        let folders = match cache.load_folders(&id) {
            Ok(folders) => folders,
            Err(error) => {
                tracing::warn!("warm load failed for {id}: {error}");
                continue;
            }
        };
        for folder in &folders {
            match cache.load_envelopes(&id, &folder.id) {
                Ok(envelopes) if !envelopes.is_empty() => {
                    store.hydrate(id.clone(), folder.id.clone(), envelopes);
                }
                Ok(_empty) => {}
                Err(error) => tracing::warn!("warm load of {} failed: {error}", folder.id),
            }
        }
        if !folders.is_empty() {
            store.set_folders(id, folders);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use nitidus_mail::{EnvelopeId, EnvelopeSummary, Flags, FolderMeta, JobId, MailEvent};

    use super::*;
    use crate::config::account::AccountConfig;

    fn config_with_account(name: &str) -> Config {
        let mut config = Config::default();
        config.accounts.push(AccountConfig {
            name: name.to_owned(),
            ..Default::default()
        });
        config
    }

    fn populate_cache(path: &Path) {
        let account = AccountId::new("local");
        let writer = MailCache::open(path).unwrap().into_writer();
        writer.record(&MailEvent::Folders {
            account: account.clone(),
            folders: vec![FolderMeta {
                id: FolderId::new("INBOX"),
                name: "INBOX".to_owned(),
                unread: 1,
                total: 1,
            }],
        });
        writer.record(&MailEvent::EnvelopeBatch {
            account,
            folder: FolderId::new("INBOX"),
            job: JobId(1),
            batch: vec![EnvelopeSummary {
                id: EnvelopeId::new("warm"),
                subject: "warm subject".to_owned(),
                from_display: String::new(),
                from_addr: String::new(),
                date_epoch_secs: 42,
                flags: Flags::default(),
                message_id: "warm@example".to_owned(),
                references: Vec::new(),
            }],
            done: true,
        });
        writer.close();
    }

    #[test]
    fn warm_load_hydrates_store_from_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mail.db");
        populate_cache(&path);

        let cache = open_cache_at(&path, &mut Vec::new()).unwrap();
        let mut store = MailStore::default();
        warm_load(&cache, &config_with_account("local"), &mut store);

        let account = AccountId::new("local");
        assert_eq!(store.folders(&account).len(), 1);
        let envelopes = store.envelopes(&account, &FolderId::new("INBOX"));
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].subject, "warm subject");
    }

    #[test]
    fn corrupt_cache_is_recreated() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mail.db");
        fs::write(&path, "garbage that is definitely not sqlite at all").unwrap();
        let mut notices = Vec::new();
        assert!(open_cache_at(&path, &mut notices).is_some());
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn newer_schema_runs_cacheless_with_notice() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mail.db");
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection.pragma_update(None, "user_version", 99).unwrap();
        }
        let mut notices = Vec::new();
        assert!(open_cache_at(&path, &mut notices).is_none());
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("newer"), "{notices:?}");
        assert!(path.exists(), "a newer nitidus's cache must not be deleted");
    }

    #[test]
    fn unsupported_accounts_produce_notices_not_errors() {
        let mut config = config_with_account("work");
        config.accounts[0].backend = Some(Backend::Imap(Default::default()));
        let mut engine = MailEngine::new(1).unwrap();
        let mut notices = Vec::new();
        register_accounts(
            &mut engine,
            &config,
            &mut SyncTracker::default(),
            &mut notices,
        )
        .unwrap();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("imap"), "{notices:?}");
    }

    #[test]
    fn missing_maildir_path_fails_registration() {
        let mut config = config_with_account("broken");
        config.accounts[0].backend = Some(Backend::Maildir(crate::config::account::MaildirBackend {
            path: "/definitely/not/a/maildir".into(),
        }));
        let mut engine = MailEngine::new(1).unwrap();
        let error = register_accounts(
            &mut engine,
            &config,
            &mut SyncTracker::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("broken"));
    }
}
