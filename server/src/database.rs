use std::{path::Path, time::Duration};

use rusqlite::{Connection, Error, ErrorCode};

use crate::StoreError;

const BUSY_TIMEOUT: Duration = Duration::from_millis(400);

pub(crate) fn connect(path: &Path) -> Result<Connection, StoreError> {
    let connection = Connection::open(path).map_err(sql_error)?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(sql_error)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(sql_error)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(sql_error)?;
    Ok(connection)
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), StoreError> {
    connection
        .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
        .map_err(sql_error)?;
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS enrollment_tokens (
             digest TEXT PRIMARY KEY CHECK(length(digest) = 64),
             expires_at INTEGER NOT NULL,
             consumed_at INTEGER
         );
         CREATE TABLE IF NOT EXISTS devices (
             pc_id TEXT PRIMARY KEY CHECK(length(pc_id) = 32),
             label TEXT NOT NULL,
             mirror_directory TEXT NOT NULL UNIQUE,
             credential_digest TEXT NOT NULL UNIQUE CHECK(length(credential_digest) = 64),
             created_at INTEGER NOT NULL,
             revoked_at INTEGER
         );
         CREATE TABLE IF NOT EXISTS device_state (
             pc_id TEXT PRIMARY KEY REFERENCES devices(pc_id) ON DELETE CASCADE,
             generation INTEGER NOT NULL DEFAULT 0 CHECK(generation >= 0)
         );
         CREATE TABLE IF NOT EXISTS selections (
             pc_id TEXT NOT NULL REFERENCES device_state(pc_id) ON DELETE CASCADE,
             selection_id TEXT NOT NULL CHECK(length(selection_id) = 32),
             source_json TEXT NOT NULL,
             root_kind TEXT NOT NULL DEFAULT 'file' CHECK(root_kind IN ('file', 'directory')),
             active INTEGER NOT NULL CHECK(active IN (0, 1)),
             PRIMARY KEY(pc_id, selection_id)
         );
         CREATE TABLE IF NOT EXISTS entry_metadata (
             pc_id TEXT NOT NULL REFERENCES devices(pc_id) ON DELETE CASCADE,
             source_json TEXT NOT NULL,
             revision INTEGER NOT NULL CHECK(revision > 0),
             file_size TEXT NOT NULL CHECK(length(file_size) > 0),
             digest TEXT NOT NULL CHECK(length(digest) = 64),
             last_write_seconds INTEGER NOT NULL,
             last_write_nanos INTEGER NOT NULL CHECK(last_write_nanos >= 0 AND last_write_nanos < 1000000000),
             device TEXT NOT NULL,
             inode TEXT NOT NULL,
             PRIMARY KEY(pc_id, source_json)
         );
         CREATE TABLE IF NOT EXISTS directory_metadata (
             pc_id TEXT NOT NULL REFERENCES devices(pc_id) ON DELETE CASCADE,
             source_json TEXT NOT NULL,
             revision INTEGER NOT NULL CHECK(revision > 0),
             device TEXT NOT NULL,
             inode TEXT NOT NULL,
             PRIMARY KEY(pc_id, source_json)
         );
         CREATE TABLE IF NOT EXISTS entry_revisions (
             pc_id TEXT NOT NULL REFERENCES devices(pc_id) ON DELETE CASCADE,
             source_json TEXT NOT NULL,
             revision INTEGER NOT NULL CHECK(revision > 0),
             PRIMARY KEY(pc_id, source_json)
         );
         PRAGMA user_version = 2;
         COMMIT;",
        )
        .map_err(sql_error)?;
    migrate_selection_root_kind(connection)
}

fn migrate_selection_root_kind(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .map_err(sql_error)?;
    let has_root_kind = {
        let mut statement = connection
            .prepare("PRAGMA table_info(selections)")
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sql_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_error)?
            .iter()
            .any(|name| name == "root_kind")
    };
    if !has_root_kind {
        connection
            .execute_batch(
                "ALTER TABLE selections ADD COLUMN root_kind TEXT NOT NULL DEFAULT 'file' \
                 CHECK(root_kind IN ('file', 'directory'));",
            )
            .map_err(sql_error)?;
    }
    connection
        .execute_batch(
            "INSERT INTO entry_revisions(pc_id, source_json, revision) \
             SELECT pc_id, source_json, revision FROM entry_metadata WHERE true \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET \
                 revision = max(entry_revisions.revision, excluded.revision); \
             INSERT INTO entry_revisions(pc_id, source_json, revision) \
             SELECT pc_id, source_json, revision FROM directory_metadata WHERE true \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET \
                 revision = max(entry_revisions.revision, excluded.revision); \
             PRAGMA user_version = 2; COMMIT;",
        )
        .map_err(sql_error)
}

pub(crate) fn sql_error(error: Error) -> StoreError {
    if matches!(
        &error,
        Error::SqliteFailure(code, _)
            if matches!(code.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    ) {
        StoreError::Busy
    } else {
        StoreError::Database(error)
    }
}

pub(crate) fn io_error(error: std::io::Error) -> StoreError {
    StoreError::Filesystem(error)
}

#[cfg(test)]
mod tests;
