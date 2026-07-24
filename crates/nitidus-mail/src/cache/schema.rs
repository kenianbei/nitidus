//! Pragmas and `user_version` migrations for the envelope cache.

use std::time::Duration;

use rusqlite::Connection;
use rusqlite_migration::{M, MigrationDefinitionError, Migrations};

use super::CacheError;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const SCHEMA_V1: &str = "\
CREATE TABLE folders (
    account TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    unread INTEGER NOT NULL,
    total INTEGER NOT NULL,
    PRIMARY KEY (account, id)
) STRICT;
CREATE TABLE envelopes (
    account TEXT NOT NULL,
    folder TEXT NOT NULL,
    id TEXT NOT NULL,
    subject TEXT NOT NULL,
    from_display TEXT NOT NULL,
    from_addr TEXT NOT NULL,
    date_epoch_secs INTEGER NOT NULL,
    flags INTEGER NOT NULL,
    seen_job INTEGER NOT NULL,
    PRIMARY KEY (account, folder, id)
) STRICT;
CREATE INDEX envelopes_by_folder_date
    ON envelopes (account, folder, date_epoch_secs DESC);
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(SCHEMA_V1)])
}

pub fn prepare(connection: &mut Connection) -> Result<(), CacheError> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    migrations().to_latest(connection).map_err(|error| {
        if is_newer_schema(&error) {
            CacheError::NewerSchema
        } else {
            CacheError::Database(error.to_string())
        }
    })
}

fn is_newer_schema(error: &rusqlite_migration::Error) -> bool {
    matches!(
        error,
        rusqlite_migration::Error::MigrationDefinition(
            MigrationDefinitionError::DatabaseTooFarAhead
        )
    )
}
