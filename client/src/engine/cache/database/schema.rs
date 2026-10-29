use std::{fs::File, io::Read, path::Path};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};

use super::{Binding, DatabaseError};

const VERSION: i64 = 1;

pub fn inspect(path: &std::path::Path, exists: bool) -> Result<(), DatabaseError> {
    if !exists {
        return Ok(());
    }
    validate_header(path)?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > VERSION {
        return Err(DatabaseError::Unsupported(format!(
            "schema version {version} is newer than supported version {VERSION}"
        )));
    }
    if version == VERSION {
        let format = connection
            .query_row(
                "SELECT fingerprint_format FROM state_binding WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                if error.to_string().contains("no such table") {
                    DatabaseError::Unsupported("version 1 schema is incomplete".into())
                } else {
                    error.into()
                }
            })?;
        if let Some(format) = format
            && format != Binding::FINGERPRINT_FORMAT
        {
            return Err(DatabaseError::Unsupported(format!(
                "fingerprint format {format:?} is not supported"
            )));
        }
    }
    Ok(())
}

fn validate_header(path: &Path) -> Result<(), DatabaseError> {
    let mut file = File::open(path).map_err(|error| {
        DatabaseError::Recovery(format!("could not read database header: {error}"))
    })?;
    let length = file
        .metadata()
        .map_err(|error| DatabaseError::Recovery(format!("could not inspect database: {error}")))?
        .len();
    if length == 0 {
        return Ok(());
    }
    let mut header = [0; 16];
    if length < header.len() as u64 || file.read_exact(&mut header).is_err() {
        return Err(DatabaseError::Corrupt(
            "database header is truncated".into(),
        ));
    }
    if &header != b"SQLite format 3\0" {
        return Err(DatabaseError::Corrupt(
            "database header is not a SQLite database".into(),
        ));
    }
    Ok(())
}

pub fn configure(connection: &Connection) -> Result<(), DatabaseError> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         PRAGMA cache_size = -4096;",
    )?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("delete") {
        return Err(DatabaseError::Unsupported(format!(
            "SQLite selected journal mode {journal_mode:?}"
        )));
    }
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if foreign_keys != 1 {
        return Err(DatabaseError::Unsupported(
            "SQLite foreign key enforcement is disabled".into(),
        ));
    }
    Ok(())
}

pub fn ensure_v1(connection: &mut Connection) -> Result<(), DatabaseError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => {
            let tables: i64 = connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if tables != 0 {
                return Err(DatabaseError::Unsupported(
                    "unversioned SQLite database contains tables".into(),
                ));
            }
            create_v1(connection)?;
        }
        VERSION => {}
        other => {
            return Err(DatabaseError::Unsupported(format!(
                "schema version {other} is not supported"
            )));
        }
    }
    Ok(())
}

pub fn create_v1(connection: &mut Connection) -> Result<(), DatabaseError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE state_binding (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            server_url TEXT NOT NULL CHECK (length(server_url) BETWEEN 1 AND 2048),
            pc_id TEXT NOT NULL CHECK (length(pc_id) = 32),
            mirror_directory TEXT NOT NULL CHECK (length(mirror_directory) BETWEEN 1 AND 255),
            fingerprint_format TEXT NOT NULL CHECK (fingerprint_format = 'blake3-v1')
        );
        CREATE TABLE cached_selections (
            selection_id TEXT PRIMARY KEY CHECK (length(selection_id) BETWEEN 1 AND 128),
            definition_json TEXT NOT NULL CHECK (length(definition_json) BETWEEN 1 AND 262144)
        ) WITHOUT ROWID;
        CREATE TABLE cached_files (
            selection_id TEXT NOT NULL,
            source_json TEXT NOT NULL CHECK (length(source_json) BETWEEN 1 AND 262144),
            volume_identity TEXT NOT NULL CHECK (length(volume_identity) BETWEEN 1 AND 4096),
            volume_serial BLOB NOT NULL CHECK (typeof(volume_serial) = 'blob' AND length(volume_serial) = 8),
            file_id BLOB NOT NULL CHECK (typeof(file_id) = 'blob' AND length(file_id) = 16),
            file_size INTEGER NOT NULL CHECK (file_size >= 0),
            last_write_seconds INTEGER NOT NULL,
            last_write_nanos INTEGER NOT NULL CHECK (last_write_nanos BETWEEN 0 AND 999999999),
            change_time INTEGER NOT NULL CHECK (change_time != 0),
            digest BLOB NOT NULL CHECK (typeof(digest) = 'blob' AND length(digest) = 32),
            verified_seconds INTEGER NOT NULL CHECK (verified_seconds >= 0),
            verified_nanos INTEGER NOT NULL CHECK (verified_nanos BETWEEN 0 AND 999999999),
            PRIMARY KEY (selection_id, source_json),
            FOREIGN KEY (selection_id) REFERENCES cached_selections(selection_id) ON DELETE CASCADE
        ) WITHOUT ROWID;
        PRAGMA user_version = 1;",
    )?;
    transaction.commit()?;
    Ok(())
}
