//! Persistent envelope cache (SQLite, WAL). Cache-tier by contract:
//! deleting the database only costs a re-scan, never data. The UI reads
//! it exactly once per run (warm start); afterwards the connection moves
//! onto a dedicated writer thread and all reads come from memory.

mod schema;
mod writer;

pub use writer::CacheWriter;

use std::path::Path;

use rusqlite::Connection;

use crate::types::{AccountId, EnvelopeId, EnvelopeSummary, Flags, FolderId, FolderMeta};

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache schema is newer than this nitidus understands")]
    NewerSchema,
    #[error("cache database error: {0}")]
    Database(String),
}

impl From<rusqlite::Error> for CacheError {
    fn from(error: rusqlite::Error) -> Self {
        CacheError::Database(error.to_string())
    }
}

pub struct MailCache {
    connection: Connection,
}

impl MailCache {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        let mut connection = Connection::open(path)?;
        schema::prepare(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn load_folders(&self, account: &AccountId) -> Result<Vec<FolderMeta>, CacheError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, unread, total FROM folders WHERE account = ?1 ORDER BY name",
        )?;
        let rows = statement.query_map([account.as_str()], |row| {
            Ok(FolderMeta {
                id: FolderId::new(row.get::<_, String>(0)?),
                name: row.get(1)?,
                unread: row.get(2)?,
                total: row.get(3)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    pub fn load_envelopes(
        &self,
        account: &AccountId,
        folder: &FolderId,
    ) -> Result<Vec<EnvelopeSummary>, CacheError> {
        let mut statement = self.connection.prepare(
            "SELECT id, subject, from_display, from_addr, date_epoch_secs, flags,
                    message_id, reference_ids
             FROM envelopes WHERE account = ?1 AND folder = ?2
             ORDER BY date_epoch_secs DESC",
        )?;
        let rows = statement.query_map([account.as_str(), folder.as_str()], |row| {
            Ok(EnvelopeSummary {
                id: EnvelopeId::new(row.get::<_, String>(0)?),
                subject: row.get(1)?,
                from_display: row.get(2)?,
                from_addr: row.get(3)?,
                date_epoch_secs: row.get(4)?,
                flags: Flags::from_bits(row.get(5)?),
                message_id: row.get(6)?,
                references: split_references(&row.get::<_, String>(7)?),
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    /// Moves the connection onto the dedicated writer thread; the
    /// returned handle is the only way to touch the database from here on.
    pub fn into_writer(self) -> CacheWriter {
        writer::spawn(self.connection)
    }
}

/// Message-ids cannot contain newlines, so the reference chain stores
/// newline-joined.
pub(crate) fn join_references(references: &[String]) -> String {
    references.join("\n")
}

fn split_references(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        return Vec::new();
    }
    joined.lines().map(str::to_owned).collect()
}
