//! The cache writer thread: sole owner of the SQLite connection after
//! warm start. Write failures log and drop the op — the cache is
//! repaired by the next full scan, never worth failing the UI over.

use std::thread::JoinHandle;

use rusqlite::Connection;

use super::CacheError;
use crate::event::MailEvent;
use crate::types::{AccountId, EnvelopeSummary, FolderId, JobId};

enum CacheOp {
    UpsertFolders {
        account: AccountId,
        folders: Vec<crate::types::FolderMeta>,
    },
    UpsertBatch {
        account: AccountId,
        folder: FolderId,
        job: JobId,
        batch: Vec<EnvelopeSummary>,
        done: bool,
    },
}

pub struct CacheWriter {
    ops: flume::Sender<CacheOp>,
    thread: Option<JoinHandle<()>>,
}

pub(super) fn spawn(connection: Connection) -> CacheWriter {
    let (ops_tx, ops_rx) = flume::unbounded();
    let thread = std::thread::spawn(move || run(connection, &ops_rx));
    CacheWriter {
        ops: ops_tx,
        thread: Some(thread),
    }
}

impl CacheWriter {
    /// Non-blocking; events the cache does not persist are ignored.
    pub fn record(&self, event: &MailEvent) {
        let op = match event {
            MailEvent::Folders { account, folders } => CacheOp::UpsertFolders {
                account: account.clone(),
                folders: folders.clone(),
            },
            MailEvent::EnvelopeBatch {
                account,
                folder,
                job,
                batch,
                done,
            } => CacheOp::UpsertBatch {
                account: account.clone(),
                folder: folder.clone(),
                job: *job,
                batch: batch.clone(),
                done: *done,
            },
            _ => return,
        };
        if self.ops.send(op).is_err() {
            tracing::warn!("cache writer thread gone; dropping cache update");
        }
    }

    /// Drains pending ops and joins the thread — deterministic shutdown
    /// for tests and app exit.
    pub fn close(mut self) {
        let thread = self.thread.take();
        drop(self);
        if let Some(thread) = thread
            && thread.join().is_err()
        {
            tracing::warn!("cache writer thread panicked");
        }
    }
}

fn run(mut connection: Connection, ops: &flume::Receiver<CacheOp>) {
    while let Ok(op) = ops.recv() {
        if let Err(error) = apply(&mut connection, &op) {
            tracing::warn!("cache write failed: {error}");
        }
    }
}

fn apply(connection: &mut Connection, op: &CacheOp) -> Result<(), CacheError> {
    let tx = connection.transaction()?;
    match op {
        CacheOp::UpsertFolders { account, folders } => {
            tx.execute("DELETE FROM folders WHERE account = ?1", [account.as_str()])?;
            for folder in folders {
                tx.execute(
                    "INSERT INTO folders (account, id, name, unread, total)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        account.as_str(),
                        folder.id.as_str(),
                        &folder.name,
                        folder.unread,
                        folder.total,
                    ),
                )?;
            }
            tx.execute(
                "DELETE FROM envelopes WHERE account = ?1
                 AND folder NOT IN (SELECT id FROM folders WHERE account = ?1)",
                [account.as_str()],
            )?;
        }
        CacheOp::UpsertBatch {
            account,
            folder,
            job,
            batch,
            done,
        } => {
            for envelope in batch {
                upsert_envelope(&tx, account, folder, *job, envelope)?;
            }
            if *done {
                tx.execute(
                    "DELETE FROM envelopes
                     WHERE account = ?1 AND folder = ?2 AND seen_job <> ?3",
                    (account.as_str(), folder.as_str(), job.0 as i64),
                )?;
            }
        }
    }
    tx.commit().map_err(Into::into)
}

fn upsert_envelope(
    tx: &rusqlite::Transaction<'_>,
    account: &AccountId,
    folder: &FolderId,
    job: JobId,
    envelope: &EnvelopeSummary,
) -> Result<(), CacheError> {
    tx.execute(
        "INSERT INTO envelopes
             (account, folder, id, subject, from_display, from_addr,
              date_epoch_secs, flags, seen_job)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (account, folder, id) DO UPDATE SET
             subject = excluded.subject,
             from_display = excluded.from_display,
             from_addr = excluded.from_addr,
             date_epoch_secs = excluded.date_epoch_secs,
             flags = excluded.flags,
             seen_job = excluded.seen_job",
        (
            account.as_str(),
            folder.as_str(),
            envelope.id.as_str(),
            &envelope.subject,
            &envelope.from_display,
            &envelope.from_addr,
            envelope.date_epoch_secs,
            envelope.flags.bits(),
            job.0 as i64,
        ),
    )?;
    Ok(())
}
